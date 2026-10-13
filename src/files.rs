//! Immutable, bounded, in-memory file discovery, qualified reads, and context assembly.
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

    fn request_bytes(&self, limit: usize) -> Result<usize, FilesError> {
        let representation_bytes = match &self.representation {
            Representation::Utf8 => 0,
            Representation::JsonString { pointer } => pointer.len(),
            Representation::Unsupported { name } => name.len(),
        };
        if self
            .source
            .len()
            .checked_add(self.revision.len())
            .and_then(|bytes| bytes.checked_add(representation_bytes))
            .is_none_or(|bytes| bytes > limit)
        {
            return Err(FilesError::RequestLimit("reference bytes"));
        }
        serialized_len(self, limit).map_err(|_| FilesError::RequestLimit("reference bytes"))
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
        if self.audience.len() > byte_limit {
            return Err(FilesError::RequestLimit("admission bytes"));
        }
        let mut remaining = byte_limit;
        for value in std::iter::once(&self.principal)
            .chain(std::iter::once(&self.destination))
            .chain(self.audience.iter())
        {
            remaining = remaining
                .checked_sub(value.len())
                .ok_or(FilesError::RequestLimit("admission bytes"))?;
        }
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
    /// Between 1 and 64; the root value has depth 1.
    pub max_depth: usize,
    pub max_nodes: usize,
    /// Sum of decoded UTF-8 bytes in string values AND object member names.
    pub max_decoded_bytes: usize,
}

impl JsonLimits {
    fn valid(&self) -> bool {
        (1..=64).contains(&self.max_depth) && self.max_nodes > 0
    }
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
    /// These closure bounds are cumulative across ALL roots in one assembly,
    /// including failed optional bundles and repeated work across bundles.
    /// Each root receives floor(bound / public request root count) independently.
    /// Unused quota is never transferred; incomplete closures remain unavailable.
    pub max_closure_references: usize,
    pub max_closure_edges: usize,
    pub max_closure_bytes: usize,
    pub max_pattern_bytes: usize,
    pub max_regex_size_bytes: usize,
    pub json: JsonLimits,
}

impl CorpusLimits {
    fn closure_quota(&self, roots: usize) -> Self {
        let count = roots.max(1);
        Self {
            max_closure_references: self.max_closure_references / count,
            max_closure_edges: self.max_closure_edges / count,
            max_closure_bytes: self.max_closure_bytes / count,
            ..self.clone()
        }
    }

