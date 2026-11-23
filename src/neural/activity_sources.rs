//! Exact retained resources from another activity, as observed on accepted choices
//! and their completed effects. A file's content author survives metadata changes.
pub(in crate::neural) mod delivery;
mod effects;
pub(super) mod resources;
pub use delivery::delivery_files;
pub use super::experience::prior_checks::{prior_checks, PriorChecks};

use super::{context::Snapshot, failure, provider_evidence, store, Result};
use clap::Args;
use resources::{Asset, Catalog};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Filter by the activity in which the file's content was saved.
    #[arg(long)]
    #[serde(default)]
    pub source_work: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    20
}

impl Default for Query {
    fn default() -> Self {
        Self {
            source_work: None,
            offset: 0,
            page_size: page_size(),
        }
    }
}

pub fn inspect(db: &Connection, work: &str, persona: Option<&str>, query: &Query) -> Result<Value> {
    report(db, work, persona, None, query)
}

pub(super) fn own(db: &Connection, work: &str, actor: &str, query: &Query) -> Result<Value> {
    report(db, work, Some(actor), Some(actor), query)
}

fn decision(call: &str) -> String {
    format!("experience:{call}@1")
}

struct Moment<'a> {
    snapshot: &'a Snapshot,
    created: i64,
    event: Option<i64>,
    evidence: &'a str,
}

impl Moment<'_> {
    fn order(&self) -> (i64, i64, bool) {
        (
            self.created,
            self.event
                .or(self.snapshot.observation_through)
                .unwrap_or(self.snapshot.cursor),
            self.event.is_none(),
        )
    }
}

// Input preparation and effect completion have separate clocks. Drain a second's
// events once all the decisions prepared in that second have been inspected.
struct Page<'a> {
    query: &'a Query,
    reader: Option<&'a str>,
    pending: BTreeMap<(i64, i64, bool), Vec<Value>>,
    items: Vec<Value>,
    kinds: BTreeMap<&'static str, usize>,
    files: BTreeSet<String>,
    works: BTreeSet<String>,
    total: usize,
    ordinary: usize,
    verified: usize,
    legacy: usize,
    unverified_receipts: usize,
}

impl<'a> Page<'a> {
    fn new(reader: Option<&'a str>, query: &'a Query) -> Self {
        Self {
            query,
            reader,
            pending: BTreeMap::new(),
            items: Vec::new(),
            files: BTreeSet::new(),
            works: BTreeSet::new(),
            total: 0,
            ordinary: 0,
            verified: 0,
            legacy: 0,
            unverified_receipts: 0,
            kinds: [
                "catalog",
                "text",
                "image",
                "execution",
                "delivery",
                "human_request",
            ]
            .into_iter()
            .map(|kind| (kind, 0))
            .collect(),
        }
    }

    fn add(
        &mut self,
        time: &Moment<'_>,
        asset: &Asset,
        kind: &'static str,
        status: &str,
        role: &str,
        tool: &str,
        range: Option<[usize; 2]>,
        detail: Value,
    ) {
        if asset.work == time.snapshot.work
            || self
                .query
                .source_work
                .as_ref()
                .is_some_and(|w| w != &asset.work)
            || asset.created > time.created
            || asset
                .written_event
                .zip(time.event.or(time.snapshot.observation_through))
                .is_some_and(|(written, through)| written > through)
        {
            return;
        }
        *self.kinds.entry(kind).or_default() += 1;
        self.files.insert(asset.reference.clone());
        self.works.insert(asset.work.clone());
        let source_call = self
            .reader
            .is_none_or(|reader| reader == asset.author)
            .then(|| decision(&asset.call));
        let row = json!({"kind":kind,"status":status,"role":role,"tool":tool,
            "reference":asset.reference,"path":asset.path,"author":asset.author,"source_work":asset.work,
            "source_revision":asset.content_revision,"source_decision":source_call,
            "actor":time.snapshot.actor,"decision":decision(&time.snapshot.invocation),
            "event":time.event,"created_unix":time.created,"evidence":time.evidence,"range":range,"detail":detail});
        self.pending.entry(time.order()).or_default().push(row);
    }

    fn drain(&mut self, before: Option<i64>) {
        while self
            .pending
            .first_key_value()
            .is_some_and(|(k, _)| before.is_none_or(|t| k.0 < t))
        {
            for item in self.pending.pop_first().expect("nonempty timeline").1 {
                if self.total >= self.query.offset && self.items.len() < self.query.page_size {
                    self.items.push(item);
                }
                self.total += 1;
            }
        }
    }
}

fn report(
    db: &Connection,
    work: &str,
    persona: Option<&str>,
    reader: Option<&str>,
    query: &Query,
) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Use a positive page size"));
    }
    store::required(db, "work", work)?;
    if let Some(actor) = persona {
        store::required(db, "persona", actor)?;
    }
    let mut catalog = Catalog::new(db, work, query.source_work.as_deref());
    let mut page = Page::new(reader, query);
    let mut receipts = effects::receipts(db, work, persona)?;
    let mut choices = db.prepare(
        "SELECT i.id,i.actor,i.snapshot,i.created FROM invocations i JOIN objects s ON s.digest=i.snapshot
         WHERE i.work=?1 AND i.state='accepted' AND (?2 IS NULL OR i.actor=?2)
         AND json_type(CAST(s.bytes AS TEXT),'$.model')='text' ORDER BY i.created,i.rowid")?;
    let mut rows = choices.query(params![work, persona])?;
    while let Some(row) = rows.next()? {
        let call: String = row.get(0)?;
        let actor: String = row.get(1)?;
        let snapshot_id: String = row.get(2)?;
        let created: i64 = row.get(3)?;
        page.drain(Some(created));
        page.ordinary += 1;
        let Ok(accepted) = provider_evidence::accepted(db, &actor, &call, true) else {
            continue;
        };
        page.verified += 1;
        page.legacy += usize::from(accepted.binding == "accepted_record");
        let supplied = if reader.is_some() {
            decision(&call)
        } else {
            format!("object:{snapshot_id}")
        };
        let moment = Moment {
            snapshot: &accepted.snapshot,
            created,
            event: None,
            evidence: &supplied,
        };
        resources::supplied(&mut catalog, &moment, &mut page)?;
        if let Some(events) = receipts.remove(&call) {
            effects::observe(&mut catalog, &accepted, &events, &mut page)?;
        }
    }
    page.drain(None);
    let next = query.offset.saturating_add(page.items.len());
    Ok(
        json!({"work":work,"persona":persona,"source_work":query.source_work,
        "offset":query.offset,"page_size":query.page_size,"total":page.total,
        "next_offset":(next < page.total).then_some(next),"items":page.items,
        "coverage":{"ordinary_decisions":page.ordinary,"verified_decisions":page.verified,
            "unverified_decisions":page.ordinary-page.verified,"legacy_decisions":page.legacy,
            "unverified_receipts":page.unverified_receipts,"unattributed_files":catalog.unattributed,
            "by_kind":page.kinds,"distinct_files":page.files.len(),"source_activities":page.works.len()}}),
    )
}
