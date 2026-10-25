//! Schema-derived shape hints. Never echo rejected keys, tags or authored text.
use super::*;
use serde_path_to_error::{Path, Segment};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeHint {
    /// A schema-owned top-level member, optionally with an actual operation index.
    pub section: String,
    /// Fields of this exact known object variant, not a suggested repair.
    pub allowed_fields: Vec<String>,
    /// Missing members and definite mismatches at schema-owned nested paths.
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

pub(super) fn root_member_hint(key: &str) -> ShapeHint {
    let schema = decision_schema();
    schema["properties"]
        .as_object()
        .and_then(|properties| properties.get_key_value(key))
        .map(|(known, _)| ShapeHint {
            section: format!("response.{known}"),
            allowed_fields: Vec::new(),
            issues: Vec::new(),
        })
        .unwrap_or_default()
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
    hint.issues = crate::schema_diagnostics::issues(target_schema, &schema, target, &hint.section);
    hint
}
