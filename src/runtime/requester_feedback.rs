//! Requester direction remains available across reads and accepted dispositions.
//! A disposition clears the pending queue, without removing its original body.
use super::*;

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

fn retained_observation<'a>(
    state: &'a State,
    work: &str,
    a: &f::Admission,
    record: &Receipt,
) -> Option<&'a Observation> {
    let observation = state.observations.get(&record.observation)?;
    let origin = observation.exposure.as_ref()?;
    (observation.actor == a.principal
        && observation.work == work
        && observation.kind == "received_local_operator_feedback"
        && observation.invocation.is_none()
        && digest(observation).is_ok_and(|hash| hash == record.observation_sha256)
        && exposure_permitted(state, origin, a)
        && exposure_permitted(state, &record.exposure, a))
    .then_some(observation)
}

/// Retain the exact direction even after its author records an account of it.
/// Historical supply does not make the item pending again or verify compliance.
pub(super) fn observation_ids(
    state: &State,
    work: &str,
    focus: &Focus,
    a: &f::Admission,
) -> Vec<String> {
    let mut seen = focus.pending_requester_feedback.clone();
    let mut ids: Vec<_> = seen.iter().cloned().collect();
    for record in &focus.feedback_dispositions {
        if retained_observation(state, work, a, record).is_some()
            && seen.insert(record.observation.clone())
        {
            ids.push(record.observation.clone());
        }
    }
    ids.sort_by(|left, right| {
        state
            .observations
            .get(left)
            .map(|o| o.at_unix)
            .cmp(&state.observations.get(right).map(|o| o.at_unix))
            .then_with(|| left.cmp(right))
    });
    ids
}

