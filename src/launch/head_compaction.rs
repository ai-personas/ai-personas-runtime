//! Exact operator-authorized reclamation of unused ordinary head capacity.
use super::*;

pub fn validate(request: &s::HeadCompactionRequest, operator: &str) -> Result<()> {
    identity(operator)?;
    require(request.operator == operator, "head compaction operator mismatch")?;
    s::Store::validate_head_compaction_request(request)?;
    Ok(())
}

pub fn compact_at(root: &Path, request: &s::HeadCompactionRequest, apply: bool) -> Result<Value> {
    validate(request, &request.operator)?;
    let root = canonical_state_root(root, false)?;
    let mut store = s::Store::open_runtime(&root.join("state.sqlite3"), storage_limits(), false)?;
    let _owner = store.try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (receipt, replayed) = store.compact_object_heads(request, apply, |runtime, launch| {
        store_capacity::authorize_heads(runtime, launch, &request.operator)
            .map_err(|error| s::StoreError::Invalid(error.to_string()))
    })?;
    Ok(json!({"receipt":receipt,"replayed":replayed,"applied":apply || replayed,
        "inference_performed":false,"resumed":false,"limits_changed":false,"budget_reset":false,
        "note":"Preview reserves nothing. Apply reclaims only unused Ordinary head allocation and requires schema-2-aware native helpers afterward. Every existing data byte and failed attempt remains retained; no SQLite file shrink is promised."}))
}
