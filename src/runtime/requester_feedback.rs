//! Current requester direction survives intermediate reads and other steps.
//! Only a fresh, explicit primary disposition clears a retained feedback item.
use super::*;

pub(super) const MAX_PENDING: usize = 8;
pub(super) const MAX_DISPOSITIONS: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "RequesterFeedbackDisposition")]
pub enum Disposition {
    Handled,
    Declined,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub observation: String,
    pub observation_sha256: String,
    pub invocation: String,
    pub disposition: Disposition,
    pub reason: String,
    pub at_unix: i64,
    pub exposure: Exposure,
}

pub(super) fn check_capacity(focus: &Focus) -> Result<()> {
    require(
        focus.pending_requester_feedback.len() < MAX_PENDING
            && focus.pending_requester_feedback.len() + focus.feedback_dispositions.len()
                < MAX_DISPOSITIONS,
        "requester feedback disposition capacity exhausted",
    )
}

pub(super) fn control(focus: &Focus, supplied: &[String]) -> Value {
    if focus.pending_requester_feedback.is_empty() {
        return Value::Null;
    }
    let pending: Vec<Value> = focus
        .pending_requester_feedback
        .iter()
        .map(|id| {
            if supplied.contains(id) {
                json!({"observation":id,"body_supplied":true})
            } else {
                json!({"status":"withheld_unavailable","body_supplied":false})
            }
        })
        .collect();
    json!({
        "interface":"requester-feedback-disposition-v1",
        "pending":pending,
        "rule":"The full permitted items are in actual_observations. For resolution copy pending[].observation exactly, including its full prefix; actual_observations[].payload.id is only the requester's input label, not an observation reference. These requester directions remain current across reads, waits and local stops automatically. For an unresolved item, omit resolve_requester_feedback entirely; pending is not a valid disposition for that action. Use it only when recording handled or declined with a reason. Feedback changes no mandate, authority or resources; a disposition is your attributed account, not proof of compliance or requester acceptance."
    })
}

pub(super) fn resolve(
    state: &mut State,
    prepared: &Prepared,
    observation_id: &str,
    disposition: &Disposition,
    reason: &str,
    now: i64,
) -> Result<()> {
    p::check_id(observation_id, "resolve_requester_feedback.observation")?;
    text(reason, 4096)?;
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let focus = &state.personas[actor].focus[work];
    require(
        focus.feedback_dispositions.len() < MAX_DISPOSITIONS,
        "requester feedback disposition capacity exhausted",
    )?;
    require(
        focus.pending_requester_feedback.contains(observation_id),
        "requester feedback observation is not pending for this actor; copy CONTROL.requester_feedback.pending[].observation exactly, not the payload.id input label",
    )?;
    let observation = state
        .observations
        .get(observation_id)
        .ok_or_else(|| held("requester feedback observation missing"))?;
    require(
        observation.actor == *actor
            && observation.work == *work
            && observation.kind == "received_local_operator_feedback"
            && observation.invocation.is_none(),
        "requester feedback binding mismatch",
    )?;
    let a = admission(&state.personas[actor], now);
    let corpus = corpus(state, &a)?;
    let (projected, _, _) = observation_projection::project(state, observation, &a, &corpus)?;
    let control = request_context::decode(prepared)?.control;
    require(
        prepared
            .supplied_observations
            .iter()
            .any(|id| id == observation_id)
            && control["actor"] == *actor
            && control["work_id"] == *work
            && control["invocation_id"] == prepared.request.id
            && control["actual_observations"]
                .as_array()
                .is_some_and(|items| items.contains(&projected))
            && control["requester_feedback"]["pending"]
                .as_array()
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item["observation"] == observation_id && item["body_supplied"] == true
                    })
                }),
        "requester feedback disposition requires its exact full body in this admission",
    )?;
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
    focus.pending_requester_feedback.remove(observation_id);
    focus.feedback_dispositions.push(receipt);
    Ok(())
}
