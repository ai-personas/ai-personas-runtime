//! Metadata for this actor's own admitted requests from the current work.
//! Child bodies and private focus selections remain explicit history choices.
use super::*;

fn children<'a>(state: &'a State, actor: &str, parent: &str)
    -> impl Iterator<Item = (&'a need_proposals::AdmittedChild, &'a w::Work)> + 'a
{
    let actor = actor.to_owned();
    let parent = parent.to_owned();
    state.intake_episodes.values().flat_map(move |episode| {
        let actor = actor.clone();
        let parent = parent.clone();
        episode.children().filter_map(move |child| {
            if child.parent_work(episode) != parent { return None; }
            let work = state.board.work(&w::WorkId(child.binding.work.clone())).ok()?;
            matches!(&work.need.origin, w::Origin::Persona(requester) if requester.0 == actor)
                .then_some((child, work))
        })
    })
}

/// Only commit metadata for exact known decisions, never an archive inventory.
pub(super) fn decision_invocations<'a>(state: &'a State, actor: &str, parent: &str)
    -> BTreeSet<&'a str>
{
    let mut ids = BTreeSet::new();
    for (child, work) in children(state, actor, parent) {
        if let Some(meta) = state.work_meta.get(&child.binding.work) {
            if let NeedOrigin::PersonaResponse { invocation_id, .. } = &meta.original.origin {
                ids.insert(invocation_id.as_str());
            }
        }
        for peer in work.participants.keys() {
            if let Some(focus) = state.personas.get(&peer.0)
                .and_then(|persona| persona.focus.get(&child.binding.work))
            {
                if let Some(origin) = &focus.judgment_origin {
                    ids.insert(origin.invocation.as_str());
                }
                if let Some(record) = &focus.authored_continuation {
                    ids.insert(record.invocation.as_str());
                }
            }
        }
    }
    ids
}

fn lookup(material: &history::records::Material, invocation: &str) -> Value {
    let mut value = json!({
        "invocation":invocation,"body_supplied":false,
        "query":{"kind":"history_records_query","record_kind":"adopted_decision",
            "mode":{"kind":"literal","needle":invocation}}
    });
    if let Some(reference) = material.requested_decisions.get(invocation)
        .filter(|record| record.invocation_id == invocation)
        .and_then(history::records::committed_reference)
    {
        value["reference"] = json!(reference);
    }
    value
}

pub(super) fn context(
    state: &State,
    material: &history::records::Material,
    actor: &str,
    parent: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Option<Value>> {
    let mut records = Vec::new();
    let mut omitted = false;
    for (child, work) in children(state, actor, parent) {
        let id = &child.binding.work;
        let Some(meta) = state.work_meta.get(id) else { omitted = true; continue; };
        let Ok(mut inherited) = work_exposure(state, id) else { omitted = true; continue; };
        if !exposure_permitted(state, &inherited, a) { omitted = true; continue; }
        let NeedOrigin::PersonaResponse { invocation_id, operation_id } = &meta.original.origin
            else { omitted = true; continue; };
        let mut proposal = lookup(material, invocation_id);
        proposal["operation"] = json!(operation_id);
        let mut participants = Vec::new();
        let mut participants_omitted = false;
        for peer in work.participants.keys() {
            let Some(persona) = state.personas.get(&peer.0)
                else { participants_omitted = true; continue; };
            let mut peer_exposure = Exposure::default();
            if let Some(birth) = &persona.birth {
                if !exposure_permitted(state, &birth.exposure, a) {
                    participants_omitted = true;
                    continue;
                }
                extend_exposure(&mut peer_exposure, &birth.exposure);
            }
            let acceptance = work.acceptance(peer).map(|(id, _)| id);
            if let Some(acceptance) = acceptance {
                match arrangements::retained_acceptance_origin(state, id, acceptance, a) {
                    Ok(Some(origin)) => extend_exposure(&mut peer_exposure, &origin),
                    Ok(None) => {},
                    Err(_) => { participants_omitted = true; continue; },
                }
            }
            let mut value = json!({
                "persona":peer,"label":persona.seed.label,"current_acceptance":acceptance,
                "last_attributed_work_judgment":{"status":"none_retained","body_supplied":false},
                "last_authored_continuation":{"status":"none_retained","body_supplied":false}
            });
            if let Some(focus) = persona.focus.get(id) {
                if let Some(origin) = &focus.judgment_origin {
                    match &focus.judgment_exposure {
                        Some(lineage) if exposure_permitted(state, lineage, a) => {
                            let mut judgment = lookup(material, &origin.invocation);
                            judgment["origin"] = json!(origin);
                            value["last_attributed_work_judgment"] = judgment;
                            extend_exposure(&mut peer_exposure, lineage);
                        },
                        _ => value["last_attributed_work_judgment"] =
                            json!({"status":"withheld_unavailable","body_supplied":false}),
                    }
                }
                if let Some(record) = &focus.authored_continuation {
                    if exposure_permitted(state, &record.exposure, a) {
                        let mut continuation = lookup(material, &record.invocation);
                        // The typed variant is recorded status, not a summary of
                        // its purpose, reason, dependencies or timing conditions.
                        continuation["kind"] = json!(match record.value {
                            p::Continuation::Immediate { .. } => "immediate",
                            p::Continuation::ReconsiderAt { .. } => "reconsider_at",
                            p::Continuation::Wait { .. } => "wait",
                            p::Continuation::Pause { .. } => "pause",
                            p::Continuation::Stop { .. } => "stop",
                        });
                        value["last_authored_continuation"] = continuation;
                        extend_exposure(&mut peer_exposure, &record.exposure);
                    } else {
                        value["last_authored_continuation"] =
                            json!({"status":"withheld_unavailable","body_supplied":false});
                    }
                }
            }
            extend_exposure(&mut inherited, &peer_exposure);
            participants.push(value);
        }
        records.push(json!({
            "work":id,"proposal":proposal,
            "admission_receipt":child.binding.receipt_id,
            "admitted_at_unix":child.binding.admitted_at_unix,
            "work_version":work.version,"current_mandate_revision":work.current_mandate,
            "recorded_phase":match work.phase {
                w::Phase::Open => "open", w::Phase::Paused { .. } => "paused",
                w::Phase::Ended { .. } => "ended", w::Phase::Completed { .. } => "completed",
            },
            "participants":participants,"participants_omitted":participants_omitted
        }));
        extend_exposure(exposure, &inherited);
    }
    if records.is_empty() && !omitted { return Ok(None); }
    Ok(Some(json!({
        "profile":"own-admitted-requests-v1","parent_work":parent,
        "records":records,"records_omitted":omitted,
        "meaning":"Admission snapshot of requests you authored in this immediate parent work that the native driver actually admitted. This is not a pending-proposal inventory or a list of all episode descendants. Identities are local to this runtime. Current acceptances, recorded phase and last authored continuation kinds are metadata; they establish neither a running process nor completion, quality or present intent. A continuation kind omits its reason, purpose, dependencies and timing; read the exact decision when those matter. Stop/Pause is not success. Judgment origins keep their original mandate revision. Exact references can use history_record_read or input_history_choices; fallback queries may also find decisions citing the invocation. No child request, judgment, result, artifact or private selected context body is supplied here. You choose useful reads, communication and continuation. These records create no assignment, wait, lock, resume or automatic replay. Omission does not establish absence of work."
    })))
}
