//! Capture only compiled Exposure values while serializing one State field.
//! The ordinary serializer remains active everywhere outside that scope.
use super::super::{f, identity, Exposure, ScopedPolicy, DOMAIN};
use crate::runtime::{exposure::ExposureManifest, ScopedPolicyValue};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

pub(in crate::runtime::state_storage) struct Encoded {
    pub value: Value,
    pub digest: String,
    bytes: Vec<u8>,
    pub index: Option<usize>,
}

impl Encoded {
    fn new(value: &impl Serialize) -> serde_json::Result<Self> {
        let value = serde_json::to_value(value)?;
        let bytes = serde_json::to_vec(&value)?;
        let digest = identity(&value)?;
        Ok(Self {
            value,
            digest,
            bytes,
            index: None,
        })
    }
}

#[derive(Default)]
pub(in crate::runtime::state_storage) struct Cache {
    sources: HashMap<usize, (Arc<f::ReferenceValue>, Encoded)>,
    policies: HashMap<usize, (Arc<ScopedPolicyValue>, Encoded)>,
    manifests: HashMap<usize, (Arc<ExposureManifest>, String)>,
}

impl Cache {
    pub fn source(&mut self, reference: &f::Reference) -> serde_json::Result<&mut Encoded> {
        let value = reference.shared_value();
        let key = Arc::as_ptr(&value) as usize;
        if !self.sources.contains_key(&key) {
            let encoded = Encoded::new(value.as_ref())?;
            self.sources.insert(key, (value, encoded));
        }
        Ok(&mut self.sources.get_mut(&key).unwrap().1)
    }

    pub fn policy(&mut self, policy: &ScopedPolicy) -> serde_json::Result<&mut Encoded> {
        let value = policy.0.clone();
        let key = Arc::as_ptr(&value) as usize;
        if !self.policies.contains_key(&key) {
            let encoded = Encoded::new(value.as_ref())?;
            self.policies.insert(key, (value, encoded));
        }
        Ok(&mut self.policies.get_mut(&key).unwrap().1)
    }

    fn manifest(&mut self, exposure: &Exposure) -> serde_json::Result<String> {
        let value = exposure.shared_manifest();
        let address = Arc::as_ptr(&value) as usize;
        if let Some((_, identity)) = self.manifests.get(&address) {
            return Ok(identity.clone());
        }
        // Match the complete canonical JSON previously produced from Value:
        // [DOMAIN,{"policies":[...],"sources":[...]}]. Atom bytes are themselves
        // ordinary complete canonical JSON, never hashes or serialized handles.
        let mut hash = Sha256::new();
        hash.update(b"[");
        hash.update(serde_json::to_vec(DOMAIN)?);
        hash.update(b",{\"policies\":[");
        for (i, policy) in value.policies.iter().enumerate() {
            if i != 0 {
                hash.update(b",");
            }
            hash.update(&self.policy(policy)?.bytes);
        }
        hash.update(b"],\"sources\":[");
        for (i, reference) in value.sources.iter().enumerate() {
            if i != 0 {
                hash.update(b",");
            }
            hash.update(&self.source(reference)?.bytes);
        }
        hash.update(b"]}]");
        let identity = format!("{:x}", hash.finalize());
        self.manifests.insert(address, (value, identity.clone()));
        Ok(identity)
    }
}

#[derive(Clone)]
struct Active {
    cache: Rc<RefCell<Cache>>,
    captured: Rc<RefCell<Vec<(String, Exposure)>>>,
}

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

struct Restore(Option<Active>);
impl Drop for Restore {
    fn drop(&mut self) {
        ACTIVE.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

pub(super) fn without<T>(read: impl FnOnce() -> T) -> T {
    let previous = ACTIVE.with(|slot| slot.borrow_mut().take());
    let _restore = Restore(previous);
    read()
}

pub(super) fn value<T: Serialize>(
    cache: &Rc<RefCell<Cache>>,
    value: &T,
) -> serde_json::Result<(Value, Vec<(String, Exposure)>)> {
    let captured = Rc::new(RefCell::new(Vec::new()));
    let previous = ACTIVE.with(|slot| {
        slot.replace(Some(Active {
            cache: cache.clone(),
            captured: captured.clone(),
        }))
    });
    let restore = Restore(previous);
    let result = serde_json::to_value(value);
    drop(restore);
    let captured = std::mem::take(&mut *captured.borrow_mut());
    Ok((result?, captured))
}

pub(in crate::runtime::state_storage) fn reference(
    exposure: &Exposure,
) -> serde_json::Result<Option<String>> {
    let Some(active) = ACTIVE.with(|slot| slot.borrow().clone()) else {
        return Ok(None);
    };
    let identity = active.cache.borrow_mut().manifest(exposure)?;
    active
        .captured
        .borrow_mut()
        .push((identity.clone(), exposure.clone()));
    Ok(Some(identity))
}
