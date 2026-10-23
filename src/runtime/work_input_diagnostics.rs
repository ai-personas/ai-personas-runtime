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
    if let Some(required) = variant["required"].as_array() {
        for field in required.iter().filter_map(Value::as_str) {
            if properties.contains_key(field) && received.get(field).is_none() {
                details.push(format!("inputs.{field} is required"));
            }
        }
    }
    // Internally tagged enum deserialization can lose its nested field path.
    // Inspect known immediate fields for primitive/enum mismatches instead of
    // guessing a path from that error's prose or a supplied unknown field name.
    for (field, property) in properties {
        if let Some(value) = received.get(field) {
            if let Some(expected) = mismatch(property, &schema, value, &mut BTreeSet::new()) {
                details.push(format!("inputs.{field} expects {expected}"));
            }
        }
    }
    // Keep the original Serde error and its RuntimeError::Json classification.
    // No successful parse takes this path, and no authored value is replaced.
    <serde_json::Error as serde::de::Error>::custom(format!("{error}; {}", details.join("; ")))
}

fn mismatch(
    schema: &Value,
    root: &Value,
    received: &Value,
    visiting: &mut BTreeSet<*const Value>,
) -> Option<String> {
    let address = schema as *const Value;
    if !visiting.insert(address) {
        return None;
    }
    let result = constraint(schema, root, received, visiting);
    visiting.remove(&address);
    result
}

fn constraint(
    schema: &Value,
    root: &Value,
    received: &Value,
    visiting: &mut BTreeSet<*const Value>,
) -> Option<String> {
    if let Some(reference) = schema["$ref"].as_str() {
        let target = root.pointer(reference.strip_prefix('#')?)?;
        return mismatch(target, root, received, visiting);
    }
    if let Some(options) = schema["oneOf"].as_array().or_else(|| schema["anyOf"].as_array()) {
        let mut expected = BTreeSet::new();
        for option in options {
            // A compatible or unrecognized branch supplies no mismatch hint.
            // This is intentionally narrower than full JSON-schema validation.
            expected.insert(mismatch(option, root, received, visiting)?);
        }
        return (!expected.is_empty()).then(|| expected.into_iter().collect::<Vec<_>>().join(" or "));
    }
    if let Some(values) = schema["enum"].as_array() {
        if !values.contains(received) {
            return Some(format!("one of {}", schema["enum"]));
        }
    }
    if let Some(value) = schema.get("const") {
        if value != received {
            return Some(format!("{value}"));
        }
    }
    let matches_type = |name: &str| match name {
        "object" => received.is_object(),
        "array" => received.is_array(),
        "string" => received.is_string(),
        "boolean" => received.is_boolean(),
        "null" => received.is_null(),
        "integer" => received.is_i64() || received.is_u64(),
        "number" => received.is_number(),
        _ => true,
    };
    if let Some(name) = schema["type"].as_str() {
        return (!matches_type(name)).then(|| format!("JSON {name}"));
    }
    if let Some(types) = schema["type"].as_array() {
        let names = types.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        if !names.is_empty() && !names.iter().any(|name| matches_type(name)) {
            return Some(format!("JSON {}", names.join(" or ")));
        }
    }
    None
}
