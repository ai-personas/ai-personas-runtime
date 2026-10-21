//! One foreground owner, independent transport futures, sequential adoption.
use super::*;
use futures_util::{stream::FuturesUnordered, StreamExt};

fn failed_allows_progress(app: &mut Runtime, id: &str, owned: &BTreeSet<String>) -> Result<bool> {
    let invocation = match app.invocation(id) {
        Ok(invocation) => invocation,
        Err(RuntimeError::Store(s::StoreError::Missing(_))) => return Ok(false),
        Err(error) => return Err(error),
    };
    if invocation.state != s::InvocationState::Failed {
        return Ok(false);
    }
    let state = app.snapshot()?;
    app.invocation_leases_owned(&state, owned)
}

enum Wake {
    Call(provider_calls::Completion),
    Jobs(Result<()>),
}

pub(super) async fn run<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
) -> Result<Output> {
    let (_, saved) = load(app)?;
    saved.execution_provider()?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    let mut remaining = saved.setup.max_considerations;
    let mut steps = 0;
    let mut stop = None;
    let mut pending = FuturesUnordered::new();
    let mut owned = BTreeSet::new();
    let mut exclusive = BTreeSet::new();
    let mut deferred: Vec<(String, AdoptionStatus)> = Vec::new();
    let mut initial_handover = true;

    loop {
        // Initial handover and completed-rejection retirement retain their
        // terminal-lease transaction fences. Drain peers before applying them,
        // using the actual completed IDs, never Saved.works' newest call.
        if pending.is_empty() {
            for (id, status) in std::mem::take(&mut deferred) {
                let follow_up = (|| {
                    driver.before_step(app)?;
                    if status == AdoptionStatus::CharacterOnly {
                        if !handover_invocation(app, work, &id)? {
                            return Ok(Some("character_fence"));
                        }
                    } else if !app.retire_rejected_continuation(&id, unix_now()?)?
                        && !failed_call_allows_peer_progress(app, &id)?
                    {
                        return Ok(Some("held"));
                    }
                    Ok::<_, RuntimeError>(None)
                })();
                match follow_up {
                    Ok(Some(reason)) => {
                        stop.get_or_insert(reason);
                    }
                    Err(error) => {
                        stop.get_or_insert(stop_for_error(&error));
                    }
                    Ok(None) => (),
                }
            }
        }

        // All fallible work after admission is caught here or below. A local
        // error stops new admissions but still drains every actual transport.
        if stop.is_none() {
            let maintenance = (|| {
                app.publish_completed_host_commands(unix_now()?)?;
                if initial_handover {
                    driver.before_step(app)?;
                    initial_handover = false;
                    if !handover(app, work)? {
                        return Ok(Some("character_fence"));
                    }
                }
                let state = app.snapshot()?;
                if !matches!(
                    state.board.work(&w::WorkId(work.into()))?.phase,
                    w::Phase::Open
                ) {
                    return Ok(Some("no_eligible_route"));
                }
                Ok::<_, RuntimeError>(None)
            })();
            match maintenance {
                Ok(Some(reason)) => {
                    stop = Some(reason);
                }
                Err(error) => {
                    stop = Some(stop_for_error(&error));
                }
                Ok(None) => (),
            }
        }

        while stop.is_none()
            && deferred.is_empty()
            && exclusive.is_empty()
            && steps < saved.setup.max_steps
            && remaining > 0
        {
            let mut attempted_id = None;
            let mut needs_exclusive = false;
            let admission = (|| {
                driver.before_step(app)?;
                let (count, selected) = select_owned(app, work, remaining, unix_now()?, &owned)?;
                remaining -= count;
                let Some(persona) = selected else {
                    return Ok(None);
                };
                // First responsibility acceptance changes shared work authority.
                // Give unaccepted participants a current serial opportunity;
                // already accepted independent peers may overlap afterward.
                let state = app.snapshot()?;
                needs_exclusive = state
                    .board
                    .work(&w::WorkId(work.into()))?
                    .acceptance(&w::PersonaId(persona.clone()))
                    .is_none()
                    || state.personas[&persona].focus[work]
                        .repair_trigger
                        .is_some();
                if needs_exclusive && !pending.is_empty() {
                    return Ok(None);
                }
                driver.before_step(app)?;
                steps += 1;
                let (request, current) =
                    reserve_owned(app, work, &persona, driver.prospective_intake(), &owned)?;
                attempted_id = Some(request.id.clone());
                driver.start(app, request, &current, unix_now()?).map(Some)
            })();
            match admission {
                Ok(Some(call)) => {
                    if needs_exclusive || call.request.mode == Mode::SelectionRepair {
                        exclusive.insert(call.request.id.clone());
                    }
                    owned.insert(call.request.id);
                    pending.push(call.completion);
                }
                Ok(None) => break,
                Err(error) => {
                    let peer_progress = attempted_id
                        .as_deref()
                        .map(|id| failed_allows_progress(app, id, &owned))
                        .transpose();
                    match peer_progress {
                        Ok(Some(true)) => (),
                        Err(binding_error) => {
                            stop = Some(stop_for_error(&binding_error));
                        }
                        _ => {
                            stop = Some(stop_for_error(&error));
                        }
                    }
                }
            }
        }
        if stop.is_none() {
            if steps >= saved.setup.max_steps {
                stop = Some("step_limit");
            } else if remaining == 0 {
                stop = Some("consideration_bound");
            }
        }

        if !pending.is_empty() {
            let wake = if stop.is_none() && app.has_background_host_commands() {
                tokio::select! {
                    call = pending.next() => Wake::Call(call.expect("nonempty pending calls")),
                    jobs = app.wait_for_host_command() => Wake::Jobs(jobs),
                }
            } else {
                Wake::Call(pending.next().await.expect("nonempty pending calls"))
            };
            match wake {
                Wake::Jobs(Err(error)) => {
                    stop.get_or_insert(stop_for_error(&error));
                }
                Wake::Jobs(Ok(())) => (),
                Wake::Call(completion) => {
                    let id = completion.prepared.request.id.clone();
                    owned.remove(&id);
                    exclusive.remove(&id);
                    match driver.finish(app, completion) {
                        Ok(report)
                            if matches!(
                                report.status,
                                AdoptionStatus::Held | AdoptionStatus::CharacterOnly
                            ) =>
                        {
                            deferred.push((id, report.status));
                        }
                        Ok(_) => (),
                        Err(error) => match failed_allows_progress(app, &id, &owned) {
                            Ok(true) => (),
                            Ok(false) => {
                                stop.get_or_insert(stop_for_error(&error));
                            }
                            Err(binding_error) => {
                                stop.get_or_insert(stop_for_error(&binding_error));
                            }
                        },
                    }
                }
            }
            continue;
        }
        if !deferred.is_empty() {
            continue;
        }
        if stop.is_some() {
            break;
        }
        if app.has_background_host_commands() {
            if let Err(error) = app.wait_for_host_command().await {
                stop = Some(stop_for_error(&error));
            }
            continue;
        }
        stop = Some(match app.snapshot() {
            Ok(state) if active_invocations(&state).is_empty() => "no_eligible_route",
            Ok(_) => "held",
            Err(error) => stop_for_error(&error),
        });
        break;
    }
    let mut view = result(app, work)?;
    view.stop = stop.map(str::to_owned);
    Ok(view)
}
