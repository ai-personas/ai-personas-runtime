//! Posting order is not reading order. A reciprocal message may have reached
//! the inbox after a decision's input was frozen but before its send committed.
use super::{failure, store, Result};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) struct Window {
    pub input_through: i64,
    pub arrivals: Vec<i64>,
}

impl Window {
    // The recipient and sender can already discover these *same-activity*
    // exchanges. Supply a bounded address, not either party's private input,
    // or another automatic copy of old message bodies and attachments.
    pub fn cue(&self) -> Option<Value> {
        self.arrivals.last().map(|latest| {
            json!({
                "count":self.arrivals.len(),"latest":format!("experience:event-{latest}@1")
            })
        })
    }
}

pub(super) fn before_send(
    db: &Connection,
    work: &str,
    sender: &str,
    recipient: &str,
    through: Option<i64>,
    cursor: i64,
    sent: i64,
) -> Result<Option<Window>> {
    let Some(input_through) = through else {
        return Ok(None);
    };
    if sender == recipient || matches!(recipient, "user" | "*") {
        return Ok(None);
    }
    // The participation cursor is filtered and is NOT a station-wide clock.
    if input_through < cursor || input_through >= sent || input_through < 0 {
        return Err(failure(
            "Peer message has an inconsistent sending input boundary",
        ));
    }
    let mut q = db.prepare(
        "SELECT seq FROM events WHERE work=?1 AND target=?2 AND actor=?3
        AND seq>?4 AND seq<?5 AND kind='message' AND invocation IS NOT NULL ORDER BY seq",
    )?;
    let arrivals = q
        .query_map(params![work, sender, recipient, input_through, sent], |r| {
            r.get(0)
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(Window {
        input_through,
        arrivals,
    }))
}

#[derive(Deserialize)]
struct InputBoundary {
    actor: String,
    work: String,
    invocation: String,
    cursor: i64,
    observation_through: Option<i64>,
}

/// Enrich this inbox projection, not the published message or its digest.
/// No timing is inferred from wall-clock seconds, nearby turns or a reply link.
pub(super) fn annotate_incoming(
    db: &Connection,
    work: &str,
    reader: &str,
    event: &mut Value,
) -> Result<()> {
    if event["kind"] != "message" || event["target"] != reader || event["actor"] == reader {
        return Ok(());
    }
    let (Some(call), Some(sender), Some(sent)) = (
        event["invocation"].as_str(),
        event["actor"].as_str(),
        event["event"].as_i64(),
    ) else {
        return Ok(());
    };
    let (source, response, receipt): (String,String,String) = db.query_row(
        "SELECT snapshot,response,receipt FROM invocations WHERE id=?1 AND actor=?2 AND work=?3 AND state='accepted'",
        params![call,sender,work], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let boundary: InputBoundary = store::object(db, &source)?;
    if boundary.actor != sender || boundary.work != work || boundary.invocation != call {
        return Err(failure("Peer message does not identify its sender’s input"));
    }
    if boundary.observation_through.is_none() {
        return Ok(());
    }
    store::verify_completion(
        db,
        call,
        sender,
        work,
        &source,
        &response,
        &receipt,
        "responded",
    )?;
    if let Some(cue) = before_send(
        db,
        work,
        sender,
        reader,
        boundary.observation_through,
        boundary.cursor,
        sent,
    )?
    .and_then(|w| w.cue())
    {
        event["your_messages_after_sender_input"] = cue;
    }
    Ok(())
}
