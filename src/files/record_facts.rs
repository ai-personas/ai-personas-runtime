//! Reuse pure measurements of the same immutable file record allocation.
//! A weak reference verifies allocation identity without retaining file bodies.
//! Arc copy-on-write either clones the record or dissociates its weak pointers;
//! a changed record therefore cannot reuse these facts. Current corpus policy,
//! lineage, identities, bindings and accounting still require their own checks.
use super::{serialized_len, FileRevision, FilesError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Default)]
struct Facts {
    encoded_bytes: OnceLock<usize>,
    computed_revision: OnceLock<String>,
}

pub(super) fn encoded_bytes(record: &Arc<FileRevision>) -> Result<usize, ()> {
    let facts = for_record(record);
    if facts.encoded_bytes.get().is_none() {
        let bytes = serialized_len(record, usize::MAX)?;
        let _ = facts.encoded_bytes.set(bytes);
    }
    Ok(*facts.encoded_bytes.get().expect("successful byte measurement"))
}

pub(super) fn computed_revision(record: &Arc<FileRevision>) -> Result<String, FilesError> {
    let facts = for_record(record);
    if facts.computed_revision.get().is_none() {
        let revision = record.computed_revision()?;
        let _ = facts.computed_revision.set(revision);
    }
    Ok(facts.computed_revision.get().expect("successful revision measurement").clone())
}

struct Entry {
    record: Weak<FileRevision>,
    facts: Arc<Facts>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<usize, Entry>,
    inserted_since_sweep: usize,
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn for_record(record: &Arc<FileRevision>) -> Arc<Facts> {
    let key = Arc::as_ptr(record) as usize;
    let mut cache = CACHE.get_or_init(|| Mutex::new(Cache::default()))
        .lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = cache.entries.get(&key) {
        if entry.record.upgrade().is_some_and(|prior| Arc::ptr_eq(&prior, record)) {
            return Arc::clone(&entry.facts);
        }
    }
    let facts = Arc::new(Facts::default());
    cache.entries.insert(key, Entry {
        record: Arc::downgrade(record),
        facts: Arc::clone(&facts),
    });
    cache.inserted_since_sweep += 1;
    // Collect dead metadata only; impose no limit on live records or history.
    if cache.inserted_since_sweep >= 1024 {
        cache.entries.retain(|_, entry| entry.record.strong_count() > 0);
        cache.inserted_since_sweep = 0;
    }
    facts
}
