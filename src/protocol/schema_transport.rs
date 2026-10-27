//! Provider decoding assistance derived from the native response contract.
//! Native validation remains authoritative, including constraints omitted here.
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn ordinary(need_proposals: bool) -> Value {
    let mut schema = super::ordinary_schema(need_proposals);
    remove_annotations(&mut schema);
    schema
}

/// The prompt already supplies the complete descriptions and defaults. Remove
/// only presentation annotations from schema nodes, never similarly named
/// object properties, enum/const values, or open-ended authored payloads.
fn remove_annotations(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else { return; };
    for key in ["title", "description", "$comment", "default", "examples"] {
        object.remove(key);
    }
    for key in ["$defs", "definitions", "properties", "patternProperties", "dependentSchemas"] {
        if let Some(fields) = object.get_mut(key).and_then(Value::as_object_mut) {
            for field in fields.values_mut() { remove_annotations(field); }
        }
    }
    for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(branches) = object.get_mut(key).and_then(Value::as_array_mut) {
            for branch in branches { remove_annotations(branch); }
        }
    }
    for key in ["items", "additionalProperties", "additionalItems", "contains",
        "not", "if", "then", "else", "propertyNames", "unevaluatedProperties",
        "unevaluatedItems", "contentSchema"]
    {
        if let Some(child) = object.get_mut(key) { remove_annotations(child); }
    }
}

pub(super) fn initialization() -> Option<Value> {
    let mut schema = super::initialization_schema();
    convert(&mut schema)?;
    Some(schema)
}

fn convert(schema: &mut Value) -> Option<()> {
    let object = schema.as_object_mut()?;
    // Do not silently narrow a future open-ended interface. Such a contract
    // continues to use JSON-object transport and complete native validation.
    if object.keys().any(|key| !matches!(key.as_str(),
        "$schema" | "$comment" | "$defs" | "$ref" | "title" | "description"
        | "type" | "properties" | "required" | "additionalProperties" | "items"
        | "oneOf" | "anyOf" | "const" | "enum" | "default" | "format"
        | "minimum" | "maximum" | "minItems" | "maxItems" | "minLength"
        | "maxLength" | "uniqueItems"))
    {
        return None;
    }
    if object.get("type") == Some(&Value::String("object".into()))
        && (object.get("additionalProperties") != Some(&Value::Bool(false))
            || !object.get("properties").is_some_and(Value::is_object))
    {
        return None;
    }
    if let Some(variants) = object.remove("oneOf") {
        if object.contains_key("anyOf") { return None; }
        // Every generated union here is discriminated by a distinct fixed
        // kind. Its alternatives are disjoint, so anyOf has the same meaning.
        let mut tags = BTreeSet::new();
        for variant in variants.as_array()? {
            let tag = variant["properties"]["kind"]["const"].as_str()?;
            if !tags.insert(tag) { return None; }
        }
        object.insert("anyOf".into(), variants);
    }
    for key in ["$schema", "$comment", "title", "default", "minLength", "maxLength", "uniqueItems"] {
        object.remove(key);
    }
    // The provider rejects annotations beside a reference. The prompt keeps
    // the full field description; the referenced wire shape is unchanged.
    if object.contains_key("$ref") {
        object.remove("description");
        if object.len() != 1 { return None; }
    }
    // Schemars emits Rust integer-width annotations, not provider formats.
    // The integer type, numeric bounds and native integer parsing remain.
    if let Some(format) = object.remove("format") {
        if !matches!(format.as_str(), Some("uint32" | "uint64")) { return None; }
    }
    if let Some(constant) = object.remove("const") {
        if object.contains_key("enum") { return None; }
        object.insert("enum".into(), Value::Array(vec![constant]));
    }
    if let Some(properties) = object.get_mut("properties") {
        let properties = properties.as_object_mut()?;
        for value in properties.values_mut() { convert(value)?; }
        // Nullable fields keep their null alternative. Defaulted collections
        // become explicit arrays; the model can supply [] for no entries.
        let required = properties.keys().cloned().map(Value::String).collect();
        object.insert("required".into(), Value::Array(required));
    }
    if let Some(definitions) = object.get_mut("$defs") {
        for value in definitions.as_object_mut()?.values_mut() { convert(value)?; }
    }
    if let Some(variants) = object.get_mut("anyOf") {
        for value in variants.as_array_mut()? { convert(value)?; }
    }
    if let Some(items) = object.get_mut("items") { convert(items)?; }
    Some(())
}
