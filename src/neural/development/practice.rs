//! The scope of a behavioral lesson and the disposition of numerical weights
//! are different decisions. A local correction is sufficient when the owner
//! says so. An intention, a performed practice and a finding are not synonyms.
use super::{observed_experience, owned, write_update, Disposition, Revisit, Update};
use crate::neural::{
    context::Snapshot,
    experience, failure,
    protocol::{AppraisalChoice, DecisionV4, NextModel},
    store, Result,
};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Choice {
    /// Keep this result local. A reasoned choice not to generalize needs no new item.
    Local { reason: String },
    /// Reserve judgment without opening an experiment or claiming a behavioral finding.
    Undecided { reason: String },
    /// Keep a prospective experiment for a later encounter; no application or success is recorded now.
    NextTime { plan: Plan },
    /// Actually try the approach in your own reply or named operation in this decision.
    Practice { plan: Plan, action: Action },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Reply,
    Operation { id: String },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Exact event from action_results, recalled or retained action evidence, or a full read of an outcome together with the full read of your acting decision. Use the original outcome, not the receipt for reading it.
    pub outcome: String,
    /// Situation in which this behavioral contrast matters to you. Not a global rule.
    pub when: String,
    /// What you would try doing, or deliberately keep doing.
    pub approach: String,
    /// Another plausible approach against which to judge it.
    pub alternative: String,
    /// What observable consequence would help distinguish them?
    pub watch_for: String,
    /// Optional earlier strength or situation you do not want to change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve: Option<String>,
    /// Optional exact current question you have read; omit to open one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}
impl Action {
    pub(super) fn endpoint(&self, d: &DecisionV4) -> Result<Value> {
        match self {
            Self::Reply if d.reply.as_deref().is_some_and(|s| !s.trim().is_empty()) => {
                Ok(json!({"kind":"reply"}))
            }
            Self::Operation { id } => {
                let o =
                    d.operations.iter().find(|o| o.id == *id).ok_or_else(|| {
                        failure("This practice names no operation in this decision")
                    })?;
                Ok(json!({"kind":"operation","id":id,"tool":o.tool}))
            }
            _ => Err(failure(
                "A practice in your reply requires the actual nonempty reply",
            )),
        }
    }
}

fn on_desk(data: &Value) -> impl Iterator<Item = &Value> {
    data["development"]["action_results"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            data["decision_continuity"]["before_action"]["evidence_then"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.get("action_before_observation")),
        )
}
/// Expose the separate scope choice on a material return, not every search,
/// every N calls, or because the host deemed the feedback positive or negative.
pub(in crate::neural) fn scope(data: &Value) -> Option<Value> {
    let mut outcomes = std::collections::BTreeSet::new();
    for v in on_desk(data) {
        if v["relation"] == "direct_reply"
            || v["return_kind"] == "dispatched_result"
            || v["forecasts_before_action"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        {
            if let Some(r) = v["outcome"].as_str() {
                outcomes.insert(r);
            }
        }
    }
    (!outcomes.is_empty()).then(||json!({"outcomes":outcomes,"choice":"Choose behavior independently of persona_maintenance: local if this stays here, undecided if you reserve judgment, next_time with your own scoped experiment, or practice in an actual action now. No numeric change is required or rewarded. A next_time intention is not an application."}))
}

pub(super) fn receipt<'a>(s: &'a Snapshot, reference: &str) -> Option<&'a Value> {
    if s.development_version < 9 || s.model.is_none() || !s.data["context_recovery"].is_null() {
        return None;
    }
    s.data["new_events"]
        .as_array()?
        .iter()
        .filter(|e| {
            e["kind"] == "personal_practice" && e["actor"] == s.actor && e["target"] == s.actor
        })
        .flat_map(|e| {
            [
                &e["supplied_result"]["question"],
                &e["supplied_result"]["application"],
            ]
        })
        .find(|v| v["reference"] == reference && v["record"]["owner"] == s.actor)
}

