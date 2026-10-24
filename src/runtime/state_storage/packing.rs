//! Pack each retained record before adding it to the complete stored body.
//! Only the existing manifest inventory selects exposure fields. Authored JSON
//! remains opaque, even when it contains reference-shaped objects.
use super::*;
use serde_json::Map;

#[derive(Default)]
struct Packing {
    candidates: BTreeMap<String, Candidate>,
}

struct Candidate {
    value: Value,
    copies: usize,
    remaining: usize,
    pooled: bool,
}

impl Packing {
    fn retain(&mut self, exposure: &mut Value) -> serde_json::Result<()> {
        let key = identity(exposure)?;
        let full = std::mem::replace(exposure, json!({REFERENCE: &key}));
        match self.candidates.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Candidate {
                    value: full,
                    copies: 1,
                    remaining: 1,
                    pooled: false,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let candidate = entry.get_mut();
                if candidate.value != full {
                    return Err(invalid_storage("retained exposure identity conflict"));
                }
                candidate.copies += 1;
                candidate.remaining += 1;
            }
        }
        Ok(())
    }

    fn field(&mut self, name: &str, value: Value) -> serde_json::Result<Value> {
        // Reuse the exact versioned inventory against this partial state. Its
        // two required collections are present even when another field is the
        // one being serialized. Values move into and out of this small wrapper.
        let mut root = json!({"observations": {}, "messages": {}});
        root[name] = value;
        manifests::edit(&mut root, POOL_VERSION, |exposure| self.retain(exposure))?;
        Ok(root.as_object_mut().unwrap().remove(name).unwrap())
    }

    fn records<T: Serialize>(
        &mut self,
        name: &str,
        records: &BTreeMap<String, T>,
    ) -> serde_json::Result<Value> {
        let mut packed = Map::new();
        for (key, record) in records {
            let mut single = Map::new();
            single.insert(key.clone(), serde_json::to_value(record)?);
            let mut field = self.field(name, Value::Object(single))?;
            packed.insert(key.clone(), field.as_object_mut().unwrap().remove(key).unwrap());
        }
        Ok(Value::Object(packed))
    }

    fn finish(mut self, mut value: Value) -> serde_json::Result<Value> {
        for candidate in self.candidates.values_mut() {
            let bytes = serde_json::to_vec(&candidate.value)?.len();
            let copies = candidate.copies;
            // Preserve the existing storage decision and exact pool identity.
            candidate.pooled = copies > 1
                && bytes.saturating_mul(copies - 1) > 128 * (copies + 1);
        }
        manifests::edit(&mut value, POOL_VERSION, |exposure| {
            let key = exposure
                .get(REFERENCE)
                .and_then(Value::as_str)
                .filter(|_| exposure.as_object().is_some_and(|fields| fields.len() == 1))
                .ok_or_else(|| invalid_storage("unpacked retained exposure"))?;
            let candidate = self.candidates.get_mut(key)
                .ok_or_else(|| invalid_storage("retained exposure absent"))?;
            candidate.remaining = candidate.remaining.checked_sub(1)
                .ok_or_else(|| invalid_storage("retained exposure count mismatch"))?;
            if !candidate.pooled {
                *exposure = if candidate.remaining == 0 {
                    std::mem::take(&mut candidate.value)
                } else {
                    candidate.value.clone()
                };
            }
            Ok(())
        })?;
        let mut entries = Map::new();
        for (key, candidate) in self.candidates {
            if candidate.remaining != 0 {
                return Err(invalid_storage("retained exposure count mismatch"));
            }
            if candidate.pooled {
                entries.insert(key, candidate.value);
            }
        }
        if !entries.is_empty() {
            // Move the completed pool into place without serializing its large
            // values through a second owned Value tree.
            let mut pool = Map::new();
            pool.insert("version".into(), POOL_VERSION.into());
            pool.insert("entries".into(), Value::Object(entries));
            value[POOL] = Value::Object(pool);
        }
        Ok(value)
    }
}

pub(super) fn to_value(state: &State) -> serde_json::Result<Value> {
    // Match every State field explicitly, so adding one requires updating this
    // representation as well as ExpandedState's decoding/default declarations.
    let State {
        format, config, personas, sources, work_guides, accepted_bindings,
        semantic_warnings, board, work_meta, observations, image_captures, messages,
        local_deliveries, interpretations, local_evidence, intake_episodes,
        intake_episode_selections, computation_grants, computation_jobs,
        http_fetch_grants, http_fetch_jobs, host_command_jobs, optional_activities,
        persona_creations, operator_events,
    } = state;
    let mut packing = Packing::default();
    let value = phase_timing::measure("state.serialize_records", || {
        let mut fields = Map::new();
        macro_rules! field {
            ($value:ident) => {
                fields.insert(stringify!($value).into(),
                    packing.field(stringify!($value), serde_json::to_value($value)?)?);
            };
        }
        macro_rules! records {
            ($value:ident) => {
                fields.insert(stringify!($value).into(),
                    packing.records(stringify!($value), $value)?);
            };
        }
        field!(format);
        field!(config);
        records!(personas);
        records!(sources);
        if work_guides.is_some() { field!(work_guides); }
        field!(accepted_bindings);
        records!(semantic_warnings);
        field!(board);
        records!(work_meta);
        records!(observations);
        if !image_captures.is_empty() { records!(image_captures); }
        records!(messages);
        field!(local_deliveries);
        field!(interpretations);
        field!(local_evidence);
        if !intake_episodes.is_empty() { records!(intake_episodes); }
        if !intake_episode_selections.is_empty() { records!(intake_episode_selections); }
        if !computation_grants.is_empty() { records!(computation_grants); }
        if !computation_jobs.is_empty() { records!(computation_jobs); }
        if !http_fetch_grants.is_empty() { records!(http_fetch_grants); }
        if !http_fetch_jobs.is_empty() { records!(http_fetch_jobs); }
        if !host_command_jobs.is_empty() { records!(host_command_jobs); }
        if !optional_activities.is_empty() { records!(optional_activities); }
        if !persona_creations.is_empty() { records!(persona_creations); }
        field!(operator_events);
        Ok(Value::Object(fields))
    })?;
    phase_timing::measure("state.pack_exposures", || packing.finish(value))
}
