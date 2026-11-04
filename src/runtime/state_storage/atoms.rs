//! Lossless sharing inside retained exposure manifests. Only typed provenance
//! entries use these indexes; authored JSON and native file bytes stay opaque.
use super::*;
use serde_json::Map;

fn table_identity(sources: &[Value], policies: &[Value]) -> serde_json::Result<String> {
    Ok(s::sha256_bytes(&serde_json::to_vec(&(
        "runtime-provenance-tables-v1",
        sources,
        policies,
    ))?))
}

fn manifest_identity(table: &str, manifest: &Value) -> serde_json::Result<String> {
    Ok(s::sha256_bytes(&serde_json::to_vec(&(
        "runtime-indexed-exposure-v1",
        table,
        manifest,
    ))?))
}

#[derive(Default)]
struct Table {
    by_digest: HashMap<String, usize>,
    entries: Vec<Value>,
}

impl Table {
    fn cached(
        &mut self,
        encoded: &mut super::packing::capture::Encoded,
    ) -> serde_json::Result<Value> {
        if let Some(index) = encoded.index {
            return Ok(Value::from(index));
        }
        let index = if let Some(index) = self.by_digest.get(&encoded.digest) {
            if self.entries[*index] != encoded.value {
                return Err(invalid_storage("retained provenance identity conflict"));
            }
            *index
        } else {
            let index = self.entries.len();
            self.entries.push(encoded.value.clone());
            self.by_digest.insert(encoded.digest.clone(), index);
            index
        };
        encoded.index = Some(index);
        Ok(Value::from(index))
    }

    fn intern(&mut self, value: Value) -> serde_json::Result<Value> {
        let digest = identity(&value)?;
        let index = if let Some(index) = self.by_digest.get(&digest) {
            if self.entries[*index] != value {
                return Err(invalid_storage("retained provenance identity conflict"));
            }
            *index
        } else {
            let index = self.entries.len();
            self.entries.push(value);
            self.by_digest.insert(digest, index);
            index
        };
        Ok(Value::from(index))
    }
}

#[derive(Default)]
pub(super) struct Builder {
    sources: Table,
    policies: Table,
    indexed: Vec<(String, Value)>,
}

impl Builder {
    pub(super) fn expanded(
        &mut self,
        old_identity: String,
        mut manifest: Value,
    ) -> serde_json::Result<()> {
        for (field, table) in [
            ("sources", &mut self.sources),
            ("policies", &mut self.policies),
        ] {
            let entries = manifest
                .get_mut(field)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| invalid_storage("retained provenance list unavailable"))?;
            for entry in entries {
                *entry = table.intern(std::mem::take(entry))?;
            }
        }
        self.indexed.push((old_identity, manifest));
        Ok(())
    }

    pub(super) fn typed(
        &mut self,
        old_identity: String,
        exposure: &Exposure,
        cache: &mut super::packing::capture::Cache,
    ) -> serde_json::Result<()> {
        let sources = exposure
            .sources
            .iter()
            .map(|value| self.sources.cached(cache.source(value)?))
            .collect::<serde_json::Result<Vec<_>>>()?;
        let policies = exposure
            .policies
            .iter()
            .map(|value| self.policies.cached(cache.policy(value)?))
            .collect::<serde_json::Result<Vec<_>>>()?;
        self.indexed
            .push((old_identity, json!({"sources":sources,"policies":policies})));
        Ok(())
    }

    pub(super) fn finish(self, state: &mut Value) -> serde_json::Result<Value> {
        let Self {
            sources,
            policies,
            indexed,
        } = self;
        let mut packed = Map::new();
        let mut identities = HashMap::new();
        let table_sha256 = table_identity(&sources.entries, &policies.entries)?;
        for (old_identity, manifest) in indexed {
            let new_identity = manifest_identity(&table_sha256, &manifest)?;
            if packed.insert(new_identity.clone(), manifest).is_some() {
                return Err(invalid_storage("retained manifest identity conflict"));
            }
            identities.insert(old_identity, new_identity);
        }
        manifests::edit(state, POOL_VERSION, |manifest| {
            if let Some(reference) = manifest.get_mut(REFERENCE) {
                let replacement = reference
                    .as_str()
                    .and_then(|key| identities.get(key))
                    .ok_or_else(|| invalid_storage("retained manifest identity absent"))?;
                *reference = Value::String(replacement.clone());
            }
            Ok(())
        })?;
        // Move the tables into place; do not copy their complete owned JSON trees.
        let mut pool = Map::new();
        pool.insert("version".into(), POOL_VERSION.into());
        pool.insert("entries".into(), Value::Object(packed));
        pool.insert("table_sha256".into(), Value::String(table_sha256));
        pool.insert("source_references".into(), Value::Array(sources.entries));
        pool.insert("policy_references".into(), Value::Array(policies.entries));
        Ok(Value::Object(pool))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    entries: serde::de::IgnoredAny,
    source_references: serde::de::IgnoredAny,
    policy_references: serde::de::IgnoredAny,
    table_sha256: String,
}

