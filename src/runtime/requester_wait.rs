//! Only a newly deposited, attributed requester input can satisfy this route.
//! It is not a persona message and does not fabricate an inference invocation.
use super::*;

pub(super) fn matches(observation: &Observation, work: &str, actor: &str, requester: &str) -> bool {
    observation.work == work
        && observation.actor == actor
        && observation.invocation.is_none()
        && match observation.kind.as_str() {
            "received_local_operator_feedback" => {
                observation.payload["sender"] == requester
                    && observation.payload["recipient"] == actor
                    && observation.payload["origin"] == "local_operator_input"
            }
            "shared_question_requester_reply" => {
                observation.payload["attribution"]["author"] == requester
                    && observation.payload["attribution"]["origin"] == "local_requester_input"
                    && observation.payload["attribution"]["invocation"].is_null()
            }
            _ => false,
        }
}

pub(super) fn satisfy(state: &mut State, id: &str, requester: &str, now: i64) -> Result<()> {
    let observation = state
        .observations
        .get(id)
        .ok_or_else(|| held("new requester observation absent"))?
        .clone();
    let work = &observation.work;
    let actor = &observation.actor;
    require(
        matches(&observation, work, actor, requester)
            // A queued input retains its arrival time. This function is called
            // only when its observation is first installed, not on inbox replay.
            && observation.at_unix <= now
            && state.work_meta[work].intake_operator == requester,
        "requester wait requires a newly deposited bound input",
    )?;
    // A string shared by a persona and the requester must not let human input
    // satisfy a route advertised as a persona-message wait.
    if state.personas.contains_key(requester) {
        return Ok(());
    }
    let current = state.board.work(&w::WorkId(work.clone()))?;
    let candidate = state.work_meta[work]
        .message_waits
        .values()
        .find(|wait| {
            wait.observation_id().is_none()
                && wait.actor == *actor
                && wait.from == requester
                && message_wait_visible(state, work, wait, actor, now)
                && state.personas[actor].focus.get(work).is_some_and(|focus| {
                    focus.ordinary_continuation
                        && matches!(focus.continuation, Some(p::Continuation::Wait { .. }))
                })
                && current
                    .participants
                    .get(&w::PersonaId(actor.clone()))
                    .and_then(|p| p.continuation.as_ref())
                    .is_some_and(|next| next.waits_for(&wait.key.dependency))
        })
        .cloned();
    let Some(mut wait) = candidate else {
        return Ok(());
    };
    wait.requester_observation = Some(id.into());
    if !message_wait_visible(state, work, &wait, actor, now) {
        return Ok(());
    }
    let event = w::Event {
        key: wait.key.clone(),
        observation: w::ExactRef::new(id, 1, &serde_json::to_vec(&observation)?),
    };
    let context = w::RuntimeContext::from_runtime(
        w::WorkId(work.clone()),
        current.version,
        current.mandate().authority.clone(),
        w::Origin::Human(requester.into()),
        w::RuntimeRight::Observation,
    );
    state.board.observe(
        &context,
        w::RuntimeOperation::EventFor {
            event,
            participants: BTreeSet::from([w::PersonaId(actor.clone())]),
        },
    )?;
    state
        .work_meta
        .get_mut(work)
        .unwrap()
        .message_waits
        .insert(wait.id.clone(), wait);
    Ok(())
}
