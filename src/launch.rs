//! Explicit foreground orchestration over the existing Runtime and Store.
use super::*;
#[path = "launch/results.rs"]
mod results;
/// Operator-only Stage 1 launcher protocol; not a persona tool.
#[path = "launch/session.rs"]
pub mod session;
pub use results::Output;
#[path = "launch/development.rs"]
mod development;
pub use development::{development, development_at};
#[path = "launch/feedback.rs"]
mod feedback;
pub use feedback::{feedback, FeedbackInput};
#[path = "launch/questions.rs"]
mod question_commands;
pub use question_commands::{answer_question, shared_questions, QuestionAnswerInput};
#[cfg(unix)]
#[path = "launch/native.rs"]
mod native;
#[cfg(unix)]
pub use native::{native_continue_delegation, native_run, NativeInput};
#[path = "launch/migration.rs"]
mod migration;
pub use migration::{
    migrate_provider, migration_snapshot, preview_provider_migration, MigrationSnapshot,
    PersonaBinding, ProviderMigrationOutcome, ProviderMigrationReceipt, ProviderMigrationRequest,
};
#[path = "launch/admission_extension.rs"]
mod admission_extension;
pub use admission_extension::{
    extend_launch_admission, launch_admission_snapshot, preview_launch_admission_extension,
    LaunchAdmissionExtensionOutcome, LaunchAdmissionExtensionReceipt,
    LaunchAdmissionExtensionRequest, LaunchAdmissionSnapshot,
};

#[path = "launch/intake_delegation.rs"]
pub mod intake_delegation;
pub use intake_delegation::{
    admit_need_proposal, grant_intake_delegation, intake_delegation_snapshot,
    preview_admit_need_proposal, preview_intake_delegation, preview_intake_delegation_revocation,
    revoke_intake_delegation, AdmitNeedProposalOutcome, AdmitNeedProposalReceipt,
    AdmitNeedProposalRequest, IntakeDelegationRequest, IntakeDelegationRevocationRequest,
};

#[path = "launch/human_intake.rs"]
mod human_intake;
pub use human_intake::{
    admit_human_need, human_intake_snapshot, preview_human_intake, HumanIntakeRequest,
};

#[path = "launch/work_resource_grant.rs"]
mod work_resource_grant;
#[path = "launch/repair_allocation.rs"]
mod repair_allocation;
pub use repair_allocation::{
    allocate_work_repair_at, work_repair_snapshot_at, RepairAllocationRequest,
};
#[path = "launch/store_capacity.rs"]
pub mod store_capacity;
#[path = "launch/context_capacity.rs"]
pub mod context_capacity;
#[path = "launch/transport_capacity.rs"]
pub mod transport_capacity;
pub use work_resource_grant::{
    grant_work_resources, preview_work_resource_grant, work_resource_grant_snapshot,
    WorkResourceGrantFence, WorkResourceGrantOutcome, WorkResourceGrantReceipt,
    WorkResourceGrantRequest, WorkResourceIncrease,
};

#[path = "launch/bootstrap.rs"]
pub mod bootstrap;

