//! Atomic human-origin intake with observational allocation metadata. Never issues persona delegation,
//! resets an old allowance, resumes an old focus, or dispatches inference.
use super::*;

const RECEIPT_KIND: &str = "bounded-human-intake";
const AUTH_BASIS: &str = "trusted operator attestation referencing prior explicit human authorization; not independent authentication";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanIntakeFence {
    pub runtime: intake_delegation::IntakeReference,
    pub launch: intake_delegation::IntakeReference,
    pub steps: u64,
    pub max_total_steps: u64,
    pub work_count: u64,
    pub max_works: u64,
    pub max_steps: usize,
}

/// The trusted operator must supply the existing approval's exact statement
/// and provenance. A digest establishes content identity, never human identity.
/// Saved ceiling values are provenance, not admission limits or delegation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanAuthorizationEvidence {
    pub reference: String,
    pub statement: String,
    pub statement_sha256: String,
    pub authorized_max_total_steps: u64,
    pub authorized_max_works: u64,
    pub model: String,
    pub effort: String,
    pub destination: String,
    pub cumulative_token_ceiling: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanIntakeRequest {
    pub id: String,
    pub operator: String,
    pub purpose: String,
    #[serde(default)]
    pub authorize_human_intake: bool,
    pub authorization: HumanAuthorizationEvidence,
    pub expected: HumanIntakeFence,
    pub work_id: String,
    /// Literal UTF-8 request prose, not a path to be dereferenced.
    pub request: String,
    pub request_sha256: String,
    pub require_acceptance: bool,
    pub allocation: WorkAllocation,
    pub new_max_total_steps: u64,
    pub new_max_works: u64,
    pub conservative_tokens_used: u64,
    pub per_call_reserve_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanIntakeReceipt {
    pub version: u32,
    pub request_sha256: String,
    pub request: HumanIntakeRequest,
    pub created_at_unix: i64,
    pub target_runtime: s::ObjectRef,
    pub target_launch: s::ObjectRef,
    pub need_sha256: String,
    pub origin: w::Origin,
    pub authorization_basis: String,
    pub capacity_reserved: bool,
    pub inference_performed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct HumanIntakeOutcome {
    pub receipt: HumanIntakeReceipt,
    pub replayed: bool,
}

fn receipt_id(id: &str) -> String {
    format!("bounded-human-intake:{id}")
}
fn bounded_text(value: &str, maximum: usize) -> Result<()> {
    text(value, maximum)?;
    require(
        !value.trim().is_empty(),
        "nonblank human-intake text required",
    )
}
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn check_ref(reference: &intake_delegation::IntakeReference, id: &str) -> Result<()> {
    require(
        reference.id == id && reference.revision > 0,
        "invalid human-intake fence reference",
    )?;
    increment(reference.revision)?;
    require(valid_sha256(&reference.sha256), "invalid fence SHA256")
}
impl HumanIntakeRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        identity(operator)?;
        identity(&self.id)?;
        identity(&self.operator)?;
        identity(&self.work_id)?;
        require(
            self.operator == operator,
            "human-intake operator or receipt identity mismatch",
        )?;
        bounded_text(&self.purpose, 4096)?;
        bounded_text(&self.request, 32_768)?;
        require(
            s::sha256_bytes(self.request.as_bytes()) == self.request_sha256,
            "exact human-intake request-body hash required",
        )?;
        let auth = &self.authorization;
        bounded_text(&auth.reference, 4096)?;
        bounded_text(&auth.statement, 4096)?;
        require(
            s::sha256_bytes(auth.statement.as_bytes()) == auth.statement_sha256,
            "authorization statement hash mismatch",
        )?;
        identity(&auth.model)?;
        identity(&auth.effort)?;
        identity(&auth.destination)?;
        check_ref(&self.expected.runtime, STATE_ID)?;
        check_ref(&self.expected.launch, ID)?;
        self.allocation.validate()?;
        let expected = &self.expected;
        let needed_works = increment(expected.work_count)?;
        require(
            self.work_id == format!("work-{needed_works:08}"),
            "human-intake work identity must be the next saved identity",
        )?;
        admission_extension::validate_token_attestation(
            self.conservative_tokens_used,
            auth.cumulative_token_ceiling,
            self.per_call_reserve_tokens,
        )?;
        Ok(())
    }
}

