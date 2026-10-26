//! Compact presentation of the generated response schema, never a new parser.
//! Every schema description and constraint stays attached to its own type.
use serde_json::{Map, Value};
use std::collections::BTreeSet;

const INTRO: &str = "Compact wire types (compact-decision-types-v1), generated from the application JSON Schema. Return one Decision JSON object, not type declarations. Fields are required unless marked ?. Objects reject extra fields unless [key: string] is shown; that index type constrains every extra field. Array<T> contains elements of type T. JSON allows any JSON value; never allows none; integer requires an integer JSON number. JSON literals denote exact values. oneOf(...) requires exactly one matching alternative; anyOf(...) requires at least one; allOf(...) requires every alternative. A | B allows either type. Each /* JSON */ note retains the JSON Schema descriptions, constraints and defaults for the immediately following type or field. Defaults do not make a required field optional. Native parsing, exact current references and cross-field checks still apply.\n\n";

/// Fall back to the complete generated JSON for any unsupported construct, or
/// when the readable form would be larger. No definition is selected or hidden.
pub(super) fn render(schema: &Value) -> String {
    let original = serde_json::to_string(schema).expect("generated JSON schema");
    compact(schema)
        .filter(|text| text.len() < original.len())
        .unwrap_or(original)
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn compact(schema: &Value) -> Option<String> {
    let object = schema.as_object()?;
    let definitions = object.get("$defs")?.as_object()?;
    if !definitions.keys().all(|name| {
        identifier(name)
            && !matches!(
                name.as_str(),
                "Decision"
                    | "JSON"
                    | "never"
                    | "Array"
                    | "oneOf"
                    | "anyOf"
                    | "allOf"
                    | "string"
                    | "integer"
                    | "number"
                    | "boolean"
                    | "null"
                    | "true"
                    | "false"
                    | "type"
            )
    }) {
        return None;
    }
    let names = definitions.keys().map(String::as_str).collect();
    let mut root = object.clone();
    root.remove("$defs");
    let mut output = INTRO.to_owned();
    output.push_str("type Decision = ");
    output.push_str(&node(&Value::Object(root), &names)?);
    output.push_str(";\n");
    for (name, definition) in definitions {
        output.push_str(&format!(
            "\ntype {name} = {}\n;\n",
            node(definition, &names)?
        ));
    }
    Some(output)
}

fn annotation(key: &str) -> bool {
    matches!(
        key,
        "$schema"
            | "$comment"
            | "title"
            | "description"
            | "default"
            | "format"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "multipleOf"
            | "minLength"
            | "maxLength"
            | "pattern"
            | "minItems"
            | "maxItems"
            | "uniqueItems"
            | "minProperties"
            | "maxProperties"
    )
}

fn note(values: Map<String, Value>) -> Option<String> {
    if values.is_empty() {
        return Some(String::new());
    }
    // JSON's escaped slash preserves the exact string while preventing an
    // authored schema description from terminating its presentation comment.
    let json = serde_json::to_string(&values).ok()?.replace("*/", "*\\/");
    Some(format!("/* {json} */\n"))
}

fn node(schema: &Value, names: &BTreeSet<&str>) -> Option<String> {
    if let Some(value) = schema.as_bool() {
        return Some(if value { "JSON" } else { "never" }.into());
    }
    let object = schema.as_object()?;
    let mut annotations = Map::new();
    let mut shape = Map::new();
    for (key, value) in object {
        if annotation(key) {
            annotations.insert(key.clone(), value.clone());
        } else {
            shape.insert(key.clone(), value.clone());
        }
    }
    let selectors: Vec<_> = ["$ref", "const", "enum", "oneOf", "anyOf", "allOf"]
        .into_iter()
        .filter(|key| shape.contains_key(*key))
        .collect();
    let body = if selectors.len() == 1 {
        let selector = selectors[0];
        let value = shape.remove(selector)?;
        // A sibling type is an additional constraint, including on literals
        // and references. Preserve it rather than assuming it is redundant.
        if let Some(value) = shape.remove("type") {
            annotations.insert("type".into(), value);
        }
        if !shape.is_empty() {
            return None;
        }
        match selector {
            "$ref" => {
                let name = value.as_str()?.strip_prefix("#/$defs/")?;
                if !names.contains(name) {
                    return None;
                }
                name.to_owned()
            }
            "const" => literal(&value)?,
            "enum" => {
                let values = value.as_array()?;
                if values.is_empty() {
                    return None;
                }
                values
                    .iter()
                    .map(literal)
                    .collect::<Option<Vec<_>>>()?
                    .join(" | ")
            }
            _ => {
                let alternatives = value.as_array()?;
                if alternatives.is_empty() {
                    return None;
                }
                let types = alternatives
                    .iter()
                    .map(|value| node(value, names))
                    .collect::<Option<Vec<_>>>()?;
                format!("{selector}(\n{}\n)", types.join(",\n"))
            }
        }
    } else if selectors.is_empty() {
        let kind = shape.remove("type");
        match kind.as_ref() {
            None if shape.is_empty() => "JSON".into(),
            Some(Value::Array(kinds)) if shape.is_empty() && !kinds.is_empty() => kinds
                .iter()
                .map(|kind| primitive(kind.as_str()?))
                .collect::<Option<Vec<_>>>()?
                .join(" | "),
            Some(Value::String(kind)) if kind == "object" => object_type(&mut shape, names)?,
            Some(Value::String(kind)) if kind == "array" => {
                let items = shape.remove("items").unwrap_or(Value::Bool(true));
                if !shape.is_empty() {
                    return None;
                }
                format!("Array<{}>", node(&items, names)?)
            }
            Some(Value::String(kind)) if shape.is_empty() => primitive(kind)?.into(),
            _ => return None,
        }
    } else {
        return None;
    };
    Some(format!("{}{body}", note(annotations)?))
}

fn primitive(kind: &str) -> Option<&str> {
    matches!(kind, "string" | "integer" | "number" | "boolean" | "null").then_some(kind)
}

fn literal(value: &Value) -> Option<String> {
    // Composite literals would overlap object/array type syntax. Keep their
    // complete canonical schema instead of requiring a second notation.
    if value.is_array() || value.is_object() {
        return None;
    }
    serde_json::to_string(value).ok()
}

fn object_type(shape: &mut Map<String, Value>, names: &BTreeSet<&str>) -> Option<String> {
    let properties = shape
        .remove("properties")
        .unwrap_or_else(|| Value::Object(Map::new()));
    let properties = properties.as_object()?;
    let required = shape
        .remove("required")
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let required = required
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<BTreeSet<_>>>()?;
    if !required.iter().all(|name| properties.contains_key(*name)) {
        return None;
    }
    let additional = shape
        .remove("additionalProperties")
        .unwrap_or(Value::Bool(true));
    if !shape.is_empty() {
        return None;
    }
    let mut fields = Vec::new();
    for (name, value) in properties {
        let field = if identifier(name) {
            name.clone()
        } else {
            serde_json::to_string(name).ok()?
        };
        fields.push(format!(
            "{field}{}: {};",
            if required.contains(name.as_str()) {
                ""
            } else {
                "?"
            },
            node(value, names)?
        ));
    }
    if additional != Value::Bool(false) {
        fields.push(format!("[key: string]: {};", node(&additional, names)?));
    }
    Some(format!("{{\n{}\n}}", fields.join("\n")))
}
