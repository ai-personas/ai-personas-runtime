//! A persona may propose a literal edit while the choice and its consequence
//! are still on its desk. Nothing here picks a trait, fits a weight, activates a
//! model, runs an assessment or compares the persona with another owner.
use super::{observed_experience, subsequent_event, Update};
use crate::neural::{
    context::{self, Snapshot},
    experience, failure, operations,
    protocol::Persona,
    provider_evidence, store, Result,
};
use crate::persona_model::{Appraisal, Edit, FeatureContract, ModelPackage, ParameterPatch};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    /// Open your own evidence-linked question and propose an inactive candidate.
    Trial(Trial),
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Protection {
    /// Your own earlier ordinary decision, supplied on the previous-choice card or read in full.
    pub decision: String,
    /// Output axis IDs you want unchanged on that particular recorded input.
    pub inclinations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    /// Your own accepted experience:call-ID@1, not another person's interpretation.
    pub decision: String,
    /// An observed experience:event-N@1: a direct result, linked response, later tool
    /// observation of yours or incoming event in that activity. See the earlier choice's
    /// outcome_sources_now. Sequence is not causation.
    pub outcome: String,
    pub question: String,
    /// Your interpretation of what happened, not an externally assigned lesson.
    pub reason: String,
    /// What different choice or observable consequence would count for or against this edit?
    pub prediction: String,
    /// What is worth preserving about the way *you* work? Include a possible cost.
    pub preserve: String,
    pub alternative_explanation: String,
    /// Your exact identity.active_model digest. Coordinates and numbers are your choice.
    pub expected_model: String,
    pub edits: Vec<Edit>,
    /// Optional numerical guard on a case you choose; not a population norm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protect: Option<Protection>,
}

