//! A new appraisal can precede the act it is meant to inform. Only carry
//! evidence the owner actually used on the first call, for one fresh choice.
use crate::{
    neural::{
        context::Snapshot,
        failure,
        protocol::{ContextResource, Persona},
        provider_evidence, Result,
    },
    persona_model::Appraisal,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;
mod actions;
mod images;
mod text;

pub(in crate::neural) struct Grounds {
    pub(in crate::neural) events: Vec<Value>,
    pub(in crate::neural) text: Vec<text::TextRange>,
    pub(in crate::neural) images: Vec<images::Selection>,
}

pub(in crate::neural) fn evidence(
    db: &Connection,
    s: &Snapshot,
    a: &Appraisal,
    next: &[ContextResource],
) -> Result<Grounds> {
    let mut out = Grounds {
        events: Vec::new(),
        text: Vec::new(),
        images: Vec::new(),
    };
    for r in a
        .features
        .values()
        .filter(|f| f.value.is_some())
        .flat_map(|f| &f.evidence)
        .collect::<BTreeSet<_>>()
    {
        if s.data["work"]["reference"] == *r {
            continue;
        } // The full brief is mandatory.
        if r.starts_with("object:") || r.starts_with("asset:") {
            if let Some(image) = images::reselected(db, s, next, r)? {
                out.images.push(image);
            } else {
                out.text.extend(text::reselected(&s.data, next, r)?);
            }
            continue;
        }
        if !r.starts_with("experience:call-") {
            out.events.push(actions::observation(db, s, r)?);
        }
    }
    actions::check_citations(a, &out.events)?;
    Ok(out)
}

pub(in crate::neural) fn carry(
    db: &Connection,
    data: &Value,
    image_inputs: &[(String, String)],
    owner: &str,
    work: &str,
    p: &Persona,
) -> Result<Option<Value>> {
    let e = &data["last_contextual_evaluation"];
    let last = &data["decision_continuity"]["previous_work_decision"];
    let Some(id) = last["invocation"].as_str().filter(|_| {
        last["character_at_choice"]["next_appraisal"] == "before_action"
            && e["outputs"].is_object()
            && e["selection"]["decision"] == last["invocation"]
    }) else {
        return Ok(None);
    };
    let source = provider_evidence::accepted(db, owner, id, true)?;
    let s = source.snapshot;
    let r = source.response;
    let choice = &r["next_context"]["appraisal"];
    let a = serde_json::from_value::<Appraisal>(choice["appraisal"].clone())?;
    if s.development_version < 8
        || s.work != work
        || s.model != p.model
        || s.epoch != p.epoch
        || !s.data["context_recovery"].is_null()
        || s.data["work"]["reference"] != data["work"]["reference"]
        || choice["kind"] != "before_action"
        || serde_json::to_value(&a)? != e["appraisal"]
    {
        return Err(failure(
            "Pre-action appraisal does not match the fresh decision basis",
        ));
    }
    let next: Vec<ContextResource> =
        serde_json::from_value(r["next_context"]["references"].clone())?;
    let grounds = evidence(db, &s, &a, &next)?;
    let mut frame = json!({"owner":owner,"work":work,"source":format!("experience:{id}@1"),"model":p.model,"epoch":p.epoch,"evaluation":e["reference"],"reason":choice["reason"],"alternatives":choice["alternatives"],"evidence_then":grounds.events,"scope":"The decision is still yours. These JSON observations informed your appraisal before action, not new events or fresh files. Image bytes require explicit selection. Check any new arrivals before proceeding."});
    if !grounds.text.is_empty() {
        text::verify_next(data, &grounds.text)?;
        frame["selected_text_for_next_choice"] = serde_json::to_value(grounds.text)?;
    }
    if !grounds.images.is_empty() {
        images::verify_next(db, data, image_inputs, &grounds.images)?;
        frame["reselected_images"] = serde_json::to_value(grounds.images)?;
    }
    Ok(Some(frame))
}

pub(super) fn choice_observed(s: &Snapshot, decision: &str) -> bool {
    s.data["decision_continuity"]["before_action"]["evidence_then"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|v| {
            let action = &v["action_before_observation"];
            v["experience"]
                .as_str()
                .is_some_and(|e| observed(s, e).is_some() && action["outcome"] == e)
                && action["decision"] == decision
                && action["owner"] == s.actor
                && action["performed"].is_object()
        })
}

pub(in crate::neural) fn observed<'a>(s: &'a Snapshot, r: &str) -> Option<&'a Value> {
    let frame = &s.data["decision_continuity"]["before_action"];
    let e = &s.data["last_contextual_evaluation"];
    if s.development_version < 8
        || !s.data["context_recovery"].is_null()
        || frame["owner"] != s.actor
        || frame["work"] != s.work
        || frame["model"] != serde_json::json!(s.model)
        || frame["epoch"] != s.epoch
        || !e["outputs"].is_object()
        || frame["evaluation"] != e["reference"]
        || frame["source"] != s.data["decision_continuity"]["previous_work_decision"]["experience"]
    {
        return None;
    }
    frame["evidence_then"]
        .as_array()?
        .iter()
        .find(|v| {
            v["experience"] == r
                && (v["observation"]["event"]["experience"] == r
                    || v["observation"]["record"]["reference"] == r)
        })
        .and_then(|v| v.get("observation"))
}
