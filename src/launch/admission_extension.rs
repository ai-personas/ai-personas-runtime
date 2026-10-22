//! Trusted-operator updates to historical launch-global step metadata.
//!
//! Work and stage identities retain the operator's purpose; they do not scope
//! the recorded value to that work or stage. No persona tool, inference,
//! credential resolution, token measurement, reset, refund, or resume occurs.
//!
//! Token figures are an operator snapshot attestation, not measured usage,
//! an application quota, reserved provider capacity or new token funds.
//!
//! Inspection uses ordinary metered Store reads. Runtime::open may perform
//! normal orphan-dispatch recovery, including for snapshot and preview. New
//! apply acquires the existing execution owner, whose normal recovery may mark
//! orphaned Dispatched records Uncertain before rejection. This extension never
//! rewrites, reconciles, settles, adopts, or replays invocation history.
//!
//! The invocation guard checks persona pointers and every retained invocation
//! through the Store's ordinarily metered enumeration. Disconnected
//! unresolved records also block a new extension; terminal history stays intact.
use super::*;

const RECEIPT_KIND: &str = "launch-admission-extension";
const RECEIPT_VERSION: u32 = 1;
const SCOPE: &str = "launch_global";
const TOKEN_BASIS: &str = "operator_snapshot_attestation_only";

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
    #[serde(default)]
    pub authorize_extension: bool,
    /// Operator-attested cumulative usage; never measured by this operation.
    pub conservative_tokens_used: u64,
    pub conservative_token_ceiling: u64,
    /// Historical operator estimate; this does not establish actual usage.
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
    identity(value)
}

fn extension_id(id: &str) -> Result<()> {
    bounded_identity(id)
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

// Typed request values, not incoming JSON formatting, identify this metadata
// update. Saved ceilings do not limit admission or require an increase.
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
    validate_token_attestation(
        request.conservative_tokens_used,
        request.conservative_token_ceiling,
        request.per_call_reserve_tokens,
    )?;
    let value = serde_json::to_value(request)?;
    digest(&value)
}

// Saved token attestations are provenance, not admission quotas or usage evidence.
pub(super) fn validate_token_attestation(
    _conservative_tokens_used: u64,
    _conservative_token_ceiling: u64,
    _per_call_reserve_tokens: u64,
) -> Result<()> {
    Ok(())
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
        setup.cohort > 0
            && (1..=setup.cohort).contains(&setup.owner)
            && setup.max_considerations > 0,
        "invalid saved foreground bounds",
    )?;
    let config = &state.config;
    require(
        config.persona_capacity == setup.persona_capacity
            && config.max_work_attempts == setup.max_work_attempts,
        "saved provider exceeds current runtime authorization or bounds",
    )?;
    for id in people(setup.cohort) {
        require(id != saved.operator, "saved operator collides with cohort")?;
        let persona = state
            .personas
            .get(&id)
            .ok_or_else(|| held("saved cohort persona missing"))?;
        require(
            seed_matches_saved(saved, &provider, &id, &persona.seed)?,
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
    Ok(())
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
    app.store.require_quiescent_invocations()?;
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

    // Carry current episode bindings across this metadata update, as in human
    // intake and work-resource updates. An already stale binding stays stale.
    let previous_binding = intake_delegation::episode_launch_binding(&saved)?;
    let mut next_saved = saved.clone();
    next_saved.setup.max_total_steps = request.new_max_total_steps;
    let next_binding = intake_delegation::episode_launch_binding(&next_saved)?;
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    let mut carried = Vec::new();
    if previous_binding != next_binding {
        for (id, episode) in &state.intake_episodes {
            if episode.launch_binding_sha256 == previous_binding {
                runtime.body["intake_episodes"][id]["launch_binding_sha256"] = json!(next_binding);
                carried.push(id.clone());
            }
        }
    }
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
            "target_launch": target_launch,
            "episode_binding_transition": {
                "from": previous_binding, "to": next_binding, "carried_episodes": carried
            }
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
        .compare_and_swap_batch_requiring_quiescent_invocations(planned.changes)?;
    Ok(LaunchAdmissionExtensionOutcome {
        receipt: planned.receipt,
        replayed: false,
    })
}
