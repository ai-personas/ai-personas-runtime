//! Lossless storage of repeated observation/message exposure manifests.
//! The in-memory State and every historical evidence identity remain expanded.
use super::*;

const POOL: &str = "retained_exposures";
const REFERENCE: &str = "retained_exposure";
const DOMAIN: &str = "runtime-retained-exposure-v1";
// A compact record must not permit unbounded allocation while being decoded.
// Store continues to enforce its separate, smaller serialized-record limits.
const MAX_EXPANDED_BYTES: usize = 64 * 1024 * 1024;
// Store's complete record/control envelope is at most 16 MiB. Leave that much
// additional decoded room for its prepaid pause/ending mutations, which must
// remain readable even when ordinary state growth has reached its own limit.
const MAX_SAVED_EXPANDED_BYTES: usize = MAX_EXPANDED_BYTES - 16 * 1024 * 1024;

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
    computation_grants: BTreeMap<String, tools::Grant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    computation_jobs: BTreeMap<String, tools::JobMarker>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    http_fetch_grants: BTreeMap<String, http_tools::Grant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    http_fetch_jobs: BTreeMap<String, http_tools::JobMarker>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    optional_activities: BTreeMap<String, optional_activity::Opportunity>,
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

// Visit only typed host fields. Authored text, arbitrary observation payloads,
// delivery bundles, archives and nested historical records are never rewritten.
fn manifests(
    state: &mut Value,
    mut visit: impl FnMut(&mut Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    for name in ["observations", "messages"] {
        let records = state
            .get_mut(name)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| invalid_storage("retained record collection unavailable"))?;
        for record in records.values_mut() {
            if let Some(exposure) = record.get_mut("exposure").filter(|v| !v.is_null()) {
                visit(exposure)?;
            }
        }
    }
    Ok(())
}

fn pack(mut value: Value) -> serde_json::Result<Value> {
    if serde_json::to_vec(&value)?.len() > MAX_SAVED_EXPANDED_BYTES {
        return Err(invalid_storage(
            "expanded runtime state exceeds its byte bound",
        ));
    }
    let mut candidates: BTreeMap<String, (Value, usize)> = BTreeMap::new();
    manifests(&mut value, |exposure| {
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
    manifests(&mut value, |exposure| {
        let key = identity(exposure)?;
        if entries.contains_key(&key) {
            *exposure = json!({REFERENCE: key});
        }
        Ok(())
    })?;
    value[POOL] = serde_json::to_value(Pool {
        version: 1,
        entries,
    })?;
    Ok(value)
}

fn unpack(mut value: Value) -> serde_json::Result<Value> {
    let pool = value.as_object_mut().and_then(|object| object.remove(POOL));
    let mut expanded_bytes = serde_json::to_vec(&value)?.len();
    if expanded_bytes > MAX_EXPANDED_BYTES {
        return Err(invalid_storage(
            "expanded runtime state exceeds its byte bound",
        ));
    }
    let Some(pool) = pool else {
        return Ok(value);
    };
    let pool: Pool = serde_json::from_value(pool)?;
    if pool.version != 1 || pool.entries.is_empty() {
        return Err(invalid_storage("unsupported retained exposure pool"));
    }
    let mut sizes = BTreeMap::new();
    for (key, exposure) in &pool.entries {
        // Hash checks bind the complete manifest, including source revisions,
        // representation/ranges, policy versions, restrictions and array order.
        if &identity(exposure)? != key {
            return Err(invalid_storage("retained exposure digest mismatch"));
        }
        serde_json::from_value::<Exposure>(exposure.clone())?;
        sizes.insert(key.clone(), serde_json::to_vec(exposure)?.len());
    }
    let mut used = BTreeSet::new();
    // Bound the complete expansion before cloning any referenced manifest.
    manifests(&mut value, |exposure| {
        if let Some(reference) = exposure.get(REFERENCE) {
            let key = reference
                .as_str()
                .filter(|_| exposure.as_object().is_some_and(|o| o.len() == 1))
                .ok_or_else(|| invalid_storage("invalid retained exposure reference"))?;
            let bytes = sizes
                .get(key)
                .ok_or_else(|| invalid_storage("retained exposure absent"))?;
            let reference_bytes = serde_json::to_vec(exposure)?.len();
            expanded_bytes = expanded_bytes
                .checked_sub(reference_bytes)
                .and_then(|n| n.checked_add(*bytes))
                .filter(|n| *n <= MAX_EXPANDED_BYTES)
                .ok_or_else(|| invalid_storage("expanded runtime state exceeds its byte bound"))?;
            used.insert(key.to_owned());
        }
        Ok(())
    })?;
    if used.len() != pool.entries.len() {
        return Err(invalid_storage("unreferenced retained exposure"));
    }
    manifests(&mut value, |exposure| {
        if let Some(key) = exposure.get(REFERENCE).and_then(Value::as_str) {
            *exposure = pool.entries[key].clone();
        }
        Ok(())
    })?;
    Ok(value)
}

impl Serialize for State {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let expanded = ExpandedState::serialize(self, serde_json::value::Serializer)
            .map_err(serde::ser::Error::custom)?;
        pack(expanded)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for State {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = unpack(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)?;
        ExpandedState::deserialize(value).map_err(serde::de::Error::custom)
    }
}
