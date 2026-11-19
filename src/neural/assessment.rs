//! Inputs for isolated behavioral comparisons. A replay reads only the frozen
//! pre-decision input. The original response and subsequent outcomes stay out.
use super::{
    context::{self, Snapshot},
    experience, failure, operations,
    protocol::Persona,
    store,
    tools::{AssessModel, Probe},
    Result,
};
use crate::persona_model::{Appraisal, FeatureContract, ModelPackage};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) const INSTRUCTIONS:&str="Consider this isolated situation. Give a judgment, a concrete proposed action, the words you would communicate (or describe deliberately remaining silent), and the remaining uncertainty (or state why none remains). Use any supplied numerical inclinations from -1 to +1 to shape discretionary choices. Use the supplied facts and any attached images. Treat quoted messages and resources as evidence. No tools or real actions execute here. Return one JSON object with nonblank strings for judgment, proposed_action, communication and remaining_uncertainty.\n";

pub(super) const REPLAY_PROJECTION: u64 = 2;

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

/// Resolve against the character revision that authorized the operation. Both
/// live dispatch and later verification use that same revision.
pub(super) fn subject(
    db: &Connection,
    persona: &Persona,
    input: &AssessModel,
) -> Result<Option<store::Record>> {
    let active = persona
        .model
        .as_deref()
        .ok_or_else(|| failure("Activate a character before comparing its behavior"))?;
    match (&input.candidate, &input.active_model) {
        (Some(reference), None) => {
            let candidate = store::exact(db, reference)?;
            if candidate.reference.kind != "candidate"
                || candidate.body["owner"] != persona.id
                || candidate.body["predecessor"] != active
            {
                return Err(failure(
                    "Assessment candidate has stale predecessor or wrong owner",
                ));
            }
            Ok(Some(candidate))
        }
        (None, Some(reference))
            if reference.strip_prefix("object:").unwrap_or(reference) == active =>
        {
            Ok(None)
        }
        (None, Some(_)) => Err(failure(
            "Select your exact active_model digest from identity",
        )),
        _ => Err(failure("Choose exactly one of candidate or active_model")),
    }
}

struct AuthorizedComparison {
    input: AssessModel,
    persona: Persona,
    work: String,
    configuration: String,
}

