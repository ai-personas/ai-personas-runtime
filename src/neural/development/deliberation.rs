//! A new appraisal can precede the act it is meant to inform. Only carry
//! evidence the owner actually used on the first call, for one fresh choice.
use crate::{
    neural::{
        context::Snapshot, experience::recall, failure, protocol::Persona, provider_evidence,
        Result,
    },
    persona_model::Appraisal,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(in crate::neural) fn evidence(s: &Snapshot, a: &Appraisal) -> Result<Vec<Value>> {
    let mut out = BTreeMap::new();
    for r in a
        .features
        .values()
        .filter(|f| f.value.is_some())
        .flat_map(|f| &f.evidence)
    {
        if s.data["work"]["reference"] == *r {
            continue;
        } // The full brief is mandatory.
        let Some(body) =
            recall::previous_observation(s, r).filter(|_| r.starts_with("experience:event-"))
        else {
            return Err(failure("For before_action, use the supplied work reference or exact observed event bodies. Use evaluate and select other materials for a different appraisal."));
        };
        out.entry(r.clone()).or_insert_with(|| {
            let mut item = json!({"experience":r,"observation":body});
            // Keep the acting choice with its consequence if they were both on the owner's desk.
            let live = s.data["development"]["action_results"]
                .as_array()
                .into_iter()
                .flatten();
            let selected = s.data["selected_resources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v["considered"] == *r && recall::authentic_selection(s, v))
                .map(|v| &v["action_before_observation"]);
            let retained = s.data["decision_continuity"]["before_action"]["evidence_then"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v["experience"] == *r && observed(s, r).is_some())
                .map(|v| &v["action_before_observation"]);
            if let Some(action) = live
                .chain(selected)
                .chain(retained)
                .find(|v| v["outcome"] == *r && v["owner"] == s.actor && v["performed"].is_object())
            {
                item["action_before_observation"] = action.clone()
            }
            item
        });
    }
    Ok(out.into_values().collect())
}

pub(in crate::neural) fn carry(
    db: &Connection,
    data: &Value,
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
    Ok(Some(
        json!({"owner":owner,"work":work,"source":format!("experience:{id}@1"),"model":p.model,"epoch":p.epoch,"evaluation":e["reference"],"reason":choice["reason"],"alternatives":choice["alternatives"],"evidence_then":evidence(&s,&a)?,"scope":"The decision is still yours. These JSON observations informed your appraisal before action, not new events or fresh files. Image bytes require explicit selection. Check any new arrivals before proceeding."}),
    ))
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
