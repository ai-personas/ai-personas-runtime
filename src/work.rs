//! Pure accepted-work state machine. No I/O, inference, scheduling side effects, or storage.
//!
//! Integration API:
//! - Register actual persona identities with `register_persona`; create work with `create`.
//! - The trusted runtime constructs non-deserializable contexts using `from_runtime`.
//!   Context identity, invocation, authority, epoch, time, observed events, and separation
//!   MUST come from actual admission/exposure records, never persona/operator JSON.
//! - Only after validating the WHOLE actual persona response, call `apply_response`.
//! - `observe` accepts separately authenticated governance decisions and actual receipts.
//!   It cannot accept responsibility on a persona's behalf.
//! - Persist the resulting board and its event/wait changes in ONE store transaction,
//!   with a store-level compare-and-swap. In-memory atomicity is not durable atomicity.
//! - Persist the cursor returned by `next_eligible` with actual admission. Supply busy or
//!   otherwise inadmissible personas in `excluded`; this function grants no resources.
//! - Deserialize only authenticated, integrity-checked snapshots produced by this API.
//!   Deserialization is recovery, NOT validation or authentication of imported state.
//!
//! References and snapshots establish recorded identity, not truth or model exposure.
//! Evidence snapshots here are captured at assessment adoption, not retroactively at
//! inference. The runtime must independently enforce grants, source access, lineage,
//! resource/retention bounds, whole-response validity, and exclusive persona invocation.
//! Scheduling fairness assumes a progressing caller, a persisted cursor, and continuing
//! eligibility in a finite queue. Neither eligibility nor an accepted response is success.
//!
//! Deliberate limits: one active commitment per persona/work; exclusive outcome and
//! reviewer-role ownership; whole-commitment handoffs; artifact-bearing mandates.
//! Live mandate amendments after any acceptance are rejected. Exact voluntary duty
//! changes use arrangement proposals within the immutable authorization envelope.
//! Legacy governing rules remain fixed. Automatic retries, arbitrary
//! imports, effect dispatch, and semantic correctness verification are unsupported.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

mod arrangements;
pub use arrangements::{
    formation_need_ref, Arrangement, ArrangementAssentPolicy, ArrangementConsent,
    ArrangementDecisionRule, ArrangementDecline, ArrangementProposal, ArrangementTerms,
    ArtifactTransfer, CommitmentChange, FormationDelegation, MAX_ARRANGEMENT_CHANGES,
    MAX_ARRANGEMENT_HOLDERS, MAX_ARRANGEMENT_PROPOSALS,
};

