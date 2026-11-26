//! Inspect a completed response the runtime refused, without replaying adoption.
//! A later completion and a later input containing the refusal are distinct.
mod inputs;
use super::{context::Snapshot, Event};
use crate::neural::{
    accepted_choice::Choice, character, development, failure, provider_evidence, store, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

struct Refused {
    input: Snapshot,
    response: Option<Value>,
    links: Value,
}

impl Refused {
    fn load(db: &Connection, event: &Event) -> Result<Self> {
        let call = event
            .invocation
            .as_deref()
            .ok_or_else(|| failure("Missing rejected invocation"))?;
        if event.kind != "decision_rejected"
            || event.actor != "runtime"
            || event.body["invocation"] != call
            || event.body["effects_adopted"] != false
            || event.body["reason"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err(failure(
                "Choose the runtime’s recorded rejection for this response",
            ));
        }
        let (source, reply, receipt): (String, String, String) = db.query_row(
            "SELECT snapshot,response,receipt FROM invocations WHERE id=?1 AND actor=?2 AND work=?3 AND state='rejected'",
            params![call,event.target,event.work], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let input: Snapshot = store::object(db, &source)?;
        if input.actor != event.target
            || input.work != event.work
            || input.invocation != call
            || input.observation_through.unwrap_or(input.cursor) >= event.seq
            || event.body["rejected_response"] != format!("object:{reply}")
        {
            return Err(failure("Rejection notice differs from its completed call"));
        }
        let untouched: bool = db.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM outbox WHERE invocation=?1)
            AND NOT EXISTS(SELECT 1 FROM events WHERE invocation=?1 AND seq<>?2)
            AND NOT EXISTS(SELECT 1 FROM heads WHERE kind='decision_effects' AND id=?1)
            AND NOT EXISTS(SELECT 1 FROM versions WHERE kind='decision_effects' AND id=?1)
            AND NOT EXISTS(SELECT 1 FROM versions WHERE invocation=?1 AND NOT
                (kind IN ('provider_completion','character_use') AND id=?1 AND revision=1))",
            params![call, event.seq],
            |r| r.get(0),
        )?;
        if !untouched {
            return Err(failure(
                "This call has records inconsistent with an unadopted response",
            ));
        }
        store::verify_completion(
            db,
            call,
            &event.target,
            &event.work,
            &source,
            &reply,
            &receipt,
            "responded",
        )?;
        let attempt: Value = store::object(db, &receipt)?;
        character::verify_input(db, &input, call, &attempt)?;
        let bytes = store::bytes(db, &reply)?;
        let matched = provider_evidence::check_final(db, &receipt, &attempt, &bytes)?;
        // Bad JSON, a missing field or a wrong invocation in the response can
        // itself be the reason for refusal. Never repair or execute that text.
        let response = crate::files::parse_json_unique(&bytes)
            .ok()
            .filter(Value::is_object);
        let links = json!({"input":format!("object:{source}"),"response":format!("object:{reply}"),
            "receipt":format!("object:{receipt}"),"binding":if matched {"provider_output"} else {"provider_completion"}});
        Ok(Self {
            input,
            response,
            links,
        })
    }
}

fn proposal(r: &Value) -> Value {
    let operations = r["operations"].as_array().map(|ops| {
        ops.iter()
            .map(|o| json!({"id":o["id"].as_str(),"tool":o["tool"].as_str()}))
            .collect::<Vec<_>>()
    });
    json!({"named_invocation":r["invocation"].as_str(),"status":r["work"]["status"].as_str(),"continuation":r["continuation"]["kind"].as_str(),
        "next_step":r["continuation"]["purpose"].as_str().or(r["continuation"]["reason"].as_str()),"operations":operations,
        "asset_maintenance":r.get("asset_maintenance")})
}

fn received(input: &Snapshot, event: &Event) -> Result<bool> {
    if input.work != event.work || input.actor != event.target {
        return Err(failure("Later input belongs to another participation"));
    }
    if !input.data["context_recovery"].is_null() {
        return Ok(false);
    }
    let events = input.data["new_events"]
        .as_array()
        .ok_or_else(|| failure("Later input has no inbox register"))?;
    let reference = event.reference();
    let mut named = events
        .iter()
        .filter(|v| v["event"] == event.seq || v["experience"] == reference);
    let Some(entry) = named.next() else {
        return Ok(false);
    };
    if input.observation_through.unwrap_or(input.cursor) < event.seq
        || named.next().is_some()
        || !event.incoming_matches(entry)
    {
        return Err(failure(
            "Later inbox entry differs from the rejection notice",
        ));
    }
    Ok(true)
}

