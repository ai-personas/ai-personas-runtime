//! Copy-on-write retained records. Ordinary serialization exposes the complete
//! typed records; only State storage uses the lossless, independently encoded
//! records. A changed record invalidates its encoding before any mutable access.
use std::borrow::Borrow;
use std::collections::{btree_map, BTreeMap, HashMap};
use std::ops::Index;
use std::sync::{Arc, OnceLock};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod typed_json;

const FORMAT: &str = "runtime-retained-record-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Stored {
    retained_record: String,
    kind: String,
    json_bytes: usize,
    json_sha256: String,
    parts: Vec<usize>,
}

pub(super) const POOL: &str = "retained_record_chunks";

#[derive(Debug)]
pub(super) struct Chunk {
    digest: String,
    encoded: String,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct Encoding {
    kind: String,
    json_bytes: usize,
    json_sha256: String,
    chunks: Vec<Arc<Chunk>>,
}

#[derive(Debug)]
struct Record<T> {
    value: T,
    stored: OnceLock<Encoding>,
}

impl<T: Clone> Clone for Record<T> {
    fn clone(&self) -> Self {
        // The only mutable path clones a shared record and must recode it.
        Self { value: self.value.clone(), stored: OnceLock::new() }
    }
}

#[derive(Debug)]
pub struct RetainedRecords<T> {
    records: BTreeMap<String, Arc<Record<T>>>,
}

impl<T> Default for RetainedRecords<T> {
    fn default() -> Self { Self { records: BTreeMap::new() } }
}

impl<T> Clone for RetainedRecords<T> {
    fn clone(&self) -> Self { Self { records: self.records.clone() } }
}

impl<T> RetainedRecords<T> {
    pub fn new() -> Self { Self::default() }
    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &String> + ExactSizeIterator {
        self.records.keys()
    }
    pub fn iter(&self) -> Iter<'_, T> { Iter(self.records.iter()) }
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.records.values().map(|record| &record.value)
    }
    pub fn get<Q: Ord + ?Sized>(&self, key: &Q) -> Option<&T>
    where String: Borrow<Q> {
        self.records.get(key).map(|record| &record.value)
    }
    pub fn get_key_value<Q: Ord + ?Sized>(&self, key: &Q) -> Option<(&String, &T)>
    where String: Borrow<Q> {
        self.records.get_key_value(key).map(|(key, record)| (key, &record.value))
    }
    pub fn contains_key<Q: Ord + ?Sized>(&self, key: &Q) -> bool
    where String: Borrow<Q> { self.records.contains_key(key) }
    pub fn insert(&mut self, key: String, value: T) {
        self.records.insert(key, Arc::new(Record { value, stored: OnceLock::new() }));
    }
    pub fn entry(&mut self, key: String) -> Entry<'_, T> { Entry(self.records.entry(key)) }
}

impl<T: Clone> RetainedRecords<T> {
    pub fn into_values(self) -> impl DoubleEndedIterator<Item = T> + ExactSizeIterator {
        self.records.into_values().map(|record| match Arc::try_unwrap(record) {
            Ok(record) => record.value,
            Err(record) => record.value.clone(),
        })
    }
    pub fn get_mut<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<&mut T>
    where String: Borrow<Q> {
        self.records.get_mut(key).map(mutable)
    }
    pub fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> + ExactSizeIterator {
        self.records.values_mut().map(mutable)
    }
}

fn mutable<T: Clone>(record: &mut Arc<Record<T>>) -> &mut T {
    let record = Arc::make_mut(record);
    record.stored.take();
    &mut record.value
}

pub struct Entry<'a, T>(btree_map::Entry<'a, String, Arc<Record<T>>>);
impl<'a, T: Clone> Entry<'a, T> {
    pub fn or_insert_with(self, value: impl FnOnce() -> T) -> &'a mut T {
        mutable(self.0.or_insert_with(|| Arc::new(Record { value: value(), stored: OnceLock::new() })))
    }
}

