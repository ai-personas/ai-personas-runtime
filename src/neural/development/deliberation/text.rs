//! A file can ground a pause without turning a filename into a read, or
//! repeating its contents in two parts of the next prompt. Use the frozen
//! reader envelopes only; never reopen a host path or resolve today's head.
use crate::neural::{failure, protocol::ContextResource, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(in crate::neural) struct TextRange {
    reference: String,
    range: [usize; 2],
    resource_bytes: usize,
    text_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_blob: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

impl TextRange {
    fn observed(read: &Value) -> Option<Self> {
        let reference = read["reference"].as_str()?;
        let content = match reference.strip_prefix("object:") {
            Some(digest) => digest,
            None if reference.starts_with("asset:") => read["metadata"]["blob"].as_str()?,
            None => return None,
        };
        if content.len() != 64
            || !content
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        let text = read["text"].as_str()?;
        let [start, end] = serde_json::from_value::<[usize; 2]>(read["range"].clone()).ok()?;
        let bytes = usize::try_from(read["bytes"].as_u64()?).ok()?;
        if end > bytes || end.checked_sub(start)? != text.len() || (text.is_empty() && bytes != 0) {
            return None;
        }
        let sha256 = crate::store::sha256_bytes(text.as_bytes());
        if start == 0 && end == bytes && sha256 != content {
            return None;
        }
        Some(Self {
            reference: reference.to_owned(),
            range: [start, end],
            resource_bytes: bytes,
            text_sha256: sha256,
            asset_blob: reference.starts_with("asset:").then(|| content.to_owned()),
            path: read["metadata"]["path"].as_str().map(str::to_owned),
        })
    }

    fn selected_by(&self, selection: &ContextResource) -> bool {
        match selection {
            ContextResource::Full(r) => {
                *r == self.reference && self.range == [0, self.resource_bytes]
            }
            ContextResource::Excerpt {
                reference,
                start,
                end,
            } => *reference == self.reference && self.range == [*start, *end],
            ContextResource::Considered { .. } => false,
        }
    }
}

fn reads<'a>(data: &'a Value, reference: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |v| v["reference"] == reference)
}

pub(super) fn reselected(
    data: &Value,
    next: &[ContextResource],
    reference: &str,
) -> Result<BTreeSet<TextRange>> {
    let ranges: BTreeSet<_> = reads(data, reference)
        .filter_map(TextRange::observed)
        .filter(|r| next.iter().any(|s| r.selected_by(s)))
        .collect();
    if ranges.is_empty() || !data["context_recovery"].is_null() {
        return Err(failure(format!("For before_action, {reference} needs supplied text or a whole image input. Reselect the same text range or whole image reference in next_context.references. A name, wider range or manifest alone is not that observation.")));
    }
    Ok(ranges)
}

pub(super) fn verify_next(data: &Value, expected: &[TextRange]) -> Result<()> {
    let selection: Vec<ContextResource> =
        serde_json::from_value(data["participation"]["selection"]["references"].clone())?;
    let references: BTreeSet<_> = expected.iter().map(|r| r.reference.as_str()).collect();
    let supplied: BTreeSet<_> = references
        .into_iter()
        .flat_map(|r| reads(data, r))
        .filter(|read| read.get("opened_from").is_none())
        .filter_map(TextRange::observed)
        .collect();
    if !data["context_recovery"].is_null()
        || !expected
            .iter()
            .all(|r| supplied.contains(r) && selection.iter().any(|s| r.selected_by(s)))
    {
        return Err(failure("The reselected text on the fresh input differs from the text used for the pre-action appraisal"));
    }
    Ok(())
}
