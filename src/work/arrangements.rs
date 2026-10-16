//! Exact, voluntary changes within the immutable mandate's authority envelope.
use super::*;

pub const MAX_ARRANGEMENT_PROPOSALS: usize = 32;
pub const MAX_ARRANGEMENT_CHANGES: usize = 32;
pub const MAX_ARRANGEMENT_HOLDERS: usize = 16;
const MAX_ARRANGEMENT_RESPONSES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrangementAssentPolicy {
    AffectedPartyAssentV1,
}

/// Requester-issued permission to allocate the named open duties and select or
/// revise a result rule. The selected rule never becomes amendment authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormationDelegation {
    pub revision: u64,
    pub issuer: Origin,
    pub authorization: ExactRef,
    pub need: ExactRef,
    pub mandate_revision: u64,
    pub allocable_outcomes: BTreeSet<OutcomeId>,
    pub allocable_review_roles: BTreeSet<RoleId>,
    pub assent: ArrangementAssentPolicy,
}

pub fn formation_need_ref(work: &WorkId, need: &Need) -> ExactRef {
    ExactRef::new(
        format!(
            "formation-need-{}",
            hex::encode(Sha256::digest(work.0.as_bytes()))
        ),
        1,
        &serde_json::to_vec(&("formation-need-v1", work, need))
            .expect("serializable formation need"),
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrangementDecisionRule {
    Designated(PersonaId),
    Quorum {
        eligible: BTreeSet<PersonaId>,
        minimum: usize,
    },
}
impl ArrangementDecisionRule {
    pub fn holders(&self) -> BTreeSet<PersonaId> {
        match self {
            Self::Designated(persona) => BTreeSet::from([persona.clone()]),
            Self::Quorum { eligible, .. } => eligible.clone(),
        }
    }
    pub(super) fn satisfied(&self, actors: &BTreeSet<PersonaId>) -> bool {
        match self {
            Self::Designated(persona) => actors.contains(persona),
            Self::Quorum { eligible, minimum } => actors.intersection(eligible).count() >= *minimum,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitmentChange {
    pub prior_acceptance: RecordId,
    pub terms: CommitmentTerms,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactTransfer {
    pub from: PersonaId,
    pub to: PersonaId,
    pub artifact: ArtifactRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementTerms {
    pub delegation: Option<ExactRef>,
    pub base: Option<ExactRef>,
    pub changes: BTreeMap<PersonaId, CommitmentChange>,
    /// None retains existing authority, including an unestablished initial rule.
    pub decision_rule: Option<ArrangementDecisionRule>,
    pub artifact_transfers: Vec<ArtifactTransfer>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementConsent {
    pub acceptance: RecordId,
    pub by: Attribution,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementDecline {
    pub acceptance: RecordId,
    pub by: Attribution,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementProposal {
    pub terms: ArrangementTerms,
    pub by: Attribution,
    pub consents: BTreeMap<PersonaId, ArrangementConsent>,
    pub declines: BTreeMap<PersonaId, ArrangementDecline>,
    pub adopted: Option<ExactRef>,
}
impl ArrangementProposal {
    /// Binds immutable authored terms and attribution, never a mutable tally.
    pub fn exact(&self, id: &RecordId) -> ExactRef {
        ExactRef::new(
            id.0.clone(),
            1,
            &serde_json::to_vec(&("arrangement-proposal-v1", &self.terms, &self.by))
                .expect("serializable arrangement proposal"),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrangement {
    pub revision: u64,
    pub proposal: ExactRef,
    pub decision_rule: Option<ArrangementDecisionRule>,
    pub acceptances: BTreeMap<PersonaId, RecordId>,
    pub consents: BTreeMap<PersonaId, ArrangementConsent>,
    pub by: Attribution,
}
impl Arrangement {
    pub fn exact(&self, id: &RecordId) -> ExactRef {
        ExactRef::new(
            id.0.clone(),
            self.revision,
            &serde_json::to_vec(&("accepted-arrangement-v1", self))
                .expect("serializable arrangement"),
        )
    }
}

impl Work {
    /// Exact complete authorization envelope, including immutable need, mandate,
    /// eligible actors, allocable scope and amendment assent policy.
    pub fn formation_delegation_ref(&self) -> Option<ExactRef> {
        let DecisionRule::PersonaArranged { delegation, .. } = &self.mandate().decision_rule else {
            return None;
        };
        Some(ExactRef::new(
            format!(
                "formation-{}",
                hex::encode(Sha256::digest(delegation.authorization.id.as_bytes()))
            ),
            delegation.revision,
            &serde_json::to_vec(&("formation-delegation-v1", &self.need, self.mandate()))
                .expect("serializable formation delegation"),
        ))
    }

    pub fn arrangement_ref(&self) -> Option<ExactRef> {
        let id = self.current_arrangement.as_ref()?;
        self.arrangements.get(id).map(|value| value.exact(id))
    }
    pub fn effective_decision_rule(&self) -> Option<ArrangementDecisionRule> {
        if let Some(id) = &self.current_arrangement {
            return self.arrangements.get(id)?.decision_rule.clone();
        }
        match &self.mandate().decision_rule {
            DecisionRule::Designated(persona) => {
                Some(ArrangementDecisionRule::Designated(persona.clone()))
            }
            DecisionRule::Quorum { eligible, minimum } => Some(ArrangementDecisionRule::Quorum {
                eligible: eligible.clone(),
                minimum: *minimum,
            }),
            DecisionRule::PersonaArranged { .. } => None,
        }
    }
    /// Current-state explanation only; does not certify proposal adoption.
    pub fn arrangement_required_consents(&self, terms: &ArrangementTerms) -> BTreeSet<PersonaId> {
        required_consents(self, terms)
    }
    /// Amendment authority is the immutable requester delegation, never the
    /// chosen result rule. This projection does not certify current assent.
    pub fn arrangement_governance_required(
        &self,
        terms: &ArrangementTerms,
    ) -> Option<ArrangementAssentPolicy> {
        match &self.mandate().decision_rule {
            DecisionRule::PersonaArranged { delegation, .. } if governance_changed(self, terms) => {
                Some(delegation.assent.clone())
            }
            _ => None,
        }
    }
}

pub(super) fn validate_delegation(
    work: &WorkId,
    need: &Need,
    mandate: &Mandate,
    issuer: &Origin,
) -> Result<()> {
    if let DecisionRule::PersonaArranged { delegation, .. } = &mandate.decision_rule {
        require(
            delegation.revision > 0
                && &delegation.issuer == issuer
                && matches!(&delegation.issuer, Origin::Human(person) if text(person))
                && delegation.authorization == mandate.authority
                && delegation.need == formation_need_ref(work, need)
                && delegation.mandate_revision == mandate.revision
                && delegation
                    .allocable_outcomes
                    .iter()
                    .all(|id| mandate.outcomes.contains_key(id))
                && delegation
                    .allocable_review_roles
                    .iter()
                    .all(|id| mandate.review_roles.contains_key(id)),
            "requester-issued exact formation delegation",
        )?;
    }
    Ok(())
}

fn validate_rule(w: &Work, rule: &ArrangementDecisionRule) -> Result<()> {
    let DecisionRule::PersonaArranged { eligible: pool, .. } = &w.mandate().decision_rule else {
        return Err(WorkError::Unauthorized);
    };
    let holders = rule.holders();
    require(
        !holders.is_empty() && holders.len() <= MAX_ARRANGEMENT_HOLDERS && holders.is_subset(pool),
        "arrangement decision holders within authorized pool",
    )?;
    if let ArrangementDecisionRule::Quorum { minimum, .. } = rule {
        require(
            *minimum > 0 && *minimum <= holders.len(),
            "arrangement quorum",
        )?;
    }
    for holder in &holders {
        accepted(w, holder)?;
    }
    Ok(())
}

pub(super) fn actor_in_pool(w: &Work, persona: &PersonaId) -> Result<()> {
    if let DecisionRule::PersonaArranged { eligible, .. } = &w.mandate().decision_rule {
        if !eligible.contains(persona) {
            return Err(WorkError::Unauthorized);
        }
    }
    Ok(())
}

fn validate_terms(w: &Work, terms: &ArrangementTerms) -> Result<()> {
    if terms.base != w.arrangement_ref() || terms.delegation != w.formation_delegation_ref() {
        return Err(WorkError::Conflict);
    }
    require(
        text(&terms.reason)
            && terms.reason.len() <= 4096
            && terms.changes.len() <= MAX_ARRANGEMENT_CHANGES
            && terms.artifact_transfers.len() <= MAX_ARRANGEMENT_CHANGES
            && (!terms.changes.is_empty()
                || terms.decision_rule.is_some()
                || !terms.artifact_transfers.is_empty()),
        "bounded nonempty arrangement proposal",
    )?;
    if let Some(rule) = &terms.decision_rule {
        validate_rule(w, rule)?;
    }
    require(
        !terms.changes.is_empty()
            || !terms.artifact_transfers.is_empty()
            || governance_changed(w, terms),
        "arrangement changes accepted terms, artifacts or authority",
    )?;
    let mut previous_outcomes = BTreeSet::new();
    let mut previous_roles = BTreeSet::new();
    let mut next_outcomes = BTreeSet::new();
    let mut next_roles = BTreeSet::new();
    for (persona, change) in &terms.changes {
        actor_in_pool(w, persona)?;
        let (id, old) = accepted(w, persona)?;
        if id != &change.prior_acceptance || change.terms == old.terms {
            return Err(WorkError::Conflict);
        }
        require(
            change.terms.mandate_revision == w.current_mandate
                && text(&change.terms.exact_terms)
                && change.terms.exact_terms.len() <= 16_384
                && change
                    .terms
                    .outcomes
                    .iter()
                    .all(|id| w.mandate().outcomes.contains_key(id))
                && change
                    .terms
                    .review_roles
                    .iter()
                    .all(|id| w.mandate().review_roles.contains_key(id)),
            "exact successor commitment within mandate",
        )?;
    }
    for persona in w.participants.keys() {
        if let Some((_, old)) = w.acceptance(persona) {
            previous_outcomes.extend(old.terms.outcomes.iter().cloned());
            previous_roles.extend(old.terms.review_roles.iter().cloned());
            let next = terms
                .changes
                .get(persona)
                .map_or(&old.terms, |change| &change.terms);
            for outcome in &next.outcomes {
                if !next_outcomes.insert(outcome.clone()) {
                    return Err(WorkError::Conflict);
                }
            }
            for role in &next.review_roles {
                if !next_roles.insert(role.clone()) {
                    return Err(WorkError::Conflict);
                }
            }
        }
    }
    require(
        previous_outcomes.is_subset(&next_outcomes) && previous_roles.is_subset(&next_roles),
        "previous duties must remain held or transfer with exact consent",
    )?;
    if let DecisionRule::PersonaArranged { delegation, .. } = &w.mandate().decision_rule {
        require(
            next_outcomes
                .difference(&previous_outcomes)
                .all(|id| delegation.allocable_outcomes.contains(id))
                && next_roles
                    .difference(&previous_roles)
                    .all(|id| delegation.allocable_review_roles.contains(id)),
            "new allocation within exact requester delegation",
        )?;
    }
    let mut transferred = BTreeSet::new();
    for transfer in &terms.artifact_transfers {
        actor_in_pool(w, &transfer.from)?;
        actor_in_pool(w, &transfer.to)?;
        accepted(w, &transfer.from)?;
        accepted(w, &transfer.to)?;
        require(transfer.from != transfer.to, "distinct artifact recipient")?;
        let artifact = w
            .artifacts
            .get(&transfer.artifact.id)
            .ok_or(WorkError::Missing("artifact"))?;
        if artifact.owner != transfer.from
            || !artifact_current(w, &transfer.artifact)
            || !transferred.insert(transfer.artifact.id.clone())
        {
            return Err(WorkError::Conflict);
        }
        // This transfers only an exact artifact. The sender keeps an accepted
        // commitment and every separately recorded dependency/blocker duty.
    }
    Ok(())
}

fn required_consents(w: &Work, terms: &ArrangementTerms) -> BTreeSet<PersonaId> {
    let mut required: BTreeSet<_> = terms.changes.keys().cloned().collect();
    for transfer in &terms.artifact_transfers {
        required.insert(transfer.from.clone());
        required.insert(transfer.to.clone());
    }
    if governance_changed(w, terms) {
        required.extend(
            w.participants
                .keys()
                .filter(|persona| w.acceptance(persona).is_some())
                .cloned(),
        );
        if let Some(rule) = &terms.decision_rule {
            required.extend(rule.holders());
        }
    }
    required
}

fn governance_changed(w: &Work, terms: &ArrangementTerms) -> bool {
    terms
        .decision_rule
        .as_ref()
        .is_some_and(|rule| Some(rule) != w.effective_decision_rule().as_ref())
}

pub(super) fn propose(
    w: &mut Work,
    by: &Attribution,
    id: RecordId,
    terms: ArrangementTerms,
    consent_own: bool,
) -> Result<()> {
    actor_in_pool(w, &by.persona)?;
    accepted(w, &by.persona)?;
    require(
        crate::protocol::check_id(&id.0, "arrangement proposal").is_ok()
            && w.arrangement_proposals.len() < MAX_ARRANGEMENT_PROPOSALS,
        "arrangement proposal identity and capacity",
    )?;
    if w.arrangement_proposals.contains_key(&id) || w.arrangements.contains_key(&id) {
        return Err(WorkError::Conflict);
    }
    validate_terms(w, &terms)?;
    w.arrangement_proposals.insert(
        id.clone(),
        ArrangementProposal {
            terms,
            by: by.clone(),
            consents: BTreeMap::new(),
            declines: BTreeMap::new(),
            adopted: None,
        },
    );
    if consent_own {
        let exact = w.arrangement_proposals[&id].exact(&id);
        consent(w, by, id, exact)?;
    }
    Ok(())
}

fn pending(w: &Work, id: &RecordId, exact: &ExactRef) -> Result<ArrangementProposal> {
    let proposal = w
        .arrangement_proposals
        .get(id)
        .ok_or(WorkError::Missing("arrangement proposal"))?;
    if &proposal.exact(id) != exact {
        return Err(WorkError::Terms);
    }
    if proposal.adopted.is_some() {
        return Err(WorkError::Conflict);
    }
    validate_terms(w, &proposal.terms)?;
    Ok(proposal.clone())
}

pub(super) fn decline(
    w: &mut Work,
    by: &Attribution,
    id: RecordId,
    exact: ExactRef,
    reason: String,
) -> Result<()> {
    actor_in_pool(w, &by.persona)?;
    let (acceptance, _) = accepted(w, &by.persona)?;
    let acceptance = acceptance.clone();
    let proposal = pending(w, &id, &exact)?;
    require(
        text(&reason) && reason.len() <= 4096,
        "arrangement decline reason",
    )?;
    if proposal.declines.contains_key(&by.persona) {
        return Err(WorkError::Conflict);
    }
    require(
        required_consents(w, &proposal.terms).contains(&by.persona),
        "decline has an affected duty or result-rule scope",
    )?;
    require(
        proposal.consents.len() + proposal.declines.len() < MAX_ARRANGEMENT_RESPONSES,
        "arrangement response capacity",
    )?;
    w.arrangement_proposals
        .get_mut(&id)
        .unwrap()
        .declines
        .insert(
            by.persona.clone(),
            ArrangementDecline {
                acceptance,
                by: by.clone(),
                reason,
            },
        );
    Ok(())
}

pub(super) fn consent(w: &mut Work, by: &Attribution, id: RecordId, exact: ExactRef) -> Result<()> {
    actor_in_pool(w, &by.persona)?;
    let (acceptance, _) = accepted(w, &by.persona)?;
    let acceptance = acceptance.clone();
    let proposal = pending(w, &id, &exact)?;
    if proposal.consents.contains_key(&by.persona) || proposal.declines.contains_key(&by.persona) {
        return Err(WorkError::Conflict);
    }
    let required = required_consents(w, &proposal.terms);
    let current_rule = w.effective_decision_rule();
    require(
        required.contains(&by.persona),
        "consent has an affected duty or result-rule scope",
    )?;
    require(
        proposal.consents.len() + proposal.declines.len() < MAX_ARRANGEMENT_RESPONSES,
        "arrangement response capacity",
    )?;
    w.arrangement_proposals
        .get_mut(&id)
        .unwrap()
        .consents
        .insert(
            by.persona.clone(),
            ArrangementConsent {
                acceptance,
                by: by.clone(),
            },
        );
    let proposal = w.arrangement_proposals[&id].clone();
    let valid: BTreeSet<_> = proposal
        .consents
        .iter()
        .filter_map(|(persona, consent)| {
            (!proposal.declines.contains_key(persona))
                .then(|| w.acceptance(persona))
                .flatten()
                .filter(|(current, _)| *current == &consent.acceptance)
                .map(|_| persona.clone())
        })
        .collect();
    if !required.is_subset(&valid) {
        return Ok(());
    }
    // All preimages are checked again at the single adoption point. Neither a
    // proposed change nor a partial consent changes another participant's duty.
    validate_terms(w, &proposal.terms)?;
    for (persona, change) in &proposal.terms.changes {
        let bytes = serde_json::to_vec(&("arrangement-acceptance-v1", &exact, persona))
            .expect("serializable acceptance identity");
        let acceptance = RecordId(format!(
            "arrangement-{}",
            hex::encode(Sha256::digest(bytes))
        ));
        if w.acceptances.contains_key(&acceptance) || w.offers.contains_key(&acceptance) {
            return Err(WorkError::Conflict);
        }
        w.acceptances.insert(
            acceptance.clone(),
            Acceptance {
                arrangement: None,
                terms: change.terms.clone(),
                by: proposal.consents[persona].by.clone(),
            },
        );
        w.participants.get_mut(persona).unwrap().acceptance = Some(acceptance);
    }
    for transfer in &proposal.terms.artifact_transfers {
        w.artifacts.get_mut(&transfer.artifact.id).unwrap().owner = transfer.to.clone();
    }
    let revision = match &w.current_arrangement {
        Some(current) => increment(w.arrangements[current].revision)?,
        None => 1,
    };
    let arrangement = Arrangement {
        revision,
        proposal: exact,
        decision_rule: proposal.terms.decision_rule.clone().or(current_rule),
        acceptances: w
            .participants
            .keys()
            .filter_map(|persona| {
                w.acceptance(persona)
                    .map(|(id, _)| (persona.clone(), id.clone()))
            })
            .collect(),
        consents: proposal
            .consents
            .into_iter()
            .filter(|(persona, _)| valid.contains(persona))
            .collect(),
        by: by.clone(),
    };
    let exact = arrangement.exact(&id);
    for persona in proposal.terms.changes.keys() {
        let acceptance = w.participants[persona].acceptance.as_ref().unwrap();
        w.acceptances.get_mut(acceptance).unwrap().arrangement = Some(exact.clone());
    }
    w.arrangements.insert(id.clone(), arrangement);
    w.current_arrangement = Some(id.clone());
    w.arrangement_proposals.get_mut(&id).unwrap().adopted = Some(exact);
    Ok(())
}

/// Legacy whole-commitment transfer cannot silently remove governing rights.
pub(super) fn check_legacy_handoff(w: &Work, sender: &PersonaId, to: &PersonaId) -> Result<()> {
    if w.current_arrangement.is_some() {
        return Err(WorkError::Unsupported(
            "an accepted arrangement requires an explicit consented duty/artifact transfer",
        ));
    }
    if let Some(rule) = w.effective_decision_rule() {
        let mut remaining: BTreeSet<_> = w
            .participants
            .keys()
            .filter(|persona| *persona != sender)
            .filter(|persona| w.acceptance(persona).is_some())
            .cloned()
            .collect();
        remaining.insert(to.clone());
        if rule.holders().contains(sender) && !rule.satisfied(&remaining) {
            return Err(WorkError::Unsupported("handoff would strand current decision authority; use an explicitly consented arrangement"));
        }
    }
    Ok(())
}