fn fence(
    runtime: &s::StoredObject,
    launch: &s::StoredObject,
    saved: &Saved,
) -> Result<HumanIntakeFence> {
    Ok(HumanIntakeFence {
        runtime: intake_delegation::IntakeReference::of(runtime)?,
        launch: intake_delegation::IntakeReference::of(launch)?,
        steps: saved.steps,
        max_total_steps: saved.setup.max_total_steps,
        work_count: saved
            .works
            .len()
            .try_into()
            .map_err(|_| invalid("work count overflow"))?,
        max_works: saved.setup.max_works,
        max_steps: saved.setup.max_steps,
    })
}

/// Metered snapshot only; no allocation or capacity reservation.
pub fn human_intake_snapshot(app: &mut Runtime, operator: &str) -> Result<Value> {
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    Ok(
        json!({"operator":operator,"expected":fence(&runtime,&launch,&saved)?,
        "configured_work_allocation":WorkAllocation::configured(&state.config),
        "application_quotas_enforced":false,
        "navigation_per_call":state.config.navigation_per_call,
        "capacity_reserved":false,"snapshot_atomic":false}),
    )
}

fn validate_receipt(receipt: &HumanIntakeReceipt) -> Result<()> {
    let request = &receipt.request;
    request.validate(&request.operator)?;
    require(
        receipt.version == 1
            && receipt.created_at_unix >= 0
            && receipt.request_sha256 == digest(request)?
            && receipt.origin == w::Origin::Human(request.operator.clone())
            && receipt.authorization_basis == AUTH_BASIS
            && !receipt.capacity_reserved
            && !receipt.inference_performed
            && receipt.target_runtime.id == STATE_ID
            && receipt.target_runtime.revision == increment(request.expected.runtime.revision)?
            && receipt.target_launch.id == ID
            && receipt.target_launch.revision == increment(request.expected.launch.revision)?
            && valid_sha256(&receipt.target_runtime.sha256)
            && valid_sha256(&receipt.target_launch.sha256)
            && valid_sha256(&receipt.need_sha256),
        "invalid human-intake receipt",
    )
}


fn replay(
    app: &mut Runtime,
    request: &HumanIntakeRequest,
    hash: &str,
) -> Result<Option<HumanIntakeReceipt>> {
    let id = receipt_id(&request.id);
    let Some(object) = app.store.read(&id)? else {
        return Ok(None);
    };
    let (_, state) = app.load()?;
    let (_, saved) = load(app)?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    require(
        object.kind == RECEIPT_KIND && object.revision == 1 && object.id == id,
        "invalid human-intake receipt object",
    )?;
    let receipt: HumanIntakeReceipt = serde_json::from_value(object.body)?;
    validate_receipt(&receipt)?;
    if receipt.request_sha256 != hash || receipt.request != *request {
        return Err(s::StoreError::Conflict(id).into());
    }
    Ok(Some(receipt))
}

