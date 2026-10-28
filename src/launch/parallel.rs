//! One foreground owner, independent transport futures, sequential adoption.
use super::*;
use futures_util::{stream::FuturesUnordered, StreamExt};
use foreground::Exclusivity;
use std::collections::VecDeque;

fn failed_call_allows_progress(app: &mut Runtime, id: &str, owned: &BTreeSet<String>) -> Result<bool> {
    let invocation = match app.invocation(id) {
        Ok(invocation) => invocation,
        Err(RuntimeError::Store(s::StoreError::Missing(_))) => return Ok(false),
        Err(error) => return Err(error),
    };
    if !matches!(invocation.state, s::InvocationState::Failed | s::InvocationState::Uncertain) {
        return Ok(false);
    }
    if invocation.state == s::InvocationState::Failed {
        app.retire_rejected_continuation_owned(id, unix_now()?, owned)?;
    }
    let state = app.snapshot()?;
    app.invocation_leases_owned(&state, owned)
}

enum Wake {
    Call(provider_calls::Completion),
    Jobs(Result<()>),
    Clock(Result<()>),
}

fn maintenance_state(app: &mut Runtime) -> Result<State> {
    phase_timing::measure("launch.maintenance_state", || {
        let (head, state) = app.load()?;
        require(head.id == STATE_ID && head.kind == "runtime-state" && state.format == 1,
            "invalid runtime state")?;
        Ok(state)
    })
}

pub(super) async fn run<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    owner: &s::ExecutionOwner,
) -> Result<Output> {
    run_with_subtree(app, work, driver, Some(owner)).await
}

pub(super) async fn run_delegated<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    owner: &s::ExecutionOwner,
) -> Result<Output> {
    run_with_subtree(app, work, driver, Some(owner)).await
}

