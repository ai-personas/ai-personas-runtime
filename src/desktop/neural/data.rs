//! Read-only operator projection. Peer delivery visibility in the runtime is
//! unchanged: this local workspace inspector reads the owner's saved records.
use ai_personas::{
    neural::{self, store},
    persona_model::{Appraisal, Evaluation, FeatureContract, ModelPackage},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub(super) struct Character {
    pub model: ModelPackage,
    pub baseline: Evaluation,
}
#[derive(Default)]
pub(super) struct Snapshot {
    pub status: Value,
    pub events: Vec<Value>,
    pub cursor: i64,
    pub assets: Vec<Value>,
    pub characters: BTreeMap<String, Character>,
    pub evaluations: BTreeMap<String, Evaluation>,
    pub candidates: Vec<Value>,
    pub active: Vec<Value>,
    pub directory: PathBuf,
}
pub(super) struct Preview {
    pub directory: Arc<tempfile::TempDir>,
    pub path: PathBuf,
    pub digest: String,
    pub metadata: Value,
}
fn open(root: &Path) -> neural::Result<Connection> {
    let db =
        Connection::open_with_flags(root.join("neural.sqlite"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let format: String =
        db.query_row("SELECT value FROM metadata WHERE key='format'", [], |r| {
            r.get(0)
        })?;
    if format != "neural-runtime-v2" {
        return Err(neural::failure("Unsupported workspace format"));
    }
    db.execute_batch("BEGIN DEFERRED")?;
    Ok(db)
}
pub(super) fn snapshot(root: &Path, after: i64) -> neural::Result<Snapshot> {
    let db = open(root)?;
    let status = store::status(&db)?;
    let mut events = Vec::new();
    // Raw tool/provider payloads are intentionally loaded only through their
    // exact resource, never decoded or exposed as a recurring UI activity feed.
    let mut q = db.prepare("SELECT seq,work,actor,target,kind,digest,created FROM events WHERE seq>?1 AND kind!='tool_result' ORDER BY seq")?;
    let rows = q
        .query_map([after], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (seq, work, actor, target, kind, digest, created) in rows {
        events.push(json!({"event":seq,"work":work,"actor":actor,"target":target,"kind":kind,"body":store::object::<Value>(&db,&digest)?,"digest":digest,"created":created}));
    }
    let cursor = db.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
    let mut assets = Vec::new();
    let heads = store::list(&db, "asset")?;
    let current: std::collections::BTreeSet<_> =
        heads.iter().map(|a| a.reference.handle()).collect();
    let mut retained = heads;
    let mut included = current.clone();
    for delivery in status["deliveries"].as_array().into_iter().flatten() {
        for reference in delivery["body"]["artifacts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if included.insert(reference.to_owned()) {
                retained.push(store::exact(&db, reference)?);
            }
        }
    }
    for asset in retained {
        let blob = asset.body["blob"]
            .as_str()
            .ok_or_else(|| neural::failure("Artifact has no retained content"))?;
        let (size, media): (i64, String) = db.query_row(
            "SELECT length(bytes),media FROM objects WHERE digest=?1",
            [blob],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let created: i64 = db.query_row(
            "SELECT created FROM versions WHERE kind='asset' AND id=?1 AND revision=?2",
            rusqlite::params![asset.reference.id, asset.reference.revision],
            |r| r.get(0),
        )?;
        assets.push(json!({"reference":asset.reference.handle(),"body":asset.body,"size":size,"media":media,"created":created,"revision":asset.reference.revision,"current":current.contains(&asset.reference.handle())}));
    }
    assets.sort_by_key(|a| std::cmp::Reverse(a["created"].as_i64().unwrap_or(0)));
    let contract = FeatureContract::standard();
    let mut characters = BTreeMap::new();
    for p in status["personas"].as_array().into_iter().flatten() {
        if let Some(digest) = p["body"]["model"].as_str() {
            let model: ModelPackage = store::object(&db, digest)?;
            let baseline = model
                .evaluate(&contract, &Appraisal::default())
                .map_err(neural::failure)?;
            characters.insert(
                p["body"]["id"].as_str().unwrap_or("").into(),
                Character { model, baseline },
            );
        }
    }
    let mut evaluations = BTreeMap::new();
    for p in status["participation"].as_array().into_iter().flatten() {
        if let Some(reference) = p["body"]["last_evaluation"].as_str() {
            if let Some(digest) = reference.strip_prefix("object:") {
                evaluations.insert(reference.into(), store::object(&db, digest)?);
            }
        }
    }
    let candidates = store::list(&db, "candidate")?
        .into_iter()
        .map(|r| json!({"reference":r.reference.handle(),"body":r.body}))
        .collect();
    let mut q=db.prepare("SELECT id,actor,work,state,created FROM invocations WHERE state IN ('prepared','dispatched','responded') ORDER BY created")?;
    let active=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"actor":r.get::<_,String>(1)?,"work":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"created":r.get::<_,i64>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
    let directory = store::head(&db, "config", "runtime")?
        .and_then(|r| r.body["working_directory"].as_str().map(PathBuf::from))
        .unwrap_or_default();
    db.execute_batch("COMMIT")?;
    Ok(Snapshot {
        status,
        events,
        cursor,
        assets,
        characters,
        evaluations,
        candidates,
        active,
        directory,
    })
}
pub(super) fn preview(root: &Path, reference: &str) -> neural::Result<Preview> {
    let db = open(root)?;
    let (bytes, name, metadata) = if let Some(digest) = reference.strip_prefix("object:") {
        let bytes = store::bytes(&db, digest)?;
        let suffix = if serde_json::from_slice::<Value>(&bytes).is_ok() {
            "json"
        } else {
            "bin"
        };
        (
            bytes,
            format!(
                "object-{}.{}",
                digest.chars().take(12).collect::<String>(),
                suffix
            ),
            json!({"reference":reference}),
        )
    } else {
        let record = store::exact(&db, reference)?;
        if record.reference.kind == "asset" {
            let digest = record.body["blob"]
                .as_str()
                .ok_or_else(|| neural::failure("Artifact has no content"))?;
            let name = Path::new(record.body["path"].as_str().unwrap_or("artifact.bin"))
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            (store::bytes(&db, digest)?, name, record.body)
        } else {
            (
                serde_json::to_vec_pretty(&record.body)?,
                format!("{}-{}.json", record.reference.kind, record.reference.id),
                json!({"reference":reference}),
            )
        }
    };
    db.execute_batch("COMMIT")?;
    let digest = ai_personas::store::sha256_bytes(&bytes);
    let directory = Arc::new(
        tempfile::Builder::new()
            .prefix("personas-preview-")
            .tempdir()?,
    );
    let path = directory.path().join(if name.is_empty() {
        "artifact.bin"
    } else {
        &name
    });
    std::fs::write(&path, &bytes)?;
    Ok(Preview {
        directory,
        path,
        digest,
        metadata,
    })
}