const ID: &str = "runtime-launch-v1";
const POLICY: &str = "launch-local";
/// Attributed starting material, not an accepted character or invented experience.
/// The initial persona invocation authors its own character and fragments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaSeed {
    pub label: String,
    pub text: String,
    pub provenance: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub cohort: usize,
    pub owner: usize,
    pub destination: String,
    pub provider: DirectProviderSettings,
    pub working_directory: PathBuf,
    pub share_cohort: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub http_origins: BTreeSet<String>,
    pub max_steps: usize,
    pub max_considerations: usize,
    pub max_work_attempts: u64,
    pub max_total_steps: u64,
    pub max_works: u64,
}
const PROVIDER_MIGRATION: &str = "saved provider is incompatible with direct HTTP execution; explicitly migrate provider mode and authorization while preserving history, counters, grants and unresolved attempts; existing records remain readable through open/result";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSetup {
    cohort: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    persona_seeds: BTreeMap<String, PersonaSeed>,
    owner: usize,
    destination: String,
    provider: serde_json::Value,
    working_directory: PathBuf,
    share_cohort: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    http_origins: BTreeSet<String>,
    max_steps: usize,
    max_considerations: usize,
    max_work_attempts: u64,
    max_total_steps: u64,
    max_works: u64,
}
impl SavedSetup {
    fn execution_provider(&self) -> Result<ResearchSettings> {
        // CLI identity remains forbidden even if a settings deserializer accepts it.
        if ["executable", "cli_version", "temporary_directory"]
            .iter()
            .any(|key| self.provider.get(*key).is_some())
        {
            return Err(invalid(PROVIDER_MIGRATION));
        }
        let provider: DirectProviderSettings = serde_json::from_value(self.provider.clone())
            .map_err(|_| invalid(PROVIDER_MIGRATION))?;
        provider
            .validate()
            .map_err(|_| invalid(PROVIDER_MIGRATION))?;
        Ok(provider)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    format: u32,
    operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    research_opt_in: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direct_provider_opt_in: Option<bool>,
    setup: SavedSetup,
    working_directory: PathBuf,
    works: BTreeMap<String, Option<String>>,
    steps: u64,
}
impl Saved {
    fn execution_provider(&self) -> Result<DirectProviderSettings> {
        if self.format != 2 || self.direct_provider_opt_in != Some(true) {
            return Err(invalid(PROVIDER_MIGRATION));
        }
        self.setup.execution_provider()
    }
}
fn people(count: usize) -> Vec<String> {
    (1..=count).map(|n| format!("persona-{n:04}")).collect()
}
fn load(app: &mut Runtime) -> Result<(s::StoredObject, Saved)> {
    let object = app
        .store
        .read(ID)?
        .ok_or_else(|| invalid("run setup first"))?;
    require(object.kind == "foreground-launch", "launch object kind")?;
    let saved: Saved = serde_json::from_value(object.body.clone())?;
    require(
        (saved.format == 1
            && saved.research_opt_in == Some(true)
            && saved.direct_provider_opt_in.is_none())
            || (saved.format == 2
                && saved.direct_provider_opt_in == Some(true)
                && saved.research_opt_in.is_none()),
        "saved launch format and authorization metadata are invalid",
    )?;
    Ok((object, saved))
}
fn save(app: &mut Runtime, old: Option<&s::StoredObject>, saved: &Saved) -> Result<()> {
    let (head, state) = app.load()?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks launch writes",
    )?;
    app.store.compare_and_swap_batch(vec![s::Change {
        expected_revision: old.map(|o| o.revision),
        object: s::StoredObject {
            id: ID.into(),
            revision: old.map_or(Ok(1), |o| increment(o.revision))?,
            kind: "foreground-launch".into(),
            body: serde_json::to_value(saved)?,
        },
        depends_on: vec![head.exact_ref()?],
    }])?;
    Ok(())
}
pub fn state_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(std::env::var_os("HOME").ok_or_else(|| invalid("HOME is absent"))?)
            .join(".local/state"),
    };
    require(base.is_absolute(), "default state base must be absolute")?;
    Ok(base.join("ai-personas"))
}
fn private_tree(path: &Path) -> Result<PathBuf> {
    if let Err(error) = fs::symlink_metadata(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error.into());
        }
        let parent = path
            .parent()
            .ok_or_else(|| invalid("state parent absent"))?;
        if !parent.exists() {
            private_tree(parent)?;
        }
    }
    secure_state_root(path, true)
}
fn seed_for(saved: &Saved, provider: &ResearchSettings, id: &str) -> Seed {
    let mut audience: BTreeSet<_> = people(saved.setup.cohort).into_iter().collect();
    audience.insert(saved.operator.clone());
    let supplied = saved.setup.persona_seeds.get(id);
    Seed {
        id: id.into(),
        label: supplied.map_or_else(|| id.into(), |seed| seed.label.clone()),
        text: supplied.map_or_else(
            || format!("Unaccepted creation material for {id}; no prior experience asserted."),
            |seed| seed.text.clone(),
        ),
        provenance: supplied.map_or_else(
            || "operator-supplied generic seed".into(),
            |seed| seed.provenance.clone(),
        ),
        requested_model: provider.model.clone(),
        requested_effort: provider.effort.clone(),
        model_confirmed: true,
        destination: saved.setup.destination.clone(),
        audience: audience.clone(),
        grants: Grants {
            fragments: true,
            assets: true,
            character: true,
            work_commands: true,
            write_policies: BTreeSet::from([POLICY.into()]),
            communicate_with: audience,
        },
    }
}
fn ensure_cohort(app: &mut Runtime, saved: &Saved) -> Result<()> {
    let provider = saved.execution_provider()?;
    let state = app.snapshot()?;
    for id in people(saved.setup.cohort) {
        let seed = seed_for(saved, &provider, &id);
        if let Some(existing) = state.personas.get(&id) {
            require(
                serde_json::to_value(&existing.seed)? == serde_json::to_value(&seed)?,
                "saved cohort binding changed",
            )?;
        } else {
            app.add_persona_seed(&saved.operator, seed)?;
        }
    }
    Ok(())
}
fn setup_parts(operator: &str, settings: Setup, opt_in: bool) -> Result<(Config, Saved)> {
    identity(operator)?;
    require(
        opt_in && settings.share_cohort,
        "explicit direct-provider access and cohort sharing consent required",
    )?;
    require(
        (1..=8).contains(&settings.cohort) && (1..=settings.cohort).contains(&settings.owner),
        "cohort must be 1..=8 and owner must name a cohort position",
    )?;
    require(
        (1..=128).contains(&settings.max_steps)
            && (1..=128).contains(&settings.max_considerations)
            && (1..=128).contains(&settings.max_work_attempts)
            && settings.max_total_steps > 0
            && settings.max_total_steps
                <= storage_limits().total.attempts - storage_limits().protected_closeout.attempts
            && (1..=128).contains(&settings.max_works),
        "invalid foreground bounds",
    )?;
    let ids = people(settings.cohort);
    require(
        !ids.iter().any(|id| id == operator),
        "requester collides with a cohort identity",
    )?;
    let mut audience: BTreeSet<_> = ids.iter().cloned().collect();
    audience.insert(operator.into());
    require(settings.http_origins.len() <= 8
        && settings.http_origins.iter().all(|origin| crate::http_fetch::valid_origin(origin)),
        "setup HTTPS origins must be canonical; at most eight")?;
    let mut processing_destinations = settings.http_origins.clone();
    processing_destinations.insert(settings.destination.clone());
    let policy = f::Policy {
        id: POLICY.into(),
        revision: "1".into(),
        readers: ids.into_iter().collect(),
        processing_destinations,
        audience,
        derivative_use: f::DerivativeUse::AllowDerivatives,
        expires_at_unix: None,
        revoked_at_unix: None,
    };
    let mut config = Config::local(f::PolicySnapshot {
        active_policies: vec![policy],
        revoked_policies: BTreeSet::new(),
    });
    config.local_recipients = BTreeSet::from([operator.into()]);
    config.allow_direct_provider = true;
    config.max_work_attempts = settings.max_work_attempts;
    settings.provider.validate()?;
    require(
        settings.provider.limits.max_prompt_bytes <= config.context_bytes
            && settings.provider.limits.max_response_bytes <= config.response_bytes
            && settings.provider.limits.max_stream_bytes as u64
                <= config.transport_output_reserve_bytes,
        "provider output exceeds saved runtime bounds",
    )?;
    let working = fs::canonicalize(&settings.working_directory)?;
    require(working.is_dir(), "working directory must exist")?;
    let settings: SavedSetup = serde_json::from_value(serde_json::to_value(settings)?)?;
    settings.execution_provider()?;
    let saved = Saved {
        format: 2,
        operator: operator.into(),
        direct_provider_opt_in: Some(opt_in),
        research_opt_in: None,
        setup: settings,
        working_directory: working,
        works: BTreeMap::new(),
        steps: 0,
    };
    Ok((config, saved))
}

