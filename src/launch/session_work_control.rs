//! Ordinary launcher controls. Stops apply atomically to current work without
//! acquiring its execution owner. Resume is a separate, snapshot-bound action.
//! Every saved key returns its historical outcome without repeating the action.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopAction {
    Pause,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkStopRequest {
    pub id: String,
    pub operator: String,
    pub work: String,
    pub action: StopAction,
    pub reason: String,
}

pub fn stop_work(root: &Path, operator: &str, request: &WorkStopRequest) -> BoundaryResult<Value> {
    for id in [&request.id, &request.operator, &request.work] {
        valid_id(id)?;
    }
    check(request.operator == operator, BoundaryError::Unauthorized)?;
    text(&request.reason, usize::MAX).map_err(|_| BoundaryError::InvalidRequest)?;
    let mut app = open(root, operator)?;
    let id = record_id("launch-work-stop-v1", &request.id);
    let control = s::RuntimeControl {
        operator: operator.into(),
        work: request.work.clone(),
        reason: request.reason.clone(),
        action: match request.action {
            StopAction::Pause => s::RuntimeControlAction::Pause,
            StopAction::Cancel => s::RuntimeControlAction::Canceled,
        },
    };
    let (recorded, receipt, replayed) = app
        .store
        .trusted_host()
        .commit_keyed_runtime_control(&id, control)
        .map_err(|error| match error {
            s::StoreError::Conflict(_) => BoundaryError::KeyConflict,
            other => other.into(),
        })?;
    Ok(
        json!({"id":request.id,"work":request.work,"request":request,
        "control_recorded":recorded.is_some(),"replayed":replayed,"runtime_request":recorded,"receipt":receipt,
        "inference_performed":false,"persona_resumed":false,"goal_completed":false,
        "scope":"The recorded local control is historical evidence. Inspect current state separately. Running calls and commands observe the control and retain their actual outcomes; remote termination is not established."}),
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResumeRequest {
    pub id: String,
    pub operator: String,
    pub work: String,
    pub expected_runtime: s::ObjectRef,
    pub expected_launch: s::ObjectRef,
}

fn resume_outcome(request: &WorkResumeRequest, recorded: bool, replayed: bool) -> Value {
    json!({"id":request.id,"work":request.work,"request":request,
        "control_recorded":recorded,"replayed":replayed,"snapshot_changed":!recorded,
        "inference_performed":false,"persona_resumed":false,"goal_completed":false,
        "scope":"Lifting the work pause preserves individual continuation choices and unresolved outcomes. Inspect current eligibility before starting a session."})
}

pub fn resume_work(
    root: &Path,
    operator: &str,
    request: &WorkResumeRequest,
) -> BoundaryResult<Value> {
    for id in [&request.id, &request.operator, &request.work] {
        valid_id(id)?;
    }
    check(request.operator == operator, BoundaryError::Unauthorized)?;
    check(
        valid_reference(&request.expected_runtime, STATE_ID)
            && valid_reference(&request.expected_launch, ID),
        BoundaryError::InvalidRequest,
    )?;
    let mut app = open(root, operator)?;
    // Reading an already committed outcome must work even during later activity.
    // A new resume additionally takes the owner and repeats these reads below.
    let (_, state, _, _) = authorized(&mut app, operator)?;
    if replayed_resume(&state, request)? {
        return Ok(resume_outcome(request, true, true));
    }
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or(BoundaryError::OwnerBusy)?;
    let (runtime, mut state, launch, _) = authorized(&mut app, operator)?;
    if replayed_resume(&state, request)? {
        return Ok(resume_outcome(request, true, true));
    }
    if runtime.exact_ref()? != request.expected_runtime
        || launch.exact_ref()? != request.expected_launch
    {
        return Ok(resume_outcome(request, false, false));
    }
    check(
        app.invocation_leases_quiescent(&state)?,
        BoundaryError::Blocked,
    )?;
    need_proposals::check_work(&mut app, &state, &request.work, unix_now()?)?;
    resume_work_state(&mut state, operator, &request.work)?;
    let event = state
        .operator_events
        .last_mut()
        .ok_or(BoundaryError::InvalidState)?;
    event["launcher_resume_request"] = serde_json::to_value(request)?;
    // The keyed outcome is part of the same retained state as the phase change.
    // Existing resume commit logic also renews the spent pause slot atomically.
    app.store.trusted_host().commit_runtime_work_change(
        &runtime.exact_ref()?,
        state_object(&runtime, &state)?,
        &request.work,
        &launch.exact_ref()?,
    )?;
    Ok(resume_outcome(request, true, false))
}

fn replayed_resume(state: &State, request: &WorkResumeRequest) -> BoundaryResult<bool> {
    let mut found = false;
    for event in &state.operator_events {
        let Some(value) = event.get("launcher_resume_request") else {
            continue;
        };
        let saved: WorkResumeRequest = serde_json::from_value(value.clone())?;
        if saved.id != request.id {
            continue;
        }
        check(
            !found
                && event["kind"] == "explicit_work_resume"
                && event["operator"] == saved.operator
                && event["work"] == saved.work,
            BoundaryError::InvalidState,
        )?;
        check(saved == *request, BoundaryError::KeyConflict)?;
        found = true;
    }
    Ok(found)
}
