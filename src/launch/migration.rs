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

/// The explicit request selects the provider. Historical confirmation fields
/// are optional metadata, not live capability checks or additional approvals.
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
    /// Change the credential route while retaining each persona's current model
    /// and reasoning effort, for example an authorized API-key to OAuth switch.
    #[serde(default)]
    pub preserve_persona_models: bool,
    /// When retaining models, optionally choose one reasoning effort for the
    /// cohort without replacing its individual model assignments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_effort_override: Option<String>,
    /// Explicit recovery of an earlier migration that left episode bindings on
    /// its prior launch. Both current heads must still equal that receipt's
    /// exact targets; no intervening work or unrelated stale grant is repaired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_episode_bindings_from: Option<String>,
    #[serde(default)]
    pub allow_direct_provider: bool,
    #[serde(default)]
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
    #[serde(default)]
    pub preserve_persona_models: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_effort_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_episode_bindings_from: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carried_intake_episodes: Vec<String>,
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
    identity(id)
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
    require(!bindings.is_empty(), "migration requires persona bindings")?;
    for (id, binding) in bindings {
        identity(id)?;
        identity(&binding.requested_model)?;
        identity(&binding.requested_effort)?;
    }
    Ok(())
}

fn effort_override(preserve_models: bool, effort: &Option<String>) -> Result<()> {
    if let Some(effort) = effort {
        require(preserve_models, "a persona effort override requires retaining persona models")?;
        identity(effort)?;
    }
    Ok(())
}

// Run before any Store read, including an idempotency lookup. Typed maps and sets have
// deterministic order; incoming JSON formatting is not part of request identity.
fn validate_request(request: &ProviderMigrationRequest, now: i64) -> Result<String> {
    migration_id(&request.id)?;
    if let Some(id) = &request.repair_episode_bindings_from {
        migration_id(id)?;
        require(id != &request.id, "a migration cannot repair itself")?;
    }
    identity(&request.operator)?;
    text(&request.reason, 4096)?;
    identity(&request.authorized_destination)?;
    reference(&request.expected_runtime, STATE_ID)?;
    reference(&request.expected_launch, ID)?;
    binding_bounds(&request.expected_persona_bindings)?;
    effort_override(request.preserve_persona_models, &request.persona_effort_override)?;
    require(now >= 0, "negative migration time")?;

    request.provider.validate()?;
    digest(&serde_json::to_value(request)?)
}