    fn validate(&self) -> Result<(), FilesError> {
        if !self.json.valid()
            || self.max_record_bytes == 0
            || self.max_total_record_bytes == 0
            || self.max_regex_size_bytes == 0
        {
            return Err(FilesError::InvalidLimits);
        }
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
            if index >= limits.max_revisions {
                return Err(FilesError::CorpusLimit("input revisions"));
            }
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
            if record.bytes.len() > limits.max_file_bytes {
                return Err(FilesError::CorpusLimit("file bytes"));
            }
            if record.qualifications.len() > limits.max_qualifications_per_revision {
                return Err(FilesError::CorpusLimit("direct qualifications"));
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
            if total_payload > limits.max_total_bytes {
                return Err(FilesError::CorpusLimit("total payload bytes"));
            }
            if total_encoded > limits.max_total_record_bytes {
                return Err(FilesError::CorpusLimit("total record bytes"));
            }
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

    pub fn limits(&self) -> &CorpusLimits {
        &self.limits
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
        if total > self.limits.max_total_record_bytes {
            return Err(FilesError::CorpusLimit("total record bytes"));
        }
        self.policy = next;
        Ok(self)
    }

    /// Install a complete trusted accepted-binding snapshot. File bytes and their
    /// digests remain unchanged, so separately adopted warnings can form cycles.
    /// The caller must persist/authenticate every adoption or justified resolution
    /// and fence stale snapshots. This method supplies no authoring authority.
    pub fn with_bindings(mut self, bindings: Vec<AcceptedBinding>) -> Result<Self, FilesError> {
        if bindings.len() > self.limits.max_revisions {
            return Err(FilesError::CorpusLimit("accepted bindings"));
        }
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
                        || required.len() > self.limits.max_qualifications_per_revision
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
                    if reason.trim().is_empty() || reason.len() > self.limits.max_file_bytes {
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
        if total > self.limits.max_total_record_bytes {
            return Err(FilesError::CorpusLimit("total record bytes"));
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
            if count > self.limits.max_qualifications_per_revision {
                return Err(FilesError::CorpusLimit("direct qualifications"));
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

    fn policy_permitted(&self, p: &Policy, admission: &Admission, usage: Usage) -> bool {
        self.policy.current.get(&p.id) == Some(p)
            && !self.policy.revoked.contains(&p.identity())
            && p.readers.contains(&admission.principal)
            && p.processing_destinations.contains(&admission.destination)
            && admission.audience.is_subset(&p.audience)
            && p.expires_at_unix.is_none_or(|t| admission.now_unix < t)
            && p.revoked_at_unix.is_none_or(|t| admission.now_unix < t)
            && (matches!(usage, Usage::Observation)
                || p.derivative_use == DerivativeUse::AllowDerivatives)
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
    /// A read reserves the configured worst-case closure scan/inspection allowance
    /// before resolving any private dependency, including on failure. This avoids
    /// revealing inaccessible dependency counts through public budget accounting.
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
                self.limits.max_closure_bytes,
                checked_add(admission_bytes, reference_bytes)?,
            )?,
            inspected_revisions: self.limits.max_closure_references,
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
        if context_bytes > byte_limit {
            return Err(FilesError::ReadOverflow {
                required: context_bytes,
                limit: byte_limit,
            });
        }
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
    let count = checked_add(
        snapshot.active_policies.len(),
        snapshot.revoked_policies.len(),
    )?;
    if count > limits.max_policy_entries {
        return Err(FilesError::CorpusLimit("policy entries"));
    }
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

fn canonical_relative_path(path: &str) -> bool {
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
    fn charge(value: &mut usize, amount: usize, limit: usize) -> Result<(), ResolveError> {
        let next = value.checked_add(amount).ok_or(ResolveError::Limit)?;
        if next > limit {
            return Err(ResolveError::Limit);
        }
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
/// Any eligible selected material that does not fit causes SelectedOverflow.
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
    if roots > corpus.limits.max_selection_references {
        return Err(FilesError::SelectionLimit);
    }
    if request_bytes > corpus.limits.max_total_record_bytes {
        return Err(FilesError::RequestLimit("assembly request bytes"));
    }
    for reference in mandatory
        .iter()
        .chain(selected_fragments.iter())
        .chain(selected_assets.iter())
    {
        request_bytes = request_bytes
            .checked_add(reference.request_bytes(corpus.limits.max_record_bytes)?)
            .filter(|bytes| *bytes <= corpus.limits.max_total_record_bytes)
            .ok_or(FilesError::RequestLimit("assembly request bytes"))?;
    }
    let fixed = checked_add(fixed_overhead_bytes, output_reserve_bytes)?;
    let mut entries = Vec::new();
    let mut index = BTreeMap::new();
    let mut withheld = Vec::new();
    // Reserve independent shares from public request shape, never private work.
    // Across all roots, each dimension sums to at most its configured bound.
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
    let mandatory_bytes = checked_add(fixed, context_length(&entries, &[])?)?;
    if mandatory_bytes > byte_limit {
        return Err(FilesError::MandatoryOverflow {
            required: mandatory_bytes,
            limit: byte_limit,
        });
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
    if total_bytes > byte_limit {
        return Err(FilesError::SelectedOverflow {
            required: total_bytes,
            limit: byte_limit,
        });
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Host-owned cumulative allowance. Do not construct a new budget to continue the
/// same operation/episode. Public accounting uses conservative authorization and
/// closure charges so it does not disclose private corpus/dependency counts.
pub struct NavigationBudget {
    limits: NavigationLimits,
    used: NavigationUsage,
}

impl NavigationBudget {
    pub fn new(limits: NavigationLimits) -> Self {
        Self {
            limits,
            used: NavigationUsage::default(),
        }
    }

    pub fn used(&self) -> NavigationUsage {
        self.used
    }

    pub fn remaining(&self) -> NavigationLimits {
        NavigationLimits {
            calls: self.limits.calls - self.used.calls,
            scanned_bytes: self.limits.scanned_bytes - self.used.scanned_bytes,
            inspected_revisions: self.limits.inspected_revisions - self.used.inspected_revisions,
            matches: self.limits.matches - self.used.matches,
            result_bytes: self.limits.result_bytes - self.used.result_bytes,
        }
    }

    /// Charge a complete serialized result for an already admitted operation.
    /// Counting stops at the smaller of the supplied limit and remaining result
    /// allowance, without allocating an output copy. Failure debits nothing;
    /// this method neither admits call/scan work nor refunds earlier charges.
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
        fn fits(used: usize, added: usize, limit: usize) -> bool {
            used <= limit && added <= limit - used
        }
        if !fits(self.used.calls, cost.calls, self.limits.calls)
            || !fits(
                self.used.scanned_bytes,
                cost.scanned_bytes,
                self.limits.scanned_bytes,
            )
            || !fits(
                self.used.inspected_revisions,
                cost.inspected_revisions,
                self.limits.inspected_revisions,
            )
            || !fits(self.used.matches, cost.matches, self.limits.matches)
            || !fits(
                self.used.result_bytes,
                cost.result_bytes,
                self.limits.result_bytes,
            )
        {
            return false;
        }
        self.used.calls += cost.calls;
        self.used.scanned_bytes += cost.scanned_bytes;
        self.used.inspected_revisions += cost.inspected_revisions;
        self.used.matches += cost.matches;
        self.used.result_bytes += cost.result_bytes;
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

enum Matcher<'a> {
    Listing,
    Literal(&'a str),
    Regex(Regex),
}

impl Matcher<'_> {
    fn first_match(&self, text: &str) -> Option<Option<ByteRange>> {
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
    match &request.mode {
        SearchMode::Listing => {
            if request.surface != SearchSurface::Path {
                return Err(FilesError::InvalidQuery);
            }
            Ok(Matcher::Listing)
        }
        SearchMode::Literal { needle } => {
            if needle.is_empty() || needle.len() > limits.max_pattern_bytes {
                return Err(FilesError::InvalidQuery);
            }
            Ok(Matcher::Literal(needle))
        }
        SearchMode::Regex { pattern, dialect } => {
            if !matches!(dialect, RegexDialect::RustRegex1) {
                return Err(FilesError::Unsupported);
            }
            if pattern.len() > limits.max_pattern_bytes {
                return Err(FilesError::InvalidPattern);
            }
            RegexBuilder::new(pattern)
                .size_limit(limits.max_regex_size_bytes)
                .dfa_size_limit(limits.max_regex_size_bytes)
                .nest_limit(64)
                .build()
                .map(Matcher::Regex)
                .map_err(|_| FilesError::InvalidPattern)
        }
    }
}

impl Corpus {
    /// Every call conservatively charges max_total_record_bytes scan bytes and
    /// max_revisions inspections for authorization/view construction, regardless
    /// of how many inaccessible records exist. This is deliberately conservative
    /// and avoids uncharged inventory work or private-count budget side channels.
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
        if limits.result_bytes < DISCOVERY_CONTROL_BYTES {
            return Err(FilesError::InvalidLimits);
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
        let mut scanned = checked_add(
            self.limits.max_total_record_bytes,
            checked_add(query_bytes, admission_bytes)?,
        )?;
        if scanned > limits.scan_bytes {
            page.stop = Some(DiscoveryStop::PageScanLimit);
            return Ok(page);
        }
        if !navigation.charge(NavigationUsage {
            scanned_bytes: scanned,
            inspected_revisions: self.limits.max_revisions,
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
        let mut matched = 0usize;
        while position < visible.len() {
            if page.hits.len() >= limits.results {
                page.stop = Some(DiscoveryStop::PageCountLimit);
                break;
            }
            let stored = visible[position];
            let record = &stored.record;
            let cost = match &request.surface {
                SearchSurface::Path => record.path.len(),
                SearchSurface::Text {
                    representation: Representation::JsonString { .. },
                } => checked_add(record.bytes.len(), self.limits.json.max_decoded_bytes)?,
                SearchSurface::Text { .. } => record.bytes.len(),
            };
            if cost > limits.scan_bytes - scanned {
                page.stop = Some(DiscoveryStop::PageScanLimit);
                break;
            }
            if !navigation.charge(NavigationUsage {
                scanned_bytes: cost,
                inspected_revisions: 1,
                ..NavigationUsage::default()
            }) {
                page.status = SearchStatus::Exhausted;
                page.stop = Some(DiscoveryStop::Navigation);
                break;
            }
            scanned += cost;
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
                if matched >= limits.matches {
                    page.stop = Some(DiscoveryStop::PageMatchLimit);
                    break;
                }
                if !navigation.charge(NavigationUsage {
                    matches: 1,
                    ..NavigationUsage::default()
                }) {
                    page.status = SearchStatus::Exhausted;
                    page.stop = Some(DiscoveryStop::Navigation);
                    break;
                }
                matched += 1;
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
                if bytes > limits.result_bytes - page.charged_result_bytes {
                    page.stop = Some(DiscoveryStop::PageResultLimit);
                    break;
                }
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
    limits: &'a JsonLimits,
    nodes: usize,
    decoded_bytes: usize,
    failure: Option<JsonProfileError>,
}

impl JsonState<'_> {
    fn fail<E: de::Error>(&mut self, error: JsonProfileError) -> E {
        self.failure = Some(error);
        E::custom("JSON profile rejected input")
    }

    fn string<E: de::Error>(&mut self, value: &str) -> Result<(), E> {
        match self.decoded_bytes.checked_add(value.len()) {
            Some(next) if next <= self.limits.max_decoded_bytes => {
                self.decoded_bytes = next;
                Ok(())
            }
            _ => Err(self.fail(JsonProfileError::LimitExceeded)),
        }
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
        if self.depth > self.state.limits.max_depth
            || self.state.nodes >= self.state.limits.max_nodes
        {
            return Err(self.state.fail(JsonProfileError::LimitExceeded));
        }
        self.state.nodes += 1;
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
        formatter.write_str("bounded JSON with unique decoded member names")
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
        self.state.string::<E>(value)?;
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        self.state.string::<E>(&value)?;
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
            self.state.string::<A::Error>(&key)?;
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
/// recursively, and rejects invalid UTF-8, unpaired surrogates, trailing data,
/// excessive nesting/nodes, and excessive aggregate decoded string bytes.
/// Only string-valued projections are exposed by file operations; numeric values
/// are not offered as an exact lexical representation by this module.
pub fn parse_json_unique(bytes: &[u8], limits: &JsonLimits) -> Result<Value, JsonProfileError> {
    if !limits.valid() {
        return Err(JsonProfileError::InvalidLimits);
    }
    if bytes.len() > limits.max_source_bytes {
        return Err(JsonProfileError::LimitExceeded);
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err(JsonProfileError::InvalidJson);
    }
    let mut state = JsonState {
        limits,
        nodes: 0,
        decoded_bytes: 0,
        failure: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
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
    limit: usize,
}

impl Write for LengthWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .written
            .checked_add(bytes.len())
            .filter(|next| *next <= self.limit)
            .ok_or_else(|| io::Error::other("serialized size bound"))?;
        self.written = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_len<T: Serialize + ?Sized>(value: &T, limit: usize) -> Result<usize, ()> {
    let mut writer = LengthWriter { written: 0, limit };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn policy() -> Policy {
        Policy {
            id: "p".into(),
            revision: "1".into(),
            readers: set(&["reader"]),
            processing_destinations: set(&["destination"]),
            audience: set(&["recipient"]),
            derivative_use: DerivativeUse::AllowDerivatives,
            expires_at_unix: None,
            revoked_at_unix: None,
        }
    }

    fn admission() -> Admission {
        Admission {
            principal: "reader".into(),
            destination: "destination".into(),
            audience: set(&["recipient"]),
            now_unix: 100,
        }
    }

    fn limits() -> CorpusLimits {
        CorpusLimits {
            max_revisions: 128,
            max_file_bytes: 65_536,
            max_total_bytes: 1_048_576,
            max_record_bytes: 262_144,
            max_total_record_bytes: 2_097_152,
            max_policy_entries: 128,
            max_qualifications_per_revision: 64,
            max_selection_references: 128,
            max_closure_references: 256,
            max_closure_edges: 1024,
            max_closure_bytes: 4_194_304,
            max_pattern_bytes: 1024,
            max_regex_size_bytes: 131_072,
            json: JsonLimits {
                max_source_bytes: 65_536,
                max_depth: 64,
                max_nodes: 4096,
                max_decoded_bytes: 65_536,
            },
        }
    }

    fn snapshot(policies: Vec<Policy>) -> PolicySnapshot {
        PolicySnapshot {
            active_policies: policies,
            revoked_policies: BTreeSet::new(),
        }
    }

    fn file(
        source: &str,
        text: &str,
        scope: Scope,
        qualifications: Vec<Reference>,
    ) -> FileRevision {
        FileRevision {
            source: source.into(),
            revision: String::new(),
            path: format!("records/{source}.txt"),
            scope,
            bytes: text.as_bytes().to_vec(),
            author: "author".into(),
            policy: policy(),
            qualifications,
            retired: false,
        }
        .seal()
        .unwrap()
    }

    fn corpus(records: Vec<FileRevision>) -> Corpus {
        let mut policies = BTreeMap::new();
        for record in &records {
            policies.insert(record.policy.id.clone(), record.policy.clone());
        }
        Corpus::from_revisions(
            records,
            snapshot(policies.into_values().collect()),
            limits(),
        )
        .unwrap()
    }

    fn nav() -> NavigationBudget {
        NavigationBudget::new(NavigationLimits {
            calls: 100,
            scanned_bytes: 1_000_000_000,
            inspected_revisions: 1_000_000,
            matches: 10_000,
            result_bytes: 10_000_000,
        })
    }

    fn page_limits() -> PageLimits {
        PageLimits {
            scan_bytes: 100_000_000,
            matches: 1000,
            result_bytes: 1_000_000,
            results: 1000,
        }
    }

    fn listing() -> DiscoveryRequest {
        DiscoveryRequest {
            scope: Scope::Fragments,
            surface: SearchSurface::Path,
            mode: SearchMode::Listing,
            hidden_paths: HiddenPaths::Include,
            retirement: RetirementFilter::ActiveOnly,
        }
    }

    fn text_query(needle: &str) -> DiscoveryRequest {
        DiscoveryRequest {
            surface: SearchSurface::Text {
                representation: Representation::Utf8,
            },
            mode: SearchMode::Literal {
                needle: needle.into(),
            },
            ..listing()
        }
    }

    fn supply(
        corpus: &Corpus,
        mandatory: &[Reference],
        fragments: &[Reference],
        assets: &[Reference],
    ) -> Assembly {
        assemble(
            corpus,
            mandatory,
            fragments,
            assets,
            &admission(),
            1_000_000,
            17,
            31,
        )
        .unwrap()
    }

    #[test]
    fn integrity_binds_policy_author_path_and_payload() {
        let original = file("a", "exact", Scope::Fragments, vec![]);
        let mut tampered = original.clone();
        tampered.bytes.push(b'!');
        assert!(matches!(
            Corpus::from_revisions(vec![tampered], snapshot(vec![policy()]), limits()),
            Err(FilesError::DigestMismatch { .. })
        ));
        let mut changed_policy = original.clone();
        changed_policy.policy.audience.insert("another".into());
        assert_ne!(
            changed_policy.computed_revision().unwrap(),
            original.revision
        );
        assert!(matches!(
            Corpus::from_revisions(
                vec![original.clone(), changed_policy],
                snapshot(vec![policy()]),
                limits()
            ),
            Err(FilesError::IdentityConflict)
        ));
        let mut moved = original.clone();
        moved.path = "elsewhere/a.txt".into();
        assert_ne!(moved.computed_revision().unwrap(), original.revision);
        moved = original.clone();
        moved.author = "different".into();
        assert_ne!(moved.computed_revision().unwrap(), original.revision);
        let identical = corpus(vec![original.clone(), original.clone()]);
        assert_eq!(
            supply(
                &identical,
                &[],
                &[original.reference(Representation::Utf8)],
                &[]
            )
            .entries
            .len(),
            1
        );
    }

    #[test]
    fn conflicting_policy_identity_rejected_even_under_different_file_hashes() {
        let a = file("a", "a", Scope::Fragments, vec![]);
        let mut b = file("b", "b", Scope::Fragments, vec![]);
        b.policy.readers.insert("extra".into());
        b = b.seal().unwrap();
        assert!(matches!(
            Corpus::from_revisions(vec![a, b], snapshot(vec![policy()]), limits()),
            Err(FilesError::PolicyIdentityConflict)
        ));
    }

    #[test]
    fn canonical_paths_and_corpus_bounds_are_enforced() {
        for path in [
            "", "/a", "a/../b", "./a", "a//b", "a/", "a\\b", "C:a", "a\0b",
        ] {
            let mut record = file("a", "x", Scope::Assets, vec![]);
            record.path = path.into();
            record = record.seal().unwrap();
            assert!(
                matches!(
                    Corpus::from_revisions(vec![record], snapshot(vec![policy()]), limits()),
                    Err(FilesError::InvalidRecord { .. })
                ),
                "{path:?}"
            );
        }
        let mut small = limits();
        small.max_file_bytes = 2;
        assert!(matches!(
            Corpus::from_revisions(
                vec![file("a", "abc", Scope::Assets, vec![])],
                snapshot(vec![policy()]),
                small
            ),
            Err(FilesError::CorpusLimit("file bytes"))
        ));
        let mut small = limits();
        small.max_total_bytes = 3;
        assert!(matches!(
            Corpus::from_revisions(
                vec![
                    file("a", "ab", Scope::Assets, vec![]),
                    file("b", "cd", Scope::Assets, vec![])
                ],
                snapshot(vec![policy()]),
                small
            ),
            Err(FilesError::CorpusLimit("total payload bytes"))
        ));
    }

    #[test]
    fn permission_revocation_withholds_whole_bundle_without_clearing_selection() {
        let q = file("q", "condition", Scope::Assets, vec![]);
        let root = file(
            "r",
            "body",
            Scope::Fragments,
            vec![q.reference(Representation::Utf8)],
        );
        let selection = vec![root.reference(Representation::Utf8)];
        let original_selection = selection.clone();
        let c = corpus(vec![root, q]);
        assert_eq!(supply(&c, &[], &selection, &[]).entries.len(), 2);
        let revoked = PolicySnapshot {
            active_policies: vec![policy()],
            revoked_policies: BTreeSet::from([policy().identity()]),
        };
        let c = c.with_policy_snapshot(revoked).unwrap();
        let result = supply(&c, &[], &selection, &[]);
        assert!(result.entries.is_empty());
        assert_eq!(result.withheld.len(), 1);
        assert_eq!(selection, original_selection);
        assert!(matches!(
            assemble(&c, &selection, &[], &[], &admission(), 1_000_000, 0, 0),
            Err(FilesError::MandatoryUnavailable { .. })
        ));
        assert!(matches!(
            c.read(&selection[0], &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(WithholdReason::Unavailable))
        ));
    }

    #[test]
    fn access_checks_each_dimension_and_expiry_boundary() {
        let mut record = file("a", "body", Scope::Fragments, vec![]);
        record.policy.expires_at_unix = Some(101);
        record = record.seal().unwrap();
        let reference = record.reference(Representation::Utf8);
        let c = corpus(vec![record]);
        assert!(c
            .read(&reference, &admission(), &mut nav(), 1_000_000)
            .is_ok());
        let mut cases = Vec::new();
        let mut denied = admission();
        denied.principal = "different".into();
        cases.push(denied);
        let mut denied = admission();
        denied.destination = "different".into();
        cases.push(denied);
        let mut denied = admission();
        denied.audience.insert("different".into());
        cases.push(denied);
        let mut denied = admission();
        denied.now_unix = 101;
        cases.push(denied);
        for denied in cases {
            assert!(matches!(
                c.read(&reference, &denied, &mut nav(), 1_000_000),
                Err(FilesError::ReadUnavailable(WithholdReason::Unavailable))
            ));
            let page = c
                .discover(&listing(), &denied, page_limits(), &mut nav(), None)
                .unwrap();
            assert!(page.hits.is_empty());
            assert_eq!(page.status, SearchStatus::NoMatch);
        }
        let mut empty = admission();
        empty.audience.clear();
        assert!(matches!(
            c.read(&reference, &empty, &mut nav(), 1_000_000),
            Err(FilesError::InvalidAdmission)
        ));
    }

    #[test]
    fn missing_or_inaccessible_qualification_never_leaks_partial_guidance() {
        let missing = Reference::full("missing", "f".repeat(64), Representation::Utf8);
        let root = file("r", "must not escape", Scope::Fragments, vec![missing]);
        let reference = root.reference(Representation::Utf8);
        let c = corpus(vec![root]);
        let result = supply(&c, &[], std::slice::from_ref(&reference), &[]);
        assert!(result.entries.is_empty());
        assert_eq!(result.withheld[0].reason, WithholdReason::Unavailable);
        assert!(!result.rendered_context.contains("must not escape"));
        assert!(!result.rendered_context.contains("missing"));
        assert!(matches!(
            c.read(&reference, &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(WithholdReason::Unavailable))
        ));

        let mut q = file("private-q", "private condition", Scope::Assets, vec![]);
        q.policy.id = "private-policy".into();
        q.policy.readers = set(&["different"]);
        q = q.seal().unwrap();
        let root = file(
            "r",
            "must not escape",
            Scope::Fragments,
            vec![q.reference(Representation::Utf8)],
        );
        let reference = root.reference(Representation::Utf8);
        let c = corpus(vec![root, q]);
        let result = supply(&c, &[], &[reference], &[]);
        assert!(result.entries.is_empty());
        assert!(!result.rendered_context.contains("private-q"));
        assert!(!result.rendered_context.contains("private condition"));
    }

    #[test]
    fn exact_qualifications_preserve_distinct_conditions_and_roles() {
        let q1 = file("q1", "same text", Scope::Assets, vec![]);
        let q2 = file("q2", "same text", Scope::Assets, vec![]);
        let root = file(
            "r",
            "body",
            Scope::Fragments,
            vec![
                q1.reference(Representation::Utf8),
                q2.reference(Representation::Utf8),
            ],
        );
        let r = root.reference(Representation::Utf8);
        let c = corpus(vec![root, q1.clone(), q2]);
        let result = supply(
            &c,
            std::slice::from_ref(&r),
            std::slice::from_ref(&r),
            &[q1.reference(Representation::Utf8)],
        );
        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.entries[0].roles.len(), 2);
        let condition = result
            .entries
            .iter()
            .find(|entry| entry.reference.source == "q1")
            .unwrap();
        assert_eq!(condition.roles.len(), 3);
        assert_eq!(result.context_bytes, result.rendered_context.len());
        assert_eq!(result.total_bytes, result.context_bytes + 48);
    }

    #[test]
    fn graph_walker_handles_cycles_and_bounds_without_fabricating_content_addressed_records() {
        // This tests the actual production traversal separately from record-hash
        // admission. Mutually content-addressed records are not fabricated.
        let a = Reference::full("a", "a".repeat(64), Representation::Utf8);
        let b = Reference::full("b", "b".repeat(64), Representation::Utf8);
        let distinct_slice = Reference {
            start: Some(0),
            end: Some(1),
            ..b.clone()
        };
        let graph = BTreeMap::from([
            (a.clone(), vec![b.clone(), distinct_slice.clone()]),
            (b.clone(), vec![a.clone()]),
            (distinct_slice.clone(), vec![a.clone()]),
        ]);
        let mut counter = ClosureCounter::default();
        let walk = walk_qualifications(&a, &mut counter, &limits(), |reference| {
            Ok((graph[reference].clone(), 1))
        })
        .unwrap();
        assert_eq!(walk.order, vec![a.clone(), b, distinct_slice]);
        assert_eq!(walk.edges.len(), 4);
        let mut bounded = limits();
        bounded.max_closure_references = 2;
        assert!(matches!(
            walk_qualifications(
                &a,
                &mut ClosureCounter::default(),
                &bounded,
                |reference| Ok((graph[reference].clone(), 1))
            ),
            Err(ResolveError::Limit)
        ));
    }

    #[test]
    fn closure_limit_is_cumulative_across_selected_roots() {
        let a = file("a", "a", Scope::Fragments, vec![]);
        let b = file("b", "b", Scope::Fragments, vec![]);
        let refs = vec![
            a.reference(Representation::Utf8),
            b.reference(Representation::Utf8),
        ];
        let mut bounded = limits();
        bounded.max_closure_references = 1;
        let c = Corpus::from_revisions(vec![a, b], snapshot(vec![policy()]), bounded).unwrap();
        let result = assemble(&c, &[], &refs, &[], &admission(), 1_000_000, 0, 0).unwrap();
        assert!(result.entries.is_empty());
        assert_eq!(result.withheld.len(), 2);
        assert!(result
            .withheld
            .iter()
            .all(|bundle| bundle.reason == WithholdReason::Unavailable));
        assert_eq!(supply(&c, &[], &refs[..1], &[]).entries.len(), 1);
        assert!(c
            .read(&refs[0], &admission(), &mut nav(), 1_000_000)
            .is_ok());
    }

    #[test]
    fn overflow_never_drops_selected_bodies_or_output_reserve() {
        let record = file("a", "abcdefgh", Scope::Fragments, vec![]);
        let reference = record.reference(Representation::Utf8);
        let c = corpus(vec![record]);
        let selected = supply(&c, &[], std::slice::from_ref(&reference), &[]);
        assert!(matches!(
            assemble(
                &c,
                &[],
                std::slice::from_ref(&reference),
                &[],
                &admission(),
                selected.total_bytes - 1,
                17,
                31
            ),
            Err(FilesError::SelectedOverflow { .. })
        ));
        let mandatory = supply(&c, std::slice::from_ref(&reference), &[], &[]);
        assert!(matches!(
            assemble(
                &c,
                &[reference],
                &[],
                &[],
                &admission(),
                mandatory.total_bytes - 1,
                17,
                31
            ),
            Err(FilesError::MandatoryOverflow { .. })
        ));
        assert!(matches!(
            assemble(&c, &[], &[], &[], &admission(), usize::MAX, usize::MAX, 1),
            Err(FilesError::ArithmeticOverflow)
        ));
    }

    #[test]
    fn utf8_ranges_validate_boundaries_and_never_follow_a_new_revision() {
        let record = file("a", "aé🙂z", Scope::Fragments, vec![]);
        let reference = record.reference(Representation::Utf8);
        let c = corpus(vec![record.clone()]);
        let range = Reference {
            start: Some(1),
            end: Some(3),
            ..reference.clone()
        };
        assert_eq!(
            c.read(&range, &admission(), &mut nav(), 1_000_000)
                .unwrap()
                .entries[0]
                .text,
            "é"
        );
        let bad = Reference {
            start: Some(2),
            end: Some(3),
            ..reference.clone()
        };
        assert!(matches!(
            c.read(&bad, &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(WithholdReason::InvalidRange))
        ));
        let bad = Reference {
            start: Some(0),
            end: None,
            ..reference.clone()
        };
        assert!(matches!(
            c.read(&bad, &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(
                WithholdReason::InvalidReference
            ))
        ));
        let mut successor = record;
        successor.bytes = b"different".to_vec();
        successor = successor.seal().unwrap();
        let changed = corpus(vec![successor]);
        assert!(matches!(
            changed.read(&reference, &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(WithholdReason::Unavailable))
        ));
    }

    #[test]
    fn strict_json_rejects_nested_decoded_duplicates_and_invalid_surrogates() {
        let json_limits = limits().json;
        assert_eq!(
            parse_json_unique(br#"{"outer":{"a":1,"\u0061":2}}"#, &json_limits),
            Err(JsonProfileError::DuplicateKey)
        );
        assert_eq!(
            parse_json_unique(br#"{"x":"\uD800"}"#, &json_limits),
            Err(JsonProfileError::InvalidJson)
        );
        assert!(parse_json_unique(br#"{"x":"\uD83D\uDE00"}"#, &json_limits).is_ok());
        assert_eq!(
            parse_json_unique(b"{} false", &json_limits),
            Err(JsonProfileError::InvalidJson)
        );
        let mut shallow = json_limits.clone();
        shallow.max_depth = 2;
        assert_eq!(
            parse_json_unique(b"[[[0]]]", &shallow),
            Err(JsonProfileError::LimitExceeded)
        );
        let mut tiny = json_limits;
        tiny.max_decoded_bytes = 1;
        assert_eq!(
            parse_json_unique(br#"{"x":"ab"}"#, &tiny),
            Err(JsonProfileError::LimitExceeded)
        );
    }

    #[test]
    fn json_projection_decodes_once_and_search_labels_decoded_offsets() {
        let record = file(
            "a",
            r#"{"a/b":{"~":"x\\n\n\u00e9"}}"#,
            Scope::Fragments,
            vec![],
        );
        let representation = Representation::JsonString {
            pointer: "/a~1b/~0".into(),
        };
        let reference = record.reference(representation.clone());
        let c = corpus(vec![record]);
        let read = c
            .read(&reference, &admission(), &mut nav(), 1_000_000)
            .unwrap();
        assert_eq!(read.entries[0].text, "x\\n\né");
        let query = DiscoveryRequest {
            surface: SearchSurface::Text {
                representation: representation.clone(),
            },
            mode: SearchMode::Literal {
                needle: "é".into()
            },
            ..listing()
        };
        let result = c
            .discover(&query, &admission(), page_limits(), &mut nav(), None)
            .unwrap();
        assert_eq!(result.status, SearchStatus::Complete);
        assert_eq!(
            result.hits[0].matched_range,
            Some(ByteRange { start: 4, end: 6 })
        );
        assert_eq!(result.hits[0].reference.representation, representation);
        let unsupported = Reference {
            representation: Representation::Unsupported {
                name: "other".into(),
            },
            ..reference
        };
        assert!(matches!(
            c.read(&unsupported, &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(WithholdReason::Unsupported))
        ));
    }

    #[test]
    fn missing_nullable_intent_fields_are_not_defaulted() {
        let mut encoded = serde_json::to_value(policy()).unwrap();
        encoded.as_object_mut().unwrap().remove("expires_at_unix");
        assert!(serde_json::from_value::<Policy>(encoded).is_err());
        let reference = Reference::full("a", "a".repeat(64), Representation::Utf8);
        let mut encoded = serde_json::to_value(reference).unwrap();
        encoded.as_object_mut().unwrap().remove("start");
        assert!(serde_json::from_value::<Reference>(encoded).is_err());
    }

    #[test]
    fn irrelevant_corpus_does_not_grow_or_change_assembly_manifest() {
        let a = file("a", "selected", Scope::Fragments, vec![]);
        let reference = a.reference(Representation::Utf8);
        let alone = corpus(vec![a.clone()]);
        let expanded = corpus(vec![
            a,
            file("z", &"irrelevant".repeat(1000), Scope::Assets, vec![]),
        ]);
        assert_eq!(
            supply(&alone, &[], std::slice::from_ref(&reference), &[]),
            supply(&expanded, &[], &[reference], &[])
        );
    }

    #[test]
    fn reading_does_not_select_and_selection_checks_scope_and_derivative_use() {
        let mut record = file("a", "body", Scope::Assets, vec![]);
        record.policy.derivative_use = DerivativeUse::ExactOnly;
        record = record.seal().unwrap();
        let reference = record.reference(Representation::Utf8);
        let c = corpus(vec![record]);
        let mut budget = nav();
        let read = c
            .read(&reference, &admission(), &mut budget, 1_000_000)
            .unwrap();
        assert_eq!(read.entries[0].text, "body");
        assert!(supply(&c, &[], &[], &[]).entries.is_empty());
        assert_eq!(supply(&c, &[], &[], &[reference]).withheld.len(), 1);
        assert_eq!(budget.used().calls, 1);
        let record = file("b", "body", Scope::Assets, vec![]);
        let reference = record.reference(Representation::Utf8);
        let c = corpus(vec![record]);
        assert!(matches!(
            assemble(&c, &[], &[reference], &[], &admission(), 1_000_000, 0, 0),
            Err(FilesError::SelectedKindMismatch(_))
        ));
    }

    #[test]
    fn search_partial_coverage_and_cumulative_budget_remain_distinct_from_no_match() {
        let c = corpus(vec![
            file("a", "aaaa", Scope::Fragments, vec![]),
            file("b", "aaaa", Scope::Fragments, vec![]),
        ]);
        let query = text_query("a");
        let header = c.limits.max_total_record_bytes
            + serialized_len(&query, usize::MAX).unwrap()
            + serialized_len(&admission(), usize::MAX).unwrap();
        let mut page_bound = page_limits();
        page_bound.scan_bytes = header + 4;
        let mut budget = nav();
        let first = c
            .discover(&query, &admission(), page_bound, &mut budget, None)
            .unwrap();
        assert_eq!(first.status, SearchStatus::Incomplete);
        assert_eq!(first.hits.len(), 1);
        assert!(!first.coverage_complete());
        let second = c
            .discover(
                &query,
                &admission(),
                page_limits(),
                &mut budget,
                first.next.as_ref(),
            )
            .unwrap();
        assert_eq!(second.status, SearchStatus::Complete);
        assert_eq!(second.hits[0].reference.source, "b");
        assert_eq!(budget.used().calls, 2);
        assert_eq!(budget.used().matches, 2);
        assert!(first.rendered_json().unwrap().len() <= first.charged_result_bytes);
        let mut bounded = NavigationBudget::new(NavigationLimits {
            calls: 10,
            scanned_bytes: header + 4,
            inspected_revisions: 1000,
            matches: 10,
            result_bytes: 100_000,
        });
        let first = c
            .discover(&query, &admission(), page_limits(), &mut bounded, None)
            .unwrap();
        assert_eq!(first.status, SearchStatus::Exhausted);
        assert_eq!(first.hits.len(), 1);
        let second = c
            .discover(
                &query,
                &admission(),
                page_limits(),
                &mut bounded,
                first.next.as_ref(),
            )
            .unwrap();
        assert_eq!(second.status, SearchStatus::Exhausted);
        assert!(second.hits.is_empty());
        assert_eq!(bounded.used().scanned_bytes, header + 4);
    }

    fn assert_stale_page(result: Result<SearchPage, FilesError>) -> SearchPage {
        let page = result.unwrap();
        assert_eq!(page.status, SearchStatus::Unavailable);
        assert_eq!(page.stop, Some(DiscoveryStop::StalePage));
        assert!(page.snapshot.is_none());
        assert!(page.hits.is_empty());
        assert!(page.next.is_none());
        assert!(!page.coverage_complete());
        assert!(page.rendered_json().unwrap().len() <= page.charged_result_bytes);
        page
    }

    #[test]
    fn pagination_rejects_changed_view_query_or_admission() {
        let a = file("a", "a", Scope::Fragments, vec![]);
        let b = file("b", "b", Scope::Fragments, vec![]);
        let c = corpus(vec![a.clone(), b.clone()]);
        let mut one = page_limits();
        one.results = 1;
        let first = c
            .discover(&listing(), &admission(), one, &mut nav(), None)
            .unwrap();
        let cursor = first.next.as_ref().unwrap();
        let changed = corpus(vec![a, b, file("c", "c", Scope::Fragments, vec![])]);
        let stale = assert_stale_page(changed.discover(
            &listing(),
            &admission(),
            one,
            &mut nav(),
            Some(cursor),
        ));
        assert_eq!(stale.consulted.len(), 3);
        let stale = assert_stale_page(c.discover(
            &text_query("a"),
            &admission(),
            one,
            &mut nav(),
            Some(cursor),
        ));
        assert_eq!(stale.consulted.len(), 2);
        let mut later = admission();
        later.destination = "other-destination".into();
        let stale =
            assert_stale_page(c.discover(&listing(), &later, one, &mut nav(), Some(cursor)));
        assert!(stale.consulted.is_empty());
    }

    #[test]
    fn discovery_cursor_expiry_and_revocation_change_the_permitted_view() {
        let mut expiring = policy();
        expiring.expires_at_unix = Some(admission().now_unix + 2);
        let records = ["a", "b"]
            .into_iter()
            .map(|source| {
                let mut record = file(source, "body", Scope::Fragments, vec![]);
                record.policy = expiring.clone();
                record.seal().unwrap()
            })
            .collect::<Vec<_>>();
        let c = corpus(records.clone());
        let mut one = page_limits();
        one.results = 1;
        let mut budget = nav();
        let first = c
            .discover(&listing(), &admission(), one, &mut budget, None)
            .unwrap();
        let mut later = admission();
        later.now_unix += 1;
        assert!(c
            .discover(&listing(), &later, one, &mut budget, first.next.as_ref())
            .is_ok());
        later.now_unix += 1;
        let stale = assert_stale_page(c.discover(
            &listing(),
            &later,
            one,
            &mut budget,
            first.next.as_ref(),
        ));
        assert!(stale.consulted.is_empty());
        let mut revoked = snapshot(vec![expiring.clone()]);
        revoked.revoked_policies.insert(expiring.identity());
        let revoked = Corpus::from_revisions(records, revoked, limits()).unwrap();
        let stale = assert_stale_page(revoked.discover(
            &listing(),
            &admission(),
            one,
            &mut budget,
            first.next.as_ref(),
        ));
        assert!(stale.consulted.is_empty());
    }

    #[test]
    fn discovery_binary_last_revision_never_becomes_no_match() {
        let mut binary = file("a", "placeholder", Scope::Fragments, vec![]);
        binary.bytes = vec![0xff];
        binary = binary.seal().unwrap();
        let c = corpus(vec![binary]);
        let page = c
            .discover(
                &text_query("needle"),
                &admission(),
                page_limits(),
                &mut nav(),
                None,
            )
            .unwrap();
        assert_eq!(page.status, SearchStatus::Unavailable);
        assert!(!page.coverage_complete());
        assert!(page.next.is_none());
        // Explicit path listing remains available without decoding the body.
        let paths = c
            .discover(&listing(), &admission(), page_limits(), &mut nav(), None)
            .unwrap();
        assert_eq!(paths.hits.len(), 1);
        assert!(paths.coverage_complete());
    }

    #[test]
    fn discovery_cursor_survives_clock_only_change() {
        let c = corpus(vec![
            file("a", "a", Scope::Fragments, vec![]),
            file("b", "b", Scope::Fragments, vec![]),
        ]);
        let mut one = page_limits();
        one.results = 1;
        let mut budget = nav();
        let first = c
            .discover(&listing(), &admission(), one, &mut budget, None)
            .unwrap();
        let mut later = admission();
        later.now_unix += 1;
        let second = c
            .discover(&listing(), &later, one, &mut budget, first.next.as_ref())
            .unwrap();
        assert_eq!(second.hits[0].reference.source, "b");
        assert!(second.coverage_complete());
    }

    #[test]
    fn discovery_binary_unavailable_continuation_reaches_later_text_without_complete_claim() {
        let mut binary = file("a", "placeholder", Scope::Fragments, vec![]);
        binary.bytes = vec![0xff];
        binary = binary.seal().unwrap();
        let c = corpus(vec![binary, file("b", "needle", Scope::Fragments, vec![])]);
        let query = text_query("needle");
        let mut budget = nav();
        let first = c
            .discover(&query, &admission(), page_limits(), &mut budget, None)
            .unwrap();
        assert_eq!(first.status, SearchStatus::Unavailable);
        assert!(!first.coverage_complete());
        let second = c
            .discover(
                &query,
                &admission(),
                page_limits(),
                &mut budget,
                first.next.as_ref(),
            )
            .unwrap();
        assert_eq!(second.hits.len(), 1);
        assert_eq!(second.hits[0].reference.source, "b");
        assert!(!second.coverage_complete());
        assert!(second.next.is_none());
        assert_eq!(budget.used().calls, 2);
    }

    #[test]
    fn inaccessible_inventory_changes_neither_candidates_snapshot_nor_budget_charge() {
        let visible = file("a", "visible", Scope::Fragments, vec![]);
        let mut hidden = file("secret-name", "secret-body", Scope::Fragments, vec![]);
        hidden.policy.id = "secret-policy".into();
        hidden.policy.readers = set(&["other"]);
        hidden = hidden.seal().unwrap();
        let alone = corpus(vec![visible.clone()]);
        let expanded = corpus(vec![visible, hidden]);
        let mut first_budget = nav();
        let mut second_budget = nav();
        let first = alone
            .discover(
                &listing(),
                &admission(),
                page_limits(),
                &mut first_budget,
                None,
            )
            .unwrap();
        let second = expanded
            .discover(
                &listing(),
                &admission(),
                page_limits(),
                &mut second_budget,
                None,
            )
            .unwrap();
        assert_eq!(
            first.rendered_json().unwrap(),
            second.rendered_json().unwrap()
        );
        assert_eq!(first_budget.used(), second_budget.used());
        assert!(!second.rendered_json().unwrap().contains("secret"));
    }

    #[test]
    fn search_result_and_match_limits_never_claim_exhaustive_absence() {
        let c = corpus(vec![file("a", "aaaa", Scope::Fragments, vec![])]);
        let query = text_query("a");
        let mut limited = page_limits();
        limited.matches = 0;
        let page = c
            .discover(&query, &admission(), limited, &mut nav(), None)
            .unwrap();
        assert_eq!(page.status, SearchStatus::Incomplete);
        assert_eq!(page.stop, Some(DiscoveryStop::PageMatchLimit));
        limited = page_limits();
        limited.result_bytes = DISCOVERY_CONTROL_BYTES;
        let page = c
            .discover(&query, &admission(), limited, &mut nav(), None)
            .unwrap();
        assert_eq!(page.status, SearchStatus::Incomplete);
        assert_eq!(page.stop, Some(DiscoveryStop::PageResultLimit));
        assert!(page.hits.is_empty());
        let miss = c
            .discover(
                &text_query("absent"),
                &admission(),
                page_limits(),
                &mut nav(),
                None,
            )
            .unwrap();
        assert_eq!(miss.status, SearchStatus::NoMatch);
        assert!(miss.coverage_complete());
    }

    #[test]
    fn declared_regex_and_invalid_json_have_explicit_outcomes() {
        let c = corpus(vec![file("a", "aaaa", Scope::Fragments, vec![])]);
        let mut query = text_query("unused");
        query.mode = SearchMode::Regex {
            pattern: "^a+$".into(),
            dialect: RegexDialect::RustRegex1,
        };
        assert_eq!(
            c.discover(&query, &admission(), page_limits(), &mut nav(), None)
                .unwrap()
                .hits
                .len(),
            1
        );
        query.mode = SearchMode::Regex {
            pattern: "[".into(),
            dialect: RegexDialect::RustRegex1,
        };
        assert!(matches!(
            c.discover(&query, &admission(), page_limits(), &mut nav(), None),
            Err(FilesError::InvalidPattern)
        ));
        query.mode = SearchMode::Regex {
            pattern: "a".into(),
            dialect: RegexDialect::Unsupported {
                name: "other".into(),
            },
        };
        assert!(matches!(
            c.discover(&query, &admission(), page_limits(), &mut nav(), None),
            Err(FilesError::Unsupported)
        ));
        let c = corpus(vec![file(
            "a",
            r#"{"x":"a","x":"b"}"#,
            Scope::Fragments,
            vec![],
        )]);
        query.surface = SearchSurface::Text {
            representation: Representation::JsonString {
                pointer: "/x".into(),
            },
        };
        query.mode = SearchMode::Literal { needle: "a".into() };
        let result = c
            .discover(&query, &admission(), page_limits(), &mut nav(), None)
            .unwrap();
        assert_eq!(result.status, SearchStatus::Unavailable);
        assert_eq!(
            result.stop,
            Some(DiscoveryStop::SurfaceUnavailable(
                WithholdReason::InvalidJson
            ))
        );
        assert!(!result.coverage_complete());
    }

    #[test]
    fn retired_history_is_readable_but_not_current_context() {
        let mut record = file("a", "historical", Scope::Fragments, vec![]);
        record.retired = true;
        record = record.seal().unwrap();
        let reference = record.reference(Representation::Utf8);
        let c = corpus(vec![record]);
        assert!(
            c.read(&reference, &admission(), &mut nav(), 1_000_000)
                .unwrap()
                .entries[0]
                .retired
        );
        assert_eq!(supply(&c, &[], &[reference], &[]).withheld.len(), 1);
        assert_eq!(
            c.discover(&listing(), &admission(), page_limits(), &mut nav(), None)
                .unwrap()
                .status,
            SearchStatus::NoMatch
        );
        let mut query = listing();
        query.retirement = RetirementFilter::RetiredOnly;
        assert_eq!(
            c.discover(&query, &admission(), page_limits(), &mut nav(), None)
                .unwrap()
                .hits
                .len(),
            1
        );
    }
    #[test]
    fn empty_guidance_ranges_are_rejected_but_zero_width_search_hits_remain_legal() {
        let warning = file(
            "warning",
            "Do not omit this condition",
            Scope::Fragments,
            vec![],
        );
        let empty = Reference {
            start: Some(0),
            end: Some(0),
            ..warning.reference(Representation::Utf8)
        };
        let c = corpus(vec![warning.clone()]);
        assert!(matches!(
            c.read(&empty, &admission(), &mut nav(), 1_000_000),
            Err(FilesError::ReadUnavailable(
                WithholdReason::InvalidReference
            ))
        ));
        assert_eq!(
            project_text(&warning, &empty, &limits().json),
            Err(WithholdReason::InvalidRange)
        );
        let root = file("root", "body", Scope::Fragments, vec![empty.clone()]);
        assert!(matches!(
            Corpus::from_revisions(
                vec![root, warning.clone()],
                snapshot(vec![policy()]),
                limits()
            ),
            Err(FilesError::InvalidRecord {
                reason: "qualification reference",
                ..
            })
        ));
        let root = file("root", "body", Scope::Fragments, vec![]);
        let binding = accepted_binding(
            "empty-warning",
            &root,
            BindingDisposition::Qualify {
                required: vec![empty],
            },
        );
        assert!(matches!(
            corpus(vec![root, warning]).with_bindings(vec![binding]),
            Err(FilesError::InvalidRecord {
                reason: "invalid required accompaniment",
                ..
            })
        ));
        let query = DiscoveryRequest {
            mode: SearchMode::Regex {
                pattern: "^".into(),
                dialect: RegexDialect::RustRegex1,
            },
            ..text_query("unused")
        };
        let page = c
            .discover(&query, &admission(), page_limits(), &mut nav(), None)
            .unwrap();
        assert_eq!(
            page.hits[0].matched_range,
            Some(ByteRange { start: 0, end: 0 })
        );
        assert_eq!(page.hits[0].reference.start, None);
        assert_eq!(
            c.read(&page.hits[0].reference, &admission(), &mut nav(), 1_000_000)
                .unwrap()
                .entries[0]
                .text,
            "Do not omit this condition"
        );
    }

    #[test]
    fn external_reference_and_admission_bytes_are_bounded_and_accounted() {
        let record = file("a", "body", Scope::Fragments, vec![]);
        let reference = record.reference(Representation::Utf8);
        let mut bounded = limits();
        bounded.max_record_bytes = 2048;
        bounded.max_total_record_bytes = 2048;
        let c = Corpus::from_revisions(vec![record], snapshot(vec![policy()]), bounded).unwrap();
        let oversized = [
            Reference {
                source: "s".repeat(2049),
                ..reference.clone()
            },
            Reference {
                representation: Representation::JsonString {
                    pointer: "/".repeat(2049),
                },
                ..reference.clone()
            },
            Reference {
                representation: Representation::Unsupported {
                    name: "x".repeat(2049),
                },
                ..reference.clone()
            },
            Reference {
                source: "\u{0001}".repeat(400),
                ..reference.clone()
            },
        ];
        for reference in oversized {
            let mut budget = nav();
            assert!(matches!(
                c.read(&reference, &admission(), &mut budget, 1_000_000),
                Err(FilesError::RequestLimit("reference bytes"))
            ));
            assert_eq!(budget.used(), NavigationUsage::default());
            assert!(matches!(
                assemble(&c, &[], &[reference], &[], &admission(), 1_000_000, 0, 0),
                Err(FilesError::RequestLimit("reference bytes"))
            ));
        }
        let mut admissions = Vec::new();
        let mut huge = admission();
        huge.principal = "p".repeat(2049);
        admissions.push(huge);
        let mut huge = admission();
        huge.destination = "d".repeat(2049);
        admissions.push(huge);
        let mut huge = admission();
        huge.audience = BTreeSet::from(["a".repeat(2049)]);
        admissions.push(huge);
        let mut huge = admission();
        huge.audience = (0..2049).map(|i| format!("recipient-{i}")).collect();
        admissions.push(huge);
        for huge in admissions {
            assert!(matches!(
                c.read(&reference, &huge, &mut nav(), 1_000_000),
                Err(FilesError::RequestLimit("admission bytes"))
            ));
            assert!(matches!(
                assemble(&c, &[], &[], &[], &huge, 1_000_000, 0, 0),
                Err(FilesError::RequestLimit("admission bytes"))
            ));
            assert!(matches!(
                c.discover(&listing(), &huge, page_limits(), &mut nav(), None),
                Err(FilesError::RequestLimit("admission bytes"))
            ));
        }
        let large_root = Reference::full("x".repeat(600), "f".repeat(64), Representation::Utf8);
        assert!(matches!(
            assemble(
                &c,
                &[],
                &vec![large_root; 4],
                &[],
                &admission(),
                1_000_000,
                0,
                0
            ),
            Err(FilesError::RequestLimit("assembly request bytes"))
        ));
        let reference_bytes = serialized_len(&reference, usize::MAX).unwrap();
        assert_eq!(
            reference.request_bytes(reference_bytes),
            Ok(reference_bytes)
        );
        assert!(matches!(
            reference.request_bytes(reference_bytes - 1),
            Err(FilesError::RequestLimit(_))
        ));
        let admission_bytes = serialized_len(&admission(), usize::MAX).unwrap();
        assert_eq!(admission().validate(admission_bytes), Ok(admission_bytes));
        assert!(matches!(
            admission().validate(admission_bytes - 1),
            Err(FilesError::RequestLimit(_))
        ));
        let mut budget = nav();
        c.read(&reference, &admission(), &mut budget, 1_000_000)
            .unwrap();
        assert_eq!(
            budget.used().scanned_bytes,
            c.limits.max_closure_bytes + reference_bytes + admission_bytes
        );
    }

    #[test]
    fn private_graph_size_cannot_change_failures_or_a_later_independent_root() {
        let root = file("root", "unusable body", Scope::Fragments, vec![]);
        let good = file("good", "independent", Scope::Fragments, vec![]);
        let mut denied = file("hidden-denied", "hidden condition", Scope::Assets, vec![]);
        denied.policy.id = "hidden-policy".into();
        denied.policy.readers = set(&["another-reader"]);
        denied = denied.seal().unwrap();
        let private_policy = denied.policy.clone();
        let denied_ref = denied.reference(Representation::Utf8);
        let extras: Vec<_> = (0..16)
            .map(|i| {
                file(
                    &format!("hidden-extra-{i}"),
                    "hidden detail",
                    Scope::Assets,
                    vec![],
                )
            })
            .collect();
        let mut large_required = vec![denied_ref.clone()];
        large_required.extend(
            extras
                .iter()
                .map(|record| record.reference(Representation::Utf8)),
        );
        let mut records = vec![root.clone(), good.clone(), denied];
        records.extend(extras);
        let references = vec![
            root.reference(Representation::Utf8),
            good.reference(Representation::Utf8),
        ];
        // Cover both a denied root itself and an admitted root whose required
        // closure is denied. The latter also reaches limits before target access.
        for deny_root in [false, true] {
            let mut small = accepted_binding(
                "hidden-warning",
                &root,
                BindingDisposition::Qualify {
                    required: vec![denied_ref.clone()],
                },
            );
            let mut large = accepted_binding(
                "hidden-warning",
                &root,
                BindingDisposition::Qualify {
                    required: large_required.clone(),
                },
            );
            if deny_root {
                small.policy = private_policy.clone();
                small = small.seal().unwrap();
                large.policy = private_policy.clone();
                large = large.seal().unwrap();
            }
            let root_bytes = serialized_len(&root, usize::MAX).unwrap();
            let small_bytes = root_bytes + serialized_len(&small, usize::MAX).unwrap();
            assert!(root_bytes + serialized_len(&large, usize::MAX).unwrap() > small_bytes * 2);
            for dimension in [
                ClosureDimension::References,
                ClosureDimension::Edges,
                ClosureDimension::Bytes,
            ] {
                let mut bounded = limits();
                match dimension {
                    ClosureDimension::References => bounded.max_closure_references = 4,
                    ClosureDimension::Edges => bounded.max_closure_edges = 2,
                    ClosureDimension::Bytes => bounded.max_closure_bytes = small_bytes * 2,
                }
                let build = |binding: AcceptedBinding| {
                    Corpus::from_revisions(
                        records.clone(),
                        snapshot(vec![policy(), private_policy.clone()]),
                        bounded.clone(),
                    )
                    .unwrap()
                    .with_bindings(vec![binding])
                    .unwrap()
                };
                let small_corpus = build(small.clone());
                let large_corpus = build(large.clone());
                let first = supply(&small_corpus, &[], &references, &[]);
                let second = supply(&large_corpus, &[], &references, &[]);
                assert_eq!(first, second, "{dimension:?}, deny_root={deny_root}");
                assert_eq!(first.entries.len(), 1);
                assert_eq!(first.entries[0].reference, references[1]);
                assert_eq!(
                    first.withheld,
                    vec![WithheldBundle {
                        bundle: BundleId {
                            kind: BundleKind::SelectedFragment,
                            index: 0
                        },
                        reason: WithholdReason::Unavailable,
                    }]
                );
                assert!(!first.rendered_context.contains("hidden"));
                assert!(!first.rendered_context.contains("unusable body"));
                let mut first_budget = nav();
                let mut second_budget = nav();
                let first_error = small_corpus
                    .read(&references[0], &admission(), &mut first_budget, 1_000_000)
                    .unwrap_err();
                let second_error = large_corpus
                    .read(&references[0], &admission(), &mut second_budget, 1_000_000)
                    .unwrap_err();
                assert_eq!(
                    first_error,
                    FilesError::ReadUnavailable(WithholdReason::Unavailable)
                );
                assert_eq!(first_error, second_error);
                assert_eq!(first_budget.used(), second_budget.used());
                for c in [&small_corpus, &large_corpus] {
                    assert_eq!(
                        assemble(c, &references[..1], &[], &[], &admission(), 1_000_000, 0, 0)
                            .unwrap_err(),
                        FilesError::MandatoryUnavailable {
                            index: 0,
                            reason: WithholdReason::Unavailable
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn dependency_projection_failures_are_generic_and_atomic() {
        let root = file("root", "must stay withheld", Scope::Fragments, vec![]);
        let warning = file(
            "hidden-warning",
            r#"{"x":"one","x":"two"}"#,
            Scope::Assets,
            vec![],
        );
        let reference = root.reference(Representation::Utf8);
        for required in [
            warning.reference(Representation::JsonString {
                pointer: "/x".into(),
            }),
            Reference {
                start: Some(0),
                end: Some(999),
                ..warning.reference(Representation::Utf8)
            },
            Reference::full("hidden-missing", "f".repeat(64), Representation::Utf8),
        ] {
            let binding = accepted_binding(
                "hidden-binding",
                &root,
                BindingDisposition::Qualify {
                    required: vec![required],
                },
            );
            let c = corpus(vec![root.clone(), warning.clone()])
                .with_bindings(vec![binding])
                .unwrap();
            let result = supply(&c, &[], std::slice::from_ref(&reference), &[]);
            assert!(result.entries.is_empty());
            assert_eq!(result.withheld[0].reason, WithholdReason::Unavailable);
            assert!(!result.rendered_context.contains("hidden"));
            assert!(!result.rendered_context.contains("must stay withheld"));
            assert_eq!(
                c.read(&reference, &admission(), &mut nav(), 1_000_000)
                    .unwrap_err(),
                FilesError::ReadUnavailable(WithholdReason::Unavailable)
            );
        }
    }

    #[test]
    fn accepted_json_format_enforces_the_profile_without_inference_or_resealing() {
        for text in [r#"{"x":1,"\u0078":2}"#, r#"{"x":"\uD800"}"#, "{} false"] {
            let record = file("json-in-a-txt-path", text, Scope::Fragments, vec![]);
            let reference = record.reference(Representation::Utf8);
            let binding = accepted_binding(
                "json-format",
                &record,
                BindingDisposition::SourceFormat {
                    format: SourceFormat::Json {
                        profile: JSON_PROFILE.into(),
                    },
                },
            );
            let c = corpus(vec![record.clone()])
                .with_bindings(vec![binding])
                .unwrap();
            assert_eq!(record.computed_revision().unwrap(), reference.revision);
            assert_eq!(
                c.read(&reference, &admission(), &mut nav(), 1_000_000)
                    .unwrap()
                    .entries[0]
                    .text,
                text
            );
            let result = supply(&c, &[], std::slice::from_ref(&reference), &[]);
            assert!(result.entries.is_empty());
            assert_eq!(result.withheld[0].reason, WithholdReason::InvalidJson);
            // Even a valid-looking raw prefix cannot bypass whole-source validation.
            let prefix = Reference {
                start: Some(0),
                end: Some(1),
                ..reference.clone()
            };
            assert!(supply(&c, &[], &[prefix], &[]).entries.is_empty());
            assert!(matches!(
                assemble(&c, &[reference], &[], &[], &admission(), 1_000_000, 0, 0),
                Err(FilesError::MandatoryUnavailable {
                    reason: WithholdReason::InvalidJson,
                    ..
                })
            ));
        }
        let mut plain = file("plain", r#"{"x":1,"x":2}"#, Scope::Fragments, vec![]);
        plain.path = "ordinary-text.json".into();
        plain = plain.seal().unwrap();
        let reference = plain.reference(Representation::Utf8);
        assert_eq!(
            supply(
                &corpus(vec![plain.clone()]),
                &[],
                std::slice::from_ref(&reference),
                &[]
            )
            .entries
            .len(),
            1
        );
        let binding = accepted_binding(
            "plain-format",
            &plain,
            BindingDisposition::SourceFormat {
                format: SourceFormat::PlainText,
            },
        );
        let c = corpus(vec![plain]).with_bindings(vec![binding]).unwrap();
        assert_eq!(
            supply(&c, &[], &[reference], &[]).entries[0].text,
            r#"{"x":1,"x":2}"#
        );

        let json = file("valid", r#"{"x":"cue"}"#, Scope::Fragments, vec![]);
        let raw = json.reference(Representation::Utf8);
        let projected = json.reference(Representation::JsonString {
            pointer: "/x".into(),
        });
        let binding = accepted_binding(
            "json-format",
            &json,
            BindingDisposition::SourceFormat {
                format: SourceFormat::Json {
                    profile: JSON_PROFILE.into(),
                },
            },
        );
        let c = corpus(vec![json.clone()])
            .with_bindings(vec![binding.clone()])
            .unwrap();
        assert_eq!(
            supply(&c, &[], std::slice::from_ref(&raw), &[]).entries[0].text,
            r#"{"x":"cue"}"#
        );
        assert_eq!(supply(&c, &[], &[projected], &[]).entries[0].text, "cue");
        let mut bounded = limits();
        bounded.json.max_depth = 1;
        let c = Corpus::from_revisions(vec![json.clone()], snapshot(vec![policy()]), bounded)
            .unwrap()
            .with_bindings(vec![binding.clone()])
            .unwrap();
        let incomplete = supply(&c, &[], std::slice::from_ref(&raw), &[]);
        assert!(incomplete.entries.is_empty());
        assert_eq!(incomplete.withheld[0].reason, WithholdReason::Unavailable);
        assert!(c.read(&raw, &admission(), &mut nav(), 1_000_000).is_ok());
        let other_format = accepted_binding(
            "plain-format",
            &json,
            BindingDisposition::SourceFormat {
                format: SourceFormat::PlainText,
            },
        );
        assert!(matches!(
            corpus(vec![json.clone()]).with_bindings(vec![binding, other_format]),
            Err(FilesError::SourceFormatConflict)
        ));
        let unsupported = accepted_binding(
            "unknown-format",
            &json,
            BindingDisposition::SourceFormat {
                format: SourceFormat::Json {
                    profile: "unknown-profile".into(),
                },
            },
        );
        assert!(matches!(
            corpus(vec![json]).with_bindings(vec![unsupported]),
            Err(FilesError::InvalidRecord {
                reason: "unsupported source format profile",
                ..
            })
        ));
    }

    #[test]
    fn all_binding_restrictions_are_rendered_and_individually_enforced() {
        let mut broad = policy();
        broad.readers.insert("other".into());
        broad.processing_destinations.insert("elsewhere".into());
        broad.audience.insert("extra".into());
        let mut root = file("root", "body", Scope::Fragments, vec![]);
        root.policy = broad.clone();
        root = root.seal().unwrap();
        let mut warning = file("warning", "Condition and history", Scope::Assets, vec![]);
        warning.policy = broad.clone();
        warning = warning.seal().unwrap();
        let reference = root.reference(Representation::Utf8);
        let required = Reference {
            start: Some(0),
            end: Some(9),
            ..warning.reference(Representation::Utf8)
        };
        let mut restricted = policy();
        restricted.id = "restriction-policy".into();
        restricted.expires_at_unix = Some(101);
        let mut format = accepted_binding(
            "format",
            &root,
            BindingDisposition::SourceFormat {
                format: SourceFormat::PlainText,
            },
        );
        format.policy = broad.clone();
        format = format.seal().unwrap();
        let mut qualification = accepted_binding(
            "required-condition",
            &root,
            BindingDisposition::Qualify {
                required: vec![required.clone()],
            },
        );
        qualification.policy = restricted.clone();
        qualification = qualification.seal().unwrap();
        let bindings = vec![format.clone(), qualification.clone()];
        let build = || {
            Corpus::from_revisions(
                vec![root.clone(), warning.clone()],
                snapshot(vec![broad.clone(), restricted.clone()]),
                limits(),
            )
            .unwrap()
            .with_bindings(bindings.clone())
            .unwrap()
        };
        let c = build();
        let result = supply(&c, &[], std::slice::from_ref(&reference), &[]);
        let rendered: Value = serde_json::from_str(&result.rendered_context).unwrap();
        assert_eq!(
            rendered["entries"][0]["policy"],
            serde_json::to_value(&broad).unwrap()
        );
        let restrictions = rendered["entries"][0]["binding_restrictions"]
            .as_array()
            .unwrap();
        assert_eq!(restrictions.len(), bindings.len());
        for (actual, expected) in restrictions.iter().zip(&bindings) {
            assert_eq!(actual["source_role"], "accepted_binding");
            assert_eq!(actual["binding"], serde_json::to_value(expected).unwrap());
        }
        assert_eq!(result.entries[1].reference, required);
        assert_eq!(result.entries[1].text, "Condition");
        assert!(result.entries[1].roles.contains(&EntryRole::Qualification {
            bundle: BundleId {
                kind: BundleKind::SelectedFragment,
                index: 0
            },
            required_by: reference.clone(),
        }));
        assert_eq!(result.context_bytes, result.rendered_context.len());
        assert!(matches!(
            assemble(
                &c,
                &[],
                std::slice::from_ref(&reference),
                &[],
                &admission(),
                result.total_bytes - 1,
                17,
                31
            ),
            Err(FilesError::SelectedOverflow { .. })
        ));
        let mut denied = Vec::new();
        let mut case = admission();
        case.principal = "other".into();
        denied.push(case);
        let mut case = admission();
        case.destination = "elsewhere".into();
        denied.push(case);
        let mut case = admission();
        case.audience.insert("extra".into());
        denied.push(case);
        let mut case = admission();
        case.now_unix = 101;
        denied.push(case);
        for case in denied {
            let result = assemble(
                &c,
                &[],
                std::slice::from_ref(&reference),
                &[],
                &case,
                1_000_000,
                0,
                0,
            )
            .unwrap();
            assert!(result.entries.is_empty());
            assert_eq!(result.withheld[0].reason, WithholdReason::Unavailable);
            assert!(!result.rendered_context.contains("restriction-policy"));
            assert!(!result.rendered_context.contains("required-condition"));
            assert_eq!(
                c.read(&reference, &case, &mut nav(), 1_000_000)
                    .unwrap_err(),
                FilesError::ReadUnavailable(WithholdReason::Unavailable)
            );
        }
        let revoked = build()
            .with_policy_snapshot(PolicySnapshot {
                active_policies: vec![broad.clone(), restricted.clone()],
                revoked_policies: BTreeSet::from([restricted.identity()]),
            })
            .unwrap();
        assert!(supply(&revoked, &[], std::slice::from_ref(&reference), &[])
            .entries
            .is_empty());
        let mut exact = qualification;
        exact.policy.revision = "2".into();
        exact.policy.derivative_use = DerivativeUse::ExactOnly;
        exact = exact.seal().unwrap();
        let observation_only = Corpus::from_revisions(
            vec![root, warning],
            snapshot(vec![broad, exact.policy.clone()]),
            limits(),
        )
        .unwrap()
        .with_bindings(vec![format, exact.clone()])
        .unwrap();
        assert!(supply(
            &observation_only,
            &[],
            std::slice::from_ref(&reference),
            &[]
        )
        .entries
        .is_empty());
        let read = observation_only
            .read(&reference, &admission(), &mut nav(), 1_000_000)
            .unwrap();
        let rendered: Value = serde_json::from_str(&read.rendered_context).unwrap();
        assert_eq!(
            rendered["entries"][0]["binding_restrictions"][1]["binding"],
            serde_json::to_value(exact).unwrap()
        );
    }

    #[test]
    fn retained_binding_policies_prevent_identity_redefinition_across_snapshots() {
        let root = file("root", "body", Scope::Fragments, vec![]);
        let warning = file("warning", "condition", Scope::Assets, vec![]);
        let reference = root.reference(Representation::Utf8);
        let mut binding = accepted_binding(
            "qualification",
            &root,
            BindingDisposition::Qualify {
                required: vec![warning.reference(Representation::Utf8)],
            },
        );
        binding.policy.id = "binding-only-policy".into();
        binding = binding.seal().unwrap();
        let binding_policy = binding.policy.clone();
        let build = || {
            Corpus::from_revisions(
                vec![root.clone(), warning.clone()],
                snapshot(vec![policy(), binding_policy.clone()]),
                limits(),
            )
            .unwrap()
            .with_bindings(vec![binding.clone()])
            .unwrap()
        };
        let removed = || {
            build()
                .with_policy_snapshot(snapshot(vec![policy()]))
                .unwrap()
        };
        assert!(
            supply(&removed(), &[], std::slice::from_ref(&reference), &[])
                .entries
                .is_empty()
        );
        let mut conflicting = binding_policy.clone();
        conflicting.audience.insert("different".into());
        assert!(matches!(
            removed().with_policy_snapshot(snapshot(vec![policy(), conflicting.clone()])),
            Err(FilesError::PolicyIdentityConflict)
        ));
        let mut replacement = binding.clone();
        replacement.policy = conflicting;
        replacement = replacement.seal().unwrap();
        assert!(matches!(
            removed().with_bindings(vec![replacement]),
            Err(FilesError::PolicyIdentityConflict)
        ));
        let restored = removed()
            .with_policy_snapshot(snapshot(vec![policy(), binding_policy.clone()]))
            .unwrap();
        assert_eq!(supply(&restored, &[], &[reference], &[]).entries.len(), 2);
    }

    fn accepted_binding(
        id: &str,
        subject: &FileRevision,
        disposition: BindingDisposition,
    ) -> AcceptedBinding {
        AcceptedBinding {
            id: id.into(),
            revision: String::new(),
            source: subject.source.clone(),
            file_revision: subject.revision.clone(),
            policy: policy(),
            disposition,
        }
        .seal()
        .unwrap()
    }
    #[test]
    fn separately_accepted_cyclic_qualifications_are_real_corpus_records() {
        let a = file("a", "Account A", Scope::Fragments, vec![]);
        let b = file("b", "Qualification B", Scope::Assets, vec![]);
        let ar = a.reference(Representation::Utf8);
        let br = b.reference(Representation::Utf8);
        let bindings = vec![
            accepted_binding(
                "a-needs-b",
                &a,
                BindingDisposition::Qualify {
                    required: vec![br.clone()],
                },
            ),
            accepted_binding(
                "b-needs-a",
                &b,
                BindingDisposition::Qualify {
                    required: vec![ar.clone()],
                },
            ),
        ];
        let c = corpus(vec![a.clone(), b]).with_bindings(bindings).unwrap();
        let result = supply(&c, &[], std::slice::from_ref(&ar), &[]);
        assert_eq!(result.entries.len(), 2);
        assert!(result
            .entries
            .iter()
            .all(|e| e.accepted_bindings.len() == 1));
        assert_eq!(a.computed_revision().unwrap(), ar.revision);
        let mut navigation = nav();
        assert_eq!(
            c.read(&ar, &admission(), &mut navigation, 1_000_000)
                .unwrap()
                .entries
                .len(),
            2
        );
    }
    #[test]
    fn discovery_cursor_binds_effective_retirement_metadata() {
        let a = file("a", "first", Scope::Fragments, vec![]);
        let b = file("b", "second", Scope::Fragments, vec![]);
        let binding = accepted_binding(
            "retire-a",
            &a,
            BindingDisposition::Retired {
                reason: "Historical only".into(),
            },
        );
        let c = corpus(vec![a, b]);
        let request = DiscoveryRequest {
            retirement: RetirementFilter::AllRetained,
            ..listing()
        };
        let bounds = PageLimits {
            results: 1,
            ..page_limits()
        };
        let first = c
            .discover(&request, &admission(), bounds, &mut nav(), None)
            .unwrap();
        let cursor = first.next.as_ref().unwrap();
        let changed = c.with_bindings(vec![binding]).unwrap();
        let stale = assert_stale_page(changed.discover(
            &request,
            &admission(),
            bounds,
            &mut nav(),
            Some(cursor),
        ));
        assert_eq!(stale.consulted.len(), 2);
        let fresh = changed
            .discover(&request, &admission(), bounds, &mut nav(), None)
            .unwrap();
        assert_ne!(first.snapshot, fresh.snapshot);
    }

    #[test]
    fn accepted_retirement_blocks_the_old_revision_but_preserves_labeled_history() {
        let a = file("old", "Exact retained bytes", Scope::Fragments, vec![]);
        let ar = a.reference(Representation::Utf8);
        let binding = accepted_binding(
            "retire-old",
            &a,
            BindingDisposition::Retired {
                reason: "An authorized retirement.".into(),
            },
        );
        let c = corpus(vec![a]).with_bindings(vec![binding]).unwrap();
        let result = supply(&c, &[], std::slice::from_ref(&ar), &[]);
        assert!(result.entries.is_empty());
        assert_eq!(result.withheld.len(), 1);
        let history = c.read(&ar, &admission(), &mut nav(), 1_000_000).unwrap();
        assert_eq!(history.entries[0].text, "Exact retained bytes");
        assert!(history.entries[0].retired);
        let rendered: Value = serde_json::from_str(&history.rendered_context).unwrap();
        let restriction = &rendered["entries"][0]["binding_restrictions"][0];
        assert_eq!(restriction["source_role"], "accepted_binding");
        assert_eq!(restriction["binding"]["disposition"]["kind"], "retired");
        assert_eq!(
            restriction["binding"]["disposition"]["reason"],
            "An authorized retirement."
        );
        assert_eq!(
            restriction["binding"]["policy"],
            serde_json::to_value(policy()).unwrap()
        );
        let page = c
            .discover(&listing(), &admission(), page_limits(), &mut nav(), None)
            .unwrap();
        assert!(page.hits.is_empty());
    }
    #[test]
    fn a_private_required_binding_withholds_its_subject_without_disclosing_the_binding() {
        let a = file("visible", "A method", Scope::Fragments, vec![]);
        let ar = a.reference(Representation::Utf8);
        let mut binding = accepted_binding(
            "private-warning",
            &a,
            BindingDisposition::Ineligible {
                reason: "Restricted basis".into(),
            },
        );
        binding.policy.id = "private-policy".into();
        binding.policy.readers.clear();
        binding = binding.seal().unwrap();
        let private = binding.policy.clone();
        let c = Corpus::from_revisions(vec![a], snapshot(vec![policy(), private]), limits())
            .unwrap()
            .with_bindings(vec![binding])
            .unwrap();
        let result = supply(&c, &[], &[ar], &[]);
        assert_eq!(result.withheld.len(), 1);
        assert!(!result.rendered_context.contains("private-warning"));
        assert!(!result.rendered_context.contains("Restricted basis"));
    }
}
