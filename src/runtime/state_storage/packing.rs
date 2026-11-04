//! Pack each retained record before adding it to the complete stored body.
//! Only the existing manifest inventory selects exposure fields. Authored JSON
//! remains opaque, even when it contains reference-shaped objects.
use super::*;
use serde_json::Map;
use std::{cell::RefCell, rc::Rc};

pub(super) mod capture;

enum Candidate {
    Typed(Exposure),
    Expanded(Value),
}

impl Candidate {
    fn same(&self, other: &Self) -> serde_json::Result<bool> {
        Ok(match (self, other) {
            (Self::Typed(a), Self::Typed(b)) => a == b,
            (Self::Expanded(a), Self::Expanded(b)) => a == b,
            (Self::Typed(a), Self::Expanded(b)) | (Self::Expanded(b), Self::Typed(a)) => {
                capture::without(|| serde_json::to_value(a))? == *b
            }
        })
    }
}

#[derive(Default)]
struct Packing {
    candidates: BTreeMap<String, Candidate>,
    cache: Rc<RefCell<capture::Cache>>,
}

impl Packing {
    fn retain(&mut self, key: String, full: Candidate) -> serde_json::Result<()> {
        match self.candidates.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(full);
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                if !entry.get().same(&full)? {
                    return Err(invalid_storage("retained exposure identity conflict"));
                }
            }
        }
        Ok(())
    }

    fn expanded_field(&mut self, name: &str, value: Value) -> serde_json::Result<Value> {
        // Reuse the exact versioned inventory against this partial state. Its
        // two required collections are present even when another field is the
        // one being serialized. Values move into and out of this small wrapper.
        let mut root = json!({"observations": {}, "messages": {}});
        root[name] = value;
        manifests::edit(&mut root, POOL_VERSION, |exposure| {
            let key = identity(exposure)?;
            let full = std::mem::replace(exposure, json!({REFERENCE: &key}));
            self.retain(key, Candidate::Expanded(full))
        })?;
        Ok(root.as_object_mut().unwrap().remove(name).unwrap())
    }

    fn field<T: Serialize>(&mut self, name: &str, value: &T) -> serde_json::Result<Value> {
        let (encoded, captured) = capture::value(&self.cache, value)?;
        let mut remaining: HashMap<String, (Exposure, usize)> = HashMap::new();
        for (key, exposure) in captured {
            let prior = remaining
                .entry(key)
                .or_insert_with(|| (exposure.clone(), 0));
            if prior.0 != exposure {
                return Err(invalid_storage("retained exposure identity conflict"));
            }
            prior.1 += 1;
        }
        let mut root = json!({"observations": {}, "messages": {}});
        root[name] = encoded;
        let mut recognized = true;
        let mut selected = Vec::new();
        manifests::edit(&mut root, POOL_VERSION, |value| {
            let found = value
                .as_object()
                .filter(|fields| fields.len() == 1)
                .and_then(|fields| fields.get(REFERENCE))
                .and_then(Value::as_str)
                .and_then(|key| remaining.get_mut(key).map(|entry| (key, entry)));
            if let Some((key, (exposure, count))) = found.filter(|(_, (_, count))| *count > 0) {
                *count -= 1;
                selected.push((key.to_owned(), exposure.clone()));
            } else {
                recognized = false;
            }
            Ok(())
        })?;
        if !recognized || remaining.values().any(|(_, count)| *count != 0) {
            // An Exposure outside the versioned inventory remains ordinary
            // JSON. Re-encode this complete field without capture rather than
            // searching arbitrary authored or historical JSON for placeholders.
            return self.expanded_field(name, capture::without(|| serde_json::to_value(value))?);
        }
        for (key, exposure) in selected {
            self.retain(key, Candidate::Typed(exposure))?;
        }
        Ok(root.as_object_mut().unwrap().remove(name).unwrap())
    }

    fn records<T: Serialize>(
        &mut self,
        name: &str,
        records: &BTreeMap<String, T>,
    ) -> serde_json::Result<Value> {
        let mut packed = Map::new();
        for (key, record) in records {
            let single = BTreeMap::from([(key, record)]);
            let mut field = self.field(name, &single)?;
            packed.insert(
                key.clone(),
                field.as_object_mut().unwrap().remove(key).unwrap(),
            );
        }
        Ok(Value::Object(packed))
    }

    fn finish(self, mut value: Value) -> serde_json::Result<Value> {
        // Different manifests also share most of their source references.
        // Pooling only duplicate complete manifests misses that sharing and
        // makes a unique job manifest expand its entire provenance again.
        if !self.candidates.is_empty() {
            let mut builder = atoms::Builder::default();
            let mut cache = self.cache.borrow_mut();
            for (key, candidate) in self.candidates {
                match candidate {
                    Candidate::Typed(exposure) => builder.typed(key, &exposure, &mut cache)?,
                    Candidate::Expanded(exposure) => builder.expanded(key, exposure)?,
                }
            }
            value[POOL] = builder.finish(&mut value)?;
        }
        Ok(value)
    }
}

