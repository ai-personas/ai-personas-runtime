use super::{Moment, Page};
use crate::neural::{context, store, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

pub(super) struct Asset {
    pub reference: String,
    pub path: String,
    pub blob: String,
    pub author: String,
    pub work: String,
    pub call: String,
    pub content_revision: String,
    pub created: i64,
    pub written_event: Option<i64>,
}

pub(super) struct Catalog<'a> {
    pub db: &'a Connection,
    work: &'a str,
    filter: Option<&'a str>,
    cache: HashMap<String, Option<Rc<Asset>>>,
    recent_bytes: Option<(String, Rc<Vec<u8>>)>,
    pub unattributed: usize,
}

impl<'a> Catalog<'a> {
    pub fn new(db: &'a Connection, work: &'a str, filter: Option<&'a str>) -> Self {
        Self {
            db,
            work,
            filter,
            cache: HashMap::new(),
            recent_bytes: None,
            unattributed: 0,
        }
    }

    pub fn source(&mut self, handle: &str) -> Result<Option<Rc<Asset>>> {
        let Some((id, revision)) = handle
            .strip_prefix("asset:")
            .and_then(|r| r.rsplit_once('@'))
        else {
            return Ok(None);
        };
        let Some(revision) = revision
            .parse::<i64>()
            .ok()
            .filter(|r| *r > 0 && format!("asset:{id}@{r}") == handle)
        else {
            return Ok(None);
        };
        if let Some(asset) = self.cache.get(handle) {
            return Ok(asset.clone());
        }
        let source = origin(self.db, id, revision)?;
        self.unattributed += usize::from(source.is_none());
        let source = source
            .filter(|asset| asset.work != self.work && self.filter.is_none_or(|w| w == asset.work))
            .map(Rc::new);
        self.cache.insert(handle.into(), source.clone());
        Ok(source)
    }

    pub fn bytes(&mut self, blob: &str) -> Result<Rc<Vec<u8>>> {
        if let Some((digest, bytes)) = &self.recent_bytes {
            if digest == blob {
                return Ok(bytes.clone());
            }
        }
        let bytes = Rc::new(store::bytes(self.db, blob)?);
        self.recent_bytes = Some((blob.into(), bytes.clone()));
        Ok(bytes)
    }
}