pub(in crate::neural) fn consider(
    db: &Connection,
    s: &Snapshot,
    d: &DecisionV4,
    participating: bool,
) -> Result<()> {
    let orient = matches!(
        d.next_context.appraisal,
        Some(AppraisalChoice::BeforeAction { .. })
    );
    let Some(choice) = &d.behavior else {
        if s.development_version >= 9
            && s.data["development"]["behavior_after_result"].is_object()
            && !orient
        {
            return Err(failure("The consequence of an actual choice is on your desk. Separately from any numerical change, choose behavior: {kind:local,reason}, {kind:undecided,reason}, {kind:next_time,plan} or {kind:practice,plan,action}. No duty to generalize or edit."));
        }
        return Ok(());
    };
    if s.development_version < 9 || s.model.is_none() || !s.data["context_recovery"].is_null() {
        return Err(failure(
            "A behavioral scope choice requires an ordinary observed encounter",
        ));
    }
    let (plan, actual) = match choice {
        Choice::Local { reason } | Choice::Undecided { reason } => {
            return if reason.trim().is_empty() {
                Err(failure(
                    "Give your reason for the behavioral scope you chose",
                ))
            } else {
                Ok(())
            }
        }
        Choice::NextTime { plan } => (plan, None),
        Choice::Practice { plan, action } => (plan, Some(action.endpoint(d)?)),
    };
    if !participating
        || !matches!(d.next_model, NextModel::Preserve)
        || orient
        || [
            &plan.when,
            &plan.approach,
            &plan.alternative,
            &plan.watch_for,
        ]
        .iter()
        .any(|v| v.trim().is_empty())
    {
        return Err(failure("Keep the active model and choose a condition, approach, alternative and observable prediction during accepted work"));
    }
    let selected = s.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| {
            v["considered"] == plan.outcome && experience::recall::authentic_selection(s, v)
        })
        .map(|v| &v["action_before_observation"]);
    let episode = on_desk(&s.data).chain(selected)
        .find(|v| v["owner"] == s.actor && v["outcome"] == plan.outcome && v["performed"].is_object());
    let acting = if let Some(episode) = episode {
        episode["decision"].as_str().ok_or_else(|| failure("Missing earlier acting decision"))?.to_owned()
    } else {
        experience::action::acting_from_reads(db, s, &plan.outcome)?.ok_or_else(|| failure(
            "Choose an actual result of your action. For an older result, select the acting decision and its original operation result, direct reply or feedback addressed to its exact issued edition together in next_context.references."
        ))?
    };
    observed_experience(db, s, &acting)?;
    observed_experience(db, s, &plan.outcome)?;
    let (relation, boundary) = super::consequence(db, s, &acting, &plan.outcome)?;
    let u = verify_character(db, s)?;
    let intention = json!({"origin":format!("experience:{}@1",s.invocation),"decision":acting,"outcome":plan.outcome,"when":plan.when,"approach":plan.approach,"alternative":plan.alternative,"watch_for":plan.watch_for,"preserve":plan.preserve,"scope":if actual.is_some(){"tried_in_this_decision"}else{"for_a_later_occasion"}});
    let q = write_update(
        db,
        s,
        Update {
            base: plan.base.clone(),
            question: plan.approach.clone(),
            interpretation: plan.approach.clone(),
            evidence: vec![acting, plan.outcome.clone()],
            contrary_evidence: vec![],
            prediction: plan.watch_for.clone(),
            alternatives: plan.alternative.clone(),
            disposition: Disposition::Investigating,
            revisit: Some(Revisit {
                condition: plan.when.clone(),
                event_kind: None,
                work: None,
                at_unix: None,
            }),
            follow_up: None,
            candidate_review: None,
            question_observation: None,
        },
        Some(&intention),
    )?;
    let reference = q["reference"]
        .as_str()
        .ok_or_else(|| failure("Intention has no development revision"))?;
    let question = owned(db, &s.actor, reference)?;
    let mut result = json!({"question":q,"meaning":"Your prospective behavioral experiment under the supplied character, not a finding or a numerical change."});
    if let Some(endpoint) = actual {
        let application = json!({"owner":s.actor,"work":s.work,"decision":s.invocation,"question":reference,"question_id":question.reference.id,
            "watch_for":plan.watch_for,"question_input":"self_authored_this_decision","action":endpoint,"practice":intention,"trigger_relation":relation,"trigger_boundary":boundary,
            "character_use":u.reference.handle(),"model":s.model,"epoch":s.epoch,"input":u.body["snapshot"],"provider_receipt":u.body["provider_receipt"]});
        let r = store::write(
            db,
            "question_use",
            &format!("{}/{}", question.reference.id, s.invocation),
            0,
            &application,
            Some(&s.invocation),
        )?;
        result["application"] = json!({"reference":r.handle(),"record":application});
    }
    let digest = store::put(db, &result)?;
    store::event(
        db,
        &s.work,
        &s.actor,
        &s.actor,
        "personal_practice",
        &json!({"application":result["application"]["reference"],"development":reference,"result":format!("object:{digest}")}),
        Some(&s.invocation),
    )?;
    Ok(())
}

