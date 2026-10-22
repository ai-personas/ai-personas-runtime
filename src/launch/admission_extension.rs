//! Trusted-operator extension of the launch-global lifetime step cap only.
//!
//! Work and stage identities retain the operator's purpose; they do not scope
//! the increased cap to that work or stage. No persona tool, inference,
//! credential resolution, token measurement, reset, refund, or resume occurs.
//!
//! Token figures are an operator snapshot attestation, not ongoing accounting
//! or new token funds. A serialized operator supervisor must enforce cumulative
//! usage plus aggregate pending reserves before every call across Stores.
//! Unknown reserves outside Store are preserved and are not invocation states.
//!
//! Inspection uses ordinary metered Store reads. Runtime::open may perform
//! normal orphan-dispatch recovery, including for snapshot and preview. New
//! apply acquires the existing execution owner, whose normal recovery may mark
//! orphaned Dispatched records Uncertain before rejection. This extension never
//! rewrites, reconciles, settles, adopts, or replays invocation history.
//!
//! The invocation guard checks persona pointers and every retained invocation
//! through the Store's bounded, ordinarily metered enumeration. Disconnected
//! unresolved records also block a new extension; terminal history stays intact.
use super::*;

const RECEIPT_KIND: &str = "launch-admission-extension";
const RECEIPT_VERSION: u32 = 1;
const SCOPE: &str = "launch_global";
const TOKEN_BASIS: &str = "operator_snapshot_attestation_only";
const MAX_REQUEST_BYTES: usize = 16_384;

// Enforce the nested request schema independently of ObjectRef's deserializer.
fn deserialize_exact_reference<'de, D>(
    deserializer: D,
) -> std::result::Result<s::ObjectRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactReference {
        id: String,
        revision: u64,
        sha256: String,
    }
    let reference = ExactReference::deserialize(deserializer)?;
    Ok(s::ObjectRef {
        id: reference.id,
        revision: reference.revision,
        sha256: reference.sha256,
    })
}

/// All fields are required. Work/stage are purpose metadata for a launch-global
/// change, not new work permissions or a restriction on which work uses the cap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchAdmissionExtensionRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub purpose: String,
    pub stage_id: String,
    pub work_id: String,
    #[serde(deserialize_with = "deserialize_exact_reference")]
    pub expected_runtime: s::ObjectRef,
    #[serde(deserialize_with = "deserialize_exact_reference")]
    pub expected_launch: s::ObjectRef,
    pub expected_steps: u64,
    pub expected_max_total_steps: u64,
    pub new_max_total_steps: u64,
    pub expected_max_steps: usize,
    pub authorize_extension: bool,
    /// Operator-attested cumulative usage; never measured by this operation.
    pub conservative_tokens_used: u64,
    pub conservative_token_ceiling: u64,
    /// Snapshot reserve for one call. The supervisor must separately account
    /// for aggregate pending reserves, including unknown usage across Stores.
    pub per_call_reserve_tokens: u64,
}

