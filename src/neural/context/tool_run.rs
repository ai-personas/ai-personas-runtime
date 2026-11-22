//! Give a persona a sense of how long its own current method has persisted.
//! Match tool names and order, not command arguments, purpose or quality. Keep
//! older result bodies out of the input unless the persona selects them.
use crate::neural::{failure, store, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn names(decision: &Value) -> Result<Vec<&str>> {
    decision["operations"]
        .as_array()
        .ok_or_else(|| failure("Missing accepted operations"))?
        .iter()
        .map(|o| {
            o["tool"]
                .as_str()
                .ok_or_else(|| failure("Missing accepted tool name"))
        })
        .collect()
}

pub(super) fn observe(
    db: &Connection,
    work: &str,
    actor: &str,
    prior: &Value,
    decision: &Value,
    response: &str,
) -> Result<Value> {
    let tools = json!(names(decision)?);
    let preceding = &prior["data"]["decision_continuity"]["same_tool_sequence"];
    let mut count = 1u64;
    let first = |r: &str, d: &Value| json!({"response":format!("object:{r}"),"continuation":d["continuation"]});
    let mut start = first(response, decision);
    if !preceding.is_null() {
        // Prepared from the last accepted choice, not from a proposal still in flight.
        if preceding["tools"] == tools {
            count += preceding["accepted_decisions"]
                .as_u64()
                .ok_or_else(|| failure("Missing accepted sequence length"))?;
            start = preceding["first"].clone();
        }
    } else {
        // A listener can attach to already-running work. Reconstruct the actual
        // suffix once; subsequent inputs extend or reset the recorded observation.
        let mut q = db.prepare("SELECT response,snapshot FROM invocations WHERE work=?1 AND actor=?2 AND state='accepted' AND response IS NOT NULL ORDER BY rowid DESC LIMIT -1 OFFSET 1")?;
        let mut rows = q.query(params![work, actor])?;
        while let Some(row) = rows.next()? {
            let r: String = row.get(0)?;
            let d: Value = store::object(db, &r)?;
            if tools != json!(names(&d)?) {
                break;
            }
            let s: Value = store::object(db, &row.get::<_, String>(1)?)?;
            if s["model"].is_null() {
                break;
            }
            count += 1;
            start = first(&r, &d);
        }
    }
    // A receipt that informed the previous choice can be combined with a new
    // observation without rerunning either operation or replaying its contents.
    let pointers = prior["data"]["new_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            if e["kind"] != "tool_result" || e["body"]["context"] != "complete" {
                return None;
            }
            let result = e["body"]["result"].as_str()?;
            let mut item = json!({"tool":e["body"]["tool"],"result":result});
            for stream in ["stdout", "stderr"] {
                let output = &e["supplied_result"]["result"][stream];
                if output["bytes"].as_u64().is_some_and(|n| n > 0) {
                    if let Some(object) = output["object"].as_str() {
                        item[stream] = json!(object);
                    }
                }
            }
            Some(item)
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"tools":tools,"accepted_decisions":count,"first":start,"results_available_to_previous_decision":pointers,
        "meaning":"Consecutive accepted decisions of yours here with these tool names in this order. Arguments, reads, writes and consequences may differ. A receipt pointer is not its contents. No limit or quality verdict."}),
    )
}