struct Planned {
    receipt: HumanIntakeReceipt,
    changes: Vec<s::Change>,
    minimum: s::Resources,
}
fn plan(
    app: &mut Runtime,
    request: &HumanIntakeRequest,
    hash: String,
    now: i64,
) -> Result<Planned> {
    let (runtime_head, state) = app.load()?;
    let (launch_head, mut saved) = load(app)?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    require(
        fence(&runtime_head, &launch_head, &saved)? == request.expected,
        "human-intake heads or counters changed",
    )?;
    require(
        app.invocation_leases_quiescent(&state)?,
        "unfinished invocation blocks human intake",
    )?;
    app.store.require_quiescent_invocations()?;
    let provider = saved.execution_provider()?;
    let auth = &request.authorization;
    require(
        auth.model == provider.model
            && auth.effort == provider.effort
            && auth.destination == saved.setup.destination,
        "human-intake authorization provider or destination mismatch",
    )?;
    request.allocation.require_within_config(&state.config)?;
    require(
        !saved.works.contains_key(&request.work_id)
            && !state.work_meta.contains_key(&request.work_id)
            && !state
                .board
                .works()
                .contains_key(&w::WorkId(request.work_id.clone())),
        "human-intake work already exists",
    )?;
    let input = human_need_input(
        &saved,
        &request.work_id,
        &request.request,
        request.require_acceptance,
    )?;
    let exposure = Exposure::new(
        Vec::new(),
        vec![scoped_policy(
            "work-need",
            &input.id,
            &digest(&input)?,
            current_policy(&state, &input.policy_id)?,
        )],
    );
    let mut next = intake::construct_host_need(
        &state,
        &saved.operator,
        &input,
        request.allocation,
        w::Origin::Human(saved.operator.clone()),
        exposure,
        None,
        None,
    )?;
    let previous_episode_binding = intake_delegation::episode_launch_binding(&saved)?;
    saved.works.insert(request.work_id.clone(), None);
    saved.setup.max_total_steps = request.new_max_total_steps;
    saved.setup.max_works = request.new_max_works;
    let next_episode_binding = intake_delegation::episode_launch_binding(&saved)?;
    let mut carried_episode_bindings = Vec::new();
    if previous_episode_binding != next_episode_binding {
        for (id, episode) in &mut next.intake_episodes {
            // Carry the current binding across this recorded metadata change.
            // Already-stale bindings remain stale; no grant, charge, opportunity,
            // hold, expiry or revocation is changed or newly authorized here.
            if episode.launch_binding_sha256 == previous_episode_binding {
                episode.launch_binding_sha256 = next_episode_binding.clone();
                carried_episode_bindings.push(id.clone());
            }
        }
    }
    next.operator_events
        .push(json!({"kind":"bounded_human_intake","id":request.id,
        "operator":request.operator,"authorization_basis":AUTH_BASIS,"request_sha256":hash,
        "authorization":request.authorization,"work":request.work_id,
        "episode_binding_transition":{"from":previous_episode_binding,"to":next_episode_binding,
            "carried_episodes":carried_episode_bindings}}));
    let runtime = state_object(&runtime_head, &next)?;
    let mut launch = launch_head.clone();
    launch.revision = increment(launch.revision)?;
    launch.body["works"] = serde_json::to_value(&saved.works)?;
    launch.body["setup"]["max_total_steps"] = json!(request.new_max_total_steps);
    launch.body["setup"]["max_works"] = json!(request.new_max_works);
    let receipt = HumanIntakeReceipt {
        version: 1,
        request_sha256: hash,
        request: request.clone(),
        created_at_unix: now,
        target_runtime: runtime.exact_ref()?,
        target_launch: launch.exact_ref()?,
        need_sha256: digest(&input)?,
        origin: w::Origin::Human(saved.operator.clone()),
        authorization_basis: AUTH_BASIS.into(),
        capacity_reserved: false,
        inference_performed: false,
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
            depends_on: vec![
                receipt.target_runtime.clone(),
                receipt.target_launch.clone(),
            ],
        },
    ];
    Ok(Planned {
        receipt,
        changes,
        minimum: s::Resources {
            attempts: request.allocation.attempts,
            input_bytes: 1,
            output_bytes: 1,
            navigation_bytes: 1,
            retained_bytes: 1,
        },
    })
}

/// Preview checks the same candidate transaction as apply, then rolls it back.
/// Normal owner recovery and read charges may occur; no future capacity is reserved.
pub fn preview_human_intake(
    app: &mut Runtime,
    request: &HumanIntakeRequest,
    now: i64,
) -> Result<HumanIntakeReceipt> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative human-intake time")?;
    let hash = digest(request)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(receipt);
    }
    let _owner = app.store.acquire_execution_owner()?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(receipt);
    }
    let planned = plan(app, request, hash, now)?;
    // This shared Store primitive checks ordinary admission, terminal invocation
    // state and root capacity only; it creates no delegation records or rights.
    app.store
        .preview_intake_delegation_batch(planned.changes, planned.minimum)?;
    Ok(planned.receipt)
}

/// One ordinary guarded CAS creates a fresh human work, exact logical allocation,
/// minimal global ceilings and immutable receipt. Prior work is never reopened.
pub fn admit_human_need(
    app: &mut Runtime,
    request: &HumanIntakeRequest,
    now: i64,
) -> Result<HumanIntakeOutcome> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative human-intake time")?;
    let hash = digest(request)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(HumanIntakeOutcome {
            receipt,
            replayed: true,
        });
    }
    let _owner = app.store.acquire_execution_owner()?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(HumanIntakeOutcome {
            receipt,
            replayed: true,
        });
    }
    let planned = plan(app, request, hash, now)?;
    app.store
        .commit_intake_delegation_batch(planned.changes, planned.minimum)?;
    Ok(HumanIntakeOutcome {
        receipt: planned.receipt,
        replayed: false,
    })
}