impl LaunchAdmissionExtensionRequest {
    /// Bound and validate input before opening state. This checks attribution,
    /// not saved authorization; snapshot/preview/apply check current config too.
    pub fn validate(&self, operator: &str) -> Result<()> {
        bounded_identity(operator)?;
        validate_request(self)?;
        require(
            self.operator == operator,
            "admission extension request operator must match --operator",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchAdmissionSnapshot {
    pub runtime: s::ObjectRef,
    pub launch: s::ObjectRef,
    pub operator: String,
    pub steps: u64,
    pub max_total_steps: u64,
    pub max_steps: usize,
}

/// Immutable historical evidence, including the complete requested attestation.
/// Target references need not remain current after subsequent work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchAdmissionExtensionReceipt {
    pub version: u32,
    pub scope: String,
    pub token_basis: String,
    pub request_sha256: String,
    pub request: LaunchAdmissionExtensionRequest,
    pub created_at_unix: i64,
    pub prior_runtime: s::ObjectRef,
    pub target_runtime: s::ObjectRef,
    pub prior_launch: s::ObjectRef,
    pub target_launch: s::ObjectRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchAdmissionExtensionOutcome {
    pub receipt: LaunchAdmissionExtensionReceipt,
    pub replayed: bool,
}

fn receipt_id(id: &str) -> String {
    format!("launch-admission-extension:{id}")
}

fn bounded_identity(value: &str) -> Result<()> {
    identity(value)?;
    require(
        value.len() <= 256,
        "admission extension identity exceeds bound",
    )
}

fn extension_id(id: &str) -> Result<()> {
    bounded_identity(id)?;
    require(
        id.chars().count() <= 96 && receipt_id(id).len() <= 256,
        "admission extension receipt identity exceeds bound",
    )
}

fn hash_string(hash: &str) -> Result<()> {
    require(
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "expected lowercase SHA-256",
    )
}

fn reference(reference: &s::ObjectRef, id: &str) -> Result<()> {
    bounded_identity(&reference.id)?;
    require(
        reference.id == id && reference.revision > 0,
        "invalid admission extension reference",
    )?;
    hash_string(&reference.sha256)
}

fn step_ceiling() -> Result<u64> {
    let limits = storage_limits();
    limits
        .total
        .attempts
        .checked_sub(limits.protected_closeout.attempts)
        .ok_or_else(|| invalid("invalid protected closeout attempt bound"))
}

// Before any Store read, including replay: bound all variable-size fields and
// arithmetic. Typed request values, not incoming JSON formatting, identify it.
fn validate_request(request: &LaunchAdmissionExtensionRequest) -> Result<String> {
    extension_id(&request.id)?;
    bounded_identity(&request.operator)?;
    bounded_identity(&request.stage_id)?;
    bounded_identity(&request.work_id)?;
    text(&request.reason, 4096)?;
    text(&request.purpose, 4096)?;
    require(
        !request.reason.trim().is_empty() && !request.purpose.trim().is_empty(),
        "admission extension reason and purpose are required",
    )?;
    reference(&request.expected_runtime, STATE_ID)?;
    reference(&request.expected_launch, ID)?;
    increment(request.expected_runtime.revision)?;
    increment(request.expected_launch.revision)?;
    require(
        request.authorize_extension,
        "explicit authorize_extension true is required",
    )?;
    require(
        (1..=128).contains(&request.expected_max_steps)
            && request.expected_max_total_steps > 0
            && request.expected_steps <= request.expected_max_total_steps
            && request.new_max_total_steps > request.expected_max_total_steps
            && request.new_max_total_steps <= step_ceiling()?,
        "invalid admission extension counters or bounds",
    )?;
    validate_token_attestation(
        request.conservative_tokens_used,
        request.conservative_token_ceiling,
        request.per_call_reserve_tokens,
    )?;
    let value = serde_json::to_value(request)?;
    require(
        serde_json::to_vec(&value)?.len() <= MAX_REQUEST_BYTES,
        "admission extension request exceeds bound",
    )?;
    digest(&value)
}

// Shared snapshot semantics only: this neither measures tokens nor reserves
// them. The existing serialized supervisor must account all pending/unknown
// reserves and cumulative usage across Stores before each actual call.
pub(super) fn validate_token_attestation(
    conservative_tokens_used: u64,
    conservative_token_ceiling: u64,
    per_call_reserve_tokens: u64,
) -> Result<()> {
    require(
        conservative_token_ceiling > 0 && per_call_reserve_tokens > 0,
        "positive operator token ceiling and per-call reserve are required",
    )?;
    let with_reserve = conservative_tokens_used
        .checked_add(per_call_reserve_tokens)
        .ok_or_else(|| invalid("operator token attestation overflow"))?;
    require(
        with_reserve <= conservative_token_ceiling,
        "operator snapshot usage plus per-call reserve exceeds its ceiling",
    )
}

fn candidates(app: &mut Runtime) -> Result<(s::StoredObject, State, s::StoredObject, Saved)> {
    let (runtime, state) = app.load()?;
    require(
        runtime.id == STATE_ID && runtime.kind == "runtime-state" && state.format == 1,
        "invalid runtime state",
    )?;
    let (launch, saved) = load(app)?;
    require(launch.id == ID, "invalid launch identity")?;
    Ok((runtime, state, launch, saved))
}

pub(super) fn authorize(state: &State, saved: &Saved, operator: &str) -> Result<()> {
    bounded_identity(operator)?;
    bounded_identity(&saved.operator)?;
    state.config.validate()?;
    require(
        saved.operator == operator && state.config.local_recipients.contains(operator),
        "saved operator and current runtime configuration must authorize the operator",
    )
}

// Nonmutating validation of the saved provider and complete cohort. Never call
// ensure_cohort here: its missing-member creation branch is not an extension.
pub(super) fn validate_saved(state: &State, saved: &Saved) -> Result<()> {
    let provider = saved.execution_provider()?;
    let setup = &saved.setup;
    bounded_identity(&setup.destination)?;
    bounded_identity(&provider.model)?;
    bounded_identity(&provider.effort)?;
    require(
        setup.share_cohort
            && (1..=8).contains(&setup.cohort)
            && (1..=setup.cohort).contains(&setup.owner)
            && (1..=128).contains(&setup.max_steps)
            && (1..=128).contains(&setup.max_considerations)
            && (1..=128).contains(&setup.max_work_attempts)
            && setup.max_total_steps > 0
            && setup.max_total_steps <= step_ceiling()?
            && (1..=128).contains(&setup.max_works),
        "invalid saved foreground bounds",
    )?;
    let config = &state.config;
    require(
        config.allow_direct_provider
            && config.persona_capacity == setup.persona_capacity
            && (setup.cohort..=8).contains(&setup.persona_capacity())
            && config.max_work_attempts == setup.max_work_attempts
            && provider.limits.max_prompt_bytes <= config.context_bytes
            && provider.limits.max_response_bytes <= config.response_bytes
            && provider.limits.max_stream_bytes as u64 <= config.transport_output_reserve_bytes,
        "saved provider exceeds current runtime authorization or bounds",
    )?;
    for id in people(setup.cohort) {
        require(id != saved.operator, "saved operator collides with cohort")?;
        let persona = state
            .personas
            .get(&id)
            .ok_or_else(|| held("saved cohort persona missing"))?;
        require(
            serde_json::to_value(&persona.seed)?
                == serde_json::to_value(seed_for(saved, &provider, &id))?,
            "saved cohort binding changed",
        )?;
    }
    for (work, last) in &saved.works {
        bounded_identity(work)?;
        if let Some(id) = last {
            bounded_identity(id)?;
        }
    }
    Ok(())
}

/// Metered inspection with current saved-operator/config authorization. The
/// exact references fence a later request; this is not an atomic read snapshot,
/// a capacity reservation, or token measurement. Normal open recovery applies.
pub fn launch_admission_snapshot(
    app: &mut Runtime,
    operator: &str,
) -> Result<LaunchAdmissionSnapshot> {
    bounded_identity(operator)?;
    let (runtime, state, launch, saved) = candidates(app)?;
    authorize(&state, &saved, operator)?;
    validate_saved(&state, &saved)?;
    Ok(LaunchAdmissionSnapshot {
        runtime: runtime.exact_ref()?,
        launch: launch.exact_ref()?,
        operator: saved.operator,
        steps: saved.steps,
        max_total_steps: saved.setup.max_total_steps,
        max_steps: saved.setup.max_steps,
    })
}

fn validate_receipt(receipt: &LaunchAdmissionExtensionReceipt) -> Result<()> {
    require(
        receipt.version == RECEIPT_VERSION
            && receipt.scope == SCOPE
            && receipt.token_basis == TOKEN_BASIS
            && receipt.created_at_unix >= 0,
        "invalid admission extension receipt metadata",
    )?;
    hash_string(&receipt.request_sha256)?;
    require(
        validate_request(&receipt.request)? == receipt.request_sha256,
        "admission extension receipt request digest is inconsistent",
    )?;
    reference(&receipt.prior_runtime, STATE_ID)?;
    reference(&receipt.target_runtime, STATE_ID)?;
    reference(&receipt.prior_launch, ID)?;
    reference(&receipt.target_launch, ID)?;
    require(
        receipt.prior_runtime == receipt.request.expected_runtime
            && receipt.prior_launch == receipt.request.expected_launch
            && receipt.target_runtime.revision == increment(receipt.prior_runtime.revision)?
            && receipt.target_launch.revision == increment(receipt.prior_launch.revision)?,
        "invalid admission extension receipt references",
    )?;
    require(
        serde_json::to_vec(receipt)?.len() <= 64 * 1024,
        "admission extension receipt exceeds bound",
    )
}

fn replay(
    app: &mut Runtime,
    request: &LaunchAdmissionExtensionRequest,
    request_sha256: &str,
) -> Result<Option<LaunchAdmissionExtensionReceipt>> {
    let id = receipt_id(&request.id);
    let Some(object) = app.store.read(&id)? else {
        return Ok(None);
    };
    // Current authorization is required, but historical heads, counters,
    // provider/cohort settings and invocation states need not still be current.
    let (_, state, _, saved) = candidates(app)?;
    authorize(&state, &saved, &request.operator)?;
    require(
        object.id == id && object.kind == RECEIPT_KIND && object.revision == 1,
        "invalid admission extension receipt record",
    )?;
    let receipt: LaunchAdmissionExtensionReceipt = serde_json::from_value(object.body)?;
    validate_receipt(&receipt)?;
    if receipt.request.id != request.id || receipt.request_sha256 != request_sha256 {
        return Err(s::StoreError::Conflict(id).into());
    }
    require(
        receipt.request == *request,
        "admission extension receipt request binding is inconsistent",
    )?;
    Ok(Some(receipt))
}

fn guard_invocations(app: &mut Runtime, state: &State) -> Result<()> {
    require(
        active_invocations(state).is_empty(),
        "active invocation blocks admission extension",
    )?;
    app.store.require_terminal_invocations()?;
    Ok(())
}

struct Planned {
    receipt: LaunchAdmissionExtensionReceipt,
    changes: Vec<s::Change>,
}

// The single construction path for preview and new apply. Apply invokes this
// after acquiring ownership, reloading heads, authorization, counters and links.
fn plan(
    app: &mut Runtime,
    request: &LaunchAdmissionExtensionRequest,
    request_sha256: String,
    now: i64,
) -> Result<Planned> {
    let (runtime_head, state, launch_head, saved) = candidates(app)?;
    authorize(&state, &saved, &request.operator)?;
    require(
        runtime_head.exact_ref()? == request.expected_runtime
            && launch_head.exact_ref()? == request.expected_launch,
        "admission extension heads changed",
    )?;
    validate_saved(&state, &saved)?;
    require(
        saved.steps == request.expected_steps
            && saved.setup.max_total_steps == request.expected_max_total_steps
            && saved.setup.max_steps == request.expected_max_steps,
        "admission extension counters or bounds changed",
    )?;
    require(
        saved.works.contains_key(&request.work_id),
        "admission extension work is absent from saved launch works",
    )?;
    guard_invocations(app, &state)?;

    // Preserve every other launch field, including omitted legacy defaults.
    // The old cap remains in its immutable prior launch revision.
    let mut launch = launch_head.clone();
    launch.revision = increment(launch.revision)?;
    launch.body["setup"]["max_total_steps"] = json!(request.new_max_total_steps);
    let target_launch = launch.exact_ref()?;

    // Only append operator evidence. No config, persona, work, resource,
    // permission, invocation, or remaining-allowance field is rewritten.
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    runtime.body["operator_events"]
        .as_array_mut()
        .ok_or_else(|| held("invalid operator event history"))?
        .push(json!({
            "kind": "launch_admission_extension",
            "id": request.id,
            "operator": request.operator,
            "reason": request.reason,
            "scope": SCOPE,
            "token_basis": TOKEN_BASIS,
            "request_sha256": request_sha256,
            "request": request,
            "target_launch": target_launch
        }));

    let receipt = LaunchAdmissionExtensionReceipt {
        version: RECEIPT_VERSION,
        scope: SCOPE.into(),
        token_basis: TOKEN_BASIS.into(),
        request_sha256,
        request: request.clone(),
        created_at_unix: now,
        prior_runtime: runtime_head.exact_ref()?,
        target_runtime: runtime.exact_ref()?,
        prior_launch: launch_head.exact_ref()?,
        target_launch,
    };
    validate_receipt(&receipt)?;
    let changes = vec![
        s::Change {
            expected_revision: Some(runtime_head.revision),
            object: runtime,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: Some(launch_head.revision),
            object: launch,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: None,
            object: s::StoredObject {
                id: receipt_id(&request.id),
                revision: 1,
                kind: RECEIPT_KIND.into(),
                body: serde_json::to_value(&receipt)?,
            },
            // Resolve against the complete proposed three-object batch.
            depends_on: vec![
                receipt.target_runtime.clone(),
                receipt.target_launch.clone(),
            ],
        },
    ];
    Ok(Planned { receipt, changes })
}

/// Build the same candidate batch as apply, using ordinary metered reads.
/// This reserves no capacity and promises neither later admission nor CAS
/// success. Matching IDs return immutable historical evidence. Preview itself
/// acquires no execution owner; normal Runtime opening may recover orphans.
pub fn preview_launch_admission_extension(
    app: &mut Runtime,
    request: &LaunchAdmissionExtensionRequest,
    now: i64,
) -> Result<LaunchAdmissionExtensionReceipt> {
    let hash = validate_request(request)?;
    require(now >= 0, "negative admission extension time")?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(receipt);
    }
    Ok(plan(app, request, hash, now)?.receipt)
}

/// Commit one ordinary three-object CAS, retaining immutable prior revisions.
/// Ordinary navigation/allocation remain charged; no balance is replenished or
/// reallocated and no continuation is authorized. Token attestation is not
/// measurement or enforcement of a cross-Store token ceiling.
///
/// Exact replay returns historical evidence without acquiring ownership or
/// authorizing execution. New apply holds the existing execution owner through
/// reload, validation and CAS. A running native execution prevents this write;
/// future native runs bind the changed cap using the unchanged binding rules.
/// Ownership acquisition may perform normal orphan recovery before rejection.
pub fn extend_launch_admission(
    app: &mut Runtime,
    request: &LaunchAdmissionExtensionRequest,
    now: i64,
) -> Result<LaunchAdmissionExtensionOutcome> {
    let hash = validate_request(request)?;
    require(now >= 0, "negative admission extension time")?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(LaunchAdmissionExtensionOutcome {
            receipt,
            replayed: true,
        });
    }
    let _owner = app.store.acquire_execution_owner()?;
    // An identical request may have committed between lookup and ownership.
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(LaunchAdmissionExtensionOutcome {
            receipt,
            replayed: true,
        });
    }
    let planned = plan(app, request, hash, now)?;
    app.store
        .compare_and_swap_batch_requiring_terminal_invocations(planned.changes)?;
    Ok(LaunchAdmissionExtensionOutcome {
        receipt: planned.receipt,
        replayed: false,
    })
}
