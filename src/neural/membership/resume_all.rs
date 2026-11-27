//! One human confirmation, ordinary addressed decision requests. Never widen a
//! reviewed group or turn an operator request into a new persona continuation.
use super::{decision_in_flight, request_resume, resume_block};
use crate::neural::{
    failure,
    protocol::{Continuation, Participation},
    store::{self, Reference},
    Result,
};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Serialize)]
pub struct Member {
    pub persona: String,
    pub participation: Reference,
    pub continuation: Continuation,
}

#[derive(Serialize)]
pub struct ResumePlan {
    pub brief: Reference,
    pub members: Vec<Member>,
    pub other_participants: usize,
    pub fingerprint: String,
}

impl ResumePlan {
    /// Read-only preview; opening the dialog cannot grant scheduling authority.
    pub fn inspect(root: &Path, work: &str) -> Result<Self> {
        let db = store::open_inspection(root)?;
        Self::at(&db, work)
    }

    fn at(db: &Connection, work: &str) -> Result<Self> {
        let activity = store::open_activity(db, work)?;
        if activity.body["id"] != work || activity.body["phase"] != "open" {
            return Err(failure("Choose an open activity to resume."));
        }
        current_edition(db, &activity.reference)?;
        let mut members = Vec::new();
        let mut other_participants = 0;
        // Participation IDs are scoped to the activity. Include identities
        // with a missing head rather than reporting them as nonparticipants.
        let prefix = format!("{work}/");
        let mut ids = db.prepare(
            "SELECT id FROM versions WHERE kind='participation' AND instr(id,?1)=1
            UNION SELECT id FROM heads WHERE kind='participation' AND instr(id,?1)=1 ORDER BY id",
        )?;
        for id in ids.query_map([&prefix], |r| r.get::<_, String>(0))? {
            let id = id?;
            let record = store::required(db, "participation", &id)?;
            current_edition(db, &record.reference)?;
            let part: Participation = serde_json::from_value(record.body)?;
            if part.work != work || id.strip_prefix(&prefix) != Some(part.persona.as_str()) {
                return Err(failure(
                    "Participant identity does not match this activity.",
                ));
            }
            if resume_block(&part).is_some() || decision_in_flight(db, work, &part.persona)? {
                other_participants += 1;
            } else {
                members.push(Member {
                    persona: part.persona,
                    participation: record.reference,
                    continuation: part.continuation,
                });
            }
        }
        let refs: Vec<_> = members.iter().map(|m| &m.participation).collect();
        let fingerprint =
            crate::store::sha256_bytes(&serde_json::to_vec(&(&activity.reference, refs))?);
        Ok(Self {
            brief: activity.reference,
            members,
            other_participants,
            fingerprint,
        })
    }
}

fn current_edition(db: &Connection, r: &Reference) -> Result<()> {
    let latest: (i64, String) = db.query_row(
        "SELECT revision,digest FROM versions WHERE kind=?1 AND id=?2 ORDER BY revision DESC LIMIT 1",
        (&r.kind, &r.id), |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if r.revision < 1 || latest.0 != r.revision || latest.1 != r.digest {
        return Err(failure(format!(
            "Cannot verify the current edition of {}.",
            r.handle()
        )));
    }
    Ok(())
}

/// The caller holds the same IMMEDIATE transaction as a single resume.
/// Either every listed participant gets one request or none do.
pub(in crate::neural) fn request_all(
    db: &Connection,
    work: &str,
    operator: &str,
    reason: &str,
    expected: &str,
) -> Result<Value> {
    let plan = ResumePlan::at(db, work)?;
    if expected != plan.fingerprint {
        return Err(failure("The brief or the list of participants to resume changed. Review the list again. No requests were sent."));
    }
    if plan.members.is_empty() {
        return Err(failure(
            "No waiting or stopped participants are available to resume.",
        ));
    }
    let requests = plan
        .members
        .iter()
        .map(|m| {
            request_resume(
                db,
                work,
                &m.persona,
                operator,
                reason,
                &m.participation.handle(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"work":work,"brief":plan.brief.handle(),"decision_requests":requests}))
}
