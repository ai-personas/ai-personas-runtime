//! Explicit foreground orchestration over the existing Runtime and Store.
use super::*;
#[path = "launch/results.rs"]
mod results;
#[path = "launch/parallel.rs"]
mod parallel;
#[path = "launch/delegated.rs"]
mod delegated;
#[path = "launch/step_accounting.rs"]
pub(crate) mod step_accounting;
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
pub use question_commands::{answer_question, shared_questions, shared_questions_at, QuestionAnswerInput};
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
#[path = "launch/inspection.rs"]
mod inspection;
#[path = "launch/task_amendment.rs"]
pub mod task_amendment;
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
#[path = "launch/optional_activity.rs"]
mod optional_activity_commands;
pub use optional_activity_commands::{
    grant_optional_activity, optional_activity_snapshot, optional_activity_snapshot_at, revoke_optional_activity,
    open_optional_activity_at, start_optional_activity, OptionalActivityOpen,
    OptionalActivityRequest, OptionalActivityRevocation, OptionalActivityStart,
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
#[path = "launch/head_compaction.rs"]
pub mod head_compaction;
#[path = "launch/context_capacity.rs"]
pub mod context_capacity;
#[path = "launch/persona_lifecycle.rs"]
pub mod persona_lifecycle;
#[path = "launch/persona_birth.rs"]
pub mod persona_birth;
#[path = "launch/work_command_guides.rs"]
pub mod work_command_guides;
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
    /// Historical population declaration; it does not cap future identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_capacity: Option<usize>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    persona_capacity: Option<usize>,
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
    fn persona_capacity(&self) -> usize {
        self.persona_capacity.unwrap_or(self.cohort)
    }
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
        if self.format != 2 {
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
    let saved = decode_saved(&object)?;
    Ok((object, saved))
}
fn decode_saved(object: &s::StoredObject) -> Result<Saved> {
    require(object.kind == "foreground-launch", "launch object kind")?;
    let saved: Saved = serde_json::from_value(object.body.clone())?;
    require(
        (saved.format == 1
            && saved.research_opt_in == Some(true)
            && saved.direct_provider_opt_in.is_none())
            || (saved.format == 2 && saved.research_opt_in.is_none()),
        "saved launch format and authorization metadata are invalid",
    )?;
    Ok(saved)
}
fn save(app: &mut Runtime, old: Option<&s::StoredObject>, saved: &Saved) -> Result<()> {
    save_owned(app, old, saved, &BTreeSet::new())
}
fn save_owned(
    app: &mut Runtime,
    old: Option<&s::StoredObject>,
    saved: &Saved,
    owned: &BTreeSet<String>,
) -> Result<()> {
    let (head, state) = app.load()?;
    require(
        app.invocation_leases_owned(&state, owned)?,
        "unfinished invocation blocks launch writes",
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
    Ok(base.join("ai-personas"))
}
fn private_tree(path: &Path) -> Result<PathBuf> {
    canonical_state_root(path, true)
}
fn seed_for(saved: &Saved, provider: &ResearchSettings, id: &str) -> Seed {
    let mut audience: BTreeSet<_> = people(saved.setup.persona_capacity()).into_iter().collect();
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
// Growing the cohort must not rewrite or reconstruct old disclosure declarations.
// Compare identity/model/creation material while preserving each seed's exact
// historical audience, grants and model-confirmation metadata.
fn seed_matches_saved(saved: &Saved, provider: &ResearchSettings, id: &str, seed: &Seed) -> Result<bool> {
    let mut expected = seed_for(saved, provider, id);
    expected.audience = seed.audience.clone();
    expected.grants = seed.grants.clone();
    expected.model_confirmed = seed.model_confirmed;
    Ok(serde_json::to_value(seed)? == serde_json::to_value(expected)?)
}

fn ensure_cohort(app: &mut Runtime, saved: &Saved) -> Result<()> {
    let provider = saved.execution_provider()?;
    let state = app.snapshot()?;
    for id in people(saved.setup.cohort) {
        let seed = seed_for(saved, &provider, &id);
        if let Some(existing) = state.personas.get(&id) {
            require(
                seed_matches_saved(saved, &provider, &id, &existing.seed)?,
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
        settings.cohort > 0 && (1..=settings.cohort).contains(&settings.owner),
        "cohort must be nonempty and owner must name a cohort position",
    )?;
    let capacity = settings.persona_capacity.unwrap_or(settings.cohort).max(settings.cohort);
    let ids = people(capacity);
    require(
        !ids.iter().any(|id| id == operator),
        "requester collides with a cohort identity",
    )?;
    let mut audience: BTreeSet<_> = ids.iter().cloned().collect();
    audience.insert(operator.into());
    require(settings.http_origins.iter().all(|origin| crate::http_fetch::valid_origin(origin)),
        "historical HTTP origins must be canonical")?;
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
    config.persona_capacity = settings.persona_capacity;
    config.allow_direct_provider = true;
    config.max_work_attempts = settings.max_work_attempts;
    settings.provider.validate()?;
    // The operator's validated provider settings define the possible transport
    // output for each attempt. Reserve that exposure from the existing Store
    // allowance instead of rejecting an otherwise valid fresh setup against a
    // smaller default. This does not increase the controlling resource totals.
    config.transport_output_reserve_bytes = config
        .transport_output_reserve_bytes
        .max(settings.provider.limits.max_stream_bytes as u64);

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
    install_persona_seeds(&mut saved, seeds)?;
    let root = private_tree(root)?;
    require(
        !saved.working_directory.starts_with(&root) && !root.starts_with(&saved.working_directory),
        "provider working directory and private state must be separate",
    )?;
    let mut app = Runtime::create(&root, config, operator)?;
    save(&mut app, None, &saved)?;
    ensure_cohort(&mut app, &saved)?;
    work_command_guides::install_fresh(&mut app, &saved)?;
    Ok(app)
}

fn install_persona_seeds(saved: &mut Saved, seeds: BTreeMap<String, PersonaSeed>) -> Result<()> {
    let ids = people(saved.setup.cohort);
    require(
        seeds.len() <= ids.len() && seeds.keys().all(|id| ids.contains(id)),
        "persona seed must name a member of this cohort",
    )?;
    saved.setup.persona_seeds = seeds;
    let provider = saved.execution_provider()?;
    for id in &ids {
        validate_seed(&seed_for(saved, &provider, id))?;
    }
    Ok(())
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
    let boundaries = "Address the request using full host command access by default, including installing and using tools, filesystem and network access. Deliver the result locally. Resource settings and recorded allocations are accounting metadata, not execution quotas.";
    let authorization = format!(
        "Requested by {}. Personas may organize, collaborate, request further work from existing personas, and amend their arrangements within the current task mandate. Each affected persona chooses whether to accept its exact commitments. Host commands, tool installation, filesystem and network access are available by default. Historical resource settings and participant declarations impose no application quotas or additional delegation permission. Invitations and result decision rights do not accept responsibility for another persona or change the requester's task mandate.",
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
                optional_activity: None,
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
        assumptions: "Host tools may be installed and used directly. Independent assurance and requester acceptance are not inferred.".into(),
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
        app.invocation_leases_quiescent(&state)?,
        "unfinished invocation blocks launch intake",
    )?;
    app.store.require_quiescent_invocations()?;
    let work_count =
        u64::try_from(saved.works.len()).map_err(|_| invalid("saved work count overflow"))?;
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
        .compare_and_swap_batch_requiring_quiescent_invocations(changes)?;
    Ok(id)
}
type Pick = (usize, Option<String>);
fn select(app: &mut Runtime, work: &str, now: i64) -> Result<Pick> {
    select_owned(app, work, now, &BTreeSet::new())
}
fn select_owned(
    app: &mut Runtime,
    work: &str,
    now: i64,
    owned: &BTreeSet<String>,
) -> Result<Pick> {
    select_owned_except(app, work, now, owned, &BTreeSet::new())
}
fn select_owned_except(
    app: &mut Runtime,
    work: &str,
    now: i64,
    owned: &BTreeSet<String>,
    visited: &BTreeSet<(String, String)>,
) -> Result<Pick> {
    let time = u64::try_from(now).map_err(|_| invalid("negative scheduling time"))?;
    let (head, state) = app.load()?;
    require(
        app.invocation_leases_owned(&state, owned)?,
        "unfinished invocation requires completion or explicit recovery",
    )?;
    let (old, mut cursor) = app.load_scheduler()?;
    let work_id = w::WorkId(work.into());
    let current = state.board.work(&work_id)?;
    let after = cursor.foreground_after.get(work)
        .map(|actor| (work_id.clone(), w::PersonaId(actor.clone())))
        .or_else(|| cursor.after.clone().filter(|(id, _)| id == &work_id));
    let pairs = || current.participants.keys().map(|actor| (work_id.clone(), actor.clone()));
    // A foreground run selects from its complete work, in circular identity
    // order. An inspection page elsewhere in the runtime cannot hide a runnable
    // participant or make this run report a false lack of eligible work.
    let page = pairs()
        .filter(|pair| after.as_ref().is_none_or(|last| pair > last))
        .chain(pairs().filter(|pair| after.as_ref().is_some_and(|last| pair <= last)));
    let mut selected = None;
    cursor.last.clear();
    for (wid, actor) in page {
        if visited.contains(&(wid.0.clone(), actor.0.clone())) {
            continue;
        }
        let opportunity = runtime_opportunity(&state, &wid, &actor, time);
        let mut entry = scheduler_entry(&state, wid.clone(), actor.clone(), opportunity, now)?;
        if wid.0 == work {
            let permit = continuation_permit(&state, &actor.0, work);
            if permit
                && matches!(state.board.work(&wid)?.phase, w::Phase::Open)
                && state.work_meta.contains_key(work)
            {
                entry.disposition = SchedulerDisposition::Queued;
            }
            if matches!(entry.disposition, SchedulerDisposition::Queued) {
                selected = Some(actor.0.clone());
            }
        }
        cursor.foreground_after.insert(wid.0.clone(), actor.0.clone());
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
    reserve_owned(app, work, persona, prospective_intake, &BTreeSet::new())
}
fn reserve_owned(
    app: &mut Runtime,
    work: &str,
    persona: &str,
    prospective_intake: bool,
    owned: &BTreeSet<String>,
) -> Result<(PrepareRequest, Saved)> {
    let (old, mut saved) = load(app)?;
    saved.execution_provider()?;
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
        save_owned(app, Some(&old), &saved, owned)?;
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
    fn concurrent(&self) -> bool {
        false
    }
    // Trusted driver intent only. Serialized persona output cannot select this.
    fn prospective_intake(&self) -> bool {
        false
    }
    fn select_intake(&mut self, selection: p::need::GrantReference) -> Result<()>;
    fn population_created(&mut self, _before: &Saved, _after: &Saved) -> Result<()> {
        Err(held("driver cannot advance a population binding"))
    }
    fn before_step(&mut self, _app: &mut Runtime) -> Result<()> {
        Ok(())
    }
    fn start(
        &mut self,
        _app: &mut Runtime,
        _request: PrepareRequest,
        _saved: &Saved,
        _now: i64,
        _owned: &BTreeSet<String>,
    ) -> Result<provider_calls::Pending> {
        Err(held("driver does not own concurrent transport"))
    }
    fn finish(
        &mut self,
        app: &mut Runtime,
        completion: provider_calls::Completion,
    ) -> Result<AdoptionReport> {
        app.finish_provider_call(completion)
    }
    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport>;
}
pub async fn run(app: &mut Runtime, work: &str) -> Result<Output> {
    let owner = app.store.acquire_execution_owner()?;
    session::run_environment_owned(app, work, &owner).await
}
/// Explicitly continue a saved foreground session through existing
/// eligible routes. Delivery status grants no semantic resume, new resources,
/// invented consent or recovery of a blocked invocation.
pub async fn continue_run(app: &mut Runtime, work: &str) -> Result<Output> {
    run(app, work).await
}
fn quiescent_call_allows_peer_progress(app: &mut Runtime, id: &str) -> Result<bool> {
    let invocation = match app.invocation(id) {
        Ok(invocation) => invocation,
        Err(RuntimeError::Store(s::StoreError::Missing(_))) => return Ok(false),
        Err(error) => return Err(error),
    };
    if !matches!(invocation.state, s::InvocationState::Failed | s::InvocationState::Uncertain) {
        return Ok(false);
    }
    let state = app.snapshot()?;
    app.invocation_leases_quiescent(&state)
}
async fn run_with<D: Driver>(app: &mut Runtime, work: &str, driver: &mut D, owner: &s::ExecutionOwner) -> Result<Output> {
    run_with_options(app, work, driver, owner, false).await
}
/// Hold the command scope inside the already acquired execution owner. Both
/// successful and failed runs drain admitted workers before their final view.
async fn run_with_owned_commands<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    owner: &s::ExecutionOwner,
) -> Result<Output> {
    driver.before_step(app)?;
    if !driver.prospective_intake() {
        if let Some(selection) =
            intake_delegation::default_for_work_owned(app, work, owner, unix_now()?)?
        {
            driver.select_intake(selection)?;
        }
    }
    let mut scope = app.background_host_commands(owner)?;
    let state = scope.snapshot()?;
    let independent = driver.concurrent()
        && !driver.prospective_intake()
        && !need_proposals::bound(&state, work)
        && state.board.work(&w::WorkId(work.into()))?.need.optional_activity.is_none();
    let outcome = if independent {
        parallel::run(&mut scope, work, driver, owner).await
    } else if driver.prospective_intake() || need_proposals::bound(&state, work) {
        delegated::run(&mut scope, work, driver, owner).await
    } else {
        run_with(&mut scope, work, driver, owner).await
    };
    while scope.has_background_host_commands() {
        scope.wait_for_host_command().await?;
    }
    let output = outcome?;
    let mut final_view = result(&mut scope, work)?;
    final_view.stop = output.stop;
    Ok(final_view)
}

async fn run_with_options<D: Driver>(
    app: &mut Runtime,
    work: &str,
    driver: &mut D,
    owner: &s::ExecutionOwner,
    yield_after_step: bool,
) -> Result<Output> {
    let (_, saved) = load(app)?;
    saved.execution_provider()?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    let stop = loop {
        app.publish_completed_host_commands(unix_now()?)?;
        app.retire_ready_rejected_continuations(work, unix_now()?)?;
        persona_birth::create_pending_for_work_owned(app, work, driver, owner)?;
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
            let (_, selected) = select(app, work, unix_now()?)?;
            let Some(persona) = selected else {
                if app.has_background_host_commands() {
                    if yield_after_step {
                        return Ok("work_step_yielded");
                    }
                    app.wait_for_host_command().await?;
                    return Ok("jobs_completed");
                }
                return Ok(if active_invocations(&app.snapshot()?).is_empty() {
                    "no_eligible_route"
                } else {
                    "held"
                });
            };
            driver.before_step(app)?;
            let (request, current) = reserve(app, work, &persona, driver.prospective_intake())?;
            let invocation_id = request.id.clone();
            let report = match driver.step(app, request, &current, unix_now()?).await {
                Ok(report) => report,
                Err(error) => {
                    // Failed or uncertain calls retain their actor's lease.
                    // Independent peers may proceed; nothing is replayed,
                    // reclassified or refunded.
                    if quiescent_call_allows_peer_progress(app, &invocation_id)? {
                        return Ok("continue");
                    }
                    return Err(error);
                }
            };
            if report.status == AdoptionStatus::Held {
                driver.before_step(app)?;
                if app.retire_rejected_continuation(&invocation_id, unix_now()?)? {
                    // A fresh admission uses the existing accepted continuation,
                    // actual rejection observations and the remaining session bound.
                    return Ok("continue");
                }
                return Ok(if quiescent_call_allows_peer_progress(app, &invocation_id)? {
                    "continue"
                } else {
                    "held"
                });
            }
            if report.status == AdoptionStatus::CharacterOnly && !handover(app, work)? {
                return Ok("character_fence");
            }
            Ok::<_, RuntimeError>("continue")
        }
        .await;
        match attempt {
            Ok("jobs_completed" | "continue") if yield_after_step => break "work_step_yielded",
            Ok("jobs_completed" | "continue") => (),
            Ok(reason) => break reason,
            Err(error) => break stop_for_error(&error),
        }
    };
    let mut view = result(app, work)?;
    view.stop = Some(stop.into());
    Ok(view)
}
fn stop_for_error(error: &RuntimeError) -> &'static str {
    match error {
        RuntimeError::Files(f::FilesError::MandatoryOverflow { required, limit }) => {
            // Only trusted numeric assembly measurements enter stderr.
            eprintln!("Mandatory context requires {required} bytes; configured limit is {limit}. Optional-selection repair cannot remove mandatory context.");
            "mandatory_context_overflow"
        }
        RuntimeError::SelectionOverflow { .. } => "selection_overflow",
        RuntimeError::Provider(_) => "provider_preflight_or_transport_failed",
        RuntimeError::Protocol(_) => "invalid_response",
        RuntimeError::LifetimeStepLimit => "lifetime_step_limit",
        RuntimeError::Held(_) => "held_or_unresolved_invocation",
        RuntimeError::Store(_) => "store_or_resource_limit",
        _ => "blocked_or_unresolved_invocation",
    }
}
/// Fresh bounded result projection; opening performs no owner recovery.
pub fn result_at(root: &Path, operator: Option<&str>, work: &str) -> Result<Output> {
    identity(work)?;
    inspection::project(root, |snapshot, _| {
        require(operator.is_none_or(|value| value == snapshot.saved.operator), "saved operator mismatch")?;
        results::project_state(&snapshot.state, work, &snapshot.saved, unix_now()?)
    })
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
    handover_invocation(app, work, id)
}
fn handover_invocation(app: &mut Runtime, work: &str, id: &str) -> Result<bool> {
    let (_, saved) = load(app)?;
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
            && actor.active_invocation.is_none()
            && app.invocation_leases_quiescent(state)?
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
            && decision_scope::work_current(state, &prepared)?
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
