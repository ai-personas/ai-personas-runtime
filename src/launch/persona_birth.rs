//! Explicit operator acceptance of exact primary-authored starting material.
//! Reserved identities are sharing destinations, not existing personas. Birth
//! consumes a lifetime slot without work, commitment, resources or inference.
use super::*;

const KIND: &str = "persona-birth-v1";

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// A chosen label, not a profession registration or an authority claim.
    pub label: String,
    /// Attributed trait directions and useful tensions for the child's own
    /// initial character authorship. Do not invent biography or experience.
    pub seed: String,
    /// Why a new continuing identity may help, including the limits of that claim.
    pub reason: String,
}
impl Proposal {
    fn validate(&self) -> Result<()> {
        for (value, limit) in [
            (&self.label, 256),
            (&self.seed, 16_384),
            (&self.reason, 4096),
        ] {
            text(value, limit)?;
            require(
                !value.trim().is_empty(),
                "nonblank birth proposal fields required",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Origin {
    pub receipt: s::ObjectRef,
    pub parent: String,
    pub invocation: String,
    pub work: String,
    pub proposal: p::ExactRef,
    pub exposure: Exposure,
    pub initialization_limit: u64,
    pub initialization_attempts: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub operator: String,
    pub parent: String,
    pub persona: String,
    pub proposal: p::ExactRef,
    pub expected_runtime: intake_delegation::IntakeReference,
    pub expected_launch: intake_delegation::IntakeReference,
    pub initialization_limit: u64,
    pub authorization_reference: String,
    pub authorize_creation: bool,
}
impl Request {
    pub fn validate(&self, operator: &str) -> Result<()> {
        for id in [&self.id, &self.operator, &self.parent, &self.persona] {
            identity(id)?;
        }
        require(
            self.operator == operator && self.authorize_creation,
            "matching operator and explicit creation authorization required",
        )?;
        require(
            self.parent != self.persona && (1..=16).contains(&self.initialization_limit),
            "invalid child identity or initialization bound",
        )?;
        let reference = file_reference(&self.proposal)?;
        require(
            reference.representation == f::Representation::Utf8
                && reference.start.is_none()
                && reference.end.is_none()
                && self
                    .proposal
                    .lifetime
                    .as_deref()
                    .is_none_or(|v| v == "work"),
            "birth requires one exact complete UTF-8 proposal",
        )?;
        for (reference, id) in [
            (&self.expected_runtime, STATE_ID),
            (&self.expected_launch, ID),
        ] {
            require(
                reference.id == id && reference.revision > 0,
                "invalid birth fence identity",
            )?;
            require(
                reference.sha256.len() == 64
                    && reference
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "birth fences require complete lowercase SHA-256 digests",
            )?;
        }
        text(&self.authorization_reference, 4096)?;
        require(
            !self.authorization_reference.trim().is_empty(),
            "creation authorization reference required",
        )?;
        require(
            serde_json::to_vec(self)?.len() <= 16_384,
            "birth request exceeds bound",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    request_sha256: String,
    request: Request,
    author_invocation: String,
    causal_work: String,
    source_body_sha256: String,
    created_at_unix: i64,
    seed_sha256: String,
}

fn receipt_id(id: &str) -> String {
    format!("{KIND}:{}", s::sha256_bytes(id.as_bytes()))
}

pub fn proposal_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Proposal)).expect("birth proposal schema")
}

fn authorize(state: &State, saved: &Saved, operator: &str) -> Result<()> {
    admission_extension::authorize(state, saved, operator)?;
    admission_extension::validate_saved(state, saved)?;
    require(
        (saved.setup.cohort..=8).contains(&saved.setup.persona_capacity()),
        "invalid reserved persona capacity",
    )?;
    require(
        state.config.persona_capacity == saved.setup.persona_capacity,
        "runtime and launch population declarations differ",
    )?;
    require(
        state.personas.keys().cloned().collect::<BTreeSet<_>>()
            == people(saved.setup.cohort).into_iter().collect(),
        "saved cohort does not exactly account for retained identities",
    )?;
    // The future identities must already have been named in the original
    // sharing policy. Creating a persona never rewrites a source policy.
    let future: BTreeSet<_> = people(saved.setup.persona_capacity()).into_iter().collect();
    let policy = current_policy(state, POLICY)?;
    require(
        future.is_subset(&policy.readers) && future.is_subset(&policy.audience),
        "future identities were not preauthorized by the current sharing policy",
    )?;
    saved.execution_provider()?;
    Ok(())
}

pub fn snapshot_at(root: &Path, operator: &str, parent: &str) -> Result<Value> {
    identity(operator)?;
    identity(parent)?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (head, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    authorize(&state, &saved, operator)?;
    require(
        people(saved.setup.cohort).iter().any(|id| id == parent),
        "parent is outside the saved cohort",
    )?;
    Ok(json!({"operator":operator,"parent":parent,
        "expected_runtime":intake_delegation::IntakeReference::of(&head)?,
        "expected_launch":intake_delegation::IntakeReference::of(&launch)?,
        "created_personas":saved.setup.cohort,"lifetime_persona_capacity":saved.setup.persona_capacity(),
        "next_persona":(saved.setup.cohort < saved.setup.persona_capacity()).then(||format!("persona-{:04}",saved.setup.cohort+1)),
        "proposal_schema":proposal_schema(),"initialization_limit_range":[1,16],
        "steps_spent":saved.steps,"max_total_steps":saved.setup.max_total_steps,
        "inference_performed":false,"capacity_reserved":false,
        "scope":"Accepting an exact persona-authored proposal creates a dormant identity only. A later explicitly admitted work provides an invitation and existing resources for bounded initial character authorship. Birth establishes no membership, work acceptance, competence, experience, resource increase or guarantee of initialization."}))
}

fn output(receipt: &Receipt, replayed: bool, applied: bool) -> Value {
    json!({"persona":receipt.request.persona,"parent":receipt.request.parent,
        "receipt_id":receipt_id(&receipt.request.id),"request_sha256":receipt.request_sha256,
        "source":receipt.request.proposal,"author_invocation":receipt.author_invocation,
        "causal_work":receipt.causal_work,"created_at_unix":receipt.created_at_unix,
        "replayed":replayed,"creation_applied":applied,"receipt_retained":applied || replayed,"inference_performed":false,
        "work_created":false,"responsibility_accepted":false,"resources_added":false,
        "initialization_performed":false,"initialization_limit":receipt.request.initialization_limit,
        "status":if applied || replayed { "creation receipt only; inspect current identity and character separately" } else { "preview only; no identity created or capacity reserved" }})
}

fn initialization_view(state: &State, persona: &Persona, operator: &str, now: i64) -> Value {
    let Some(origin) = &persona.birth else {
        return json!({"available":false});
    };
    let mut audience = admission(persona, now);
    audience.audience = BTreeSet::from([operator.into()]);
    if !exposure_permitted(state, &origin.exposure, &audience) {
        return json!({"available":false});
    }
    json!({"available":true,"limit":origin.initialization_limit,
        "attempts_spent":origin.initialization_attempts,
        "character_accepted":persona.character.is_some(),
        "character_revision":persona.character.as_ref().map(|character|&character.revision),
        "exhausted_without_character":initialization_reason(persona).is_some()})
}

pub fn create_at(root: &Path, request: &Request, apply: bool, now: i64) -> Result<Value> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative birth time")?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (head, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    authorize(&state, &saved, &request.operator)?;
    let hash = digest(request)?;
    let id = receipt_id(&request.id);
    if let Some(record) = app.store.read(&id)? {
        require(
            record.kind == KIND && record.revision == 1,
            "birth receipt binding unavailable",
        )?;
        let retained_reference = record.exact_ref()?;
        let receipt: Receipt = serde_json::from_value(record.body)?;
        require(
            receipt.version == 1
                && receipt.request_sha256 == hash
                && digest(&receipt.request)? == hash,
            "creation identity conflicts with the exact retained request",
        )?;
        let persona = state
            .personas
            .get(&request.persona)
            .ok_or_else(|| held("created identity unavailable"))?;
        require(
            persona.created_by == request.operator
                && persona.birth.as_ref().is_some_and(|origin| {
                    origin.receipt == retained_reference && origin.parent == request.parent
                }),
            "created identity provenance unavailable",
        )?;
        let mut outcome = output(&receipt, true, false);
        outcome["current_initialization"] =
            initialization_view(&state, persona, &request.operator, now);
        return Ok(outcome);
    }
    require(
        intake_delegation::IntakeReference::of(&head)? == request.expected_runtime
            && intake_delegation::IntakeReference::of(&launch)? == request.expected_launch,
        "birth snapshot changed; inspect before proposing a new creation request",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks persona creation",
    )?;
    app.store.require_terminal_invocations()?;
    require(
        saved.setup.cohort < saved.setup.persona_capacity()
            && request.persona == format!("persona-{:04}", saved.setup.cohort + 1)
            && !state.personas.contains_key(&request.persona),
        "no matching unused lifetime persona slot",
    )?;
    require(
        saved.steps < saved.setup.max_total_steps,
        "no remaining root inference allowance for a new persona",
    )?;
    require(
        people(saved.setup.cohort).contains(&request.parent),
        "parent is outside the saved cohort",
    )?;
    let parent = state
        .personas
        .get(&request.parent)
        .ok_or_else(|| held("parent unavailable"))?;
    require(
        parent.created_by == request.operator
            && parent.character.is_some()
            && lifecycle::reason(&state, &request.parent).is_none(),
        "parent is not an eligible continuing persona",
    )?;
    let reference = file_reference(&request.proposal)?;
    let source = state
        .sources
        .get(&reference.source)
        .ok_or_else(|| held("proposal source unavailable"))?;
    let managed = record(&state, &reference)?;
    require(
        source.owner == request.parent
            && !source.retired
            && !source.protected
            && source.head == reference.revision
            && managed.record.scope == f::Scope::Assets,
        "proposal must be the parent's current active supporting asset",
    )?;
    let (author, invocation_id, operation_id) = match &managed.origin {
        Some(RevisionOrigin::PersonaInvocation {
            persona,
            invocation_id,
            operation_id,
        }) => (persona, invocation_id, operation_id),
        _ => return Err(held(
            "birth seed requires actual accepted primary authorship; an import is not authorship",
        )),
    };
    require(
        author == &request.parent,
        "proposal author differs from parent",
    )?;
    let invocation = app.store.invocation(invocation_id)?;
    require(
        invocation.state == s::InvocationState::Adopted,
        "birth proposal invocation was not adopted",
    )?;
    let prepared = decode_prepared(&invocation)?;
    let adopted = app.adopted(invocation_id)?;
    require(prepared.request.persona == request.parent && prepared.request.mode == Mode::Ordinary
        && prepared.request.profile == Profile::Direct
        && adopted.actor == request.parent && adopted.work == prepared.request.work
        && adopted.response.operations.iter().any(|operation| matches!(operation,
            p::Operation::FileWrite { id, source, text, .. } if id == operation_id
                && source == &reference.source && text.as_bytes() == managed.record.bytes.as_slice())),
        "proposal does not match its accepted authored file write")?;
    require(
        managed.record.bytes.len() <= 24_576,
        "birth proposal body exceeds bound",
    )?;
    require(
        serde_json::from_slice::<Value>(&managed.record.bytes)?.is_object(),
        "birth proposal must be a JSON object",
    )?;
    let proposal: Proposal = serde_json::from_slice(&managed.record.bytes)?;
    proposal.validate()?;
    let mut next_saved = saved.clone();
    next_saved.setup.cohort += 1;
    let provenance = format!("persona-authored seed; parent {}; invocation {}; exact source {} @ {}; accepted by operator {}; no lived experience established",
        request.parent, invocation_id, request.proposal.source, request.proposal.revision, request.operator);
    next_saved.setup.persona_seeds.insert(
        request.persona.clone(),
        PersonaSeed {
            label: proposal.label,
            text: proposal.seed,
            provenance,
        },
    );
    let provider = saved.execution_provider()?;
    let seed = seed_for(&next_saved, &provider, &request.persona);
    validate_seed(&seed)?;
    require(
        seed.grants.fragments <= parent.seed.grants.fragments
            && seed.grants.assets <= parent.seed.grants.assets
            && seed.grants.character <= parent.seed.grants.character
            && seed.grants.work_commands <= parent.seed.grants.work_commands
            && seed
                .grants
                .write_policies
                .is_subset(&parent.seed.grants.write_policies)
            && seed
                .grants
                .communicate_with
                .is_subset(&parent.seed.grants.communicate_with)
            && seed.audience.is_subset(&parent.seed.audience)
            && seed.destination == parent.seed.destination,
        "birth cannot widen the parent's permissions or disclosure audience",
    )?;
    let receipt = Receipt {
        version: 1,
        request_sha256: hash,
        request: request.clone(),
        author_invocation: invocation_id.clone(),
        causal_work: prepared.request.work.clone(),
        source_body_sha256: s::sha256_bytes(&managed.record.bytes),
        created_at_unix: now,
        seed_sha256: digest(&seed)?,
    };
    let receipt_object = s::StoredObject {
        id,
        revision: 1,
        kind: KIND.into(),
        body: serde_json::to_value(&receipt)?,
    };
    let mut inherited = prepared_exposure(&prepared)?.clone();
    inherited.sources.push(reference.clone());
    if let Some(exposure) = &managed.exposure {
        extend_exposure(&mut inherited, exposure);
    }
    let mut next = state.clone();
    insert_seed(&mut next, &request.operator, seed)?;
    next.personas.get_mut(&request.persona).unwrap().birth = Some(Origin {
        receipt: receipt_object.exact_ref()?,
        parent: request.parent.clone(),
        invocation: invocation_id.clone(),
        work: prepared.request.work.clone(),
        proposal: request.proposal.clone(),
        exposure: inherited.clone(),
        initialization_limit: request.initialization_limit,
        initialization_attempts: 0,
    });
    let child_admission = admission(&next.personas[&request.persona], now);
    let mut operator_admission = admission(parent, now);
    operator_admission.audience = BTreeSet::from([request.operator.clone()]);
    require(
        exposure_permitted(&next, &inherited, &child_admission)
            && exposure_permitted(&state, &inherited, &operator_admission),
        "proposal or its inherited restrictions do not permit this child and requester",
    )?;
    f::assemble(
        &corpus(&next, &child_admission)?,
        std::slice::from_ref(&reference),
        &[],
        &[],
        &child_admission,
        next.config.context_bytes,
        0,
        next.config.output_reserve_bytes,
    )?;
    let preview = f::assemble(
        &corpus(&state, &operator_admission)?,
        std::slice::from_ref(&reference),
        &[],
        &[],
        &operator_admission,
        state.config.context_bytes,
        0,
        state.config.output_reserve_bytes,
    )?;
    let previous_binding = intake_delegation::episode_launch_binding(&saved)?;
    let next_binding = intake_delegation::episode_launch_binding(&next_saved)?;
    let mut carried = Vec::new();
    for (id, episode) in &mut next.intake_episodes {
        // Carry only a currently matching binding across this authorized
        // population change. Participants, grants, balances and all other
        // authority stay exact; already stale episodes remain stale.
        if episode.launch_binding_sha256 == previous_binding {
            episode.launch_binding_sha256 = next_binding.clone();
            carried.push(id.clone());
        }
    }
    validate_inventory(&next)?;
    next.operator_events.push(json!({"kind":"persona_birth","operator":request.operator,
        "persona":request.persona,"parent":request.parent,"receipt":receipt_object.exact_ref()?,
        "source":request.proposal,"at_unix":now,"resources_added":false,"inference_performed":false,
        "episode_binding_transition":{"from":previous_binding,"to":next_binding,"carried_episodes":carried}}));
    let runtime = state_object(&head, &next)?;
    let launch_object = s::StoredObject {
        id: ID.into(),
        revision: increment(launch.revision)?,
        kind: launch.kind.clone(),
        body: serde_json::to_value(next_saved)?,
    };
    let changes = vec![
        s::Change {
            expected_revision: Some(head.revision),
            object: runtime,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: Some(launch.revision),
            object: launch_object,
            depends_on: Vec::new(),
        },
        s::Change {
            expected_revision: None,
            object: receipt_object,
            depends_on: Vec::new(),
        },
    ];
    if apply {
        app.store
            .compare_and_swap_batch_requiring_terminal_invocations(changes)?;
    }
    let mut outcome = output(&receipt, false, apply);
    if apply {
        outcome["current_initialization"] = initialization_view(
            &next,
            &next.personas[&request.persona],
            &request.operator,
            now,
        );
    }
    if !apply {
        let references = preview
            .entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect::<Vec<_>>();
        let qualified = attributed_context(&state, &preview.rendered_context, &references)?;
        outcome["qualified_proposal"] = serde_json::from_str(&qualified)?;
        outcome["created_at_unix"] = Value::Null;
    }
    Ok(outcome)
}

pub(in crate::runtime) fn initialization_reason(persona: &Persona) -> Option<&'static str> {
    (persona.character.is_none()
        && persona
            .birth
            .as_ref()
            .is_some_and(|origin| origin.initialization_attempts >= origin.initialization_limit))
    .then_some("bounded initial character-authorship allowance exhausted")
}

pub(in crate::runtime) fn charge_initialization(persona: &mut Persona) -> Result<()> {
    if persona.character.is_none() {
        if let Some(origin) = &mut persona.birth {
            require(
                origin.initialization_attempts < origin.initialization_limit,
                "bounded initial character-authorship allowance exhausted",
            )?;
            origin.initialization_attempts = increment(origin.initialization_attempts)?;
        }
    }
    Ok(())
}

pub(in crate::runtime) fn mandatory_references(persona: &Persona) -> Result<Vec<f::Reference>> {
    let mut references = character_refs(persona)?;
    if persona.character.is_none() {
        if let Some(origin) = &persona.birth {
            references.push(file_reference(&origin.proposal)?);
        }
    }
    Ok(references)
}
