//! Atomic fresh launch setup. An exact key replays only its historical receipt;
//! it cannot reset, refill, repair an older partial setup, or rebind a live Store.
use super::*;

const RECEIPT: &str = "launch-bootstrap-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub operator: String,
    pub setup: Setup,
    /// Attributed starting profiles for every member of this fresh cohort.
    /// They are creation input, never accepted character or prior experience.
    pub persona_seeds: BTreeMap<String, PersonaSeed>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    pub id: String,
    pub operator: String,
    pub request_sha256: String,
    pub initial_runtime: s::ObjectRef,
    pub initial_launch: s::ObjectRef,
}
#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub receipt: Receipt,
    pub replayed: bool,
    pub inference_performed: bool,
    pub existing_state_reset: bool,
}

fn prepared(request: &Request) -> Result<(Config, Saved)> {
    identity(&request.id)?;
    let (config, mut saved) = setup_parts(&request.operator, request.setup.clone())?;
    install_persona_seeds(&mut saved, request.persona_seeds.clone())?;
    config.validate()?;
    Ok((config, saved))
}
/// Prospective setup, profiles and accounting metadata. No private Store is
/// created/opened and no credential is resolved or inference performed.
pub fn preview(request: &Request) -> Result<Value> {
    let (config, saved) = prepared(request)?;
    let provider = saved.execution_provider()?;
    Ok(json!({
        "request_sha256": digest(request)?, "runtime_config": config,
        "storage_limits": storage_limits(), "setup": request.setup,
        "persona_seeds": saved.setup.persona_seeds,
        "seed_grants": seed_for(&saved, &provider, &people(saved.setup.cohort)[0]).grants,
        "created_personas": people(saved.setup.cohort),
        "initial_audience_personas": people(saved.setup.cohort),
        "remaining_persona_slots": null,"population_quota_enforced":false,
        "native_work_guides":work_command_guides::fresh_summary(&config, &saved)?,
        "inference_performed": false, "authority_granted": false,
        "remote_token_hard_bound": false, "remote_dollar_hard_bound": false
    }))
}
/// Explicitly authorized atomic initialization of a new private Store. If SQLite
/// was created but initialization never committed, retry can initialize it. Once
/// committed, all state/cohort/launch/receipt/control records exist together.
/// Existing initialized Stores lacking THIS receipt are never adopted/repaired.
pub fn apply(root: &Path, request: &Request) -> Result<Outcome> {
    apply_with(root, request, || Ok(()))
}
fn apply_with(
    root: &Path,
    request: &Request,
    after_commit: impl FnOnce() -> Result<()>,
) -> Result<Outcome> {
    let (config, saved) = prepared(request)?;
    let root = private_tree(root)?;
    let mut store = s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits())?;
    let _owner = store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("bootstrap execution owner busy"))?;
    let request_sha256 = digest(request)?;
    if let Some(record) = store.read(RECEIPT)? {
        require(
            record.kind == RECEIPT && record.revision == 1,
            "invalid bootstrap receipt",
        )?;
        let receipt: Receipt = serde_json::from_value(record.body)?;
        require(
            receipt.version == 1
                && receipt.id == request.id
                && receipt.operator == request.operator
                && receipt.request_sha256 == request_sha256,
            "bootstrap key or exact authorized settings changed",
        )?;
        // Current heads may have legitimately advanced. Verify retained origins,
        // not a UI success marker, without overwriting any current state.
        let original = store.read_exact(&receipt.initial_runtime)?;
        let launch = store.read_exact(&receipt.initial_launch)?;
        require(
            original.kind == "runtime-state"
                && launch.kind == "foreground-launch"
                && launch.body["operator"] == request.operator,
            "bootstrap origins unavailable",
        )?;
        return Ok(Outcome {
            receipt,
            replayed: true,
            inference_performed: false,
            existing_state_reset: false,
        });
    }
    require(
        store.read(STATE_ID)?.is_none() && store.read(ID)?.is_none(),
        "existing Store has no matching atomic bootstrap receipt; inspect without reset",
    )?;
    store.require_quiescent_invocations()?;
    let mut state = initial_state(config, &request.operator);
    let provider = saved.execution_provider()?;
    for id in people(saved.setup.cohort) {
        let seed = seed_for(&saved, &provider, &id);
        validate_seed(&seed)?;
        insert_seed(&mut state, &request.operator, seed)?;
    }
    work_guides::install(&mut state, &request.operator, &people(saved.setup.cohort)[0], POLICY, unix_now()?)?;
    let runtime = s::StoredObject {
        id: STATE_ID.into(),
        revision: 1,
        kind: "runtime-state".into(),
        body: state_storage::to_value(&state)?,
    };
    drop(state);
    let launch = s::StoredObject {
        id: ID.into(),
        revision: 1,
        kind: "foreground-launch".into(),
        body: serde_json::to_value(saved)?,
    };
    let receipt = Receipt {
        version: 1,
        id: request.id.clone(),
        operator: request.operator.clone(),
        request_sha256,
        initial_runtime: runtime.exact_ref()?,
        initial_launch: launch.exact_ref()?,
    };
    let record = s::StoredObject {
        id: RECEIPT.into(),
        revision: 1,
        kind: RECEIPT.into(),
        body: serde_json::to_value(&receipt)?,
    };
    store.trusted_host().initialize_runtime_state_with_changes(
        runtime,
        &request.operator,
        vec![launch, record]
            .into_iter()
            .map(|object| s::Change {
                expected_revision: None,
                object,
                depends_on: Vec::new(),
            })
            .collect(),
    )?;
    after_commit()?;
    Ok(Outcome {
        receipt,
        replayed: false,
        inference_performed: false,
        existing_state_reset: false,
    })
}
