//! Describe the tool's retained return. Do not consult today's file or reports.
use super::super::{counted, number};
use ai_personas::neural::context::read_observations::{ByteView, ReadBody};
use serde_json::Value;

pub(super) fn label(result: &Value) -> Option<String> {
    let mut label = match ReadBody::from_result(result)? {
        ReadBody::Record => "Record returned".to_owned(),
        ReadBody::Bytes { view, range, total } => {
            let whole = range == (0..total);
            let kind = match view {
                ByteView::Text if whole => "Full text",
                ByteView::Text => "Text excerpt",
                ByteView::Image => "Image",
                ByteView::NoText => "No text",
            };
            if whole {
                format!("{kind} · {}", counted(total, "byte", "bytes"))
            } else {
                format!(
                    "{kind} · {}-byte range at offset {} of {}",
                    number(range.end - range.start),
                    number(range.start),
                    number(total)
                )
            }
        }
    };
    if let Some(count) = result["feedback_now"]["count"].as_u64() {
        label.push_str(&format!(
            " · {} at read",
            counted(count, "field report", "field reports")
        ));
    }
    Some(label)
}