pub fn setup(root: &Path, operator: &str, settings: Setup, opt_in: bool) -> Result<Runtime> {
    setup_with_seeds(root, operator, settings, opt_in, BTreeMap::new())
}

/// Select creation material only while creating a fresh cohort. Continuing
/// personas retain their exact accepted history and cannot be reseeded here.
pub fn setup_with_seeds(
    root: &Path,
    operator: &str,
    settings: Setup,
    opt_in: bool,
    seeds: BTreeMap<String, PersonaSeed>,
) -> Result<Runtime> {
    let (config, mut saved) = setup_parts(operator, settings, opt_in)?;
    let ids = people(saved.setup.cohort);
    require(
        seeds.len() <= ids.len() && seeds.keys().all(|id| ids.contains(id)),
        "persona seed must name a member of this cohort",
    )?;
    saved.setup.persona_seeds = seeds;
    let provider = saved.execution_provider()?;
    for id in &ids {
        validate_seed(&seed_for(&saved, &provider, id))?;
    }
    let root = private_tree(root)?;
    require(
        !saved.working_directory.starts_with(&root) && !root.starts_with(&saved.working_directory),
        "provider working directory and private state must be separate",
    )?;
    let mut app = Runtime::create(&root, config, operator)?;
    save(&mut app, None, &saved)?;
    ensure_cohort(&mut app, &saved)?;
    Ok(app)
}
pub fn open(root: &Path, operator: Option<&str>) -> Result<Runtime> {
    let mut app = Runtime::open(root)?;
    let (_, saved) = load(&mut app)?;
    require(
        operator.is_none_or(|value| value == saved.operator),
        "saved operator mismatch",
    )?;
    Ok(app)
}
fn human_need_input(
    saved: &Saved,
    id: &str,
    request: &str,
    require_acceptance: bool,
) -> Result<NeedInput> {
    let ids = people(saved.setup.cohort);
    let boundaries = "Local permitted sources and local delivery within saved bounds.";
    let authorization = format!(
        "Requested by {}; authorizes voluntary persona arrangement formation and later amendment within the current scope and mandate under exact affected-party assent, limited to the eligible saved cohort and existing grants and limits; grants no additional resources or effects, no further delegation, and no authority to accept on behalf of others. Result decision rights confer no power to amend this formation envelope.",
        saved.operator
    );
    let delegation = w::FormationDelegation {
        revision: 1,
        issuer: w::Origin::Human(saved.operator.clone()),
        authorization: work_ref(id, "authority", &authorization)?,
        need: w::formation_need_ref(
            &w::WorkId(id.into()),
            &w::Need {
                origin: w::Origin::Human(saved.operator.clone()),
                request: request.into(),
                supplied: BTreeSet::new(),
                boundaries: work_ref(id, "boundaries", boundaries)?,
            },
        ),
        mandate_revision: 1,
        allocable_outcomes: BTreeSet::from([w::OutcomeId("result".into())]),
        allocable_review_roles: BTreeSet::new(),
        assent: w::ArrangementAssentPolicy::AffectedPartyAssentV1,
    };
    Ok(NeedInput {
        id: id.into(),
        origin: NeedOrigin::Human,
        request: request.into(),
        boundaries: boundaries.into(),
        authorization,
        assumptions: "No external effects, independent assurance or requester acceptance.".into(),
        terms: "An exact local result addressing the recorded request.".into(),
        policy_id: POLICY.into(),
        outcomes: BTreeMap::from([(
            "result".into(),
            OutcomeSpec {
                description: request.into(),
                criterion: request.into(),
                required_reviews: BTreeSet::new(),
            },
        )]),
        review_roles: BTreeMap::new(),
        decision_rule: w::DecisionRule::PersonaArranged {
            eligible: ids.iter().cloned().map(w::PersonaId).collect(),
            delegation,
        },
        delivery_to: BTreeSet::from([saved.operator.clone()]),
        stakeholders: if require_acceptance {
            BTreeSet::from([saved.operator.clone()])
        } else {
            BTreeSet::new()
        },
        invitations: ids
            .iter()
            .map(|persona| Invitation {
                id: format!("{id}-{persona}"),
                to: persona.clone(),
                terms: w::CommitmentTerms {
                    mandate_revision: 1,
                    exact_terms: "Invitation to consider or contribute voluntarily within the mandate; responsibilities and decision arrangements are persona-proposed and require each affected persona's exact acceptance; no owner or reviewer is assigned.".into(),
                    outcomes: BTreeSet::new(),
                    review_roles: BTreeSet::new(),
                },
            })
            .collect(),
    })
}