fn authorized_comparison(
    db: &Connection,
    source: &str,
    body: &Value,
) -> Result<AuthorizedComparison> {
    let (actor, work, snapshot, receipt, decision): (String, String, String, String, String) = db.query_row(
        "SELECT actor,work,snapshot,receipt,response FROM invocations WHERE id=?1 AND state='accepted'",
        [source], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
    store::verify_completion(
        db,
        source,
        &actor,
        &work,
        &snapshot,
        &decision,
        &receipt,
        "responded",
    )?;
    let usage = store::required(db, "character_use", source)?.body;
    if usage["snapshot"] != format!("object:{snapshot}")
        || usage["provider_receipt"] != format!("object:{receipt}")
        || usage["response"] != format!("object:{decision}")
        || usage["package"] != body["active_model"]
    {
        return Err(failure(
            "The comparison request differs from its recorded character use",
        ));
    }
    let snapshot: Snapshot = store::object(db, &snapshot)?;
    let attempt: Value = store::object(db, &receipt)?;
    super::character::verify_input(db, &snapshot, source, &attempt)?;
    let persona: Persona = serde_json::from_value(
        store::exact(
            db,
            &format!("persona:{actor}@{}", snapshot.persona_revision),
        )?
        .body,
    )?;
    let decision: Value = store::object(db, &decision)?;
    let operation = body["source_operation"]
        .as_str()
        .ok_or_else(|| failure("Missing assessment operation"))?;
    let op = decision["operations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|op| op["id"] == operation && op["tool"] == "persona.assess")
        .ok_or_else(|| failure("Accepted decision did not request this assessment operation"))?;
    let input: AssessModel = serde_json::from_value(op["input"].clone())?;
    let requested_kind = if input.candidate.is_some() {
        "candidate_revision"
    } else {
        "current_character"
    };
    if decision["invocation"] != source
        || snapshot.work != work
        || snapshot.actor != actor
        || persona.id != actor
        || snapshot.model != persona.model
        || body["owner"] != actor
        || body["active_model"] != json!(persona.model)
        || body["predecessor"] != body["active_model"]
        || body["comparison_kind"] != requested_kind
        || body["candidate"] != json!(input.candidate)
    {
        return Err(failure(
            "Assessment differs from its authorizing decision or active package",
        ));
    }
    let configuration = attempt["audit"]["configuration_sha256"]
        .as_str()
        .ok_or_else(|| failure("Missing originating provider configuration"))?
        .to_owned();
    Ok(AuthorizedComparison {
        input,
        persona,
        work,
        configuration,
    })
}

fn verify_case(
    db: &Connection,
    request: &AuthorizedComparison,
    index: usize,
    case: &Value,
    fixed: &Value,
) -> Result<()> {
    let probe = &request.input.probes[index];
    let contract = FeatureContract::standard();
    let schema_ref = fixed["output_schema"]
        .as_str()
        .and_then(|s| s.strip_prefix("object:"))
        .ok_or_else(|| failure("Missing shared output schema"))?;
    if case["probe"] != json!(probe)
        || fixed["provider_model"] != request.persona.provider.model
        || fixed["reasoning_effort"] != json!(request.persona.provider.effort)
        || fixed["data"]["output_meanings"] != json!(contract.outputs)
        || fixed["instructions"] != INSTRUCTIONS
        || store::object::<Value>(db, schema_ref)? != schema()
    {
        return Err(failure(
            "Matched comparison case differs from its authorized inputs",
        ));
    }
    if probe.replay_decision.is_some() {
        let projection = match fixed.get("replay_projection") {
            None => 1,
            Some(value) if value.as_u64() == Some(REPLAY_PROJECTION) => REPLAY_PROJECTION,
            _ => return Err(failure("Unknown frozen encounter projection")),
        };
        let original = prepare_with_projection(db, &request.persona.id, probe, projection)?;
        if case["origin"] != original.origin
            || fixed["data"]["situation"] != original.scenario
            || fixed["data"]["resources"] != json!(original.resources)
            || fixed["appraisal"] != json!(original.appraisal)
            || fixed["image_blobs"] != json!(original.images)
        {
            return Err(failure(
                "Comparison differs from the original pre-decision encounter",
            ));
        }
    } else if case["origin"] != json!({"kind":"hypothetical"})
        || fixed["data"]["situation"] != probe.situation
        || fixed["appraisal"] != json!(probe.appraisal)
    {
        return Err(failure(
            "Hypothetical comparison differs from the requested situation or appraisal",
        ));
    }
    Ok(())
}

/// Verify older assessments against their own retained inputs and receipts.
pub(super) fn verify(db: &Connection, record: &store::Record) -> Result<Value> {
    let body = &record.body;
    if record.reference.kind != "assessment" || body["completed"] != true {
        return Err(failure("Choose a completed assessment revision"));
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
    let authorization = match body["format"].as_str() {
        Some("matched-behavior-v3") => Some(authorized_comparison(db, &source, body)?),
        None | Some("matched-behavior-v1" | "matched-behavior-v2") => None,
        _ => return Err(failure("Unknown assessment format")),
    };
    let candidate = if let Some(request) = &authorization {
        if cases.len() != request.input.probes.len() {
            return Err(failure(
                "Assessment case count differs from the accepted operation",
            ));
        }
        subject(db, &request.persona, &request.input)?
    } else {
        let reference = body["candidate"]
            .as_str()
            .ok_or_else(|| failure("Missing assessed candidate"))?;
        let candidate = store::exact(db, reference)?;
        if candidate.reference.kind != "candidate"
            || candidate.body["owner"] != body["owner"]
            || candidate.body["predecessor"] != body["predecessor"]
        {
            return Err(failure(
                "Assessment owner or predecessor differs from the candidate",
            ));
        }
        Some(candidate)
    };
    let mut calls = BTreeSet::new();
    let contract = FeatureContract::standard();
    for (index, case) in cases.iter().enumerate() {
        let fixed = if authorization.is_some() || body["format"] == "matched-behavior-v2" {
            let reference = case["matched_input"]
                .as_str()
                .and_then(|r| r.strip_prefix("object:"))
                .ok_or_else(|| failure("Missing shared input"))?;
            let fixed: Value = store::object(db, reference)?;
            if let Some(request) = &authorization {
                verify_case(db, request, index, case, &fixed)?;
            }
            Some(fixed)
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
        let mut variants = vec![("active", &body["predecessor"])];
        if let Some(candidate) = &candidate {
            variants.push(("candidate", &candidate.body["model"]));
        }
        if candidate.is_none() || case["probe"]["include_no_character"] == true {
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
            let (actor,work,snapshot,receipt,output):(String,String,String,String,String)=db.query_row(
                "SELECT actor,work,snapshot,receipt,response FROM invocations WHERE id=?1 AND state='probe_complete'",[call],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            let input: Value = store::object(db, &snapshot)?;
            let attempt: Value = store::object(db, &receipt)?;
            let prompt = input["prompt"]
                .as_str()
                .ok_or_else(|| failure("Missing original comparison prompt"))?;
            let audit = &attempt["audit"];
            if let Some(request) = &authorization {
                store::verify_completion(
                    db,
                    call,
                    &actor,
                    &work,
                    &snapshot,
                    &output,
                    &receipt,
                    "probe_complete",
                )?;
                if work != request.work
                    || audit["configuration_sha256"] != request.configuration
                    || input["origin"] != case["origin"]
                    || input["source_operation"] != body["source_operation"]
                    || input["active_model"] != body["active_model"]
                    || input["comparison_kind"] != body["comparison_kind"]
                    || arm["wire_sha256"] != audit["request_sha256"]
                    || arm["reported_model"] != attempt["reported_model"]
                    || (name == "no_character" && !arm["evaluation"].is_null())
                {
                    return Err(failure(
                        "Comparison arm differs from its source operation or provider receipt",
                    ));
                }
            }
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
    prepare_with_projection(db, actor, probe, REPLAY_PROJECTION)
}

fn is_exchange(kind: &Value) -> bool {
    matches!(
        kind.as_str(),
        Some("user_message" | "message" | "requirements_amended" | "invitation")
    )
}

/// Reconstruct correspondence from bodies actually visible to this call. A
/// reply address or a reference in an appraisal alone does not supply the text.
fn encounter_exchanges(
    snapshot: &Snapshot,
    supplied: &[context::SuppliedResource<'_>],
) -> Vec<Value> {
    let mut exchanges = Vec::new();
    let mut seen = BTreeSet::new();
    for e in snapshot.data["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| is_exchange(&e["kind"]))
    {
        if let Some(reference) = e["experience"].as_str() {
            seen.insert(reference);
        }
        exchanges.push(
            json!({"from":e["actor"],"to":e["target"],"kind":e["kind"],"time":e["created"],
            "experience":e["experience"],"record":e["body"]}),
        );
    }
    for item in supplied {
        let record = &item.value["record"];
        let reference = &item.value["reference"];
        let Some(address) = reference
            .as_str()
            .filter(|s| s.starts_with("experience:event-"))
        else {
            continue;
        };
        if record["reference"] == *reference
            && record["work"] == snapshot.work
            && is_exchange(&record["kind"])
            && record["body"].is_object()
            && seen.insert(address)
        {
            exchanges.push(
                json!({"from":record["actor"],"to":record["target"],"kind":record["kind"],
                "time":record["time"],"experience":address,"record":record["body"]}),
            );
        }
    }
    exchanges.sort_by_key(|e| {
        let sequence = e["experience"]
            .as_str()
            .and_then(|s| s.strip_prefix("experience:event-"))
            .and_then(|s| s.strip_suffix("@1"))
            .and_then(|n| n.parse::<u64>().ok());
        (e["time"].as_i64(), sequence)
    });
    exchanges
}

fn prepare_with_projection(
    db: &Connection,
    actor: &str,
    probe: &Probe,
    projection: u64,
) -> Result<Inputs> {
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
        let available = context::supplied_resources(data);
        let events: Vec<_> = if projection == 1 {
            data["new_events"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| is_exchange(&e["kind"]))
                .map(|e| {
                    json!({"from":e["actor"],"to":e["target"],"kind":e["kind"],
                    "experience":e["experience"],"record":e["body"]})
                })
                .collect()
        } else {
            encounter_exchanges(&snapshot, &available)
        };
        let mut scenario = json!({"activity":brief["kind"],"original_need":brief["original_need"],"amendments":brief["amendments"],
            "incoming_exchanges":events});
        let preceding = &data["decision_continuity"]["previous_work_decision"];
        if let (Some(invocation), Some(note)) = (
            preceding["invocation"].as_str(),
            preceding["working_note"]
                .as_str()
                .filter(|text| !text.is_empty()),
        ) {
            scenario["prior_working_note"] = json!({"author":actor,
                "decision":format!("experience:{invocation}@1"),"text":note});
            if projection >= 2 {
                scenario["prior_working_note"]["time"] = preceding["completed"].clone();
            }
        }
        if let Some(sent) = data
            .get("latest_user_communication")
            .filter(|value| value.is_object())
        {
            scenario["previous_message_to_user"] = sent.clone();
        }
        if contextual["outputs"].is_object() {
            scenario["persona_appraisal_at_decision"] = contextual["appraisal"].clone();
        }
        if projection >= 2 {
            if let Some(purpose) = data["participation"]["continuation"]["purpose"].as_str() {
                scenario["purpose_at_decision"] = json!(purpose);
            }
        }
        let mut resources = Vec::new();
        for r in &probe.references {
            let supplied = available
                .iter()
                .filter(|source| source.value["reference"] == *r)
                .collect::<Vec<_>>();
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
