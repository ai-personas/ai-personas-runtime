//! Operator-controlled storage growth and finite storage-read funding.
//! No inference, work resume, allowance reset, history deletion or settlement
//! of unknown provider usage.
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

fn authorized(store: &mut s::Store, operator: &str) -> Result<(s::StoredObject, s::StoredObject)> {
    let (runtime, launch) = store.capacity_authority()?;
    let state: State = serde_json::from_value(runtime.body.clone())?;
    let saved: Saved = serde_json::from_value(launch.body.clone())?;
    require(
        runtime.id == STATE_ID
            && state.format == 1
            && launch.id == ID
            && launch.kind == "foreground-launch",
        "invalid capacity authority heads",
    )?;
    need_proposals::validate_state(&state)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    Ok((runtime, launch))
}

pub fn snapshot(app: &mut Runtime, operator: &str) -> Result<Value> {
    snapshot_store(&mut app.store, operator)
}

fn open(root: &Path) -> Result<s::Store> {
    let root = secure_state_root(root, false)?;
    Ok(s::Store::open_runtime(
        &root.join("state.sqlite3"),
        storage_limits(),
        false,
    )?)
}

/// Explicit host-control entry: no ordinary opening reads or orphan recovery.
pub fn snapshot_at(root: &Path, operator: &str) -> Result<Value> {
    identity(operator)?;
    snapshot_store(&mut open(root)?, operator)
}

fn snapshot_store(store: &mut s::Store, operator: &str) -> Result<Value> {
    identity(operator)?;
    let (runtime, launch) = authorized(store, operator)?;
    let ledger = store.ledger()?;
    Ok(json!({
        "operator":operator,
        "expected_epoch":store.epoch()?,
        "expected_runtime":runtime.exact_ref()?,
        "expected_launch":launch.exact_ref()?,
        "expected_limits":ledger.limits,
        "ordinary":ledger.ordinary,"closeout":ledger.closeout,
        "frozen":ledger.frozen,
        "snapshot_atomic":false,"capacity_reserved":false,
        "supported_max_body_bytes":12*1024*1024,
        "supported_max_protected_closeout_retained_bytes":256*1024*1024,
        "supported_max_total_navigation_bytes":16u64*1024*1024*1024,
        "scope":"Larger snapshot bounds, protected retained-space partition and explicitly requested storage-read funding. No history, uncertainty or consumption is reset."
    }))
}

pub fn extend(
    app: &mut Runtime,
    request: &s::CapacityExtensionRequest,
    apply: bool,
) -> Result<Value> {
    extend_store(&mut app.store, request, apply)
}

/// Authority reads use existing Closeout resources; new capacity remains fenced
/// on exact state and launch heads, terminal invocations and execution ownership.
pub fn extend_at(root: &Path, request: &s::CapacityExtensionRequest, apply: bool) -> Result<Value> {
    validate(request, &request.operator)?;
    extend_store(&mut open(root)?, request, apply)
}

fn extend_store(
    store: &mut s::Store,
    request: &s::CapacityExtensionRequest,
    apply: bool,
) -> Result<Value> {
    validate(request, &request.operator)?;
    let _owner = store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (runtime, launch) = authorized(store, &request.operator)?;
    if let Some(receipt) = store.capacity_extension(&request.id)? {
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
    let receipt = store.extend_runtime_capacity(request, apply)?;
    let added_navigation = receipt.body["navigation_allowance_added_bytes"]
        .as_u64()
        .unwrap_or(0);
    Ok(json!({"receipt":receipt,"replayed":false,"applied":apply,
        "inference_performed":false,"resumed":false,"budget_reset":false,
        "total_resources_unchanged":added_navigation == 0,
        "navigation_allowance_added_bytes":added_navigation,
        "retained_history_unchanged":true,
        "note":"A preview reserves no capacity. An applied extension requires a binary supporting these retained migration receipts on subsequent opens."}))
}