/// Recover the source owner's actual input, not the latest head or another
/// person's appraisal of what the situation meant. The active predecessor can
/// be newer than the historical source; label all three projections separately.
fn replay(
    db: &Connection,
    snapshot: &Snapshot,
    reference: &str,
    before: &ModelPackage,
    after: &ModelPackage,
    contract: &FeatureContract,
) -> Result<Value> {
    observed_experience(db, snapshot, reference)?;
    let meta = experience::metadata(db, Some(&snapshot.actor), reference)?;
    let call = meta["invocation"]
        .as_str()
        .filter(|_| meta["kind"] == "accepted_decision")
        .ok_or_else(|| failure("Choose an observed ordinary decision of your own"))?;
    let source = provider_evidence::accepted(db, &snapshot.actor, call, false)?.snapshot;
    let digest = source
        .model
        .as_deref()
        .filter(|_| source.data["context_recovery"].is_null())
        .ok_or_else(|| failure("Choose a full ordinary decision with an active character"))?;
    let contextual = &source.data["last_contextual_evaluation"];
    let (basis, supplied) = if contextual["outputs"].is_object() {
        ("contextual", contextual)
    } else {
        ("baseline", &source.data["baseline_character"])
    };
    let usage = store::exact(db, &format!("character_use:{call}@1"))?;
    let (receipt, response): (String, String) = db.query_row(
        "SELECT receipt,response FROM invocations WHERE id=?1",
        [call],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if crate::neural::character::verified_supply(db, call, &receipt, &response)?.as_ref()
        != Some(&usage.body)
    {
        return Err(failure(
            "The source character use differs from the accepted input",
        ));
    }
    let recorded = crate::neural::character_paths::package(db, digest, &snapshot.actor)?;
    let appraisal: Appraisal = serde_json::from_value(supplied["appraisal"].clone())?;
    let values = |model: &ModelPackage| {
        model
            .evaluate(contract, &appraisal)
            .map(|v| v.inclinations())
            .map_err(failure)
    };
    let historical = values(&recorded)?;
    if supplied["model"] != digest
        || supplied["contract"] != recorded.contract
        || json!(historical) != supplied["outputs"]
    {
        return Err(failure(
            "Cannot reproduce the inclinations from the recorded input",
        ));
    }
    let old = values(before)?;
    let proposed = values(after)?;
    let changed: Vec<_> = old
        .keys()
        .filter(|axis| old[*axis] != proposed[*axis])
        .collect();
    Ok(
        json!({"decision":reference,"character_use":usage.reference.handle(),"work":source.work,
        "historical_package":digest,"original_basis":basis,"original_evaluation":supplied["reference"],
        "source_outputs":historical,"before":old,"after":proposed,"changed_outputs":changed}),
    )
}

/// Reconstruct an authored candidate without writing any state. Both proposal
/// creation and subsequent authorization use this same calculation.
pub(in crate::neural) fn material(
    db: &Connection,
    snapshot: &Snapshot,
    before: &ModelPackage,
    trial: &Trial,
    considered: &[String],
) -> Result<(ModelPackage, Value)> {
    if snapshot.development_version < 6
        || snapshot.model.is_none()
        || !snapshot.data["context_recovery"].is_null()
        || [
            &trial.question,
            &trial.reason,
            &trial.prediction,
            &trial.preserve,
            &trial.alternative_explanation,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
    {
        return Err(failure("A personal trial needs ordinary evidence, a question, interpretation, prediction, a strength to preserve and another possible explanation"));
    }
    if ![&trial.decision, &trial.outcome]
        .iter()
        .all(|r| considered.contains(r))
    {
        return Err(failure(
            "Include the choice and the observed outcome of your trial in considered_experience",
        ));
    }
    let contract = FeatureContract::standard();
    let evidence = vec![trial.decision.clone(), trial.outcome.clone()];
    let after = before
        .patch(
            &contract,
            &ParameterPatch {
                expected_model: trial.expected_model.clone(),
                edits: trial.edits.clone(),
                reason: trial.reason.clone(),
                evidence,
            },
        )
        .map_err(failure)?;
    if before.numerical_digest(&contract).map_err(failure)?
        == after.numerical_digest(&contract).map_err(failure)?
    {
        return Err(failure("This trial changes no numerical parameter; keep the current model or propose a literal difference"));
    }
    let trigger = replay(db, snapshot, &trial.decision, before, &after, &contract)?;
    let source = experience::metadata(db, Some(&snapshot.actor), &trial.decision)?;
    let call = source["invocation"]
        .as_str()
        .ok_or_else(|| failure("Missing source decision"))?;
    let work = source["work"]
        .as_str()
        .ok_or_else(|| failure("Missing source activity"))?;
    let (event, boundary) = subsequent_event(db, snapshot, call, &trial.outcome)?;
    let link = experience::responses_to_call(db, &snapshot.actor, work, call)?
        .into_iter()
        .find(|r| r["experience"] == trial.outcome);
    let relation = if event["work"] == work
        && event["actor"] == snapshot.actor
        && event["target"] == snapshot.actor
        && event["kind"] == "tool_result"
        && event["invocation"] == call
    {
        "operation_result"
    } else if let Some(relation) = link.as_ref().and_then(|r| r["relation"].as_str()) {
        relation
    } else if event["work"] == work
        && event["actor"] == snapshot.actor
        && event["target"] == snapshot.actor
        && event["kind"] == "tool_result"
    {
        // An owner can discover a consequence by checking later. Not a direct result
        // of the earlier choice; subsequent_event checked its chronological boundary.
        "later_tool_observation"
    } else if event["work"] == work
        && (event["actor"] != snapshot.actor || event["invocation"].is_null())
        && (event["target"] == snapshot.actor || event["target"] == "*")
    {
        "later_received_in_activity"
    } else {
        return Err(failure("Read the earlier decision's outcome_sources_now and linked_responses_now. Choose its actual tool result, a linked response, or a later tool or incoming observation from its activity—not the receipt for looking it up here"));
    };
    let protection = if let Some(guard) = &trial.protect {
        let case = if guard.decision == trial.decision {
            trigger.clone()
        } else {
            replay(db, snapshot, &guard.decision, before, &after, &contract)?
        };
        let mut seen = BTreeSet::new();
        if guard.inclinations.is_empty() {
            return Err(failure(
                "Name the inclinations you want to protect, or omit protect",
            ));
        }
        for axis in &guard.inclinations {
            if !seen.insert(axis) || !case["before"][axis].is_number() {
                return Err(failure("Protect distinct existing output axes"));
            }
            if case["before"][axis] != case["after"][axis] {
                return Err(failure(format!(
                    "Your literal edits change your protected {axis} on {}. No trial was retained.",
                    guard.decision
                )));
            }
        }
        Some(json!({"inclinations":guard.inclinations,"checked_case":case}))
    } else {
        None
    };
    Ok((
        after,
        json!({"trigger":trigger,"outcome":trial.outcome,"decision_effects":boundary,"relation":relation,
        "preserve":trial.preserve,"protection":protection,"scope":"Forward arithmetic on recorded inputs, not a behavioral assessment or proof of improvement."}),
    ))
}

fn create(
    db: &Connection,
    snapshot: &Snapshot,
    persona: &Persona,
    trial: &Trial,
    considered: &[String],
) -> Result<Value> {
    let (after, probes) = material(
        db,
        snapshot,
        &context::model(db, persona)?,
        trial,
        considered,
    )?;
    let question = super::update(
        db,
        snapshot,
        Update {
            base: None,
            question: trial.question.clone(),
            interpretation: trial.reason.clone(),
            evidence: after.evidence.clone(),
            contrary_evidence: vec![],
            prediction: trial.prediction.clone(),
            alternatives: trial.alternative_explanation.clone(),
            disposition: super::Disposition::Investigating,
            revisit: None,
            follow_up: None,
            candidate_review: None,
            question_observation: None,
        },
    )?;
    let reference = question["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing retained trial question"))?;
    let mut result = operations::candidate(
        db,
        &snapshot.actor,
        &snapshot.invocation,
        after,
        probes,
        Some(reference),
    )?;
    result["development"] = json!(reference);
    Ok(result)
}

pub(crate) fn propose(
    db: &Connection,
    snapshot: &Snapshot,
    persona: &Persona,
    choice: &Disposition,
    considered: &[String],
) -> Result<()> {
    let Disposition::Trial(trial) = choice;
    let result = create(db, snapshot, persona, trial, considered)?;
    let digest = store::put(db, &result)?;
    store::event(
        db,
        &snapshot.work,
        &snapshot.actor,
        &snapshot.actor,
        "personal_trial_proposed",
        &json!({"candidate":result["candidate"],"development":result["development"],"result":format!("object:{digest}")}),
        Some(&snapshot.invocation),
    )?;
    Ok(())
}
