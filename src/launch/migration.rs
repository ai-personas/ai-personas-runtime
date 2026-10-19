//! Explicit trusted-operator migration; never a persona tool or an admission.
//!
//! Inspection spends ordinary Store navigation. Neither snapshot nor preview
//! acquires execution ownership or resolves credentials. Runtime::open may
//! already have performed normal orphan-dispatch recovery. Applying a new
//! migration acquires ownership and may likewise recover Dispatched records to
//! Uncertain before rejecting an active invocation. Migration itself never
//! clears, reconciles, settles, adopts, or replays an invocation.
use super::*;

const RECEIPT_KIND: &str = "provider-migration";
const RECEIPT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaBinding {
    pub requested_model: String,
    pub requested_effort: String,
    pub model_confirmed: bool,
    pub owner_epoch: u64,
}

/// Confirmations are explicit operator assertions, not live capability checks.
/// The destination authorizes the new route under the existing destination
/// label; this operation cannot change policies, audiences, grants, or exposure.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderMigrationRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub expected_runtime: s::ObjectRef,
    pub expected_launch: s::ObjectRef,
    pub expected_persona_bindings: BTreeMap<String, PersonaBinding>,
    pub provider: DirectProviderSettings,
    pub allow_direct_provider: bool,
    pub model_confirmed: bool,
    pub authorized_destination: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSnapshot {
    pub runtime: s::ObjectRef,
    pub launch: s::ObjectRef,
    pub persona_bindings: BTreeMap<String, PersonaBinding>,
    pub operator: String,
    pub destination: String,
    /// Digest of the saved JSON value, including an opaque format-1 provider.
    pub provider_config_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderMigrationReceipt {
    pub version: u32,
    pub id: String,
    pub request_sha256: String,
    pub operator: String,
    pub reason: String,
    pub created_at_unix: i64,
    pub prior_runtime: s::ObjectRef,
    pub target_runtime: s::ObjectRef,
    pub prior_launch: s::ObjectRef,
    pub target_launch: s::ObjectRef,
    pub before_bindings: BTreeMap<String, PersonaBinding>,
    pub after_bindings: BTreeMap<String, PersonaBinding>,
    pub old_provider_sha256: String,
    pub new_provider_sha256: String,
    pub authorized_destination: String,
    pub target_mode: provider::AccessMode,
    pub target_profile: String,
    pub target_model: String,
    pub target_effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderMigrationOutcome {
    pub receipt: ProviderMigrationReceipt,
    pub replayed: bool,
}

fn receipt_id(id: &str) -> String {
    format!("provider-migration:{id}")
}

fn migration_id(id: &str) -> Result<()> {
    identity(id)?;
    require(
        id.chars().count() <= 96,
        "migration id exceeds 96 characters",
    )?;
    require(
        receipt_id(id).len() <= 256,
        "migration receipt identity exceeds bound",
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
    identity(&reference.id)?;
    require(
        reference.id == id && reference.revision > 0,
        "invalid migration reference",
    )?;
    hash_string(&reference.sha256)
}

fn binding_bounds(bindings: &BTreeMap<String, PersonaBinding>) -> Result<()> {
    require(
        (1..=8).contains(&bindings.len()),
        "migration requires 1..=8 bindings",
    )?;
    for (id, binding) in bindings {
        identity(id)?;
        identity(&binding.requested_model)?;
        identity(&binding.requested_effort)?;
    }
    Ok(())
}

// Run before any Store read, including an idempotency lookup. All variable-size
// request fields are bounded before serialization. Typed maps and sets have
// deterministic order; incoming JSON formatting is not part of request identity.
fn validate_request(request: &ProviderMigrationRequest, now: i64) -> Result<String> {
    migration_id(&request.id)?;
    identity(&request.operator)?;
    text(&request.reason, 4096)?;
    identity(&request.authorized_destination)?;
    reference(&request.expected_runtime, STATE_ID)?;
    reference(&request.expected_launch, ID)?;
    binding_bounds(&request.expected_persona_bindings)?;
    require(now >= 0, "negative migration time")?;
    require(
        request.allow_direct_provider && request.model_confirmed,
        "explicit direct-provider opt-in and model confirmation required",
    )?;
    request.provider.validate()?;
    digest(&serde_json::to_value(request)?)
}

fn cohort(saved: &Saved) -> Result<Vec<String>> {
    identity(&saved.operator)?;
    identity(&saved.setup.destination)?;
    require(
        (1..=8).contains(&saved.setup.cohort)
            && (1..=saved.setup.cohort).contains(&saved.setup.owner)
            && saved.setup.share_cohort,
        "invalid saved cohort or sharing authorization",
    )?;
    let ids = people(saved.setup.cohort);
    require(
        !ids.contains(&saved.operator),
        "saved operator collides with cohort",
    )?;
    Ok(ids)
}

fn bindings(state: &State, ids: &[String]) -> Result<BTreeMap<String, PersonaBinding>> {
    let mut result = BTreeMap::new();
    for id in ids {
        let persona = state
            .personas
            .get(id)
            .ok_or_else(|| held("saved cohort persona missing"))?;
        result.insert(
            id.clone(),
            PersonaBinding {
                requested_model: persona.seed.requested_model.clone(),
                requested_effort: persona.seed.requested_effort.clone(),
                model_confirmed: persona.seed.model_confirmed,
                owner_epoch: persona.owner_epoch,
            },
        );
    }
    binding_bounds(&result)?;
    Ok(result)
}

fn candidates(app: &mut Runtime) -> Result<(s::StoredObject, State, s::StoredObject, Saved)> {
    let (runtime, state) = app.load()?;
    require(
        runtime.kind == "runtime-state" && state.format == 1,
        "invalid runtime state",
    )?;
    let (launch, saved) = load(app)?;
    Ok((runtime, state, launch, saved))
}

/// Metered ordinary inspection, without ownership acquisition or credential
/// resolution. References fence a later request; this is not an atomic read
/// transaction or a promise that the heads will remain current.
pub fn migration_snapshot(app: &mut Runtime) -> Result<MigrationSnapshot> {
    let (runtime, state, launch, saved) = candidates(app)?;
    let ids = cohort(&saved)?;
    Ok(MigrationSnapshot {
        runtime: runtime.exact_ref()?,
        launch: launch.exact_ref()?,
        persona_bindings: bindings(&state, &ids)?,
        provider_config_sha256: digest(&saved.setup.provider)?,
        operator: saved.operator,
        destination: saved.setup.destination,
    })
}

fn validate_receipt(receipt: &ProviderMigrationReceipt) -> Result<()> {
    require(
        receipt.version == RECEIPT_VERSION,
        "unsupported migration receipt version",
    )?;
    migration_id(&receipt.id)?;
    identity(&receipt.operator)?;
    text(&receipt.reason, 4096)?;
    identity(&receipt.authorized_destination)?;
    identity(&receipt.target_model)?;
    identity(&receipt.target_effort)?;
    require(
        receipt.created_at_unix >= 0 && receipt.target_profile == receipt.target_mode.profile(),
        "invalid migration receipt metadata",
    )?;
    for hash in [
        &receipt.request_sha256,
        &receipt.old_provider_sha256,
        &receipt.new_provider_sha256,
    ] {
        hash_string(hash)?;
    }
    reference(&receipt.prior_runtime, STATE_ID)?;
    reference(&receipt.target_runtime, STATE_ID)?;
    reference(&receipt.prior_launch, ID)?;
    reference(&receipt.target_launch, ID)?;
    require(
        receipt.target_runtime.revision == increment(receipt.prior_runtime.revision)?
            && receipt.target_launch.revision == increment(receipt.prior_launch.revision)?,
        "invalid migration receipt revisions",
    )?;
    binding_bounds(&receipt.before_bindings)?;
    binding_bounds(&receipt.after_bindings)?;
    require(
        receipt
            .before_bindings
            .keys()
            .eq(receipt.after_bindings.keys()),
        "invalid migration receipt cohort",
    )?;
    for (id, before) in &receipt.before_bindings {
        let after = &receipt.after_bindings[id];
        require(
            after.owner_epoch == increment(before.owner_epoch)?
                && after.model_confirmed
                && after.requested_model == receipt.target_model
                && after.requested_effort == receipt.target_effort,
            "invalid migration receipt binding transition",
        )?;
    }
    require(
        serde_json::to_vec(receipt)?.len() <= 64 * 1024,
        "migration receipt exceeds bound",
    )
}

fn replay(
    app: &mut Runtime,
    request: &ProviderMigrationRequest,
    request_sha256: &str,
) -> Result<Option<ProviderMigrationReceipt>> {
    let id = receipt_id(&request.id);
    let Some(object) = app.store.read(&id)? else {
        return Ok(None);
    };
    // Authorization remains current even when the receipt describes old heads.
    // Do not parse the current provider or require the historical heads to match.
    let (_, saved) = load(app)?;
    require(
        saved.operator == request.operator,
        "saved operator mismatch",
    )?;
    require(
        object.id == id && object.kind == RECEIPT_KIND && object.revision == 1,
        "invalid migration receipt record",
    )?;
    let receipt: ProviderMigrationReceipt = serde_json::from_value(object.body)?;
    validate_receipt(&receipt)?;
    if receipt.id != request.id || receipt.request_sha256 != request_sha256 {
        return Err(s::StoreError::Conflict(id).into());
    }
    require(
        receipt.operator == request.operator
            && receipt.reason == request.reason
            && receipt.prior_runtime == request.expected_runtime
            && receipt.prior_launch == request.expected_launch
            && receipt.before_bindings == request.expected_persona_bindings
            && receipt.authorized_destination == request.authorized_destination
            && receipt.new_provider_sha256 == digest(&serde_json::to_value(&request.provider)?)?
            && receipt.target_mode == request.provider.mode
            && receipt.target_model == request.provider.model
            && receipt.target_effort == request.provider.effort,
        "migration receipt request binding is inconsistent",
    )?;
    Ok(Some(receipt))
}

struct Planned {
    receipt: ProviderMigrationReceipt,
    changes: Vec<s::Change>,
}

// The only construction path for both preview and commit. No ensure_cohort call:
// its creation branch must never participate in a migration.
fn plan(
    app: &mut Runtime,
    request: &ProviderMigrationRequest,
    request_sha256: String,
    now: i64,
) -> Result<Planned> {
    let (runtime_head, mut state, launch_head, mut saved) = candidates(app)?;
    require(
        saved.operator == request.operator,
        "saved operator mismatch",
    )?;
    require(
        saved.setup.destination == request.authorized_destination,
        "migration must explicitly authorize the existing destination",
    )?;
    require(
        runtime_head.exact_ref()? == request.expected_runtime
            && launch_head.exact_ref()? == request.expected_launch,
        "migration heads changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks provider migration",
    )?;
    let ids = cohort(&saved)?;
    let before = bindings(&state, &ids)?;
    require(
        before == request.expected_persona_bindings,
        "complete saved cohort bindings changed",
    )?;
    let limits = &request.provider.limits;
    require(
        limits.max_prompt_bytes <= state.config.context_bytes
            && limits.max_response_bytes <= state.config.response_bytes
            && limits.max_stream_bytes as u64 <= state.config.transport_output_reserve_bytes,
        "provider output exceeds saved runtime bounds",
    )?;
    let old_provider_sha256 = digest(&saved.setup.provider)?;
    let provider_value = serde_json::to_value(&request.provider)?;
    let new_provider_sha256 = digest(&provider_value)?;

    // Patch original JSON rather than reserializing all State fields. This also
    // preserves omitted legacy defaults outside the explicitly allowed delta.
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    for id in &ids {
        let persona = state
            .personas
            .get_mut(id)
            .ok_or_else(|| held("saved cohort persona missing"))?;
        let mut expected = seed_for(&saved, &request.provider, id);
        // Old Seeds are authoritative, including differing cohort model/effort
        // bindings. Only their static launch metadata must match seed_for.
        expected.requested_model = persona.seed.requested_model.clone();
        expected.requested_effort = persona.seed.requested_effort.clone();
        expected.model_confirmed = persona.seed.model_confirmed;
        require(
            serde_json::to_value(&persona.seed)? == serde_json::to_value(expected)?,
            "saved cohort static seed metadata changed",
        )?;
        persona.owner_epoch = increment(persona.owner_epoch)?;
        persona.seed.requested_model = request.provider.model.clone();
        persona.seed.requested_effort = request.provider.effort.clone();
        persona.seed.model_confirmed = true;
        let body = &mut runtime.body["personas"][id.as_str()];
        body["owner_epoch"] = json!(persona.owner_epoch);
        body["seed"]["requested_model"] = json!(persona.seed.requested_model);
        body["seed"]["requested_effort"] = json!(persona.seed.requested_effort);
        body["seed"]["model_confirmed"] = json!(true);
    }
    runtime.body["config"]["allow_direct_provider"] = json!(true);
    runtime.body["operator_events"]
        .as_array_mut()
        .ok_or_else(|| held("invalid operator event history"))?
        .push(json!({
            "kind":"provider_migration", "id":request.id,
            "request_sha256":request_sha256, "operator":request.operator, "reason":request.reason
        }));

    saved.format = 2;
    saved.direct_provider_opt_in = Some(true);
    saved.research_opt_in = None;
    saved.setup.provider = provider_value.clone();
    let mut launch = launch_head.clone();
    launch.revision = increment(launch.revision)?;
    launch.body["format"] = json!(2);
    launch.body["direct_provider_opt_in"] = json!(true);
    launch
        .body
        .as_object_mut()
        .ok_or_else(|| held("invalid launch body"))?
        .remove("research_opt_in");
    launch.body["setup"]["provider"] = provider_value;

    // Exact, nonmutating equivalent of ensure_cohort's existing-persona branch.
    // All members were required above; no missing-member creation is possible.
    for id in &ids {
        require(
            serde_json::to_value(&state.personas[id].seed)?
                == serde_json::to_value(seed_for(&saved, &request.provider, id))?,
            "migration cohort postcondition failed",
        )?;
    }
    let receipt = ProviderMigrationReceipt {
        version: RECEIPT_VERSION,
        id: request.id.clone(),
        request_sha256,
        operator: request.operator.clone(),
        reason: request.reason.clone(),
        created_at_unix: now,
        prior_runtime: runtime_head.exact_ref()?,
        target_runtime: runtime.exact_ref()?,
        prior_launch: launch_head.exact_ref()?,
        target_launch: launch.exact_ref()?,
        before_bindings: before,
        after_bindings: bindings(&state, &ids)?,
        old_provider_sha256,
        new_provider_sha256,
        authorized_destination: request.authorized_destination.clone(),
        target_mode: request.provider.mode,
        target_profile: request.provider.mode.profile().into(),
        target_model: request.provider.model.clone(),
        target_effort: request.provider.effort.clone(),
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
            // Dependencies resolve against the complete proposed batch.
            depends_on: vec![
                receipt.target_runtime.clone(),
                receipt.target_launch.clone(),
            ],
        },
    ];
    Ok(Planned { receipt, changes })
}

/// Build the same candidate batch as apply, spending ordinary navigation only.
/// This reserves no capacity and cannot guarantee later Store admission or CAS.
/// An existing matching ID returns its historical receipt, whose target need
/// not still be current. Preview performs no ownership recovery itself.
pub fn preview_provider_migration(
    app: &mut Runtime,
    request: &ProviderMigrationRequest,
    now: i64,
) -> Result<ProviderMigrationReceipt> {
    let hash = validate_request(request, now)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(receipt);
    }
    Ok(plan(app, request, hash, now)?.receipt)
}

/// Commit exactly one ordinary three-object CAS batch. Existing immutable
/// revisions remain retained; ordinary navigation and allocation are charged.
/// No work allowance, continuation permission, or funding is replenished.
///
/// Exact replay authorizes no new execution and returns historical evidence,
/// even after another migration. It performs ordinary navigation, without
/// acquiring the recovering execution owner. For a new migration, ownership
/// recovery can change orphaned invocation lifecycle records before rejection;
/// the guard is retained through the entire CAS operation.
pub fn migrate_provider(
    app: &mut Runtime,
    request: &ProviderMigrationRequest,
    now: i64,
) -> Result<ProviderMigrationOutcome> {
    let hash = validate_request(request, now)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(ProviderMigrationOutcome {
            receipt,
            replayed: true,
        });
    }
    let _owner = app.store.acquire_execution_owner()?;
    // Another migration may have committed between the first lookup and lock.
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(ProviderMigrationOutcome {
            receipt,
            replayed: true,
        });
    }
    let planned = plan(app, request, hash, now)?;
    app.store.compare_and_swap_batch(planned.changes)?;
    Ok(ProviderMigrationOutcome {
        receipt: planned.receipt,
        replayed: false,
    })
}
