//! One coherent foreground consideration pass over the actual work subtree.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Exclusivity {
    None,
    Work,
    Runtime,
}

#[derive(Clone)]
pub(super) struct Candidate {
    pub work: String,
    pub persona: String,
    pub exclusivity: Exclusivity,
}

#[derive(Default)]
pub(super) struct Selection {
    pub considered: usize,
    pub candidate: Option<Candidate>,
    pub next_scheduled_at: Option<u64>,
}

pub(super) fn select(
    app: &mut Runtime,
    works: &[String],
    after: &mut usize,
    now: i64,
    owned: &BTreeSet<String>,
    visited: &BTreeSet<(String, String)>,
    priority: Option<&(String, String)>,
    blocked_works: &BTreeSet<String>,
) -> Result<Selection> {
    let time = u64::try_from(now).map_err(|_| invalid("negative scheduling time"))?;
    require(!works.is_empty(), "foreground work subtree absent")?;
    // Load retained history once for this whole pass, not once for each work,
    // participant or exclusivity check. The cursor commit below binds this exact
    // head. Actual preparation still loads and validates current state itself.
    let (head, state) = app.load()?;
    require(app.invocation_leases_owned(&state, owned)?,
        "unfinished invocation requires completion or explicit recovery")?;
    if !owned.is_empty() {
        // Ordinary peers may continue while another owned call is pending, but
        // refilling must not starve transactions that require a settled owner.
        // Reuse this selection snapshot; the eventual helpers still validate
        // their exact current records at the quiescent maintenance boundary.
        if feedback::has_pending(app)? || question_commands::has_pending(app)? {
            return Ok(Selection::default());
        }
        for work in works {
            if persona_birth::has_pending_for_work(&state, work)
                || intake_delegation::has_pending_for_work(&state, work)
            {
                return Ok(Selection::default());
            }
        }
        // Ended failures are retired by their owning coordinator when the
        // exact response-only evidence permits it. An unresolved failed or
        // uncertain lease holds its actor, not unrelated ready participants.
    }
    let (old, mut cursor) = app.load_scheduler()?;
    let started_after = *after;
    let mut next_after = *after;
    let mut selected = None;
    let mut next_scheduled_at = None;
    cursor.last.clear();
    // A selected exclusive candidate may have waited for the affected calls to
    // settle after its cursor was committed. Reconsider that exact work/persona
    // first, using this fresh snapshot and the ordinary eligibility checks.
    // Resolve by identity because structural maintenance can change the subtree.
    let preferred = priority.and_then(|(work, actor)|
        works.iter().position(|id| id == work).map(|index| (index, Some(actor.as_str()))));
    let circular_works = (0..works.len())
        .map(|offset| ((started_after % works.len() + offset) % works.len(), None));
    for (index, preferred_actor) in preferred.into_iter().chain(circular_works) {
        let work = &works[index];
        // A first acceptance changes this work's authority, not another work's
        // sealed decision scope. Keep its current calls draining without
        // preventing independent work from receiving a turn.
        if blocked_works.contains(work) {
            continue;
        }
        // Only the exact deferred actor gets the extra first consideration.
        // If it is no longer eligible, retain both circular cursors and visit
        // the ordinary work/persona order, skipping that already checked pair.
        if preferred_actor.is_none() {
            next_after = (index + 1) % works.len();
        }
        let wid = w::WorkId(work.clone());
        let current = state.board.work(&wid)?;
        if !matches!(current.phase, w::Phase::Open) {
            continue;
        }
        let previous = cursor.foreground_after.get(work)
            .map(|actor| (wid.clone(), w::PersonaId(actor.clone())))
            .or_else(|| cursor.after.clone().filter(|(id, _)| id == &wid));
        let pairs = || current.participants.keys().map(|actor| (wid.clone(), actor.clone()));
        // Preserve the complete circular participant order and per-work cursor.
        // A page requested by an inspection never limits foreground eligibility.
        let priority_persona = priority.filter(|(id, _)| id == work)
            .map(|(_, actor)| actor.as_str());
        let first = preferred_actor.and_then(|preferred|
            pairs().find(|(_, actor)| actor.0 == preferred));
        let circular_personas = pairs()
            .filter(|pair| previous.as_ref().is_none_or(|last| pair > last))
            .chain(pairs().filter(|pair| previous.as_ref().is_some_and(|last| pair <= last)))
            .filter(|_| preferred_actor.is_none())
            .filter(|(_, actor)| Some(actor.0.as_str()) != priority_persona);
        let page = first.into_iter().chain(circular_personas);
        for (wid, actor) in page {
            if visited.contains(&(wid.0.clone(), actor.0.clone())) {
                continue;
            }
            let opportunity = runtime_opportunity(&state, &wid, &actor, time);
            let mut entry = scheduler_entry(&state, wid.clone(), actor.clone(), opportunity, now)?;
            if continuation_permit(&state, &actor.0, work, now) && state.work_meta.contains_key(work) {
                entry.disposition = SchedulerDisposition::Queued;
            }
            if matches!(entry.disposition, SchedulerDisposition::Queued) {
                // A fresh cohort's character calls can only adopt their own
                // profile/file units. They cannot accept responsibility or
                // change shared work. Existing accepted participants can make
                // those changes, so a mixed work retains its serial boundary.
                let initial_character = state.personas[&actor.0].character.is_none()
                    && current.acceptances.is_empty();
                selected = Some(Candidate {
                    work: work.clone(), persona: actor.0.clone(),
                    exclusivity: if state.personas[&actor.0].focus[work].repair_trigger.is_some() {
                        Exclusivity::Runtime
                    } else if !initial_character && current.acceptance(&actor).is_none() {
                        Exclusivity::Work
                    } else {
                        Exclusivity::None
                    },
                });
            } else if let Some(at) = scheduled_at(&state, &wid, &actor, now)? {
                // Use this same eligible-subtree snapshot for the next wake.
                // Due candidates take the ordinary admission path above.
                if at > time {
                    next_scheduled_at = Some(next_scheduled_at
                        .map_or(at, |earlier: u64| earlier.min(at)));
                }
            }
            if preferred_actor.is_none() || selected.is_some() {
                cursor.foreground_after.insert(wid.0.clone(), actor.0.clone());
                cursor.after = Some((wid, actor));
            }
            cursor.considered = increment(cursor.considered)?;
            cursor.last.push(entry);
            if selected.is_some() {
                next_after = (index + 1) % works.len();
                break;
            }
        }
        if selected.is_some() { break; }
    }
    let considered = cursor.last.len();
    if considered > 0 {
        let runtime = head.exact_ref()?;
        cursor.last_foreground_scan = Some(ForegroundScan {
            runtime: runtime.clone(), works: works.into(), started_after, next_after,
            visited: visited.clone(), owned_invocations: owned.clone(), at_unix: now,
            selected: selected.as_ref().map(|value| (value.work.clone(), value.persona.clone())),
            requires_exclusive: selected.as_ref().is_some_and(|value|
                value.exclusivity != Exclusivity::None),
            exclusive_scope: selected.as_ref().and_then(|value| match value.exclusivity {
                Exclusivity::None => None,
                Exclusivity::Work => Some("work".into()),
                Exclusivity::Runtime => Some("runtime".into()),
            }),
            deferred_exclusive_candidate: priority.cloned(),
            blocked_works: blocked_works.clone(),
        });
        let mut change = scheduler_change(old.as_ref(), &cursor)?;
        change.depends_on.push(runtime);
        app.store.compare_and_swap_batch(vec![change])?;
    }
    *after = next_after;
    Ok(Selection { considered, candidate: selected, next_scheduled_at })
}