pub fn start(app: &mut Runtime, request: &str) -> Result<String> {
    start_with_acceptance(app, request, false)
}
/// Human acceptance is a mandate requirement only when explicitly requested.
pub fn start_with_acceptance(
    app: &mut Runtime,
    request: &str,
    require_acceptance: bool,
) -> Result<String> {
    text(request, 32_768)?;
    let owner = app.store.acquire_execution_owner()?;
    start_with_acceptance_owned(app, request, require_acceptance, &owner)
}

// Native execution already owns this Store. Retain that guard through the
// ordinary transaction instead of trying to acquire a second owner.
fn start_with_acceptance_owned(
    app: &mut Runtime,
    request: &str,
    require_acceptance: bool,
    _owner: &s::ExecutionOwner,
) -> Result<String> {
    start_owned_keyed(app, request, require_acceptance, _owner, None)
}

fn start_owned_keyed(
    app: &mut Runtime,
    request: &str,
    require_acceptance: bool,
    _owner: &s::ExecutionOwner,
    key: Option<&session::NeedRequest>,
) -> Result<String> {
    text(request, 32_768)?;
    let (old, mut saved) = load(app)?;
    if let Some(key) = key {
        require(
            key.operator == saved.operator
                && key.need == request
                && key.require_acceptance == require_acceptance,
            "keyed intake request binding changed",
        )?;
    }
    let (runtime_head, state) = app.load()?;
    admission_extension::authorize(&state, &saved, &saved.operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks launch intake",
    )?;
    app.store.require_terminal_invocations()?;
    let work_count =
        u64::try_from(saved.works.len()).map_err(|_| invalid("saved work count overflow"))?;
    require(
        work_count < saved.setup.max_works,
        "work identity limit reached",
    )?;
    let id = format!("work-{:08}", increment(work_count)?);
    require(
        !saved.works.contains_key(&id)
            && !state.board.works().contains_key(&w::WorkId(id.clone()))
            && !state.work_meta.contains_key(&id),
        "launch work identity already exists; inspect retained state without selecting a replacement identity",
    )?;
    saved.works.insert(id.clone(), None);
    let input = human_need_input(&saved, &id, request, require_acceptance)?;
    let allocation = WorkAllocation::configured(&state.config);
    let policy = current_policy(&state, &input.policy_id)?;
    let exposure = Exposure {
        sources: Vec::new(),
        policies: vec![scoped_policy(
            "work-need",
            &input.id,
            &digest(&input)?,
            policy,
        )],
    };
    let next = intake::construct_host_need(
        &state,
        &saved.operator,
        &input,
        allocation,
        w::Origin::Human(saved.operator.clone()),
        exposure,
        None,
    )?;
    let runtime = state_object(&runtime_head, &next)?;
    // Preserve the saved JSON body, including omitted historical defaults.
    // Only the existing launch registry advances in this intake transaction.
    let mut launch = old.clone();
    launch.revision = increment(old.revision)?;
    launch.body["works"] = serde_json::to_value(&saved.works)?;
    let target_runtime = runtime.exact_ref()?;
    let target_launch = launch.exact_ref()?;
    let created_at_unix = unix_now()?;
    require(created_at_unix >= 0, "negative intake time")?;
    let receipt = s::StoredObject {
        id: format!("launch-human-intake:{id}"),
        revision: 1,
        kind: "launch-human-need-intake".into(),
        body: json!({
            "version": 1,
            "origin": w::Origin::Human(saved.operator.clone()),
            "work": &id,
            "need_sha256": digest(&input)?,
            "allocation": allocation,
            "allocation_basis": "existing_configured_host_creation_allowance",
            "capacity_reserved": false,
            "created_at_unix": created_at_unix,
            "prior_runtime": runtime_head.exact_ref()?,
            "target_runtime": &target_runtime,
            "prior_launch": old.exact_ref()?,
            "target_launch": &target_launch,
            "invitations_accepted": false,
            "inference_performed": false
        }),
    };
    let keyed = key
        .map(|key| session::keyed_receipt(key, &id, &receipt))
        .transpose()?;
    let mut changes = vec![
        s::Change {
            expected_revision: Some(runtime_head.revision),
            object: runtime,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: Some(old.revision),
            object: launch,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: None,
            object: receipt,
            depends_on: vec![target_runtime, target_launch],
        },
    ];
    if let Some(object) = keyed {
        changes.push(s::Change {
            expected_revision: None,
            object,
            depends_on: Vec::new(),
        });
    }
    app.store
        .compare_and_swap_batch_requiring_terminal_invocations(changes)?;
    Ok(id)
}
type Pick = (usize, Option<String>);
fn select(app: &mut Runtime, work: &str, limit: usize, now: i64) -> Result<Pick> {
    scheduler_bound(limit)?;
    let time = u64::try_from(now).map_err(|_| invalid("negative scheduling time"))?;
    let (head, state) = app.load()?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation requires explicit recovery",
    )?;
    let (old, mut cursor) = app.load_scheduler()?;
    let page = state
        .board
        .consideration_page(cursor.after.as_ref(), limit)?;
    let mut selected = None;
    cursor.last.clear();
    for (wid, actor) in page {
        let opportunity = runtime_opportunity(&state, &wid, &actor, time);
        let mut entry = scheduler_entry(&state, wid.clone(), actor.clone(), opportunity, now)?;
        if wid.0 == work {
            let permit = continuation_permit(&state, &actor.0, work);
            if permit
                && matches!(state.board.work(&wid)?.phase, w::Phase::Open)
                && state
                    .work_meta
                    .get(work)
                    .is_some_and(|m| m.attempts_left > 0)
            {
                entry.disposition = SchedulerDisposition::Queued;
            }
            if matches!(entry.disposition, SchedulerDisposition::Queued) {
                selected = Some(actor.0.clone());
            }
        }
        cursor.after = Some((wid, actor));
        cursor.considered = increment(cursor.considered)?;
        cursor.last.push(entry);
        if selected.is_some() {
            break;
        }
    }
    let considered = cursor.last.len();
    if considered > 0 {
        let mut change = scheduler_change(old.as_ref(), &cursor)?;
        change.depends_on.push(head.exact_ref()?);
        app.store.compare_and_swap_batch(vec![change])?;
    }
    Ok((considered, selected))
}
fn reserve(
    app: &mut Runtime,
    work: &str,
    persona: &str,
    prospective_intake: bool,
) -> Result<(PrepareRequest, Saved)> {
    let (old, mut saved) = load(app)?;
    saved.execution_provider()?;
    require(
        saved.steps < saved.setup.max_total_steps,
        "lifetime step limit reached",
    )?;
    saved.steps = increment(saved.steps)?;
    let id = format!("call-{:08}", saved.steps);
    *saved
        .works
        .get_mut(work)
        .ok_or_else(|| invalid("unknown launch work"))? = Some(id.clone());
    // An episode preparation commits its launch step together with its durable
    // logical charge. Do not also spend the launch counter here. The projected
    // Saved value keeps the ordinary driver's provider binding unchanged.
    // A first explicit selection is not yet bound in State. Its preparation
    // must own this same debit, just as an already bound episode does.
    if !prospective_intake && !need_proposals::bound(&app.snapshot()?, work) {
        save(app, Some(&old), &saved)?;
    }
    Ok((
        PrepareRequest {
            id,
            persona: persona.into(),
            work: work.into(),
            profile: Profile::Direct,
            mode: Mode::Ordinary,
        },
        saved,
    ))
}
trait Driver {
    // Trusted driver intent only. Serialized persona output cannot select this.
    fn prospective_intake(&self) -> bool {
        false
    }
    fn before_step(&mut self, _app: &mut Runtime) -> Result<()> {
        Ok(())
    }
    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport>;
}
struct Live;
impl Driver for Live {
    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        app.step(
            request,
            saved.execution_provider()?,
            saved.working_directory.clone(),
            now,
        )
        .await
    }
}
pub async fn run(app: &mut Runtime, work: &str) -> Result<Output> {
    run_with(app, work, &mut Live).await
}
/// Explicitly continue a bounded saved foreground session through existing
/// eligible routes. Delivery status grants no semantic resume, new resources,
/// invented consent or recovery of a blocked invocation.
pub async fn continue_run(app: &mut Runtime, work: &str) -> Result<Output> {
    run_with_options(app, work, &mut Live, true).await
}
async fn run_with<D: Driver>(app: &mut Runtime, work: &str, driver: &mut D) -> Result<Output> {
    run_with_options(app, work, driver, false).await
}
async fn run_with_options<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    _explicit_continue: bool,
) -> Result<Output> {
    let (_, saved) = load(app)?;
    saved.execution_provider()?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    let mut remaining = saved.setup.max_considerations;
    let mut stop = "step_limit";
    for _ in 0..saved.setup.max_steps {
        let mut view = result(app, work)?;
        // Scheduling uses trusted current state, never a permission-filtered
        // historical result view or an inferred disclosure principal.
        let state = app.snapshot()?;
        let operational = state
            .board
            .work(&w::WorkId(work.into()))
            .is_ok_and(|current| matches!(current.phase, w::Phase::Open));
        if !operational {
            view.stop = Some("no_eligible_route".into());
            return Ok(view);
        }
        let attempt = async {
            driver.before_step(app)?;
            if !handover(app, work)? {
                return Ok("character_fence");
            }
            let (count, selected) = select(app, work, remaining, unix_now()?)?;
            remaining -= count;
            let Some(persona) = selected else {
                return Ok("no_eligible_route");
            };
            driver.before_step(app)?;
            let (request, current) = reserve(app, work, &persona, driver.prospective_intake())?;
            let report = driver.step(app, request, &current, unix_now()?).await?;
            if report.status == AdoptionStatus::Held {
                return Ok("held");
            }
            if report.status == AdoptionStatus::CharacterOnly && !handover(app, work)? {
                return Ok("character_fence");
            }
            Ok::<_, RuntimeError>("continue")
        }
        .await;
        match attempt {
            Ok("continue") if remaining > 0 => (),
            Ok("continue") => {
                stop = "consideration_bound";
                break;
            }
            Ok(reason) => {
                stop = reason;
                break;
            }
            Err(error) => {
                stop = match error {
                    RuntimeError::Files(f::FilesError::MandatoryOverflow { required, limit }) => {
                        // Only trusted numeric assembly measurements enter stderr;
                        // never echo provider text, private paths or parser errors.
                        eprintln!("Mandatory context requires {required} bytes; configured limit is {limit}. Optional-selection repair cannot remove mandatory context.");
                        "mandatory_context_overflow"
                    }
                    RuntimeError::SelectionOverflow { .. } => "selection_overflow",
                    RuntimeError::Provider(_) => "provider_preflight_or_transport_failed",
                    RuntimeError::Protocol(_) => "invalid_response",
                    RuntimeError::Held(_) => "held_or_unresolved_invocation",
                    RuntimeError::Store(_) => "store_or_resource_limit",
                    _ => "blocked_or_unresolved_invocation",
                };
                break;
            }
        }
    }
    let mut view = result(app, work)?;
    view.stop = Some(stop.into());
    Ok(view)
}
pub fn result(app: &mut Runtime, work: &str) -> Result<Output> {
    let (_, saved) = load(app)?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    results::project(app, work, &saved)
}
fn handover(app: &mut Runtime, work: &str) -> Result<bool> {
    let (_, saved) = load(app)?;
    let Some(id) = saved.works.get(work).and_then(Option::as_deref) else {
        return Ok(true);
    };
    let inv = match app.invocation(id) {
        Ok(inv) => inv,
        Err(RuntimeError::Store(s::StoreError::Missing(_))) => return Ok(true),
        Err(error) => return Err(error),
    };
    if inv.state != s::InvocationState::Adopted {
        return Ok(true);
    }
    let accepted = app.adopted(id)?;
    if accepted.report.status != AdoptionStatus::CharacterOnly {
        return Ok(true);
    }
    let (_, state) = app.load()?;
    let persona = &state.personas[&accepted.actor];
    let focus = &persona.focus[work];
    // A retained ordinary route needs no host resume. A future Wait may remain
    // ineligible; ordinary selection decides when it can admit a fresh request.
    if focus.permit || focus.ordinary_continuation || handed_over(&state, &accepted.actor) {
        return Ok(true);
    }
    if decode_prepared(&inv)?.character_revision.is_some() {
        return Ok(false);
    }
    app.resume_with_initial(&saved.operator, &accepted.actor, work, Some(id), None)?;
    Ok(true)
}
fn handed_over(state: &State, persona: &str) -> bool {
    state
        .operator_events
        .iter()
        .any(|e| e["kind"] == "launch_initial_handover" && e["persona"] == persona)
}
pub(super) fn authorize_initial(
    app: &mut Runtime,
    state: &mut State,
    persona: &str,
    work: &str,
    id: &str,
) -> Result<()> {
    let inv = app.invocation(id)?;
    let prepared = decode_prepared(&inv)?;
    let adopted = app.adopted(id)?;
    let actor = state
        .personas
        .get(persona)
        .ok_or_else(|| held("persona absent"))?;
    require(
        !handed_over(state, persona)
            && active_invocations(state).is_empty()
            && inv.state == s::InvocationState::Adopted
            && prepared.character_revision.is_none()
            && prepared.request.persona == persona
            && prepared.request.work == work
            && prepared.request.mode == Mode::Ordinary
            && adopted.report.status == AdoptionStatus::CharacterOnly
            && adopted.actor == persona
            && adopted.work == work
            && actor.admission_serial == prepared.admission_serial
            && actor.owner_epoch == increment(prepared.owner_epoch)?
            && actor
                .character
                .as_ref()
                .is_some_and(|c| c.attribution == format!("persona:{persona}/invocation:{id}"))
            && state.board.work(&w::WorkId(work.into()))?.version == prepared.work_version
            && matches!(
                state.board.work(&w::WorkId(work.into()))?.phase,
                w::Phase::Open
            ),
        "initial handover history or current authority changed",
    )?;
    state.operator_events.push(json!({
        "kind":"launch_initial_handover","persona":persona,"work":work,"invocation":id
    }));
    Ok(())
}
