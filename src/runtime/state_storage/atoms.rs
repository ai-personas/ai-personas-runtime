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

pub(super) fn pack(entries: Map<String, Value>, state: &mut Value) -> serde_json::Result<Value> {
    let mut sources = Table::default();
    let mut policies = Table::default();
    let mut indexed = Vec::new();
    let mut packed = Map::new();
    let mut identities = HashMap::new();
    for (old_identity, mut manifest) in entries {
        for (field, table) in [("sources", &mut sources), ("policies", &mut policies)] {
            let entries = manifest
                .get_mut(field)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| invalid_storage("retained provenance list unavailable"))?;
            for entry in entries {
                *entry = table.intern(std::mem::take(entry))?;
            }
        }
        indexed.push((old_identity, manifest));
    }
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
    references: &mut HashMap<usize, &'a Value>,
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
    for source in sources {
        f::Reference::deserialize(source)?;
    }
    for policy in policies {
        ScopedPolicy::deserialize(policy)?;
    }
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
                references.insert(borrowed::location(index), entry);
            }
        }
    }
    if used_sources.contains(&false) || used_policies.contains(&false) {
        return Err(invalid_storage("unreferenced retained provenance"));
    }
    Ok(entries)
}
