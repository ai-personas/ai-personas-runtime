//! Lossless storage of file bytes and repeated typed host exposure manifests.
//! The in-memory State and file-revision identity inputs remain expanded.
use super::*;
use std::collections::HashMap;

mod borrowed;
mod manifests;

// This adapter applies only to a runtime ManagedRevision. FileRevision's public
// wire format, content, and computed_revision tuple are deliberately unchanged.
#[derive(Serialize, Deserialize)]
#[serde(remote = "f::FileRevision", deny_unknown_fields)]
pub(super) struct StoredFileRevision {
    source: String,
    revision: String,
    path: String,
    scope: f::Scope,
    #[serde(with = "retained_bytes")]
    bytes: Vec<u8>,
    author: String,
    policy: f::Policy,
    qualifications: Vec<f::Reference>,
    retired: bool,
}

mod retained_bytes {
    use base64::{display::Base64Display, engine::general_purpose::STANDARD, Engine};
    use serde::{de, Deserialize, Deserializer, Serializer};
    use std::fmt;

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        // Avoid one JSON Value per byte while constructing and cloning State.
        serializer.collect_str(&Base64Display::new(bytes, &STANDARD))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> de::Visitor<'de> for Bytes {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("standard padded base64 or retained byte array")
            }

            fn visit_str<E: de::Error>(self, encoded: &str) -> Result<Self::Value, E> {
                STANDARD.decode(encoded).map_err(E::custom)
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                sequence: A,
            ) -> Result<Self::Value, A::Error> {
                // Earlier immutable snapshots remain readable as recorded;
                // opening one does not rewrite it or change its exact reference.
                Vec::<u8>::deserialize(de::value::SeqAccessDeserializer::new(sequence))
            }
        }
        deserializer.deserialize_any(Bytes)
    }
}

