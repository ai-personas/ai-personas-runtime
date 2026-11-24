//! Shared ownership of complete source-lineage manifests within runtime state.
//! Cloning a snapshot retains the same immutable manifest. A subsequent edit
//! copies only that manifest, preserving the original decision and state view.
use super::{f, ScopedPolicy};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// Preserve first-seen order while narrowing duplicate checks to records with
/// the same source and revision. Those keys do not establish policy equality:
/// every candidate still compares the complete record. Existing duplicates are
/// retained, matching append-only merging into an already populated manifest.
pub(super) struct PolicyCollector {
    policies: Vec<ScopedPolicy>,
    positions: std::collections::HashMap<String,
        std::collections::HashMap<String, Vec<usize>>>,
}

impl PolicyCollector {
    pub(super) fn new(policies: Vec<ScopedPolicy>) -> Self {
        let mut collector = Self { policies, positions: Default::default() };
        for (position, policy) in collector.policies.iter().enumerate() {
            collector.positions.entry(policy.source.clone()).or_default()
                .entry(policy.revision.clone()).or_default().push(position);
        }
        collector
    }

    pub(super) fn insert(&mut self, policy: &ScopedPolicy) {
        if self.positions.get(&policy.source)
            .and_then(|revisions| revisions.get(&policy.revision))
            .is_some_and(|positions| positions.iter()
                .any(|&position| self.policies[position] == *policy)) {
            return;
        }
        self.positions.entry(policy.source.clone()).or_default()
            .entry(policy.revision.clone()).or_default().push(self.policies.len());
        self.policies.push(policy.clone());
    }

    pub(super) fn extend(&mut self, policies: &[ScopedPolicy]) {
        for policy in policies { self.insert(policy); }
    }

    pub(super) fn into_vec(self) -> Vec<ScopedPolicy> { self.policies }
}

struct ReadValidation {
    state: usize,
    checked: std::collections::BTreeSet<f::Reference>,
    corpus: Option<Arc<f::Corpus>>,
}
thread_local! {
    static READ_VALIDATION: std::cell::RefCell<Option<ReadValidation>> = const { std::cell::RefCell::new(None) };
}

/// Memoize successful ancestry checks and the file catalogue only during one
/// immutable projection. Model-input rendering is also an immutable projection;
/// candidate mutations before or after it cannot share this cache.
/// The address identifies the borrowed State; it is never dereferenced or kept
/// after the scope. Nested projections restore the enclosing scope on return.
pub(crate) fn with_read_validation<T>(state: &super::State, read: impl FnOnce() -> T) -> T {
    if READ_VALIDATION.with(|slot| slot.borrow().as_ref().is_some_and(|cache|
        cache.state == state as *const super::State as usize)) {
        return read();
    }
    struct Restore(Option<ReadValidation>);
    impl Drop for Restore {
        fn drop(&mut self) { READ_VALIDATION.with(|slot| *slot.borrow_mut() = self.0.take()); }
    }
    let previous = READ_VALIDATION.with(|slot| slot.replace(Some(ReadValidation {
        state: state as *const super::State as usize, checked: Default::default(), corpus: None,
    })));
    let _restore = Restore(previous);
    read()
}

/// The catalogue contains immutable records and current state metadata. Each
/// operation still supplies its own admission, selection and navigation ledger.
/// Build outside the RefCell borrow because construction checks source lineage.
pub(super) fn cached_corpus(
    state: &super::State,
    build: impl FnOnce() -> super::Result<f::Corpus>,
) -> super::Result<Arc<f::Corpus>> {
    if let Some(corpus) = READ_VALIDATION.with(|slot| slot.borrow().as_ref()
        .filter(|cache| cache.state == state as *const super::State as usize)
        .and_then(|cache| cache.corpus.clone())) {
        return Ok(corpus);
    }
    let corpus = Arc::new(build()?);
    READ_VALIDATION.with(|slot| {
        if let Some(cache) = slot.borrow_mut().as_mut()
            .filter(|cache| cache.state == state as *const super::State as usize) {
            cache.corpus = Some(corpus.clone());
        }
    });
    Ok(corpus)
}

pub(super) fn check_lineage(state: &super::State, sources: &[f::Reference]) -> super::Result<()> {
    READ_VALIDATION.with(|slot| {
        if let Ok(mut current) = slot.try_borrow_mut() {
            if let Some(cache) = current.as_mut().filter(|cache|
                cache.state == state as *const super::State as usize) {
                return super::trace_exposure_with_validated::<false>(state, sources, &[], Some(&mut cache.checked))
                    .map(|_| ());
            }
        }
        super::trace_exposure::<false>(state, sources, &[]).map(|_| ())
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureManifest {
    pub sources: Vec<f::Reference>,
    pub policies: Vec<ScopedPolicy>,
}

/// Complete exact lineage with copy-on-write ownership. Ordinary serialization
/// retains the sources/policies object. State storage can capture the typed
/// value to build its existing provenance pool without repeatedly expanding it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Exposure(Arc<ExposureManifest>);

impl Exposure {
    pub fn new(sources: Vec<f::Reference>, policies: Vec<ScopedPolicy>) -> Self {
        Self(Arc::new(ExposureManifest { sources, policies }))
    }

    pub(super) fn shared_manifest(&self) -> Arc<ExposureManifest> {
        Arc::clone(&self.0)
    }
}

impl Serialize for Exposure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(reference) = super::state_storage::capture_exposure_reference(self)
            .map_err(serde::ser::Error::custom)?
        {
            serde_json::json!({"retained_exposure": reference}).serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl Deref for Exposure {
    type Target = ExposureManifest;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Exposure {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}
