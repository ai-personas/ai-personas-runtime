//! Classify the saved read result, not the resource as it exists today.
use super::store::Record;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ByteView {
    Text,
    Image,
    NoText,
}

pub enum ReadBody {
    Record,
    Bytes {
        view: ByteView,
        range: Range<u64>,
        total: u64,
    },
}

impl ReadBody {
    pub fn from_result(result: &Value) -> Option<Self> {
        result["reference"].as_str()?;
        if !result["error"].is_null() || result["body_not_supplied"] == true {
            return None;
        }
        if let Some(record) = result.get("record") {
            return (record.is_object()
                && ["text", "binary", "image_blob"]
                    .iter()
                    .all(|k| result.get(k).is_none()))
            .then_some(Self::Record);
        }
        let [start, end] = result.get("range")?.as_array()?.as_slice() else {
            return None;
        };
        let range = start.as_u64()?..end.as_u64()?;
        let total = result["bytes"].as_u64()?;
        if range.start > range.end || range.end > total {
            return None;
        }
        let view = match (
            result.get("text"),
            result.get("image_blob"),
            result.get("binary"),
        ) {
            (Some(text), None, None) if text.as_str()?.len() as u64 == range.end - range.start => {
                ByteView::Text
            }
            (None, Some(image), None)
                if image.is_string() && result["mime"].is_string() && range == (0..total) =>
            {
                ByteView::Image
            }
            (None, None, Some(flag)) if flag == true => ByteView::NoText,
            _ => return None,
        };
        Some(Self::Bytes { view, range, total })
    }

    fn whole_field(self) -> Option<&'static str> {
        match self {
            Self::Record => Some("record"),
            Self::Bytes {
                view: ByteView::Text,
                range,
                total,
            } if range == (0..total) => Some("text"),
            _ => None,
        }
    }
}

fn same_body(a: &Value, b: &Value) -> bool {
    // An object must render identically, including signed floating-point zero.
    if a.is_object() && b.is_object() {
        a.to_string() == b.to_string()
    } else {
        a == b
    }
}

/// Only substitute a complete body present in this very input. Preserve all
/// the other read-time evidence (feedback, source, range, role, etc.) verbatim.
pub(super) fn share_bodies(events: &mut [Value], selected: &[Value], work: Option<&Record>) {
    let mut reads = events
        .iter_mut()
        .filter(|e| {
            e["kind"] == "tool_result"
                && matches!(
                    e["body"]["tool"].as_str(),
                    Some("asset.read" | "persona.read")
                )
                && e["supplied_result"]["reference"].is_string()
        })
        .peekable();
    if reads.peek().is_none() {
        return;
    }
    let mut sources: BTreeMap<&str, Vec<(&Value, String)>> = BTreeMap::new();
    let brief = work
        .filter(|w| w.body.is_object())
        .map(|w| (w.reference.handle(), &w.body));
    if let Some((reference, body)) = &brief {
        sources
            .entry(reference)
            .or_default()
            .push((body, "/work/record".into()));
    }
    for (i, view) in selected.iter().enumerate() {
        if let Some(field) = ReadBody::from_result(view).and_then(ReadBody::whole_field) {
            sources
                .entry(view["reference"].as_str().expect("read reference"))
                .or_default()
                .push((&view[field], format!("/selected_resources/{i}/{field}")));
        }
    }
    for event in reads {
        let view = &event["supplied_result"];
        let Some(field) = ReadBody::from_result(view).and_then(ReadBody::whole_field) else {
            continue;
        };
        let Some((_, pointer)) = view["reference"]
            .as_str()
            .and_then(|r| sources.get(r))
            .and_then(|rows| {
                rows.iter()
                    .find(|(other, _)| same_body(other, &view[field]))
            })
        else {
            continue;
        };
        let result = event["supplied_result"].as_object_mut().expect("read body");
        let marker = format!("{field}_supplied_at");
        if !result.contains_key(&marker) {
            result.remove(field);
            result.insert(marker, Value::String(pointer.clone()));
        }
    }
}
