//! Schema-derived shape hints. Never echo rejected keys, tags or authored text.
use super::*;
use serde_path_to_error::{Path, Segment};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeHint {
    /// A schema-owned top-level member, optionally with a bounded operation index.
    pub section: String,
    /// Fields of this exact known object variant, not a suggested repair.
    pub allowed_fields: Vec<String>,
}

impl Default for ShapeHint {
    fn default() -> Self {
        Self {
            section: "response".into(),
            allowed_fields: Vec::new(),
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
        Ok(())
    }
}

fn object_variant<'a>(schema: &'a Value, all: &'a Value, received: &Value) -> Option<&'a Value> {
    let mut current = schema;
    // Only generated local references and known discriminator variants count.
    // A wrong/unknown tag yields no field list instead of guessing a variant.
    for _ in 0..8 {
        if let Some(reference) = current["$ref"].as_str() {
            let pointer = reference.strip_prefix('#')?;
            current = all.pointer(pointer)?;
        } else if let Some(variants) = current["oneOf"]
            .as_array()
            .or_else(|| current["anyOf"].as_array())
        {
            let kind = received.get("kind")?.as_str()?;
            current = variants
                .iter()
                .find(|variant| variant["properties"]["kind"]["const"].as_str() == Some(kind))?;
        } else {
            return current["properties"].as_object().map(|_| current);
        }
    }
    None
}

pub(super) fn hint(path: &Path, bytes: &[u8]) -> ShapeHint {
    let mut hint = ShapeHint::default();
    // Called only after the bounded, duplicate-free whole JSON preflight.
    let Ok(received) = serde_json::from_slice::<Value>(bytes) else {
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
                    if *index < MAX_OPERATIONS {
                        if let Some(operation) =
                            target.as_array().and_then(|values| values.get(*index))
                        {
                            hint.section.push_str(&format!("[{index}]"));
                            target = operation;
                            target_schema = &property["items"];
                        }
                    }
                }
            }
        }
    }
    if let Some(object) = object_variant(target_schema, &schema, target) {
        let fields = object["properties"]
            .as_object()
            .expect("resolved object properties");
        // Bound the diagnostic independently of future schema growth.
        if fields.len() <= 32 && fields.keys().map(String::len).sum::<usize>() <= 512 {
            hint.allowed_fields = fields.keys().cloned().collect();
        }
    }
    hint
}
