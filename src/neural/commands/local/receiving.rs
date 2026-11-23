use super::*;
use crate::neural::human_requests::{self, SheetDelivery};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Receiving {
    /// Exact issued edition. An earlier result.deliver in the same decision can be named by $id.
    pub delivery: String,
    /// Omit to receive the entire unpacked files ZIP. Otherwise use the one-based dispatch
    /// slip number. A single-file dispatch arrives as its original file with its basename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<usize>,
    /// Which use of the files you intend to exercise. You choose the executable and arguments.
    pub purpose: String,
}

impl Receiving {
    pub(super) fn files(&self, db: &Connection) -> Result<BTreeMap<String, String>> {
        let edition = store::exact(db, &self.delivery)?;
        if edition.reference.kind != "delivery"
            || edition.reference.handle() != self.delivery
            || self.purpose.trim().is_empty()
        {
            return Err(failure(
                "Choose an exact delivery revision and describe the use to check",
            ));
        }
        let package: SheetDelivery = serde_json::from_value(edition.body)?;
        let selected = match self.dispatch {
            Some(number) => &package.dispatch(number)?.artifacts,
            None => &package.artifacts,
        };
        let mut files = human_requests::files(db, selected)?;
        if self.dispatch.is_some() && files.len() == 1 {
            let path = &mut files[0].0;
            if let Some((_, name)) = path.rsplit_once('/') {
                *path = name.to_owned();
            }
        }
        Ok(files.into_iter().collect())
    }
}

/// Part of the same transaction as the terminal outbox result. A check can be
/// made by a colleague in another activity. Notify the original issuer once.
pub(in crate::neural) fn publish(
    db: &Connection,
    actor: &str,
    call: &str,
    effect: &str,
    result: &Value,
    digest: &str,
) -> Result<()> {
    let run = &result["result"];
    let Some(subject) = run.get("receiving") else {
        return Ok(());
    };
    let subject: Receiving = serde_json::from_value(subject.clone())?;
    let edition = store::exact(db, &subject.delivery)?;
    let work = edition.body["work"]
        .as_str()
        .ok_or_else(|| failure("Missing issuing activity"))?;
    let owner = edition.body["owner"]
        .as_str()
        .ok_or_else(|| failure("Missing issuing persona"))?;
    let notice = json!({
        "delivery":subject.delivery, "dispatch":subject.dispatch, "purpose":subject.purpose,
        "checking_work":run["work"], "effect":effect, "receipt":result["receipt"],
        "result":format!("object:{digest}"), "program":run["argv0"],
        "files_staged":run["files_supplied"].as_object().map(|f| f.len()),
        "process_started":run["process_started"], "exit_code":run["exit_code"],
        "success":run["success"], "timed_out":run["timed_out"], "cancelled":run["cancelled"],
        "missing_outputs":run["missing_outputs"], "error":run["error"]
    });
    store::event(
        db,
        work,
        actor,
        owner,
        "delivery_check",
        &notice,
        Some(call),
    )?;
    Ok(())
}

/// A small pointer for rediscovery. Do not replay commands, file lists or
/// streams into every decision after a package has been checked.
pub(in crate::neural) fn latest(db: &Connection, work: &str) -> Result<BTreeMap<String, Value>> {
    let mut statement = db.prepare(
        "SELECT seq,actor,digest FROM (
            SELECT e.seq,e.actor,e.digest,row_number() OVER (
                PARTITION BY json_extract(CAST(o.bytes AS TEXT),'$.delivery') ORDER BY e.seq DESC
            ) AS recency FROM events e JOIN objects o ON o.digest=e.digest
            WHERE e.work=?1 AND e.kind='delivery_check'
        ) WHERE recency=1",
    )?;
    let mut rows = statement.query([work])?;
    let mut checks = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let body: Value = store::object(db, &row.get::<_, String>(2)?)?;
        let edition = body["delivery"]
            .as_str()
            .ok_or_else(|| failure("Receiving event has no edition"))?;
        let sequence: i64 = row.get(0)?;
        let actor: String = row.get(1)?;
        checks.insert(edition.to_owned(), json!({
            "event":format!("experience:event-{sequence}@1"), "actor":actor,
            "receipt":body["receipt"], "dispatch":body["dispatch"],
            "process_started":body["process_started"], "exit_code":body["exit_code"], "run_succeeded":body["success"]
        }));
    }
    Ok(checks)
}