/// Prospective scheduling only. The exact accepted continuation must still
/// offer a route at its authored time; ordinary admission rechecks current state.
pub(super) fn scheduled_at(state: &State, work: &w::WorkId, actor: &w::PersonaId, now: i64) -> Result<Option<u64>> {
    let Some(focus) = state.personas.get(&actor.0).and_then(|persona| persona.focus.get(&work.0)) else {
        return Ok(None);
    };
    if !focus.ordinary_continuation { return Ok(None); }
    let current = state.board.work(work)?;
    let Some(participant) = current.participants.get(actor) else { return Ok(None); };
    let at = match (&focus.continuation, &participant.continuation) {
        (Some(p::Continuation::ReconsiderAt { not_before_unix, .. }),
         Some(w::Continuation::ReconsiderAt { not_before, .. })) if not_before_unix == not_before => *not_before,
        (Some(p::Continuation::Wait { deadline_unix, .. }), Some(next))
            if next.wait_deadline() == Some(*deadline_unix) => *deadline_unix,
        _ => return Ok(None),
    };
    // The runtime clock and eligibility interface use signed Unix seconds.
    let Ok(at_unix) = i64::try_from(at) else { return Ok(None); };
    let consider_at = now.max(at_unix);
    let opportunity = runtime_opportunity(state, work, actor, consider_at as u64);
    let entry = scheduler_entry(state, work.clone(), actor.clone(), opportunity, consider_at)?;
    Ok(matches!(entry.disposition, SchedulerDisposition::Queued).then_some(at))
}

