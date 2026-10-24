//! Canonical transaction fingerprints borrow complete requests and object bodies.
use super::object_encoding::{fingerprint_serialized, CanonicalObject, CanonicalValue};
use super::{Change, Decision, Intent, InvocationSpec, ObjectRef, Resources, Result, TerminalEvidenceReservation};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

struct Canonical<'a, T: ?Sized>(&'a T);

impl<T> Serialize for Canonical<'_, [T]>
where
    for<'a> Canonical<'a, T>: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&Canonical(value))?;
        }
        sequence.end()
    }
}

// Exhaustive destructuring requires a field addition to update its canonical
// representation. Maps emit every field in lexical key order; sequences retain
// their original order and optional fields retain their explicit null values.
impl Serialize for Canonical<'_, ObjectRef> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let ObjectRef { id, revision, sha256 } = self.0;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("id", id)?;
        map.serialize_entry("revision", revision)?;
        map.serialize_entry("sha256", sha256)?;
        map.end()
    }
}

impl Serialize for Canonical<'_, Resources> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let Resources { attempts, input_bytes, output_bytes, navigation_bytes, retained_bytes } = self.0;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("attempts", attempts)?;
        map.serialize_entry("input_bytes", input_bytes)?;
        map.serialize_entry("navigation_bytes", navigation_bytes)?;
        map.serialize_entry("output_bytes", output_bytes)?;
        map.serialize_entry("retained_bytes", retained_bytes)?;
        map.end()
    }
}

impl Serialize for Canonical<'_, Change> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let Change { expected_revision, object, depends_on } = self.0;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("depends_on", &Canonical(depends_on.as_slice()))?;
        map.serialize_entry("expected_revision", expected_revision)?;
        map.serialize_entry("object", &CanonicalObject(object))?;
        map.end()
    }
}

impl Serialize for Canonical<'_, InvocationSpec> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let InvocationSpec { id, expected_epoch, request, reservation, class } = self.0;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("class", class)?;
        map.serialize_entry("expected_epoch", expected_epoch)?;
        map.serialize_entry("id", id)?;
        map.serialize_entry("request", &CanonicalValue(request))?;
        map.serialize_entry("reservation", &Canonical(reservation))?;
        map.end()
    }
}

impl Serialize for Canonical<'_, TerminalEvidenceReservation> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let TerminalEvidenceReservation { id, max_object_bytes, class } = self.0;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("class", class)?;
        map.serialize_entry("id", id)?;
        map.serialize_entry("max_object_bytes", max_object_bytes)?;
        map.end()
    }
}

impl Serialize for Canonical<'_, Intent> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let Intent { id, body, depends_on, reservation, class } = self.0;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("body", &CanonicalValue(body))?;
        map.serialize_entry("class", class)?;
        map.serialize_entry("depends_on", &Canonical(depends_on.as_slice()))?;
        map.serialize_entry("id", id)?;
        map.serialize_entry("reservation", &Canonical(reservation))?;
        map.end()
    }
}

impl Serialize for Canonical<'_, Decision> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let Decision { expected_epoch, invocation_id, response_hash, changes, intents } = self.0;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("changes", &Canonical(changes.as_slice()))?;
        map.serialize_entry("expected_epoch", expected_epoch)?;
        map.serialize_entry("intents", &Canonical(intents.as_slice()))?;
        map.serialize_entry("invocation_id", invocation_id)?;
        map.serialize_entry("response_hash", response_hash)?;
        map.end()
    }
}

pub(super) fn invocation(spec: &InvocationSpec) -> Result<String> {
    fingerprint_serialized(&Canonical(spec))
}

pub(super) fn atomic_admission(
    expected_start_epoch: u64,
    changes: &[Change],
    spec: &InvocationSpec,
    terminal_slots: &[TerminalEvidenceReservation],
) -> Result<String> {
    fingerprint_serialized(&(
        "atomic-invocation-admission-v1",
        expected_start_epoch,
        Canonical(changes),
        Canonical(spec),
        Canonical(terminal_slots),
    ))
}

pub(super) fn decision(decision: &Decision) -> Result<String> {
    fingerprint_serialized(&Canonical(decision))
}
