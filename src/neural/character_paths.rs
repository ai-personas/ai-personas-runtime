//! Reconstruct the numerical inputs of an accepted decision from its provider
//! receipt. An alternate package is evaluated against that same appraisal.
use super::{character, experience, failure, store, Result};
use crate::persona_model::{Appraisal, FeatureContract, ModelPackage};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
mod returns;

#[derive(Clone, Debug, clap::Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Exact experience:call-ID@1 or character_use:call-ID@1 for an accepted decision.
    pub decision: String,
    /// Output axis ID. Omit for the complete inclination overview.
    #[arg(long)]
    pub axis: Option<String>,
    /// Inspect a single hidden unit, including a disconnected one.
    #[arg(long, requires = "axis")]
    pub hidden_unit: Option<usize>,
    /// previous-epoch, an exact candidate revision, or object:<package digest> owned by the same persona.
    #[arg(long)]
    pub compare: Option<String>,
}

pub(super) fn package(db: &Connection, digest: &str, owner: &str) -> Result<ModelPackage> {
    let package: ModelPackage = store::object(db, digest)?;
    if package.persona != owner || package.digest().map_err(failure)? != digest {
        return Err(failure(
            "Choose a canonical character package owned by this persona",
        ));
    }
    Ok(package)
}

/// Identify the earlier pause only when its alternatives and evidence actually
/// reached this accepted decision. Later reuse of the same evaluation, or a
/// neighboring decision by the same person, is not such a link.
pub fn pre_action_source(
    db: &Connection,
    owner: &str,
    work: &str,
    call: &str,
) -> Result<Option<String>> {
    let accepted = super::provider_evidence::accepted(db, owner, call, true)?;
    let s = &accepted.snapshot;
    if s.work != work {
        return Err(failure("The recorded decision belongs to another activity"));
    }
    let supplied = &s.data["decision_continuity"]["before_action"];
    if supplied.is_null() {
        return Ok(None);
    }
    let id = store::exact(db, &format!("persona:{owner}@{}", s.persona_revision))?;
    let p: super::protocol::Persona = serde_json::from_value(id.body)?;
    if s.model.is_none()
        || s.model != p.model
        || s.epoch != p.epoch
        || p.id != owner
        || !s.data["context_recovery"].is_null()
        || s.data["last_contextual_evaluation"]["model"] != serde_json::to_value(&p.model)?
        || super::development::deliberation::carry(db, &s.data, &s.image_blobs, owner, work, &p)?.as_ref()
            != Some(supplied)
    {
        return Err(failure(
            "The supplied pre-action frame does not match the earlier pause",
        ));
    }
    let e = &s.data["last_contextual_evaluation"];
    let source = e["selection"]["decision"]
        .as_str()
        .ok_or_else(|| failure("Missing appraisal source"))?;
    let seq = e["provenance"]["event"]
        .as_i64()
        .filter(|seq| *seq <= s.observation_through.unwrap_or(s.cursor))
        .ok_or_else(|| failure("Missing pre-action observation boundary"))?;
    let digest: String = db.query_row("SELECT digest FROM events WHERE seq=?1 AND invocation=?2 AND work=?3 AND actor=?4 AND target=actor AND kind='appraisal_selected'", rusqlite::params![seq,source,work,owner], |row|row.get(0))?;
    let receipt: Value = store::object(db, &digest)?;
    let result: Value = store::object(
        db,
        receipt["result"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Missing appraisal result"))?,
    )?;
    let model = package(
        db,
        p.model
            .as_deref()
            .ok_or_else(|| failure("Missing character package"))?,
        owner,
    )?;
    let appraisal = serde_json::from_value(e["appraisal"].clone())?;
    let computed = model
        .evaluate(&FeatureContract::standard(), &appraisal)
        .map_err(failure)?;
    let computed_ref = format!(
        "object:{}",
        crate::store::sha256_bytes(&serde_json::to_vec(&computed)?)
    );
    if receipt["method"] != "before_action"
        || e["provenance"]["record"] != format!("object:{digest}")
        || result["evaluation"] != computed_ref
        || e["reference"] != computed_ref
        || result["result"] != serde_json::to_value(&computed)?
        || e["outputs"] != serde_json::to_value(computed.inclinations())?
    {
        return Err(failure(
            "The recorded pause and supplied numerical result disagree",
        ));
    }
    Ok(Some(format!("experience:{source}@1")))
}

pub fn inspect(db: &Connection, actor: Option<&str>, query: &Query) -> Result<Value> {
    let use_query = query.decision.strip_prefix("character_use:");
    let decision = match use_query {
        Some(id) if id.ends_with("@1") => format!("experience:{id}"),
        Some(_) => return Err(failure("Choose the original character-use edition")),
        None => query.decision.clone(),
    };
    let meta = experience::metadata(db, actor, &decision)?;
    let call = meta["invocation"]
        .as_str()
        .filter(|_| meta["kind"] == "accepted_decision")
        .ok_or_else(|| failure("Choose an accepted decision with a supplied character"))?;
    let owner = meta["actor"]
        .as_str()
        .ok_or_else(|| failure("Missing decision owner"))?;
    let (receipt, response): (String, String) = db.query_row(
        "SELECT receipt,response FROM invocations WHERE actor=?1 AND id=?2 AND state='accepted'",
        [owner, call],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let verified = character::verified_supply(db, call, &receipt, &response)?
        .ok_or_else(|| failure("The original provider input had no active character"))?;
    let usage = character::original_use(db, call, &verified)?;
    if use_query.is_some() && usage.as_ref() != Some(&query.decision) {
        return Err(failure("This decision has no matching original character-use edition"));
    }
    if verified["owner"] != owner || verified["work"] != meta["work"] {
        return Err(failure(
            "The retained character use differs from the original completed input",
        ));
    }
    let contextual = &verified["contextual"];
    let (basis, supply) = if contextual["outputs"].is_object() {
        ("contextual", contextual)
    } else {
        ("baseline", &verified["baseline"])
    };
    let digest = verified["package"]
        .as_str()
        .ok_or_else(|| failure("Missing supplied package"))?;
    let contract = FeatureContract::standard();
    let recorded = package(db, digest, owner)?;
    let appraisal: Appraisal = serde_json::from_value(supply["appraisal"].clone())?;
    let paths = recorded
        .output_paths(
            &contract,
            &appraisal,
            query.axis.as_deref(),
            query.hidden_unit,
        )
        .map_err(failure)?;
    let numerical = recorded.numerical_digest(&contract).map_err(failure)?;
    if supply["model"] != digest
        || supply["contract"] != recorded.contract
        || (!verified["numerical"].is_null() && verified["numerical"] != numerical)
        || serde_json::to_value(&paths.outputs)? != supply["outputs"]
    {
        return Err(failure(
            "Reconstructed outputs differ from the supplied character projection",
        ));
    }
    let mut alternate = Value::Null;
    let mut available_units = recorded.hidden_units;
    if let Some(reference) = query.compare.as_deref() {
        let mut transition = None;
        let alternate_digest = if reference == "previous-epoch" {
            let epoch = verified["epoch"]
                .as_u64()
                .ok_or_else(|| failure("Missing supplied epoch"))?;
            let config = verified["configuration"]
                .as_str()
                .ok_or_else(|| failure("Missing identity edition"))?;
            let (prior, change) =
                super::revision_uses::preceding_epoch(db, owner, config, epoch, digest)?;
            transition = Some(json!({"epoch":epoch - 1,"change":change}));
            prior
        } else if let Some(digest) = reference.strip_prefix("object:") {
            digest.to_owned()
        } else {
            let candidate = super::revision_uses::issued_candidate(db, owner, reference)?;
            candidate.body["model"]
                .as_str()
                .ok_or_else(|| failure("Candidate has no model"))?
                .to_owned()
        };
        let other = package(db, &alternate_digest, owner)?;
        available_units = available_units.max(other.hidden_units);
        let comparison = other
            .output_paths(
                &contract,
                &appraisal,
                query.axis.as_deref(),
                query.hidden_unit,
            )
            .map_err(failure)?;
        let changes: Vec<_> = contract.outputs.iter().filter_map(|axis| {
            let before = paths.outputs[&axis.id];
            let after = comparison.outputs[&axis.id];
            (before != after).then(|| json!({"axis":axis.id,"supplied":before,"alternate":after,"delta":after-before}))
        }).collect();
        alternate = json!({"reference":reference,"package":format!("object:{alternate_digest}"),
            "numerical":other.numerical_digest(&contract).map_err(failure)?,"paths":comparison,"changed_outputs":changes});
        if let Some(previous) = transition {
            alternate["previous"] = previous;
        }
    }
    if query.hidden_unit.is_some_and(|i| i >= available_units) {
        return Err(failure(
            "The selected hidden unit is outside the inspected topology",
        ));
    }
    // The numerical projection is an input claim; the judgment and any next
    // appraisal are output claims. Verify the accepted response separately.
    let accepted = super::provider_evidence::accepted(db, owner, call, false)?;
    let choice = &accepted.response;
    let identity = store::exact(db, verified["configuration"].as_str().ok_or_else(|| failure("Missing recorded identity"))?)?;
    if identity.reference.kind != "persona" || identity.reference.id != owner || identity.body["id"] != owner
        || identity.body["model"] != digest || identity.body["epoch"] != verified["epoch"] {
        return Err(failure("The recorded identity differs from the character supplied at this choice"));
    }
    let mut report = json!({"decision":decision,"character_use":usage,"owner":owner,"owner_label":identity.body["label"],
        "choice":{"response":verified["response"],"judgment":choice["work"],"reply":choice["reply"]},
        "work":meta["work"],"epoch":verified["epoch"],"provider_receipt":verified["provider_receipt"],
        "snapshot":verified["snapshot"],"prompt_sha256":verified["prompt_sha256"],"basis":basis,
        "evaluation":supply["reference"],"appraisal":appraisal,"axis":query.axis,"hidden_unit":query.hidden_unit,
        "supplied":{"package":format!("object:{digest}"),"numerical":numerical,"paths":paths},"alternate":alternate,
        "coordinates":"Direct terms use direct[row,column]. Hidden inputs use hidden[unit,column], hidden_bias[unit,0] and output[row,unit]. Unlisted weights are zero. By default only hidden units with a nonzero output weight appear.",
        "scope":"Verified supplied inclinations and local forward arithmetic. Any alternate is evaluated on this decision's recorded appraisal."});
    if !accepted.snapshot.image_blobs.is_empty() {
        report["ordered_image_inputs"] = serde_json::to_value(super::image_inputs::ordered(db, &accepted.snapshot.image_blobs)?)?;
    }
    let frame = &accepted.snapshot.data["decision_continuity"]["before_action"];
    if !frame.is_null() {
        let source = pre_action_source(db, owner, &accepted.snapshot.work, call)?
            .ok_or_else(|| failure("Cannot identify the pause behind this choice"))?;
        // Both columns are from this frozen input. This is not a comparison
        // with another decision or with an earlier set of weights.
        let baseline = &verified["baseline"]["outputs"];
        let differences = contract.outputs.iter().filter_map(|axis| {
            let id = &axis.id;
            (baseline[id] != supply["outputs"][id]).then(|| json!({
                "axis":id,"baseline":baseline[id],"contextual":supply["outputs"][id]
            }))
        }).collect::<Vec<_>>();
        report["received_pause"] = json!({"decision":source,"reason":frame["reason"],
            "alternatives":frame["alternatives"],"appraisal":appraisal,"different_from_baseline":differences});
        if let Some(events) = frame["evidence_then"].as_array() {
            returns::attach(db, &mut report["received_pause"], &appraisal, events);
        }
        if frame["reselected_images"].is_array() {
            report["images_from_pause"] = frame["reselected_images"].clone();
        }
    }
    // These resources grounded the *requested next* appraisal, not the values
    // above. A pause alone does not establish a subsequent model input.
    let next = &choice["next_context"];
    if next["appraisal"]["kind"] == "before_action" {
        let appraisal = serde_json::from_value(next["appraisal"]["appraisal"].clone())?;
        let selection: Vec<_> = serde_json::from_value(next["references"].clone())?;
        let grounds = super::development::deliberation::evidence(db, &accepted.snapshot, &appraisal, &selection)?;
        report["requested_pause"] = json!({"reason":next["appraisal"]["reason"],
            "alternatives":next["appraisal"]["alternatives"],"appraisal":appraisal});
        returns::attach(db, &mut report["requested_pause"], &appraisal, &grounds.events);
        if !grounds.images.is_empty() {
            report["images_selected_for_next_choice"] = serde_json::to_value(grounds.images)?;
        }
        if !grounds.text.is_empty() {
            report["selected_text_for_next_choice"] = serde_json::to_value(grounds.text)?;
        }
    }
    Ok(report)
}