/// Observe an authored return while unrelated transports remain owned. This
/// creates no invocation and imposes no transport timeout. Recheck wall time so
/// clock changes cannot silently move the retained return to a monotonic date.
pub(super) async fn wait_for_scheduled_time(next: Option<u64>) -> Result<()> {
    let Some(at) = next else { return std::future::pending().await; };
    loop {
        if crate::process_control::signal().is_some() { return Ok(()); }
        let now = u64::try_from(unix_now()?).map_err(|_| invalid("negative scheduling time"))?;
        if now >= at { return Ok(()); }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Park the existing owner until an authored deadline, a command result or new
/// input/control state. Clock checks create no call, continuation or recurrence.
/// The next scheduling pass uses the actual clock, including after clock changes.
pub(super) async fn wait_for_activity(app: &mut Runtime, works: &[String], owner: Option<&s::ExecutionOwner>) -> Result<bool> {
    if crate::process_control::signal().is_some() { return Ok(true); }
    let stamp = app.store.foreground_activity_stamp()?;
    // Capture before delivery so an input arriving at this boundary cannot be
    // mistaken for an unchanged inbox and left asleep until the deadline.
    if let Some(owner) = owner { deliver_inputs(app, owner, unix_now()?)?; }
    if app.store.foreground_activity_stamp()? != stamp { return Ok(true); }
    let state = app.snapshot()?;
    let now = unix_now()?;
    let mut next = None;
    for work in works {
        let wid = w::WorkId(work.clone());
        for actor in state.board.work(&wid)?.participants.keys() {
            if let Some(at) = scheduled_at(&state, &wid, actor, now)? {
                next = Some(next.map_or(at, |earlier: u64| earlier.min(at)));
            }
        }
    }
    drop(state);
    let Some(at) = next else {
        if app.store.foreground_activity_stamp()? != stamp { return Ok(true); }
        if app.has_background_host_commands() {
            app.wait_for_host_command().await?;
            return Ok(true);
        }
        return Ok(false);
    };
    loop {
        if crate::process_control::signal().is_some()
            || app.publish_completed_host_commands(unix_now()?)? > 0
            || u64::try_from(unix_now()?).map_err(|_| invalid("negative scheduling time"))? >= at
            || app.store.foreground_activity_stamp()? != stamp
        {
            return Ok(true);
        }
        // A short host observation interval also avoids overflowing a monotonic
        // timer for a distant authored date. It never shortens that date.
        app.release_decoded_state();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}
