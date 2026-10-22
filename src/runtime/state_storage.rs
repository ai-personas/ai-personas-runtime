//! Lossless storage of repeated typed host exposure manifests.
//! The in-memory State and every historical evidence identity remain expanded.
use super::*;

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

fn object_records<'a>(
    parent: &'a mut Value,
    field: &str,
) -> serde_json::Result<impl Iterator<Item = &'a mut Value>> {
    let records = parent
        .get_mut(field)
        .map(|value| {
            value
                .as_object_mut()
                .ok_or_else(|| invalid_storage("retained object collection unavailable"))
        })
        .transpose()?;
    Ok(records.into_iter().flat_map(|records| records.values_mut()))
}

fn array_records<'a>(
    parent: &'a mut Value,
    field: &str,
) -> serde_json::Result<impl Iterator<Item = &'a mut Value>> {
    let records = parent
        .get_mut(field)
        .map(|value| {
            value
                .as_array_mut()
                .ok_or_else(|| invalid_storage("retained array collection unavailable"))
        })
        .transpose()?;
    Ok(records.into_iter().flat_map(|records| records.iter_mut()))
}

fn exposure_field(
    record: &mut Value,
    field: &str,
    visit: &mut impl FnMut(&mut Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    if let Some(exposure) = record.get_mut(field).filter(|value| !value.is_null()) {
        visit(exposure)?;
    }
    Ok(())
}

fn nested_exposure(
    record: &mut Value,
    field: &str,
    visit: &mut impl FnMut(&mut Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    if let Some(nested) = record.get_mut(field).filter(|value| !value.is_null()) {
        exposure_field(nested, "exposure", visit)?;
    }
    Ok(())
}

// Visit only typed host fields. Authored text, arbitrary observation payloads,
// delivery bundles, archives and nested historical records are never rewritten.
// Version 1 permits references only in observation/message exposures. Version 2
// adds this explicit inventory of Exposure-typed State fields, never a recursive
// search for similarly named or shaped values in authored content.
fn manifests(
    state: &mut Value,
    version: u32,
    mut visit: impl FnMut(&mut Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    for name in ["observations", "messages"] {
        let records = state
            .get_mut(name)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| invalid_storage("retained record collection unavailable"))?;
        for record in records.values_mut() {
            exposure_field(record, "exposure", &mut visit)?;
        }
    }
    if version == 1 {
        return Ok(());
    }
    if version != POOL_VERSION {
        return Err(invalid_storage("unsupported retained exposure pool"));
    }
    for persona in object_records(state, "personas")? {
        nested_exposure(persona, "character", &mut visit)?;
        for focus in object_records(persona, "focus")? {
            nested_exposure(focus, "authored_continuation", &mut visit)?;
            exposure_field(focus, "judgment_exposure", &mut visit)?;
            for exposure in array_records(focus, "maintenance_exposures")? {
                visit(exposure)?;
            }
            for field in [
                "feedback_dispositions",
                "peer_feedback",
                "maintenance_dispositions",
            ] {
                for receipt in array_records(focus, field)? {
                    exposure_field(receipt, "exposure", &mut visit)?;
                }
            }
        }
    }
    for source in object_records(state, "sources")? {
        for revision in array_records(source, "revisions")? {
            exposure_field(revision, "exposure", &mut visit)?;
        }
    }
    for meta in object_records(state, "work_meta")? {
        exposure_field(meta, "exposure", &mut visit)?;
        for exposure in object_records(meta, "completion_exposures")? {
            visit(exposure)?;
        }
        if let Some(origins) = meta.get_mut("arrangement_origins") {
            for field in [
                "delegations",
                "proposals",
                "responses",
                "activations",
                "acceptances",
            ] {
                for exposure in object_records(origins, field)? {
                    visit(exposure)?;
                }
            }
        }
        for wait in object_records(meta, "message_waits")? {
            exposure_field(wait, "exposure", &mut visit)?;
        }
        for question in object_records(meta, "questions")? {
            nested_exposure(question, "origin", &mut visit)?;
            for field in ["replies", "conclusions"] {
                for record in array_records(question, field)? {
                    nested_exposure(record, "origin", &mut visit)?;
                }
            }
        }
    }
    for warning in object_records(state, "semantic_warnings")? {
        nested_exposure(warning, "authored", &mut visit)?;
        for resolution in array_records(warning, "history")? {
            nested_exposure(resolution, "authored", &mut visit)?;
        }
    }
    if let Some(bridge) = state.get_mut("local_evidence") {
        for basis in object_records(bridge, "bases")? {
            exposure_field(basis, "exposure", &mut visit)?;
        }
    }
    for field in ["computation_jobs", "http_fetch_jobs", "host_command_jobs"] {
        for job in object_records(state, field)? {
            exposure_field(job, "exposure", &mut visit)?;
        }
    }
    for opportunity in object_records(state, "optional_activities")? {
        exposure_field(opportunity, "exposure", &mut visit)?;
        for episode in object_records(opportunity, "episodes")? {
            for admission in array_records(episode, "admissions")? {
                exposure_field(admission, "exposure", &mut visit)?;
            }
            nested_exposure(episode, "ended", &mut visit)?;
        }
    }
    Ok(())
}

fn pack(mut value: Value) -> serde_json::Result<Value> {
    let mut candidates: BTreeMap<String, (Value, usize)> = BTreeMap::new();
    manifests(&mut value, POOL_VERSION, |exposure| {
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
    manifests(&mut value, POOL_VERSION, |exposure| {
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

fn unpack(mut value: Value) -> serde_json::Result<Value> {
    let pool = value.as_object_mut().and_then(|object| object.remove(POOL));
    let Some(pool) = pool else {
        return Ok(value);
    };
    let pool: Pool = serde_json::from_value(pool)?;
    if !matches!(pool.version, 1 | POOL_VERSION) || pool.entries.is_empty() {
        return Err(invalid_storage("unsupported retained exposure pool"));
    }
    for (key, exposure) in &pool.entries {
        // Hash checks bind the complete manifest, including source revisions,
        // representation/ranges, policy versions, restrictions and array order.
        if &identity(exposure)? != key {
            return Err(invalid_storage("retained exposure digest mismatch"));
        }
        serde_json::from_value::<Exposure>(exposure.clone())?;
    }
    let mut used = BTreeSet::new();
    // Validate every reference before expanding the shared manifests.
    manifests(&mut value, pool.version, |exposure| {
        if let Some(reference) = exposure.get(REFERENCE) {
            let key = reference
                .as_str()
                .filter(|_| exposure.as_object().is_some_and(|o| o.len() == 1))
                .ok_or_else(|| invalid_storage("invalid retained exposure reference"))?;
            if !pool.entries.contains_key(key) {
                return Err(invalid_storage("retained exposure absent"));
            }
            used.insert(key.to_owned());
        }
        Ok(())
    })?;
    if used.len() != pool.entries.len() {
        return Err(invalid_storage("unreferenced retained exposure"));
    }
    manifests(&mut value, pool.version, |exposure| {
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