fn cohort(saved: &Saved) -> Result<Vec<String>> {
    identity(&saved.operator)?;
    identity(&saved.setup.destination)?;
    require(
        saved.setup.cohort > 0 && (1..=saved.setup.cohort).contains(&saved.setup.owner),
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
    Ok((runtime.as_ref().clone(), state, launch, saved))
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

/// Explicit local cleanup before taking migration heads. Only failed calls with
/// an observed stop are candidates; exact terminal evidence and absence of any
/// adopted action are checked by the ordinary retirement path. This does not
/// resume work, start inference, replay effects, or settle unknown remote usage.
pub fn retire_stopped_invocations(app: &mut Runtime, operator: &str, now: i64) -> Result<Vec<String>> {
    let (_, saved) = load(app)?;
    require(saved.operator == operator, "saved operator mismatch")?;
    require(now >= 0, "negative retirement time")?;
    let _owner = app.store.acquire_execution_owner()?;
    let (head, state) = app.load()?;
    let mut retired = Vec::new();
    for id in active_invocations(&state).into_values() {
        let invocation = app.invocation(&id)?;
        if invocation.state != s::InvocationState::Failed { continue; }
        let prepared = decode_prepared(&invocation)?;
        if provider_calls::state_stop(&state, head.revision,
            prepared.runtime_control_event_cursor.unwrap_or(state.operator_events.len()),
            &prepared.request)?.is_some()
            && app.retire_rejected_continuation(&id, now)?
        {
            retired.push(id);
        }
    }
    Ok(retired)
}

fn validate_receipt(receipt: &ProviderMigrationReceipt) -> Result<()> {
    require(
        receipt.version == RECEIPT_VERSION,
        "unsupported migration receipt version",
    )?;
    migration_id(&receipt.id)?;
    if let Some(id) = &receipt.repair_episode_bindings_from {
        migration_id(id)?;
    }
    for id in &receipt.carried_intake_episodes {
        identity(id)?;
    }
    require(
        receipt
            .carried_intake_episodes
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            == receipt.carried_intake_episodes.len(),
        "duplicate carried episode",
    )?;
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
    effort_override(receipt.preserve_persona_models, &receipt.persona_effort_override)?;
    require(
        receipt
            .before_bindings
            .keys()
            .eq(receipt.after_bindings.keys()),
        "invalid migration receipt cohort",
    )?;
    for (id, before) in &receipt.before_bindings {
        let after = &receipt.after_bindings[id];
        let (model, effort) = if receipt.preserve_persona_models {
            (&before.requested_model, receipt.persona_effort_override.as_ref().unwrap_or(&before.requested_effort))
        } else {
            (&receipt.target_model, &receipt.target_effort)
        };
        require(
            after.owner_epoch == increment(before.owner_epoch)?
                && after.model_confirmed
                && &after.requested_model == model
                && &after.requested_effort == effort,
            "invalid migration receipt binding transition",
        )?;
    }
    Ok(())
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
            && receipt.target_effort == request.provider.effort
            && receipt.preserve_persona_models == request.preserve_persona_models
            && receipt.persona_effort_override == request.persona_effort_override
            && receipt.repair_episode_bindings_from == request.repair_episode_bindings_from,
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

    let old_provider_sha256 = digest(&saved.setup.provider)?;
    let previous_episode_binding = intake_delegation::episode_launch_binding(&saved)?;
    let repair = if let Some(id) = &request.repair_episode_bindings_from {
        let object = app
            .store
            .read(&receipt_id(id))?
            .ok_or_else(|| held("provider migration repair receipt absent"))?;
        require(
            object.id == receipt_id(id) && object.kind == RECEIPT_KIND && object.revision == 1,
            "provider migration repair receipt record changed",
        )?;
        let receipt: ProviderMigrationReceipt = serde_json::from_value(object.body)?;
        validate_receipt(&receipt)?;
        require(
            receipt.id == *id
                && receipt.operator == request.operator
                && receipt.target_runtime == runtime_head.exact_ref()?
                && receipt.target_launch == launch_head.exact_ref()?,
            "provider migration repair requires the unchanged exact target heads",
        )?;
        let prior_launch = app.store.read_exact(&receipt.prior_launch)?;
        let prior_runtime = app.store.read_exact(&receipt.prior_runtime)?;
        let prior_saved = decode_saved(&prior_launch)?;
        require(
            prior_saved.operator == saved.operator
                && prior_saved.setup.destination == saved.setup.destination,
            "provider migration repair authority changed",
        )?;
        Some((
            intake_delegation::episode_launch_binding(&prior_saved)?,
            prior_runtime,
        ))
    } else {
        None
    };
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
        if !request.preserve_persona_models {
            persona.seed.requested_model = request.provider.model.clone();
            persona.seed.requested_effort = request.provider.effort.clone();
        } else if let Some(effort) = &request.persona_effort_override {
            persona.seed.requested_effort = effort.clone();
        }
        persona.seed.model_confirmed = true;
        // Attribute later calls to the explicitly selected provider. Retained
        // source revisions keep their original destination attribution.
        persona.seed.destination = request.provider.mode.destination().into();
        let body = &mut runtime.body["personas"][id.as_str()];
        body["owner_epoch"] = json!(persona.owner_epoch);
        body["seed"]["requested_model"] = json!(persona.seed.requested_model);
        body["seed"]["requested_effort"] = json!(persona.seed.requested_effort);
        body["seed"]["model_confirmed"] = json!(true);
        body["seed"]["destination"] = json!(persona.seed.destination);
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
    saved.setup.destination = request.provider.mode.destination().into();
    // Every later dispatch must use the selected credential route, including
    // personas that previously had their own provider override. Model choices
    // remain explicit when the operator requests a route-only switch.
    require(saved.setup.persona_seeds.keys().all(|id| before.contains_key(id)),
        "saved persona seed is outside the migration cohort")?;
    for id in &ids {
        let current = &state.personas[id].seed;
        let seed = saved.setup.persona_seeds.entry(id.clone()).or_insert_with(|| PersonaSeed {
            label: current.label.clone(), text: current.text.clone(), provenance: current.provenance.clone(),
            model: None, effort: None, provider: None,
        });
        seed.model = request.preserve_persona_models.then(|| current.requested_model.clone());
        seed.effort = request.preserve_persona_models.then(|| current.requested_effort.clone());
        seed.provider = None;
    }
    // The exact current delegation source and admitted children remain intact.
    // Carry their launch fingerprint through this authorized configuration
    // change, as other launch changes do. Unknown stale episodes stay stale.
    let next_episode_binding = intake_delegation::episode_launch_binding(&saved)?;
    let mut carried_intake_episodes = Vec::new();
    for (id, episode) in &state.intake_episodes {
        let current = episode.launch_binding_sha256 == previous_episode_binding;
        let repairable = repair.as_ref().is_some_and(|(binding, prior)| {
            &episode.launch_binding_sha256 == binding
                && prior.body["intake_episodes"].get(id).is_some()
                && prior.body["intake_episodes"][id] == runtime_head.body["intake_episodes"][id]
        });
        if (current || repairable) && episode.launch_binding_sha256 != next_episode_binding {
            runtime.body["intake_episodes"][id]["launch_binding_sha256"] =
                json!(next_episode_binding);
            carried_intake_episodes.push(id.clone());
        }
    }
    let event = runtime.body["operator_events"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    event["carried_intake_episodes"] = json!(carried_intake_episodes);
    if let Some(id) = &request.repair_episode_bindings_from {
        event["repair_episode_bindings_from"] = json!(id);
    }
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
    launch.body["setup"]["destination"] = json!(saved.setup.destination);
    launch.body["setup"]["persona_seeds"] = serde_json::to_value(&saved.setup.persona_seeds)?;

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
        preserve_persona_models: request.preserve_persona_models,
        persona_effort_override: request.persona_effort_override.clone(),
        repair_episode_bindings_from: request.repair_episode_bindings_from.clone(),
        carried_intake_episodes,
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
