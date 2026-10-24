//! Shared ownership of complete source-lineage manifests within runtime state.
//! Cloning a snapshot retains the same immutable manifest. A subsequent edit
//! copies only that manifest, preserving the original decision and state view.
use super::{f, ScopedPolicy};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

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
