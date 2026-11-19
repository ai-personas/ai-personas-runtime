//! Scope a run to a founding activity and the further needs its participants
//! actually originated. Operator-created activities have no founding decision.
use super::{
    activity_requests::{self, Origin},
    failure, store, Result,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Tree {
    root: String,
    origins: BTreeMap<String, Origin>,
}

impl Tree {
    pub(super) fn new(root: &str) -> Self {
        Self {
            root: root.into(),
            origins: BTreeMap::new(),
        }
    }

    pub(super) fn contains(&self, work: &str) -> bool {
        self.root == work || self.origins.contains_key(work)
    }

    /// Each new edge is authenticated once per run, even when the scheduler
    /// repeatedly polls for a time-based wake. Revisions and receipts are immutable.
    pub(super) fn refresh(&mut self, db: &Connection) -> Result<Vec<String>> {
        let mut query = db.prepare(
            "WITH RECURSIVE scope(id) AS (
                SELECT ?1 UNION
                SELECT v.id FROM scope s JOIN invocations i ON i.work=s.id AND i.state='accepted'
                JOIN versions v ON v.invocation=i.id AND v.kind='work' AND v.revision=1
             ) SELECT id FROM scope WHERE id!=?1 ORDER BY id",
        )?;
        let mut rows = query.query([&self.root])?;
        let mut added = Vec::new();
        while let Some(row) = rows.next()? {
            let child: String = row.get(0)?;
            if self.origins.contains_key(&child) {
                continue;
            }
            let origin = activity_requests::origin(db, &child)?
                .ok_or_else(|| failure("A requested activity lacks its founding decision"))?;
            self.origins.insert(child.clone(), origin);
            added.push(child);
        }
        for child in &added {
            if !self.contains(&self.origins[child].parent) {
                return Err(failure("The requesting activity is outside this run"));
            }
        }
        Ok(added)
    }
}

pub fn describe(db: &Connection, root: &str) -> Result<Value> {
    store::required(db, "work", root)?;
    let mut tree = Tree::new(root);
    tree.refresh(db)?;
    let mut participants = BTreeMap::<String, Vec<Value>>::new();
    for record in store::list(db, "participation")? {
        let work = record.body["work"]
            .as_str()
            .ok_or_else(|| failure("Missing participation activity"))?;
        if tree.contains(work) {
            participants
                .entry(work.into())
                .or_default()
                .push(record.body);
        }
    }
    let mut rows = Vec::new();
    let mut stack = vec![(root, 0)];
    let mut visited = BTreeSet::new();
    let mut children = BTreeMap::<&str, Vec<&str>>::new();
    for edge in tree.origins.values() {
        children.entry(&edge.parent).or_default().push(&edge.child);
    }
    while let Some((id, depth)) = stack.pop() {
        if !visited.insert(id) {
            return Err(failure("Cycle in activity requests"));
        }
        let work = store::required(db, "work", id)?;
        rows.push(
            json!({"work":id,"brief":work.reference.handle(),"need":work.body["original_need"],
            "phase":work.body["phase"],"depth":depth,"origin":tree.origins.get(id),
            "participants":participants.remove(id).unwrap_or_default()}),
        );
        if let Some(next) = children.get(id) {
            stack.extend(next.iter().rev().map(|&child| (child, depth + 1)));
        }
    }
    Ok(json!({"root":root,"activities":rows}))
}