fn following(db: &Connection, origin: &Event, first_input: &Snapshot) -> Result<Vec<Value>> {
    // Admission order, not seconds on the clock. Keep intermediate refusals;
    // the next accepted choice need not be a retry, or even receive this notice.
    let mut q = db.prepare("SELECT id,state,snapshot FROM invocations WHERE actor=?1 AND work=?2
        AND state IN ('rejected','accepted') AND rowid>(SELECT rowid FROM invocations WHERE id=?3) ORDER BY rowid")?;
    let mut rows = q.query(params![origin.target, origin.work, origin.invocation])?;
    let mut following = Vec::new();
    while let Some(row) = rows.next()? {
        let call = row.get_ref(0)?.as_str()?;
        let state = row.get_ref(1)?.as_str()?;
        let result = (|| -> Result<Value> {
            let input = format!("object:{}", row.get_ref(2)?.as_str()?);
            if state == "accepted" {
                let choice = Choice::recorded(db, &origin.target, call)?;
                let a = &choice.accepted;
                let inbox = received(&a.snapshot, origin)?;
                return Ok(
                    json!({"invocation":call,"state":state,"received_notice":inbox,"choice":proposal(&a.response),
                    "binding":a.binding,"decision":format!("experience:{call}@1"),"input":input,
                        "compared_inputs":inputs::compare(first_input,&a.snapshot)}),
                );
            }
            let seq = db.query_row(
                "SELECT seq FROM events WHERE invocation=?1 AND kind='decision_rejected'",
                [call],
                |r| r.get(0),
            )?;
            let e = Event::get(db, seq)?.ok_or_else(|| failure("Missing later rejection"))?;
            if seq <= origin.seq {
                return Err(failure("Later response has an earlier rejection notice"));
            }
            let a = Refused::load(db, &e)?;
            let inbox = received(&a.input, origin)?;
            Ok(
                json!({"invocation":call,"state":state,"received_notice":inbox,"notice_event":seq,"notice_source":format!("object:{}",e.digest),"binding":a.links["binding"],
                "choice":a.response.as_ref().map(proposal),"input":input,"compared_inputs":inputs::compare(first_input,&a.input)}),
            )
        })();
        following.push(
            result.unwrap_or_else(|e| json!({"invocation":call,"unavailable":e.to_string()})),
        );
        if state == "accepted" {
            break;
        }
    }
    Ok(following)
}

fn report(db: &Connection, seq: i64) -> Result<Value> {
    let e = Event::get(db, seq)?.ok_or_else(|| failure("Missing rejection notice"))?;
    let a = Refused::load(db, &e)?;
    let s = &a.input;
    let prior_evidence =
        s.data["decision_continuity"]["previous_work_decision"]["judgment"]["evidence"].as_array();
    let citations = a.response.as_ref().and_then(|r| r["considered_experience"].as_array()).map(|rs| rs.iter().map(|v| {
        let reference = v.as_str();
        json!({"reference":reference,"observation":reference.and_then(|r| development::experience_basis(s,r)),
            "in_previous_judgment":reference.is_some() && prior_evidence.is_some_and(|items| items.contains(v))})
    }).collect::<Vec<_>>());
    let selected = s.data["selected_resources"].as_array().map(|rows| rows.iter().map(|v| {
        let form = if !s.data["context_recovery"].is_null() { "recovery_metadata" }
            else if v["text"].is_string() { "text" } else if v["record"].is_object() { "record" }
            else if s.image_blobs.iter().any(|(id,mime)| v["image_blob"]==*id && v["mime"]==*mime) { "image" }
            else if v["binary"]==true { "binary_metadata" }
            else if v["supplied_in"].is_string() { "pointer" } else { "other" };
        json!({"reference":v["reference"].as_str(),"path":v["workspace_path"].as_str().or(v["metadata"]["path"].as_str()),
            "form":form,"range":matches!(form,"text"|"image").then_some(&v["range"]),"bytes":v["bytes"].as_u64()})
    }).collect::<Vec<_>>());
    Ok(
        json!({"event":seq,"notice":e.reference(),"source":format!("object:{}",e.digest),"reason":e.body["reason"],
        "work":e.work,"persona":e.target,"invocation":s.invocation,"records":a.links,"proposed":a.response.as_ref().map(proposal),
        "cited_for_character":citations,"selected":selected,"incoming_count":s.data["new_events"].as_array().map(Vec::len),
        "following":following(db,&e,s)?}),
    )
}

/// One committed observation; no adoption, scheduling or current-state replay.
pub fn inspect(db: &Connection, event: i64) -> Result<Value> {
    if !db.is_autocommit() {
        return report(db, event);
    }
    let tx = db.unchecked_transaction()?;
    let value = report(&tx, event)?;
    tx.commit()?;
    Ok(value)
}
