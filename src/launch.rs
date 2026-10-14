//! Explicit foreground orchestration over the existing Runtime and Store.
use super::*;
#[path = "launch/results.rs"]
mod results;
pub use results::Output;
#[cfg(all(test, unix))]
#[path = "launch/tests.rs"]
mod tests;

const ID: &str = "runtime-launch-v1";
const POLICY: &str = "launch-local";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub cohort: usize,
    pub owner: usize,
    pub destination: String,
    pub provider: DirectProviderSettings,
    pub working_directory: PathBuf,
    pub share_cohort: bool,
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
    owner: usize,
    destination: String,
    provider: serde_json::Value,
    working_directory: PathBuf,
    share_cohort: bool,
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
    Seed {
        id: id.into(),
        label: id.into(),
        text: format!("Unaccepted creation material for {id}; no prior experience asserted."),
        provenance: "operator-supplied generic seed".into(),
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
pub fn setup(root: &Path, operator: &str, settings: Setup, opt_in: bool) -> Result<Runtime> {
    identity(operator)?;
    require(
        opt_in && settings.share_cohort,
        "explicit direct-provider access and cohort sharing consent required",
    )?;
    require(
        (2..=8).contains(&settings.cohort) && (1..=settings.cohort).contains(&settings.owner),
        "cohort must be 2..=8 and owner must name a cohort position",
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
    let policy = f::Policy {
        id: POLICY.into(),
        revision: "1".into(),
        readers: ids.into_iter().collect(),
        processing_destinations: BTreeSet::from([settings.destination.clone()]),
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
    let root = private_tree(root)?;
    require(
        !working.starts_with(&root) && !root.starts_with(&working),
        "provider working directory and private state must be separate",
    )?;
    let mut app = Runtime::create(&root, config, operator)?;
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
    let (old, mut saved) = load(app)?;
    ensure_cohort(app, &saved)?;
    require(
        (saved.works.len() as u64) < saved.setup.max_works,
        "work identity limit reached",
    )?;
    let id = format!("work-{:08}", saved.works.len() + 1);
    saved.works.insert(id.clone(), None);
    save(app, Some(&old), &saved)?;
    let ids = people(saved.setup.cohort);
    let owner = &ids[saved.setup.owner - 1];
    let input = NeedInput {
        id: id.clone(),
        origin: NeedOrigin::Human,
        request: request.into(),
        boundaries: "Local permitted sources and local delivery within saved bounds.".into(),
        authorization: format!("Requested by {}; existing limits only.", saved.operator),
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
        decision_rule: w::DecisionRule::Designated(w::PersonaId(owner.clone())),
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
                    exact_terms: if persona == owner {
                        "Offer to own the requested result and designated decision.".into()
                    } else {
                        "Offer a contribution without exclusive outcome or review ownership.".into()
                    },
                    outcomes: if persona == owner {
                        BTreeSet::from([w::OutcomeId("result".into())])
                    } else {
                        BTreeSet::new()
                    },
                    review_roles: BTreeSet::new(),
                },
            })
            .collect(),
    };
    app.add_need(&saved.operator, input)
        .map_err(|_| invalid(format!("{id}: intake failed; no automatic retry")))?;
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
fn reserve(app: &mut Runtime, work: &str, persona: &str) -> Result<(PrepareRequest, Saved)> {
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
    save(app, Some(&old), &saved)?;
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
/// Explicitly continue a saved foreground session. This permits one fresh
/// consideration past a partial acceptance boundary, never semantic resume,
/// new resources, invented consent or recovery of a blocked invocation.
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
    mut explicit_continue: bool,
) -> Result<Output> {
    let (_, saved) = load(app)?;
    saved.execution_provider()?;
    require(saved.works.contains_key(work), "unknown launch work")?;
    let mut remaining = saved.setup.max_considerations;
    let mut stop = "step_limit";
    for _ in 0..saved.setup.max_steps {
        let mut view = result(app, work)?;
        let boundary = match view.recorded_phase.as_deref() {
            Some("completed") => Some("recorded_completion"),
            Some("paused") => Some("paused"),
            Some("ended") => Some("ended"),
            _ if view.status == "withheld" => Some("unavailable"),
            _ if view.status == "intake-incomplete" => Some("intake_incomplete"),
            _ if view.status == "delivered-acceptance-unestablished" && !explicit_continue => {
                Some("acceptance_boundary")
            }
            _ => None,
        };
        if let Some(boundary) = boundary {
            view.stop = Some(boundary.into());
            return Ok(view);
        }
        explicit_continue = false;
        let attempt = async {
            if !handover(app, work)? {
                return Ok("character_fence");
            }
            let (count, selected) = select(app, work, remaining, unix_now()?)?;
            remaining -= count;
            let Some(persona) = selected else {
                return Ok("idle_or_consideration_bound");
            };
            let (request, current) = reserve(app, work, &persona)?;
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
    if persona.focus[work].permit || handed_over(&state, &accepted.actor) {
        return Ok(true);
    }
    if decode_prepared(&inv)?.character_revision.is_some() {
        return Ok(false);
    }
    app.resume_with_initial(&saved.operator, &accepted.actor, work, Some(id))?;
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
