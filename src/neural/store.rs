use super::{failure, now, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub kind: String,
    pub id: String,
    pub revision: i64,
    pub digest: String,
}
impl Reference {
    pub fn handle(&self) -> String {
        format!("{}:{}@{}", self.kind, self.id, self.revision)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub reference: Reference,
    pub body: Value,
}

pub struct Store {
    pub root: PathBuf,
    pub db: Connection,
}

impl Store {
    pub fn create(root: &Path, initialize: impl FnOnce(&Transaction<'_>)->Result<()>) -> Result<Self> {
        let mut folders=std::fs::DirBuilder::new();
        folders.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            folders.mode(0o700);
        }
        folders.create(root)?;
        let root=root.canonicalize()?;
        let stage=tempfile::Builder::new().prefix(".personas-init-").tempdir_in(&root)?;
        let file=tempfile::NamedTempFile::new_in(stage.path())?;
        let mut db=Connection::open(file.path())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            BEGIN IMMEDIATE;
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO metadata VALUES('format','neural-runtime-v2');
            CREATE TABLE objects(digest TEXT PRIMARY KEY, media TEXT NOT NULL, bytes BLOB NOT NULL);
            CREATE TABLE heads(kind TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,digest TEXT NOT NULL REFERENCES objects(digest),PRIMARY KEY(kind,id));
            CREATE TABLE versions(kind TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,digest TEXT NOT NULL REFERENCES objects(digest),invocation TEXT,created INTEGER NOT NULL,PRIMARY KEY(kind,id,revision));
            CREATE TABLE events(seq INTEGER PRIMARY KEY AUTOINCREMENT,work TEXT NOT NULL,actor TEXT NOT NULL,target TEXT NOT NULL,kind TEXT NOT NULL,digest TEXT NOT NULL REFERENCES objects(digest),invocation TEXT,created INTEGER NOT NULL);
            CREATE INDEX event_delivery ON events(work,target,seq);
            CREATE TABLE invocations(id TEXT PRIMARY KEY,work TEXT NOT NULL,actor TEXT NOT NULL,state TEXT NOT NULL,snapshot TEXT NOT NULL REFERENCES objects(digest),response TEXT REFERENCES objects(digest),receipt TEXT REFERENCES objects(digest),created INTEGER NOT NULL,updated INTEGER NOT NULL);
            CREATE UNIQUE INDEX one_decision_authority ON invocations(actor) WHERE state IN ('prepared','dispatched','responded');
            CREATE INDEX invocation_work ON invocations(work,created);
            CREATE TABLE outbox(id TEXT PRIMARY KEY,invocation TEXT NOT NULL REFERENCES invocations(id),ordinal INTEGER NOT NULL,work TEXT NOT NULL,actor TEXT NOT NULL,operation TEXT NOT NULL REFERENCES objects(digest),state TEXT NOT NULL,result TEXT REFERENCES objects(digest),UNIQUE(invocation,ordinal));
            CREATE INDEX pending_outbox ON outbox(state,invocation,ordinal);
            CREATE TABLE controls(scope TEXT PRIMARY KEY,state TEXT NOT NULL,revision INTEGER NOT NULL,reason TEXT NOT NULL,updated INTEGER NOT NULL);
            CREATE TABLE asset_index(id TEXT PRIMARY KEY,path TEXT NOT NULL,owner TEXT NOT NULL,cues TEXT NOT NULL);
            CREATE INDEX asset_paths ON asset_index(path);
            COMMIT;")?;
        let transaction=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        initialize(&transaction)?;
        transaction.commit()?;
        db.close().map_err(|(_,e)|e)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(root.join("neural.sqlite")).map_err(|e|e.error)?;
        #[cfg(unix)]
        std::fs::File::open(&root)?.sync_all()?;
        Self::open(&root)
    }

    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let db = Connection::open_with_flags(
            root.join("neural.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
        let format: String =
            db.query_row("SELECT value FROM metadata WHERE key='format'", [], |r| {
                r.get(0)
            })?;
        if format != "neural-runtime-v2" {
            return Err(failure("unsupported neural runtime format"));
        }
        Ok(Self { root, db })
    }

    pub fn transaction<T>(&mut self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    pub fn snapshot<T>(&mut self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    pub fn control(&mut self, scope: &str, state: &str, reason: &str, origin: super::controls::Origin<'_>) -> Result<Reference> {
        self.transaction(|db| super::controls::change(db, scope, state, reason, origin, None))
    }

    pub fn status(&mut self) -> Result<Value> {
        self.snapshot(|db| status(db))
    }
}

/// An operator inspection needs neither a writable database nor a runner lock.
/// The first read pins a single version for every receipt and model in the view.
/// Dropping the connection releases the read transaction.
pub fn open_inspection(root: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(root.join("neural.sqlite"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.execute_batch("BEGIN DEFERRED")?;
    let format: String = db.query_row("SELECT value FROM metadata WHERE key='format'", [], |row| row.get(0))?;
    if format != "neural-runtime-v2" { return Err(failure("Unsupported workspace format")); }
    Ok(db)
}

/// Read the workspace projection inside the caller's existing transaction.
/// Desktop inspection can therefore combine status, messages and artifact
/// metadata at one SQLite snapshot without acquiring the execution writer.
pub fn status(db: &Connection) -> Result<Value> {
    let mut invocations = db.prepare("SELECT state,COUNT(*) FROM invocations GROUP BY state")?;
    let counts = invocations.query_map([], |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<std::collections::BTreeMap<_,_>,_>>()?;
    let mut effects = db.prepare("SELECT state,COUNT(*) FROM outbox GROUP BY state")?;
    let effect_counts = effects.query_map([], |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<std::collections::BTreeMap<_,_>,_>>()?;
    let mut scopes=db.prepare("SELECT scope FROM controls ORDER BY scope")?;
    let scopes=scopes.query_map([],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let controls=scopes.iter().map(|scope|super::controls::current(db,scope)).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect::<Vec<_>>();
    let outcomes=list(db,"outcome")?;
    let ownership=super::handoff::ownership(db,&outcomes)?;
    Ok(json!({"format":"neural-runtime-v2","personas":list(db,"persona")?,
        "work":list(db,"work")?,"participation":list(db,"participation")?,
        "installations":list(db,"installation")?,
        "development":list(db,"development")?,"question_applications":super::development::applications(db,None,None)?,
        "acceptances":list(db,"acceptance")?,"delivery_reviews":delivery_reviews(db)?,"delivery_mandates":delivery_mandates(db,None)?,
        "outcomes":outcomes,"outcome_ownership":ownership,"handoffs":super::handoff::offers(db,None)?,"deliveries":list(db,"delivery")?,"human_requests":list(db,"human_request")?,
        "jobs":job_index(db,false)?,
        "invocations":counts,"operations":effect_counts,"controls":controls,
        "meaning":"Stored activity, authored judgments and retained deliveries; not an independent quality verdict."}))
}

/// Human reviews apply to the exact delivered revision. The committed write
/// order also covers older review records that had no timestamp in their body.
/// Recover the brief that actually reached the accepted authoring call, including
/// for earlier deliveries whose immutable body stored only the activity ID.
pub fn delivery_mandates(db: &Connection, work: Option<&str>) -> Result<std::collections::BTreeMap<String, Value>> {
    let mut query = db.prepare("SELECT h.id,h.revision,i.work,w.revision,i.snapshot,
        json_extract(CAST(s.bytes AS TEXT),'$.data.work.reference')
        FROM heads h JOIN versions v ON v.kind=h.kind AND v.id=h.id AND v.revision=h.revision
        JOIN objects b ON b.digest=h.digest JOIN invocations i ON i.id=v.invocation AND i.state='accepted'
        JOIN objects s ON s.digest=i.snapshot JOIN heads w ON w.kind='work' AND w.id=i.work
        WHERE h.kind='delivery' AND (?1 IS NULL OR i.work=?1)
        AND i.work=json_extract(CAST(b.bytes AS TEXT),'$.work') AND i.actor=json_extract(CAST(b.bytes AS TEXT),'$.owner')
        AND i.id=json_extract(CAST(s.bytes AS TEXT),'$.invocation')")?;
    let pairs = query.query_map([work], |r| {
        let version: String = r.get(5)?;
        let current = format!("work:{}@{}",r.get::<_,String>(2)?,r.get::<_,i64>(3)?);
        Ok((format!("delivery:{}@{}",r.get::<_,String>(0)?,r.get::<_,i64>(1)?),
            json!({"delivered_under":version,"current_brief":current,"same_revision":version==current,
                "original_input":format!("object:{}",r.get::<_,String>(4)?)})))
    })?.collect::<std::result::Result<_,_>>()?;
    Ok(pairs)
}

// Keep the latest disposition used in the activity view and the complete
// history used by the package inspector on the same immutable review records.
fn visit_delivery_reviews(db:&Connection,mut visit:impl FnMut(&str,Value))->Result<()> {
    let mut query=db.prepare("SELECT id,revision,digest,created FROM versions WHERE kind='acceptance' ORDER BY rowid")?;
    let mut rows=query.query([])?;
    while let Some(row)=rows.next()? {
        let id:String=row.get(0)?;
        let revision:i64=row.get(1)?;
        let review:Value=object(db,&row.get::<_,String>(2)?)?;
        let delivery=review["delivery"].as_str().ok_or_else(||failure("Review is missing its exact delivery"))?;
        visit(delivery,json!({"reference":format!("acceptance:{id}@{revision}"),"reviewer":review["user"],
            "accepted":review["accepted"],"reason":review["reason"],"created":row.get::<_,i64>(3)?,
            "brief_at_review":review["brief_at_review"]}));
    }
    Ok(())
}

pub fn delivery_reviews(db:&Connection)->Result<std::collections::BTreeMap<String,Value>> {
    let mut latest=std::collections::BTreeMap::new();
    visit_delivery_reviews(db,|delivery,review|{latest.insert(delivery.to_owned(),review);})?;
    Ok(latest)
}

pub(super) fn operator_review_history(db:&Connection,deliveries:&std::collections::BTreeSet<String>)->Result<std::collections::BTreeMap<String,Vec<Value>>>{
    let mut history=std::collections::BTreeMap::new();
    visit_delivery_reviews(db,|delivery,review|{
        if deliveries.contains(delivery){history.entry(delivery.to_owned()).or_insert_with(Vec::new).push(review);}
    })?;
    Ok(history)
}

pub fn put_bytes(db: &Connection, media: &str, bytes: &[u8]) -> Result<String> {
    let digest = crate::store::sha256_bytes(bytes);
    insert_bytes(db, &digest, media, bytes)?;
    Ok(digest)
}

pub(super) fn insert_bytes(db: &Connection, digest: &str, media: &str, bytes: &[u8]) -> Result<bool> {
    Ok(db.execute(
        "INSERT INTO objects(digest,media,bytes) VALUES(?1,?2,?3) ON CONFLICT(digest) DO NOTHING",
        params![digest, media, bytes],
    )? == 1)
}
pub fn put<T: Serialize>(db: &Connection, value: &T) -> Result<String> {
    put_bytes(db, "application/json", &serde_json::to_vec(value)?)
}
pub fn bytes(db: &Connection, digest: &str) -> Result<Vec<u8>> {
    let bytes: Vec<u8> =
        db.query_row("SELECT bytes FROM objects WHERE digest=?1", [digest], |r| {
            r.get(0)
        })?;
    if crate::store::sha256_bytes(&bytes) != digest {
        return Err(failure("retained object digest mismatch"));
    }
    Ok(bytes)
}
pub fn object<T: serde::de::DeserializeOwned>(db: &Connection, digest: &str) -> Result<T> {
    Ok(serde_json::from_slice(&bytes(db, digest)?)?)
}

/// Compare an invocation's retained objects to the record written when the
/// provider completed. Adoption and read-only comparisons share this binding.
pub(super) fn verify_completion(
    db: &Connection, call: &str, actor: &str, work: &str, snapshot: &str,
    response: &str, receipt: &str, disposition: &str,
) -> Result<()> {
    let terminal = required(db, "provider_completion", call)?.body;
    if terminal["actor"] != actor || terminal["work"] != work || terminal["invocation"] != call
        || terminal["response"] != format!("object:{response}") || terminal["snapshot"] != format!("object:{snapshot}")
        || terminal["receipt"] != format!("object:{receipt}") || terminal["disposition"] != disposition {
        return Err(failure("The retained decision, input or receipt differs from the observed provider completion"));
    }
    Ok(())
}
pub fn head(db: &Connection, kind: &str, id: &str) -> Result<Option<Record>> {
    let row: Option<(i64, String)> = db
        .query_row(
            "SELECT revision,digest FROM heads WHERE kind=?1 AND id=?2",
            params![kind, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(revision, digest)| {
        Ok(Record {
            body: object(db, &digest)?,
            reference: Reference {
                kind: kind.into(),
                id: id.into(),
                revision,
                digest,
            },
        })
    })
    .transpose()
}
pub fn required(db: &Connection, kind: &str, id: &str) -> Result<Record> {
    head(db, kind, id)?.ok_or_else(|| failure(format!("missing {kind}:{id}")))
}
pub(super) fn open_activity(db: &Connection, work: &str) -> Result<Record> {
    let activity = required(db, "work", work)?;
    if activity.body["phase"] == "dissolved" {
        return Err(failure("This activity has ended. Reopen it before starting new work"));
    }
    Ok(activity)
}
pub fn exact(db: &Connection, handle: &str) -> Result<Record> {
    let (kind, rest) = handle
        .split_once(':')
        .ok_or_else(|| failure("reference requires kind:id@revision"))?;
    let (id, revision) = rest
        .rsplit_once('@')
        .ok_or_else(|| failure("reference requires an exact revision"))?;
    let revision: i64 = revision.parse()?;
    let digest: String = db.query_row(
        "SELECT digest FROM versions WHERE kind=?1 AND id=?2 AND revision=?3",
        params![kind, id, revision],
        |r| r.get(0),
    ).optional()?.ok_or_else(||failure(format!("No retained version matches exact reference {handle}; copy its observed handle or search its path/cues")))?;
    Ok(Record {
        body: object(db, &digest)?,
        reference: Reference {
            kind: kind.into(),
            id: id.into(),
            revision,
            digest,
        },
    })
}
pub(super) fn written_by(db: &Connection, record: &Record, call: &str) -> Result<bool> {
    let r = &record.reference;
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM versions WHERE kind=?1 AND id=?2 AND revision=?3 AND digest=?4 AND invocation=?5)",
        params![r.kind,r.id,r.revision,r.digest,call],|row| row.get(0))?)
}

pub fn list(db: &Connection, kind: &str) -> Result<Vec<Record>> {
    let mut statement =
        db.prepare("SELECT id,revision,digest FROM heads WHERE kind=?1 ORDER BY id")?;
    let rows = statement
        .query_map([kind], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, revision, digest)| {
            Ok(Record {
                body: object(db, &digest)?,
                reference: Reference {
                    kind: kind.into(),
                    id,
                    revision,
                    digest,
                },
            })
        })
        .collect()
}

/// Select current records by their recorded activity, keeping the immutable body verification.
pub fn in_work(db: &Connection, kind: &str, work: &str) -> Result<Vec<Record>> {
    let mut query=db.prepare("SELECT h.id,h.revision,h.digest FROM heads h JOIN objects o ON h.digest=o.digest
        WHERE h.kind=?1 AND json_extract(CAST(o.bytes AS TEXT),'$.work')=?2 ORDER BY h.id")?;
    let rows=query.query_map(params![kind,work],|row|Ok(Reference{kind:kind.into(),id:row.get(0)?,revision:row.get(1)?,digest:row.get(2)?}))?;
    rows.map(|row|{let reference=row?;Ok(Record{body:object(db,&reference.digest)?,reference})}).collect()
}

/// Call only inside the decision transaction. Expected zero means creation.
pub fn write<T: Serialize>(
    db: &Connection,
    kind: &str,
    id: &str,
    expected: i64,
    value: &T,
    invocation: Option<&str>,
) -> Result<Reference> {
    let current = head(db, kind, id)?;
    if current.as_ref().map_or(0, |r| r.reference.revision) != expected {
        return Err(failure(format!(
            "stale {kind}:{id}; expected revision {expected}"
        )));
    }
    let revision = expected
        .checked_add(1)
        .ok_or_else(|| failure("revision overflow"))?;
    let digest = put(db, value)?;
    db.execute("INSERT INTO heads VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET revision=excluded.revision,digest=excluded.digest",params![kind,id,revision,digest])?;
    db.execute(
        "INSERT INTO versions VALUES(?1,?2,?3,?4,?5,?6)",
        params![kind, id, revision, digest, invocation, now()],
    )?;
    Ok(Reference {
        kind: kind.into(),
        id: id.into(),
        revision,
        digest,
    })
}

pub fn event<T: Serialize>(
    db: &Connection,
    work: &str,
    actor: &str,
    target: &str,
    kind: &str,
    value: &T,
    invocation: Option<&str>,
) -> Result<i64> {
    let digest = put(db, value)?;
    db.execute("INSERT INTO events(work,actor,target,kind,digest,invocation,created) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![work,actor,target,kind,digest,invocation,now()])?;
    Ok(db.last_insert_rowid())
}
pub fn events(db: &Connection, work: &str, actor: &str, after: i64) -> Result<Vec<Value>> {
    let mut statement=db.prepare("SELECT seq,actor,target,kind,digest,invocation,created FROM events e WHERE work=?1 AND seq>?2 AND
        (target=?3 OR (target='*' AND EXISTS(SELECT 1 FROM events j WHERE j.work=e.work AND j.target=?3 AND j.kind='invitation' AND j.seq<=e.seq))) ORDER BY seq")?;
    let rows = statement
        .query_map(params![work, after, actor], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter().map(|(seq,actor,target,kind,digest,invocation,created)| Ok(json!({"event":seq,"experience":format!("experience:event-{seq}@1"),"actor":actor,"target":target,"kind":kind,"body":object::<Value>(db,&digest)?,"digest":digest,"invocation":invocation,"created":created}))).collect()
}

pub fn has_arrival(db:&Connection, work:&str, actor:&str, after:i64, kinds:&[String])->Result<bool> {
    let specific = kinds.iter().any(|k| k.starts_with("human_request:"));
    let mut query=db.prepare("SELECT e.kind, e.invocation IS NOT NULL AND e.target=?3 AND e.actor<>?3,
        CASE WHEN ?4 AND e.invocation IS NULL AND e.target=?3 AND e.kind IN ('user_message','human_request_progress')
        THEN (SELECT json_extract(CAST(bytes AS TEXT),'$.human_request') FROM objects WHERE digest=e.digest) END
        FROM events e WHERE work=?1 AND seq>?2 AND
        (target=?3 OR (target='*' AND EXISTS(SELECT 1 FROM events j WHERE j.work=e.work AND j.target=?3 AND j.kind='invitation' AND j.seq<=e.seq)))")?;
    let mut rows=query.query(params![work,after,actor,specific])?;
    while let Some(row)=rows.next()? {
        let kind: String = row.get(0)?;
        let report: Option<String> = row.get(2)?;
        if report.is_some_and(|handle| kinds.iter().any(|k| super::human_requests::newer_report(k, &handle))) {
            return Ok(true);
        }
        // Recording one's own next-context selection introduces no new observation.
        let status = matches!(kind.as_str(),"request_joined"|"request_declined"|"request_left");
        let returned = matches!(kind.as_str(), "request_reply" | "request_delivery" | "request_declined" | "request_left");
        let incoming_peer_message = matches!(kind.as_str(), "message" | "request_reply" | "request_delivery" | "requester_message" | "requester_review" | "request_declined" | "request_left") && row.get::<_,bool>(1)?;
        if (kinds.is_empty() && kind != "appraisal_selected") || kinds.iter().any(|requested|
            requested == &kind || (requested == "peer_message" && incoming_peer_message)
            || (requested == "result_review" && kind == "package_review")
            || (matches!(requested.as_str(), "delivery" | "result_delivery") && matches!(kind.as_str(), "delivery" | "request_delivery"))
            || (requested == "request_status" && status) || (requested == "request_return" && returned)) {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn stopped(db: &Connection, work: &str, actor: &str) -> Result<Option<Value>> {
    let ended: Option<(i64, String)> = db.query_row(
        "SELECT h.revision,json_extract(CAST(o.bytes AS TEXT),'$.last_dissolution')
         FROM heads h JOIN objects o ON o.digest=h.digest WHERE h.kind='work' AND h.id=?1
         AND json_extract(CAST(o.bytes AS TEXT),'$.phase')='dissolved'",
        [work], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    if let Some((revision, dissolution)) = ended {
        return Ok(Some(json!({"scope":format!("work:{work}"),"state":"dissolved","revision":revision,"dissolution":dissolution})));
    }
    for scope in [
        "global".to_string(),
        format!("work:{work}"),
        format!("persona:{actor}"),
    ] {
        let row: Option<(String, i64, String)> = db
            .query_row(
                "SELECT state,revision,reason FROM controls WHERE scope=?1",
                [&scope],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((state, revision, reason)) = row {
            if state != "running" {
                return Ok(Some(
                    json!({"scope":scope,"state":state,"revision":revision,"reason":reason}),
                ));
            }
        }
    }
    Ok(None)
}

pub fn control_versions(db: &Connection, work: &str, actor: &str) -> Result<Value> {
    let mut revisions = serde_json::Map::new();
    for scope in [
        "global".to_string(),
        format!("work:{work}"),
        format!("persona:{actor}"),
    ] {
        let revision: Option<i64> = db
            .query_row(
                "SELECT revision FROM controls WHERE scope=?1",
                [&scope],
                |r| r.get(0),
            )
            .optional()?;
        revisions.insert(scope, json!(revision.unwrap_or(0)));
    }
    Ok(Value::Object(revisions))
}


/// Historical job records remain available when inspecting an existing workspace.
pub fn job_index(db: &Connection, unfinished_only: bool) -> Result<Vec<Value>> {
    Ok(list(db, "job")?.into_iter().filter(|record| {
        !unfinished_only || matches!(record.body["state"].as_str(), Some("running" | "submitted" | "uncertain"))
    }).map(|record| {
        let mut body = record.body;
        body["reference"] = json!(record.reference.handle());
        body
    }).collect())
}
