//! Immutable in-memory file discovery, qualified reads and context assembly without quotas.
//!
//! This module performs no filesystem, network, inference, persistence, or selection
//! mutation. Callers must supply an authoritative current PolicySnapshot and a fresh
//! Admission. A digest establishes integrity, not authorship or authorization.
//!
//! Discovery covers explicitly scoped retained revisions, never an inferred latest
//! revision. Ordering is lexical (source, revision). Hidden paths are explicit;
//! there is no ignore-file mechanism. Literal matching is case-sensitive UTF-8.
//! Regex matching uses regex 1 with its normal Unicode and inline-flag semantics.
//! Matches never cross files or decoded JSON fields. One candidate (the first
//! match) is returned per revision. Discovery candidates are not usable guidance.
//!
//! Only rendered_context is the counted context transport for Assembly/ReadBundle.
//! Metadata, source roles, exact restrictions, and qualification edges are included
//! in that rendering. The caller must account for everything outside it through
//! fixed_overhead_bytes, and must recheck actual provider expansion separately.

use regex::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Write};
use thiserror::Error;

pub const FILE_HASH_PROFILE: &str = "persona-file-record-sha256-v1";
pub const JSON_PROFILE: &str = "utf8-unique-decoded-keys-scalar-strings-v1";
pub const CONTEXT_PROFILE: &str = "qualified-exact-context-json-v1";
/// A conservative bound for discovery's fixed JSON control envelope. Candidate
/// objects and separators are charged additionally. Host cursor handles are not
/// part of that envelope and must be accounted for by the integrating transport.
pub const DISCOVERY_CONTROL_BYTES: usize = 1024;

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Fragments,
    Assets,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Representation {
    Utf8,
    /// One string-valued RFC 6901 pointer, decoded exactly once. Ranges refer to
    /// bytes of that decoded string, not bytes of the JSON source.
    JsonString {
        pointer: String,
    },
    /// Explicit extension point; never treated as raw UTF-8.
    Unsupported {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub source: String,
    pub revision: String,
    pub representation: Representation,
    #[serde(deserialize_with = "required_option")]
    pub start: Option<usize>,
    #[serde(deserialize_with = "required_option")]
    pub end: Option<usize>,
}

impl Reference {
    pub fn full(
        source: impl Into<String>,
        revision: impl Into<String>,
        representation: Representation,
    ) -> Self {
        Self {
            source: source.into(),
            revision: revision.into(),
            representation,
            start: None,
            end: None,
        }
    }

    fn request_bytes(&self, historical_limit: usize) -> Result<usize, FilesError> {
        serialized_len(self, historical_limit).map_err(|_| FilesError::ArithmeticOverflow)
    }

    fn structurally_valid(&self) -> bool {
        valid_identifier(&self.source)
            && valid_digest(&self.revision)
            && match (self.start, self.end) {
                (None, None) => true,
                (Some(start), Some(end)) => start < end,
                _ => false,
            }
            && match &self.representation {
                Representation::Utf8 => true,
                Representation::JsonString { pointer } => decode_pointer(pointer).is_ok(),
                Representation::Unsupported { name } => valid_identifier(name),
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivativeUse {
    /// Exact observations/discovery are allowed; assembly for a new generative
    /// decision is not. This module does not authorize retention of derivatives.
    ExactOnly,
    AllowDerivatives,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyIdentity {
    pub id: String,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub id: String,
    pub revision: String,
    pub readers: BTreeSet<String>,
    pub processing_destinations: BTreeSet<String>,
    /// Every intended recipient must occur in this exact allowed-audience set.
    pub audience: BTreeSet<String>,
    pub derivative_use: DerivativeUse,
    /// Explicit null means no expiry. Missing JSON fields are rejected.
    #[serde(deserialize_with = "required_option")]
    pub expires_at_unix: Option<i64>,
    /// Revocation becomes effective at this instant, inclusively.
    #[serde(deserialize_with = "required_option")]
    pub revoked_at_unix: Option<i64>,
}

impl Policy {
    pub fn identity(&self) -> PolicyIdentity {
        PolicyIdentity {
            id: self.id.clone(),
            revision: self.revision.clone(),
        }
    }

    fn valid(&self) -> bool {
        valid_identifier(&self.id)
            && valid_identifier(&self.revision)
            && self.readers.iter().all(|s| valid_identifier(s))
            && self
                .processing_destinations
                .iter()
                .all(|s| valid_identifier(s))
            && self.audience.iter().all(|s| valid_identifier(s))
    }
}

/// Trusted input from the host's current policy authority. At most one current
/// revision per policy id is permitted. A file bound to an older policy revision
/// becomes unavailable; policy revisions are never silently substituted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySnapshot {
    pub active_policies: Vec<Policy>,
    /// Immediate revocations of exact identities, independent of immutable files.
    pub revoked_policies: BTreeSet<PolicyIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub principal: String,
    pub destination: String,
    pub audience: BTreeSet<String>,
    pub now_unix: i64,
}

impl Admission {
    fn validate(&self, byte_limit: usize) -> Result<usize, FilesError> {
        let bytes = serialized_len(self, byte_limit)
            .map_err(|_| FilesError::RequestLimit("admission bytes"))?;
        if !valid_identifier(&self.principal)
            || !valid_identifier(&self.destination)
            || self.audience.is_empty()
            || !self.audience.iter().all(|s| valid_identifier(s))
        {
            return Err(FilesError::InvalidAdmission);
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRevision {
    pub source: String,
    pub revision: String,
    pub path: String,
    pub scope: Scope,
    pub bytes: Vec<u8>,
    pub author: String,
    pub policy: Policy,
    pub qualifications: Vec<Reference>,
    pub retired: bool,
}

impl FileRevision {
    /// SHA-256 of compact serde_json serialization of this ordered tuple:
    /// (profile, source, path, scope, bytes, author, policy, qualifications, retired).
    /// Sets have BTreeSet order; qualification order is preserved. The only omitted
    /// record field is revision itself. All metadata therefore changes identity.
    pub fn computed_revision(&self) -> Result<String, FilesError> {
        hash_json(&(
            FILE_HASH_PROFILE,
            &self.source,
            &self.path,
            self.scope,
            &self.bytes,
            &self.author,
            &self.policy,
            &self.qualifications,
            self.retired,
        ))
    }

    /// Convenience for producing a candidate record. Sealing is not acceptance,
    /// authentication, or a grant; Corpus construction still validates it.
    pub fn seal(mut self) -> Result<Self, FilesError> {
        self.revision = self.computed_revision()?;
        Ok(self)
    }

    pub fn reference(&self, representation: Representation) -> Reference {
        Reference::full(&self.source, &self.revision, representation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonLimits {
    pub max_source_bytes: usize,
    /// Historical depth setting; parsing does not enforce a configured depth.
    pub max_depth: usize,
    pub max_nodes: usize,
    /// Sum of decoded UTF-8 bytes in string values AND object member names.
    pub max_decoded_bytes: usize,
}


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusLimits {
    /// Also bounds constructor input records, including duplicate submissions.
    pub max_revisions: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    /// Also caps each external Reference and Admission before parsing or cloning.
    pub max_record_bytes: usize,
    /// Logical compact JSON bytes of unique complete records plus policy snapshot.
    /// Also caps summed Admission/root Reference bytes in one assembly request.
    /// This is not a claim about allocator overhead or external retained copies.
    pub max_total_record_bytes: usize,
    pub max_policy_entries: usize,
    pub max_qualifications_per_revision: usize,
    pub max_selection_references: usize,
    /// Historical closure estimates. Current traversal expands the complete
    /// graph and detects repeated references without a resource quota.
    pub max_closure_references: usize,
    pub max_closure_edges: usize,
    pub max_closure_bytes: usize,
    pub max_pattern_bytes: usize,
    pub max_regex_size_bytes: usize,
    pub json: JsonLimits,
}

impl CorpusLimits {
    fn closure_quota(&self, _roots: usize) -> Self {
        self.clone()
    }

    fn validate(&self) -> Result<(), FilesError> {
        // Saved numeric settings are historical metadata, not admission gates.
        Ok(())
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WithholdReason {
    /// Deliberately does not distinguish absent, denied, revoked, expired, retired,
    /// stale policy identity, or inaccessible/incomplete qualification closure.
    /// No dependency identifier or private failure dimension is disclosed.
    Unavailable,
    InvalidReference,
    InvalidRange,
    InvalidUtf8,
    InvalidJson,
    RepresentationLimit,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureDimension {
    References,
    Edges,
    Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FilesError {
    #[error("invalid structural limits")]
    InvalidLimits,
    #[error("admission requires an explicit principal, destination, and nonempty audience")]
    InvalidAdmission,
    #[error("invalid policy snapshot")]
    InvalidPolicySnapshot,
    #[error("one policy identity has conflicting immutable content")]
    PolicyIdentityConflict,
    #[error("one file identity has conflicting immutable content")]
    IdentityConflict,
    #[error("record {index} is invalid: {reason}")]
    InvalidRecord { index: usize, reason: &'static str },
    #[error("record {index} does not match its SHA-256 revision")]
    DigestMismatch { index: usize },
    #[error("corpus bound exceeded: {0}")]
    CorpusLimit(&'static str),
    #[error("request bound exceeded: {0}")]
    RequestLimit(&'static str),
    #[error("one file identity has conflicting accepted source formats")]
    SourceFormatConflict,
    #[error("invalid discovery request")]
    InvalidQuery,
    #[error("invalid or over-limit regular expression")]
    InvalidPattern,
    #[error("unsupported representation or pattern dialect")]
    Unsupported,
    #[error("pagination identity is stale")]
    StalePage,
    #[error("navigation allowance exhausted")]
    NavigationExhausted,
    #[error("qualification closure bound exceeded: {0:?}")]
    ClosureLimit(ClosureDimension),
    #[error("too many requested context roots")]
    SelectionLimit,
    #[error("mandatory bundle {index} unavailable: {reason:?}")]
    MandatoryUnavailable {
        index: usize,
        reason: WithholdReason,
    },
    #[error("selected bundle has the wrong scope: {0:?}")]
    SelectedKindMismatch(BundleId),
    #[error("mandatory context requires {required} bytes, limit {limit}")]
    MandatoryOverflow { required: usize, limit: usize },
    #[error("complete selected context requires {required} bytes, limit {limit}")]
    SelectedOverflow { required: usize, limit: usize },
    #[error("exact qualified read unavailable: {0:?}")]
    ReadUnavailable(WithholdReason),
    #[error("qualified read requires {required} bytes, limit {limit}")]
    ReadOverflow { required: usize, limit: usize },
    #[error("size arithmetic overflow")]
    ArithmeticOverflow,
    #[error("serialization failed")]
    Serialization,
}

/// A separately accepted restriction or accompaniment on immutable file bytes.
/// The host authenticates adoption, current authority and snapshot freshness.
/// Replacing this snapshot is not permission to remove an unresolved warning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedBinding {
    pub id: String,
    pub revision: String,
    pub source: String,
    pub file_revision: String,
    pub policy: Policy,
    pub disposition: BindingDisposition,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BindingDisposition {
    /// Applies to every representation/range of the exact subject revision.
    Qualify { required: Vec<Reference> },
    /// Exact historical reads remain possible; generative use is ineligible.
    Retired { reason: String },
    /// A safe disposition when a required warning or its basis is unavailable.
    Ineligible { reason: String },
    /// Authoritatively adopted format, independent of path, punctuation and digest.
    /// The host authenticates adoption and any subsequent replacement/removal.
    SourceFormat { format: SourceFormat },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceFormat {
    /// Also the legacy interpretation when no format binding exists.
    PlainText,
    /// Only JSON_PROFILE is supported. Generative use validates the whole source,
    /// including when the requested representation is raw UTF-8 or a byte range.
    Json { profile: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BindingIdentity {
    pub id: String,
    pub revision: String,
}

/// Full restrictions inherit the containing entry's root/qualification roles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "source_role", rename_all = "snake_case")]
pub enum ContextRestriction {
    AcceptedBinding { binding: AcceptedBinding },
}
impl AcceptedBinding {
    pub fn computed_revision(&self) -> Result<String, FilesError> {
        hash_json(&(
            "accepted-file-binding-v1",
            &self.id,
            &self.source,
            &self.file_revision,
            &self.policy,
            &self.disposition,
        ))
    }
    pub fn seal(mut self) -> Result<Self, FilesError> {
        self.revision = self.computed_revision()?;
        Ok(self)
    }
}

struct StoredRevision {
    record: FileRevision,
    encoded_bytes: usize,
}

struct PolicyView {
    current: BTreeMap<String, Policy>,
    revoked: BTreeSet<PolicyIdentity>,
    encoded_bytes: usize,
}

/// No mutable record access or unfiltered inventory is exposed.
pub struct Corpus {
    records: BTreeMap<(String, String), StoredRevision>,
    policy: PolicyView,
    limits: CorpusLimits,
    bindings: BTreeMap<(String, String), Vec<AcceptedBinding>>,
    binding_bytes: usize,
}

impl Corpus {
    pub fn from_revisions<I>(
        revisions: I,
        policies: PolicySnapshot,
        limits: CorpusLimits,
    ) -> Result<Self, FilesError>
    where
        I: IntoIterator<Item = FileRevision>,
    {
        limits.validate()?;
        let policy = build_policy_view(policies, &limits)?;
        let mut known_policies: BTreeMap<PolicyIdentity, Policy> = policy
            .current
            .values()
            .map(|p| (p.identity(), p.clone()))
            .collect();
        let mut records: BTreeMap<(String, String), StoredRevision> = BTreeMap::new();
        let mut total_payload = 0usize;
        let mut total_encoded = policy.encoded_bytes;
        for (index, record) in revisions.into_iter().enumerate() {
            let encoded_bytes = serialized_len(&record, limits.max_record_bytes)
                .map_err(|_| FilesError::CorpusLimit("record bytes"))?;
            let key = (record.source.clone(), record.revision.clone());
            if let Some(existing) = records.get(&key) {
                if existing.record != record {
                    return Err(FilesError::IdentityConflict);
                }
                continue;
            }
            if !valid_identifier(&record.source)
                || !valid_identifier(&record.author)
                || !valid_digest(&record.revision)
                || !record.policy.valid()
            {
                return Err(FilesError::InvalidRecord {
                    index,
                    reason: "identity or policy",
                });
            }
            if !canonical_relative_path(&record.path) {
                return Err(FilesError::InvalidRecord {
                    index,
                    reason: "noncanonical relative path",
                });
            }
            if !record
                .qualifications
                .iter()
                .all(Reference::structurally_valid)
            {
                return Err(FilesError::InvalidRecord {
                    index,
                    reason: "qualification reference",
                });
            }
            total_payload = checked_add(total_payload, record.bytes.len())?;
            total_encoded = checked_add(total_encoded, encoded_bytes)?;
            register_policy(&mut known_policies, &record.policy)?;
            if record.computed_revision()? != record.revision {
                return Err(FilesError::DigestMismatch { index });
            }
            records.insert(
                key,
                StoredRevision {
                    record,
                    encoded_bytes,
                },
            );
        }
        Ok(Self {
            records,
            policy,
            limits,
            bindings: BTreeMap::new(),
            binding_bytes: 0,
        })
    }

    fn material_bytes(&self) -> Result<usize, FilesError> {
        self.records.values().try_fold(
            checked_add(self.policy.encoded_bytes, self.binding_bytes)?,
            |total, stored| checked_add(total, stored.encoded_bytes),
        )
    }

    pub fn limits(&self) -> &CorpusLimits {
        &self.limits
    }

    /// Bounded metadata lookup, not a body read or permission grant. The caller
    /// checks complete source ancestry first; assemble still checks and expands
    /// every returned qualification. Use this corpus's binding snapshot so
    /// derived tool-result qualifications are not lost.
    pub(crate) fn required_qualifications(&self, sources: &[Reference]) -> Option<Vec<Reference>> {
        let mut required = BTreeSet::new();
        for source in sources {
            if !source.structurally_valid() {
                return None;
            }
            let stored = self.records.get(&(source.source.clone(), source.revision.clone()))?;
            let groups = std::iter::once(stored.record.qualifications.as_slice()).chain(
                self.bindings_for(&stored.record).iter().filter_map(|binding| {
                    match &binding.disposition {
                        BindingDisposition::Qualify { required } => Some(required.as_slice()),
                        _ => None,
                    }
                }),
            );
            for group in groups {
                required.extend(group.iter().cloned());
            }
        }
        Some(required.into_iter().collect())
    }

    /// Replace only the authoritative access view. Immutable file bytes, policies,
    /// identities, and qualifications remain unchanged. The host authenticates
    /// this replacement and prevents rollback to an obsolete authority snapshot.
    pub fn with_policy_snapshot(mut self, snapshot: PolicySnapshot) -> Result<Self, FilesError> {
        let next = build_policy_view(snapshot, &self.limits)?;
        let mut known: BTreeMap<PolicyIdentity, Policy> = self
            .policy
            .current
            .values()
            .map(|p| (p.identity(), p.clone()))
            .collect();
        for stored in self.records.values() {
            register_policy(&mut known, &stored.record.policy)?;
        }
        for binding in self.bindings.values().flatten() {
            register_policy(&mut known, &binding.policy)?;
        }
        for policy in next.current.values() {
            register_policy(&mut known, policy)?;
        }
        let mut total = checked_add(next.encoded_bytes, self.binding_bytes)?;
        for stored in self.records.values() {
            total = checked_add(total, stored.encoded_bytes)?;
        }
        self.policy = next;
        Ok(self)
    }

    /// Install a complete trusted accepted-binding snapshot. File bytes and their
    /// digests remain unchanged, so separately adopted warnings can form cycles.
    /// The caller must persist/authenticate every adoption or justified resolution
    /// and fence stale snapshots. This method supplies no authoring authority.
    pub fn with_bindings(mut self, bindings: Vec<AcceptedBinding>) -> Result<Self, FilesError> {
        let mut by_subject: BTreeMap<(String, String), Vec<AcceptedBinding>> = BTreeMap::new();
        let mut identities = BTreeSet::new();
        let mut bytes = 0;
        let mut known: BTreeMap<PolicyIdentity, Policy> = self
            .policy
            .current
            .values()
            .map(|p| (p.identity(), p.clone()))
            .collect();
        for stored in self.records.values() {
            register_policy(&mut known, &stored.record.policy)?;
        }
        for binding in self.bindings.values().flatten() {
            register_policy(&mut known, &binding.policy)?;
        }
        for (index, binding) in bindings.into_iter().enumerate() {
            let encoded = serialized_len(&binding, self.limits.max_record_bytes)
                .map_err(|_| FilesError::CorpusLimit("binding record bytes"))?;
            if !valid_identifier(&binding.id)
                || !valid_identifier(&binding.source)
                || !valid_digest(&binding.file_revision)
                || !binding.policy.valid()
                || binding.computed_revision()? != binding.revision
                || !identities.insert(binding.id.clone())
            {
                return Err(FilesError::InvalidRecord {
                    index,
                    reason: "invalid or conflicting accepted binding",
                });
            }
            match &binding.disposition {
                BindingDisposition::Qualify { required } => {
                    if required.is_empty()
                        || !required.iter().all(Reference::structurally_valid)
                    {
                        return Err(FilesError::InvalidRecord {
                            index,
                            reason: "invalid required accompaniment",
                        });
                    }
                }
                BindingDisposition::SourceFormat { format } => {
                    if matches!(format, SourceFormat::Json { profile } if profile.as_str() != JSON_PROFILE)
                    {
                        return Err(FilesError::InvalidRecord {
                            index,
                            reason: "unsupported source format profile",
                        });
                    }
                }
                BindingDisposition::Retired { reason }
                | BindingDisposition::Ineligible { reason } => {
                    if reason.trim().is_empty() {
                        return Err(FilesError::InvalidRecord {
                            index,
                            reason: "invalid disposition reason",
                        });
                    }
                }
            }
            register_policy(&mut known, &binding.policy)?;
            bytes = checked_add(bytes, encoded)?;
            by_subject
                .entry((binding.source.clone(), binding.file_revision.clone()))
                .or_default()
                .push(binding);
        }
        let mut total = checked_add(self.policy.encoded_bytes, bytes)?;
        for stored in self.records.values() {
            total = checked_add(total, stored.encoded_bytes)?;
        }
        for (key, values) in &by_subject {
            let mut count = self
                .records
                .get(key)
                .map_or(0, |s| s.record.qualifications.len());
            let mut format: Option<&SourceFormat> = None;
            for value in values {
                if let BindingDisposition::Qualify { required } = &value.disposition {
                    count = checked_add(count, required.len())?;
                }
                if let BindingDisposition::SourceFormat { format: next } = &value.disposition {
                    if format.is_some_and(|previous| previous != next) {
                        return Err(FilesError::SourceFormatConflict);
                    }
                    format = Some(next);
                }
            }
        }
        self.bindings = by_subject;
        self.binding_bytes = bytes;
        Ok(self)
    }

    fn bindings_for(&self, record: &FileRevision) -> &[AcceptedBinding] {
        self.bindings
            .get(&(record.source.clone(), record.revision.clone()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    fn retired(&self, record: &FileRevision) -> bool {
        record.retired
            || self
                .bindings_for(record)
                .iter()
                .any(|b| matches!(b.disposition, BindingDisposition::Retired { .. }))
    }
    fn project_for_use(
        &self,
        record: &FileRevision,
        reference: &Reference,
        usage: Usage,
    ) -> Result<String, WithholdReason> {
        // Explicit JSON projections already validate the entire strict source.
        // Raw observations remain diagnostic; accepted JSON used as context must
        // satisfy its profile before any raw range can become usable guidance.
        if matches!(usage, Usage::Context)
            && matches!(&reference.representation, Representation::Utf8)
            && self.bindings_for(record).iter().any(|binding| {
                matches!(
                    &binding.disposition,
                    BindingDisposition::SourceFormat {
                        format: SourceFormat::Json { .. }
                    }
                )
            })
        {
            parse_json_unique(&record.bytes, &self.limits.json).map_err(|error| match error {
                JsonProfileError::LimitExceeded | JsonProfileError::InvalidLimits => {
                    WithholdReason::RepresentationLimit
                }
                JsonProfileError::InvalidJson | JsonProfileError::DuplicateKey => {
                    WithholdReason::InvalidJson
                }
            })?;
        }
        project_text(record, reference, &self.limits.json)
    }

    fn policy_permitted(&self, _policy: &Policy, _admission: &Admission, _usage: Usage) -> bool {
        // Exact policy records describe provenance; they impose no access gate.
        true
    }

    fn permitted(&self, record: &FileRevision, admission: &Admission, usage: Usage) -> bool {
        self.policy_permitted(&record.policy, admission, usage)
            && self.bindings_for(record).iter().all(|b| {
                self.policy_permitted(&b.policy, admission, usage)
                    && !matches!(b.disposition, BindingDisposition::Ineligible { .. })
            })
            && (matches!(usage, Usage::Observation) || !self.retired(record))
    }

    fn authorized<'a>(
        &'a self,
        reference: &Reference,
        admission: &Admission,
        usage: Usage,
    ) -> Result<&'a StoredRevision, ResolveError> {
        if !reference.structurally_valid() {
            return Err(ResolveError::Unavailable(WithholdReason::InvalidReference));
        }
        let key = (reference.source.clone(), reference.revision.clone());
        let stored = self
            .records
            .get(&key)
            .ok_or(ResolveError::Unavailable(WithholdReason::Unavailable))?;
        if !self.permitted(&stored.record, admission, usage) {
            return Err(ResolveError::Unavailable(WithholdReason::Unavailable));
        }
        Ok(stored)
    }

    fn bundle(
        &self,
        root: &Reference,
        admission: &Admission,
        usage: Usage,
        expected: Option<Scope>,
        limits: &CorpusLimits,
    ) -> Result<Bundle, ResolveError> {
        // Public entry points bound root bytes before this validation or cloning.
        if !root.structurally_valid() {
            return Err(ResolveError::Unavailable(WithholdReason::InvalidReference));
        }
        let graph =
            walk_qualifications(root, &mut ClosureCounter::default(), limits, |reference| {
                let stored = self.authorized(reference, admission, usage)?;
                if reference == root && expected.is_some_and(|scope| stored.record.scope != scope) {
                    return Err(ResolveError::KindMismatch);
                }
                let mut required = stored.record.qualifications.clone();
                let mut encoded = stored.encoded_bytes;
                for binding in self.bindings_for(&stored.record) {
                    encoded = encoded
                        .checked_add(
                            serialized_len(binding, self.limits.max_record_bytes)
                                .map_err(|_| ResolveError::Limit)?,
                        )
                        .ok_or(ResolveError::Limit)?;
                    if let BindingDisposition::Qualify {
                        required: additional,
                    } = &binding.disposition
                    {
                        required.extend(additional.clone());
                    }
                }
                Ok((required, encoded))
            })
            .map_err(|error| match error {
                ResolveError::KindMismatch => ResolveError::KindMismatch,
                _ => ResolveError::Unavailable(WithholdReason::Unavailable),
            })?;
        let mut entries = Vec::with_capacity(graph.order.len());
        for reference in graph.order {
            let stored = self
                .authorized(&reference, admission, usage)
                .map_err(|_| ResolveError::Unavailable(WithholdReason::Unavailable))?;
            let text = self
                .project_for_use(&stored.record, &reference, usage)
                .map_err(|reason| {
                    ResolveError::Unavailable(
                        if &reference == root && reason != WithholdReason::RepresentationLimit {
                            reason
                        } else {
                            WithholdReason::Unavailable
                        },
                    )
                })?;
            entries.push(ContextEntry {
                reference,
                path: stored.record.path.clone(),
                scope: stored.record.scope,
                author: stored.record.author.clone(),
                policy: stored.record.policy.clone(),
                retired: self.retired(&stored.record),
                accepted_bindings: self
                    .bindings_for(&stored.record)
                    .iter()
                    .map(|b| BindingIdentity {
                        id: b.id.clone(),
                        revision: b.revision.clone(),
                    })
                    .collect(),
                binding_restrictions: self
                    .bindings_for(&stored.record)
                    .iter()
                    .map(|binding| ContextRestriction::AcceptedBinding {
                        binding: binding.clone(),
                    })
                    .collect(),
                text,
                roles: BTreeSet::new(),
            });
        }
        Ok(Bundle {
            entries,
            edges: graph.edges,
        })
    }

    /// Exact, qualified observation; does not select anything. Retired records may
    /// be observed as labeled history, but cannot enter generative assembly.
    /// A read accounts for the complete corpus material used to resolve it,
    /// including an unsuccessful read. These quantities are not resource quotas.
    pub fn read(
        &self,
        reference: &Reference,
        admission: &Admission,
        navigation: &mut NavigationBudget,
        byte_limit: usize,
    ) -> Result<ReadBundle, FilesError> {
        let admission_bytes = admission.validate(self.limits.max_record_bytes)?;
        let reference_bytes = reference.request_bytes(self.limits.max_record_bytes)?;
        navigation.begin_call()?;
        if !navigation.charge(NavigationUsage {
            scanned_bytes: checked_add(
                self.material_bytes()?,
                checked_add(admission_bytes, reference_bytes)?,
            )?,
            inspected_revisions: self.records.len(),
            ..NavigationUsage::default()
        }) {
            return Err(FilesError::NavigationExhausted);
        }
        let bundle = self
            .bundle(reference, admission, Usage::Observation, None, &self.limits)
            .map_err(|error| match error {
                ResolveError::Unavailable(reason) => FilesError::ReadUnavailable(reason),
                _ => FilesError::ReadUnavailable(WithholdReason::Unavailable),
            })?;
        let mut entries = Vec::new();
        let mut index = BTreeMap::new();
        merge_bundle(
            &mut entries,
            &mut index,
            bundle,
            BundleId {
                kind: BundleKind::Read,
                index: 0,
            },
        );
        let context_bytes = context_length(&entries, &[])?;
        if !navigation.charge(NavigationUsage {
            result_bytes: context_bytes,
            ..NavigationUsage::default()
        }) {
            return Err(FilesError::NavigationExhausted);
        }
        let rendered_context = render_context(&entries, &[])?;
        let manifest_sha256 = hash_json(&(
            "qualified-read-manifest-v1",
            admission,
            reference,
            byte_limit,
            &rendered_context,
        ))?;
        Ok(ReadBundle {
            entries,
            context_bytes,
            rendered_context,
            manifest_sha256,
        })
    }
}

fn build_policy_view(
    snapshot: PolicySnapshot,
    limits: &CorpusLimits,
) -> Result<PolicyView, FilesError> {
    let encoded_bytes = serialized_len(&snapshot, limits.max_total_record_bytes)
        .map_err(|_| FilesError::CorpusLimit("policy bytes"))?;
    let mut current: BTreeMap<String, Policy> = BTreeMap::new();
    for policy in snapshot.active_policies {
        if !policy.valid() {
            return Err(FilesError::InvalidPolicySnapshot);
        }
        if let Some(previous) = current.get(&policy.id) {
            if previous != &policy {
                return Err(if previous.revision == policy.revision {
                    FilesError::PolicyIdentityConflict
                } else {
                    FilesError::InvalidPolicySnapshot
                });
            }
        } else {
            current.insert(policy.id.clone(), policy);
        }
    }
    if snapshot
        .revoked_policies
        .iter()
        .any(|p| !valid_identifier(&p.id) || !valid_identifier(&p.revision))
    {
        return Err(FilesError::InvalidPolicySnapshot);
    }
    Ok(PolicyView {
        current,
        revoked: snapshot.revoked_policies,
        encoded_bytes,
    })
}

fn register_policy(
    known: &mut BTreeMap<PolicyIdentity, Policy>,
    policy: &Policy,
) -> Result<(), FilesError> {
    let identity = policy.identity();
    if let Some(previous) = known.get(&identity) {
        if previous != policy {
            return Err(FilesError::PolicyIdentityConflict);
        }
    } else {
        known.insert(identity, policy.clone());
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn canonical_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[derive(Clone, Copy)]
enum Usage {
    Observation,
    Context,
}

#[derive(Default)]
struct ClosureCounter {
    references: usize,
    edges: usize,
    bytes: usize,
}

#[derive(Debug)]
enum ResolveError {
    Unavailable(WithholdReason),
    Limit,
    KindMismatch,
}

struct QualificationGraph {
    order: Vec<Reference>,
    edges: Vec<(Reference, Reference)>,
}

/// The graph walker is independent of hashing so cycle behavior is directly
/// testable. All production loaders below resolve only admitted exact records.
fn walk_qualifications<F>(
    root: &Reference,
    counter: &mut ClosureCounter,
    limits: &CorpusLimits,
    mut load: F,
) -> Result<QualificationGraph, ResolveError>
where
    F: FnMut(&Reference) -> Result<(Vec<Reference>, usize), ResolveError>,
{
    fn charge(value: &mut usize, amount: usize, _historical_limit: usize) -> Result<(), ResolveError> {
        let next = value.checked_add(amount).ok_or(ResolveError::Limit)?;
        *value = next;
        Ok(())
    }
    charge(&mut counter.references, 1, limits.max_closure_references)?;
    let mut order = vec![root.clone()];
    let mut seen = BTreeSet::from([root.clone()]);
    let mut edges = Vec::new();
    let mut position = 0;
    while position < order.len() {
        let current = order[position].clone();
        position += 1;
        let (qualifications, encoded_bytes) = load(&current)?;
        charge(&mut counter.bytes, encoded_bytes, limits.max_closure_bytes)?;
        for qualification in qualifications {
            charge(&mut counter.edges, 1, limits.max_closure_edges)?;
            edges.push((current.clone(), qualification.clone()));
            if !seen.contains(&qualification) {
                charge(&mut counter.references, 1, limits.max_closure_references)?;
                seen.insert(qualification.clone());
                order.push(qualification);
            }
        }
    }
    Ok(QualificationGraph { order, edges })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleKind {
    Mandatory,
    SelectedFragment,
    SelectedAsset,
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct BundleId {
    pub kind: BundleKind,
    /// Position in the corresponding caller-supplied slice, not a corpus index.
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum EntryRole {
    Root {
        bundle: BundleId,
    },
    Qualification {
        bundle: BundleId,
        required_by: Reference,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextEntry {
    pub reference: Reference,
    pub path: String,
    pub scope: Scope,
    pub author: String,
    pub policy: Policy,
    pub retired: bool,
    pub accepted_bindings: Vec<BindingIdentity>,
    /// Every enforced binding, including exact policies and dispositions. Emitted
    /// only after the entire qualified bundle is admitted and fully resolved.
    pub binding_restrictions: Vec<ContextRestriction>,
    /// Exact UTF-8 text or exact once-decoded JSON string/range. No summaries.
    pub text: String,
    pub roles: BTreeSet<EntryRole>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WithheldBundle {
    /// Safe input-slot identity; no inaccessible root/dependency metadata.
    pub bundle: BundleId,
    pub reason: WithholdReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Assembly {
    pub entries: Vec<ContextEntry>,
    pub withheld: Vec<WithheldBundle>,
    pub rendered_context: String,
    pub context_bytes: usize,
    pub fixed_overhead_bytes: usize,
    pub output_reserve_bytes: usize,
    /// context_bytes + fixed_overhead_bytes + output_reserve_bytes.
    pub total_bytes: usize,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReadBundle {
    pub entries: Vec<ContextEntry>,
    pub rendered_context: String,
    pub context_bytes: usize,
    pub manifest_sha256: String,
}

struct Bundle {
    entries: Vec<ContextEntry>,
    edges: Vec<(Reference, Reference)>,
}

fn merge_bundle(
    entries: &mut Vec<ContextEntry>,
    index: &mut BTreeMap<Reference, usize>,
    bundle: Bundle,
    id: BundleId,
) {
    let mut root = true;
    for mut entry in bundle.entries {
        if root {
            entry.roles.insert(EntryRole::Root { bundle: id });
            root = false;
        }
        if let Some(&position) = index.get(&entry.reference) {
            entries[position].roles.extend(entry.roles);
        } else {
            index.insert(entry.reference.clone(), entries.len());
            entries.push(entry);
        }
    }
    for (parent, child) in bundle.edges {
        // Every graph edge's child was fully resolved before bundle construction.
        if let Some(&position) = index.get(&child) {
            entries[position].roles.insert(EntryRole::Qualification {
                bundle: id,
                required_by: parent,
            });
        }
    }
}

#[derive(Serialize)]
struct ContextView<'a> {
    profile: &'static str,
    entries: &'a [ContextEntry],
    withheld: &'a [WithheldBundle],
}

fn context_length(
    entries: &[ContextEntry],
    withheld: &[WithheldBundle],
) -> Result<usize, FilesError> {
    serialized_len(
        &ContextView {
            profile: CONTEXT_PROFILE,
            entries,
            withheld,
        },
        usize::MAX,
    )
    .map_err(|_| FilesError::Serialization)
}

fn render_context(
    entries: &[ContextEntry],
    withheld: &[WithheldBundle],
) -> Result<String, FilesError> {
    serde_json::to_string(&ContextView {
        profile: CONTEXT_PROFILE,
        entries,
        withheld,
    })
    .map_err(|_| FilesError::Serialization)
}

/// No selection is mutated. An ineligible optional bundle is withheld atomically.
/// Every eligible selected bundle is supplied without a byte ceiling.
/// Qualification traversal is breadth-first in declared reference order; exact
/// references are deduplicated without conflating ranges, representations, source
/// roles, policy restrictions, or distinct incoming qualification conditions.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    corpus: &Corpus,
    mandatory: &[Reference],
    selected_fragments: &[Reference],
    selected_assets: &[Reference],
    admission: &Admission,
    byte_limit: usize,
    fixed_overhead_bytes: usize,
    output_reserve_bytes: usize,
) -> Result<Assembly, FilesError> {
    let mut request_bytes = admission.validate(corpus.limits.max_record_bytes)?;
    let roots = checked_add(
        checked_add(mandatory.len(), selected_fragments.len())?,
        selected_assets.len(),
    )?;
    for reference in mandatory
        .iter()
        .chain(selected_fragments.iter())
        .chain(selected_assets.iter())
    {
        request_bytes = request_bytes
            .checked_add(reference.request_bytes(corpus.limits.max_record_bytes)?)
            .ok_or(FilesError::RequestLimit("assembly request bytes"))?;
    }
    let fixed = checked_add(fixed_overhead_bytes, output_reserve_bytes)?;
    let mut entries = Vec::new();
    let mut index = BTreeMap::new();
    let mut withheld = Vec::new();
    // Expand complete qualification graphs without an application closure quota.
    let closure_limits = corpus.limits.closure_quota(roots);
    for (position, reference) in mandatory.iter().enumerate() {
        let bundle = corpus
            .bundle(reference, admission, Usage::Context, None, &closure_limits)
            .map_err(|error| match error {
                ResolveError::Unavailable(reason) => FilesError::MandatoryUnavailable {
                    index: position,
                    reason,
                },
                _ => FilesError::MandatoryUnavailable {
                    index: position,
                    reason: WithholdReason::Unavailable,
                },
            })?;
        merge_bundle(
            &mut entries,
            &mut index,
            bundle,
            BundleId {
                kind: BundleKind::Mandatory,
                index: position,
            },
        );
    }
    for (references, kind, scope) in [
        (
            selected_fragments,
            BundleKind::SelectedFragment,
            Scope::Fragments,
        ),
        (selected_assets, BundleKind::SelectedAsset, Scope::Assets),
    ] {
        for (position, reference) in references.iter().enumerate() {
            let id = BundleId {
                kind,
                index: position,
            };
            match corpus.bundle(
                reference,
                admission,
                Usage::Context,
                Some(scope),
                &closure_limits,
            ) {
                Ok(bundle) => merge_bundle(&mut entries, &mut index, bundle, id),
                Err(ResolveError::Unavailable(reason)) => {
                    withheld.push(WithheldBundle { bundle: id, reason })
                }
                Err(ResolveError::Limit) => withheld.push(WithheldBundle {
                    bundle: id,
                    reason: WithholdReason::Unavailable,
                }),
                Err(ResolveError::KindMismatch) => {
                    return Err(FilesError::SelectedKindMismatch(id))
                }
            }
        }
    }
    let context_bytes = context_length(&entries, &withheld)?;
    let total_bytes = checked_add(fixed, context_bytes)?;
    let rendered_context = render_context(&entries, &withheld)?;
    let manifest_sha256 = hash_json(&(
        "exact-assembly-manifest-v1",
        admission,
        mandatory,
        selected_fragments,
        selected_assets,
        byte_limit,
        fixed_overhead_bytes,
        output_reserve_bytes,
        &rendered_context,
    ))?;
    Ok(Assembly {
        entries,
        withheld,
        rendered_context,
        context_bytes,
        fixed_overhead_bytes,
        output_reserve_bytes,
        total_bytes,
        manifest_sha256,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HiddenPaths {
    Include,
    Exclude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetirementFilter {
    ActiveOnly,
    RetiredOnly,
    AllRetained,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SearchSurface {
    Path,
    Text { representation: Representation },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegexDialect {
    RustRegex1,
    Unsupported { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SearchMode {
    Listing,
    Literal {
        needle: String,
    },
    Regex {
        pattern: String,
        dialect: RegexDialect,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRequest {
    pub scope: Scope,
    pub surface: SearchSurface,
    pub mode: SearchMode,
    pub hidden_paths: HiddenPaths,
    pub retirement: RetirementFilter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageLimits {
    pub scan_bytes: usize,
    /// Matched candidate evaluations, including ones that could not be returned.
    pub matches: usize,
    pub result_bytes: usize,
    pub results: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigationLimits {
    pub calls: usize,
    pub scanned_bytes: usize,
    pub inspected_revisions: usize,
    pub matches: usize,
    pub result_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct NavigationUsage {
    pub calls: usize,
    pub scanned_bytes: usize,
    pub inspected_revisions: usize,
    pub matches: usize,
    pub result_bytes: usize,
}

/// Host-owned cumulative accounting. Saved allowances do not cap navigation.
/// Result fitting and requested page sizes are handled by each caller.
pub struct NavigationBudget {
    used: NavigationUsage,
}

impl NavigationBudget {
    pub fn new(_historical_limits: NavigationLimits) -> Self {
        Self {
            used: NavigationUsage::default(),
        }
    }

    pub fn used(&self) -> NavigationUsage {
        self.used
    }

    pub fn remaining(&self) -> NavigationLimits {
        NavigationLimits {
            calls: usize::MAX - self.used.calls,
            scanned_bytes: usize::MAX - self.used.scanned_bytes,
            inspected_revisions: usize::MAX - self.used.inspected_revisions,
            matches: usize::MAX - self.used.matches,
            result_bytes: usize::MAX - self.used.result_bytes,
        }
    }

    /// Charge a complete serialized result for an already admitted operation.
    /// The caller's result-fitting limit prevents partial input supply, without
    /// imposing a cumulative navigation quota. Failure records no partial result.
    pub fn charge_serialized_result<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
        byte_limit: usize,
    ) -> bool {
        let limit = byte_limit.min(self.remaining().result_bytes);
        let Ok(bytes) = serialized_len(value, limit) else {
            return false;
        };
        self.charge(NavigationUsage {
            result_bytes: bytes,
            ..NavigationUsage::default()
        })
    }

    pub(crate) fn begin_call(&mut self) -> Result<(), FilesError> {
        if self.charge(NavigationUsage {
            calls: 1,
            ..NavigationUsage::default()
        }) {
            Ok(())
        } else {
            Err(FilesError::NavigationExhausted)
        }
    }

    pub(crate) fn charge(&mut self, cost: NavigationUsage) -> bool {
        // Only a machine counter overflow is unrepresentable. No configured
        // allowance can stop an otherwise valid navigation operation.
        let Some(calls) = self.used.calls.checked_add(cost.calls) else { return false };
        let Some(scanned_bytes) = self.used.scanned_bytes.checked_add(cost.scanned_bytes) else { return false };
        let Some(inspected_revisions) = self.used.inspected_revisions.checked_add(cost.inspected_revisions) else { return false };
        let Some(matches) = self.used.matches.checked_add(cost.matches) else { return false };
        let Some(result_bytes) = self.used.result_bytes.checked_add(cost.result_bytes) else {
            return false;
        };
        self.used = NavigationUsage { calls, scanned_bytes, inspected_revisions, matches, result_bytes };
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiscoveryHit {
    /// Full candidate representation, never an implied selection or full read.
    pub reference: Reference,
    pub path: String,
    pub scope: Scope,
    pub retired: bool,
    /// Path matches have path-byte offsets. Text matches have offsets in the
    /// declared representation. Listing has no matched range.
    pub matched_range: Option<ByteRange>,
    pub surface: SearchSurface,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchStatus {
    Complete,
    Incomplete,
    Exhausted,
    NoMatch,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub enum DiscoveryStop {
    PageScanLimit,
    PageMatchLimit,
    PageResultLimit,
    PageCountLimit,
    Navigation,
    StalePage,
    SurfaceUnavailable(WithholdReason),
}

/// Trusted in-process cursor. Callers cannot deserialize offsets. The runtime
/// retains its crate-private checkpoint behind a persona/work-bound opaque handle.
#[derive(Clone)]
pub struct PageCursor {
    retained: RetainedPageCursor,
}

/// Host state only; never accepted as a caller-supplied cursor or rendered.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetainedPageCursor {
    snapshot: String,
    query: String,
    next_index: usize,
    emitted_before: bool,
    unavailable_before: bool,
}

impl PageCursor {
    pub(crate) fn retain(&self) -> RetainedPageCursor {
        self.retained.clone()
    }
}

impl RetainedPageCursor {
    pub(crate) fn restore(&self) -> PageCursor {
        PageCursor {
            retained: self.clone(),
        }
    }
}

impl fmt::Debug for PageCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PageCursor(<opaque>)")
    }
}

#[derive(Clone, Debug)]
pub struct SearchPage {
    /// Permission-filtered view hash, not a hash of the inaccessible inventory.
    /// None means the bounded authorization/view pass could not be performed.
    pub snapshot: Option<String>,
    pub status: SearchStatus,
    pub stop: Option<DiscoveryStop>,
    pub hits: Vec<DiscoveryHit>,
    pub next: Option<PageCursor>,
    pub charged_result_bytes: usize,
    /// Host-only lineage of the permission-filtered query view, never rendered.
    pub(crate) consulted: Vec<Reference>,
}

impl SearchPage {
    pub fn coverage_complete(&self) -> bool {
        matches!(self.status, SearchStatus::Complete | SearchStatus::NoMatch)
    }

    /// Bounded candidate/control output. Cursor state remains host-only.
    pub fn rendered_json(&self) -> Result<String, FilesError> {
        #[derive(Serialize)]
        struct View<'a> {
            profile: &'static str,
            snapshot: &'a Option<String>,
            status: SearchStatus,
            stop: Option<DiscoveryStop>,
            hits: &'a [DiscoveryHit],
            coverage_complete: bool,
            has_more: bool,
            charged_result_bytes: usize,
        }
        serde_json::to_string(&View {
            profile: "lexical-discovery-v1",
            snapshot: &self.snapshot,
            status: self.status,
            stop: self.stop,
            hits: &self.hits,
            coverage_complete: self.coverage_complete(),
            has_more: self.next.is_some(),
            charged_result_bytes: self.charged_result_bytes,
        })
        .map_err(|_| FilesError::Serialization)
    }
}

pub(crate) enum Matcher<'a> {
    Listing,
    Literal(&'a str),
    Regex(Regex),
}

impl Matcher<'_> {
    pub(crate) fn first_match(&self, text: &str) -> Option<Option<ByteRange>> {
        match self {
            Self::Listing => Some(None),
            Self::Literal(needle) => text.find(needle).map(|start| {
                Some(ByteRange {
                    start,
                    end: start + needle.len(),
                })
            }),
            Self::Regex(regex) => regex.find(text).map(|m| {
                Some(ByteRange {
                    start: m.start(),
                    end: m.end(),
                })
            }),
        }
    }
}

fn compile_matcher<'a>(
    request: &'a DiscoveryRequest,
    limits: &CorpusLimits,
) -> Result<Matcher<'a>, FilesError> {
    match &request.surface {
        SearchSurface::Path => {}
        SearchSurface::Text {
            representation: Representation::Unsupported { .. },
        } => return Err(FilesError::Unsupported),
        SearchSurface::Text {
            representation: Representation::JsonString { pointer },
        } => {
            decode_pointer(pointer).map_err(|_| FilesError::InvalidQuery)?;
        }
        SearchSurface::Text {
            representation: Representation::Utf8,
        } => {}
    }
    if matches!(&request.mode, SearchMode::Listing) && request.surface != SearchSurface::Path {
        return Err(FilesError::InvalidQuery);
    }
    compile_search_mode(&request.mode, limits)
}

/// Shared bounded lexical matcher. Purpose-specific adapters define their own
/// exact search serialization, permissions, identities and continuation state.
pub(crate) fn compile_search_mode<'a>(
    mode: &'a SearchMode,
    _historical_limits: &CorpusLimits,
) -> Result<Matcher<'a>, FilesError> {
    match mode {
        SearchMode::Listing => Ok(Matcher::Listing),
        SearchMode::Literal { needle } => {
            if needle.is_empty() {
                return Err(FilesError::InvalidQuery);
            }
            Ok(Matcher::Literal(needle))
        }
        SearchMode::Regex { pattern, dialect } => {
            if !matches!(dialect, RegexDialect::RustRegex1) {
                return Err(FilesError::Unsupported);
            }
            RegexBuilder::new(pattern)
                .size_limit(usize::MAX)
                .dfa_size_limit(usize::MAX)
                .nest_limit(u32::MAX)
                .build()
                .map(Matcher::Regex)
                .map_err(|_| FilesError::InvalidPattern)
        }
    }
}

impl Corpus {
    /// Every call accounts for the complete retained corpus used to construct
    /// the view. Saved numeric limits do not constrain scans or result bytes.
    /// Subject scans, query bytes, candidate matches, and returned bytes are
    /// additional cumulative charges. No automatic continuation occurs.
    pub fn discover(
        &self,
        request: &DiscoveryRequest,
        admission: &Admission,
        limits: PageLimits,
        navigation: &mut NavigationBudget,
        cursor: Option<&PageCursor>,
    ) -> Result<SearchPage, FilesError> {
        let admission_bytes = admission.validate(self.limits.max_record_bytes)?;
        if limits.results == 0 {
            return Err(FilesError::InvalidQuery);
        }
        navigation.begin_call()?;
        if !navigation.charge(NavigationUsage {
            result_bytes: DISCOVERY_CONTROL_BYTES,
            ..NavigationUsage::default()
        }) {
            return Err(FilesError::NavigationExhausted);
        }
        let mut page = SearchPage {
            snapshot: None,
            status: SearchStatus::Incomplete,
            stop: None,
            hits: Vec::new(),
            next: None,
            charged_result_bytes: DISCOVERY_CONTROL_BYTES,
            consulted: Vec::new(),
        };
        let query_bytes = serialized_len(request, self.limits.max_record_bytes)
            .map_err(|_| FilesError::InvalidQuery)?;
        let scanned = checked_add(
            self.material_bytes()?,
            checked_add(query_bytes, admission_bytes)?,
        )?;
        if !navigation.charge(NavigationUsage {
            scanned_bytes: scanned,
            inspected_revisions: self.records.len(),
            ..NavigationUsage::default()
        }) {
            page.status = SearchStatus::Exhausted;
            page.stop = Some(DiscoveryStop::Navigation);
            return Ok(page);
        }
        let matcher = compile_matcher(request, &self.limits)?;
        let visible: Vec<&StoredRevision> = self
            .records
            .values()
            .filter(|stored| {
                let record = &stored.record;
                // Access is checked before examining metadata for requested scopes.
                self.permitted(record, admission, Usage::Observation)
                    && record.scope == request.scope
                    && match request.hidden_paths {
                        HiddenPaths::Include => true,
                        HiddenPaths::Exclude => {
                            !record.path.split('/').any(|part| part.starts_with('.'))
                        }
                    }
                    && match request.retirement {
                        RetirementFilter::ActiveOnly => !self.retired(record),
                        RetirementFilter::RetiredOnly => self.retired(record),
                        RetirementFilter::AllRetained => true,
                    }
            })
            .collect();
        // Mutable accepted retirement affects advertised metadata even when the
        // retained immutable source identities remain the same.
        let identities: Vec<_> = visible
            .iter()
            .map(|stored| {
                (
                    stored.record.source.as_str(),
                    stored.record.revision.as_str(),
                    self.retired(&stored.record),
                    self.bindings_for(&stored.record),
                )
            })
            .collect();
        let query = hash_json(&("discovery-query-v1", request))?;
        let snapshot = hash_json(&(
            "permitted-discovery-view-v2",
            &admission.principal,
            &admission.destination,
            &admission.audience,
            &query,
            &identities,
        ))?;
        // Even a stale continuation is an observation about this permitted view.
        // Retain its restrictions before validating the old checkpoint; hidden
        // records were excluded above and no file bodies are supplied here.
        page.consulted = visible
            .iter()
            .map(|stored| stored.record.reference(Representation::Utf8))
            .collect();
        let (mut position, emitted_before, mut unavailable_before) = if let Some(cursor) = cursor {
            let cursor = &cursor.retained;
            if cursor.snapshot != snapshot
                || cursor.query != query
                || cursor.next_index > visible.len()
            {
                page.status = SearchStatus::Unavailable;
                page.stop = Some(DiscoveryStop::StalePage);
                return Ok(page);
            }
            (
                cursor.next_index,
                cursor.emitted_before,
                cursor.unavailable_before,
            )
        } else {
            (0, false, false)
        };
        page.snapshot = Some(snapshot.clone());
        while position < visible.len() {
            if page.hits.len() >= limits.results {
                page.stop = Some(DiscoveryStop::PageCountLimit);
                break;
            }
            let stored = visible[position];
            let record = &stored.record;
            let cost = match &request.surface {
                SearchSurface::Path => record.path.len(),
                SearchSurface::Text { .. } => record.bytes.len(),
            };
            if !navigation.charge(NavigationUsage {
                scanned_bytes: cost,
                inspected_revisions: 1,
                ..NavigationUsage::default()
            }) {
                page.status = SearchStatus::Exhausted;
                page.stop = Some(DiscoveryStop::Navigation);
                break;
            }
            let representation = match &request.surface {
                SearchSurface::Path => Representation::Utf8,
                SearchSurface::Text { representation } => representation.clone(),
            };
            let reference = record.reference(representation);
            let text = match &request.surface {
                SearchSurface::Path => Ok(record.path.clone()),
                SearchSurface::Text { .. } => project_text(record, &reference, &self.limits.json),
            };
            let text = match text {
                Ok(text) => text,
                Err(reason) => {
                    page.status = SearchStatus::Unavailable;
                    page.stop = Some(DiscoveryStop::SurfaceUnavailable(reason));
                    // This revision was inspected but its projection is unavailable.
                    // Explicit continuation moves past it without claiming coverage.
                    unavailable_before = true;
                    position += 1;
                    break;
                }
            };
            if let Some(matched_range) = matcher.first_match(&text) {
                if !navigation.charge(NavigationUsage {
                    matches: 1,
                    ..NavigationUsage::default()
                }) {
                    page.status = SearchStatus::Exhausted;
                    page.stop = Some(DiscoveryStop::Navigation);
                    break;
                }
                let hit = DiscoveryHit {
                    reference,
                    path: record.path.clone(),
                    scope: record.scope,
                    retired: self.retired(record),
                    matched_range,
                    surface: request.surface.clone(),
                };
                let bytes = checked_add(
                    serialized_len(&hit, usize::MAX).map_err(|_| FilesError::Serialization)?,
                    1,
                )?;
                if !navigation.charge(NavigationUsage {
                    result_bytes: bytes,
                    ..NavigationUsage::default()
                }) {
                    page.status = SearchStatus::Exhausted;
                    page.stop = Some(DiscoveryStop::Navigation);
                    break;
                }
                page.charged_result_bytes += bytes;
                page.hits.push(hit);
            }
            position += 1;
        }
        if position == visible.len() {
            if unavailable_before {
                page.status = SearchStatus::Unavailable;
                page.stop.get_or_insert(DiscoveryStop::SurfaceUnavailable(
                    WithholdReason::Unavailable,
                ));
            } else {
                page.status = if page.hits.is_empty() && !emitted_before {
                    SearchStatus::NoMatch
                } else {
                    SearchStatus::Complete
                };
                page.stop = None;
            }
        } else {
            page.next = Some(PageCursor {
                retained: RetainedPageCursor {
                    snapshot,
                    query,
                    next_index: position,
                    emitted_before: emitted_before || !page.hits.is_empty(),
                    unavailable_before,
                },
            });
        }
        Ok(page)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum JsonProfileError {
    #[error("invalid JSON profile limits")]
    InvalidLimits,
    #[error("JSON profile resource limit exceeded")]
    LimitExceeded,
    #[error("invalid UTF-8 JSON or non-scalar string escape")]
    InvalidJson,
    #[error("duplicate decoded object member name")]
    DuplicateKey,
}

struct JsonState<'a> {
    _historical_limits: &'a JsonLimits,
    failure: Option<JsonProfileError>,
}

impl JsonState<'_> {
    fn fail<E: de::Error>(&mut self, error: JsonProfileError) -> E {
        self.failure = Some(error);
        E::custom("JSON profile rejected input")
    }
}

struct JsonSeed<'a, 'b> {
    state: &'a mut JsonState<'b>,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for JsonSeed<'_, '_> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct JsonVisitor<'a, 'b> {
    state: &'a mut JsonState<'b>,
    depth: usize,
}

impl<'de> Visitor<'de> for JsonVisitor<'_, '_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON with unique decoded member names")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(JsonSeed {
            state: &mut *self.state,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(self.state.fail(JsonProfileError::DuplicateKey));
            }
            let value = object.next_value_seed(JsonSeed {
                state: &mut *self.state,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

/// Independent strict parser. Checks duplicate member names after JSON decoding,
/// recursively, and rejects invalid UTF-8, unpaired surrogates and trailing data
/// without imposing application byte, nesting, node or string limits.
/// Only string-valued projections are exposed by file operations; numeric values
/// are not offered as an exact lexical representation by this module.
pub fn parse_json_unique(bytes: &[u8], limits: &JsonLimits) -> Result<Value, JsonProfileError> {
    if std::str::from_utf8(bytes).is_err() {
        return Err(JsonProfileError::InvalidJson);
    }
    let mut state = JsonState {
        _historical_limits: limits,
        failure: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let parsed = JsonSeed {
        state: &mut state,
        depth: 1,
    }
    .deserialize(&mut deserializer);
    let value = match parsed {
        Ok(value) => value,
        Err(_) => return Err(state.failure.unwrap_or(JsonProfileError::InvalidJson)),
    };
    deserializer
        .end()
        .map_err(|_| JsonProfileError::InvalidJson)?;
    Ok(value)
}

fn decode_pointer(pointer: &str) -> Result<Vec<String>, ()> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    if !pointer.starts_with('/') {
        return Err(());
    }
    pointer[1..]
        .split('/')
        .map(|token| {
            let mut decoded = String::new();
            let mut chars = token.chars();
            while let Some(ch) = chars.next() {
                if ch == '~' {
                    decoded.push(match chars.next() {
                        Some('0') => '~',
                        Some('1') => '/',
                        _ => return Err(()),
                    });
                } else {
                    decoded.push(ch);
                }
            }
            Ok(decoded)
        })
        .collect()
}

fn resolve_pointer<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, WithholdReason> {
    let tokens = decode_pointer(pointer).map_err(|_| WithholdReason::InvalidReference)?;
    let mut current = value;
    for token in tokens {
        current = match current {
            Value::Object(object) => object.get(&token).ok_or(WithholdReason::Unavailable)?,
            Value::Array(array) => {
                if token.is_empty()
                    || !token.bytes().all(|b| b.is_ascii_digit())
                    || (token.len() > 1 && token.starts_with('0'))
                {
                    return Err(WithholdReason::Unavailable);
                }
                let index = token
                    .parse::<usize>()
                    .map_err(|_| WithholdReason::Unavailable)?;
                array.get(index).ok_or(WithholdReason::Unavailable)?
            }
            _ => return Err(WithholdReason::Unavailable),
        };
    }
    Ok(current)
}

fn project_text(
    record: &FileRevision,
    reference: &Reference,
    limits: &JsonLimits,
) -> Result<String, WithholdReason> {
    let full = match &reference.representation {
        Representation::Utf8 => std::str::from_utf8(&record.bytes)
            .map_err(|_| WithholdReason::InvalidUtf8)?
            .to_owned(),
        Representation::JsonString { pointer } => {
            let value = parse_json_unique(&record.bytes, limits).map_err(|error| match error {
                JsonProfileError::LimitExceeded | JsonProfileError::InvalidLimits => {
                    WithholdReason::RepresentationLimit
                }
                JsonProfileError::InvalidJson | JsonProfileError::DuplicateKey => {
                    WithholdReason::InvalidJson
                }
            })?;
            resolve_pointer(&value, pointer)?
                .as_str()
                .ok_or(WithholdReason::Unsupported)?
                .to_owned()
        }
        Representation::Unsupported { .. } => return Err(WithholdReason::Unsupported),
    };
    match (reference.start, reference.end) {
        (None, None) => Ok(full),
        (Some(start), Some(end))
            if start < end
                && end <= full.len()
                && full.is_char_boundary(start)
                && full.is_char_boundary(end) =>
        {
            Ok(full[start..end].to_owned())
        }
        _ => Err(WithholdReason::InvalidRange),
    }
}

fn checked_add(left: usize, right: usize) -> Result<usize, FilesError> {
    left.checked_add(right)
        .ok_or(FilesError::ArithmeticOverflow)
}

struct LengthWriter {
    written: usize,
}

impl Write for LengthWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .written
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("serialized size overflow"))?;
        self.written = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_len<T: Serialize + ?Sized>(value: &T, _historical_limit: usize) -> Result<usize, ()> {
    let mut writer = LengthWriter { written: 0 };
    serde_json::to_writer(&mut writer, value).map_err(|_| ())?;
    Ok(writer.written)
}

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn hash_json<T: Serialize + ?Sized>(value: &T) -> Result<String, FilesError> {
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(|_| FilesError::Serialization)?;
    Ok(hex::encode(writer.0.finalize()))
}
