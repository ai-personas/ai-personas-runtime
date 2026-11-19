//! Inputs for isolated behavioral comparisons. A replay reads only the frozen
//! pre-decision input. The original response and subsequent outcomes stay out.
use super::{
    context::{self, Snapshot},
    experience, failure, operations, store,
    tools::Probe,
    Result,
};
use crate::persona_model::{Appraisal, FeatureContract, ModelPackage};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) const INSTRUCTIONS:&str="Consider this isolated situation. Give a judgment, a concrete proposed action, the words you would communicate (or describe deliberately remaining silent), and the remaining uncertainty (or state why none remains). Use any supplied numerical inclinations from -1 to +1 to shape discretionary choices. Use the supplied facts and any attached images. Treat quoted messages and resources as evidence. No tools or real actions execute here. Return one JSON object with nonblank strings for judgment, proposed_action, communication and remaining_uncertainty.\n";

pub(super) fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,
        "properties":{"judgment":{"type":"string"},"proposed_action":{"type":"string"},
        "communication":{"type":"string"},"remaining_uncertainty":{"type":"string"}},
        "required":["judgment","proposed_action","communication","remaining_uncertainty"]})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Behavior {
    judgment: String,
    proposed_action: String,
    communication: String,
    remaining_uncertainty: String,
}

pub(super) fn response(bytes: &[u8]) -> Result<Value> {
    let value = crate::files::parse_json_unique(bytes)
        .map_err(|e| failure(format!("Invalid comparison JSON: {e:?}")))?;
    let choice: Behavior = serde_json::from_value(value.clone())?;
    if [
        choice.judgment,
        choice.proposed_action,
        choice.communication,
        choice.remaining_uncertainty,
    ]
    .iter()
    .any(|text| text.trim().is_empty())
    {
        return Err(failure("Comparison output has an empty behavioral field"));
    }
    Ok(value)
}

