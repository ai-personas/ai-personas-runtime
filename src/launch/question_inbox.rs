//! Immutable requester answers, delivered by the existing foreground owner.
use super::*;

const INBOX: &str = "shared-question-answer-inbox-v1";
const INPUT: &str = "shared-question-answer-input-v1";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inbox {
    sequence: u64,
    pending: BTreeMap<u64, String>,
    unavailable: BTreeMap<u64, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Delivery {
    Queued,
    Delivered { at_unix: i64, existing_waits_satisfied: usize },
    Unavailable { at_unix: i64, reason: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    operator: String,
    input: QuestionAnswerInput,
    received_at_unix: i64,
    received_runtime: s::ObjectRef,
    sequence: u64,
    delivery: Delivery,
}

fn id(operator: &str, input: &QuestionAnswerInput) -> Result<String> {
    Ok(format!("shared-question-answer-input:{}", digest(&(
        operator, &input.work, &input.reply.question, &input.reply.id,
    ))?))
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

fn inbox(records: &s::RuntimeInspection<'_, '_>) -> Result<(Option<s::StoredObject>, Inbox)> {
    let head = records.object(INBOX)?;
    let value = head.as_ref().map(|head| {
        require(head.kind == INBOX, "question answer inbox kind changed")?;
        Ok::<_, RuntimeError>(serde_json::from_value(head.body.clone())?)
    }).transpose()?.unwrap_or_default();
    Ok((head, value))
}

fn answer(records: &s::RuntimeInspection<'_, '_>, key: &str) -> Result<(s::StoredObject, Answer)> {
    let head = records.object(key)?.ok_or_else(|| held("queued question answer missing"))?;
    require(head.kind == INPUT, "question answer input kind changed")?;
    let value: Answer = serde_json::from_value(head.body.clone())?;
    value.input.validate()?;
    identity(&value.operator)?;
    require(id(&value.operator, &value.input)? == head.id && value.received_at_unix >= 0
        && value.sequence > 0 && value.received_runtime.id == STATE_ID
        && value.received_runtime.revision > 0, "question answer receipt binding changed")?;
    Ok((head, value))
}

fn view(answer: &Answer, replayed: bool) -> Value {
    let waits = match &answer.delivery {
        Delivery::Delivered { existing_waits_satisfied, .. } => *existing_waits_satisfied,
        _ => 0,
    };
    json!({"work":answer.input.work,"question":answer.input.reply.question,"reply":answer.input.reply.id,
        "received_at_unix":answer.received_at_unix,"delivery":answer.delivery,
        "queued":matches!(answer.delivery, Delivery::Queued),"replayed":replayed,
        "inference_performed":false,"persona_resumed":false,"existing_waits_satisfied":waits,
        "changes_mandate_or_consent":false,"budget_reset":false})
}

pub(super) fn submit(app: &mut Runtime, operator: &str, input: &QuestionAnswerInput, now: i64) -> Result<Value> {
    input.validate()?;
    identity(operator)?;
    require(now >= 0, "negative question answer time")?;
    let key = id(operator, input)?;
    let result = app.store.work_input_transaction(None, |runtime, launch, records| {
        let mut candidate = Runtime::decode_inspection_state(&runtime)?;
        admission_extension::authorize(&candidate, &decode_saved(&launch)?, operator)?;
        questions::operator_view(&candidate, operator, &input.work, now)?;
        if records.object(&key)?.is_some() {
            let (_, old) = answer(records, &key)?;
            require(old.operator == operator && digest(&old.input)? == digest(input)?,
                "question answer identity conflict")?;
            return Ok::<_, RuntimeError>((view(&old, true), Vec::new()));
        }
        // Validate the exact visible question and reply on a private candidate.
        // No observation or event enters an already admitted model input.
        questions::operator_reply_received(&mut candidate, operator, &input.work, &input.reply, now, now)?;
        let (head, mut inbox) = inbox(records)?;
        inbox.sequence = increment(inbox.sequence)?;
        let answer = Answer { operator: operator.into(), input: input.clone(), received_at_unix: now,
            received_runtime: runtime.exact_ref()?, sequence: inbox.sequence, delivery: Delivery::Queued };
        inbox.pending.insert(answer.sequence, key.clone());
        Ok((view(&answer, false), vec![change(None, &key, INPUT, &answer)?,
            change(head.as_ref(), INBOX, INBOX, &inbox)?]))
    })??;
    if result["queued"] == true {
        if let Some(owner) = app.store.try_execution_owner_without_recovery()? {
            deliver(app, &owner, now)?;
            return app.store.work_input_transaction(None, |_, _, records| {
                let (_, retained) = answer(records, &key)?;
                Ok::<_, RuntimeError>((view(&retained, result["replayed"] == true), Vec::new()))
            })?;
        }
    }
    Ok(result)
}

pub(super) fn has_pending(app: &mut Runtime) -> Result<bool> {
    let Some(head) = app.store.read(INBOX)? else { return Ok(false); };
    require(head.kind == INBOX, "question answer inbox kind changed")?;
    let inbox: Inbox = serde_json::from_value(head.body)?;
    Ok(!inbox.pending.is_empty())
}

pub(super) fn deliver(app: &mut Runtime, owner: &s::ExecutionOwner, now: i64) -> Result<usize> {
    if app.has_background_host_commands() { return Ok(0); }
    let Some(head) = app.store.read(INBOX)? else { return Ok(0); };
    let pending: Inbox = serde_json::from_value(head.body)?;
    if pending.pending.is_empty() { return Ok(0); }
    app.store.work_input_transaction(Some(owner), |runtime, launch, records| {
        let mut state = Runtime::decode_inspection_state(&runtime)?;
        if !check_invocation_leases(&state, &BTreeSet::new(), |id| {
            records.invocation(id)?.ok_or_else(|| held("question answer delivery lease missing"))
        })? { return Ok::<_, RuntimeError>((0, Vec::new())); }
        let saved = decode_saved(&launch)?;
        let (head, mut inbox) = inbox(records)?;
        let mut changes = Vec::new();
        let mut delivered = 0;
        for (sequence, key) in std::mem::take(&mut inbox.pending) {
            let (old, mut answer) = answer(records, &key)?;
            require(answer.sequence == sequence && matches!(answer.delivery, Delivery::Queued),
                "question answer queue binding changed")?;
            // An interrupted command still owns its effect after restart.
            if state.host_command_jobs.values().any(|job| {
                job.work == answer.input.work && job.observation.is_none()
            }) {
                inbox.pending.insert(sequence, key);
                continue;
            }
            let mut candidate = state.clone();
            let install = (|| {
                admission_extension::authorize(&candidate, &saved, &answer.operator)?;
                let before: usize = candidate.board.works().values().map(|work| work.events.len()).sum();
                let replayed = questions::operator_reply_received(&mut candidate, &answer.operator,
                    &answer.input.work, &answer.input.reply, answer.received_at_unix, now)?;
                if !replayed {
                    candidate.operator_events.push(json!({"kind":"shared_question_reply","operator":answer.operator,
                        "work":answer.input.work,"question":answer.input.reply.question,"reply":answer.input.reply.id,
                        "received_at_unix":answer.received_at_unix,"at_unix":now}));
                }
                Ok::<_, RuntimeError>(candidate.board.works().values().map(|work| work.events.len()).sum::<usize>()
                    .saturating_sub(before))
            })();
            answer.delivery = match install {
                Ok(existing_waits_satisfied) => {
                    state = candidate;
                    delivered += 1;
                    Delivery::Delivered { at_unix: now, existing_waits_satisfied }
                }
                Err(error) => {
                    inbox.unavailable.insert(sequence, key.clone());
                    Delivery::Unavailable { at_unix: now, reason: error.to_string() }
                }
            };
            changes.push(change(Some(&old), &key, INPUT, &answer)?);
        }
        if changes.is_empty() { return Ok((0, changes)); }
        changes.push(change(head.as_ref(), INBOX, INBOX, &inbox)?);
        if delivered > 0 {
            changes.push(s::Change { expected_revision: Some(runtime.revision),
                object: state_object(&runtime, &state)?, depends_on: Vec::new() });
        }
        Ok((delivered, changes))
    })?
}

pub(super) fn append_view(records: &s::RuntimeInspection<'_, '_>, operator: &str, work: &str, value: &mut Value) -> Result<()> {
    let (_, inbox) = inbox(records)?;
    let mut inputs = Vec::new();
    for key in inbox.pending.values().chain(inbox.unavailable.values()) {
        let (_, answer) = answer(records, key)?;
        if answer.operator == operator && answer.input.work == work {
            inputs.push(view(&answer, true));
        }
    }
    value["answer_delivery"] = json!(inputs);
    Ok(())
}
