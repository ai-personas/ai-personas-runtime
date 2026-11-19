//! A page image can be identical for distinct PDFs. Keep the producing effect
//! and its event attached to the image when checking an accepted provider input.
use super::{context::Snapshot, provider_evidence, store, Event, Result};
use crate::neural::{operations, pdf_evidence, protocol::Operation};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

struct Render {
    event: Event,
    producer_order: i64,
    result: Value,
    page: pdf_evidence::Page,
    supplied: Option<(i64, Value)>,
}

#[derive(Default)]
pub(super) struct Pages {
    renders: BTreeMap<i64, Render>,
}

impl Pages {
    pub(super) fn find(db: &Connection, message: &Event, pdfs: &BTreeSet<String>) -> Result<Self> {
        let mut pages = Self::default();
        if pdfs.is_empty() {
            return Ok(pages);
        }
        let mut stmt = db.prepare(
            "SELECT e.seq, b.invocation, i.rowid, b.ordinal, b.operation, b.result
             FROM events e JOIN objects notice ON e.digest=notice.digest
             JOIN outbox b ON b.id=json_extract(CAST(notice.bytes AS TEXT),'$.effect')
             JOIN objects result ON result.digest=b.result
             JOIN invocations i ON i.id=b.invocation AND i.work=b.work AND i.actor=b.actor
             WHERE e.work=?1 AND e.actor=?2 AND e.target=e.actor AND e.seq>?3
               AND e.kind='tool_result' AND e.invocation=b.invocation
               AND b.work=e.work AND b.actor=e.actor AND b.state='completed' AND i.state='accepted'
               AND json_extract(CAST(notice.bytes AS TEXT),'$.tool')='document.render_pdf'
               AND json_extract(CAST(notice.bytes AS TEXT),'$.result')='object:'||b.result
               AND json_extract(CAST(result.bytes AS TEXT),'$.source_bytes') IN (SELECT value FROM json_each(?4))
             GROUP BY b.id HAVING count(*)=1 ORDER BY i.rowid,e.seq",
        )?;
        let mut rows = stmt.query(params![
            message.work,
            message.target,
            message.seq,
            serde_json::to_string(pdfs)?
        ])?;
        let mut last = String::new();
        let mut producer = None;
        while let Some(row) = rows.next()? {
            let call: String = row.get(1)?;
            if call != last {
                producer = provider_evidence::accepted(db, &message.target, &call, true).ok();
                last = call;
            }
            let Some(accepted) = &producer else { continue };
            let seq: i64 = row.get(0)?;
            let Some(event) = Event::get(db, seq)? else {
                continue;
            };
            // A decision already in flight when the file arrived cannot attest
            // to a render requested from this incoming file.
            if accepted.snapshot.model.is_none()
                || !accepted.snapshot.data["context_recovery"].is_null()
                || !accepted
                    .snapshot
                    .observation_through
                    .is_some_and(|n| message.seq <= n && n < seq)
                || event.body["context"] != "complete"
            {
                continue;
            }
            let ordinal: i64 = row.get(3)?;
            let op: Value = store::object(db, &row.get::<_, String>(4)?)?;
            let result: Value = store::object(db, &row.get::<_, String>(5)?)?;
            let Some(ops) = accepted.response["operations"].as_array() else {
                continue;
            };
            let Ok(ordinal) = usize::try_from(ordinal) else {
                continue;
            };
            if !matches_operation(db, &event, ops, ordinal, &op)? {
                continue;
            }
            let Ok(page) = pdf_evidence::verify(db, &op["input"], &result) else {
                continue;
            };
            if pdfs.contains(&page.source_bytes) {
                pages.renders.insert(
                    seq,
                    Render {
                        event,
                        producer_order: row.get(2)?,
                        result,
                        page,
                        supplied: None,
                    },
                );
            }
        }
        Ok(pages)
    }

    pub(super) fn image_references(&self) -> BTreeSet<String> {
        self.renders
            .values()
            .map(|r| r.page.image.clone())
            .collect()
    }

    pub(super) fn after_message_receipt(&mut self, first_received: i64) {
        self.renders
            .retain(|_, render| render.producer_order >= first_received);
    }

    pub(super) fn all_supplied(&self) -> bool {
        self.renders.values().all(|r| r.supplied.is_some())
    }

