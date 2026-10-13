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
//! Live mandate amendments after any acceptance are rejected. End the old mandate and
//! explicitly authorize new work instead. Partial transfers, automatic retries, arbitrary
//! imports, effect dispatch, and semantic correctness verification are unsupported.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

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
pub enum DecisionRule {
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
    Wait {
        dependency: ExactRef,
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
    /// Historical authorized closure; `completion_supported` checks present applicability.
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
    pub candidate: Candidate,
    pub by: Attribution,
    pub judgments: Vec<RecordId>,
    pub supporters: BTreeSet<RecordId>,
    pub assessments: BTreeSet<RecordId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub shared: RecordId,
    pub candidate: Candidate,
    pub authority: ExactRef,
    pub by: Attribution,
    /// Exact immutable prefix of Work::blocker_dispositions bound at release.
    pub blocker_dispositions: usize,
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
    pub artifacts: BTreeMap<ArtifactId, Artifact>,
    pub evidence: BTreeMap<RecordId, EvidenceRecord>,
    pub unavailable_evidence: BTreeMap<RecordId, String>,
    pub assessments: Vec<Assessment>,
    pub judgments: Vec<IndividualJudgment>,
    pub findings: BTreeMap<RecordId, Finding>,
    pub blocker_dispositions: Vec<BlockerDisposition>,
    pub shared: BTreeMap<RecordId, SharedDecision>,
    pub releases: BTreeMap<RecordId, Release>,
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
    Decline {
        offer: RecordId,
        reason: String,
    },
    OfferHandoff {
        id: RecordId,
        to: PersonaId,
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
pub enum RuntimeOperation {
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
            artifacts: BTreeMap::new(),
            evidence: BTreeMap::new(),
            unavailable_evidence: BTreeMap::new(),
            assessments: Vec::new(),
            judgments: Vec::new(),
            findings: BTreeMap::new(),
            blocker_dispositions: Vec::new(),
            shared: BTreeMap::new(),
            releases: BTreeMap::new(),
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
            for command in response.commands {
                open(w)?;
                apply_command(w, &personas, ctx, &by, command)?;
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
                    w.phase = Phase::Open;
                    for p in w.participants.values_mut() {
                        if p.acceptance.is_some()
                            && matches!(
                                p.continuation,
                                Some(Continuation::Paused { .. } | Continuation::Wait { .. })
                            )
                        {
                            p.wake = Some(Wake::ExplicitResume);
                        }
                    }
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
    pub fn completion_supported(&self, work: &WorkId) -> bool {
        self.work(work).ok().is_some_and(|w| match &w.phase {
            Phase::Completed { release, .. } => closure_ready(w, release),
            _ => false,
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
                && matches!(&participant.continuation,
                    Some(Continuation::Wait { dependency, .. }) if dependency == &event.key.dependency)
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
    if w.offers.contains_key(&id) {
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
) -> Result<()> {
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
    if !matches!(o.status, OfferStatus::Pending) || w.acceptance(&by.persona).is_some() {
        return Err(WorkError::Conflict);
    }
    if let Some((sender, commitment)) = &o.from {
        let (current, a) = accepted(w, sender)?;
        if current != commitment || a.terms != terms || sender == &by.persona {
            return Err(WorkError::Conflict);
        }
        check_handoff_accountability(w, sender)?;
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
    match &w.mandate().decision_rule {
        DecisionRule::Designated(p) => voters.contains(p),
        DecisionRule::Quorum { eligible, minimum } => {
            voters.intersection(eligible).count() >= *minimum
        }
    }
}
fn decision_right(w: &Work, p: &PersonaId) -> Result<()> {
    accepted(w, p)?;
    let permitted = match &w.mandate().decision_rule {
        DecisionRule::Designated(owner) => p == owner,
        DecisionRule::Quorum { eligible, .. } => eligible.contains(p),
    };
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
    required_assessments(w, &s.candidate).is_ok_and(|ids| ids == s.assessments)
        && rule_satisfied(w, &s.supporters, &s.candidate)
}
fn release_current(w: &Work, id: &RecordId) -> bool {
    w.releases.get(id).is_some_and(|r| {
        r.authority == w.mandate().authority
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
        Command::Accept { offer, exact_terms } => return accept_offer(w, by, offer, exact_terms),
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
                Command::Invite { id, to, terms } => offer(w, personas, id, to, terms, None)?,
                Command::OfferHandoff { id, to } => {
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
                    require(text(&id.0), "release identity")?;
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
                    end_continuations(w, "Authorized accepted-goal closure");
                    w.version.epoch = increment(w.version.epoch)?;
                    w.phase = Phase::Completed {
                        release,
                        by: by.clone(),
                    };
                }
                Command::Accept { .. } | Command::Decline { .. } => unreachable!(),
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
            if has_responsibility && !matches!(w.phase, Phase::Completed { .. }) {
                return Err(WorkError::Unauthorized);
            }
        }
        _ if !has_responsibility || !matches!(w.phase, Phase::Open) => {
            return Err(WorkError::NotAccepted)
        }
        Continuation::Immediate { purpose } => require(text(purpose), "immediate purpose")?,
        Continuation::Wait {
            dependency,
            deadline,
            purpose,
        } => {
            require(*deadline > now && text(purpose), "bounded wait and purpose")?;
            let key = EventKey {
                dependency: dependency.clone(),
                epoch: w.version.epoch,
            };
            let registered = w
                .dependencies
                .iter()
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
        Continuation::Wait { deadline, .. } => match &p.wake {
            Some(Wake::Event(key))
                if key.epoch == w.version.epoch
                    && event_permitted(w, key, persona)
                    && !p.observed_events.contains(key) =>
            {
                Some(OpportunityCause::Event(key.clone()))
            }
            Some(Wake::ExplicitResume) => Some(OpportunityCause::ExplicitResume),
            _ if now >= *deadline => Some(OpportunityCause::Deadline),
            _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    fn set<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeSet<T> {
        items.into_iter().collect()
    }
    fn r(s: &str) -> ExactRef {
        ExactRef::new(s, 1, s.as_bytes())
    }
    fn ready() -> Continuation {
        Continuation::Immediate {
            purpose: "Continue accepted scope".into(),
        }
    }
    fn ended() -> Continuation {
        Continuation::Ended {
            reason: "Explicit disposition".into(),
        }
    }
    fn runtime(b: &WorkBoard, right: RuntimeRight) -> RuntimeContext {
        let w = b.work(&"w".into()).unwrap();
        RuntimeContext::from_runtime(
            "w".into(),
            w.version,
            w.mandate().authority.clone(),
            Origin::Human("h".into()),
            right,
        )
    }
    fn host(b: &mut WorkBoard, right: RuntimeRight, op: RuntimeOperation) -> Result<()> {
        b.observe(&runtime(b, right), op)
    }
    fn context(b: &WorkBoard, p: &str) -> DecisionContext {
        let w = b.work(&"w".into()).unwrap();
        DecisionContext::from_runtime(
            "w".into(),
            p.into(),
            InvocationId(format!("{p}-{}", w.version.revision)),
            w.version,
            w.mandate().authority.clone(),
            10,
            BTreeMap::new(),
            BTreeSet::new(),
        )
    }
    fn reply(
        b: &mut WorkBoard,
        p: &str,
        commands: Vec<Command>,
        continuation: Continuation,
    ) -> Result<()> {
        b.apply_response(
            &context(b, p),
            DecisionResponse {
                commands,
                continuation,
            },
        )
    }
    fn terms(outcomes: bool, review: bool) -> CommitmentTerms {
        CommitmentTerms {
            mandate_revision: 1,
            exact_terms: "Exact offered responsibility".into(),
            outcomes: if outcomes {
                set(["o".into()])
            } else {
                BTreeSet::new()
            },
            review_roles: if review {
                set(["r".into()])
            } else {
                BTreeSet::new()
            },
        }
    }
    fn invite(b: &mut WorkBoard, id: &str, p: &str, t: CommitmentTerms) {
        host(
            b,
            RuntimeRight::Governance,
            RuntimeOperation::Invite {
                id: id.into(),
                to: p.into(),
                terms: t,
            },
        )
        .unwrap();
    }
    fn accept(b: &mut WorkBoard, id: &str, p: &str, t: CommitmentTerms) {
        reply(
            b,
            p,
            vec![Command::Accept {
                offer: id.into(),
                exact_terms: t,
            }],
            ready(),
        )
        .unwrap();
    }
    fn board(review: Option<Independence>, stakeholders: bool) -> WorkBoard {
        let mut b = WorkBoard::default();
        for p in ["a", "b", "c"] {
            b.register_persona(p.into()).unwrap();
        }
        let mandate = Mandate {
            revision: 1,
            terms: "Accepted scope".into(),
            authority: r("authority"),
            assumptions: r("assumptions"),
            outcomes: BTreeMap::from([(
                "o".into(),
                RequiredOutcome {
                    description: "Required result".into(),
                    criterion: r("criterion"),
                    required_reviews: if review.is_some() {
                        set(["r".into()])
                    } else {
                        BTreeSet::new()
                    },
                },
            )]),
            review_roles: review
                .map(|independence| {
                    BTreeMap::from([(
                        "r".into(),
                        ReviewRole {
                            terms: "Exact review scope".into(),
                            funding: r("funding"),
                            policy: r("policy"),
                            independence,
                        },
                    )])
                })
                .unwrap_or_default(),
            decision_rule: DecisionRule::Designated("a".into()),
            delivery_to: set(["h".into()]),
            stakeholders: if stakeholders {
                set(["h".into()])
            } else {
                BTreeSet::new()
            },
        };
        let ctx = RuntimeContext::from_runtime(
            "w".into(),
            Version {
                revision: 0,
                epoch: 0,
            },
            r("authority"),
            Origin::Human("h".into()),
            RuntimeRight::NewAuthorization,
        );
        b.create(
            &ctx,
            Need {
                origin: Origin::Human("h".into()),
                request: "Original exact need".into(),
                supplied: BTreeSet::new(),
                boundaries: r("bounds"),
            },
            mandate,
        )
        .unwrap();
        b
    }
    fn active(review: Option<Independence>, stakeholders: bool) -> WorkBoard {
        let mut b = board(review, stakeholders);
        invite(&mut b, "oa", "a", terms(true, false));
        accept(&mut b, "oa", "a", terms(true, false));
        invite(&mut b, "ob", "b", terms(false, review.is_some()));
        accept(&mut b, "ob", "b", terms(false, review.is_some()));
        b
    }
    fn artifact(b: &mut WorkBoard) -> Candidate {
        reply(
            b,
            "a",
            vec![Command::PutArtifact {
                id: "x".into(),
                base: None,
                bytes: b"v1".to_vec(),
                inputs: BTreeSet::new(),
                policy: r("policy"),
            }],
            ready(),
        )
        .unwrap();
        let a = b.work(&"w".into()).unwrap().artifacts[&ArtifactId::from("x")].revisions[&1]
            .reference
            .clone();
        host(
            b,
            RuntimeRight::Observation,
            RuntimeOperation::Evidence(EvidenceRecord {
                id: "e".into(),
                exact: r("observation"),
                source: SourceKind::RuntimeObservation,
                availability: Availability::Available,
                artifacts: set([a.clone()]),
                criteria: set([r("criterion")]),
            }),
        )
        .unwrap();
        Candidate {
            mandate_revision: 1,
            artifacts: BTreeMap::from([("o".into(), a)]),
        }
    }
    fn judge(b: &mut WorkBoard, c: &Candidate, p: &str, id: &str, sufficient: bool) {
        reply(
            b,
            p,
            vec![Command::Judge(JudgmentInput {
                id: id.into(),
                candidate: c.clone(),
                sufficient,
                claims: BTreeMap::from([(
                    "o".into(),
                    Claim {
                        met: sufficient,
                        explanation: "Scoped judgment of observed result".into(),
                        evidence: set(["e".into()]),
                    },
                )]),
                explanation: "Attributed sufficiency judgment".into(),
            })],
            ready(),
        )
        .unwrap();
    }
    fn assess(b: &mut WorkBoard, c: &Candidate, known: bool) {
        let a = c.artifacts[&OutcomeId::from("o")].clone();
        let mut ctx = context(b, "b");
        if known {
            ctx.separation.insert(
                a.clone(),
                Separation::Known {
                    distinct_actor: true,
                    distinct_execution: true,
                    distinct_model: false,
                    qualified_outside: false,
                    conflicts: BTreeSet::new(),
                    attestation: r("separation"),
                },
            );
        }
        b.apply_response(
            &ctx,
            DecisionResponse {
                commands: vec![Command::Assess(AssessmentInput {
                    id: "assessment".into(),
                    artifact: a,
                    criterion: r("criterion"),
                    role: "r".into(),
                    evidence: set(["e".into()]),
                    passes: true,
                    explanation: "Reasoned assessment under accepted criterion".into(),
                    limitations: vec!["Limited to this exact subject".into()],
                })],
                continuation: ready(),
            },
        )
        .unwrap();
    }
    fn release(b: &mut WorkBoard, c: &Candidate) -> Result<()> {
        reply(
            b,
            "a",
            vec![
                Command::DecideShared {
                    id: "shared".into(),
                    candidate: c.clone(),
                },
                Command::Release {
                    id: "release".into(),
                    shared: "shared".into(),
                },
            ],
            ready(),
        )
    }
    fn deliver(b: &mut WorkBoard) {
        host(
            b,
            RuntimeRight::Observation,
            RuntimeOperation::Delivered {
                id: "delivery".into(),
                release: "release".into(),
                recipient: "h".into(),
                receipt: r("delivery"),
            },
        )
        .unwrap();
    }
    #[test]
    fn invitation_is_not_acceptance_and_decline_is_legitimate() {
        let mut b = board(None, false);
        invite(&mut b, "offer", "a", terms(true, false));
        assert!(b
            .work(&"w".into())
            .unwrap()
            .acceptance(&"a".into())
            .is_none());
        assert!(b.next_eligible(None, &BTreeSet::new(), 10).is_none());
        reply(
            &mut b,
            "a",
            vec![Command::Decline {
                offer: "offer".into(),
                reason: "Declined".into(),
            }],
            ended(),
        )
        .unwrap();
        assert!(matches!(
            b.work(&"w".into()).unwrap().ownership(&"o".into()),
            Accountability::Gap(_)
        ));
    }
    #[test]
    fn forged_acceptance_and_stale_terms_are_rejected_without_mutation() {
        let mut b = board(None, false);
        invite(&mut b, "offer", "a", terms(true, false));
        invite(&mut b, "other", "b", terms(false, false));
        let saved = b.clone();
        assert_eq!(
            reply(
                &mut b,
                "b",
                vec![Command::Accept {
                    offer: "offer".into(),
                    exact_terms: terms(true, false)
                }],
                ready()
            ),
            Err(WorkError::Unauthorized)
        );
        assert_eq!(b, saved);
        let mut m = b.work(&"w".into()).unwrap().mandate().clone();
        m.revision = 2;
        m.terms = "Changed scope".into();
        host(
            &mut b,
            RuntimeRight::Governance,
            RuntimeOperation::ReviseUnacceptedMandate(m),
        )
        .unwrap();
        let saved = b.clone();
        assert_eq!(
            reply(
                &mut b,
                "a",
                vec![Command::Accept {
                    offer: "offer".into(),
                    exact_terms: terms(true, false)
                }],
                ready()
            ),
            Err(WorkError::Terms)
        );
        assert_eq!(b, saved);
    }
    #[test]
    fn stale_revision_and_epoch_precede_mutation() {
        let mut b = active(None, false);
        let stale = context(&b, "a");
        reply(&mut b, "b", vec![], ready()).unwrap();
        let saved = b.clone();
        assert!(matches!(
            b.apply_response(
                &stale,
                DecisionResponse {
                    commands: vec![],
                    continuation: ready()
                }
            ),
            Err(WorkError::StaleRevision { .. })
        ));
        assert_eq!(b, saved);
        let mut wrong_epoch = context(&b, "a");
        wrong_epoch.version.epoch += 1;
        assert!(matches!(
            b.apply_response(
                &wrong_epoch,
                DecisionResponse {
                    commands: vec![],
                    continuation: ready()
                }
            ),
            Err(WorkError::StaleEpoch { .. })
        ));
        assert_eq!(b, saved);
    }
    #[test]
    fn dissent_is_preserved_without_automatic_veto() {
        let mut b = active(None, false);
        let c = artifact(&mut b);
        judge(&mut b, &c, "a", "yes", true);
        judge(&mut b, &c, "b", "no", false);
        reply(
            &mut b,
            "b",
            vec![Command::Finding {
                id: "issue".into(),
                artifact: None,
                text: "Unadopted concern".into(),
                evidence: BTreeSet::new(),
            }],
            ready(),
        )
        .unwrap();
        release(&mut b, &c).unwrap();
        let w = b.work(&"w".into()).unwrap();
        assert!(w.shared[&RecordId::from("shared")]
            .judgments
            .contains(&"no".into()));
        assert!(w.findings.contains_key(&"issue".into()));
        assert!(w.blocker_dispositions.is_empty());
    }
    #[test]
    fn individual_sufficiency_is_not_shared_closure() {
        let mut b = active(None, false);
        let c = artifact(&mut b);
        judge(&mut b, &c, "a", "yes", true);
        assert!(b.work(&"w".into()).unwrap().shared.is_empty());
        assert!(reply(
            &mut b,
            "a",
            vec![Command::CloseGoal {
                release: "absent".into()
            }],
            ended()
        )
        .is_err());
        assert!(matches!(b.work(&"w".into()).unwrap().phase, Phase::Open));
    }
    #[test]
    fn delivery_is_not_stakeholder_acceptance_and_positive_closure_works() {
        let mut b = active(None, true);
        let c = artifact(&mut b);
        judge(&mut b, &c, "a", "yes", true);
        release(&mut b, &c).unwrap();
        deliver(&mut b);
        assert_eq!(
            reply(
                &mut b,
                "a",
                vec![Command::CloseGoal {
                    release: "release".into()
                }],
                ended()
            ),
            Err(WorkError::Unmet)
        );
        host(
            &mut b,
            RuntimeRight::StakeholderAcceptance,
            RuntimeOperation::Stakeholder {
                release: "release".into(),
                accepts: true,
                receipt: r("acceptance"),
            },
        )
        .unwrap();
        reply(
            &mut b,
            "a",
            vec![Command::CloseGoal {
                release: "release".into(),
            }],
            ended(),
        )
        .unwrap();
        assert!(b.completion_supported(&"w".into()));
    }
    #[test]
    fn changed_artifact_stales_assessment_without_changing_history() {
        let mut b = active(Some(Independence::DistinctExecution), false);
        let c = artifact(&mut b);
        assess(&mut b, &c, true);
        let old = b.work(&"w".into()).unwrap().assessments[0].clone();
        judge(&mut b, &c, "a", "yes", true);
        release(&mut b, &c).unwrap();
        assert!(b.release_current(&"w".into(), &"release".into()));
        reply(
            &mut b,
            "a",
            vec![Command::PutArtifact {
                id: "x".into(),
                base: Some(c.artifacts[&OutcomeId::from("o")].clone()),
                bytes: b"v2".to_vec(),
                inputs: BTreeSet::new(),
                policy: r("policy"),
            }],
            ready(),
        )
        .unwrap();
        assert_eq!(
            b.assessment_status(&"w".into(), &"assessment".into())
                .unwrap(),
            AssessmentStatus::Stale
        );
        assert_eq!(b.work(&"w".into()).unwrap().assessments[0], old);
        assert!(!b.release_current(&"w".into(), &"release".into()));
    }
    #[test]
    fn unknown_review_separation_is_not_independent() {
        let mut b = active(Some(Independence::DistinctActor), false);
        let c = artifact(&mut b);
        assess(&mut b, &c, false);
        judge(&mut b, &c, "a", "yes", true);
        assert_eq!(
            b.assessment_status(&"w".into(), &"assessment".into())
                .unwrap(),
            AssessmentStatus::NotQualified
        );
        assert_eq!(release(&mut b, &c), Err(WorkError::Unmet));
    }
    #[test]
    fn missing_evidence_and_later_unavailability_cannot_pass() {
        let mut b = active(Some(Independence::NotRequired), false);
        let c = artifact(&mut b);
        let a = c.artifacts[&OutcomeId::from("o")].clone();
        let bad = AssessmentInput {
            id: "bad".into(),
            artifact: a,
            criterion: r("criterion"),
            role: "r".into(),
            evidence: set(["invented".into()]),
            passes: true,
            explanation: "Commands succeeded".into(),
            limitations: vec![],
        };
        let saved = b.clone();
        assert_eq!(
            reply(&mut b, "b", vec![Command::Assess(bad)], ready()),
            Err(WorkError::Unmet)
        );
        assert_eq!(b, saved);
        assess(&mut b, &c, false);
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::EvidenceUnavailable {
                id: "e".into(),
                reason: "No longer available".into(),
            },
        )
        .unwrap();
        assert_eq!(
            b.assessment_status(&"w".into(), &"assessment".into())
                .unwrap(),
            AssessmentStatus::Unverifiable
        );
    }
    fn dependency(b: &mut WorkBoard) -> EventKey {
        let key = EventKey {
            dependency: r("dependency"),
            epoch: b.work(&"w".into()).unwrap().version.epoch,
        };
        host(
            b,
            RuntimeRight::Observation,
            RuntimeOperation::RegisterDependency(Dependency {
                key: key.clone(),
                accountability: Accountability::Owner("b".into()),
            }),
        )
        .unwrap();
        key
    }
    fn wait_for(key: &EventKey) -> Continuation {
        Continuation::Wait {
            dependency: key.dependency.clone(),
            deadline: 30,
            purpose: "Await the exact dependency".into(),
        }
    }
    #[test]
    fn wait_registration_has_no_lost_wake_on_either_side_and_replay_is_not_fresh() {
        for before in [false, true] {
            let mut b = active(None, false);
            let key = dependency(&mut b);
            let event = Event {
                key: key.clone(),
                observation: r("event"),
            };
            if before {
                host(
                    &mut b,
                    RuntimeRight::Observation,
                    RuntimeOperation::Event(event.clone()),
                )
                .unwrap();
            }
            reply(&mut b, "a", vec![], wait_for(&key)).unwrap();
            if !before {
                host(
                    &mut b,
                    RuntimeRight::Observation,
                    RuntimeOperation::Event(event.clone()),
                )
                .unwrap();
            }
            let recovered = b.clone();
            assert_eq!(
                recovered
                    .participant(&"w".into(), &"a".into())
                    .unwrap()
                    .continuation,
                Some(wait_for(&key))
            );
            assert_eq!(
                eligibility(recovered.work(&"w".into()).unwrap(), &"a".into(), 10),
                Some(OpportunityCause::Event(key.clone()))
            );
            let mut ctx = context(&b, "a");
            ctx.observed.insert(key.clone());
            b.apply_response(
                &ctx,
                DecisionResponse {
                    commands: vec![],
                    continuation: wait_for(&key),
                },
            )
            .unwrap();
            host(
                &mut b,
                RuntimeRight::Observation,
                RuntimeOperation::Event(event),
            )
            .unwrap();
            assert_eq!(
                eligibility(b.work(&"w".into()).unwrap(), &"a".into(), 10),
                None
            );
        }
    }
    #[test]
    fn event_between_inspection_and_registration_rejects_stale_batch() {
        let mut b = active(None, false);
        let key = dependency(&mut b);
        let ctx = context(&b, "a");
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(Event {
                key: key.clone(),
                observation: r("event"),
            }),
        )
        .unwrap();
        let saved = b.clone();
        assert!(matches!(
            b.apply_response(
                &ctx,
                DecisionResponse {
                    commands: vec![],
                    continuation: wait_for(&key)
                }
            ),
            Err(WorkError::StaleRevision { .. })
        ));
        assert_eq!(b, saved);
        reply(&mut b, "a", vec![], wait_for(&key)).unwrap();
        assert!(b
            .participant(&"w".into(), &"a".into())
            .unwrap()
            .wake
            .is_some());
    }
    #[test]
    fn resource_block_and_everyone_paused_are_not_completion() {
        let mut b = active(None, false);
        reply(
            &mut b,
            "a",
            vec![],
            Continuation::Blocked {
                reason: "Resources exhausted".into(),
                accountability: Accountability::Gap("No accepted capacity owner".into()),
                reconsider_at: 40,
            },
        )
        .unwrap();
        reply(
            &mut b,
            "b",
            vec![],
            Continuation::Paused {
                reason: "Explicit pause".into(),
            },
        )
        .unwrap();
        assert!(b.next_eligible(None, &BTreeSet::new(), 10).is_none());
        assert!(matches!(b.work(&"w".into()).unwrap().phase, Phase::Open));
        assert_eq!(
            reply(&mut b, "a", vec![], ended()),
            Err(WorkError::Unauthorized)
        );
        host(
            &mut b,
            RuntimeRight::Governance,
            RuntimeOperation::End {
                kind: EndingKind::Unfinished,
                reason: "Authorized unfinished ending".into(),
            },
        )
        .unwrap();
        assert!(!b.completion_supported(&"w".into()));
    }
    #[test]
    fn canceled_work_is_not_revived_by_resume_or_late_events() {
        let mut b = active(None, false);
        let key = dependency(&mut b);
        reply(&mut b, "a", vec![], wait_for(&key)).unwrap();
        host(
            &mut b,
            RuntimeRight::Governance,
            RuntimeOperation::End {
                kind: EndingKind::Canceled,
                reason: "Canceled by authority".into(),
            },
        )
        .unwrap();
        assert_eq!(
            host(&mut b, RuntimeRight::Governance, RuntimeOperation::Resume),
            Err(WorkError::Stopped)
        );
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(Event {
                key,
                observation: r("late"),
            }),
        )
        .unwrap();
        assert!(b.next_eligible(None, &BTreeSet::new(), 100).is_none());
        assert!(b
            .participant(&"w".into(), &"a".into())
            .unwrap()
            .wake
            .is_none());
        let mut mandate = b.work(&"w".into()).unwrap().mandate().clone();
        mandate.authority = r("new-authorization");
        let ctx = RuntimeContext::from_runtime(
            "new".into(),
            Version {
                revision: 0,
                epoch: 0,
            },
            mandate.authority.clone(),
            Origin::Human("h".into()),
            RuntimeRight::NewAuthorization,
        );
        b.reauthorize(&ctx, &"w".into(), mandate).unwrap();
        assert!(b.work(&"new".into()).unwrap().acceptances.is_empty());
        assert!(matches!(
            b.work(&"w".into()).unwrap().phase,
            Phase::Ended { .. }
        ));
    }
    #[test]
    fn valid_handoff_race_has_one_winner_and_offer_keeps_sender_responsible() {
        let mut b = board(None, false);
        invite(&mut b, "oa", "a", terms(true, false));
        accept(&mut b, "oa", "a", terms(true, false));
        reply(
            &mut b,
            "a",
            vec![
                Command::OfferHandoff {
                    id: "to-b".into(),
                    to: "b".into(),
                },
                Command::OfferHandoff {
                    id: "to-c".into(),
                    to: "c".into(),
                },
            ],
            ready(),
        )
        .unwrap();
        assert_eq!(
            b.work(&"w".into()).unwrap().ownership(&"o".into()),
            Accountability::Owner("a".into())
        );
        let ctx_b = context(&b, "b");
        let ctx_c = context(&b, "c");
        b.apply_response(
            &ctx_b,
            DecisionResponse {
                commands: vec![Command::Accept {
                    offer: "to-b".into(),
                    exact_terms: terms(true, false),
                }],
                continuation: ready(),
            },
        )
        .unwrap();
        let saved = b.clone();
        assert!(matches!(
            b.apply_response(
                &ctx_c,
                DecisionResponse {
                    commands: vec![Command::Accept {
                        offer: "to-c".into(),
                        exact_terms: terms(true, false)
                    }],
                    continuation: ready()
                }
            ),
            Err(WorkError::StaleRevision { .. })
        ));
        assert_eq!(b, saved);
        assert!(reply(
            &mut b,
            "c",
            vec![Command::Accept {
                offer: "to-c".into(),
                exact_terms: terms(true, false)
            }],
            ready()
        )
        .is_err());
        assert_eq!(
            b.work(&"w".into()).unwrap().ownership(&"o".into()),
            Accountability::Owner("b".into())
        );
    }
    #[test]
    fn unchanged_qualified_result_survives_activity_and_round_robin_is_deterministic() {
        let mut b = active(Some(Independence::DistinctExecution), false);
        let c = artifact(&mut b);
        assess(&mut b, &c, true);
        judge(&mut b, &c, "a", "yes", true);
        release(&mut b, &c).unwrap();
        reply(&mut b, "b", vec![], ready()).unwrap();
        assert!(b.release_current(&"w".into(), &"release".into()));
        let first = b.next_eligible(None, &BTreeSet::new(), 10).unwrap();
        let cursor = (first.work.clone(), first.persona.clone());
        let second = b
            .next_eligible(Some(&cursor), &BTreeSet::new(), 10)
            .unwrap();
        assert_ne!(first.persona, second.persona);
        let wrapped = b
            .next_eligible(Some(&(second.work, second.persona)), &BTreeSet::new(), 10)
            .unwrap();
        assert_eq!(wrapped.persona, first.persona);
        deliver(&mut b);
        reply(
            &mut b,
            "a",
            vec![Command::CloseGoal {
                release: "release".into(),
            }],
            ended(),
        )
        .unwrap();
        assert!(b.completion_supported(&"w".into()));
    }
    #[test]
    fn changed_blocker_history_never_revives_an_accepted_release() {
        let mut b = active(None, true);
        let c = artifact(&mut b);
        judge(&mut b, &c, "a", "yes", true);
        release(&mut b, &c).unwrap();
        deliver(&mut b);
        host(
            &mut b,
            RuntimeRight::StakeholderAcceptance,
            RuntimeOperation::Stakeholder {
                release: "release".into(),
                accepts: true,
                receipt: r("acceptance"),
            },
        )
        .unwrap();
        let original = b.work(&"w".into()).unwrap().clone();
        assert!(closure_ready(&original, &"release".into()));
        reply(
            &mut b,
            "b",
            vec![Command::Finding {
                id: "later-issue".into(),
                artifact: None,
                text: "A concern adopted after delivery and acceptance".into(),
                evidence: BTreeSet::new(),
            }],
            ready(),
        )
        .unwrap();
        for is_open in [true, false] {
            host(
                &mut b,
                RuntimeRight::Governance,
                RuntimeOperation::Blocker {
                    finding: "later-issue".into(),
                    open: is_open,
                    accountability: Accountability::Owner("b".into()),
                    rule: r("blocker-rule"),
                    explanation: "Explicit governance disposition".into(),
                },
            )
            .unwrap();
            assert!(!b.release_current(&"w".into(), &"release".into()));
            let saved = b.clone();
            assert_eq!(
                reply(
                    &mut b,
                    "a",
                    vec![Command::CloseGoal {
                        release: "release".into()
                    }],
                    ended()
                ),
                Err(WorkError::Unmet)
            );
            assert_eq!(b, saved);
        }
        reply(
            &mut b,
            "a",
            vec![Command::Release {
                id: "release-2".into(),
                shared: "shared".into(),
            }],
            ready(),
        )
        .unwrap();
        assert_eq!(
            b.work(&"w".into()).unwrap().releases[&RecordId::from("release-2")]
                .blocker_dispositions,
            2
        );
        assert!(b.release_current(&"w".into(), &"release-2".into()));
        assert!(!b.release_current(&"w".into(), &"release".into()));
        let saved = b.clone();
        assert_eq!(
            reply(
                &mut b,
                "a",
                vec![Command::CloseGoal {
                    release: "release-2".into()
                }],
                ended()
            ),
            Err(WorkError::Unmet)
        );
        assert_eq!(b, saved);
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Delivered {
                id: "delivery-2".into(),
                release: "release-2".into(),
                recipient: "h".into(),
                receipt: r("delivery-2"),
            },
        )
        .unwrap();
        let saved = b.clone();
        assert_eq!(
            reply(
                &mut b,
                "a",
                vec![Command::CloseGoal {
                    release: "release-2".into()
                }],
                ended()
            ),
            Err(WorkError::Unmet)
        );
        assert_eq!(b, saved);
        host(
            &mut b,
            RuntimeRight::StakeholderAcceptance,
            RuntimeOperation::Stakeholder {
                release: "release-2".into(),
                accepts: true,
                receipt: r("acceptance-2"),
            },
        )
        .unwrap();
        reply(
            &mut b,
            "a",
            vec![Command::CloseGoal {
                release: "release-2".into(),
            }],
            ended(),
        )
        .unwrap();
        assert!(b.completion_supported(&"w".into()));
        let w = b.work(&"w".into()).unwrap();
        assert_eq!(
            w.releases[&RecordId::from("release")],
            original.releases[&RecordId::from("release")]
        );
        assert_eq!(w.deliveries[0], original.deliveries[0]);
        assert_eq!(
            w.stakeholder_decisions[0],
            original.stakeholder_decisions[0]
        );
    }
    #[test]
    fn handoff_checks_dependencies_at_acceptance_even_after_event_arrival() {
        let mut b = active(None, false);
        artifact(&mut b);
        reply(
            &mut b,
            "a",
            vec![Command::OfferHandoff {
                id: "to-c".into(),
                to: "c".into(),
            }],
            ready(),
        )
        .unwrap();
        let key = EventKey {
            dependency: r("sender-dependency"),
            epoch: b.work(&"w".into()).unwrap().version.epoch,
        };
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::RegisterDependency(Dependency {
                key: key.clone(),
                accountability: Accountability::Owner("a".into()),
            }),
        )
        .unwrap();
        reply(&mut b, "b", vec![], wait_for(&key)).unwrap();
        for event_arrived in [false, true] {
            if event_arrived {
                host(
                    &mut b,
                    RuntimeRight::Observation,
                    RuntimeOperation::Event(Event {
                        key: key.clone(),
                        observation: r("sender-event"),
                    }),
                )
                .unwrap();
            }
            let saved = b.clone();
            assert_eq!(
                reply(
                    &mut b,
                    "c",
                    vec![Command::Accept {
                        offer: "to-c".into(),
                        exact_terms: terms(true, false)
                    }],
                    ready()
                ),
                Err(WorkError::Unsupported(
                    "handoff while a current dependency names the sender as owner"
                ))
            );
            assert_eq!(b, saved);
            assert_eq!(
                b.work(&"w".into()).unwrap().ownership(&"o".into()),
                Accountability::Owner("a".into())
            );
        }
    }
    #[test]
    fn handoff_requires_live_blocker_disposition_and_preserves_human_history() {
        for remains_open in [false, true] {
            let mut b = active(None, false);
            artifact(&mut b);
            reply(
                &mut b,
                "a",
                vec![
                    Command::Finding {
                        id: "issue".into(),
                        artifact: None,
                        text: "Accountable obligation".into(),
                        evidence: BTreeSet::new(),
                    },
                    Command::OfferHandoff {
                        id: "to-c".into(),
                        to: "c".into(),
                    },
                ],
                ready(),
            )
            .unwrap();
            host(
                &mut b,
                RuntimeRight::Governance,
                RuntimeOperation::Blocker {
                    finding: "issue".into(),
                    open: true,
                    accountability: Accountability::Owner("a".into()),
                    rule: r("blocker-rule"),
                    explanation: "Human authority assigned this obligation".into(),
                },
            )
            .unwrap();
            let saved = b.clone();
            assert_eq!(
                reply(
                    &mut b,
                    "c",
                    vec![Command::Accept {
                        offer: "to-c".into(),
                        exact_terms: terms(true, false)
                    }],
                    ready()
                ),
                Err(WorkError::Unsupported(
                    "handoff while an open blocker names the sender as owner"
                ))
            );
            assert_eq!(b, saved);
            let accountability = if remains_open {
                Accountability::Gap("Explicitly authorized unresolved ownership gap".into())
            } else {
                Accountability::Owner("a".into())
            };
            assert_eq!(
                host(
                    &mut b,
                    RuntimeRight::Observation,
                    RuntimeOperation::Blocker {
                        finding: "issue".into(),
                        open: remains_open,
                        accountability: accountability.clone(),
                        rule: r("disposition-rule"),
                        explanation: "Explicit resolution or ownership gap".into()
                    }
                ),
                Err(WorkError::Unauthorized)
            );
            assert_eq!(b, saved);
            host(
                &mut b,
                RuntimeRight::Governance,
                RuntimeOperation::Blocker {
                    finding: "issue".into(),
                    open: remains_open,
                    accountability,
                    rule: r("disposition-rule"),
                    explanation: "Explicit resolution or ownership gap".into(),
                },
            )
            .unwrap();
            let historical = b.work(&"w".into()).unwrap().clone();
            let ctx = context(&b, "c");
            b.apply_response(
                &ctx,
                DecisionResponse {
                    commands: vec![Command::Accept {
                        offer: "to-c".into(),
                        exact_terms: terms(true, false),
                    }],
                    continuation: ready(),
                },
            )
            .unwrap();
            let w = b.work(&"w".into()).unwrap();
            assert!(w.acceptance(&"a".into()).is_none());
            assert_eq!(
                w.acceptance(&"c".into()).unwrap().0,
                &RecordId::from("to-c")
            );
            assert_eq!(w.ownership(&"o".into()), Accountability::Owner("c".into()));
            assert_eq!(
                w.acceptances[&RecordId::from("to-c")].by,
                Attribution {
                    persona: "c".into(),
                    invocation: ctx.invocation.clone(),
                    version: ctx.version
                }
            );
            assert_eq!(
                w.offers[&RecordId::from("to-c")].status,
                OfferStatus::Accepted(w.acceptances[&RecordId::from("to-c")].by.clone())
            );
            assert_eq!(
                w.acceptances[&RecordId::from("oa")],
                historical.acceptances[&RecordId::from("oa")]
            );
            assert_eq!(w.findings, historical.findings);
            assert_eq!(w.blocker_dispositions, historical.blocker_dispositions);
            assert_eq!(
                w.artifacts[&ArtifactId::from("x")].owner,
                PersonaId::from("c")
            );
            assert_eq!(
                w.artifacts[&ArtifactId::from("x")].revisions,
                historical.artifacts[&ArtifactId::from("x")].revisions
            );
        }
    }
    #[test]
    fn handoff_refuses_another_participants_live_blocked_accountability() {
        let mut b = active(None, false);
        reply(
            &mut b,
            "a",
            vec![Command::OfferHandoff {
                id: "to-c".into(),
                to: "c".into(),
            }],
            ready(),
        )
        .unwrap();
        reply(
            &mut b,
            "b",
            vec![],
            Continuation::Blocked {
                reason: "Awaiting the accountable owner".into(),
                accountability: Accountability::Owner("a".into()),
                reconsider_at: 40,
            },
        )
        .unwrap();
        let saved = b.clone();
        assert_eq!(
            reply(
                &mut b,
                "c",
                vec![Command::Accept {
                    offer: "to-c".into(),
                    exact_terms: terms(true, false)
                }],
                ready()
            ),
            Err(WorkError::Unsupported(
                "handoff while a blocked continuation names the sender as owner"
            ))
        );
        assert_eq!(b, saved);
        reply(
            &mut b,
            "b",
            vec![],
            Continuation::Blocked {
                reason: "Explicitly reconsidered ownership".into(),
                accountability: Accountability::Gap("No accepted owner for this obligation".into()),
                reconsider_at: 40,
            },
        )
        .unwrap();
        let waiting = b.participant(&"w".into(), &"b".into()).unwrap().clone();
        accept(&mut b, "to-c", "c", terms(true, false));
        assert_eq!(b.participant(&"w".into(), &"b".into()), Some(&waiting));
        assert_eq!(
            b.work(&"w".into()).unwrap().ownership(&"o".into()),
            Accountability::Owner("c".into())
        );
    }
    #[test]
    fn legacy_wait_binding_cannot_name_a_departed_owner() {
        let mut b = active(None, false);
        let key = dependency(&mut b);
        // Recovery fixture: a legacy dependency still names an owner without acceptance.
        let owner = b
            .works
            .get_mut(&WorkId::from("w"))
            .unwrap()
            .participants
            .get_mut(&PersonaId::from("b"))
            .unwrap();
        owner.acceptance = None;
        owner.continuation = Some(ended());
        owner.wake = None;
        let saved = b.clone();
        assert_eq!(
            reply(&mut b, "a", vec![], wait_for(&key)),
            Err(WorkError::NotAccepted)
        );
        assert_eq!(b, saved);
    }
    #[test]
    fn exact_event_redelivery_preserves_pending_decisions_and_consumed_wakes() {
        let mut b = active(None, false);
        let key = dependency(&mut b);
        reply(&mut b, "a", vec![], wait_for(&key)).unwrap();
        let event = Event {
            key: key.clone(),
            observation: r("event"),
        };
        let prior_version = b.work(&"w".into()).unwrap().version;
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(event.clone()),
        )
        .unwrap();
        assert_eq!(
            b.work(&"w".into()).unwrap().version,
            Version {
                revision: prior_version.revision + 1,
                epoch: prior_version.epoch
            }
        );
        let mut pending = context(&b, "a");
        pending.observed.insert(key.clone());
        let saved = b.clone();
        for _ in 0..3 {
            host(
                &mut b,
                RuntimeRight::Observation,
                RuntimeOperation::Event(event.clone()),
            )
            .unwrap();
            assert_eq!(b, saved);
        }
        b.apply_response(
            &pending,
            DecisionResponse {
                commands: vec![],
                continuation: wait_for(&key),
            },
        )
        .unwrap();
        let saved = b.clone();
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(event),
        )
        .unwrap();
        assert_eq!(b, saved);
        assert_eq!(
            eligibility(b.work(&"w".into()).unwrap(), &"a".into(), 10),
            None
        );
    }
    #[test]
    fn duplicate_events_still_require_current_authorized_context_and_exact_content() {
        let mut b = active(None, false);
        let key = dependency(&mut b);
        let event = Event {
            key: key.clone(),
            observation: r("event"),
        };
        let stale = runtime(&b, RuntimeRight::Observation);
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(event.clone()),
        )
        .unwrap();
        let saved = b.clone();
        assert!(matches!(
            b.observe(&stale, RuntimeOperation::Event(event.clone())),
            Err(WorkError::StaleRevision { .. })
        ));
        assert_eq!(b, saved);
        let mut wrong_epoch = runtime(&b, RuntimeRight::Observation);
        wrong_epoch.version.epoch += 1;
        assert!(matches!(
            b.observe(&wrong_epoch, RuntimeOperation::Event(event.clone())),
            Err(WorkError::StaleEpoch { .. })
        ));
        assert_eq!(b, saved);
        assert_eq!(
            host(
                &mut b,
                RuntimeRight::Governance,
                RuntimeOperation::Event(event.clone())
            ),
            Err(WorkError::Unauthorized)
        );
        assert_eq!(b, saved);
        let mut wrong_authority = runtime(&b, RuntimeRight::Observation);
        wrong_authority.authority = r("other-authority");
        assert_eq!(
            b.observe(&wrong_authority, RuntimeOperation::Event(event)),
            Err(WorkError::Unauthorized)
        );
        assert_eq!(b, saved);
        assert_eq!(
            host(
                &mut b,
                RuntimeRight::Observation,
                RuntimeOperation::Event(Event {
                    key,
                    observation: r("different-event")
                })
            ),
            Err(WorkError::Conflict)
        );
        assert_eq!(b, saved);
    }
    #[test]
    fn duplicate_event_does_not_need_an_available_revision_number() {
        let mut b = active(None, false);
        let key = dependency(&mut b);
        let event = Event {
            key,
            observation: r("event"),
        };
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(event.clone()),
        )
        .unwrap();
        b.works
            .get_mut(&WorkId::from("w"))
            .unwrap()
            .version
            .revision = u64::MAX;
        let saved = b.clone();
        host(
            &mut b,
            RuntimeRight::Observation,
            RuntimeOperation::Event(event),
        )
        .unwrap();
        assert_eq!(b, saved);
        assert_eq!(
            reply(&mut b, "b", vec![], ready()),
            Err(WorkError::Invalid("counter overflow"))
        );
        assert_eq!(b, saved);
    }
    #[test]
    fn invalid_tail_rolls_back_whole_response_and_serializable_state_is_explicit() {
        fn serializable<T: Serialize + for<'de> Deserialize<'de>>() {}
        serializable::<WorkBoard>();
        let mut b = active(None, false);
        let saved = b.clone();
        assert!(reply(
            &mut b,
            "a",
            vec![
                Command::Finding {
                    id: "f".into(),
                    artifact: None,
                    text: "Valid prefix".into(),
                    evidence: BTreeSet::new()
                },
                Command::Release {
                    id: "r".into(),
                    shared: "missing".into()
                }
            ],
            ready()
        )
        .is_err());
        assert_eq!(b, saved);
    }
}
