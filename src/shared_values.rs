//! Share equal immutable values without caching an authorization or lookup.
//! Weak entries retain no value bodies. Hashes only locate candidates; complete
//! equality is required before sharing, and edits use the caller's copy-on-write.
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde::de::{self, Deserialize, Deserializer, Visitor};
use std::any::Any;
use std::cell::RefCell;
use std::fmt;

type Supplied = Arc<dyn Any + Send + Sync>;
thread_local! {
    static SUPPLIED: RefCell<Option<Supplied>> = const { RefCell::new(None) };
}

struct Restore(Option<Supplied>);
impl Drop for Restore {
    fn drop(&mut self) {
        SUPPLIED.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Transfer an already decoded immutable value during one synchronous visitor
/// call. The previous slot is restored on success, error or unwinding. This is
/// neither a persistent cache nor a serialized handle.
pub(crate) fn supply<T: Any + Send + Sync, R>(value: Arc<T>, visit: impl FnOnce() -> R) -> R {
    let previous = SUPPLIED.with(|slot| slot.borrow_mut().replace(value));
    let _restore = Restore(previous);
    visit()
}

/// Ordinary JSON newtypes visit `visit_newtype_struct`, which performs the
/// original complete decode and interning. Only the private storage decoder
/// directly invokes `visit_unit` while supplying an exact validated typed atom.
/// Null, integers, strings and reference-shaped authored JSON cannot select
/// that transfer path or name a cached value.
pub(crate) fn deserialize<'de, T, D>(
    pool: &'static OnceLock<Mutex<Pool<T>>>,
    name: &'static str,
    deserializer: D,
) -> Result<Arc<T>, D::Error>
where
    T: Deserialize<'de> + Eq + Hash + Any + Send + Sync,
    D: Deserializer<'de>,
{
    struct Shared<T: 'static>(&'static OnceLock<Mutex<Pool<T>>>);
    impl<'de, T> Visitor<'de> for Shared<T>
    where
        T: Deserialize<'de> + Eq + Hash + Any + Send + Sync,
    {
        type Value = Arc<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a complete immutable value")
        }

        fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
            T::deserialize(deserializer).map(|value| intern(self.0, value))
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            match SUPPLIED.with(|slot| slot.borrow_mut().take()) {
                Some(value) => value.downcast::<T>()
                    .map_err(|_| E::custom("shared decoded value type mismatch")),
                None => T::deserialize(de::value::UnitDeserializer::<E>::new())
                    .map(|value| intern(self.0, value)),
            }
        }
    }
    deserializer.deserialize_newtype_struct(name, Shared(pool))
}

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
