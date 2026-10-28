//! A syntactic carrier for complete native decisions. Open JSON payloads stay
//! open; numerical, reference and cross-field checks remain native validation.
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const TOOL: &str = "submit_decision";
pub(super) const INSTRUCTIONS: &str = "Return the complete Decision JSON as the input of submit_decision. This tool only carries your response; it executes no operation. All operations are validated and applied by the persona runtime after the complete response arrives.";
pub(super) const ACKNOWLEDGMENT: &str = "Decision response received. This acknowledgment establishes neither adoption nor execution. Consult the current CONTROL and supplied observations for actual outcomes.";

const JSON: &str = r#"
json_value: json_object | json_array | JSON_STRING | JSON_NUMBER | "true" | "false" | "null"
json_object: "{" [JSON_STRING ":" json_value ("," JSON_STRING ":" json_value)*] "}"
json_array: "[" [json_value ("," json_value)*] "]"
JSON_STRING: /"([^"\\\x00-\x1f]|\\["\\\/bfnrt]|\\u[0-9a-fA-F]{4})*"/
JSON_NUMBER: /-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?/
JSON_INTEGER: /-?(0|[1-9][0-9]*)/
%ignore /[ \t\n\r]+/
"#;

/// Unsupported future schema constructs retain the existing non-strict schema
/// carrier instead of silently closing an open field or changing its meaning.
pub(super) fn compile(schema: &Value) -> Option<String> {
    let definitions = schema.get("$defs")?.as_object()?;
    let mut compiler = Compiler {
        names: definitions.keys().enumerate().map(|(i, name)| (name.clone(), format!("d{i}"))).collect(),
        rules: Vec::new(),
    };
    let root = compiler.node(schema)?;
    for (name, definition) in definitions {
        let body = compiler.node(definition)?;
        compiler.rules.push(format!("{}: {body}", compiler.names[name]));
    }
    Some(format!("start: {root}\n{}\n{JSON}", compiler.rules.join("\n")))
}

struct Compiler {
    names: BTreeMap<String, String>,
    rules: Vec<String>,
}

impl Compiler {
    fn rule(&mut self, body: String) -> String {
        let name = format!("r{}", self.rules.len());
        self.rules.push(format!("{name}: {body}"));
        name
    }

    fn node(&mut self, schema: &Value) -> Option<String> {
        if schema == &Value::Bool(true) { return Some("json_value".into()); }
        let object = schema.as_object()?;
        if object.keys().any(|key| !matches!(key.as_str(),
            "$schema" | "$defs" | "$ref" | "title" | "description" | "$comment" | "default"
            | "type" | "properties" | "required" | "additionalProperties" | "items"
            | "oneOf" | "anyOf" | "const" | "enum" | "format" | "minimum" | "maximum"
            | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf" | "minItems"
            | "maxItems" | "uniqueItems" | "minLength" | "maxLength" | "pattern")) {
            return None;
        }
        // Only the generated shape is compiled. Its annotations and value
        // constraints are still supplied in the prompt and checked natively.
        let selectors: Vec<_> = ["$ref", "const", "enum", "oneOf", "anyOf"]
            .into_iter().filter(|key| object.contains_key(*key)).collect();
        if selectors.len() > 1 { return None; }
        if let Some(selector) = selectors.first() {
            return match *selector {
                "$ref" => {
                    let name = object["$ref"].as_str()?.strip_prefix("#/$defs/")?;
                    self.names.get(name).cloned()
                }
                "const" => literal_json(&object["const"]),
                "enum" => {
                    let values = object["enum"].as_array()?;
                    if values.is_empty() { return None; }
                    let choices: Option<Vec<_>> = values.iter().map(literal_json).collect();
                    Some(self.rule(choices?.join(" | ")))
                }
                key => {
                    let variants = object[key].as_array()?;
                    if variants.is_empty() { return None; }
                    let choices: Option<Vec<_>> = variants.iter().map(|v| self.node(v)).collect();
                    Some(self.rule(choices?.join(" | ")))
                }
            };
        }
        match object.get("type") {
            None if object.keys().all(|key| matches!(key.as_str(), "title" | "description" | "$comment" | "default")) => Some("json_value".into()),
            Some(Value::Array(types)) if !types.is_empty() => {
                let choices: Option<Vec<_>> = types.iter().map(|kind| self.typed(kind.as_str()?, schema)).collect();
                Some(self.rule(choices?.join(" | ")))
            }
            Some(Value::String(kind)) => self.typed(kind, schema),
            _ => None,
        }
    }

    fn typed(&mut self, kind: &str, schema: &Value) -> Option<String> {
        Some(match kind {
            "string" => "JSON_STRING".into(),
            "number" => "JSON_NUMBER".into(),
            "integer" => "JSON_INTEGER".into(),
            "boolean" => self.rule("\"true\" | \"false\"".into()),
            "null" => "\"null\"".into(),
            "array" => {
                let item = self.node(schema.get("items").unwrap_or(&Value::Bool(true)))?;
                self.rule(format!("\"[\" [{item} (\",\" {item})*] \"]\""))
            }
            "object" => self.object(schema)?,
            _ => return None,
        })
    }

    fn object(&mut self, schema: &Value) -> Option<String> {
        let properties = schema.get("properties").and_then(Value::as_object);
        if properties.is_none_or(|fields| fields.is_empty()) {
            return match schema.get("additionalProperties") {
                Some(Value::Bool(false)) => Some("\"{\" \"}\"".into()),
                None | Some(Value::Bool(true)) => Some("json_object".into()),
                Some(value) => {
                    let value = self.node(value)?;
                    Some(self.rule(format!("\"{{\" [JSON_STRING \":\" {value} (\",\" JSON_STRING \":\" {value})*] \"}}\"")))
                }
            };
        }
        // Mixed declared/open-property objects need an unordered-key grammar.
        // Keep their original carrier until that construct is implemented.
        if schema.get("additionalProperties") != Some(&Value::Bool(false)) { return None; }
        let properties = properties?;
        let required = schema.get("required").and_then(Value::as_array);
        let required = |name: &str| required.is_some_and(|names| names.iter().any(|v| v.as_str() == Some(name)));
        let order = ["version", "invocation_id", "epoch", "kind", "id", "work", "operations", "reply",
            "fragment_maintenance", "asset_maintenance", "next_fragments", "next_assets", "continuation"];
        let mut fields: Vec<_> = properties.iter().collect();
        fields.sort_by_key(|(name, _)| (order.iter().position(|n| n == name).unwrap_or(order.len()), *name));
        // Use a required member as the first anchor. Optional members then
        // include their own comma; omission does not require a placeholder.
        let first = fields.iter().position(|(name, _)| required(name))?;
        let (name, value) = fields.remove(first);
        let mut body = format!("\"{{\" {} \":\" {}", literal_json(&Value::String(name.clone()))?, self.node(value)?);
        for (name, value) in fields {
            let field = format!("\",\" {} \":\" {}", literal_json(&Value::String(name.clone()))?, self.node(value)?);
            body.push_str(&if required(name) { format!(" {field}") } else { format!(" [{field}]") });
        }
        body.push_str(" \"}\"");
        Some(self.rule(body))
    }
}

fn literal_json(value: &Value) -> Option<String> {
    // First encode the JSON value, then quote those exact bytes for Lark.
    serde_json::to_string(&serde_json::to_string(value).ok()?).ok()
}