const POOL: &str = "retained_exposures";
const REFERENCE: &str = "retained_exposure";
const DOMAIN: &str = "runtime-retained-exposure-v1";
const POOL_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
#[serde(remote = "State", deny_unknown_fields)]
struct ExpandedState {
    format: u32,
    config: Config,
    personas: BTreeMap<String, Persona>,
    sources: BTreeMap<String, Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    work_guides: Option<work_guides::Catalog>,
    #[serde(default)]
    accepted_bindings: Vec<f::AcceptedBinding>,
    #[serde(default)]
    semantic_warnings: BTreeMap<String, warnings::SemanticWarning>,
    board: w::WorkBoard,
    work_meta: BTreeMap<String, WorkMeta>,
    observations: BTreeMap<String, Observation>,
    messages: BTreeMap<String, Message>,
    local_deliveries: BTreeMap<String, Value>,
    interpretations: BTreeMap<String, Value>,
    #[serde(default)]
    local_evidence: LocalEvidenceBridge,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    intake_episodes: BTreeMap<String, need_proposals::IntakeEpisode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    intake_episode_selections: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    computation_grants: BTreeMap<String, tools::Grant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    computation_jobs: BTreeMap<String, tools::JobMarker>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    http_fetch_grants: BTreeMap<String, http_tools::Grant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    http_fetch_jobs: BTreeMap<String, http_tools::JobMarker>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    host_command_jobs: BTreeMap<String, tools::JobMarker>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    optional_activities: BTreeMap<String, optional_activity::Opportunity>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    persona_creations: BTreeMap<String, launch::persona_birth::Pending>,
    operator_events: Vec<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pool {
    version: u32,
    entries: BTreeMap<String, Value>,
}

fn invalid_storage(message: &str) -> serde_json::Error {
    <serde_json::Error as serde::de::Error>::custom(message)
}

fn identity(value: &Value) -> serde_json::Result<String> {
    Ok(s::sha256_bytes(&serde_json::to_vec(&(DOMAIN, value))?))
}

fn pack(mut value: Value) -> serde_json::Result<Value> {
    let mut candidates: BTreeMap<String, (Value, usize)> = BTreeMap::new();
    manifests::edit(&mut value, POOL_VERSION, |exposure| {
        let key = identity(exposure)?;
        let entry = candidates
            .entry(key)
            .or_insert_with(|| (exposure.clone(), 0));
        if entry.0 != *exposure {
            return Err(invalid_storage("retained exposure identity conflict"));
        }
        entry.1 += 1;
        Ok(())
    })?;
    let mut entries = BTreeMap::new();
    for (key, (exposure, copies)) in candidates {
        let bytes = serde_json::to_vec(&exposure)?.len();
        // Charge a conservative 128 bytes per reference and table entry. Small
        // or unique manifests remain inline; compaction never inflates them.
        if copies > 1 && bytes.saturating_mul(copies - 1) > 128 * (copies + 1) {
            entries.insert(key, exposure);
        }
    }
    if entries.is_empty() {
        return Ok(value);
    }
    manifests::edit(&mut value, POOL_VERSION, |exposure| {
        let key = identity(exposure)?;
        if entries.contains_key(&key) {
            *exposure = json!({REFERENCE: key});
        }
        Ok(())
    })?;
    value[POOL] = serde_json::to_value(Pool {
        version: POOL_VERSION,
        entries,
    })?;
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolHeader {
    version: u32,
    entries: serde::de::IgnoredAny,
}

fn exposure_references(value: &Value) -> serde_json::Result<Option<HashMap<usize, &Value>>> {
    let Some(pool) = value.get(POOL) else {
        return Ok(None);
    };
    let header = PoolHeader::deserialize(pool)?;
    let _ = header.entries;
    // Header deserialization already checks both named and positional struct
    // forms. Borrow the entries from the form it actually accepted.
    let entries = match pool {
        Value::Array(fields) => fields.get(1),
        _ => pool.get("entries"),
    }
    .and_then(Value::as_object)
    .ok_or_else(|| invalid_storage("retained exposure entries unavailable"))?;
    if !matches!(header.version, 1 | POOL_VERSION) || entries.is_empty() {
        return Err(invalid_storage("unsupported retained exposure pool"));
    }
    for (key, exposure) in entries {
        // Keep complete digest and typed-shape checks before resolving any use.
        if &identity(exposure)? != key {
            return Err(invalid_storage("retained exposure digest mismatch"));
        }
        Exposure::deserialize(exposure)?;
    }
    let mut used = BTreeSet::new();
    let mut references = HashMap::new();
    manifests::visit(value, header.version, |exposure| {
        if let Some(reference) = exposure.get(REFERENCE) {
            let key = reference
                .as_str()
                .filter(|_| exposure.as_object().is_some_and(|o| o.len() == 1))
                .ok_or_else(|| invalid_storage("invalid retained exposure reference"))?;
            let retained = entries
                .get(key)
                .ok_or_else(|| invalid_storage("retained exposure absent"))?;
            used.insert(key.to_owned());
            references.insert(borrowed::location(exposure), retained);
        }
        Ok(())
    })?;
    if used.len() != entries.len() {
        return Err(invalid_storage("unreferenced retained exposure"));
    }
    Ok(Some(references))
}

/// Build the complete stored body once. Value consumers can take this packed
/// tree directly instead of traversing it again through a Value serializer.
pub(super) fn to_value(state: &State) -> serde_json::Result<Value> {
    let expanded = phase_timing::measure("state.serialize_expanded", || {
        ExpandedState::serialize(state, serde_json::value::Serializer)
    })?;
    phase_timing::measure("state.pack_exposures", || pack(expanded))
}

impl Serialize for State {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        to_value(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

/// Decode the borrowed stored body without cloning it or expanding a second
/// JSON tree. Only the explicit typed Exposure locations can resolve pool entries.
pub(super) fn decode(value: &Value) -> serde_json::Result<State> {
    let references = phase_timing::measure("state.validate_exposure_pool", || {
        exposure_references(value)
    })?;
    phase_timing::measure("state.deserialize_borrowed", || match references {
        Some(references) => borrowed::deserialize(value, references),
        None => ExpandedState::deserialize(value),
    })
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        decode(&value).map_err(serde::de::Error::custom)
    }
}
