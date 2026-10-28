//! Explain a rejected native command using its current schema. This never
//! validates, repairs or executes an input, and runs only after Serde rejects it.
use super::*;

pub(super) fn enrich(received: &Value, error: serde_json::Error, section: &str) -> serde_json::Error {
    let schema = work_schema();
    let hint = p::ShapeHint {
        section: section.into(),
        allowed_fields: crate::schema_diagnostics::object_variant(&schema, &schema, received)
            .and_then(|variant| variant["properties"].as_object())
            .map(|fields| fields.keys().cloned().collect())
            .unwrap_or_default(),
        // Inspect the whole union even when no command can be selected. This
        // locates a missing or unknown inputs.kind without choosing its value.
        issues: crate::schema_diagnostics::issues(&schema, &schema, received, section),
    };
    // Keep the decoder's structural category and RuntimeError::Json class,
    // without copying rejected keys or values into a later recovery prompt.
    // The exact response remains retained; no successful parse takes this path.
    <serde_json::Error as serde::de::Error>::custom(format!(
        "{hint}: {}", p::shape_rule(&error)
    ))
}