pub struct Iter<'a, T>(btree_map::Iter<'a, String, Arc<Record<T>>>);
impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (&'a String, &'a T);
    fn next(&mut self) -> Option<Self::Item> { self.0.next().map(|(key, value)| (key, &value.value)) }
    fn size_hint(&self) -> (usize, Option<usize>) { self.0.size_hint() }
}
impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> { self.0.next_back().map(|(key, value)| (key, &value.value)) }
}
impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<'a, T> IntoIterator for &'a RetainedRecords<T> {
    type Item = (&'a String, &'a T);
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}
impl<T, Q: Ord + ?Sized> Index<&Q> for RetainedRecords<T>
where String: Borrow<Q> {
    type Output = T;
    fn index(&self, key: &Q) -> &T { self.get(key).expect("retained record key absent") }
}

impl<T: Serialize> Serialize for RetainedRecords<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self { map.serialize_entry(key, value)?; }
        map.end()
    }
}

fn error(message: impl std::fmt::Display) -> serde_json::Error {
    <serde_json::Error as serde::de::Error>::custom(message)
}

/// Byte sharing is independent of authored content and domain semantics. These
/// are encoding block sizes, not limits on any record, file, context or task.
fn chunks(bytes: &[u8], mut visit: impl FnMut(&[u8]) -> serde_json::Result<()>)
    -> serde_json::Result<()>
{
    static GEAR: OnceLock<[u64; 256]> = OnceLock::new();
    let gear = GEAR.get_or_init(|| std::array::from_fn(|i| {
        let mut z = (i as u64).wrapping_add(0x9e3779b97f4a7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }));
    let (mut start, mut hash) = (0, 0u64);
    for (i, byte) in bytes.iter().enumerate() {
        hash = hash.wrapping_shl(1).wrapping_add(gear[usize::from(*byte)]);
        let size = i + 1 - start;
        if (size >= 128 && hash & 511 == 0) || size >= 2048 {
            visit(&bytes[start..=i])?;
            start = i + 1;
            hash = 0;
        }
    }
    if start < bytes.len() { visit(&bytes[start..])?; }
    Ok(())
}

#[derive(Default)]
pub(super) struct Encoder {
    by_digest: HashMap<String, usize>,
    chunks: Vec<Arc<Chunk>>,
}

impl Encoder {
    fn retain(&mut self, chunk: Arc<Chunk>) -> serde_json::Result<usize> {
        if let Some(index) = self.by_digest.get(&chunk.digest) {
            let prior = &self.chunks[*index];
            if !Arc::ptr_eq(prior, &chunk) && prior.encoded != chunk.encoded {
                return Err(error("retained record chunk identity conflict"));
            }
            return Ok(*index);
        }
        let index = self.chunks.len();
        self.by_digest.insert(chunk.digest.clone(), index);
        self.chunks.push(chunk);
        Ok(index)
    }

    fn intern(&mut self, bytes: &[u8]) -> serde_json::Result<Arc<Chunk>> {
        let digest = crate::store::sha256_bytes(bytes);
        if let Some(index) = self.by_digest.get(&digest) {
            let prior = &self.chunks[*index];
            if prior.bytes != bytes { return Err(error("retained record chunk identity conflict")); }
            return Ok(prior.clone());
        }
        let chunk = Arc::new(Chunk { digest, encoded: STANDARD.encode(bytes), bytes: bytes.to_vec() });
        self.retain(chunk.clone())?;
        Ok(chunk)
    }

    pub(super) fn records<T: Serialize>(&mut self, records: &RetainedRecords<T>, kind: &str)
        -> serde_json::Result<Value>
    {
        let mut values = serde_json::Map::new();
        for (key, record) in &records.records {
            if record.stored.get().is_none() {
                let bytes = serde_json::to_vec(&record.value)?;
                let mut parts = Vec::new();
                chunks(&bytes, |part| { parts.push(self.intern(part)?); Ok(()) })?;
                let _ = record.stored.set(Encoding { kind: kind.into(), json_bytes: bytes.len(),
                    json_sha256: crate::store::sha256_bytes(&bytes), chunks: parts });
            }
            let encoded = record.stored.get().expect("retained encoding initialized");
            if encoded.kind != kind { return Err(error("retained record kind differs")); }
            let parts = encoded.chunks.iter().map(|part| self.retain(part.clone()))
                .collect::<serde_json::Result<Vec<_>>>()?;
            values.insert(key.clone(), serde_json::to_value(Stored {
                retained_record: FORMAT.into(), kind: kind.into(), json_bytes: encoded.json_bytes,
                json_sha256: encoded.json_sha256.clone(), parts,
            })?);
        }
        Ok(Value::Object(values))
    }

    pub(super) fn finish(self) -> Value {
        Value::Array(self.chunks.iter().map(|chunk| Value::String(chunk.encoded.clone())).collect())
    }
}

pub(super) struct Decoder {
    chunks: Vec<Arc<Chunk>>,
    used: Vec<bool>,
    atoms: typed_json::Atoms,
}

impl Decoder {
    pub(super) fn new(value: Option<&Value>, cached: &[Arc<Chunk>]) -> serde_json::Result<Self> {
        let cached: HashMap<_, _> = cached.iter().map(|chunk| (chunk.encoded.as_str(), chunk)).collect();
        let entries = value.map(|value| value.as_array()
            .ok_or_else(|| error("retained record chunk pool unavailable"))).transpose()?;
        let mut chunks = Vec::new();
        let mut unique = std::collections::HashSet::new();
        for value in entries.into_iter().flatten() {
            let encoded = value.as_str().ok_or_else(|| error("invalid retained record chunk"))?;
            if !unique.insert(encoded) { return Err(error("duplicate retained record chunk")); }
            let chunk = if let Some(chunk) = cached.get(encoded) { (*chunk).clone() } else {
                let bytes = STANDARD.decode(encoded).map_err(error)?;
                if bytes.is_empty() { return Err(error("empty retained record chunk")); }
                Arc::new(Chunk { digest: crate::store::sha256_bytes(&bytes), encoded: encoded.into(), bytes })
            };
            chunks.push(chunk);
        }
        Ok(Self { used: vec![false; chunks.len()], chunks, atoms: Default::default() })
    }

    pub(super) fn finish(self) -> serde_json::Result<Vec<Arc<Chunk>>> {
        if self.used.contains(&false) { return Err(error("unreferenced retained record chunk")); }
        Ok(self.chunks)
    }
}

impl<T: serde::de::DeserializeOwned> RetainedRecords<T> {
    pub(super) fn load<'de>(
        value: &'de Value, kind: &str, cached: &Self, pool: &mut Decoder,
        mut inline: impl FnMut(&'de Value) -> serde_json::Result<T>,
    ) -> serde_json::Result<Self> {
        let entries = value.as_object().ok_or_else(|| error("retained record collection unavailable"))?;
        let mut records = BTreeMap::new();
        for (key, value) in entries {
            if value.get("retained_record").is_none() {
                records.insert(key.clone(), Arc::new(Record { value: inline(value)?, stored: OnceLock::new() }));
                continue;
            }
            let stored = Stored::deserialize(value)?;
            if stored.retained_record != FORMAT || stored.kind != kind {
                return Err(error("unsupported retained record encoding or kind"));
            }
            let mut parts = Vec::new();
            let mut length = 0usize;
            for index in stored.parts {
                let chunk = pool.chunks.get(index).ok_or_else(|| error("retained record chunk absent"))?;
                pool.used[index] = true;
                length = length.checked_add(chunk.bytes.len()).ok_or_else(|| error("retained record length overflow"))?;
                parts.push(chunk.clone());
            }
            // The untrusted declared length is an integrity claim, never an
            // allocation request. Allocate only for the actual verified parts.
            if length != stored.json_bytes { return Err(error("retained record length mismatch")); }
            if let Some(prior) = cached.records.get(key).filter(|prior| prior.stored.get().is_some_and(|old|
                old.kind == kind && old.json_bytes == length && old.json_sha256 == stored.json_sha256
                && old.chunks.len() == parts.len() && old.chunks.iter().zip(&parts).all(|(a, b)|
                    Arc::ptr_eq(a, b) || a.encoded == b.encoded)))
            {
                records.insert(key.clone(), prior.clone());
                continue;
            }
            let mut bytes = Vec::with_capacity(length);
            for part in &parts { bytes.extend_from_slice(&part.bytes); }
            if crate::store::sha256_bytes(&bytes) != stored.json_sha256 {
                return Err(error("retained record digest mismatch"));
            }
            let value = pool.atoms.decode(&bytes)?;
            let encoding = OnceLock::new();
            let _ = encoding.set(Encoding { kind: kind.into(), json_bytes: length,
                json_sha256: stored.json_sha256, chunks: parts });
            records.insert(key.clone(), Arc::new(Record { value, stored: encoding }));
        }
        Ok(Self { records })
    }
}