async fn run_with_subtree<D: Driver>(
    app: &mut Runtime,
    root: &str,
    driver: &mut D,
    owner: Option<&s::ExecutionOwner>,
) -> Result<Output> {
    let (_, saved) = load(app)?;
    saved.execution_provider()?;
    require(saved.works.contains_key(root), "unknown launch work")?;
    let mut stop = None;
    let mut pending = FuturesUnordered::new();
    let mut owned = BTreeSet::new();
    let mut owned_work = BTreeMap::new();
    let mut exclusive = BTreeMap::new();
    let mut deferred: Vec<(String, String, AdoptionStatus)> = Vec::new();
    let mut pending_handovers = BTreeMap::new();
    let mut handover_barrier = BTreeSet::new();
    let mut works = vec![root.to_owned()];
    let mut after = 0;
    let mut visited = BTreeSet::new();
    let mut round_started = false;
    let mut pending_exclusive: VecDeque<foreground::Candidate> = VecDeque::new();

    loop {
        if crate::process_control::signal().is_some() {
            stop.get_or_insert("interrupted");
        }
        // Character handover and any recovery that could not retire only its
        // failed lease retain their quiescent fences. Use the actual completed
        // IDs, never Saved.works' newest call.
        if pending.is_empty() {
            handover_barrier.clear();
            for (id, work, status) in std::mem::take(&mut deferred) {
                let follow_up = (|| {
                    driver.before_step(app)?;
                    if status == AdoptionStatus::CharacterOnly {
                        if !handover_invocation(app, &work, &id)? {
                            return Ok(Some("character_fence"));
                        }
                    } else if !app.retire_rejected_continuation(&id, unix_now()?)?
                        && !quiescent_call_allows_peer_progress(app, &id)?
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
            pending_handovers.clear();
        }

        // All fallible work after admission is caught here or below. A local
        // error stops new admissions but still drains every actual transport.
        if stop.is_none() {
            let maintenance = (|| {
                app.publish_completed_host_commands(unix_now()?)?;
                if !round_started {
                    // Structural maintenance remains atomic and quiescent.
                    // Between these boundaries, ready peers may receive another
                    // turn without waiting for unrelated transports to finish.
                    require(pending.is_empty(), "subtree changed while transports were owned")?;
                    driver.before_step(app)?;
                    if let Some(owner) = owner {
                        deliver_inputs(app, owner, unix_now()?)?;
                        let mut state = maintenance_state(app)?;
                        works = delegated::subtree(&state, root)?;
                        for work in &works {
                            let calls = rejected_continuation::active_invocations_for_work(&state, work)?;
                            // Empty maintenance checks share this round's current
                            // snapshot. Release it before helpers that can mutate
                            // state, then reload even if they make no change.
                            if !calls.is_empty() {
                                drop(state);
                                phase_timing::measure("launch.maintenance_rejections", ||
                                    app.retire_ready_rejected_continuations(work, calls, unix_now()?))?;
                                state = maintenance_state(app)?;
                            }
                            if persona_birth::has_pending_for_work(&state, work) {
                                drop(state);
                                phase_timing::measure("launch.maintenance_creation", ||
                                    persona_birth::create_pending_for_work_owned(app, work, driver, owner))?;
                                state = maintenance_state(app)?;
                            }
                            if intake_delegation::has_pending_for_work(&state, work) {
                                drop(state);
                                phase_timing::measure("launch.maintenance_admission", ||
                                    intake_delegation::admit_pending_for_work_owned(app, work, owner))?;
                                state = maintenance_state(app)?;
                            }
                        }
                        works = delegated::subtree(&state, root)?;
                    }
                    for work in &works {
                        if !handover(app, work)? {
                            return Ok(Some("character_fence"));
                        }
                    }
                    round_started = true;
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

        let mut next_scheduled_at = None;
        while stop.is_none()
            // A completed character unit waits for a settled handover, but
            // does not hold independent initial-character attempts or ordinary
            // peers. Selection still excludes held actors and drains before
            // any transition that changes shared responsibility.
            && (deferred.is_empty() || (!handover_barrier.is_empty()
                && deferred.iter().all(|(_, _, status)| *status == AdoptionStatus::CharacterOnly)))
            && !exclusive.values().any(|scope| *scope == Exclusivity::Runtime)
            && (!pending_exclusive.iter().any(|candidate|
                candidate.exclusivity == Exclusivity::Runtime) || pending.is_empty())
        {
            if crate::process_control::signal().is_some() {
                stop.get_or_insert("interrupted");
                break;
            }
            let mut attempted_id = None;
            let mut exclusivity = Exclusivity::None;
            let mut deferred_candidate = false;
            let admission = (|| {
                driver.before_step(app)?;
                let ready = pending_exclusive.iter().position(|candidate| match candidate.exclusivity {
                    Exclusivity::Work => !owned_work.values().any(|work| work == &candidate.work),
                    Exclusivity::Runtime => pending.is_empty(),
                    Exclusivity::None => true,
                });
                let priority = ready.map(|index| {
                    let candidate = &pending_exclusive[index];
                    (candidate.work.clone(), candidate.persona.clone())
                });
                let mut blocked_works: BTreeSet<String> = exclusive.iter()
                    .filter(|(_, scope)| **scope == Exclusivity::Work)
                    .filter_map(|(id, _)| owned_work.get(id).cloned())
                    .collect();
                blocked_works.extend(pending_exclusive.iter().enumerate()
                    .filter(|(index, _)| Some(*index) != ready)
                    .map(|(_, candidate)| candidate.work.clone()));
                let selection = foreground::select(
                    app, &works, &mut after, unix_now()?, &owned, &visited,
                    priority.as_ref(), &blocked_works, &pending_handovers,
                )?;
                // A pending structural transaction may prevent any consideration.
                // Retain the preference until an actual fresh scan considers it.
                if selection.considered > 0 {
                    if let Some(index) = ready { pending_exclusive.remove(index); }
                }
                let Some(candidate) = selection.candidate else {
                    next_scheduled_at = selection.next_scheduled_at;
                    return Ok(None);
                };
                // First responsibility acceptance changes only its work's
                // authority. Other work retains its exact sealed scope checks.
                // Initial character calls in a fresh cohort change only their
                // own character units; their handovers wait for settled calls.
                exclusivity = candidate.exclusivity;
                let must_drain = match exclusivity {
                    Exclusivity::None => false,
                    Exclusivity::Work => owned_work.values().any(|work| work == &candidate.work),
                    Exclusivity::Runtime => !pending.is_empty(),
                };
                if must_drain {
                    // Selection has already advanced the saved cursor. Retain
                    // one exact candidate per blocked work and reconsider it
                    // once its own calls settle, without refilling that work.
                    pending_exclusive.push_back(candidate);
                    deferred_candidate = true;
                    return Ok(None);
                }
                let foreground::Candidate { work, persona, .. } = candidate;
                visited.insert((work.clone(), persona.clone()));
                driver.before_step(app)?;
                let (request, current) =
                    reserve_owned(app, &work, &persona, driver.prospective_intake(), &owned)?;
                attempted_id = Some(request.id.clone());
                driver.start(app, request, &current, unix_now()?, &owned).map(Some)
            })();
            match admission {
                Ok(Some(call)) => {
                    if call.request.mode == Mode::SelectionRepair {
                        exclusivity = Exclusivity::Runtime;
                    }
                    if exclusivity != Exclusivity::None {
                        exclusive.insert(call.request.id.clone(), exclusivity);
                    }
                    owned_work.insert(call.request.id.clone(), call.request.work.clone());
                    owned.insert(call.request.id);
                    // Provider I/O and control observation must keep progressing
                    // while this coordinator prepares or adopts another decision.
                    pending.push(transport_tasks::spawn(call.completion));
                }
                Ok(None) if deferred_candidate => continue,
                Ok(None) => break,
                Err(error) => {
                    let peer_progress = attempted_id
                        .as_deref()
                        .map(|id| failed_call_allows_progress(app, id, &owned))
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
        if !pending.is_empty() {
            let wait_for_jobs = stop.is_none() && app.has_background_host_commands();
            app.release_decoded_state();
            let wake = tokio::select! {
                call = pending.next() => Wake::Call(call.expect("nonempty pending calls")),
                jobs = app.wait_for_host_command(), if wait_for_jobs => Wake::Jobs(jobs),
                clock = foreground::wait_for_scheduled_time(next_scheduled_at),
                    if stop.is_none() => Wake::Clock(clock),
            };
            if crate::process_control::signal().is_some() {
                stop.get_or_insert("interrupted");
            }
            match wake {
                Wake::Jobs(Err(error)) | Wake::Clock(Err(error)) => {
                    stop.get_or_insert(stop_for_error(&error));
                }
                Wake::Jobs(Ok(())) | Wake::Clock(Ok(())) => (),
                Wake::Call(completion) => {
                    let id = completion.prepared.request.id.clone();
                    let work = completion.prepared.request.work.clone();
                    let persona = completion.prepared.request.persona.clone();
                    owned.remove(&id);
                    owned_work.remove(&id);
                    exclusive.remove(&id);
                    handover_barrier.remove(&id);
                    match driver.finish(app, completion) {
                        Ok(report) if report.status == AdoptionStatus::CharacterOnly => {
                            // The accepted unit has released its call lease, but
                            // its handover still binds this actor and work.
                            pending_handovers.insert(persona, work.clone());
                            if deferred.is_empty() {
                                // Refill only while calls already pending at
                                // this boundary remain. Once they finish, drain
                                // any successors so handover cannot starve.
                                handover_barrier = owned.clone();
                            }
                            deferred.push((id, work, report.status));
                        }
                        Ok(report) if report.status == AdoptionStatus::Held => {
                            match unix_now().and_then(|now|
                                app.retire_rejected_continuation_owned(&id, now, &owned)) {
                                Ok(true) => (),
                                Ok(false) => deferred.push((id, work, report.status)),
                                Err(error) => { stop.get_or_insert(stop_for_error(&error)); }
                            }
                        }
                        Ok(_) => (),
                        Err(error) => match failed_call_allows_progress(app, &id, &owned) {
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
            // A result or an authored return time can make a previously visited
            // persona ready again. Active invocations remain excluded by their
            // exact lease state, and the saved circular cursors preserve order.
            // Deferred candidates retain their exact preference and block only
            // the affected work. Other ready peers can be considered afresh.
            visited.clear();
            if pending.is_empty() {
                // Always perform structural maintenance before refilling a
                // settled owner, even when a peer just requested another turn.
                round_started = false;
            }
            continue;
        }
        if !deferred.is_empty() {
            continue;
        }
        if stop.is_some() {
            break;
        }
        if !visited.is_empty() {
            visited.clear();
            round_started = false;
            continue;
        }
        match foreground::wait_for_activity(app, &works, owner).await {
            Ok(true) => {
                round_started = false;
                continue;
            }
            Ok(false) => (),
            Err(error) => {
                stop = Some(stop_for_error(&error));
                break;
            }
        }
        stop = Some(match app.snapshot() {
            Ok(state) if active_invocations(&state).is_empty() => "no_eligible_route",
            Ok(_) => "held",
            Err(error) => stop_for_error(&error),
        });
        break;
    }
    let mut view = result(app, root)?;
    view.stop = stop.map(str::to_owned);
    Ok(view)
}
