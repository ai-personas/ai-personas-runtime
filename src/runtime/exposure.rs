//! Shared ownership of complete source-lineage manifests within runtime state.
//! Cloning a snapshot retains the same immutable manifest. A subsequent edit
//! copies only that manifest, preserving the original decision and state view.
use super::{f, ScopedPolicy};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

struct ReadValidation {
    state: usize,
    checked: std::collections::BTreeSet<f::Reference>,
}
thread_local! {
    static READ_VALIDATION: std::cell::RefCell<Option<ReadValidation>> = const { std::cell::RefCell::new(None) };
}

/// Memoize successful ancestry checks only during one immutable projection.
/// The address identifies the borrowed State; it is never dereferenced or kept
/// after the scope. Nested projections restore the enclosing scope on return.
pub(crate) fn with_read_validation<T>(state: &super::State, read: impl FnOnce() -> T) -> T {
    struct Restore(Option<ReadValidation>);
    impl Drop for Restore {
        fn drop(&mut self) { READ_VALIDATION.with(|slot| *slot.borrow_mut() = self.0.take()); }
    }
    let previous = READ_VALIDATION.with(|slot| slot.replace(Some(ReadValidation {
        state: state as *const super::State as usize, checked: Default::default(),
    })));
    let _restore = Restore(previous);
    read()
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

/// Complete exact lineage with copy-on-write ownership. Serialization retains
/// the ordinary sources/policies object; sharing creates no new wire references.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Exposure(Arc<ExposureManifest>);

impl Exposure {
    pub fn new(sources: Vec<f::Reference>, policies: Vec<ScopedPolicy>) -> Self {
        Self(Arc::new(ExposureManifest { sources, policies }))
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