    pub(super) fn observe(&mut self, snapshot: &Snapshot, order: i64, proof: &Value) {
        if self.renders.is_empty() || snapshot.image_blobs.is_empty() {
            return;
        }
        let images: BTreeSet<_> = snapshot
            .image_blobs
            .iter()
            .filter_map(|(digest, mime)| (mime == "image/png").then_some(digest.as_str()))
            .collect();
        for incoming in snapshot.data["new_events"].as_array().into_iter().flatten() {
            let Some(render) = incoming["event"]
                .as_i64()
                .and_then(|seq| self.renders.get_mut(&seq))
            else {
                continue;
            };
            if render.event.incoming_matches(incoming)
                && incoming["supplied_result"] == render.result
            {
                render.observe(snapshot, order, &images, proof, "render_result");
            }
        }
        let selected = snapshot.data["selected_resources"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for resource in selected {
            let Some(seq) = resource["reference"]
                .as_str()
                .and_then(|r| r.strip_prefix("experience:event-"))
                .and_then(|r| r.strip_suffix("@1"))
                .and_then(|r| r.parse::<i64>().ok())
            else {
                continue;
            };
            let Some(render) = self.renders.get_mut(&seq) else {
                continue;
            };
            if render.event.experience_matches(resource)
                && selected.iter().any(|r| {
                    r["reference"] == render.page.image
                        && r["image_blob"] == render.result["image_blob"]
                        && r["mime"] == "image/png"
                        && r["bytes"] == render.result["bytes"]
                        && r["range"] == json!([0, render.result["bytes"]])
                })
            {
                render.observe(snapshot, order, &images, proof, "selected_render_and_image");
            }
        }
    }

    pub(super) fn for_file(&self, source: &str, message_received: bool) -> Vec<Value> {
        let mut pages = BTreeMap::<u32, &Render>::new();
        if message_received {
            for render in self
                .renders
                .values()
                .filter(|r| r.page.source_bytes == source)
            {
                let first = pages.entry(render.page.page).or_insert(render);
                if let Some((order, _)) = &render.supplied {
                    if first
                        .supplied
                        .as_ref()
                        .is_none_or(|(other, _)| order < other)
                    {
                        *first = render;
                    }
                }
            }
        }
        pages.values().map(|r| json!({
            "page":r.page.page,"page_count":r.page.page_count,
            "source":r.page.source,"source_bytes":r.page.source_bytes,"image":r.page.image,
            "width":r.page.width,"height":r.page.height,
            "render":{"event":r.event.reference(),"time":r.event.created,
                "decision":format!("experience:{}@1",r.event.invocation.as_deref().unwrap_or_default()),
                "effect":r.event.body["effect"],"result":r.event.body["result"]},
            "supplied":r.supplied.as_ref().map(|(_,p)|p)
        })).collect()
    }
}

impl Render {
    fn observe(
        &mut self,
        s: &Snapshot,
        order: i64,
        images: &BTreeSet<&str>,
        proof: &Value,
        via: &str,
    ) {
        if self.supplied.is_some()
            || order <= self.producer_order
            || !s.observation_through.is_some_and(|n| n >= self.event.seq)
            || self.result["image_blob"]
                .as_str()
                .is_none_or(|h| !images.contains(h))
        {
            return;
        }
        // accepted(..., true) verified the ordered provider image audit from the
        // retained bytes. The source-bound event separately identifies the page.
        let mut observation = proof.clone();
        observation["via"] = json!(via);
        self.supplied = Some((order, observation));
    }
}

fn matches_operation(
    db: &Connection,
    event: &Event,
    ops: &[Value],
    ordinal: usize,
    queued: &Value,
) -> Result<bool> {
    let Some(raw) = ops.get(ordinal) else {
        return Ok(false);
    };
    if raw["tool"] != "document.render_pdf" || raw["id"] != event.body["operation"] {
        return Ok(false);
    }
    let Ok(mut op) = serde_json::from_value::<Operation>(raw.clone()) else {
        return Ok(false);
    };
    let mut aliases = BTreeMap::new();
    if let Some(alias) = op.input["reference"]
        .as_str()
        .and_then(|r| r.strip_prefix('$'))
    {
        let matches: Vec<_> = ops[..ordinal].iter().filter(|o| o["id"] == alias).collect();
        let [prior] = matches.as_slice() else {
            return Ok(false);
        };
        let Some(tool) = prior["tool"]
            .as_str()
            .filter(|t| operations::returns_alias(t))
        else {
            return Ok(false);
        };
        let mut stmt = db.prepare("SELECT e.digest FROM events e JOIN objects o ON o.digest=e.digest
            WHERE e.invocation=?1 AND e.work=?2 AND e.actor=?3 AND e.target=e.actor AND e.kind='tool_result'
              AND e.seq<?4 AND json_extract(CAST(o.bytes AS TEXT),'$.operation')=?5 AND json_extract(CAST(o.bytes AS TEXT),'$.tool')=?6
              AND json_type(CAST(o.bytes AS TEXT),'$.effect') IS NULL")?;
        let mut rows = stmt.query(params![
            event.invocation,
            event.work,
            event.actor,
            event.seq,
            alias,
            tool
        ])?;
        let Some(row) = rows.next()? else {
            return Ok(false);
        };
        let body: Value = store::object(db, &row.get::<_, String>(0)?)?;
        if rows.next()?.is_some() {
            return Ok(false);
        }
        let Some(id) = body["result"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
        else {
            return Ok(false);
        };
        let result: Value = store::object(db, id)?;
        let Some(reference) = result["reference"].as_str() else {
            return Ok(false);
        };
        aliases.insert(alias.to_owned(), reference.to_owned());
    }
    Ok(operations::resolve_operation(&mut op, &aliases).is_ok()
        && serde_json::to_value(op)? == *queued)
}
