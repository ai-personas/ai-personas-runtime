//! Explicit operator recovery of interrupted work, without inference or replay.
use super::*;

fn authorize_work(state: &State, saved: &Saved, operator: &str, work: &str) -> BoundaryResult<()> {
    check(saved.works.contains_key(work)
        && state.work_meta.get(work).is_some_and(|meta| meta.intake_operator == operator),
        BoundaryError::Unauthorized)
}

pub fn recover_work(root: &Path, operator: &str, work: &str) -> BoundaryResult<Value> {
    valid_id(work)?;
    let mut app = open(root, operator)?;
    let (_, state, _, saved) = authorized(&mut app, operator)?;
    authorize_work(&state, &saved, operator, work)?;
    drop(state);
    let owner = app.store.try_execution_owner_with_recovery()?.ok_or(BoundaryError::OwnerBusy)?;
    let (_, state, _, saved) = authorized(&mut app, operator)?;
    authorize_work(&state, &saved, operator, work)?;
    let calls = rejected_continuation::active_invocations_for_work(&state, work)?;
    drop(state);
    let now = unix_now()?;
    let mut released = Vec::new();
    let mut held = Vec::new();
    for id in calls {
        let invocation = app.store.trusted_host().invocation(&id)?;
        if decode_prepared(&invocation)?.request.work != work { continue; }
        let (runtime, state, launch, saved) = authorized(&mut app, operator)?;
        authorize_work(&state, &saved, operator, work)?;
        drop(state);
        if app.retire_orphaned_continuation(&id, now, &owner,
            Some((runtime.exact_ref()?, launch.exact_ref()?)))? {
            released.push(id);
        } else {
            held.push(id);
        }
    }
    let (_, state, _, saved) = authorized(&mut app, operator)?;
    authorize_work(&state, &saved, operator, work)?;
    let published = app.publish_retained_host_command_results(&state, &[work.into()], now)?;
    Ok(json!({"work":work,"released_local_calls":released,"still_held_calls":held,
        "published_command_results":published,"inference_performed":false,
        "replay_authorized":false,"continuations_changed":false,"work_resumed":false,
        "remote_outcomes_reconciled":false,
        "scope":"Owner recovery records abandoned dispatches as uncertain. Only proven response-only local leases are released; original attempts, missing terminals and unknown usage remain retained. Existing command results may be published, but no command is dispatched. Current stops and pauses remain effective."}))
}