pub(super) fn to_value(state: &State) -> serde_json::Result<Value> {
    capture::without(|| pack_state(state))
}

fn pack_state(state: &State) -> serde_json::Result<Value> {
    // Match every State field explicitly, so adding one requires updating this
    // representation as well as ExpandedState's decoding/default declarations.
    let State {
        format,
        config,
        personas,
        sources,
        work_guides,
        accepted_bindings,
        semantic_warnings,
        board,
        work_meta,
        observations,
        image_captures,
        messages,
        local_deliveries,
        interpretations,
        local_evidence,
        intake_episodes,
        intake_episode_selections,
        computation_grants,
        computation_jobs,
        http_fetch_grants,
        http_fetch_jobs,
        host_command_jobs,
        optional_activities,
        persona_creations,
        operator_events,
    } = state;
    let mut packing = Packing::default();
    let mut retained = retained_records::Encoder::default();
    let value = phase_timing::measure("state.serialize_records", || {
        let mut fields = Map::new();
        macro_rules! field {
            ($value:ident) => {
                fields.insert(
                    stringify!($value).into(),
                    packing.field(stringify!($value), $value)?,
                );
            };
        }
        macro_rules! records {
            ($value:ident) => {
                fields.insert(
                    stringify!($value).into(),
                    packing.records(stringify!($value), $value)?,
                );
            };
        }
        field!(format);
        field!(config);
        records!(personas);
        fields.insert("sources".into(), retained.records(sources, "source")?);
        if work_guides.is_some() {
            field!(work_guides);
        }
        field!(accepted_bindings);
        records!(semantic_warnings);
        field!(board);
        records!(work_meta);
        fields.insert(
            "observations".into(),
            retained.records(observations, "observation")?,
        );
        if !image_captures.is_empty() {
            records!(image_captures);
        }
        fields.insert("messages".into(), retained.records(messages, "message")?);
        field!(local_deliveries);
        field!(interpretations);
        field!(local_evidence);
        if !intake_episodes.is_empty() {
            records!(intake_episodes);
        }
        if !intake_episode_selections.is_empty() {
            records!(intake_episode_selections);
        }
        if !computation_grants.is_empty() {
            records!(computation_grants);
        }
        if !computation_jobs.is_empty() {
            records!(computation_jobs);
        }
        if !http_fetch_grants.is_empty() {
            records!(http_fetch_grants);
        }
        if !http_fetch_jobs.is_empty() {
            records!(http_fetch_jobs);
        }
        if !host_command_jobs.is_empty() {
            records!(host_command_jobs);
        }
        if !optional_activities.is_empty() {
            records!(optional_activities);
        }
        if !persona_creations.is_empty() {
            records!(persona_creations);
        }
        field!(operator_events);
        fields.insert(retained_records::POOL.into(), retained.finish());
        Ok(Value::Object(fields))
    })?;
    phase_timing::measure("state.pack_exposures", || packing.finish(value))
}
