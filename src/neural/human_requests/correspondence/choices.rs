use super::Event;
use crate::neural::{
    accepted_choice::Choice, context, experience, failure, operations, tools, Result,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

impl Choice {
    pub(super) fn response(
        &self,
        db: &Connection,
        event: &Event,
        report: &Event,
        body: &Value,
    ) -> Result<()> {
        let s = &self.accepted.snapshot;
        if !self.contains(event.seq)
            || event.seq <= report.seq
            || report.kind != "user_message"
            || body["reply_to"] != report.reference()
        {
            return Err(failure(
                "The requester response is not subsequent to this addressed human report",
            ));
        }
        let parent = report.reference();
        operations::check_observed_reply(db, s, "user", &parent)?;
        let recorded = experience::read(db, Some(&s.actor), &parent)?;
        // A full observed event takes precedence over a retained reply address.
        // An address alone binds the sender, recipient and exact event, without
        // crediting a read of the reported text or any attached files.
        let incoming = s.data["new_events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["experience"] == parent);
        let reads = context::supplied_resources(&s.data);
        let mut full = false;
        for e in incoming {
            full = true;
            if e["event"] != report.seq
                || e["actor"] != report.author
                || e["target"] != report.recipient
                || e["kind"] != report.kind
                || e["digest"] != report.digest
                || e["created"] != report.time
                || e.get("invocation") != Some(&Value::Null)
                || e["body"] != recorded["record"]["body"]
            {
                return Err(failure(
                    "The report differs from the event supplied to the responding persona",
                ));
            }
        }
        for read in reads
            .iter()
            .filter(|r| r.value["reference"] == parent && r.value["record"].is_object())
        {
            full = true;
            if read.value["record"] != recorded["record"] {
                return Err(failure("The report differs from the selected experience"));
            }
        }
        if !full
            && !s.data["reply_targets"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| {
                    v["experience"] == parent
                        && v["author"] == report.author
                        && v["kind"] == report.kind
                        && v["recipient_for_reply"] == "user"
                        && v["received_at"] == report.time
                })
        {
            return Err(failure(
                "The actual report sender differs from the supplied reply address",
            ));
        }
        match event.kind.as_str() {
            "reply" => {
                let count: i64 = db.query_row("SELECT count(*) FROM events WHERE work=?1 AND actor=?2 AND invocation=?3 AND kind='reply'",params![s.work,s.actor,s.invocation],|r|r.get(0))?;
                if count != 1
                    || body["operational_persona"] != true
                    || !body["text"].is_string()
                    || body["text"] != self.accepted.response["reply"]
                    || body["reply_to"] != self.accepted.response["reply_to"]
                {
                    return Err(failure(
                        "The published reply differs from its author's accepted response",
                    ));
                }
            }
            "message" => {
                let (op, result, seq) =
                    self.operation(db, "peer.message", "event", &json!(event.seq))?;
                let message: tools::Message = serde_json::from_value(op.input)?;
                if seq <= event.seq
                    || message.to != "user"
                    || result["delivered_to_inbox"] != true
                    || serde_json::to_value(message)? != *body
                {
                    return Err(failure(
                        "The published message differs from its author's accepted operation",
                    ));
                }
            }
            _ => return Err(failure("Expected a requester reply or message")),
        }
        Ok(())
    }
}
