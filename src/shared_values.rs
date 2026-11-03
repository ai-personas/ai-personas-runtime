//! Share equal immutable values without caching an authorization or lookup.
//! Weak entries retain no value bodies. Hashes only locate candidates; complete
//! equality is required before sharing, and edits use the caller's copy-on-write.
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub(crate) struct Pool<T> {
    entries: HashMap<u64, Vec<Weak<T>>>,
    inserted_since_sweep: usize,
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            inserted_since_sweep: 0,
        }
    }
}

pub(crate) fn intern<T: Eq + Hash>(pool: &OnceLock<Mutex<Pool<T>>>, value: T) -> Arc<T> {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    let key = hasher.finish();
    let mut pool = pool.get_or_init(|| Mutex::new(Pool::default()))
        .lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let candidates = pool.entries.entry(key).or_default();
    candidates.retain(|prior| prior.strong_count() > 0);
    if let Some(prior) = candidates.iter().filter_map(Weak::upgrade)
        .find(|prior| prior.as_ref() == &value)
    {
        return prior;
    }
    let shared = Arc::new(value);
    candidates.push(Arc::downgrade(&shared));
    pool.inserted_since_sweep += 1;
    // This is garbage collection of dead cache entries, not a limit on live
    // values, retained records, histories or application resources.
    if pool.inserted_since_sweep >= 1024 {
        pool.entries.retain(|_, candidates| {
            candidates.retain(|prior| prior.strong_count() > 0);
            !candidates.is_empty()
        });
        pool.inserted_since_sweep = 0;
    }
    shared
}