// The version's writer is the authority for the write. asset.organize preserves
// the previous revision's bytes and writer. Even an identical new save has its
// own content provenance. Historical versions need no author/work JSON fields.
fn origin(db: &Connection, id: &str, revision: i64) -> Result<Option<Asset>> {
    let reference = format!("asset:{id}@{revision}");
    let mut path = None;
    let mut blob = None;
    let mut created = 0;
    let mut written_event = None;
    for rev in (1..=revision).rev() {
        let version: Option<(String, String, String, String, i64)> = db
            .query_row(
                "SELECT v.digest,i.id,i.work,i.actor,v.created FROM versions v
             JOIN invocations i ON i.id=v.invocation AND i.state='accepted'
             WHERE v.kind='asset' AND v.id=?1 AND v.revision=?2",
                params![id, rev],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((digest, call, work, author, at)) = version else {
            return Ok(None);
        };
        let body: Value = store::object(db, &digest)?;
        let (Some(content), Some(filename)) = (body["blob"].as_str(), body["path"].as_str()) else {
            return Ok(None);
        };
        if rev == revision {
            path = Some(filename.to_owned());
            blob = Some(content.to_owned());
            created = at;
        }
        if blob.as_deref() != Some(content) {
            return Ok(None);
        }
        let written = format!("asset:{id}@{rev}");
        let mut receipts = db.prepare_cached(
            "SELECT e.digest,e.seq FROM events e JOIN objects b ON b.digest=e.digest
             WHERE e.work=?1 AND e.target=?2 AND e.actor=?2 AND e.invocation=?3 AND e.kind='tool_result'
             AND json_extract(CAST(b.bytes AS TEXT),'$.tool') IN ('asset.save','asset.register','asset.import','asset.organize')")?;
        let mut rows = receipts.query(params![work, author, call])?;
        let mut writer = None;
        while let Some(row) = rows.next()? {
            let event: Value = store::object(db, &row.get::<_, String>(0)?)?;
            let Some(result_id) = event["result"]
                .as_str()
                .and_then(|r| r.strip_prefix("object:"))
            else {
                continue;
            };
            let result: Value = store::object(db, result_id)?;
            if result["reference"] == written {
                if writer.is_some() {
                    return Ok(None);
                }
                writer = Some((event["tool"].clone(), result));
                // Import publishes the asset and its terminal outbox receipt in
                // separate transactions. An intervening input can see the file.
                if rev == revision && event["tool"] != "asset.import" {
                    written_event = Some(row.get(1)?);
                }
            }
        }
        let previous = format!("asset:{id}@{}", rev - 1);
        match writer {
            Some((tool, _)) if tool == "asset.organize" => {
                if rev == 1
                    || body
                        .get("previous_revision")
                        .is_some_and(|v| v != &previous)
                {
                    return Ok(None);
                }
                continue;
            }
            Some((_, receipt)) if receipt["blob"] != content => return Ok(None),
            // Some early records only bind the version to its originating call.
            // Creation and a change of bytes identify a content write. A later
            // same-byte version needs an explicit content or organization origin.
            None if rev > 1 && store::exact(db, &previous)?.body["blob"] == content => {
                if body["organized_by"] == author
                    && body["organized_in_work"] == work
                    && body["previous_revision"] == previous
                {
                    continue;
                }
                if body["origin_decision"] != call
                    || body["last_content_author"] != author
                    || body["work"] != work
                {
                    return Ok(None);
                }
            }
            _ => {}
        }
        if body["owner"] != author
            || body.get("origin_decision").is_some_and(|v| v != &call)
            || body
                .get("last_content_author")
                .is_some_and(|v| v != &author)
            || body.get("work").is_some_and(|v| v != &work)
        {
            return Ok(None);
        }
        return Ok(Some(Asset {
            reference,
            path: path.expect("exact filename"),
            blob: content.into(),
            author,
            work,
            call,
            content_revision: written,
            created,
            written_event,
        }));
    }
    Ok(None)
}

pub(super) fn supplied(
    catalog: &mut Catalog<'_>,
    time: &Moment<'_>,
    page: &mut Page<'_>,
) -> Result<()> {
    let snapshot = time.snapshot;
    if !snapshot.data["context_recovery"].is_null() {
        return Ok(());
    }
    let mut seen = BTreeSet::new();
    let mut read_bodies = BTreeSet::new();
    for event in snapshot.data["new_events"].as_array().into_iter().flatten() {
        if event["kind"] != "tool_result"
            || event["actor"] != snapshot.actor
            || event["target"] != snapshot.actor
        {
            continue;
        }
        let result = &event["supplied_result"];
        if matches!(
            event["body"]["tool"].as_str(),
            Some("asset.read" | "persona.read")
        ) {
            read_bodies.insert(std::ptr::from_ref(result));
        }
        if event["body"]["tool"] == "asset.search" && result["body_supplied"] == false {
            for row in result["items"].as_array().into_iter().flatten() {
                if let Some(reference) = row["reference"].as_str() {
                    if let Some(asset) = catalog.source(reference)? {
                        if row["path"] == asset.path && seen.insert(("catalog", reference, 0, 0)) {
                            page.add(
                                time,
                                &asset,
                                "catalog",
                                "metadata_supplied",
                                "catalog_row",
                                "asset.search",
                                None,
                                Value::Null,
                            );
                        }
                    }
                }
            }
        }
        if event["body"]["tool"] == "asset.find" {
            if let Some(reference) = result["reference"].as_str() {
                if let Some(asset) = catalog.source(reference)? {
                    if result["metadata"]["path"] == asset.path
                        && result["blob"] == format!("object:{}", asset.blob)
                    {
                        for excerpt in result["supplied_excerpts"].as_array().into_iter().flatten()
                        {
                            if excerpt["reference"] == reference {
                                read_bodies.insert(std::ptr::from_ref(excerpt));
                            }
                        }
                        if !seen.insert(("find", reference, 0, 0)) {
                            continue;
                        }
                        page.add(
                            time,
                            &asset,
                            "catalog",
                            "metadata_supplied",
                            "search_metadata",
                            "asset.find",
                            None,
                            Value::Null,
                        );
                    }
                }
            }
        }
    }
    for source in context::supplied_resources(&snapshot.data) {
        // The shared enumerator borrows these same frozen JSON values. Preserve
        // their event attribution even if another event echoes an identical body.
        if source.via != "selection" && !read_bodies.contains(&std::ptr::from_ref(source.value)) {
            continue;
        }
        let value = source.value;
        let Some(reference) = value["reference"].as_str() else {
            continue;
        };
        let Some(asset) = catalog.source(reference)? else {
            continue;
        };
        if value["metadata"]
            .get("blob")
            .is_some_and(|b| b != &asset.blob)
        {
            continue;
        }
        let (Some(start), Some(end), Some(size)) = (
            value["range"][0].as_u64(),
            value["range"][1].as_u64(),
            value["bytes"].as_u64(),
        ) else {
            continue;
        };
        if value["range"].as_array().is_none_or(|r| r.len() != 2) || start >= end || end > size {
            continue;
        }
        let bytes = catalog.bytes(&asset.blob)?;
        if size != bytes.len() as u64 {
            continue;
        }
        let range = [start as usize, end as usize];
        let (kind, role) = if let Some(text) = value["text"].as_str() {
            if bytes[range[0]..range[1]] != *text.as_bytes() {
                continue;
            }
            (
                "text",
                if start == 0 && end == size {
                    "full_text"
                } else {
                    "text_excerpt"
                },
            )
        } else if value["image_blob"] == asset.blob && start == 0 && end == size {
            let Some(mime) = context::image_media(&bytes) else {
                continue;
            };
            if value["mime"] != mime
                || !snapshot
                    .image_blobs
                    .iter()
                    .any(|(b, m)| b == &asset.blob && m == mime)
            {
                continue;
            }
            ("image", "visual_input")
        } else {
            continue;
        };
        if seen.insert((kind, reference, start, end)) {
            let tool = if source.via == "selection" {
                "next_context.references"
            } else {
                source.via
            };
            page.add(
                time,
                &asset,
                kind,
                "body_supplied",
                role,
                tool,
                Some(range),
                json!({"file_bytes":size}),
            );
        }
    }
    Ok(())
}