pub(super) fn control(
    state: &State,
    work: &str,
    focus: &Focus,
    a: &f::Admission,
    supplied: &[String],
    exposure: &mut Exposure,
) -> Result<Value> {
    if focus.pending_requester_feedback.is_empty() && focus.feedback_dispositions.is_empty() {
        return Ok(Value::Null);
    }
    let directions = observation_ids(state, work, focus, a);
    let latest_received_at = directions
        .iter()
        .filter_map(|id| state.observations.get(id).map(|o| o.at_unix))
        .max();
    let latest_received: Vec<_> = directions
        .iter()
        .filter_map(|id| {
            let observation = state.observations.get(id)?;
            (Some(observation.at_unix) == latest_received_at).then(|| {
                json!({
                    "observation":id,"received_at_unix":observation.at_unix,
                    "body_supplied":supplied.contains(id),
                    "pending":focus.pending_requester_feedback.contains(id)
                })
            })
        })
        .collect();
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
    // Keep the accepted account and its reason alongside the exact original
    // feedback body. A previous authored plan is not a success receipt.
    let mut recent = Vec::new();
    let mut omitted = false;
    for record in focus.feedback_dispositions.iter().rev() {
        let Some(observation) = retained_observation(state, work, a, record) else {
            omitted = true;
            continue;
        };
        let value = json!({
            "observation":record.observation,"invocation":record.invocation,
            "disposition":record.disposition,"at_unix":record.at_unix,
            "reason":record.reason,"feedback_body_supplied":supplied.contains(&record.observation)
        });
        recent.push(value);
        extend_exposure(
            exposure,
            observation.exposure.as_ref().expect("validated lineage"),
        );
        extend_exposure(exposure, &record.exposure);
    }
    Ok(json!({
        "interface":"requester-feedback-disposition-v2",
        "pending":pending,
        "pending_state":if focus.pending_requester_feedback.is_empty() { "none_pending" } else { "pending" },
        "recent_dispositions":recent,"recent_dispositions_omitted":omitted,
        "latest_received":latest_received,
        "direction_order":"Original feedback bodies in actual_observations are ordered by received_at_unix, oldest first, independently of pending/disposition status. latest_received identifies the most recently received timestamp group; equal timestamps do not establish an order within that group. Use later explicit corrections where they supersede older direction, while retaining the original attribution and history.",
        "accounting":"pending is the current tracked queue for this actor and work. none_pending means no tracked item currently needs a disposition. recent_dispositions contains every available accepted account and its exact reason, newest first. feedback_body_supplied identifies original feedback bodies retained in actual_observations after disposition. Use this history to keep requester direction available while continuing the work; a disposition does not erase the direction or prove that a planned edit was completed. Historical items are not newly received and must not be resolved again. Later explicit corrections can supersede older directions. Unavailable history establishes no absence of other obligations. Handled records the persona's account, not verified compliance or requester acceptance.",
        "attribution":"Each item's sender identifies its author. Local operator input is attributed to the intake operator; CONTROL.requester identifies the need's requester, who may be a different persona. No item establishes that persona's assent or acceptance.",
        "rule":"The full permitted items are in actual_observations. For resolution copy pending[].observation exactly, including its full prefix; actual_observations[].payload.id is only the requester's input label, not an observation reference. These requester directions remain current across reads, waits and local stops automatically. For an unresolved item, omit resolve_requester_feedback entirely; pending is not a valid disposition for that action. Use it only when recording handled or declined with a reason. Feedback changes no mandate, authority or resources; a disposition is your attributed account, not proof of compliance or requester acceptance."
    }))
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

/// A complete account of the requester's own tracked inputs and the persona's
/// attributed dispositions. Never a verdict on whether a requested edit exists.
pub(super) fn project_for_recipient(
    state: &State,
    work_id: &str,
    recipient: &str,
    now: i64,
    _max_bytes: usize,
) -> Result<Value> {
    let view = |items: Vec<Value>, truncated: bool| {
        json!({
            "items":items,"truncated":truncated,
            "scope":"Currently permitted tracked requester feedback and attributed persona accounts. Handled does not establish compliance, an artifact change or requester acceptance; omitted history establishes no absence of obligations."
        })
    };
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    if !state.config.local_recipients.contains(recipient) {
        return Ok(view(Vec::new(), false));
    }
    let mut candidates = Vec::new();
    for actor in work.participants.keys() {
        let Some(persona) = state.personas.get(&actor.0) else {
            continue;
        };
        let Some(focus) = persona.focus.get(work_id) else {
            continue;
        };
        let mut a = admission(persona, now);
        a.audience = BTreeSet::from([recipient.into()]);
        let inherited = work_exposure(state, work_id)?;
        if !exposure_permitted(state, &inherited, &a) {
            continue;
        }
        let ids: BTreeSet<_> = focus
            .pending_requester_feedback
            .iter()
            .chain(focus.feedback_dispositions.iter().map(|r| &r.observation))
            .collect();
        for id in ids {
            let Some(observation) = state.observations.get(id) else {
                continue;
            };
            if observation.actor != actor.0
                || observation.work != work_id
                || observation.kind != "received_local_operator_feedback"
                || observation.invocation.is_some()
                || observation.payload["sender"] != recipient
                || observation.payload["recipient"] != actor.0
                || observation.payload["work"] != work_id
                || !observation.sources.is_empty()
                || !observation.qualified_roots.is_empty()
            {
                continue;
            }
            let Some(exposure) = observation.exposure.as_ref() else {
                continue;
            };
            // Do not carry restrictions from one unrelated feedback item into
            // the next. Every body has its own retained permission ancestry.
            let mut permitted = inherited.clone();
            extend_exposure(&mut permitted, exposure);
            if !exposure_permitted(state, &permitted, &a) {
                continue;
            }
            let pending = focus.pending_requester_feedback.contains(id);
            let receipt = focus
                .feedback_dispositions
                .iter()
                .rev()
                .find(|receipt| receipt.observation == *id);
            let receipt = receipt.filter(|receipt| {
                exposure_permitted(state, &receipt.exposure, &a)
                    && digest(observation).is_ok_and(|hash| hash == receipt.observation_sha256)
            });
            candidates.push((
                !pending,
                std::cmp::Reverse(observation.at_unix),
                id,
                persona,
                observation,
                pending,
                receipt,
            ));
        }
    }
    candidates.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
    let mut items = Vec::new();
    for (_, _, id, persona, observation, pending, receipt) in candidates {
        let status = if pending {
            "pending"
        } else {
            match receipt.map(|r| &r.disposition) {
                Some(Disposition::Handled) => "handled",
                Some(Disposition::Declined) => "declined",
                None => "disposition_unavailable",
            }
        };
        let item = json!({"persona":persona.seed.id,"label":persona.seed.label,
            "observation":id,"input_id":observation.payload["id"],
            "received_at_unix":observation.at_unix,"status":status,
            "feedback_text":observation.payload["text"],"feedback_body_supplied":true,
            "account":receipt.map(|r|json!({"invocation":r.invocation,
                "at_unix":r.at_unix,"reason":r.reason})),
            "compliance_verified":false,"requester_acceptance_established":false});
        items.push(item);
    }
    Ok(view(items, false))
}
