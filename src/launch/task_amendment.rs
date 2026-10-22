//! Operator-authored changes to a live task. Original requests, accepted history,
//! running transports and resource accounts are retained independently.
use super::*;


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub request: String,
    pub boundaries: String,
    pub terms: String,
    pub assumptions: String,
    pub outcomes: BTreeMap<String, OutcomeSpec>,
    pub review_roles: BTreeMap<String, RoleSpec>,
    /// Omission preserves the governing envelope, with its exact successor
    /// mandate binding. Explicit changes remain an operator decision.
    pub decision_rule: Option<w::DecisionRule>,
    pub delivery_to: BTreeSet<String>,
    pub stakeholders: BTreeSet<String>,
}

impl Scope {
    fn from_input(input: &NeedInput) -> Self {
        Self {
            request: input.request.clone(),
            boundaries: input.boundaries.clone(),
            terms: input.terms.clone(),
            assumptions: input.assumptions.clone(),
            outcomes: input.outcomes.clone(),
            review_roles: input.review_roles.clone(),
            decision_rule: Some(input.decision_rule.clone()),
            delivery_to: input.delivery_to.clone(),
            stakeholders: input.stakeholders.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub operator: String,
    pub work: String,
    pub expected_runtime: s::ObjectRef,
    pub expected_launch: s::ObjectRef,
    pub scope: Scope,
    /// Explicit new offers; they do not accept responsibility for any persona.
    pub invitations: Vec<Invitation>,
    pub resume: BTreeSet<String>,
    #[serde(default)]
    pub authorize_amend: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SupersededCall {
    pub persona: String,
    pub owner_epoch: u64,
    pub prepare_hash: String,
    pub request_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OtherTaskCall {
    pub work: String,
    pub state_at_save: s::InvocationState,
    pub binding: SupersededCall,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub request: Request,
    pub request_sha256: String,
    pub at_unix: i64,
    pub previous_work_version: w::Version,
    pub mandate_revision: u64,
    /// Exact effective authored input. The original intake record is unchanged.
    pub input: NeedInput,
    pub superseded_calls: BTreeMap<String, SupersededCall>,
    /// Observed competing ownership, not superseded authority or permission to
    /// retire another task's invocation. The resumption waits for that owner.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub other_task_calls: BTreeMap<String, OtherTaskCall>,
    pub resumptions: BTreeMap<String, String>,
    pub exposure: Exposure,
}

fn authorize(snapshot: &inspection::Snapshot, operator: &str, work: &str) -> Result<()> {
    identity(operator)?;
    identity(work)?;
    require(
        snapshot.saved.operator == operator,
        "saved operator mismatch",
    )?;
    require(
        snapshot.saved.works.contains_key(work),
        "amendment requires saved work",
    )?;
    require(
        snapshot
            .state
            .work_meta
            .get(work)
            .is_some_and(|m| m.intake_operator == operator),
        "amendment requires the task's intake operator",
    )
}

pub fn snapshot(root: &Path, operator: &str, work: &str) -> Result<Value> {
    let (app, snapshot) = inspection::Snapshot::open(root)?;
    authorize(&snapshot, operator, work)?;
    let state = &snapshot.state;
    let meta = &state.work_meta[work];
    let board = state.board.work(&w::WorkId(work.into()))?;
    require(
        meta.amendments
            .last()
            .map_or(board.current_mandate == 1, |record| {
                record.mandate_revision == board.current_mandate
            }),
        "current authored scope is unavailable through this amendment interface",
    )?;
    let input = meta.amendments.last().map_or(&meta.original, |r| &r.input);
    let mut scope = Scope::from_input(input);
    scope.decision_rule = Some(board.mandate().decision_rule.clone());
    let result = json!({
        "runtime":snapshot.runtime.exact_ref()?,"launch":snapshot.launch.exact_ref()?,
        "work":work,"work_version":board.version,"mandate_revision":board.current_mandate,
        "next_mandate_revision":increment(board.current_mandate)?,"current_scope":scope,
        "participants":board.participants.keys().map(|p| p.0.clone()).collect::<Vec<_>>(),
        "attempts_left":meta.attempts_left,"phase":board.phase,
        "scope_saved":false,"inference_performed":false,
        "rule":"An amendment preserves the original request and earlier records. New offers require fresh persona acceptance. Explicit resume targets receive a fresh opportunity only under current lifecycle, transport, context and remaining resource conditions."
    });
    snapshot.unchanged(&app)?;
    Ok(result)
}

fn outcome(record: &Record, replayed: bool) -> Value {
    json!({"id":record.request.id,"work":record.request.work,
        "mandate_revision":record.mandate_revision,"scope_saved":true,"replayed":replayed,
        "resumption_requested":record.request.resume,"resumptions_at_save":record.resumptions,
        "superseded_invocations":record.superseded_calls.keys().collect::<Vec<_>>(),
        "other_task_invocations":record.other_task_calls.keys().collect::<Vec<_>>(),
        "snapshot_changed":false,
        "current_status":"Inspect current state; replay does not renew resumption.",
        "original_request_preserved":true,"old_acceptances_are_current":false,
        "inference_performed":false,"transport_cancellation_requested":false,"budget_reset":false})
}

pub fn amend(root: &Path, operator: &str, request: &Request) -> Result<Value> {
    identity(&request.id)?;
    require(
        request.operator == operator,
        "amendment operator mismatch",
    )?;
    let (mut app, snapshot) = inspection::Snapshot::open(root)?;
    authorize(&snapshot, operator, &request.work)?;
    let request_sha256 = digest(request)?;
    let meta = &snapshot.state.work_meta[&request.work];
    if let Some(record) = meta.amendments.iter().find(|r| r.request.id == request.id) {
        require(
            record.request_sha256 == request_sha256,
            "amendment identity conflict",
        )?;
        snapshot.unchanged(&app)?;
        return Ok(outcome(record, true));
    }
    if snapshot.runtime.exact_ref()? != request.expected_runtime
        || snapshot.launch.exact_ref()? != request.expected_launch
    {
        return Ok(
            json!({"id":request.id,"work":request.work,"scope_saved":false,
            "snapshot_changed":true,"replayed":false,"inference_performed":false,
            "transport_cancellation_requested":false,"budget_reset":false}),
        );
    }
    let board = snapshot
        .state
        .board
        .work(&w::WorkId(request.work.clone()))?;
    require(
        board.need.optional_activity.is_none(),
        "optional activity is not an assigned task",
    )?;
    require(
        matches!(board.phase, w::Phase::Open | w::Phase::Paused { .. }),
        "amendment requires live work",
    )?;
    let participants: BTreeSet<_> = board.participants.keys().map(|id| id.0.clone()).collect();
    require(
        request.resume.is_subset(&participants)
            && request
                .invitations
                .iter()
                .all(|i| participants.contains(&i.to)),
        "amendment cannot revive removed participation",
    )?;
    let now = unix_now()?;
    let inherited = work_exposure(&snapshot.state, &request.work)?;
    let mut superseded_calls = BTreeMap::new();
    let mut other_task_calls = BTreeMap::new();
    for id in &participants {
        let persona = &snapshot.state.personas[id];
        require(
            persona.created_by == operator,
            "amendment participant belongs to another operator",
        )?;
        if let Some(call) = &persona.active_invocation {
            let invocation = app.store.trusted_host().invocation(call)?;
            let prepared = decode_prepared(&invocation)?;
            require(
                prepared.request.id == *call
                    && prepared.request.persona == *id
                    && prepared.owner_epoch == persona.owner_epoch
                    && prepared.admission_serial == persona.admission_serial,
                "amendment invocation lease mismatch",
            )?;
            let binding = SupersededCall {
                persona: id.clone(),
                owner_epoch: persona.owner_epoch,
                prepare_hash: invocation.prepare_hash.clone(),
                request_sha256: digest(&invocation.request)?,
            };
            if prepared.request.work != request.work {
                require(
                    !matches!(invocation.state, s::InvocationState::Prepared
                        | s::InvocationState::Dispatched | s::InvocationState::Responded)
                        || decision_scope::isolates_other_work(&prepared),
                    "older or globally scoped invocation in another task must settle before amendment",
                )?;
                other_task_calls.insert(
                    call.clone(),
                    OtherTaskCall {
                        work: prepared.request.work.clone(),
                        state_at_save: invocation.state.clone(),
                        binding,
                    },
                );
                continue;
            }
            if matches!(
                invocation.state,
                s::InvocationState::Prepared
                    | s::InvocationState::Dispatched
                    | s::InvocationState::Responded
                    | s::InvocationState::Failed
            ) {
                superseded_calls.insert(call.clone(), binding);
            }
        }
    }
    let revision = increment(board.current_mandate)?;
    let mut input = meta.original.clone();
    input.request = request.scope.request.clone();
    input.boundaries = request.scope.boundaries.clone();
    input.terms = request.scope.terms.clone();
    input.assumptions = request.scope.assumptions.clone();
    input.outcomes = request.scope.outcomes.clone();
    input.review_roles = request.scope.review_roles.clone();
    input.delivery_to = request.scope.delivery_to.clone();
    input.stakeholders = request.scope.stakeholders.clone();
    input.invitations = request.invitations.clone();
    input.decision_rule = request
        .scope
        .decision_rule
        .clone()
        .unwrap_or_else(|| board.mandate().decision_rule.clone());
    if let w::DecisionRule::PersonaArranged { delegation, .. } = &mut input.decision_rule {
        require(
            delegation.issuer == w::Origin::Human(operator.into())
                && delegation.authorization == board.mandate().authority
                && delegation.need
                    == w::formation_need_ref(&w::WorkId(request.work.clone()), &board.need),
            "amendment formation authority differs from current task",
        )?;
        delegation.revision = increment(delegation.revision)?;
        delegation.mandate_revision = revision;
    }
    intake::validate_input(operator, &input)?;
    for invitation in &input.invitations {
        require(
            invitation.terms.mandate_revision == revision,
            "amendment invitation must name the new mandate revision",
        )?;
    }
    let mandate = intake::mandate(&input, revision, board.mandate().authority.clone())?;
    let previous_work_version = board.version;
    let mut next = snapshot.state.clone();
    observe(
        &mut next,
        operator,
        &request.work,
        w::RuntimeRight::Governance,
        w::RuntimeOperation::AmendMandate(mandate),
    )?;
    for invitation in &input.invitations {
        observe(
            &mut next,
            operator,
            &request.work,
            w::RuntimeRight::Governance,
            w::RuntimeOperation::Invite {
                id: w::RecordId(invitation.id.clone()),
                to: w::PersonaId(invitation.to.clone()),
                terms: invitation.terms.clone(),
            },
        )?;
    }
    arrangements::retain_amendment_origin(&mut next, &request.work, &inherited)?;
    let mut resumptions = BTreeMap::new();
    for id in &participants {
        let lifecycle_blocked = lifecycle::reason(&next, id).is_some();
        let persona = next.personas.get_mut(id).unwrap();
        let failed = persona.failed_attempt_hold.is_some()
            || persona.active_invocation.as_ref().is_some_and(|call| {
                !superseded_calls.contains_key(call) && !other_task_calls.contains_key(call)
            });
        let focus = persona
            .focus
            .get_mut(&request.work)
            .ok_or_else(|| held("task focus absent"))?;
        focus.permit = false;
        focus.explicit_resume_permit = false;
        focus.ordinary_continuation = false;
        let requested = request.resume.contains(id);
        let status = if !requested {
            "not_requested"
        } else if lifecycle_blocked {
            "blocked_lifecycle"
        } else if failed {
            "blocked_existing_invocation_recovery"
        } else {
            focus.permit = true;
            focus.explicit_resume_permit = true;
            if let Some(other) = persona
                .active_invocation
                .as_ref()
                .and_then(|call| other_task_calls.get(call))
            {
                match other.state_at_save {
                    s::InvocationState::Failed | s::InvocationState::Uncertain => {
                        "awaiting_other_task_recovery"
                    }
                    s::InvocationState::Adopted => "awaiting_other_task_handover",
                    _ => "awaiting_other_task_invocation",
                }
            } else if persona.active_invocation.is_some() {
                "awaiting_old_scope_disposition"
            } else {
                "requested_fresh_admission"
            }
        };
        if persona.active_invocation.is_none() {
            persona.owner_epoch = increment(persona.owner_epoch)?;
        }
        resumptions.insert(id.clone(), status.into());
    }
    let record = Record {
        request: request.clone(),
        request_sha256,
        at_unix: now,
        previous_work_version,
        mandate_revision: revision,
        input,
        superseded_calls,
        other_task_calls,
        resumptions,
        exposure: inherited,
    };
    let amended = next.board.work(&w::WorkId(request.work.clone()))?;
    for episode in next.intake_episodes.values_mut()
        .filter(|episode| episode.work_binding(&request.work).is_some())
    {
        require(episode.work_binding(&request.work)
            == Some((&board.mandate().authority, previous_work_version.epoch)),
            "episode work binding changed before amendment")?;
        episode.record_amendment(&request.work, need_proposals::ScopeAmendment {
            request_sha256: record.request_sha256.clone(),
            mandate_revision: revision,
            authority: amended.mandate().authority.clone(),
            work_epoch: amended.version.epoch,
        })?;
    }
    next.work_meta
        .get_mut(&request.work)
        .unwrap()
        .amendments
        .push(record.clone());
    next.operator_events
        .push(json!({"kind":"task_amendment","operator":operator,
        "work":request.work,"id":request.id,"mandate_revision":revision,
        "transport_cancellation_requested":false,"budget_reset":false}));
    validate_inventory(&next)?;
    need_proposals::validate_state(&next)?;
    app.store.trusted_host().commit_runtime_amendment(
        &request.expected_runtime,
        state_object(&snapshot.runtime, &next)?,
        &request.work,
        &request.expected_launch,
    )?;
    Ok(outcome(&record, false))
}

/// A saved amendment may precede a new session rather than arrive during a call.
/// Retire only its exact complete rejected decisions at a quiescent boundary.
pub(super) fn retire_ready(app: &mut Runtime, work: &str) -> Result<()> {
    let state = app.snapshot()?;
    let calls = state.work_meta.get(work).and_then(|meta| meta.amendments.last())
        .map(|amendment| amendment.superseded_calls.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    for id in calls {
        let invocation = app.store.trusted_host().invocation(&id)?;
        if invocation.state != s::InvocationState::Failed {
            continue;
        }
        let prepared = decode_prepared(&invocation)?;
        let (head, state) = app.load()?;
        retire_completed(app, &invocation, &prepared, &head, &state, unix_now()?)?;
    }
    Ok(())
}

/// Retire only an exact complete rejected call named by a saved amendment.
/// The amendment supplied explicit resumption separately; this transition never
/// creates a persona-authored continuation or diagnoses unknown rejection text.
pub(crate) fn retire_completed(
    app: &mut Runtime,
    invocation: &s::InvocationRecord,
    prepared: &Prepared,
    head: &s::StoredObject,
    state: &State,
    now: i64,
) -> Result<bool> {
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let Some(meta) = state.work_meta.get(work) else {
        return Ok(false);
    };
    let Some(amendment) = meta.amendments.last() else {
        return Ok(false);
    };
    let Some(captured) = amendment.superseded_calls.get(&invocation.id) else {
        return Ok(false);
    };
    let Some(persona) = state.personas.get(actor) else {
        return Ok(false);
    };
    let current = state.board.work(&w::WorkId(work.clone()))?;
    if invocation.state != s::InvocationState::Failed
        || invocation.expired_at_epoch.is_some()
        || invocation.reconciliation.is_some()
        || prepared.request.profile != Profile::Direct
        || prepared.request.id != invocation.id
        || prepared.work_version.epoch >= current.version.epoch
        || amendment.mandate_revision != current.current_mandate
        || captured.persona != *actor
        || captured.owner_epoch != persona.owner_epoch
        || captured.prepare_hash != invocation.prepare_hash
        || captured.request_sha256 != digest(&invocation.request)?
        || persona.active_invocation.as_deref() != Some(invocation.id.as_str())
        || persona.owner_epoch != prepared.owner_epoch
        || persona.admission_serial != prepared.admission_serial
        || persona.failed_attempt_hold.is_some()
        || !app.invocation_leases_quiescent(state)?
    {
        return Ok(false);
    }
    let evidence = app.terminal_evidence(&invocation.id)?;
    let reference = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let Some(body) = evidence.body["response_body"]["text"].as_str() else {
        return Ok(false);
    };
    let hash = s::sha256_bytes(body.as_bytes());
    if evidence.kind != "runtime-terminal-transport"
        || reference.id != terminal_id(&invocation.id)
        || reference.revision != 1
        || evidence.body["invocation_id"] != invocation.id
        || evidence.body["prepare_hash"] != invocation.prepare_hash
        || evidence.body["prepared_epoch"] != prepared.epoch
        || evidence.body["prompt_sha256"] != prepared.prompt_sha256
        || s::sha256_bytes(prepared.prompt.as_bytes()) != prepared.prompt_sha256
        || receipt["kind"] != "provider_complete"
        || receipt["synthetic"] != false
        || receipt["completion_disposition"] != "ready"
        || receipt["attempt"]["response_completed"] != true
        || receipt["attempt"]["stream_eof"] != true
        || prepared.provider_preparation.as_ref() != Some(&receipt["audit"])
        || receipt["audit"]["invocation_id"] != invocation.id
        || receipt["audit"]["prompt_sha256"] != prepared.prompt_sha256
        || invocation.response_hash.as_deref() != Some(hash.as_str())
        || receipt["inner_sha256"]
            .as_str()
            .or_else(|| receipt["transport_sha256"].as_str())
            != Some(hash.as_str())
        || observability::adoption_error(invocation, &evidence)?["status"] != "recorded"
    {
        return Ok(false);
    }
    let Some(mut exposure) = prepared.exposure.clone() else {
        return Ok(false);
    };
    extend_exposure(&mut exposure, &amendment.exposure);
    let id = composite_id("task-amendment-recovery", &[&invocation.id]);
    require(
        !state.observations.contains_key(&id),
        "amendment recovery identity conflict",
    )?;
    let mut next = state.clone();
    next.observations.insert(id.clone(), Observation {
        id:id.clone(), actor:actor.clone(), work:work.clone(), invocation:Some(invocation.id.clone()),
        kind:"task_amendment_recovery".into(), at_unix:now,
        payload:json!({"terminal":reference,"amendment":amendment.request.id,
            "mandate_revision":amendment.mandate_revision,
            "detail":"A complete decision from before the saved task amendment was rejected. Its original response, failure and charges remain retained. This observation makes no diagnosis of that rejection and replays no operation. Follow the current adopted scope and actual current admission.",
            "response_body_supplied":false,"continuation_changed":false,"budget_reset":false}),
        sources:prepared.sources.clone(), exposure:Some(exposure),
        qualified_roots:Vec::new(), qualification_sources:None,
    });
    let persona = next.personas.get_mut(actor).unwrap();
    persona.active_invocation = None;
    persona.owner_epoch = increment(persona.owner_epoch)?;
    persona
        .focus
        .get_mut(work)
        .unwrap()
        .pending_observations
        .push(id);
    next.operator_events
        .push(json!({"kind":"amended_invocation_retired","work":work,
        "persona":actor,"invocation":invocation.id,"amendment":amendment.request.id,
        "terminal":reference,"continuation_changed":false,"budget_reset":false}));
    app.store
        .trusted_host()
        .commit_runtime_rejected_continuation(
            &head.exact_ref()?,
            state_object(head, &next)?,
            invocation,
            &reference,
        )?;
    Ok(true)
}

pub(crate) fn control(meta: &WorkMeta, work: &w::Work) -> Option<Value> {
    let record = meta.amendments.last()?;
    if record.mandate_revision != work.current_mandate {
        return None;
    }
    Some(
        json!({"id":record.request.id,"operator":record.request.operator,
        "adopted_at_unix":record.at_unix,"mandate_revision":record.mandate_revision,
        "current_scope":Scope::from_input(&record.input),
        "prior_amendments_retained":meta.amendments.len()-1,
        "meaning":"This is the current explicit operator amendment. original_need remains the exact historical intake; this accepted scope records its authorized changes. Prior outcomes, acceptances and completion records are historical and do not establish acceptance or completion of this new mandate. Resources and tool grants are unchanged."}),
    )
}

/// The author's own retained amendment text. This result projection makes no
/// assertion about current admission, completion, or other parties' evidence.
pub(crate) fn recorded_for_operator(meta: &WorkMeta, operator: &str) -> Option<Value> {
    let record = meta.amendments.last()?;
    if meta.intake_operator != operator || record.request.operator != operator {
        return None;
    }
    Some(json!({"id":record.request.id,"operator":operator,
        "adopted_at_unix":record.at_unix,"mandate_revision":record.mandate_revision,
        "saved_scope":Scope::from_input(&record.input),
        "prior_amendments_retained":meta.amendments.len()-1,
        "meaning":"Retained operator-authored amendment. Earlier replies, deliveries and completion records may predate it; they do not establish completion of the amended task. Inspect current task state separately."}))
}
