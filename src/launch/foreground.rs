//! One coherent foreground consideration pass over the actual work subtree.
use super::*;

pub(super) struct Candidate {
    pub work: String,
    pub persona: String,
    pub requires_exclusive: bool,
}

pub(super) fn select(
    app: &mut Runtime,
    works: &[String],
    after: &mut usize,
    now: i64,
    owned: &BTreeSet<String>,
    visited: &BTreeSet<(String, String)>,
) -> Result<(usize, Option<Candidate>)> {
    let time = u64::try_from(now).map_err(|_| invalid("negative scheduling time"))?;
    require(!works.is_empty(), "foreground work subtree absent")?;
    // Load retained history once for this whole pass, not once for each work,
    // participant or exclusivity check. The cursor commit below binds this exact
    // head. Actual preparation still loads and validates current state itself.
    let (head, state) = app.load()?;
    require(app.invocation_leases_owned(&state, owned)?,
        "unfinished invocation requires completion or explicit recovery")?;
    let (old, mut cursor) = app.load_scheduler()?;
    let started_after = *after;
    let mut next_after = *after;
    let mut selected = None;
    cursor.last.clear();
    for _ in 0..works.len() {
        let work = &works[next_after % works.len()];
        next_after = (next_after % works.len() + 1) % works.len();
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
        let page = pairs()
            .filter(|pair| previous.as_ref().is_none_or(|last| pair > last))
            .chain(pairs().filter(|pair| previous.as_ref().is_some_and(|last| pair <= last)));
        for (wid, actor) in page {
            if visited.contains(&(wid.0.clone(), actor.0.clone())) {
                continue;
            }
            let opportunity = runtime_opportunity(&state, &wid, &actor, time);
            let mut entry = scheduler_entry(&state, wid.clone(), actor.clone(), opportunity, now)?;
            if continuation_permit(&state, &actor.0, work) && state.work_meta.contains_key(work) {
                entry.disposition = SchedulerDisposition::Queued;
            }
            if matches!(entry.disposition, SchedulerDisposition::Queued) {
                selected = Some(Candidate {
                    work: work.clone(), persona: actor.0.clone(),
                    requires_exclusive: current.need.optional_activity.is_some()
                        || current.acceptance(&actor).is_none()
                        || state.personas[&actor.0].focus[work].repair_trigger.is_some(),
                });
            }
            cursor.foreground_after.insert(wid.0.clone(), actor.0.clone());
            cursor.after = Some((wid, actor));
            cursor.considered = increment(cursor.considered)?;
            cursor.last.push(entry);
            if selected.is_some() { break; }
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
            requires_exclusive: selected.as_ref().is_some_and(|value| value.requires_exclusive),
        });
        let mut change = scheduler_change(old.as_ref(), &cursor)?;
        change.depends_on.push(runtime);
        app.store.compare_and_swap_batch(vec![change])?;
    }
    *after = next_after;
    Ok((considered, selected))
}
