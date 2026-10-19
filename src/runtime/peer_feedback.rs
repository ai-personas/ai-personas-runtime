//! The recipient chooses which peer feedback needs a continuing disposition.
//! Retention is not a sender commitment, mandate change, or scheduling event.
use super::*;

const MAX_PENDING: usize = 8;
const MAX_RECORDS: usize = 128;
const MAX_PENDING_TEXT_BYTES: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "PeerFeedbackDisposition")]
pub enum Disposition {
    Pending,
    Handled,
    Declined,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    observation: String,
    observation_sha256: String,
    invocation: String,
    disposition: Disposition,
    reason: String,
    at_unix: i64,
    exposure: Exposure,
}

fn pending(focus: &Focus) -> BTreeMap<&str, &Receipt> {
    let mut current = BTreeMap::new();
    for record in &focus.peer_feedback {
        if record.disposition == Disposition::Pending {
            current.insert(record.observation.as_str(), record);
        } else {
            current.remove(record.observation.as_str());
        }
    }
    current
}

pub(super) fn has_pending(focus: &Focus) -> bool {
    !pending(focus).is_empty()
}

fn visible(state: &State, record: &Receipt, a: &f::Admission) -> bool {
    exposure_permitted(state, &record.exposure, a)
        && state
            .observations
            .get(&record.observation)
            .is_some_and(|o| {
                o.actor == a.principal
                    && digest(o).is_ok_and(|sha256| sha256 == record.observation_sha256)
            })
}

pub(super) fn observation_ids(state: &State, focus: &Focus, a: &f::Admission) -> Vec<String> {
    pending(focus)
        .into_values()
        .filter(|record| visible(state, record, a))
        .map(|record| record.observation.clone())
        .collect()
}

pub(super) fn control(
    state: &State,
    focus: &Focus,
    a: &f::Admission,
    supplied: &[String],
    exposure: &mut Exposure,
) -> Result<Value> {
    let records = pending(focus);
    if records.is_empty() {
        return Ok(Value::Null);
    }
    let mut items = Vec::new();
    for record in records.into_values() {
        if visible(state, record, a) && supplied.contains(&record.observation) {
            extend_exposure(exposure, &record.exposure);
            items.push(json!({
                "observation":record.observation,"revision":digest(record)?,
                "invocation":record.invocation,"reason":record.reason,"body_supplied":true
            }));
        } else {
            items.push(json!({"status":"withheld_unavailable","body_supplied":false}));
        }
    }
    Ok(json!({
        "interface":"peer-feedback-disposition-v1","pending":items,
        "rule":"You chose these peer messages as consequential unresolved feedback. Their full permitted bodies remain in actual_observations across reads, waits and local stops. Leave them pending without another operation, or use account_peer_feedback to record handled or declined with a reason. Resolve your pending items before CloseGoal. This is your attributed account, not proof of compliance, sender agreement, requester acceptance or changed authority. Retention creates no wake or funding."
    }))
}

pub(super) fn account(
    state: &mut State,
    prepared: &Prepared,
    observation_id: &str,
    disposition: &Disposition,
    reason: &str,
    now: i64,
) -> Result<()> {
    p::check_id(observation_id, "account_peer_feedback.observation")?;
    text(reason, 4096)?;
    require(
        prepared.request.mode == Mode::Ordinary,
        "peer feedback requires ordinary mode",
    )?;
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let focus = &state.personas[actor].focus[work];
    require(
        focus.peer_feedback.len() < MAX_RECORDS
            && !focus
                .peer_feedback
                .iter()
                .any(|r| r.observation == observation_id && r.invocation == prepared.request.id),
        "peer feedback record bound or duplicate disposition in this response",
    )?;
    let previous = focus
        .peer_feedback
        .iter()
        .rev()
        .find(|r| r.observation == observation_id);
    require(
        previous.is_none_or(|r| r.disposition == Disposition::Pending),
        "peer feedback already has a final disposition",
    )?;
    let observation = state
        .observations
        .get(observation_id)
        .ok_or_else(|| held("peer feedback observation missing"))?;
    let message = state
        .messages
        .get(observation_id)
        .ok_or_else(|| held("peer feedback message missing"))?;
    require(
        observation.actor == *actor
            && observation.work == *work
            && observation.kind == "received_local_persona_message"
            && observation.invocation.as_ref() == Some(&message.invocation)
            && message.recipient == *actor
            && message.work == *work
            && message.sender != *actor
            && state.personas.contains_key(&message.sender)
            && observation.payload["sender"] == message.sender
            && observation.payload["text"] == message.text,
        "peer feedback must be an actual message from another persona in this work",
    )?;
    require(
        *disposition != Disposition::Pending || message.text.len() <= MAX_PENDING_TEXT_BYTES,
        "pending peer feedback text exceeds 16384 bytes; no body is truncated",
    )?;
    let a = admission(&state.personas[actor], now);
    let corpus = corpus(state, &a)?;
    let (projected, _, _) = observation_projection::project(state, observation, &a, &corpus)?;
    let control = request_context::decode(prepared)?.control;
    require(
        s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
            && prepared
                .supplied_observations
                .iter()
                .any(|id| id == observation_id)
            && control["actor"] == *actor
            && control["work_id"] == *work
            && control["invocation_id"] == prepared.request.id
            && control["actual_observations"]
                .as_array()
                .is_some_and(|items| items.contains(&projected)),
        "peer feedback disposition requires its exact full body in this admission",
    )?;
    if let Some(previous) = previous {
        let revision = digest(previous)?;
        require(
            visible(state, previous, &a)
                && control["peer_feedback"]["pending"]
                    .as_array()
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item["observation"] == observation_id
                                && item["revision"] == revision
                                && item["body_supplied"] == true
                        })
                    }),
            "pending peer feedback requires its current supplied attributed disposition",
        )?;
    }
    let receipt = Receipt {
        observation: observation_id.into(),
        observation_sha256: digest(observation)?,
        invocation: prepared.request.id.clone(),
        disposition: disposition.clone(),
        reason: reason.into(),
        at_unix: now,
        exposure: prepared_exposure(prepared)?.clone(),
    };
    let focus = state
        .personas
        .get_mut(actor)
        .unwrap()
        .focus
        .get_mut(work)
        .unwrap();
    focus.peer_feedback.push(receipt);
    let pending_count = pending(focus).len();
    // Reserve one final disposition slot for each pending item; an update or
    // another pending item cannot use the last slots needed to account for it.
    require(
        pending_count <= MAX_PENDING && focus.peer_feedback.len() + pending_count <= MAX_RECORDS,
        "peer feedback retention bound; final disposition slots remain reserved",
    )
}
