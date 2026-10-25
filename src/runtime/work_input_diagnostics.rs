//! Explain a rejected native command using its current schema. This never
//! validates, repairs or executes an input, and runs only after Serde rejects it.
use super::*;

pub(super) fn enrich(received: &Value, error: serde_json::Error) -> serde_json::Error {
    let schema = work_schema();
    let Some(variant) = schema["oneOf"].as_array().and_then(|variants| {
        let kind = received.get("kind")?.as_str()?;
        variants.iter().find(|variant| {
            variant["properties"]["kind"]["const"].as_str() == Some(kind)
        })
    }) else {
        // An unknown command has no known object variant to describe.
        return error;
    };
    let Some(properties) = variant["properties"].as_object() else {
        return error;
    };
    let command = variant["properties"]["kind"]["const"].as_str().unwrap();
    let mut details = vec![format!(
        "native work input {command}; allowed fields: {}",
        properties.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
    )];
    details.extend(crate::schema_diagnostics::issues(variant, &schema, received, "inputs"));
    // Keep the original Serde error and its RuntimeError::Json classification.
    // No successful parse takes this path, and no authored value is replaced.
    <serde_json::Error as serde::de::Error>::custom(format!("{error}; {}", details.join("; ")))
}
