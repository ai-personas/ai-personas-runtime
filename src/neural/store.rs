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
    pub fn create(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        drop(options.open(root.join("neural.sqlite"))?);
        let db = Connection::open(root.join("neural.sqlite"))?;
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
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        Ok(Self {
            root: root.canonicalize()?,
            db,
        })
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

    pub fn control(&mut self, scope: &str, state: &str, reason: &str) -> Result<()> {
        if !["running", "paused", "canceled"].contains(&state) {
            return Err(failure("invalid control state"));
        }
        self.transaction(|tx| {
            tx.execute("INSERT INTO controls VALUES(?1,?2,1,?3,?4) ON CONFLICT(scope) DO UPDATE SET state=excluded.state,revision=controls.revision+1,reason=excluded.reason,updated=excluded.updated",
                params![scope,state,reason,now()])?;
            Ok(())
        })
    }

    pub fn status(&mut self) -> Result<Value> {
        self.snapshot(|db| {
            let mut invocations = db.prepare("SELECT state,COUNT(*) FROM invocations GROUP BY state")?;
            let counts = invocations.query_map([], |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?)))?
                .collect::<std::result::Result<std::collections::BTreeMap<_,_>,_>>()?;
            let mut effects = db.prepare("SELECT state,COUNT(*) FROM outbox GROUP BY state")?;
            let effect_counts = effects.query_map([], |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?)))?
                .collect::<std::result::Result<std::collections::BTreeMap<_,_>,_>>()?;
            let mut controls=db.prepare("SELECT scope,state,reason,revision FROM controls ORDER BY scope")?;
            let controls=controls.query_map([],|r|Ok(json!({"scope":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"reason":r.get::<_,String>(2)?,"revision":r.get::<_,i64>(3)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
            Ok(json!({"format":"neural-runtime-v2","personas":list(db,"persona")?,
                "work":list(db,"work")?,"participation":list(db,"participation")?,
                "outcomes":list(db,"outcome")?,"deliveries":list(db,"delivery")?,
                "invocations":counts,"operations":effect_counts,"controls":controls,
                "meaning":"Stored activity, authored judgments and retained deliveries; not an independent quality verdict."}))
        })
    }
}

pub fn put_bytes(db: &Connection, media: &str, bytes: &[u8]) -> Result<String> {
    let digest = crate::store::sha256_bytes(bytes);
    db.execute(
        "INSERT OR IGNORE INTO objects VALUES(?1,?2,?3)",
        params![digest, media, bytes],
    )?;
    Ok(digest)
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
    )?;
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
    let mut statement=db.prepare("SELECT seq,actor,target,kind,digest,invocation,created FROM events WHERE work=?1 AND seq>?2 AND (target=?3 OR target='*') ORDER BY seq")?;
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
    rows.into_iter().map(|(seq,actor,target,kind,digest,invocation,created)| Ok(json!({"event":seq,"actor":actor,"target":target,"kind":kind,"body":object::<Value>(db,&digest)?,"digest":digest,"invocation":invocation,"created":created}))).collect()
}
pub fn stopped(db: &Connection, work: &str, actor: &str) -> Result<Option<Value>> {
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
