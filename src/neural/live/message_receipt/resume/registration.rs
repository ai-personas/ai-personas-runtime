use super::*;
use crate::neural::{
    cli::operator_request_address,
    protocol::{Continuation, Participation, PendingResume},
};

pub(super) struct Registration {
    pub prior: Record,
    pub first: Record,
    pub last_pending: i64,
    pub registered_write: i64,
    pub cleared_write: Option<i64>,
    pub clearing_writer: Option<String>,
    pub operator_receipt: Option<String>,
}

impl Registration {
    pub(super) fn read(db: &Connection, e: &Event) -> Result<Self> {
        let handle = e.body["previous_participation"]
            .as_str()
            .ok_or_else(|| failure("Missing preserved participation"))?;
        let prior = store::exact(db, handle)?;
        let id = format!("{}/{}", e.work, e.target);
        let mut p: Participation = serde_json::from_value(prior.body.clone())?;
        if e.invocation.is_some()
            || e.actor.trim().is_empty()
            || e.body["reason"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty())
            || prior.reference.kind != "participation"
            || prior.reference.id != id
            || prior.reference.handle() != handle
            || prior.reference.revision < 1
            || p.work != e.work
            || p.persona != e.target
            || p.cursor >= e.seq
            || p.declined
            || p.departure.is_some()
            || p.closed_by_activity.is_some()
            || p.pending_resume.is_some()
            || matches!(p.continuation, Continuation::Ready { .. })
        {
            return Err(failure(
                "Not an operator request against this participant’s wait or stop",
            ));
        }
        let n = successor(prior.reference.revision)?;
        let first = store::exact(db, &format!("participation:{id}@{n}"))?;
        p.pending_resume = Some(PendingResume {
            requested_by: e.actor.clone(),
            event: e.reference(),
        });
        if first.body != serde_json::to_value(p)? {
            return Err(failure(
                "The registered participation differs from the preserved choice and request",
            ));
        }
        let (brief_rev, registered_write):(i64,i64)=db.query_row("SELECT w.revision,p.rowid FROM versions w JOIN versions p ON p.kind='participation' AND p.id=?1 AND p.revision=?2
            AND p.digest=?3 AND p.invocation IS NULL JOIN versions old ON old.kind=p.kind AND old.id=p.id AND old.revision=?5 AND old.rowid<p.rowid
            WHERE w.kind='work' AND w.id=?4 AND w.rowid<p.rowid ORDER BY w.rowid DESC LIMIT 1",
            params![id,n,first.reference.digest,e.work,prior.reference.revision], |r|Ok((r.get(0)?,r.get(1)?)))?;
        let brief = store::exact(db, &format!("work:{}@{brief_rev}", e.work))?;
        if e.body["brief"] != brief.reference.handle()
            || brief.body["id"] != e.work
            || brief.body["phase"] != "open"
        {
            return Err(failure(
                "The brief differs from the open edition at registration",
            ));
        }
        let operator_receipt = operator_receipt(db, e, &prior, &first)?;

        // A model change in another activity can invalidate an appraisal here
        // without consuming the permission. Stop at the first edition without
        // this marker, never a later re-invitation or second request.
        let mut last_pending = n;
        let mut previous_write = registered_write;
        let mut cleared_write = None;
        let mut clearing_writer = None;
        let mut q=db.prepare("SELECT revision,digest,invocation,rowid FROM versions WHERE kind='participation' AND id=?1 AND revision>?2 ORDER BY revision")?;
        let mut rows = q.query(params![id, n])?;
        while let Some(row) = rows.next()? {
            let r: i64 = row.get(0)?;
            let write: i64 = row.get(3)?;
            if successor(last_pending)? != r || write <= previous_write {
                return Err(failure(
                    "Missing or out-of-order participation edition after this request",
                ));
            }
            let b: Value = store::object(db, row.get_ref(1)?.as_str()?)?;
            if b["work"] != e.work || b["persona"] != e.target {
                return Err(failure("Participation changed identity after registration"));
            }
            if b["pending_resume"] != first.body["pending_resume"] {
                cleared_write = Some(write);
                clearing_writer = row.get(2)?;
                break;
            }
            if b["continuation"] != first.body["continuation"]
                || b["cursor"] != first.body["cursor"]
            {
                return Err(failure(
                    "Pending permission no longer preserves the original choice and cursor",
                ));
            }
            previous_write = write;
            last_pending = r;
        }
        let head = store::required(db, "participation", &id)?;
        let pending = cleared_write.is_none();
        if store::exact(db, &head.reference.handle())?.reference != head.reference
            || head.reference.revision < last_pending
            || (head.reference.revision == last_pending) != pending
            || (head.body["pending_resume"] == first.body["pending_resume"]) != pending
        {
            return Err(failure(
                "Current participation does not agree with the request history",
            ));
        }
        Ok(Self {
            prior,
            first,
            last_pending,
            clearing_writer,
            registered_write,
            cleared_write,
            operator_receipt,
        })
    }
}

/// The event and permission write exist even for an operator who did not
/// supply an idempotency key. If there's a receipt, don't ignore a conflict.
fn operator_receipt(
    db: &Connection,
    e: &Event,
    prior: &Record,
    first: &Record,
) -> Result<Option<String>> {
    let base = prior.reference.handle();
    let registered = first.reference.handle();
    let mut q=db.prepare("SELECT v.id,v.invocation,v.digest FROM versions v JOIN objects o ON o.digest=v.digest WHERE v.kind='operator_request' AND v.revision=1 AND
        (json_extract(CAST(o.bytes AS TEXT),'$.command.resume.expected')=?1 OR json_extract(CAST(o.bytes AS TEXT),'$.result.pending_resume.event')=?2 OR json_extract(CAST(o.bytes AS TEXT),'$.result.participation')=?3)")?;
    let mut rows = q.query(params![base, e.reference(), registered])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let id: String = row.get(0)?;
    let receipt: Value = store::object(db, row.get_ref(2)?.as_str()?)?;
    let key = receipt["request_id"]
        .as_str()
        .ok_or_else(|| failure("Missing operator request ID"))?;
    let result = &receipt["result"];
    if row.get::<_, Option<String>>(1)?.is_some()
        || rows.next()?.is_some()
        || receipt["operator"] != e.actor
        || operator_request_address(&e.actor, key)? != id
        || receipt["command"]
            != json!({"resume":{"work":e.work,"persona":e.target,"reason":e.body["reason"],"expected":base}})
        || result["work"] != e.work
        || result["resumed"] != e.target
        || result["previous_participation"] != base
        || result["participation"] != registered
        || result["pending_resume"] != first.body["pending_resume"]
        || result["preserved_continuation"] != first.body["continuation"]
    {
        return Err(failure(
            "Operator receipt differs from this decision request",
        ));
    }
    Ok(Some(format!("operator_request:{id}@1")))
}