pub(super) fn verify_character(db: &Connection, s: &Snapshot) -> Result<store::Record> {
    let (receipt, response): (String, String) = db.query_row(
        "SELECT receipt,response FROM invocations WHERE id=?1 AND actor=?2 AND state IN ('responded','accepted')",
        params![s.invocation, s.actor],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let u = store::required(db, "character_use", &s.invocation)?;
    if !store::written_by(db, &u, &s.invocation)?
        || crate::neural::character::verified_supply(db, &s.invocation, &receipt, &response)?
            .as_ref()
            != Some(&u.body)
    {
        return Err(failure(
            "Behavioral experiment is not bound to the supplied character",
        ));
    }
    Ok(u)
}

/// An observation is interpreted against the owner's actual application, not an
/// object that merely names a genuine decision or omits its restrictive endpoint.
pub(super) fn verify_application(db: &Connection, record: &store::Record) -> Result<()> {
    let b = &record.body;
    let owner = b["owner"]
        .as_str()
        .ok_or_else(|| failure("Missing practice owner"))?;
    let call = b["decision"]
        .as_str()
        .ok_or_else(|| failure("Missing practice decision"))?;
    if !store::written_by(db, record, call)?
        || record.reference.revision != 1
        || !b["question_id"]
            .as_str()
            .is_some_and(|q| record.reference.id == format!("{q}/{call}"))
    {
        return Err(failure(
            "Application is not the revision issued by the named choice",
        ));
    }
    let accepted = crate::neural::provider_evidence::accepted(db, owner, call, true)?;
    // Ordinary applications predate named endpoints. They still have to match
    // the owner's actual examining choice, observed input and active character.
    let u = verify_character(db, &accepted.snapshot)?;
    if b["character_use"] != u.reference.handle()
        || b["provider_receipt"] != u.body["provider_receipt"]
        || b["input"] != u.body["snapshot"]
    {
        return Err(failure(
            "Application names a different character supply or provider input",
        ));
    }
    let d: DecisionV4 = serde_json::from_value(accepted.response)?;
    let matching = if b["question_input"] == "self_authored_this_decision" {
        let question = store::exact(
            db,
            b["question"]
                .as_str()
                .ok_or_else(|| failure("Missing applied question"))?,
        )?;
        match &d.behavior {
            Some(Choice::Practice { plan, action }) => {
                b["action"] == action.endpoint(&d)?
                    && b["watch_for"] == plan.watch_for
                    && question.reference.kind == "development"
                    && question.reference.id == b["question_id"]
                    && store::written_by(db, &question, call)?
                    && question.body["behavioral_plan"] == b["practice"]
                    && b["practice"]["origin"] == format!("experience:{call}@1")
                    && b["practice"]["outcome"] == plan.outcome
            }
            _ => false,
        }
    } else {
        d.examining
            .iter()
            .filter(|e| e.question == b["question"] && e.watch_for == b["watch_for"])
            .any(|e| match &e.action {
                Some(a) => a.endpoint(&d).is_ok_and(|v| v == b["action"]),
                None => b["action"].is_null(),
            })
    };
    if !matching
        || b["work"] != accepted.snapshot.work
        || b["model"] != json!(accepted.snapshot.model)
        || b["epoch"] != accepted.snapshot.epoch
    {
        return Err(failure(
            "Application differs from the choice and character that produced it",
        ));
    }
    Ok(())
}

/// Bind the forecast to what the owner actually submitted, not just a matching
/// tool name in an editable question-use record.
fn reached(origin: &Value, application: &Value) -> bool {
    if origin["performed"]["endpoint"] != application["action"]
        || origin["owner"] != application["owner"]
        || origin["work"] != application["work"]
        || application["decision"]
            .as_str()
            .is_none_or(|id| origin["decision"] != format!("experience:{id}@1"))
    {
        return false;
    }
    origin["forecasts_before_action"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|f| {
            if application["question_input"] == "self_authored_this_decision" {
                let p = &f["new_plan_then"];
                p.is_object()
                    && p["watch_for"] == application["watch_for"]
                    && [
                        "outcome",
                        "when",
                        "approach",
                        "alternative",
                        "watch_for",
                        "preserve",
                    ]
                    .iter()
                    .all(|k| p[*k] == application["practice"][*k])
            } else {
                f["question_then"] == application["question"]
                    && f["watch_for"] == application["watch_for"]
            }
        })
}

/// Exact results of the specified endpoint. Neither dispatch, a nearby message,
/// nor a successful lookup of an old choice counts as an observed response to a reply.
pub(super) fn responses(db: &Connection, body: &Value, linked: Vec<Value>) -> Result<Vec<Value>> {
    let ep = &body["action"];
    let (owner, work, call) = (
        body["owner"]
            .as_str()
            .ok_or_else(|| failure("Practice lacks an owner"))?,
        body["work"]
            .as_str()
            .ok_or_else(|| failure("Practice lacks a work"))?,
        body["decision"]
            .as_str()
            .ok_or_else(|| failure("Practice lacks a decision"))?,
    );
    let mut out = vec![];
    if ep["kind"] == "operation" {
        let mut q=db.prepare_cached("SELECT e.seq FROM events e JOIN objects b ON b.digest=e.digest WHERE e.work=?1 AND e.actor=?2 AND e.target=?2 AND e.invocation=?3 AND e.kind='tool_result' AND json_extract(CAST(b.bytes AS TEXT),'$.operation')=?4 ORDER BY e.seq")?;
        for seq in q.query_map(params![work, owner, call, ep["id"].as_str()], |r| {
            r.get::<_, i64>(0)
        })? {
            let n = seq?;
            let event = format!("experience:event-{n}@1");
            if experience::action::origin(db, owner, work, &event)
                .ok()
                .flatten()
                .is_some_and(|v| reached(&v, body))
            {
                out.push(json!({"experience":event,"sequence":n,"author":owner,"kind":"tool_result","relation":"operation_result","target":ep["id"]}));
            }
        }
    }
    for row in linked {
        if let Some(event) = row["experience"].as_str() {
            if experience::action::origin(db, owner, work, event)
                .ok()
                .flatten()
                .is_some_and(|v| reached(&v, body))
            {
                out.push(row)
            }
        }
    }
    out.sort_by_key(|r| r["sequence"].as_i64());
    Ok(out)
}
