//! Encode and hash complete canonical objects without cloning their JSON bodies.
use super::{Result, StoredObject};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{BufWriter, Write};

pub(super) struct CanonicalValue<'a>(pub(super) &'a Value);

impl Serialize for CanonicalValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(fields) => {
                // Sort borrowed entries even when serde_json/preserve_order is
                // enabled. Keys, strings and nested bodies remain borrowed.
                let mut entries: Vec<_> = fields.iter().collect();
                entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, &CanonicalValue(value))?;
                }
                map.end()
            }
            Value::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(&CanonicalValue(value))?;
                }
                sequence.end()
            }
            // Use serde_json's existing number, string and scalar encoding.
            value => value.serialize(serializer),
        }
    }
}

pub(super) struct CanonicalObject<'a>(pub(super) &'a StoredObject);

impl Serialize for CanonicalObject<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        // Exhaustive destructuring makes a new StoredObject field require an
        // explicit update here. Emit all four fields in canonical key order.
        let StoredObject {
            id,
            revision,
            kind,
            body,
        } = self.0;
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("body", &CanonicalValue(body))?;
        map.serialize_entry("id", id)?;
        map.serialize_entry("kind", kind)?;
        map.serialize_entry("revision", revision)?;
        map.end()
    }
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn encode(object: &StoredObject) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&CanonicalObject(object))?)
}

pub(super) fn fingerprint(object: &StoredObject) -> Result<String> {
    fingerprint_serialized(&CanonicalObject(object))
}

/// The caller supplies a serializer that emits every field in canonical order.
pub(super) fn fingerprint_serialized(value: &impl Serialize) -> Result<String> {
    let mut hash = Sha256::new();
    {
        // Buffer small JSON writes; this is not a record-size ceiling.
        let mut writer = BufWriter::new(HashWriter(&mut hash));
        serde_json::to_writer(&mut writer, value)?;
        writer.flush()?;
    }
    Ok(hex::encode(hash.finalize()))
}
