//! Reconstruct the numerical inputs of an accepted decision from its provider
//! receipt. An alternate package is evaluated against that same appraisal.
use super::{character, experience, failure, store, Result};
use crate::persona_model::{Appraisal, FeatureContract, ModelPackage};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

pub fn inspect(db: &Connection, actor: Option<&str>, query: &Query) -> Result<Value> {
    let decision = match query.decision.strip_prefix("character_use:") {
        Some(id) => {
            store::exact(db, &query.decision)?;
            format!("experience:{id}")
        }
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
    let usage = store::head(db, "character_use", call)?;
    if verified["owner"] != owner
        || verified["work"] != meta["work"]
        || usage.as_ref().is_some_and(|u| {
            [
                "owner",
                "work",
                "epoch",
                "package",
                "baseline",
                "contextual",
                "snapshot",
                "response",
                "provider_receipt",
                "prompt_sha256",
            ]
            .iter()
            .any(|&key| u.body[key] != verified[key])
        })
    {
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
            let epoch = verified["epoch"].as_u64().ok_or_else(|| failure("Missing supplied epoch"))?;
            let config = verified["configuration"].as_str().ok_or_else(|| failure("Missing identity edition"))?;
            let (prior, change) = super::revision_uses::preceding_epoch(db, owner, config, epoch, digest)?;
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
    let choice: Value = store::object(db, &response)?;
    Ok(
        json!({"decision":decision,"character_use":usage.map(|u| u.reference.handle()),"owner":owner,
        "choice":{"response":verified["response"],"judgment":choice["work"],"reply":choice["reply"]},
        "work":meta["work"],"epoch":verified["epoch"],"provider_receipt":verified["provider_receipt"],
        "snapshot":verified["snapshot"],"prompt_sha256":verified["prompt_sha256"],"basis":basis,
        "evaluation":supply["reference"],"appraisal":appraisal,"axis":query.axis,"hidden_unit":query.hidden_unit,
        "supplied":{"package":format!("object:{digest}"),"numerical":numerical,"paths":paths},"alternate":alternate,
        "coordinates":"Direct terms use direct[row,column]. Hidden inputs use hidden[unit,column], hidden_bias[unit,0] and output[row,unit]. Unlisted weights are zero. By default only hidden units with a nonzero output weight appear.",
        "scope":"Verified supplied inclinations and local forward arithmetic. Any alternate is evaluated on this decision's recorded appraisal."}),
    )
}
