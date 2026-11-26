//! Persona-selected content inspection. A library file may be a draft. Unlike
//! the delivery desk, these operands make no assertion of issuance or review.
use super::inspection::registered;
use super::{failure, fingerprint, paths, Difference, MemberAddress, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn page() -> usize {
    12
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisteredQuery {
    /// Exact accessible asset revision of the baseline ZIP; not a host path.
    pub baseline: String,
    /// Exact accessible asset revision of the ZIP to compare with it.
    pub selected: String,
    /// Leave empty to compare original paths. Otherwise use a complete enclosing folder prefix ending in '/'. No file may be excluded.
    #[serde(default)]
    pub baseline_root: String,
    /// Chosen enclosing folder prefix in the selected ZIP, or empty for original paths.
    #[serde(default)]
    pub selected_root: String,
    /// Instead of a path comparison page, locate one source member’s bytes anywhere in the other ZIP. Use the full original path; offset and page_size select matching paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locate: Option<MemberAddress>,
    /// Case-insensitive literal substring of the compared path. Counts still describe all regular files.
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub differences_only: bool,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page")]
    pub page_size: usize,
}
impl RegisteredQuery {
    pub(in crate::neural) fn validate(&self) -> Result<()> {
        if self.page_size == 0 {
            return Err(failure("Use a positive ZIP comparison page size"));
        }
        if self.locate.is_some() && (!self.query.is_empty() || self.differences_only) {
            return Err(failure("Location lookup searches every path in the other ZIP. Omit query and differences_only."));
        }
        if [&self.baseline, &self.selected]
            .iter()
            .any(|r| !r.starts_with("asset:"))
        {
            return Err(failure("Choose two exact registered asset revisions"));
        }
        Ok(())
    }
}

pub fn compare_registered(db: &Connection, actor: &str, q: &RegisteredQuery) -> Result<Value> {
    q.validate()?;
    let (a, left) = registered(db, Some(actor), &q.baseline)?;
    let files = fingerprint(&left)?;
    drop(left);
    // Read and authorize the selected revision independently, including when
    // two registrations share bytes. Equal bytes need not share a history.
    let (b, right) = registered(db, Some(actor), &q.selected)?;
    let other = if a["sha256"] == b["sha256"] {
        files.clone()
    } else {
        fingerprint(&right)?
    };
    drop(right);
    let comparison = paths::compare(files, other, &q.baseline_root, &q.selected_root)?;
    let basis = "Exact registered bytes; not evidence that either file was issued, used, reviewed or accepted.";
    if let Some(anchor) = &q.locate {
        return Ok(json!({"baseline":a,"selected":b,"locations":comparison.locate(anchor,q.offset,q.page_size)?,"basis":basis}));
    }
    let mut counts = [0; 4];
    let mut total = 0usize;
    let mut items = Vec::new();
    let literal = q.query.to_lowercase();
    for m in &comparison.members {
        counts[m.difference as usize] += 1;
        if (q.differences_only && m.difference == Difference::Same)
            || (!literal.is_empty() && !m.path.to_lowercase().contains(&literal))
        {
            continue;
        }
        if total >= q.offset && items.len() < q.page_size {
            items.push(m);
        }
        total += 1;
    }
    let end = q.offset.saturating_add(items.len());
    Ok(
        json!({"baseline":a,"selected":b,"baseline_root":comparison.baseline_root,"selected_root":comparison.selected_root,
        "baseline_enclosing_folder":comparison.baseline_enclosing_folder,"selected_enclosing_folder":comparison.selected_enclosing_folder,
        "counts":{"same":counts[Difference::Same as usize],"changed":counts[Difference::Changed as usize],"only_baseline":counts[Difference::OnlyBaseline as usize],"only_selected":counts[Difference::OnlySelected as usize]},
        "matching":total,"offset":q.offset,"next_offset":(end<total).then_some(end),"items":items,"scope":comparison.scope,
        "basis":basis}),
    )
}