macro_rules! id {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_owned()) } }
    )+ };
}
id!(
    WorkId,
    PersonaId,
    ArtifactId,
    OutcomeId,
    RoleId,
    RecordId,
    InvocationId
);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExactRef {
    pub id: String,
    pub revision: u64,
    pub digest: [u8; 32],
}
impl ExactRef {
    /// Computes identity only; this does not authenticate the source or grant access.
    pub fn new(id: impl Into<String>, revision: u64, bytes: &[u8]) -> Self {
        Self {
            id: id.into(),
            revision,
            digest: Sha256::digest(bytes).into(),
        }
    }
    fn valid(&self) -> bool {
        !self.id.trim().is_empty() && self.revision > 0
    }
}
/// Trusted runtime identity for a real wait intention, not a future message.
/// Receipt and audience checks remain with normal authenticated event handling.
pub(crate) struct MessageWaitRegistration {
    pub dependency: ExactRef,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: ArtifactId,
    pub revision: u64,
    pub digest: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub revision: u64,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Human(String),
    Persona(PersonaId),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    pub origin: Origin,
    pub request: String,
    pub supplied: BTreeSet<ExactRef>,
    pub boundaries: ExactRef,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredOutcome {
    pub description: String,
    pub criterion: ExactRef,
    pub required_reviews: BTreeSet<RoleId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Independence {
    NotRequired,
    DistinctActor,
    DistinctExecution,
    QualifiedOutside,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRole {
    pub terms: String,
    pub funding: ExactRef,
    pub policy: ExactRef,
    pub independence: Independence,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Keep the bounded public value API; boxing a variant would change callers.
#[allow(clippy::large_enum_variant)]
pub enum DecisionRule {
    /// Prospective authorization to choose a rule through exact persona consent.
    /// The pool bounds arrangement actors and authority; it is no quorum or default leader.
    PersonaArranged {
        eligible: BTreeSet<PersonaId>,
        delegation: FormationDelegation,
    },
    Designated(PersonaId),
    Quorum {
        eligible: BTreeSet<PersonaId>,
        minimum: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mandate {
    pub revision: u64,
    pub terms: String,
    pub authority: ExactRef,
    pub assumptions: ExactRef,
    pub outcomes: BTreeMap<OutcomeId, RequiredOutcome>,
    pub review_roles: BTreeMap<RoleId, ReviewRole>,
    pub decision_rule: DecisionRule,
    pub delivery_to: BTreeSet<String>,
    pub stakeholders: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitmentTerms {
    /// References the complete immutable mandate, including its adopted decision rule.
    pub mandate_revision: u64,
    pub exact_terms: String,
    pub outcomes: BTreeSet<OutcomeId>,
    pub review_roles: BTreeSet<RoleId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribution {
    pub persona: PersonaId,
    pub invocation: InvocationId,
    pub version: Version,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAttribution {
    pub principal: Origin,
    pub authority: ExactRef,
    pub version: Version,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferStatus {
    Pending,
    Accepted(Attribution),
    Declined { by: Attribution, reason: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub to: PersonaId,
    pub terms: CommitmentTerms,
    pub from: Option<(PersonaId, RecordId)>,
    pub status: OfferStatus,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<ExactRef>,
    pub terms: CommitmentTerms,
    pub by: Attribution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accountability {
    Owner(PersonaId),
    Gap(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Continuation {
    Immediate {
        purpose: String,
    },
    /// A primary-authored future opportunity under this same live mandate.
    ReconsiderAt {
        not_before: u64,
        purpose: String,
    },
    Wait {
        dependency: ExactRef,
        deadline: u64,
        purpose: String,
    },
    /// Resume for any one of these exact sources, or at the deadline. A fresh
    /// persona decision determines whether more replies are still needed.
    WaitAny {
        dependencies: BTreeSet<ExactRef>,
        deadline: u64,
        purpose: String,
    },
    Blocked {
        reason: String,
        accountability: Accountability,
        reconsider_at: u64,
    },
    Paused {
        reason: String,
    },
    Ended {
        reason: String,
    },
}
pub const MAX_WAIT_DEPENDENCIES: usize = 8;
impl Continuation {
    pub(crate) fn waits_for(&self, reference: &ExactRef) -> bool {
        match self {
            Self::Wait { dependency, .. } => dependency == reference,
            Self::WaitAny { dependencies, .. } => dependencies.contains(reference),
            _ => false,
        }
    }
    pub(crate) fn wait_deadline(&self) -> Option<u64> {
        match self {
            Self::Wait { deadline, .. } | Self::WaitAny { deadline, .. } => Some(*deadline),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventKey {
    pub dependency: ExactRef,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub key: EventKey,
    pub accountability: Accountability,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub key: EventKey,
    pub observation: ExactRef,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wake {
    Event(EventKey),
    ExplicitResume,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    pub acceptance: Option<RecordId>,
    pub continuation: Option<Continuation>,
    pub wake: Option<Wake>,
    /// Accepted actual decisions; neither running inference nor successful work is inferred.
    pub activity: Vec<Attribution>,
    pub observed_events: BTreeSet<EventKey>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndingKind {
    Canceled,
    Unfinished,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Open,
    Paused {
        reason: String,
        by: RuntimeAttribution,
    },
    Ended {
        kind: EndingKind,
        reason: String,
        by: RuntimeAttribution,
    },
    /// Legacy serialized completion phase. New completion records do not end activity.
    /// Explicit targeted resumption preserves this record before returning to Open.
    Completed {
        release: RecordId,
        by: Attribution,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRevision {
    pub reference: ArtifactRef,
    pub bytes: Vec<u8>,
    pub producer: Attribution,
    pub inputs: BTreeSet<ExactRef>,
    pub policy: ExactRef,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub owner: PersonaId,
    pub current: u64,
    pub revisions: BTreeMap<u64, ArtifactRevision>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    RuntimeObservation,
    ExecutionReceipt,
    HumanJudgment,
    ExternalAssessment,
    ModelJudgment,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Availability {
    Available,
    Unavailable(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: RecordId,
    pub exact: ExactRef,
    pub source: SourceKind,
    pub availability: Availability,
    pub artifacts: BTreeSet<ArtifactRef>,
    pub criteria: BTreeSet<ExactRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Separation {
    Unknown,
    SelfAssessment,
    Known {
        distinct_actor: bool,
        distinct_execution: bool,
        distinct_model: bool,
        qualified_outside: bool,
        conflicts: BTreeSet<String>,
        attestation: ExactRef,
    },
}
impl Separation {
    pub fn satisfies(&self, policy: Independence) -> bool {
        if policy == Independence::NotRequired {
            return true;
        }
        match self {
            Self::Known {
                distinct_actor,
                distinct_execution,
                qualified_outside,
                conflicts,
                attestation,
                ..
            } if conflicts.is_empty() && attestation.valid() => match policy {
                Independence::DistinctActor => *distinct_actor,
                Independence::DistinctExecution => *distinct_actor && *distinct_execution,
                Independence::QualifiedOutside => *distinct_actor && *qualified_outside,
                Independence::NotRequired => true,
            },
            _ => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssessmentInput {
    pub id: RecordId,
    pub artifact: ArtifactRef,
    pub criterion: ExactRef,
    pub role: RoleId,
    pub evidence: BTreeSet<RecordId>,
    pub passes: bool,
    pub explanation: String,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    pub input: AssessmentInput,
    pub by: Attribution,
    pub accepted_role: RecordId,
    pub separation: Separation,
    pub snapshot: Vec<EvidenceRecord>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssessmentStatus {
    Current,
    Stale,
    Unverifiable,
    NotQualified,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub mandate_revision: u64,
    pub artifacts: BTreeMap<OutcomeId, ArtifactRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub met: bool,
    pub explanation: String,
    pub evidence: BTreeSet<RecordId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgmentInput {
    pub id: RecordId,
    pub candidate: Candidate,
    pub sufficient: bool,
    pub claims: BTreeMap<OutcomeId, Claim>,
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndividualJudgment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<ExactRef>,
    pub input: JudgmentInput,
    pub by: Attribution,
    pub acceptance: RecordId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: RecordId,
    pub artifact: Option<ArtifactRef>,
    pub text: String,
    pub evidence: BTreeSet<RecordId>,
    pub by: Attribution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerDisposition {
    pub finding: RecordId,
    pub open: bool,
    pub accountability: Accountability,
    pub rule: ExactRef,
    pub explanation: String,
    pub by: RuntimeAttribution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedDecision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<ExactRef>,
    pub candidate: Candidate,
    pub by: Attribution,
    pub judgments: Vec<RecordId>,
    pub supporters: BTreeSet<RecordId>,
    pub assessments: BTreeSet<RecordId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<ExactRef>,
    pub shared: RecordId,
    pub candidate: Candidate,
    pub authority: ExactRef,
    pub by: Attribution,
    /// Exact immutable prefix of Work::blocker_dispositions bound at release.
    pub blocker_dispositions: usize,
}
/// An authorized completion judgment for an exact release at its accepted decision.
/// Later changes may remove current support without erasing this historical record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub release: RecordId,
    pub by: Attribution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    pub id: RecordId,
    pub release: RecordId,
    pub recipient: String,
    pub receipt: ExactRef,
    pub by: RuntimeAttribution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StakeholderDecision {
    pub release: RecordId,
    pub stakeholder: String,
    pub accepts: bool,
    pub receipt: ExactRef,
    pub by: RuntimeAttribution,
}
/// Read-only prerequisites for one exact release, not a grant to close work.
/// Callers must separately check current work phase, decision rights and access.
/// Missing acceptance includes an absent or latest negative stakeholder decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosureReadiness {
    pub release: RecordId,
    pub release_current: bool,
    pub missing_deliveries: BTreeSet<String>,
    pub missing_stakeholder_acceptance: BTreeSet<String>,
    pub ready: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub need: Need,
    pub predecessor: Option<WorkId>,
    pub version: Version,
    pub phase: Phase,
    pub current_mandate: u64,
    pub mandates: BTreeMap<u64, Mandate>,
    pub participants: BTreeMap<PersonaId, Participant>,
    pub offers: BTreeMap<RecordId, Offer>,
    pub acceptances: BTreeMap<RecordId, Acceptance>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arrangement_proposals: BTreeMap<RecordId, ArrangementProposal>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arrangements: BTreeMap<RecordId, Arrangement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_arrangement: Option<RecordId>,
    pub artifacts: BTreeMap<ArtifactId, Artifact>,
    pub evidence: BTreeMap<RecordId, EvidenceRecord>,
    pub unavailable_evidence: BTreeMap<RecordId, String>,
    pub assessments: Vec<Assessment>,
    pub judgments: Vec<IndividualJudgment>,
    pub findings: BTreeMap<RecordId, Finding>,
    pub blocker_dispositions: Vec<BlockerDisposition>,
    pub shared: BTreeMap<RecordId, SharedDecision>,
    pub releases: BTreeMap<RecordId, Release>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completions: Vec<Completion>,
    pub deliveries: Vec<Delivery>,
    pub stakeholder_decisions: Vec<StakeholderDecision>,
    pub dependencies: Vec<Dependency>,
    pub events: Vec<Event>,
    /// Missing entries describe legacy unrestricted host observations. New local
    /// event ingress records the exact participants permitted to receive it.
    #[serde(default)]
    pub event_audiences: Vec<(EventKey, BTreeSet<PersonaId>)>,
    pub applied_invocations: BTreeSet<InvocationId>,
}
impl Work {
    /// Latest authored completion, including the unchanged legacy serialized form.
    /// This is historical metadata, not an activity state or present support claim.
    pub fn latest_completion(&self) -> Option<(&RecordId, &Attribution)> {
        self.completions
            .last()
            .map(|completion| (&completion.release, &completion.by))
            .or(match &self.phase {
                Phase::Completed { release, by } => Some((release, by)),
                _ => None,
            })
    }
    pub fn mandate(&self) -> &Mandate {
        &self.mandates[&self.current_mandate]
    }
    pub fn acceptance(&self, persona: &PersonaId) -> Option<(&RecordId, &Acceptance)> {
        let id = self.participants.get(persona)?.acceptance.as_ref()?;
        let a = self.acceptances.get(id)?;
        (a.terms.mandate_revision == self.current_mandate).then_some((id, a))
    }
    pub fn ownership(&self, outcome: &OutcomeId) -> Accountability {
        self.participants
            .keys()
            .find(|p| {
                self.acceptance(p)
                    .is_some_and(|(_, a)| a.terms.outcomes.contains(outcome))
            })
            .cloned()
            .map(Accountability::Owner)
            .unwrap_or_else(|| Accountability::Gap("No current accepted owner".into()))
    }
}

/// No Deserialize and no public constructor: only the trusted crate/runtime may mint this.
pub struct DecisionContext {
    work: WorkId,
    persona: PersonaId,
    invocation: InvocationId,
    version: Version,
    authority: ExactRef,
    now: u64,
    separation: BTreeMap<ArtifactRef, Separation>,
    observed: BTreeSet<EventKey>,
}
impl DecisionContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_runtime(
        work: WorkId,
        persona: PersonaId,
        invocation: InvocationId,
        version: Version,
        authority: ExactRef,
        now: u64,
        separation: BTreeMap<ArtifactRef, Separation>,
        observed: BTreeSet<EventKey>,
    ) -> Self {
        Self {
            work,
            persona,
            invocation,
            version,
            authority,
            now,
            separation,
            observed,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRight {
    NewAuthorization,
    Governance,
    Observation,
    StakeholderAcceptance,
}
/// This capability asserts independently checked current authority for the specific operation.
pub struct RuntimeContext {
    work: WorkId,
    version: Version,
    authority: ExactRef,
    principal: Origin,
    right: RuntimeRight,
}
impl RuntimeContext {
    pub(crate) fn from_runtime(
        work: WorkId,
        version: Version,
        authority: ExactRef,
        principal: Origin,
        right: RuntimeRight,
    ) -> Self {
        Self {
            work,
            version,
            authority,
            principal,
            right,
        }
    }
    fn attribution(&self) -> RuntimeAttribution {
        RuntimeAttribution {
            principal: self.principal.clone(),
            authority: self.authority.clone(),
            version: self.version,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub commands: Vec<Command>,
    pub continuation: Continuation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Invite {
        id: RecordId,
        to: PersonaId,
        terms: CommitmentTerms,
    },
    Accept {
        offer: RecordId,
        exact_terms: CommitmentTerms,
    },
    AcceptArrangedOffer {
        offer: RecordId,
        exact_terms: CommitmentTerms,
        arrangement: ExactRef,
    },
    Decline {
        offer: RecordId,
        reason: String,
    },
    OfferHandoff {
        id: RecordId,
        to: PersonaId,
    },
    ProposeArrangement {
        id: RecordId,
        terms: ArrangementTerms,
        consent_own: bool,
    },
    AcceptArrangement {
        proposal: RecordId,
        exact: ExactRef,
    },
    DeclineArrangement {
        proposal: RecordId,
        exact: ExactRef,
        reason: String,
    },
    PutArtifact {
        id: ArtifactId,
        base: Option<ArtifactRef>,
        bytes: Vec<u8>,
        inputs: BTreeSet<ExactRef>,
        policy: ExactRef,
    },
    /// The runtime binds this authored judgment to an exact artifact body
    /// actually supplied to the decision. It grants no independent assurance.
    RecordModelEvidence {
        id: RecordId,
        artifact: ArtifactRef,
        outcome: OutcomeId,
        exact: ExactRef,
    },
    Finding {
        id: RecordId,
        artifact: Option<ArtifactRef>,
        text: String,
        evidence: BTreeSet<RecordId>,
    },
    Assess(AssessmentInput),
    Judge(JudgmentInput),
    DecideShared {
        id: RecordId,
        candidate: Candidate,
    },
    Release {
        id: RecordId,
        shared: RecordId,
    },
    CloseGoal {
        release: RecordId,
    },
}
/// These are authenticated external facts/decisions, not a persona response schema.
// Keep the bounded public value API; boxing a variant would change callers.
#[allow(clippy::large_enum_variant)]
pub enum RuntimeOperation {
    /// Host acknowledgment only; no responsibility, authority or lifecycle change.
    AcknowledgeFailedAttempt {
        persona: PersonaId,
    },
    Invite {
        id: RecordId,
        to: PersonaId,
        terms: CommitmentTerms,
    },
    ReviseUnacceptedMandate(Mandate),
    Pause {
        reason: String,
    },
    Resume,
    ResumeParticipant {
        persona: PersonaId,
    },
    End {
        kind: EndingKind,
        reason: String,
    },
    Evidence(EvidenceRecord),
    EvidenceUnavailable {
        id: RecordId,
        reason: String,
    },
    RegisterDependency(Dependency),
    Event(Event),
    EventFor {
        event: Event,
        participants: BTreeSet<PersonaId>,
    },
    Blocker {
        finding: RecordId,
        open: bool,
        accountability: Accountability,
        rule: ExactRef,
        explanation: String,
    },
    Delivered {
        id: RecordId,
        release: RecordId,
        recipient: String,
        receipt: ExactRef,
    },
    Stakeholder {
        release: RecordId,
        accepts: bool,
        receipt: ExactRef,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpportunityCause {
    Immediate,
    Event(EventKey),
    Deadline,
    Reconsideration,
    ExplicitResume,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opportunity {
    pub work: WorkId,
    pub persona: PersonaId,
    pub version: Version,
    pub cause: OpportunityCause,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkBoard {
    works: BTreeMap<WorkId, Work>,
    personas: BTreeSet<PersonaId>,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkError {
    #[error("stale revision: expected {expected}, actual {actual}")]
    StaleRevision { expected: u64, actual: u64 },
    #[error("stale epoch: expected {expected}, actual {actual}")]
    StaleEpoch { expected: u64, actual: u64 },
    #[error("missing {0}")]
    Missing(&'static str),
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("conflicting immutable identity or ownership")]
    Conflict,
    #[error("context or decision lacks authority")]
    Unauthorized,
    #[error("exact current offered terms do not match")]
    Terms,
    #[error("work is stopped")]
    Stopped,
    #[error("persona has not accepted current responsibility")]
    NotAccepted,
    #[error("required completion evidence, ownership, review, delivery, or acceptance is absent")]
    Unmet,
    #[error("invocation has already been applied")]
    Replay,
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
}
type Result<T> = std::result::Result<T, WorkError>;
fn require(ok: bool, what: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(WorkError::Invalid(what))
    }
}
fn text(s: &str) -> bool {
    !s.trim().is_empty()
}
fn increment(n: u64) -> Result<u64> {
    n.checked_add(1)
        .ok_or(WorkError::Invalid("counter overflow"))
}

impl WorkBoard {
    pub fn work(&self, id: &WorkId) -> Result<&Work> {
        self.works.get(id).ok_or(WorkError::Missing("work"))
    }
    pub fn works(&self) -> &BTreeMap<WorkId, Work> {
        &self.works
    }
    pub fn personas(&self) -> &BTreeSet<PersonaId> {
        &self.personas
    }
    pub fn participant(&self, work: &WorkId, persona: &PersonaId) -> Option<&Participant> {
        self.works.get(work)?.participants.get(persona)
    }
    pub fn artifact(&self, work: &WorkId, reference: &ArtifactRef) -> Option<&ArtifactRevision> {
        let a = self
            .works
            .get(work)?
            .artifacts
            .get(&reference.id)?
            .revisions
            .get(&reference.revision)?;
        (&a.reference == reference).then_some(a)
    }
    pub(crate) fn register_persona(&mut self, persona: PersonaId) -> Result<()> {
        require(text(&persona.0), "persona identity")?;
        self.personas.insert(persona);
        Ok(())
    }
    pub fn create(&mut self, ctx: &RuntimeContext, need: Need, mandate: Mandate) -> Result<()> {
        self.create_inner(ctx, need, mandate, None)
    }
    /// Explicit new authorization creates a new work identity; it never reopens the old one.
    pub fn reauthorize(
        &mut self,
        ctx: &RuntimeContext,
        predecessor: &WorkId,
        mandate: Mandate,
    ) -> Result<()> {
        let old = self.work(predecessor)?;
        require(
            matches!(old.phase, Phase::Ended { .. }),
            "ended predecessor",
        )?;
        require(
            ctx.authority != old.mandate().authority,
            "new exact authorization",
        )?;
        let need = old.need.clone();
        self.create_inner(ctx, need, mandate, Some(predecessor.clone()))
    }
    fn create_inner(
        &mut self,
        ctx: &RuntimeContext,
        need: Need,
        mandate: Mandate,
        predecessor: Option<WorkId>,
    ) -> Result<()> {
        if ctx.right != RuntimeRight::NewAuthorization || ctx.authority != mandate.authority {
            return Err(WorkError::Unauthorized);
        }
        require(
            ctx.version
                == Version {
                    revision: 0,
                    epoch: 0,
                },
            "creation version",
        )?;
        require(
            text(&ctx.work.0) && text(&need.request) && need.boundaries.valid(),
            "original need",
        )?;
        require(
            need.supplied.iter().all(ExactRef::valid),
            "supplied references",
        )?;
        match &need.origin {
            Origin::Human(h) => require(text(h), "human origin")?,
            Origin::Persona(p) => require(self.personas.contains(p), "persona origin")?,
        }
        if self.works.contains_key(&ctx.work) {
            return Err(WorkError::Conflict);
        }
        validate_mandate(&mandate, &self.personas)?;
        arrangements::validate_delegation(&ctx.work, &need, &mandate, &ctx.principal)?;
        require(mandate.revision == 1, "initial mandate revision")?;
        let work = Work {
            need,
            predecessor,
            version: Version {
                revision: 1,
                epoch: 1,
            },
            phase: Phase::Open,
            current_mandate: 1,
            mandates: BTreeMap::from([(1, mandate)]),
            participants: BTreeMap::new(),
            offers: BTreeMap::new(),
            acceptances: BTreeMap::new(),
            arrangement_proposals: BTreeMap::new(),
            arrangements: BTreeMap::new(),
            current_arrangement: None,
            artifacts: BTreeMap::new(),
            evidence: BTreeMap::new(),
            unavailable_evidence: BTreeMap::new(),
            assessments: Vec::new(),
            judgments: Vec::new(),
            findings: BTreeMap::new(),
            blocker_dispositions: Vec::new(),
            shared: BTreeMap::new(),
            releases: BTreeMap::new(),
            completions: Vec::new(),
            deliveries: Vec::new(),
            stakeholder_decisions: Vec::new(),
            dependencies: Vec::new(),
            events: Vec::new(),
            event_audiences: Vec::new(),
            applied_invocations: BTreeSet::new(),
        };
        self.works.insert(ctx.work.clone(), work);
        Ok(())
    }
    // The callback returns false only for an authenticated, fenced no-op.
    fn transaction(
        &mut self,
        id: &WorkId,
        expected: Version,
        change: impl FnOnce(&mut Work) -> Result<bool>,
    ) -> Result<()> {
        let prior = self.work(id)?;
        if expected.revision != prior.version.revision {
            return Err(WorkError::StaleRevision {
                expected: expected.revision,
                actual: prior.version.revision,
            });
        }
        if expected.epoch != prior.version.epoch {
            return Err(WorkError::StaleEpoch {
                expected: expected.epoch,
                actual: prior.version.epoch,
            });
        }
        let mut next = prior.clone();
        if !change(&mut next)? {
            return Ok(());
        }
        next.version.revision = increment(prior.version.revision)?;
        self.works.insert(id.clone(), next);
        Ok(())
    }
    pub fn apply_response(
        &mut self,
        ctx: &DecisionContext,
        response: DecisionResponse,
    ) -> Result<()> {
        self.apply_response_with_message_wait(ctx, response, None)
    }
    /// Atomically bind an optional voluntary-message wait to the actual response.
    pub(crate) fn apply_response_with_message_wait(
        &mut self,
        ctx: &DecisionContext,
        response: DecisionResponse,
        registration: Option<MessageWaitRegistration>,
    ) -> Result<()> {
        self.apply_response_with_message_waits(ctx, response, registration.into_iter().collect())
    }
    /// Every registration and the authored wait set commit as one response.
    pub(crate) fn apply_response_with_message_waits(
        &mut self,
        ctx: &DecisionContext,
        response: DecisionResponse,
        registrations: Vec<MessageWaitRegistration>,
    ) -> Result<()> {
        require(
            registrations.len() <= MAX_WAIT_DEPENDENCIES,
            "message wait bound",
        )?;
        let personas = self.personas.clone();
        self.transaction(&ctx.work, ctx.version, |w| {
            open(w)?;
            if ctx.authority != w.mandate().authority || !personas.contains(&ctx.persona) {
                return Err(WorkError::Unauthorized);
            }
            require(text(&ctx.invocation.0), "invocation identity")?;
            if w.applied_invocations.contains(&ctx.invocation) {
                return Err(WorkError::Replay);
            }
            if !w.participants.contains_key(&ctx.persona) {
                return Err(WorkError::NotAccepted);
            }
            require(
                ctx.observed.iter().all(|k| {
                    w.events.iter().any(|e| &e.key == k) && event_permitted(w, k, &ctx.persona)
                }),
                "actual observed event bindings",
            )?;
            let registers_message_wait = !registrations.is_empty();
            for registration in registrations {
                require(registration.dependency.valid(), "message wait dependency")?;
                require(
                    response.continuation.waits_for(&registration.dependency),
                    "matching message wait",
                )?;
                require(
                    !response
                        .commands
                        .iter()
                        .any(|command| matches!(command, Command::Decline { .. })),
                    "message wait without decline",
                )?;
                if w.dependencies
                    .iter()
                    .any(|d| d.key.dependency.id == registration.dependency.id)
                {
                    return Err(WorkError::Conflict);
                }
                w.dependencies.push(Dependency {
                    key: EventKey {
                        dependency: registration.dependency,
                        epoch: w.version.epoch,
                    },
                    accountability: Accountability::Gap(
                        "Message arrival is voluntary; no sender responsibility is imposed".into(),
                    ),
                });
            }
            let by = Attribution {
                persona: ctx.persona.clone(),
                invocation: ctx.invocation.clone(),
                version: ctx.version,
            };
            w.participants
                .get_mut(&ctx.persona)
                .unwrap()
                .observed_events
                .extend(ctx.observed.iter().cloned());
            let prior_completions = w.completions.len();
            for command in response.commands {
                open(w)?;
                apply_command(w, &personas, ctx, &by, command)?;
            }
            // Acceptance may occur in this response. Require its actual validated
            // result before committing a wait; all staged registrations roll back
            // if acceptance or any later command fails.
            if registers_message_wait {
                accepted(w, &ctx.persona)?;
            }
            // A new completion describes this atomic response as committed, not an
            // intermediate state invalidated by a later command in that response.
            if w.completions[prior_completions..]
                .iter()
                .any(|completion| !closure_ready(w, &completion.release))
            {
                return Err(WorkError::Unmet);
            }
            set_continuation(w, &ctx.persona, response.continuation, ctx.now)?;
            w.participants
                .get_mut(&ctx.persona)
                .unwrap()
                .activity
                .push(by);
            w.applied_invocations.insert(ctx.invocation.clone());
            Ok(true)
        })
    }
    pub fn observe(&mut self, ctx: &RuntimeContext, operation: RuntimeOperation) -> Result<()> {
        let personas = self.personas.clone();
        self.transaction(&ctx.work, ctx.version, |w| {
            if ctx.authority != w.mandate().authority {
                return Err(WorkError::Unauthorized);
            }
            let needed = match &operation {
                RuntimeOperation::Evidence(_)
                | RuntimeOperation::EvidenceUnavailable { .. }
                | RuntimeOperation::RegisterDependency(_)
                | RuntimeOperation::Event(_)
                | RuntimeOperation::EventFor { .. }
                | RuntimeOperation::Delivered { .. } => RuntimeRight::Observation,
                RuntimeOperation::Stakeholder { .. } => RuntimeRight::StakeholderAcceptance,
                _ => RuntimeRight::Governance,
            };
            if ctx.right != needed {
                return Err(WorkError::Unauthorized);
            }
            let by = ctx.attribution();
            match operation {
                RuntimeOperation::AcknowledgeFailedAttempt { persona } => {
                    require(w.participants.contains_key(&persona), "acknowledged participant")?;
                }
                RuntimeOperation::Invite { id, to, terms } => {
                    open(w)?;
                    offer(w, &personas, id, to, terms, None)?;
                }
                RuntimeOperation::ReviseUnacceptedMandate(m) => {
                    open(w)?;
                    if !w.acceptances.is_empty() {
                        return Err(WorkError::Unsupported(
                            "live mandate amendment after acceptance",
                        ));
                    }
                    validate_mandate(&m, &personas)?;
                    arrangements::validate_delegation(&ctx.work, &w.need, &m, &ctx.principal)?;
                    require(
                        m.revision == increment(w.current_mandate)? && m.authority == ctx.authority,
                        "mandate successor",
                    )?;
                    w.current_mandate = m.revision;
                    w.mandates.insert(m.revision, m);
                    w.version.epoch = increment(w.version.epoch)?;
                }
                RuntimeOperation::Pause { reason } => {
                    open(w)?;
                    require(text(&reason), "pause reason")?;
                    w.phase = Phase::Paused { reason, by };
                }
                RuntimeOperation::Resume => {
                    if !matches!(w.phase, Phase::Open | Phase::Paused { .. }) {
                        return Err(WorkError::Stopped);
                    }
                    // Lifting a work-wide pause restores only the activity
                    // gate. Preserve each participant's intent and wake; a
                    // future wait must not become an immediate reconsideration.
                    w.phase = Phase::Open;
                }
                RuntimeOperation::ResumeParticipant { persona } => {
                    let legacy_completion = match &w.phase {
                        Phase::Completed { release, by } => Some(Completion {
                            release: release.clone(),
                            by: by.clone(),
                        }),
                        Phase::Open => None,
                        _ => return Err(WorkError::Stopped),
                    };
                    if let Some(completion) = &legacy_completion {
                        // Legacy completion erased voluntary continuation. Only
                        // an explicit, accepted target receives a fresh choice;
                        // no peer continuation, epoch or allowance is restored.
                        accepted(w, &persona)?;
                        if !w.completions.contains(completion) {
                            w.completions.push(completion.clone());
                        }
                    }
                    let target_accepted = w.acceptance(&persona).is_some();
                    let participant = w
                        .participants
                        .get_mut(&persona)
                        .ok_or(WorkError::Missing("participant"))?;
                    // A later explicit target may still be a dormant legacy peer
                    // after the first target already normalized the work phase.
                    if target_accepted
                        && matches!(participant.continuation, Some(Continuation::Ended { .. }))
                    {
                        participant.continuation = Some(Continuation::Paused {
                            reason: "Explicit target may choose whether to revisit retained responsibility".into(),
                        });
                    }
                    if target_accepted
                        && matches!(
                            participant.continuation,
                            Some(Continuation::Paused { .. } | Continuation::Wait { .. } | Continuation::WaitAny { .. } | Continuation::ReconsiderAt { .. })
                        )
                    {
                        participant.wake = Some(Wake::ExplicitResume);
                    }
                    w.phase = Phase::Open;
                }
                RuntimeOperation::End { kind, reason } => {
                    if !matches!(w.phase, Phase::Open | Phase::Paused { .. }) {
                        return Err(WorkError::Stopped);
                    }
                    require(text(&reason), "ending reason")?;
                    end_continuations(w, &reason);
                    w.version.epoch = increment(w.version.epoch)?;
                    w.phase = Phase::Ended { kind, reason, by };
                }
                RuntimeOperation::Evidence(e) => {
                    require(text(&e.id.0) && e.exact.valid(), "evidence identity")?;
                    require(
                        e.artifacts
                            .iter()
                            .all(|a| historical_artifact(w, a).is_some())
                            && e.criteria.iter().all(ExactRef::valid),
                        "evidence subjects",
                    )?;
                    if let Availability::Unavailable(reason) = &e.availability {
                        require(text(reason), "evidence unavailability")?;
                    }
                    if w.evidence.contains_key(&e.id) {
                        return Err(WorkError::Conflict);
                    }
                    w.evidence.insert(e.id.clone(), e);
                }
                RuntimeOperation::EvidenceUnavailable { id, reason } => {
                    require(text(&reason), "evidence unavailability")?;
                    if !w.evidence.contains_key(&id) {
                        return Err(WorkError::Missing("evidence"));
                    }
                    w.unavailable_evidence.insert(id, reason);
                }
                RuntimeOperation::RegisterDependency(d) => {
                    open(w)?;
                    check_accountability(w, &d.accountability)?;
                    require(
                        d.key.epoch == w.version.epoch && d.key.dependency.valid(),
                        "current real dependency",
                    )?;
                    if w.dependencies.iter().any(|old| old.key == d.key) {
                        return Err(WorkError::Conflict);
                    }
                    w.dependencies.push(d);
                }
                RuntimeOperation::Event(event) => return record_event(w, event, None),
                RuntimeOperation::EventFor {
                    event,
                    participants,
                } => {
                    return record_event(w, event, Some(participants));
                }
                RuntimeOperation::Blocker {
                    finding,
                    open: is_open,
                    accountability,
                    rule,
                    explanation,
                } => {
                    open(w)?;
                    check_accountability(w, &accountability)?;
                    require(
                        rule.valid() && text(&explanation),
                        "explicit blocker disposition",
                    )?;
                    if !w.findings.contains_key(&finding) {
                        return Err(WorkError::Missing("finding"));
                    }
                    if !is_open
                        && !w
                            .blocker_dispositions
                            .iter()
                            .rev()
                            .find(|d| d.finding == finding)
                            .is_some_and(|d| d.open)
                    {
                        return Err(WorkError::Missing("adopted open blocker"));
                    }
                    w.blocker_dispositions.push(BlockerDisposition {
                        finding,
                        open: is_open,
                        accountability,
                        rule,
                        explanation,
                        by,
                    });
                }
                RuntimeOperation::Delivered {
                    id,
                    release,
                    recipient,
                    receipt,
                } => {
                    require(text(&id.0) && receipt.valid(), "delivery receipt")?;
                    if !w.releases.contains_key(&release) {
                        return Err(WorkError::Missing("release"));
                    }
                    require(
                        w.mandate().delivery_to.contains(&recipient),
                        "permitted exact recipient",
                    )?;
                    if w.deliveries.iter().any(|d| d.id == id) {
                        return Err(WorkError::Conflict);
                    }
                    w.deliveries.push(Delivery {
                        id,
                        release,
                        recipient,
                        receipt,
                        by,
                    });
                }
                RuntimeOperation::Stakeholder {
                    release,
                    accepts,
                    receipt,
                } => {
                    let stakeholder = match &ctx.principal {
                        Origin::Human(h) => h.clone(),
                        _ => return Err(WorkError::Unauthorized),
                    };
                    require(
                        w.mandate().stakeholders.contains(&stakeholder) && receipt.valid(),
                        "actual required stakeholder",
                    )?;
                    if !w.releases.contains_key(&release) {
                        return Err(WorkError::Missing("release"));
                    }
                    w.stakeholder_decisions.push(StakeholderDecision {
                        release,
                        stakeholder,
                        accepts,
                        receipt,
                        by,
                    });
                }
            }
            Ok(true)
        })
    }
    pub fn assessment_status(&self, work: &WorkId, id: &RecordId) -> Result<AssessmentStatus> {
        let w = self.work(work)?;
        let a = w
            .assessments
            .iter()
            .find(|a| &a.input.id == id)
            .ok_or(WorkError::Missing("assessment"))?;
        Ok(assessment_status(w, a))
    }
    pub fn release_current(&self, work: &WorkId, release: &RecordId) -> bool {
        self.work(work)
            .ok()
            .is_some_and(|w| release_current(w, release))
    }
    /// Diagnose the existing closure guard without recording evidence or consent.
    /// This lower-level API does not perform a caller-specific exposure check.
    pub fn closure_readiness(&self, work: &WorkId, release: &RecordId) -> Result<ClosureReadiness> {
        let w = self.work(work)?;
        let release_current = release_current(w, release);
        let missing_deliveries: BTreeSet<_> = w
            .mandate()
            .delivery_to
            .iter()
            .filter(|recipient| {
                !w.deliveries
                    .iter()
                    .any(|d| &d.release == release && &d.recipient == *recipient)
            })
            .cloned()
            .collect();
        let missing_stakeholder_acceptance: BTreeSet<_> = w
            .mandate()
            .stakeholders
            .iter()
            .filter(|stakeholder| {
                !w.stakeholder_decisions
                    .iter()
                    .rev()
                    .find(|d| &d.release == release && &d.stakeholder == *stakeholder)
                    .is_some_and(|d| d.accepts)
            })
            .cloned()
            .collect();
        let ready = release_current
            && missing_deliveries.is_empty()
            && missing_stakeholder_acceptance.is_empty();
        Ok(ClosureReadiness {
            release: release.clone(),
            release_current,
            missing_deliveries,
            missing_stakeholder_acceptance,
            ready,
        })
    }
    pub fn completion_supported(&self, work: &WorkId) -> bool {
        self.work(work).ok().is_some_and(|w| {
            w.latest_completion()
                .is_some_and(|(release, _)| closure_ready(w, release))
        })
    }
    /// A bounded live page over ALL participation identities, including stopped,
    /// future and unaccepted entries. Cursor advancement does not imply admission.
    /// Each pair occurs at most once in this page, including across wraparound.
    pub fn consideration_page(
        &self,
        after: Option<&(WorkId, PersonaId)>,
        limit: usize,
    ) -> Result<Vec<(WorkId, PersonaId)>> {
        require((1..=128).contains(&limit), "consideration page bound")?;
        let pairs = || {
            self.works.iter().flat_map(|(wid, work)| {
                work.participants
                    .keys()
                    .map(move |pid| (wid.clone(), pid.clone()))
            })
        };
        Ok(pairs()
            .filter(|pair| after.is_none_or(|cursor| pair > cursor))
            .chain(pairs().filter(|pair| after.is_some_and(|cursor| pair <= cursor)))
            .take(limit)
            .collect())
    }
    pub fn opportunity_for(
        &self,
        work: &WorkId,
        persona: &PersonaId,
        now: u64,
    ) -> Option<Opportunity> {
        let current = self.works.get(work)?;
        Some(Opportunity {
            work: work.clone(),
            persona: persona.clone(),
            version: current.version,
            cause: eligibility(current, persona, now)?,
        })
    }
    /// Alternative unobserved arrivals for the same current authored wait.
    /// The runtime still checks each source's current information permissions.
    pub(crate) fn waiting_event_keys(&self, work: &WorkId, persona: &PersonaId) -> Vec<EventKey> {
        self.works
            .get(work)
            .filter(|w| matches!(w.phase, Phase::Open) && w.acceptance(persona).is_some())
            .map(|w| pending_wait_event_keys(w, persona))
            .unwrap_or_default()
    }
    /// Pure round robin over identity order. The caller persists the returned pair as cursor.
    pub fn next_eligible(
        &self,
        after: Option<&(WorkId, PersonaId)>,
        excluded: &BTreeSet<PersonaId>,
        now: u64,
    ) -> Option<Opportunity> {
        let mut candidates = Vec::new();
        for (wid, w) in &self.works {
            for pid in w.participants.keys().filter(|p| !excluded.contains(*p)) {
                if let Some(cause) = eligibility(w, pid, now) {
                    candidates.push(Opportunity {
                        work: wid.clone(),
                        persona: pid.clone(),
                        version: w.version,
                        cause,
                    });
                }
            }
        }
        let index = after
            .and_then(|cursor| {
                candidates
                    .iter()
                    .position(|c| (&c.work, &c.persona) > (&cursor.0, &cursor.1))
            })
            .unwrap_or(0);
        candidates.get(index).cloned()
    }
}

fn event_permitted(w: &Work, key: &EventKey, persona: &PersonaId) -> bool {
    w.event_audiences
        .iter()
        .find(|(bound, _)| bound == key)
        .is_none_or(|(_, audience)| audience.contains(persona))
}
fn pending_wait_event_keys(w: &Work, persona: &PersonaId) -> Vec<EventKey> {
    let Some(participant) = w.participants.get(persona) else {
        return Vec::new();
    };
    let Some(next) = participant.continuation.as_ref() else {
        return Vec::new();
    };
    w.events.iter().rev()
        .filter(|event| {
            event.key.epoch == w.version.epoch
                && next.waits_for(&event.key.dependency)
                && event_permitted(w, &event.key, persona)
                && !participant.observed_events.contains(&event.key)
        })
        .take(MAX_WAIT_DEPENDENCIES)
        .map(|event| event.key.clone())
        .collect()
}
fn record_event(w: &mut Work, event: Event, audience: Option<BTreeSet<PersonaId>>) -> Result<bool> {
    require(event.observation.valid(), "event observation")?;
    require(
        w.dependencies.iter().any(|d| d.key == event.key),
        "registered event source",
    )?;
    if let Some(participants) = &audience {
        require(
            participants.iter().all(|p| w.participants.contains_key(p)),
            "event participants",
        )?;
    }
    if let Some(old) = w.events.iter().find(|old| old.key == event.key) {
        let old_audience = w
            .event_audiences
            .iter()
            .find(|(key, _)| key == &event.key)
            .map(|(_, people)| people);
        return if old == &event && old_audience == audience.as_ref() {
            Ok(false)
        } else {
            Err(WorkError::Conflict)
        };
    }
    if matches!(w.phase, Phase::Open) && event.key.epoch == w.version.epoch {
        for (id, participant) in &mut w.participants {
            if audience.as_ref().is_none_or(|people| people.contains(id))
                && participant.continuation.as_ref()
                    .is_some_and(|next| next.waits_for(&event.key.dependency))
                && !participant.observed_events.contains(&event.key)
            {
                participant.wake = Some(Wake::Event(event.key.clone()));
            }
        }
    }
    if let Some(people) = audience {
        w.event_audiences.push((event.key.clone(), people));
    }
    w.events.push(event);
    Ok(true)
}

fn validate_mandate(m: &Mandate, personas: &BTreeSet<PersonaId>) -> Result<()> {
    require(
        m.revision > 0 && text(&m.terms) && m.authority.valid() && m.assumptions.valid(),
        "mandate",
    )?;
    require(
        !m.outcomes.is_empty() && !m.delivery_to.is_empty(),
        "explicit outcomes and delivery recipients",
    )?;
    require(
        m.delivery_to.iter().chain(&m.stakeholders).all(|s| text(s)),
        "recipient identities",
    )?;
    for (id, outcome) in &m.outcomes {
        require(
            text(&id.0) && text(&outcome.description) && outcome.criterion.valid(),
            "required outcome",
        )?;
        require(
            outcome
                .required_reviews
                .iter()
                .all(|r| m.review_roles.contains_key(r)),
            "required review role",
        )?;
    }
    for (id, role) in &m.review_roles {
        require(
            text(&id.0) && text(&role.terms) && role.funding.valid() && role.policy.valid(),
            "review terms",
        )?;
    }
    match &m.decision_rule {
        DecisionRule::PersonaArranged { eligible, .. } => require(
            !eligible.is_empty()
                && eligible.len() <= MAX_ARRANGEMENT_HOLDERS
                && eligible.is_subset(personas),
            "explicit bounded arrangement actor and authority pool",
        ),
        DecisionRule::Designated(p) => {
            require(personas.contains(p), "designated decision identity")
        }
        DecisionRule::Quorum { eligible, minimum } => require(
            *minimum > 0
                && *minimum <= eligible.len()
                && eligible.iter().all(|p| personas.contains(p)),
            "adopted quorum",
        ),
    }
}
fn open(w: &Work) -> Result<()> {
    if matches!(w.phase, Phase::Open) {
        Ok(())
    } else {
        Err(WorkError::Stopped)
    }
}
fn accepted<'a>(w: &'a Work, p: &PersonaId) -> Result<(&'a RecordId, &'a Acceptance)> {
    w.acceptance(p).ok_or(WorkError::NotAccepted)
}
fn check_accountability(w: &Work, a: &Accountability) -> Result<()> {
    match a {
        Accountability::Owner(p) => {
            accepted(w, p)?;
            Ok(())
        }
        Accountability::Gap(s) => require(text(s), "ownership gap"),
    }
}
fn offer(
    w: &mut Work,
    personas: &BTreeSet<PersonaId>,
    id: RecordId,
    to: PersonaId,
    terms: CommitmentTerms,
    from: Option<(PersonaId, RecordId)>,
) -> Result<()> {
    require(text(&id.0) && personas.contains(&to), "offer identities")?;
    arrangements::actor_in_pool(w, &to)?;
    if let DecisionRule::PersonaArranged { delegation, .. } = &w.mandate().decision_rule {
        require(
            terms.outcomes.is_subset(&delegation.allocable_outcomes)
                && terms
                    .review_roles
                    .is_subset(&delegation.allocable_review_roles),
            "offered duties within requester delegation",
        )?;
    }
    if terms.mandate_revision != w.current_mandate {
        return Err(WorkError::Terms);
    }
    require(
        text(&terms.exact_terms)
            && terms
                .outcomes
                .iter()
                .all(|o| w.mandate().outcomes.contains_key(o))
            && terms
                .review_roles
                .iter()
                .all(|r| w.mandate().review_roles.contains_key(r)),
        "commitment scope",
    )?;
    if w.offers.contains_key(&id) || w.acceptances.contains_key(&id) {
        return Err(WorkError::Conflict);
    }
    w.participants.entry(to.clone()).or_default();
    w.offers.insert(
        id,
        Offer {
            to,
            terms,
            from,
            status: OfferStatus::Pending,
        },
    );
    Ok(())
}
// Accepting a commitment does not authorize transfer of separately recorded obligations.
// Refuse affected handoffs without rewriting their historical attribution.
fn check_handoff_accountability(w: &Work, sender: &PersonaId) -> Result<()> {
    // Current dependencies remain available to new waits even after an event arrives.
    if w.dependencies.iter().any(|d| {
        d.key.epoch == w.version.epoch
            && matches!(&d.accountability, Accountability::Owner(owner) if owner == sender)
    }) {
        return Err(WorkError::Unsupported(
            "handoff while a current dependency names the sender as owner",
        ));
    }
    let mut seen = BTreeSet::new();
    for d in w.blocker_dispositions.iter().rev() {
        if seen.insert(&d.finding)
            && d.open
            && matches!(&d.accountability, Accountability::Owner(owner) if owner == sender)
        {
            return Err(WorkError::Unsupported(
                "handoff while an open blocker names the sender as owner",
            ));
        }
    }
    if w.participants.iter().any(|(persona, p)| w.acceptance(persona).is_some()
        && matches!(&p.continuation,
            Some(Continuation::Blocked { accountability: Accountability::Owner(owner), .. }) if owner == sender)) {
        return Err(WorkError::Unsupported("handoff while a blocked continuation names the sender as owner"));
    }
    Ok(())
}
fn accept_offer(
    w: &mut Work,
    by: &Attribution,
    id: RecordId,
    terms: CommitmentTerms,
    arrangement: Option<ExactRef>,
) -> Result<()> {
    if arrangement != w.arrangement_ref() {
        return Err(WorkError::Terms);
    }
    arrangements::actor_in_pool(w, &by.persona)?;
    let o = w
        .offers
        .get(&id)
        .ok_or(WorkError::Missing("offer"))?
        .clone();
    if o.to != by.persona {
        return Err(WorkError::Unauthorized);
    }
    if o.terms != terms || terms.mandate_revision != w.current_mandate {
        return Err(WorkError::Terms);
    }
    if !matches!(o.status, OfferStatus::Pending)
        || w.acceptance(&by.persona).is_some()
        || w.acceptances.contains_key(&id)
    {
        return Err(WorkError::Conflict);
    }
    if let Some((sender, commitment)) = &o.from {
        let (current, a) = accepted(w, sender)?;
        if current != commitment || a.terms != terms || sender == &by.persona {
            return Err(WorkError::Conflict);
        }
        check_handoff_accountability(w, sender)?;
        arrangements::check_legacy_handoff(w, sender, &by.persona)?;
    }
    for p in w
        .participants
        .keys()
        .filter(|p| o.from.as_ref().is_none_or(|(sender, _)| sender != *p))
    {
        if let Some((_, a)) = w.acceptance(p) {
            if !a.terms.outcomes.is_disjoint(&terms.outcomes)
                || !a.terms.review_roles.is_disjoint(&terms.review_roles)
            {
                return Err(WorkError::Conflict);
            }
        }
    }
    if let Some((sender, _)) = &o.from {
        let p = w.participants.get_mut(sender).unwrap();
        p.acceptance = None;
        p.wake = None;
        p.continuation = Some(Continuation::Ended {
            reason: "Exact commitment transferred with recipient acceptance".into(),
        });
        for a in w.artifacts.values_mut().filter(|a| &a.owner == sender) {
            a.owner = by.persona.clone();
        }
    }
    w.acceptances.insert(
        id.clone(),
        Acceptance {
            arrangement,
            terms,
            by: by.clone(),
        },
    );
    w.participants.get_mut(&by.persona).unwrap().acceptance = Some(id.clone());
    w.offers.get_mut(&id).unwrap().status = OfferStatus::Accepted(by.clone());
    Ok(())
}
fn historical_artifact<'a>(w: &'a Work, r: &ArtifactRef) -> Option<&'a ArtifactRevision> {
    w.artifacts
        .get(&r.id)?
        .revisions
        .get(&r.revision)
        .filter(|a| &a.reference == r)
}
fn artifact_current(w: &Work, r: &ArtifactRef) -> bool {
    w.artifacts
        .get(&r.id)
        .is_some_and(|a| a.current == r.revision && historical_artifact(w, r).is_some())
}
fn candidate_current(w: &Work, c: &Candidate) -> bool {
    c.mandate_revision == w.current_mandate
        && !c.artifacts.is_empty()
        && c.artifacts
            .iter()
            .all(|(o, a)| w.mandate().outcomes.contains_key(o) && artifact_current(w, a))
}
fn evidence_valid(
    w: &Work,
    ids: &BTreeSet<RecordId>,
    artifact: &ArtifactRef,
    criterion: &ExactRef,
) -> bool {
    !ids.is_empty()
        && ids.iter().all(|id| {
            w.evidence.get(id).is_some_and(|e| {
                e.availability == Availability::Available
                    && !w.unavailable_evidence.contains_key(id)
                    && e.artifacts.contains(artifact)
                    && e.criteria.contains(criterion)
            })
        })
}
fn assessment_status(w: &Work, a: &Assessment) -> AssessmentStatus {
    let i = &a.input;
    let latest = w.assessments.iter().rev().find(|other| {
        other.input.artifact == i.artifact
            && other.input.criterion == i.criterion
            && other.input.role == i.role
    });
    if !artifact_current(w, &i.artifact)
        || latest.is_none_or(|latest| latest.input.id != i.id)
        || !w.acceptance(&a.by.persona).is_some_and(|(id, role)| {
            id == &a.accepted_role && role.terms.review_roles.contains(&i.role)
        })
    {
        return AssessmentStatus::Stale;
    }
    if !evidence_valid(w, &i.evidence, &i.artifact, &i.criterion)
        || a.snapshot.len() != i.evidence.len()
        || !a.snapshot.iter().all(|e| w.evidence.get(&e.id) == Some(e))
    {
        return AssessmentStatus::Unverifiable;
    }
    match w.mandate().review_roles.get(&i.role) {
        Some(role) if i.passes && a.separation.satisfies(role.independence) => {
            AssessmentStatus::Current
        }
        _ => AssessmentStatus::NotQualified,
    }
}
fn vote_valid(w: &Work, j: &IndividualJudgment, c: &Candidate) -> bool {
    j.input.sufficient
        && j.arrangement == w.arrangement_ref()
        && &j.input.candidate == c
        && w.acceptance(&j.by.persona)
            .is_some_and(|(id, _)| id == &j.acceptance)
        && w.judgments
            .iter()
            .rev()
            .find(|other| other.by.persona == j.by.persona && &other.input.candidate == c)
            .is_some_and(|latest| latest.input.id == j.input.id)
        && w.mandate().outcomes.iter().all(|(outcome, required)| {
            match (j.input.claims.get(outcome), c.artifacts.get(outcome)) {
                (Some(claim), Some(artifact)) => {
                    claim.met && evidence_valid(w, &claim.evidence, artifact, &required.criterion)
                }
                _ => false,
            }
        })
}
fn rule_satisfied(w: &Work, supporters: &BTreeSet<RecordId>, c: &Candidate) -> bool {
    let voters: BTreeSet<_> = w
        .judgments
        .iter()
        .filter(|j| supporters.contains(&j.input.id) && vote_valid(w, j, c))
        .map(|j| j.by.persona.clone())
        .collect();
    w.effective_decision_rule()
        .is_some_and(|rule| rule.satisfied(&voters))
}
fn decision_right(w: &Work, p: &PersonaId) -> Result<()> {
    accepted(w, p)?;
    let permitted = w
        .effective_decision_rule()
        .is_some_and(|rule| rule.holders().contains(p));
    if permitted {
        Ok(())
    } else {
        Err(WorkError::Unauthorized)
    }
}
fn required_assessments(w: &Work, c: &Candidate) -> Result<BTreeSet<RecordId>> {
    if !candidate_current(w, c) || c.artifacts.len() != w.mandate().outcomes.len() {
        return Err(WorkError::Unmet);
    }
    let mut latest_blockers = BTreeMap::new();
    for d in &w.blocker_dispositions {
        latest_blockers.insert(&d.finding, d.open);
    }
    if latest_blockers.values().any(|open| *open) {
        return Err(WorkError::Unmet);
    }
    let mut result = BTreeSet::new();
    for (outcome, spec) in &w.mandate().outcomes {
        if !matches!(w.ownership(outcome), Accountability::Owner(_)) {
            return Err(WorkError::Unmet);
        }
        let artifact = c.artifacts.get(outcome).ok_or(WorkError::Unmet)?;
        for role in &spec.required_reviews {
            let a = w
                .assessments
                .iter()
                .rev()
                .find(|a| {
                    &a.input.artifact == artifact
                        && a.input.criterion == spec.criterion
                        && &a.input.role == role
                })
                .ok_or(WorkError::Unmet)?;
            if assessment_status(w, a) != AssessmentStatus::Current {
                return Err(WorkError::Unmet);
            }
            result.insert(a.input.id.clone());
        }
    }
    Ok(result)
}
fn shared_current(w: &Work, s: &SharedDecision) -> bool {
    s.arrangement == w.arrangement_ref()
        && required_assessments(w, &s.candidate).is_ok_and(|ids| ids == s.assessments)
        && rule_satisfied(w, &s.supporters, &s.candidate)
}
fn release_current(w: &Work, id: &RecordId) -> bool {
    w.releases.get(id).is_some_and(|r| {
        r.arrangement == w.arrangement_ref()
            && r.authority == w.mandate().authority
            && r.blocker_dispositions == w.blocker_dispositions.len()
            && w.shared
                .get(&r.shared)
                .is_some_and(|s| s.candidate == r.candidate && shared_current(w, s))
    })
}
fn closure_ready(w: &Work, release: &RecordId) -> bool {
    release_current(w, release)
        && w.mandate().delivery_to.iter().all(|recipient| {
            w.deliveries
                .iter()
                .any(|d| &d.release == release && &d.recipient == recipient)
        })
        && w.mandate().stakeholders.iter().all(|stakeholder| {
            w.stakeholder_decisions
                .iter()
                .rev()
                .find(|d| &d.release == release && &d.stakeholder == stakeholder)
                .is_some_and(|d| d.accepts)
        })
}
fn apply_command(
    w: &mut Work,
    personas: &BTreeSet<PersonaId>,
    ctx: &DecisionContext,
    by: &Attribution,
    command: Command,
) -> Result<()> {
    match command {
        Command::Accept { offer, exact_terms } => {
            return accept_offer(w, by, offer, exact_terms, None)
        }
        Command::AcceptArrangedOffer {
            offer,
            exact_terms,
            arrangement,
        } => {
            return accept_offer(w, by, offer, exact_terms, Some(arrangement));
        }
        Command::Decline { offer, reason } => {
            require(text(&reason), "decline reason")?;
            let o = w
                .offers
                .get_mut(&offer)
                .ok_or(WorkError::Missing("offer"))?;
            if o.to != by.persona {
                return Err(WorkError::Unauthorized);
            }
            if o.terms.mandate_revision != w.current_mandate {
                return Err(WorkError::Terms);
            }
            if !matches!(o.status, OfferStatus::Pending) {
                return Err(WorkError::Conflict);
            }
            o.status = OfferStatus::Declined {
                by: by.clone(),
                reason,
            };
            return Ok(());
        }
        other => {
            let (acceptance_id, acceptance) = accepted(w, &by.persona)?;
            let acceptance_id = acceptance_id.clone();
            let terms = acceptance.terms.clone();
            match other {
                Command::ProposeArrangement {
                    id,
                    terms,
                    consent_own,
                } => {
                    arrangements::propose(w, by, id, terms, consent_own)?;
                }
                Command::AcceptArrangement { proposal, exact } => {
                    arrangements::consent(w, by, proposal, exact)?;
                }
                Command::DeclineArrangement {
                    proposal,
                    exact,
                    reason,
                } => {
                    arrangements::decline(w, by, proposal, exact, reason)?;
                }
                Command::Invite { id, to, terms } => offer(w, personas, id, to, terms, None)?,
                Command::OfferHandoff { id, to } => {
                    arrangements::check_legacy_handoff(w, &by.persona, &to)?;
                    require(to != by.persona, "distinct handoff recipient")?;
                    offer(
                        w,
                        personas,
                        id,
                        to,
                        terms,
                        Some((by.persona.clone(), acceptance_id)),
                    )?;
                }
                Command::PutArtifact {
                    id,
                    base,
                    bytes,
                    inputs,
                    policy,
                } => {
                    require(
                        text(&id.0) && policy.valid() && inputs.iter().all(ExactRef::valid),
                        "artifact metadata",
                    )?;
                    let revision = if let Some(a) = w.artifacts.get(&id) {
                        if a.owner != by.persona {
                            return Err(WorkError::Unauthorized);
                        }
                        if base.as_ref() != Some(&a.revisions[&a.current].reference) {
                            return Err(WorkError::Conflict);
                        }
                        increment(a.current)?
                    } else {
                        if base.is_some() {
                            return Err(WorkError::Conflict);
                        }
                        1
                    };
                    let reference = ArtifactRef {
                        id: id.clone(),
                        revision,
                        digest: Sha256::digest(&bytes).into(),
                    };
                    let a = w.artifacts.entry(id).or_insert_with(|| Artifact {
                        owner: by.persona.clone(),
                        current: revision,
                        revisions: BTreeMap::new(),
                    });
                    a.current = revision;
                    a.revisions.insert(
                        revision,
                        ArtifactRevision {
                            reference,
                            bytes,
                            producer: by.clone(),
                            inputs,
                            policy,
                        },
                    );
                }
                Command::RecordModelEvidence {
                    id,
                    artifact,
                    outcome,
                    exact,
                } => {
                    require(text(&id.0) && exact.valid(), "model evidence identity")?;
                    require(
                        artifact_current(w, &artifact),
                        "model evidence current artifact",
                    )?;
                    let criterion = w
                        .mandate()
                        .outcomes
                        .get(&outcome)
                        .ok_or(WorkError::Missing("model evidence outcome"))?
                        .criterion
                        .clone();
                    if w.evidence.contains_key(&id) {
                        return Err(WorkError::Conflict);
                    }
                    w.evidence.insert(
                        id.clone(),
                        EvidenceRecord {
                            id,
                            exact,
                            source: SourceKind::ModelJudgment,
                            availability: Availability::Available,
                            artifacts: BTreeSet::from([artifact]),
                            criteria: BTreeSet::from([criterion]),
                        },
                    );
                }
                Command::Finding {
                    id,
                    artifact,
                    text: body,
                    evidence,
                } => {
                    require(text(&id.0) && text(&body), "attributed finding")?;
                    require(
                        artifact
                            .as_ref()
                            .is_none_or(|a| historical_artifact(w, a).is_some())
                            && evidence.iter().all(|e| w.evidence.contains_key(e)),
                        "finding references",
                    )?;
                    if w.findings.contains_key(&id) {
                        return Err(WorkError::Conflict);
                    }
                    w.findings.insert(
                        id.clone(),
                        Finding {
                            id,
                            artifact,
                            text: body,
                            evidence,
                            by: by.clone(),
                        },
                    );
                }
                Command::Assess(input) => {
                    require(
                        text(&input.id.0) && text(&input.explanation),
                        "assessment judgment",
                    )?;
                    require(
                        terms.review_roles.contains(&input.role),
                        "accepted reviewer role",
                    )?;
                    require(
                        w.mandate().outcomes.values().any(|o| {
                            o.criterion == input.criterion
                                && o.required_reviews.contains(&input.role)
                        }),
                        "review criterion and scope",
                    )?;
                    if !artifact_current(w, &input.artifact)
                        || !evidence_valid(w, &input.evidence, &input.artifact, &input.criterion)
                    {
                        return Err(WorkError::Unmet);
                    }
                    if w.assessments.iter().any(|a| a.input.id == input.id) {
                        return Err(WorkError::Conflict);
                    }
                    let producer = &historical_artifact(w, &input.artifact)
                        .unwrap()
                        .producer
                        .persona;
                    let separation = if producer == &by.persona {
                        Separation::SelfAssessment
                    } else {
                        ctx.separation
                            .get(&input.artifact)
                            .cloned()
                            .unwrap_or(Separation::Unknown)
                    };
                    let snapshot = input
                        .evidence
                        .iter()
                        .map(|id| w.evidence[id].clone())
                        .collect();
                    w.assessments.push(Assessment {
                        input,
                        by: by.clone(),
                        accepted_role: acceptance_id,
                        separation,
                        snapshot,
                    });
                }
                Command::Judge(input) => {
                    require(
                        text(&input.id.0)
                            && text(&input.explanation)
                            && candidate_current(w, &input.candidate),
                        "individual judgment",
                    )?;
                    if w.judgments.iter().any(|j| j.input.id == input.id) {
                        return Err(WorkError::Conflict);
                    }
                    for (outcome, claim) in &input.claims {
                        let a = input
                            .candidate
                            .artifacts
                            .get(outcome)
                            .ok_or(WorkError::Invalid("claim subject"))?;
                        require(text(&claim.explanation), "claim explanation")?;
                        require(
                            claim.evidence.iter().all(|id| w.evidence.contains_key(id)),
                            "claim evidence identities",
                        )?;
                        if claim.met
                            && !evidence_valid(
                                w,
                                &claim.evidence,
                                a,
                                &w.mandate().outcomes[outcome].criterion,
                            )
                        {
                            return Err(WorkError::Unmet);
                        }
                    }
                    w.judgments.push(IndividualJudgment {
                        arrangement: w.arrangement_ref(),
                        input,
                        by: by.clone(),
                        acceptance: acceptance_id,
                    });
                }
                Command::DecideShared { id, candidate } => {
                    decision_right(w, &by.persona)?;
                    require(text(&id.0), "shared decision identity")?;
                    if w.shared.contains_key(&id) {
                        return Err(WorkError::Conflict);
                    }
                    let assessments = required_assessments(w, &candidate)?;
                    let supporters = w
                        .judgments
                        .iter()
                        .filter(|j| vote_valid(w, j, &candidate))
                        .map(|j| j.input.id.clone())
                        .collect();
                    if !rule_satisfied(w, &supporters, &candidate) {
                        return Err(WorkError::Unmet);
                    }
                    let judgments = w
                        .judgments
                        .iter()
                        .filter(|j| j.input.candidate == candidate)
                        .map(|j| j.input.id.clone())
                        .collect();
                    w.shared.insert(
                        id,
                        SharedDecision {
                            arrangement: w.arrangement_ref(),
                            candidate,
                            by: by.clone(),
                            judgments,
                            supporters,
                            assessments,
                        },
                    );
                }
                Command::Release { id, shared } => {
                    decision_right(w, &by.persona)?;
                    require(
                        crate::protocol::check_id(&id.0, "release").is_ok(),
                        "release identity",
                    )?;
                    if w.releases.contains_key(&id) {
                        return Err(WorkError::Conflict);
                    }
                    let s = w
                        .shared
                        .get(&shared)
                        .ok_or(WorkError::Missing("shared decision"))?;
                    if !shared_current(w, s) {
                        return Err(WorkError::Unmet);
                    }
                    let r = Release {
                        arrangement: w.arrangement_ref(),
                        shared: shared.clone(),
                        candidate: s.candidate.clone(),
                        authority: w.mandate().authority.clone(),
                        by: by.clone(),
                        blocker_dispositions: w.blocker_dispositions.len(),
                    };
                    w.releases.insert(id, r);
                }
                Command::CloseGoal { release } => {
                    decision_right(w, &by.persona)?;
                    if !closure_ready(w, &release) {
                        return Err(WorkError::Unmet);
                    }
                    w.completions.push(Completion {
                        release,
                        by: by.clone(),
                    });
                }
                Command::Accept { .. }
                | Command::AcceptArrangedOffer { .. }
                | Command::Decline { .. } => unreachable!(),
            }
        }
    }
    Ok(())
}
fn set_continuation(
    w: &mut Work,
    persona: &PersonaId,
    continuation: Continuation,
    now: u64,
) -> Result<()> {
    let has_responsibility = w.acceptance(persona).is_some();
    let mut wake = None;
    match &continuation {
        Continuation::Ended { reason } => {
            require(text(reason), "ending explanation")?;
            if has_responsibility {
                return Err(WorkError::Unauthorized);
            }
        }
        _ if !has_responsibility || !matches!(w.phase, Phase::Open) => {
            return Err(WorkError::NotAccepted)
        }
        Continuation::Immediate { purpose } => require(text(purpose), "immediate purpose")?,
        Continuation::ReconsiderAt {
            not_before,
            purpose,
        } => {
            require(
                *not_before > now && *not_before <= i64::MAX as u64 && text(purpose),
                "future bounded reconsideration and purpose",
            )?;
        }
        Continuation::Wait {
            dependency,
            deadline,
            purpose,
        } => {
            require(*deadline > now && text(purpose), "bounded wait and purpose")?;
            wake = registered_wait_wake(w, persona, std::iter::once(dependency))?;
        }
        Continuation::WaitAny {
            dependencies,
            deadline,
            purpose,
        } => {
            require(
                (2..=MAX_WAIT_DEPENDENCIES).contains(&dependencies.len())
                    && *deadline > now
                    && text(purpose),
                "bounded wait set and purpose",
            )?;
            require(
                dependencies.iter().map(|d| &d.id).collect::<BTreeSet<_>>().len()
                    == dependencies.len(),
                "distinct wait dependency identities",
            )?;
            wake = registered_wait_wake(w, persona, dependencies.iter())?;
        }
        Continuation::Blocked {
            reason,
            accountability,
            reconsider_at,
        } => {
            require(
                text(reason) && *reconsider_at > now,
                "bounded accountable block",
            )?;
            check_accountability(w, accountability)?;
        }
        Continuation::Paused { reason } => require(text(reason), "explicit pause")?,
    }
    let p = w
        .participants
        .get_mut(persona)
        .ok_or(WorkError::Missing("participant"))?;
    p.continuation = Some(continuation);
    p.wake = wake;
    Ok(())
}
fn registered_wait_wake<'a>(
    w: &Work,
    persona: &PersonaId,
    dependencies: impl Iterator<Item = &'a ExactRef>,
) -> Result<Option<Wake>> {
    let mut wake = None;
    for dependency in dependencies {
        let key = EventKey {
            dependency: dependency.clone(),
            epoch: w.version.epoch,
        };
        let registered = w.dependencies.iter()
            .find(|d| d.key == key)
            .ok_or(WorkError::Invalid("actual registered dependency"))?;
        check_accountability(w, &registered.accountability)?;
        if w.events.iter().any(|e| e.key == key)
            && event_permitted(w, &key, persona)
            && !w.participants[persona].observed_events.contains(&key)
        {
            wake = Some(Wake::Event(key));
        }
    }
    Ok(wake)
}
fn end_continuations(w: &mut Work, reason: &str) {
    for p in w.participants.values_mut() {
        p.continuation = Some(Continuation::Ended {
            reason: reason.to_owned(),
        });
        p.wake = None;
    }
}
fn eligibility(w: &Work, persona: &PersonaId, now: u64) -> Option<OpportunityCause> {
    if !matches!(w.phase, Phase::Open) || w.acceptance(persona).is_none() {
        return None;
    }
    let p = &w.participants[persona];
    match p.continuation.as_ref()? {
        Continuation::Immediate { .. } => Some(OpportunityCause::Immediate),
        Continuation::ReconsiderAt { not_before, .. } => match &p.wake {
            Some(Wake::ExplicitResume) => Some(OpportunityCause::ExplicitResume),
            _ if now >= *not_before => Some(OpportunityCause::Reconsideration),
            _ => None,
        },
        Continuation::Wait { deadline, .. } | Continuation::WaitAny { deadline, .. } => match &p.wake {
            Some(Wake::Event(key))
                if key.epoch == w.version.epoch
                    && p.continuation.as_ref()
                        .is_some_and(|next| next.waits_for(&key.dependency))
                    && event_permitted(w, key, persona)
                    && !p.observed_events.contains(key) =>
            {
                Some(OpportunityCause::Event(key.clone()))
            }
            Some(Wake::ExplicitResume) => Some(OpportunityCause::ExplicitResume),
            _ => pending_wait_event_keys(w, persona).into_iter().next()
                .map(OpportunityCause::Event)
                .or_else(|| (now >= *deadline).then_some(OpportunityCause::Deadline)),
        },
        Continuation::Blocked { reconsider_at, .. } if now >= *reconsider_at => {
            Some(OpportunityCause::Reconsideration)
        }
        Continuation::Paused { .. } if p.wake == Some(Wake::ExplicitResume) => {
            Some(OpportunityCause::ExplicitResume)
        }
        _ => None,
    }
}