/// Validate every atom, manifest and index before returning any replacements.
/// A replacement's address is scoped to this immutable JSON borrow only.
pub(super) fn references<'a>(
    pool: &'a Value,
    references: &mut HashMap<usize, borrowed::Replacement<'a>>,
) -> serde_json::Result<&'a Map<String, Value>> {
    let header = Header::deserialize(pool)?;
    let _ = (
        header.entries,
        header.source_references,
        header.policy_references,
    );
    if header.version != POOL_VERSION {
        return Err(invalid_storage("unsupported provenance pool"));
    }
    let entries = pool
        .get("entries")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_storage("retained exposure entries unavailable"))?;
    let sources = pool
        .get("source_references")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_storage("retained source table unavailable"))?;
    let policies = pool
        .get("policy_references")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_storage("retained policy table unavailable"))?;
    if table_identity(sources, policies)? != header.table_sha256 {
        return Err(invalid_storage("retained provenance table digest mismatch"));
    }
    let decoded_sources = sources
        .iter()
        .map(f::Reference::deserialize)
        .collect::<serde_json::Result<Vec<_>>>()?;
    let decoded_policies = policies
        .iter()
        .map(ScopedPolicy::deserialize)
        .collect::<serde_json::Result<Vec<_>>>()?;
    let mut used_sources = vec![false; sources.len()];
    let mut used_policies = vec![false; policies.len()];
    for (key, manifest) in entries {
        if &manifest_identity(&header.table_sha256, manifest)? != key {
            return Err(invalid_storage("retained exposure digest mismatch"));
        }
        if !manifest.as_object().is_some_and(|fields| fields.len() == 2) {
            return Err(invalid_storage("invalid indexed exposure fields"));
        }
        for (field, table, used) in [
            ("sources", sources, &mut used_sources),
            ("policies", policies, &mut used_policies),
        ] {
            let indexes = manifest
                .get(field)
                .and_then(Value::as_array)
                .ok_or_else(|| invalid_storage("retained provenance indexes unavailable"))?;
            for index in indexes {
                let position = index
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| invalid_storage("invalid retained provenance index"))?;
                let entry = table
                    .get(position)
                    .ok_or_else(|| invalid_storage("retained provenance index absent"))?;
                used[position] = true;
                let replacement = if field == "sources" {
                    borrowed::Replacement::Reference(entry, decoded_sources[position].clone())
                } else {
                    borrowed::Replacement::Policy(entry, decoded_policies[position].clone())
                };
                references.insert(borrowed::location(index), replacement);
            }
        }
    }
    if used_sources.contains(&false) || used_policies.contains(&false) {
        return Err(invalid_storage("unreferenced retained provenance"));
    }
    Ok(entries)
}
