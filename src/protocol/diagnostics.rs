//! Schema-derived shape hints. Never echo rejected keys, tags or authored text.
use super::*;
use serde_path_to_error::{Path, Segment};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeHint {
    /// A schema-owned path, optionally with actual array indices.
    pub section: String,
    /// Fields of this exact known object variant, not a suggested repair.
    pub allowed_fields: Vec<String>,
    /// Missing members and definite mismatches throughout the complete Decision.
    pub issues: Vec<String>,
}

impl Default for ShapeHint {
    fn default() -> Self {
        Self {
            section: "response".into(),
            allowed_fields: Vec::new(),
            issues: Vec::new(),
        }
    }
}

impl std::fmt::Display for ShapeHint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.section)?;
        if !self.allowed_fields.is_empty() {
            write!(
                formatter,
                "; allowed fields: {}",
                self.allowed_fields.join(", ")
            )?;
        }
        for issue in &self.issues {
            write!(formatter, "; {issue}")?;
        }
        Ok(())
    }
}

pub(super) enum JsonPathSegment {
    Member(String),
    Element(usize),
}

/// Resolve only declared structural names. A duplicate can be found before an
/// object's tag or closing delimiter, so do not deserialize the rejected input
/// or choose an authoritative variant from its first/last duplicate value.
fn path_step<'a>(
    candidates: &[&'a Value],
    root: &'a Value,
    segment: &JsonPathSegment,
) -> Option<(Vec<&'a Value>, String)> {
    let mut pending = candidates.to_vec();
    let mut visited = BTreeSet::new();
    let mut next = Vec::new();
    let mut label = None;
    while let Some(schema) = pending.pop() {
        if !visited.insert(schema as *const Value) {
            continue;
        }
        if let Some(reference) = schema["$ref"].as_str() {
            if let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p)) {
                pending.push(target);
            }
        }
        for union in ["oneOf", "anyOf", "allOf"] {
            if let Some(branches) = schema[union].as_array() {
                pending.extend(branches);
            }
        }
        match segment {
            JsonPathSegment::Member(key) => {
                if let Some((known, field)) = schema["properties"]
                    .as_object().and_then(|fields| fields.get_key_value(key))
                {
                    next.push(field);
                    label = Some(format!(".{known}"));
                } else if let Some(field) = schema.get("additionalProperties").filter(|v| v.is_object()) {
                    next.push(field);
                    label.get_or_insert_with(|| " values".into());
                }
            }
            JsonPathSegment::Element(index) => {
                if let Some(item) = schema.get("items") {
                    next.push(item);
                    label = Some(format!("[{index}]"));
                }
            }
        }
    }
    label.map(|label| (next, label))
}

pub(super) fn duplicate_member_hint(path: &[JsonPathSegment], key: &str) -> ShapeHint {
    let schema = decision_schema();
    let mut candidates = vec![&schema];
    let mut hint = ShapeHint::default();
    let member = JsonPathSegment::Member(key.to_owned());
    for segment in path.iter().chain(std::iter::once(&member)) {
        let Some((next, label)) = path_step(&candidates, &schema, segment) else {
            break;
        };
        hint.section.push_str(&label);
        candidates = next;
    }
    hint
}

pub(super) fn hint(path: &Path, bytes: &[u8]) -> ShapeHint {
    let mut hint = ShapeHint::default();
    // Called only after the complete, duplicate-free whole JSON preflight.
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let Ok(received) = Value::deserialize(&mut deserializer) else {
        return hint;
    };
    let schema = decision_schema();
    let mut target_schema = &schema;
    let mut target = &received;
    let mut segments = path.iter();
    if let Some(Segment::Map { key }) = segments.next() {
        // Look the key up in generated schema properties. Dynamic map keys and
        // rejected unknown names never enter a diagnostic or recovery message.
        if let Some((known, property)) = schema["properties"]
            .as_object()
            .and_then(|properties| properties.get_key_value(key))
        {
            hint.section = format!("response.{known}");
            target_schema = property;
            target = &received[known];
            if known == "operations" {
                if let Some(Segment::Seq { index }) = segments.next() {
                    if let Some(operation) = target.as_array().and_then(|values| values.get(*index))
                    {
                        hint.section.push_str(&format!("[{index}]"));
                        target = operation;
                        target_schema = &property["items"];
                    }
                }
            }
        }
    }
    if let Some(object) = crate::schema_diagnostics::object_variant(target_schema, &schema, target) {
        let fields = object["properties"]
            .as_object()
            .expect("resolved object properties");
        hint.allowed_fields = fields.keys().cloned().collect();
    }
    // A decoder can stop inside an operation before noticing omitted siblings
    // or response-level fields. Report all definite schema-owned mismatches in
    // the complete rejected value; the local section still locates its error.
    hint.issues = crate::schema_diagnostics::issues(&schema, &schema, &received, "response");
    hint
}