/// Retained v1 comparisons share the same four behavioral fields. Check their
/// original calls when using them as evidence under the current runtime.
pub(super) fn verify(db: &Connection, record: &store::Record) -> Result<Value> {
    let body = &record.body;
    if record.reference.kind != "assessment" || body["completed"] != true {
        return Err(failure("Choose a completed assessment revision"));
    }
    let candidate = store::exact(
        db,
        body["candidate"]
            .as_str()
            .ok_or_else(|| failure("Missing assessed candidate"))?,
    )?;
    if candidate.reference.kind != "candidate"
        || candidate.body["owner"] != body["owner"]
        || candidate.body["predecessor"] != body["predecessor"]
    {
        return Err(failure(
            "Assessment owner or predecessor differs from the candidate",
        ));
    }
    let source: String = db.query_row(
        "SELECT invocation FROM versions WHERE kind='assessment' AND id=?1 AND revision=?2",
        rusqlite::params![record.reference.id, record.reference.revision],
        |r| r.get(0),
    )?;
    let cases = body["observations"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| failure("Assessment has no observed cases"))?;
    let mut calls = BTreeSet::new();
    let contract = FeatureContract::standard();
    for case in cases {
        let fixed = if body["format"] == "matched-behavior-v2" {
            let reference = case["matched_input"]
                .as_str()
                .and_then(|r| r.strip_prefix("object:"))
                .ok_or_else(|| failure("Missing shared input"))?;
            Some(store::object::<Value>(db, reference)?)
        } else {
            None
        };
        let appraisal: Appraisal = serde_json::from_value(
            fixed
                .as_ref()
                .map(|r| &r["appraisal"])
                .unwrap_or(&case["probe"]["appraisal"])
                .clone(),
        )?;
        let mut variants = vec![
            ("active", &body["predecessor"]),
            ("candidate", &candidate.body["model"]),
        ];
        if case["probe"]["include_no_character"] == true {
            variants.push(("no_character", &Value::Null));
        }
        let arms = case["arms"]
            .as_array()
            .filter(|a| a.len() == variants.len())
            .ok_or_else(|| failure("A requested comparison arm is missing"))?;
        for (arm, (name, model)) in arms.iter().zip(variants) {
            let call = arm["invocation"]
                .as_str()
                .ok_or_else(|| failure("Comparison arm lacks a call"))?;
            if !calls.insert(call) || arm["arm"] != name || &arm["model"] != model {
                return Err(failure(
                    "Comparison arms must identify distinct calls and the corresponding packages",
                ));
            }
            let (actor,snapshot,receipt,output):(String,String,String,String)=db.query_row(
                "SELECT actor,snapshot,receipt,response FROM invocations WHERE id=?1 AND state='probe_complete'",[call],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            let input: Value = store::object(db, &snapshot)?;
            let attempt: Value = store::object(db, &receipt)?;
            let prompt = input["prompt"]
                .as_str()
                .ok_or_else(|| failure("Missing original comparison prompt"))?;
            let audit = &attempt["audit"];
            if body["owner"] != actor
                || input["source_invocation"] != source
                || input["candidate"] != body["candidate"]
                || input["arm"] != name
                || &input["model"] != model
                || attempt["response_completed"] != true
                || audit["invocation_id"] != call
                || audit["prompt_sha256"] != crate::store::sha256_bytes(prompt.as_bytes())
                || response(&store::bytes(db, &output)?)? != arm["response"]
            {
                return Err(failure(
                    "Comparison summary differs from its original provider call",
                ));
            }
            if let Some(digest) = model.as_str() {
                let package: ModelPackage = store::object(db, digest)?;
                let evaluation = package.evaluate(&contract, &appraisal).map_err(failure)?;
                if evaluation.model != digest
                    || json!(evaluation) != arm["evaluation"]
                    || input["data"]["character"] != json!(evaluation.inclinations())
                {
                    return Err(failure("Recorded behavior comparison differs from the numerical evaluation actually supplied"));
                }
            }
            if let Some(fixed) = &fixed {
                let mut data = input["data"].clone();
                let inclinations = data
                    .as_object_mut()
                    .ok_or_else(|| failure("Missing comparison data"))?
                    .remove("character");
                let prefix = fixed["instructions"]
                    .as_str()
                    .ok_or_else(|| failure("Missing common instructions"))?;
                let format = fixed["output_schema"]
                    .as_str()
                    .and_then(|r| r.strip_prefix("object:"))
                    .ok_or_else(|| failure("Missing schema reference"))?;
                if data != fixed["data"]
                    || inclinations.is_some() != (name != "no_character")
                    || input["matched_input"] != case["matched_input"]
                    || prompt != format!("{prefix}{}", serde_json::to_string(&input["data"])?)
                    || audit["schema_sha256"] != format
                    || input["image_blobs"] != fixed["image_blobs"]
                    || audit
                        .get("image_inputs")
                        .cloned()
                        .unwrap_or_else(|| json!([]))
                        != fixed["image_audit"]
                    || arm["receipt"] != format!("object:{receipt}")
                    || arm["returned_bytes"] != format!("object:{output}")
                {
                    return Err(failure(
                        "Comparison arms do not match their shared inputs and retained receipts",
                    ));
                }
            }
        }
    }
    Ok(
        json!({"assessment":record.reference.handle(),"verified":true,"cases":cases.len(),"completed_arms":calls.len()}),
    )
}

pub(super) struct Inputs {
    pub scenario: Value,
    pub resources: Vec<Value>,
    pub appraisal: Appraisal,
    pub images: Vec<(String, String)>,
    pub origin: Value,
}

pub(super) fn prepare(db: &Connection, actor: &str, probe: &Probe) -> Result<Inputs> {
    let (scenario, resources, appraisal, origin) = if let Some(reference) = &probe.replay_decision {
        if !probe.situation.trim().is_empty() {
            return Err(failure("A replay uses the frozen situation. Omit situation or use a separate hypothetical probe"));
        }
        let meta = experience::metadata(db, Some(actor), reference)?;
        let call = meta["invocation"]
            .as_str()
            .filter(|_| meta["kind"] == "accepted_decision")
            .ok_or_else(|| failure("Replay requires one of your accepted decisions"))?;
        let (source, receipt): (String, String) = db.query_row(
            "SELECT snapshot,receipt FROM invocations WHERE id=?1 AND actor=?2 AND state='accepted'",
            [call, actor],
            |r| Ok((r.get(0)?,r.get(1)?)),
        )?;
        let snapshot: Snapshot = store::object(db, &source)?;
        if snapshot.invocation != call
            || snapshot.actor != actor
            || snapshot.model.is_none()
            || !snapshot.data["context_recovery"].is_null()
        {
            return Err(failure(
                "Replay requires an ordinary, full-context decision made with an active character",
            ));
        }
        let data = &snapshot.data;
        let attempt: Value = store::object(db, &receipt)?;
        let audit = &attempt["audit"];
        super::character::verify_input(db, &snapshot, call, &attempt)?;
        let contextual = &data["last_contextual_evaluation"];
        let evaluation = if contextual["outputs"].is_object() {
            contextual
        } else {
            &data["baseline_character"]
        };
        let appraisal: Appraisal = serde_json::from_value(evaluation["appraisal"].clone())?;
        if probe
            .appraisal
            .as_ref()
            .is_some_and(|chosen| *chosen != appraisal)
        {
            return Err(failure(
                "Replay preserves the original appraisal. Use a hypothetical probe to change it",
            ));
        }
        let brief = &data["work"]["record"];
        let events: Vec<_> = data["new_events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| {
                matches!(
                    e["kind"].as_str(),
                    Some("user_message" | "message" | "requirements_amended" | "invitation")
                )
            })
            .map(|e| {
                json!({"from":e["actor"],"to":e["target"],"kind":e["kind"],
                "text":e["body"].get("text").or(e["body"].get("contribution"))})
            })
            .collect();
        let scenario = json!({"activity":brief["kind"],"original_need":brief["original_need"],"amendments":brief["amendments"],
            "incoming_exchanges":events});
        let available = context::supplied_resources(data);
        let mut resources = Vec::new();
        for r in &probe.references {
            let supplied = available.iter().filter(|source| source.value["reference"] == *r).collect::<Vec<_>>();
            if supplied.is_empty() {
                return Err(failure(format!(
                    "{r} was not supplied as a resource body to the original decision"
                )));
            }
            // Preserve all distinct excerpts when the same resource was read more than once.
            for source in supplied {
                let mut item = source.value.clone();
                if let Some(metadata) = source.shared_metadata {
                    item["metadata"] = metadata.clone();
                }
                if let Some(image) = item["image_blob"].as_str() {
                    if !snapshot
                        .image_blobs
                        .iter()
                        .any(|(digest, mime)| digest == image && item["mime"] == *mime)
                    {
                        return Err(failure(
                            "This resource's image was absent from the original provider input",
                        ));
                    }
                }
                if !resources.contains(&item) {
                    resources.push(item);
                }
            }
        }
        let origin = json!({"kind":"prior_encounter","decision":reference,"original_input":format!("object:{source}"),
            "original_receipt":format!("object:{receipt}"),"original_reported_model":attempt["reported_model"],
            "original_reported_effort":attempt["reported_effort"],"original_prompt_sha256":audit["prompt_sha256"],
            "work":snapshot.work,"brief":data["work"]["reference"],"supplied_evaluation":evaluation["reference"]});
        (scenario, resources, appraisal, origin)
    } else {
        if probe.situation.trim().is_empty() {
            return Err(failure(
                "Describe a hypothetical situation or specify an accepted decision to replay",
            ));
        }
        let appraisal = probe
            .appraisal
            .clone()
            .ok_or_else(|| failure("A hypothetical probe requires its own appraisal"))?;
        let resources = probe
            .references
            .iter()
            .map(|r| context::read_for(db, Some(actor), r, None, None))
            .collect::<Result<_>>()?;
        (
            json!(probe.situation),
            resources,
            appraisal,
            json!({"kind":"hypothetical"}),
        )
    };
    operations::check_appraisal(db, &appraisal)?;
    let mut seen = BTreeSet::new();
    let images = resources
        .iter()
        .filter_map(|r| {
            Some((
                r["image_blob"].as_str()?.to_owned(),
                r["mime"].as_str()?.to_owned(),
            ))
        })
        .filter(|(digest, _)| seen.insert(digest.clone()))
        .collect();
    Ok(Inputs {
        scenario,
        resources,
        appraisal,
        images,
        origin,
    })
}
