//! Attributed intake-operator feedback within existing work, without a new need.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackInput {
    pub id: String,
    pub work: String,
    pub persona: String,
    pub text: String,
    /// Exact earlier feedback observation IDs whose directions this input replaces.
    /// Original observations and any persona accounts remain retained history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
}

impl FeedbackInput {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.id, &self.work, &self.persona] {
            identity(value)?;
        }
        let mut seen = BTreeSet::new();
        for id in &self.supersedes {
            identity(id)?;
            require(seen.insert(id), "duplicate superseded feedback observation")?;
        }
        require(
            !self.text.trim().is_empty(),
            "feedback text must be nonblank",
        )
    }
}

const INBOX: &str = "requester-feedback-inbox-v1";
const INPUT_KIND: &str = "requester-feedback-input-v1";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inbox {
    sequence: u64,
    pending: BTreeMap<u64, String>,
    unavailable: BTreeMap<u64, String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Delivery {
    Queued,
    Delivered { at_unix: i64 },
    Unavailable { at_unix: i64, reason: String },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputRecord {
    operator: String,
    input: FeedbackInput,
    observation: Observation,
    sequence: u64,
    received_context: Option<ReceivedContext>,
    delivery: Delivery,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceivedContext {
    runtime: s::ObjectRef,
    launch: s::ObjectRef,
    epoch: u64,
    calls: Vec<ReceivedCall>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceivedCall {
    persona: String,
    invocation: String,
    state: s::InvocationState,
    request_sha256: String,
}

fn input_id(operator: &str, input: &FeedbackInput) -> Result<String> {
    Ok(format!("requester-feedback-input:{}", digest(&(operator, &input.id))?))
}

fn inbox(records: &s::RuntimeInspection<'_, '_>) -> Result<(Option<s::StoredObject>, Inbox)> {
    let head = records.object(INBOX)?;
    let value = head.as_ref().map(|head| {
        require(head.kind == INBOX, "feedback inbox kind changed")?;
        Ok::<_, RuntimeError>(serde_json::from_value(head.body.clone())?)
    }).transpose()?.unwrap_or_default();
    Ok((head, value))
}

fn entry(records: &s::RuntimeInspection<'_, '_>, id: &str) -> Result<(s::StoredObject, InputRecord)> {
    let head = records.object(id)?.ok_or_else(|| held("queued feedback record missing"))?;
    require(head.kind == INPUT_KIND, "feedback input kind changed")?;
    let value: InputRecord = serde_json::from_value(head.body.clone())?;
    value.input.validate()?;
    require(input_id(&value.operator, &value.input)? == head.id,
        "feedback input identity changed")?;
    require(value.observation.id == bounded_composite_id("operator-feedback", &[&value.operator, &value.input.id])
        && value.observation.actor == value.input.persona && value.observation.work == value.input.work
        && value.observation.invocation.is_none() && value.observation.at_unix >= 0
        && value.observation.kind == "received_local_operator_feedback"
        && value.sequence > 0 && value.received_context.is_some(), "feedback receipt binding changed")?;
    Ok((head, value))
}

fn change<T: Serialize>(old: Option<&s::StoredObject>, id: &str, kind: &str, body: &T) -> Result<s::Change> {
    Ok(s::Change {
        expected_revision: old.map(|head| head.revision),
        object: s::StoredObject { id: id.into(), kind: kind.into(),
            revision: old.map(|head| increment(head.revision)).transpose()?.unwrap_or(1),
            body: serde_json::to_value(body)? },
        depends_on: Vec::new(),
    })
}

fn authorize(state: &State, saved: &Saved, operator: &str, input: &FeedbackInput, now: i64) -> Result<Exposure> {
    input.validate()?;
    identity(operator)?;
    require(now >= 0, "negative feedback time")?;
    admission_extension::authorize(state, saved, operator)?;
    require(saved.works.contains_key(&input.work), "feedback requires an existing saved work")?;
    let meta = state.work_meta.get(&input.work).ok_or_else(|| held("work unavailable"))?;
    require(meta.intake_operator == operator, "feedback requires the work's intake operator")?;
    let persona = state.personas.get(&input.persona).ok_or_else(|| held("persona unavailable"))?;
    require(persona.focus.contains_key(&input.work), "feedback recipient is outside this requester's work")?;
    let exposure = work_exposure(state, &input.work)?;
    require(exposure_permitted(state, &exposure, &admission(persona, now)), "feedback work unavailable")?;
    Ok(exposure)
}

fn observation(
    state: &State, operator: &str, input: &FeedbackInput, now: i64,
    exposure: Exposure, queued: &[InputRecord],
) -> Result<Observation> {
    let id = bounded_composite_id("operator-feedback", &[operator, &input.id]);
    let mut superseded = Vec::new();
    for previous in &input.supersedes {
        let original = state.observations.get(previous).or_else(|| {
            queued.iter().find(|record| record.observation.id == *previous).map(|record| &record.observation)
        }).ok_or_else(|| held("superseded feedback observation missing"))?;
        require(original.id == *previous && previous != &id
            && original.kind == "received_local_operator_feedback"
            && original.actor == input.persona && original.work == input.work
            && original.invocation.is_none() && original.payload["sender"] == operator
            && original.payload["recipient"] == input.persona && original.payload["work"] == input.work,
            "supersedes must name this requester's exact feedback to this persona and work")?;
        superseded.push(history::records::observation_identity(original)
            .ok_or_else(|| held("superseded feedback identity unavailable"))?.0);
    }
    let mut payload = json!({"id":input.id,"sender":operator,"recipient":input.persona,
        "work":input.work,"text":input.text,"origin":"local_operator_input",
        "independently_authenticated":false,"changes_mandate_or_authority":false});
    if !superseded.is_empty() { payload["supersedes"] = json!(superseded); }
    Ok(Observation { id, actor: input.persona.clone(), work: input.work.clone(), invocation: None,
        kind: "received_local_operator_feedback".into(), at_unix: now, payload,
        sources: Vec::new(), exposure: Some(exposure), qualified_roots: Vec::new(), qualification_sources: None })
}

fn current_targets(state: &State, input: &FeedbackInput, queued: &[InputRecord]) -> Result<()> {
    let focus = &state.personas[&input.persona].focus[&input.work];
    for previous in &input.supersedes {
        require(!focus.feedback_replacements.contains_key(previous)
            && !queued.iter().any(|record| record.input.supersedes.contains(previous))
            && (focus.pending_requester_feedback.contains(previous)
                || focus.feedback_dispositions.iter().any(|record| &record.observation == previous)
                || queued.iter().any(|record| &record.observation.id == previous)),
            "supersedes must name a current or queued direction without a later replacement; inspect its successor")?;
    }
    Ok(())
}

fn view(state: &State, record: &InputRecord, receipt: Option<s::ObjectRef>, replayed: bool) -> Value {
    let queued = matches!(record.delivery, Delivery::Queued);
    let delivered = matches!(record.delivery, Delivery::Delivered { .. });
    let focus = state.personas.get(&record.input.persona).and_then(|p| p.focus.get(&record.input.work));
    json!({"observation":record.observation.id,"work":record.input.work,"persona":record.input.persona,
        "replayed":replayed,"receipt":receipt,"delivery":record.delivery,"queued":queued,
        "received_context":record.received_context,
        "supply":if queued {"retained for delivery at a settled decision boundary"} else if delivered {
            "pending fresh permitted admission"} else {"not delivered; inspect delivery reason"},
        "persona_resumed":false,"inference_performed":false,"budget_reset":false,
        "retained_until_explicit_disposition":queued || (delivered && focus.is_some_and(|f| f.pending_requester_feedback.contains(&record.observation.id))),
        "superseded_observations":record.input.supersedes,
        "current_direction":if delivered {focus.map(|f| !f.feedback_replacements.contains_key(&record.observation.id))} else {None},
        "original_observations_and_accounts_retained":true})
}

/// Persist the exact input independently of execution ownership. An idle owner
/// can deliver immediately; a running owner delivers after its admitted calls
/// settle. Neither intake nor delivery supplies a body to an existing prompt.
pub fn feedback(app: &mut Runtime, operator: &str, input: &FeedbackInput, now: i64) -> Result<Value> {
    let enqueue = |runtime: s::StoredObject, launch: s::StoredObject, records: &s::RuntimeInspection<'_, '_>| {
        let state = Runtime::decode_inspection_state(&runtime)?;
        let saved = decode_saved(&launch)?;
        let exposure = authorize(&state, &saved, operator, input, now)?;
        let id = input_id(operator, input)?;
        if records.object(&id)?.is_some() {
            let (head, retained) = entry(records, &id)?;
            require(retained.operator == operator && retained.input == *input, "feedback identity conflict")?;
            return Ok::<_, RuntimeError>((view(&state, &retained, Some(head.exact_ref()?), true), Vec::new()));
        }
        let (old, mut inbox) = inbox(records)?;
        let queued = inbox.pending.values().map(|id| entry(records, id).map(|(_, value)| value)).collect::<Result<Vec<_>>>()?;
        let observation = observation(&state, operator, input, now, exposure, &queued)?;
        if let Some(existing) = state.observations.get(&observation.id) {
            require(existing.kind == observation.kind && existing.actor == observation.actor
                && existing.work == observation.work && existing.invocation.is_none()
                && existing.payload == observation.payload, "feedback identity conflict")?;
            let retained = InputRecord { operator: operator.into(), input: input.clone(), observation: existing.clone(),
                sequence: 0, received_context: None, delivery: Delivery::Delivered { at_unix: existing.at_unix } };
            return Ok((view(&state, &retained, None, true), Vec::new()));
        }
        current_targets(&state, input, &queued)?;
        inbox.sequence = increment(inbox.sequence)?;
        let mut calls = Vec::new();
        for (actor, persona) in &state.personas {
            if let Some(id) = &persona.active_invocation {
                let invocation = records.invocation(id)?.ok_or_else(|| held("feedback intake lease missing"))?;
                calls.push(ReceivedCall { persona: actor.clone(), invocation: id.clone(),
                    state: invocation.state, request_sha256: digest(&invocation.request)? });
            }
        }
        let record = InputRecord { operator: operator.into(), input: input.clone(), observation,
            sequence: inbox.sequence, received_context: Some(ReceivedContext {
                runtime: runtime.exact_ref()?, launch: launch.exact_ref()?, epoch: records.epoch(), calls,
            }), delivery: Delivery::Queued };
        inbox.pending.insert(record.sequence, id.clone());
        let input_change = change(None, &id, INPUT_KIND, &record)?;
        let result = view(&state, &record, Some(input_change.object.exact_ref()?), false);
        Ok((result, vec![input_change, change(old.as_ref(), INBOX, INBOX, &inbox)?]))
    };
    let result = app.store.feedback_transaction(None, enqueue)??;
    if result["queued"] != true { return Ok(result); }
    if let Some(owner) = app.store.try_execution_owner_without_recovery()? {
        deliver(app, &owner, now)?;
        let id = input_id(operator, input)?;
        return app.store.feedback_transaction(None, |runtime, _, records| {
            let state = Runtime::decode_inspection_state(&runtime)?;
            let (head, record) = entry(records, &id)?;
            Ok::<_, RuntimeError>((view(&state, &record, Some(head.exact_ref()?), result["replayed"] == true), Vec::new()))
        })?;
    }
    Ok(result)
}

fn install(state: &mut State, saved: &Saved, record: &InputRecord, now: i64) -> Result<()> {
    let input = &record.input;
    authorize(state, saved, &record.operator, input, now)?;
    let original = &record.observation;
    let expected = observation(state, &record.operator, input, original.at_unix,
        original.exposure.clone().ok_or_else(|| held("queued feedback provenance missing"))?, &[])?;
    require(digest(&expected)? == digest(original)?, "queued feedback observation binding changed")?;
    if let Some(existing) = state.observations.get(&original.id) {
        return require(digest(existing)? == digest(original)?, "feedback observation conflict");
    }
    current_targets(state, input, &[])?;
    let references: Vec<p::HistoryRecordReference> = serde_json::from_value(
        original.payload.get("supersedes").cloned().unwrap_or_else(|| json!([])))?;
    state.observations.insert(original.id.clone(), original.clone());
    let focus = state.personas.get_mut(&input.persona).unwrap().focus.get_mut(&input.work).unwrap();
    for reference in &references {
        let p::HistoryRecordReference::Observation { observation_id, .. } = reference else {
            return Err(held("feedback replacement requires an observation"));
        };
        focus.feedback_replacements.insert(observation_id.clone(), requester_feedback::Replacement {
            reference: reference.clone(), successor: original.id.clone(), at_unix: original.at_unix });
        focus.pending_requester_feedback.remove(observation_id);
        focus.pending_observations.retain(|id| id != observation_id);
    }
    focus.pending_observations.push(original.id.clone());
    focus.pending_requester_feedback.insert(original.id.clone());
    requester_wait::satisfy(state, &original.id, &record.operator, now)?;
    state.operator_events.push(json!({"kind":"work_feedback","operator":record.operator,
        "observation":original.id,"work":input.work,"recipient":input.persona,"at_unix":now,
        "received_at_unix":original.at_unix,"superseded_feedback":references}));
    Ok(())
}

/// The coordinator calls this before another admission and before returning its
/// final result. Failed/uncertain leases keep their dispositions; no recovery,
/// cancellation, grant or resumption is manufactured by delivering user input.
pub(super) fn deliver(app: &mut Runtime, owner: &s::ExecutionOwner, now: i64) -> Result<usize> {
    if app.has_background_host_commands() { return Ok(0); }
    let Some(head) = app.store.read(INBOX)? else { return Ok(0) };
    let pending: Inbox = serde_json::from_value(head.body)?;
    if pending.pending.is_empty() { return Ok(0); }
    app.store.feedback_transaction(Some(owner), |runtime, launch, records| {
        let mut state = Runtime::decode_inspection_state(&runtime)?;
        if !check_invocation_leases(&state, &BTreeSet::new(), |id| {
            records.invocation(id)?.ok_or_else(|| held("feedback delivery lease missing"))
        })? { return Ok::<_, RuntimeError>((0, Vec::new())); }
        let saved = decode_saved(&launch)?;
        let (old, mut inbox) = inbox(records)?;
        let mut changes = Vec::new();
        let mut delivered = 0;
        for (sequence, id) in std::mem::take(&mut inbox.pending) {
            let (head, mut record) = entry(records, &id)?;
            require(record.sequence == sequence && matches!(record.delivery, Delivery::Queued),
                "feedback queue binding changed")?;
            // A restarted caller has no in-memory worker handles. Retained
            // unsettled commands still own their effects and must be reconciled
            // independently; an input receipt must not change their task scope.
            if state.host_command_jobs.values().any(|job| {
                job.work == record.input.work && job.observation.is_none()
            }) {
                inbox.pending.insert(sequence, id);
                continue;
            }
            let mut candidate = state.clone();
            record.delivery = match install(&mut candidate, &saved, &record, now) {
                Ok(()) => { state = candidate; delivered += 1; Delivery::Delivered { at_unix: now } },
                Err(error) => { inbox.unavailable.insert(sequence, id.clone());
                    Delivery::Unavailable { at_unix: now, reason: error.to_string() } },
            };
            changes.push(change(Some(&head), &id, INPUT_KIND, &record)?);
        }
        if changes.is_empty() { return Ok((0, changes)); }
        changes.push(change(old.as_ref(), INBOX, INBOX, &inbox)?);
        if delivered > 0 {
            changes.push(s::Change { expected_revision: Some(runtime.revision),
                object: state_object(&runtime, &state)?, depends_on: Vec::new() });
        }
        Ok((delivered, changes))
    })?
}

pub(super) fn append_view(
    output: &mut Output, state: &State, operator: &str, records: &s::RuntimeInspection<'_, '_>,
) -> Result<()> {
    let (_, inbox) = inbox(records)?;
    let mut queued = Vec::new();
    for id in inbox.pending.values().chain(inbox.unavailable.values()) {
        let (head, record) = entry(records, id)?;
        if record.operator == operator && record.input.work == output.work_id {
            queued.push((head, record));
        }
    }
    if queued.is_empty() { return Ok(()); }
    let replacements: BTreeMap<_, _> = queued.iter().filter(|(_, r)| matches!(r.delivery, Delivery::Queued))
        .flat_map(|(_, r)| r.input.supersedes.iter().map(|id| (id.clone(), r.observation.id.clone()))).collect();
    let feedback = output.requester_feedback.get_or_insert_with(|| json!({
        "items":[],"truncated":false,"scope":"Retained inputs with no currently projected persona account."
    }));
    let items = feedback["items"].as_array_mut().ok_or_else(|| held("requester feedback items absent"))?;
    for item in items.iter_mut() {
        if let Some(successor) = item["observation"].as_str().and_then(|id| replacements.get(id)) {
            item["replacement_queued_by"] = json!(successor);
            item["queued_current_direction"] = json!(false);
        }
    }
    queued.sort_by_key(|(_, record)| record.sequence);
    for (head, record) in queued {
        let waiting = matches!(record.delivery, Delivery::Queued);
        items.push(json!({"observation":record.observation.id,"persona":record.input.persona,
            "label":state.personas.get(&record.input.persona).map(|p| &p.seed.label),
            "feedback_text":record.input.text,"status":if waiting {"queued"} else {"delivery_unavailable"},
            "delivery":record.delivery,"receipt":head.exact_ref()?,"current_direction":null,
            "queued_current_direction":waiting && !replacements.contains_key(&record.observation.id),
            "replacement_queued_by":replacements.get(&record.observation.id),
            "received_at_unix":record.observation.at_unix,"account":null,
            "meaning":"Retained requester input; not yet a delivered runtime observation or supplied persona context."}));
    }
    Ok(())
}
