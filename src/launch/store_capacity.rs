//! Operator-controlled structural storage growth. No inference, work resume,
//! allowance reset, history deletion or settlement of unknown provider usage.
use super::*;

pub fn validate(request: &s::CapacityExtensionRequest, operator: &str) -> Result<()> {
    identity(operator)?;
    require(
        request.operator == operator,
        "capacity extension operator mismatch",
    )?;
    s::Store::validate_capacity_request(request)?;
    Ok(())
}

fn authorized(app: &mut Runtime, operator: &str) -> Result<(s::StoredObject, s::StoredObject)> {
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(app)?;
    let (current, _) = app.load()?;
    require(
        current.exact_ref()? == runtime.exact_ref()?,
        "capacity authority changed during inspection",
    )?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    Ok((runtime, launch))
}

pub fn snapshot(app: &mut Runtime, operator: &str) -> Result<Value> {
    identity(operator)?;
    let (runtime, launch) = authorized(app, operator)?;
    let ledger = app.store.ledger()?;
    Ok(json!({
        "operator":operator,
        "expected_epoch":app.store.epoch()?,
        "expected_runtime":runtime.exact_ref()?,
        "expected_launch":launch.exact_ref()?,
        "expected_limits":ledger.limits,
        "ordinary":ledger.ordinary,"closeout":ledger.closeout,
        "frozen":ledger.frozen,
        "snapshot_atomic":false,"capacity_reserved":false,
        "supported_max_body_bytes":12*1024*1024,
        "supported_max_protected_closeout_retained_bytes":256*1024*1024,
        "scope":"Larger snapshot bounds and protected retained-space partition within unchanged total resources. No history, uncertainty or consumption is reset."
    }))
}

pub fn extend(
    app: &mut Runtime,
    request: &s::CapacityExtensionRequest,
    apply: bool,
) -> Result<Value> {
    validate(request, &request.operator)?;
    authorized(app, &request.operator)?;
    if let Some(receipt) = app.store.capacity_extension(&request.id)? {
        require(
            s::Store::capacity_request_matches(&receipt, request)?,
            "capacity identity is bound to a different request",
        )?;
        return Ok(
            json!({"receipt":receipt,"replayed":true,"applied":true,"inference_performed":false,"resumed":false,"budget_reset":false}),
        );
    }
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (runtime, launch) = authorized(app, &request.operator)?;
    // Another caller may have committed this identity before ownership was
    // acquired. Return its exact receipt instead of treating it as a stale fence.
    if let Some(receipt) = app.store.capacity_extension(&request.id)? {
        require(
            s::Store::capacity_request_matches(&receipt, request)?,
            "capacity identity is bound to a different request",
        )?;
        return Ok(
            json!({"receipt":receipt,"replayed":true,"applied":true,"inference_performed":false,"resumed":false,"budget_reset":false}),
        );
    }
    require(
        runtime.exact_ref()? == request.expected_runtime
            && launch.exact_ref()? == request.expected_launch,
        "capacity extension snapshot changed",
    )?;
    let receipt = app.store.extend_runtime_capacity(request, apply)?;
    Ok(json!({"receipt":receipt,"replayed":false,"applied":apply,
        "inference_performed":false,"resumed":false,"budget_reset":false,
        "total_resources_unchanged":true,
        "retained_history_unchanged":true,
        "note":"A preview reserves no capacity. An applied extension requires a binary supporting these retained migration receipts on subsequent opens."}))
}
