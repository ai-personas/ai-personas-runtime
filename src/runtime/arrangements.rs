//! Persona-authored arrangement inputs and complete admitted proposal supply.
//! These projections inherit the work core's current permission checks. They
//! grant neither authority nor resources and never change peer continuation.
use super::*;

/// Host-retained immutable origins. Empty historical maps establish no lineage.
/// No later work exposure backfills a record.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Origins {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    delegations: BTreeMap<String, Exposure>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    proposals: BTreeMap<String, Exposure>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    responses: BTreeMap<String, Exposure>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    activations: BTreeMap<String, Exposure>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    acceptances: BTreeMap<String, Exposure>,
}

impl Origins {
    pub(super) fn is_empty(&self) -> bool {
        self.delegations.is_empty()
            && self.proposals.is_empty()
            && self.responses.is_empty()
            && self.activations.is_empty()
            && self.acceptances.is_empty()
    }

    pub(super) fn for_need(state: &State, work_id: &str, inherited: &Exposure) -> Result<Self> {
        let work = state.board.work(&w::WorkId(work_id.into()))?;
        let mut result = Self::default();
        if let Some(exact) = work.formation_delegation_ref() {
            result.delegations.insert(
                delegation_key(work_id, &exact)?,
                expanded_exposure(state, inherited)?,
            );
        }
        Ok(result)
    }
}

pub(super) fn retain_amendment_origin(state: &mut State, work_id: &str, inherited: &Exposure) -> Result<()> {
    let current = Origins::for_need(state, work_id, inherited)?;
    let origins = &mut state.work_meta.get_mut(work_id)
        .ok_or_else(|| held("work metadata absent"))?.arrangement_origins;
    for (key, exposure) in current.delegations {
        require(!origins.delegations.contains_key(&key), "amendment delegation origin already exists")?;
        origins.delegations.insert(key, exposure);
    }
    Ok(())
}

fn delegation_key(work: &str, exact: &w::ExactRef) -> Result<String> {
    digest(&("arrangement-delegation-origin-v1", work, exact))
}

fn proposal_key(work: &str, id: &w::RecordId, proposal: &w::ArrangementProposal) -> Result<String> {
    digest(&(
        "arrangement-proposal-origin-v1",
        work,
        id,
        proposal.exact(id),
        &proposal.by,
    ))
}

fn response_key<T: Serialize>(
    work: &str,
    id: &w::RecordId,
    proposal: &w::ArrangementProposal,
    kind: &str,
    persona: &w::PersonaId,
    response: &T,
) -> Result<String> {
    digest(&(
        "arrangement-response-origin-v1",
        work,
        id,
        proposal.exact(id),
        kind,
        persona,
        response,
    ))
}

fn activation_key(work: &str, id: &w::RecordId, arrangement: &w::Arrangement) -> Result<String> {
    digest(&(
        "arrangement-activation-origin-v1",
        work,
        id,
        arrangement.exact(id),
        &arrangement.by,
    ))
}

fn acceptance_key(work: &str, id: &w::RecordId, acceptance: &w::Acceptance) -> Result<String> {
    digest(&("arrangement-acceptance-origin-v1", work, id, acceptance))
}

fn insert_origin(
    origins: &mut BTreeMap<String, Exposure>,
    key: String,
    exposure: Exposure,
) -> Result<()> {
    if let Some(existing) = origins.get(&key) {
        require(
            existing == &exposure,
            "immutable arrangement origin conflict",
        )?;
    } else {
        origins.insert(key, exposure);
    }
    Ok(())
}

fn current_author(by: &w::Attribution, prepared: &Prepared, before: &w::Work) -> Result<()> {
    require(
        by.persona.0 == prepared.request.persona
            && by.invocation.0 == prepared.request.id
            && by.version == before.version,
        "arrangement event attribution differs from this admission",
    )
}

/// Run after the board transition, in the same staged transaction. Only newly
/// recorded events from this exact invocation receive origin capsules.
pub(super) fn retain_origins(
    state: &mut State,
    before: &w::Work,
    prepared: &Prepared,
) -> Result<Option<Exposure>> {
    let work_id = &prepared.request.work;
    let after = state.board.work(&w::WorkId(work_id.clone()))?;
    let changed = after.arrangement_proposals != before.arrangement_proposals
        || after.arrangements != before.arrangements
        || after.acceptances.iter().any(|(id, acceptance)| {
            !before.acceptances.contains_key(id) && acceptance.arrangement.is_some()
        });
    if !changed {
        return Ok(None);
    }
    let mut authored = expanded_exposure(state, prepared_exposure(prepared)?)?;
    let mut origins = state.work_meta[work_id].arrangement_origins.clone();
    if let Some(exact) = after.formation_delegation_ref() {
        extend_exposure(
            &mut authored,
            origins
                .delegations
                .get(&delegation_key(work_id, &exact)?)
                .ok_or_else(|| held("formation delegation original lineage unavailable"))?,
        );
    }
    let mut retained = authored.clone();
    for (id, proposal) in &after.arrangement_proposals {
        let old = before.arrangement_proposals.get(id);
        if let Some(old) = old {
            require(
                old.exact(id) == proposal.exact(id),
                "immutable arrangement proposal changed",
            )?;
        } else {
            current_author(&proposal.by, prepared, before)?;
            insert_origin(
                &mut origins.proposals,
                proposal_key(work_id, id, proposal)?,
                authored.clone(),
            )?;
        }
        for (persona, consent) in &proposal.consents {
            if old.is_some_and(|old| old.consents.contains_key(persona)) {
                continue;
            }
            current_author(&consent.by, prepared, before)?;
            require(
                &consent.by.persona == persona,
                "arrangement consent actor mismatch",
            )?;
            let mut inherited = authored.clone();
            extend_exposure(
                &mut inherited,
                origins
                    .proposals
                    .get(&proposal_key(work_id, id, proposal)?)
                    .ok_or_else(|| held("arrangement proposal original lineage unavailable"))?,
            );
            extend_exposure(&mut retained, &inherited);
            insert_origin(
                &mut origins.responses,
                response_key(work_id, id, proposal, "consent", persona, consent)?,
                inherited,
            )?;
        }
        for (persona, decline) in &proposal.declines {
            if old.is_some_and(|old| old.declines.contains_key(persona)) {
                continue;
            }
            current_author(&decline.by, prepared, before)?;
            require(
                &decline.by.persona == persona,
                "arrangement decline actor mismatch",
            )?;
            let mut inherited = authored.clone();
            extend_exposure(
                &mut inherited,
                origins
                    .proposals
                    .get(&proposal_key(work_id, id, proposal)?)
                    .ok_or_else(|| held("arrangement proposal original lineage unavailable"))?,
            );
            extend_exposure(&mut retained, &inherited);
            insert_origin(
                &mut origins.responses,
                response_key(work_id, id, proposal, "decline", persona, decline)?,
                inherited,
            )?;
        }
    }
    for (id, arrangement) in &after.arrangements {
        if before.arrangements.contains_key(id) {
            continue;
        }
        current_author(&arrangement.by, prepared, before)?;
        let proposal = after
            .arrangement_proposals
            .get(id)
            .ok_or_else(|| held("accepted arrangement proposal unavailable"))?;
        require(
            arrangement.proposal == proposal.exact(id),
            "arrangement activation proposal mismatch",
        )?;
        let mut inherited = authored.clone();
        extend_exposure(
            &mut inherited,
            origins
                .proposals
                .get(&proposal_key(work_id, id, proposal)?)
                .ok_or_else(|| held("arrangement proposal original lineage unavailable"))?,
        );
        for (persona, consent) in &arrangement.consents {
            extend_exposure(
                &mut inherited,
                origins
                    .responses
                    .get(&response_key(
                        work_id, id, proposal, "consent", persona, consent,
                    )?)
                    .ok_or_else(|| held("arrangement consent original lineage unavailable"))?,
            );
        }
        let inherited = expanded_exposure(state, &inherited)?;
        extend_exposure(&mut retained, &inherited);
        insert_origin(
            &mut origins.activations,
            activation_key(work_id, id, arrangement)?,
            inherited.clone(),
        )?;
        for persona in proposal.terms.changes.keys() {
            let acceptance_id = arrangement
                .acceptances
                .get(persona)
                .ok_or_else(|| held("arrangement successor acceptance unavailable"))?;
            require(
                !before.acceptances.contains_key(acceptance_id),
                "arrangement activation reused historical acceptance",
            )?;
            let acceptance = after
                .acceptances
                .get(acceptance_id)
                .ok_or_else(|| held("arrangement successor terms unavailable"))?;
            insert_origin(
                &mut origins.acceptances,
                acceptance_key(work_id, acceptance_id, acceptance)?,
                inherited.clone(),
            )?;
        }
    }
    for (id, acceptance) in &after.acceptances {
        if before.acceptances.contains_key(id) {
            continue;
        }
        let Some(exact) = &acceptance.arrangement else {
            continue;
        };
        let key = acceptance_key(work_id, id, acceptance)?;
        if origins.acceptances.contains_key(&key) {
            continue;
        }
        // Successor commitments were captured above with their own historical
        // consent attribution. Remaining new arranged acceptances are explicit joins.
        current_author(&acceptance.by, prepared, before)?;
        let arrangement_id = w::RecordId(exact.id.clone());
        let arrangement = after
            .arrangements
            .get(&arrangement_id)
            .filter(|record| record.exact(&arrangement_id) == *exact)
            .ok_or_else(|| held("joined exact arrangement unavailable"))?;
        let mut inherited = authored.clone();
        extend_exposure(
            &mut inherited,
            origins
                .activations
                .get(&activation_key(work_id, &arrangement_id, arrangement)?)
                .ok_or_else(|| held("joined arrangement original lineage unavailable"))?,
        );
        extend_exposure(&mut retained, &inherited);
        insert_origin(&mut origins.acceptances, key, inherited)?;
    }
    state
        .work_meta
        .get_mut(work_id)
        .unwrap()
        .arrangement_origins = origins;
    Ok(Some(retained))
}

/// Expanded immutable origins may add original restrictions absent from a
/// later mutable aggregate. Preserve existing participants' core access too.
pub(super) fn require_preserved_access(
    state: &State,
    previous: &State,
    work: &w::Work,
    work_id: &str,
    proposed: &Exposure,
    now: i64,
) -> Result<()> {
    let prior = work_exposure(previous, work_id)?;
    let policy = current_policy(previous, &previous.work_meta[work_id].original.policy_id)?;
    for participant in work.participants.keys() {
        let persona = previous
            .personas
            .get(&participant.0)
            .ok_or_else(|| held("work participant persona absent"))?;
        let recipient = admission(persona, now);
        if policy_permitted(previous, policy, &recipient)
            && exposure_permitted(previous, &prior, &recipient)
        {
            require(
                exposure_permitted(state, proposed, &recipient),
                "arrangement origin would remove an existing participant's work core access",
            )?;
        }
    }
    Ok(())
}

pub(super) const DUTIES: &str = "Persona arrangements: CONTROL.arrangements reports current work-core metadata; omitted records establish neither absence nor attention. A PersonaArranged mandate records its original eligible list as metadata and supplies no initial owner or decision rule. Any accepted participant may propose exact voluntary commitment changes and a Designated or Quorum rule; holders must actually accept participation. An arrangement transfers no file-source ownership, selects no body, supplies no resource or resume permit and wakes no peer. ";

/// Exact work-record identity. This is not a file reference or an ArtifactKey.
/// digest is exactly 64 lowercase hexadecimal characters, not a byte array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkExactRef {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(range(min = 1))]
    pub revision: u64,
    #[schemars(length(equal = 64), regex(pattern = "^[0-9a-f]{64}$"))]
    pub digest: String,
}

impl WorkExactRef {
    pub(super) fn native(&self) -> Result<w::ExactRef> {
        p::check_id(&self.id, "arrangement.exact.id")?;
        let bytes = hex::decode(&self.digest).map_err(|_| invalid("arrangement digest"))?;
        let digest: [u8; 32] = bytes
            .try_into()
            .map_err(|_| invalid("arrangement digest length"))?;
        require(
            self.revision > 0 && hex::encode(digest) == self.digest,
            "arrangement exact identity is not canonical",
        )?;
        Ok(w::ExactRef {
            id: self.id.clone(),
            revision: self.revision,
            digest,
        })
    }

    fn from_native(exact: &w::ExactRef) -> Self {
        Self {
            id: exact.id.clone(),
            revision: exact.revision,
            digest: hex::encode(exact.digest),
        }
    }
}

/// A choice within the mandate's explicit delegation. Every holder of a changed
/// rule must consent personally under the requester's exact formation delegation.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArrangementDecisionRuleInput {
    Designated {
        #[schemars(length(min = 1))]
        persona: String,
    },
    Quorum {
        #[schemars(length(min = 1))]
        #[schemars(extend("items" = {"type": "string", "minLength": 1}))]
        eligible: BTreeSet<String>,
        #[schemars(range(min = 1))]
        minimum: usize,
    },
}

impl ArrangementDecisionRuleInput {
    fn native(&self) -> w::ArrangementDecisionRule {
        match self {
            Self::Designated { persona } => {
                w::ArrangementDecisionRule::Designated(w::PersonaId(persona.clone()))
            }
            Self::Quorum { eligible, minimum } => w::ArrangementDecisionRule::Quorum {
                eligible: eligible.iter().cloned().map(w::PersonaId).collect(),
                minimum: *minimum,
            },
        }
    }

    fn from_native(rule: &w::ArrangementDecisionRule) -> Self {
        match rule {
            w::ArrangementDecisionRule::Designated(persona) => Self::Designated {
                persona: persona.0.clone(),
            },
            w::ArrangementDecisionRule::Quorum { eligible, minimum } => Self::Quorum {
                eligible: eligible.iter().map(|persona| persona.0.clone()).collect(),
                minimum: *minimum,
            },
        }
    }
}

/// Complete replacement terms for one already accepted participant. Empty
/// duty sets retain participation; removed duties must have explicit new owners.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArrangementCommitment {
    #[schemars(range(min = 1))]
    pub mandate_revision: u64,
    /// Nonblank complete commitment text.
    #[schemars(length(min = 1))]
    pub exact_terms: String,
    #[schemars(extend("items" = {"type": "string", "minLength": 1}))]
    pub outcomes: BTreeSet<String>,
    #[schemars(extend("items" = {"type": "string", "minLength": 1}))]
    pub review_roles: BTreeSet<String>,
}

impl ArrangementCommitment {
    fn native(&self) -> w::CommitmentTerms {
        w::CommitmentTerms {
            mandate_revision: self.mandate_revision,
            exact_terms: self.exact_terms.clone(),
            outcomes: self.outcomes.iter().cloned().map(w::OutcomeId).collect(),
            review_roles: self.review_roles.iter().cloned().map(w::RoleId).collect(),
        }
    }

    fn from_native(terms: &w::CommitmentTerms) -> Self {
        Self {
            mandate_revision: terms.mandate_revision,
            exact_terms: terms.exact_terms.clone(),
            outcomes: terms
                .outcomes
                .iter()
                .map(|outcome| outcome.0.clone())
                .collect(),
            review_roles: terms
                .review_roles
                .iter()
                .map(|role| role.0.clone())
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitmentChangeInput {
    #[schemars(length(min = 1))]
    pub prior_acceptance: String,
    pub terms: ArrangementCommitment,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferInput {
    #[schemars(length(min = 1))]
    pub from: String,
    #[schemars(length(min = 1))]
    pub to: String,
    pub artifact: ArtifactKey,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArrangementTermsInput {
    /// Copy CONTROL.arrangements.delegation_exact; null only for fixed legacy mandates.
    pub delegation: Option<WorkExactRef>,
    /// Copy CONTROL.arrangements.current_exact; null only before any arrangement.
    pub base: Option<WorkExactRef>,
    /// Only changed commitments: each persona key names its exact current
    /// acceptance and genuinely different successor terms. Omit people
    /// whose terms stay unchanged; their commitments are retained automatically.
    /// Required consent covers changed commitment parties and artifact transfers.
    /// A decision-rule change also requires every old and new decision holder;
    /// unchanged contributors acquire no additional approval duty.
    #[schemars(extend("propertyNames" = {"type": "string", "minLength": 1}))]
    pub changes: BTreeMap<String, CommitmentChangeInput>,
    /// Null preserves the current rule, including its absence. A duty-only
    /// arrangement with no rule authorizes no shared selection or release.
    /// Fixed legacy mandates cannot replace their rule.
    pub decision_rule: Option<ArrangementDecisionRuleInput>,
    /// Explicit work-artifact ownership only; never transfers file-source ownership.
    pub artifact_transfers: Vec<ArtifactTransferInput>,
    /// Nonblank explanation of the proposed change.
    #[schemars(length(min = 1))]
    pub reason: String,
}

impl ArrangementTermsInput {
    pub(super) fn native(&self) -> Result<w::ArrangementTerms> {
        Ok(w::ArrangementTerms {
            delegation: self
                .delegation
                .as_ref()
                .map(WorkExactRef::native)
                .transpose()?,
            base: self.base.as_ref().map(WorkExactRef::native).transpose()?,
            changes: self
                .changes
                .iter()
                .map(|(persona, change)| {
                    (
                        w::PersonaId(persona.clone()),
                        w::CommitmentChange {
                            prior_acceptance: w::RecordId(change.prior_acceptance.clone()),
                            terms: change.terms.native(),
                        },
                    )
                })
                .collect(),
            decision_rule: self
                .decision_rule
                .as_ref()
                .map(ArrangementDecisionRuleInput::native),
            artifact_transfers: self
                .artifact_transfers
                .iter()
                .map(|transfer| {
                    Ok(w::ArtifactTransfer {
                        from: w::PersonaId(transfer.from.clone()),
                        to: w::PersonaId(transfer.to.clone()),
                        artifact: transfer.artifact.native()?,
                    })
                })
                .collect::<Result<_>>()?,
            reason: self.reason.clone(),
        })
    }

    fn from_native(terms: &w::ArrangementTerms) -> Self {
        Self {
            delegation: terms.delegation.as_ref().map(WorkExactRef::from_native),
            base: terms.base.as_ref().map(WorkExactRef::from_native),
            changes: terms
                .changes
                .iter()
                .map(|(persona, change)| {
                    (
                        persona.0.clone(),
                        CommitmentChangeInput {
                            prior_acceptance: change.prior_acceptance.0.clone(),
                            terms: ArrangementCommitment::from_native(&change.terms),
                        },
                    )
                })
                .collect(),
            decision_rule: terms
                .decision_rule
                .as_ref()
                .map(ArrangementDecisionRuleInput::from_native),
            artifact_transfers: terms
                .artifact_transfers
                .iter()
                .map(|transfer| ArtifactTransferInput {
                    from: transfer.from.0.clone(),
                    to: transfer.to.0.clone(),
                    artifact: ArtifactKey::from_native(&transfer.artifact),
                })
                .collect(),
            reason: terms.reason.clone(),
        }
    }
}

/// Serde accepts sequence forms for ordinary structs; require actual JSON
/// objects at every typed object boundary before deserializing the command.
pub(super) fn validate_objects(value: &Value) -> Result<()> {
    let object = |value: &Value| {
        require(
            value.is_object(),
            "arrangement input requires typed objects",
        )
    };
    match value["kind"].as_str() {
        Some("propose_arrangement") => {
            let terms = &value["terms"];
            object(terms)?;
            if !terms["delegation"].is_null() {
                object(&terms["delegation"])?;
            }
            if !terms["base"].is_null() {
                object(&terms["base"])?;
            }
            object(&terms["changes"])?;
            for change in terms["changes"]
                .as_object()
                .expect("checked object")
                .values()
            {
                object(change)?;
                object(&change["terms"])?;
            }
            if !terms["decision_rule"].is_null() {
                object(&terms["decision_rule"])?;
            }
            let transfers = terms["artifact_transfers"]
                .as_array()
                .ok_or_else(|| held("arrangement transfers must be an array"))?;
            for transfer in transfers {
                object(transfer)?;
                object(&transfer["artifact"])?;
            }
        }
        Some("accept_arrangement" | "decline_arrangement") => object(&value["exact"])?,
        Some("accept_arranged_offer") => require(
            value["arrangement"].is_object(),
            "accept_arranged_offer needs offer, terms_revision and an arrangement object; pending proposal consent uses accept_arrangement with proposal and exact",
        )?,
        _ => (),
    }
    Ok(())
}

fn unique_ids(raw: &Value, values: &BTreeSet<String>, label: &str) -> Result<()> {
    require(
        raw
                .as_array()
                .is_some_and(|items| items.len() == values.len()),
        "arrangement set members must be unique",
    )?;
    for value in values {
        p::check_id(value, label)?;
    }
    Ok(())
}

pub(super) fn validate_action(value: &Value, action: &WorkAction) -> Result<()> {
    match action {
        WorkAction::ProposeArrangement { id, terms, .. } => {
            p::check_id(id, "arrangement.id")?;
            text(&terms.reason, 4096)?;
            if let Some(base) = &terms.base {
                base.native()?;
            }
            if let Some(delegation) = &terms.delegation {
                delegation.native()?;
            }
            for (persona, change) in &terms.changes {
                identity(persona)?;
                p::check_id(&change.prior_acceptance, "arrangement.prior_acceptance")?;
                require(
                    change.terms.mandate_revision > 0,
                    "arrangement mandate revision is zero",
                )?;
                text(&change.terms.exact_terms, 16384)?;
                let raw = &value["terms"]["changes"][persona]["terms"];
                unique_ids(
                    &raw["outcomes"],
                    &change.terms.outcomes,
                    "arrangement.outcome",
                )?;
                unique_ids(
                    &raw["review_roles"],
                    &change.terms.review_roles,
                    "arrangement.review_role",
                )?;
            }
            match &terms.decision_rule {
                Some(ArrangementDecisionRuleInput::Designated { persona }) => identity(persona)?,
                Some(ArrangementDecisionRuleInput::Quorum { eligible, minimum }) => {
                    unique_ids(
                        &value["terms"]["decision_rule"]["eligible"],
                        eligible,
                        "arrangement.eligible",
                    )?;
                    require(
                        *minimum > 0 && *minimum <= eligible.len(),
                        "arrangement quorum threshold is invalid",
                    )?;
                    for persona in eligible {
                        identity(persona)?;
                    }
                }
                None => (),
            }
            let mut transferred = BTreeSet::new();
            for transfer in &terms.artifact_transfers {
                identity(&transfer.from)?;
                identity(&transfer.to)?;
                transfer.artifact.native()?;
                require(
                    transfer.from != transfer.to,
                    "arrangement artifact transfer must change owner",
                )?;
                require(
                    transferred.insert(&transfer.artifact.id),
                    "duplicate arrangement artifact transfer",
                )?;
            }
        }
        WorkAction::AcceptArrangement { proposal, exact }
        | WorkAction::DeclineArrangement {
            proposal, exact, ..
        } => {
            p::check_id(proposal, "arrangement.proposal")?;
            exact.native()?;
            if let WorkAction::DeclineArrangement { reason, .. } = action {
                text(reason, 4096)?;
            }
        }
        WorkAction::AcceptArrangedOffer {
            offer,
            terms_revision,
            arrangement,
        } => {
            p::check_id(offer, "arranged_offer.offer")?;
            require(
                terms_revision.len() == 64
                    && terms_revision
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "arranged offer terms revision must be a canonical digest",
            )?;
            arrangement.native()?;
        }
        _ => (),
    }
    Ok(())
}

fn proposal_projection(id: &w::RecordId, proposal: &w::ArrangementProposal) -> Value {
    json!({
        "id": id,
        "exact": WorkExactRef::from_native(&proposal.exact(id)),
        "terms": ArrangementTermsInput::from_native(&proposal.terms),
        "by": proposal.by,
        "consent_scope": "each projected consent is that persona's own exact proposal decision; omitted responses establish neither absence nor consent; adoption still requires every current acceptance, authority and duty-preservation check"
    })
}

/// Current arrangement-derived authority and commitments cannot escape their
/// exact activation's original restrictions through another core projection.
pub(super) fn retained_arrangement_origin(
    state: &State,
    work_id: &str,
    exact: &w::ExactRef,
    a: &f::Admission,
) -> Result<Exposure> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let id = &w::RecordId(exact.id.clone());
    let current = work
        .arrangements
        .get(id)
        .filter(|record| record.exact(id) == *exact)
        .ok_or_else(|| held("exact arrangement record unavailable"))?;
    let inherited = state.work_meta[work_id]
        .arrangement_origins
        .activations
        .get(&activation_key(work_id, id, current)?)
        .ok_or_else(|| held("current arrangement original lineage unavailable"))?;
    require(
        exposure_permitted(state, inherited, a),
        "current arrangement original lineage unavailable",
    )?;
    Ok(inherited.clone())
}

pub(super) fn require_current_origin(
    state: &State,
    work_id: &str,
    a: &f::Admission,
) -> Result<Exposure> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let mut inherited = Exposure::default();
    if let Some(exact) = work.formation_delegation_ref() {
        let original = state.work_meta[work_id]
            .arrangement_origins
            .delegations
            .get(&delegation_key(work_id, &exact)?)
            .ok_or_else(|| held("formation delegation original lineage unavailable"))?;
        require(
            exposure_permitted(state, original, a),
            "formation delegation original lineage unavailable",
        )?;
        extend_exposure(&mut inherited, original);
    }
    if let Some(exact) = work.arrangement_ref() {
        extend_exposure(
            &mut inherited,
            &retained_arrangement_origin(state, work_id, &exact, a)?,
        );
    }
    Ok(inherited)
}

fn permitted_proposal(
    state: &State,
    work_id: &str,
    id: &w::RecordId,
    proposal: &w::ArrangementProposal,
    a: &f::Admission,
) -> Result<Option<(Value, Exposure)>> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let origins = &state.work_meta[work_id].arrangement_origins;
    let Some(inherited) = origins
        .proposals
        .get(&proposal_key(work_id, id, proposal)?)
        .filter(|origin| exposure_permitted(state, origin, a))
    else {
        return Ok(None);
    };
    let mut inherited = inherited.clone();
    let mut value = proposal_projection(id, proposal);
    let mut consents = serde_json::Map::new();
    let mut declines = serde_json::Map::new();
    let mut omitted = false;
    for (persona, consent) in &proposal.consents {
        if let Some(origin) = origins
            .responses
            .get(&response_key(
                work_id, id, proposal, "consent", persona, consent,
            )?)
            .filter(|origin| exposure_permitted(state, origin, a))
        {
            consents.insert(persona.0.clone(), serde_json::to_value(consent)?);
            extend_exposure(&mut inherited, origin);
        } else {
            omitted = true;
        }
    }
    for (persona, decline) in &proposal.declines {
        if let Some(origin) = origins
            .responses
            .get(&response_key(
                work_id, id, proposal, "decline", persona, decline,
            )?)
            .filter(|origin| exposure_permitted(state, origin, a))
        {
            declines.insert(persona.0.clone(), serde_json::to_value(decline)?);
            extend_exposure(&mut inherited, origin);
        } else {
            omitted = true;
        }
    }
    value["consents"] = Value::Object(consents);
    value["declines"] = Value::Object(declines);
    value["responses_omitted"] = json!(omitted);
    value["adopted"] = Value::Null;
    value["adoption_omitted"] = json!(false);
    if let Some(exact) = &proposal.adopted {
        let origin = work
            .arrangements
            .get(id)
            .map(|record| activation_key(work_id, id, record))
            .transpose()?
            .and_then(|key| origins.activations.get(&key))
            .filter(|origin| exposure_permitted(state, origin, a));
        if let Some(origin) = origin {
            value["adopted"] = json!(WorkExactRef::from_native(exact));
            extend_exposure(&mut inherited, origin);
        } else {
            value["adoption_omitted"] = json!(true);
        }
    }
    Ok(Some((value, inherited)))
}

pub(super) fn retained_acceptance_origin(
    state: &State,
    work_id: &str,
    id: &w::RecordId,
    a: &f::Admission,
) -> Result<Option<Exposure>> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let acceptance = work
        .acceptances
        .get(id)
        .ok_or_else(|| held("exact acceptance unavailable"))?;
    let origin = state.work_meta[work_id]
        .arrangement_origins
        .acceptances
        .get(&acceptance_key(work_id, id, acceptance)?);
    if acceptance.arrangement.is_some() {
        require(
            origin.is_some_and(|origin| exposure_permitted(state, origin, a)),
            "arrangement acceptance original lineage unavailable",
        )?;
    }
    Ok(origin.cloned())
}

/// Complete current records with exact provenance. A genuinely unavailable
/// record is identified as omitted and never establishes absence or consent.
pub(super) fn context(
    state: &State,
    work_id: &str,
    actor: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Value> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    extend_exposure(exposure, &require_current_origin(state, work_id, a)?);
    let current_exact = work.arrangement_ref();
    let effective_rule = work.effective_decision_rule();
    let mut projection = json!({
        "delegation_exact": work.formation_delegation_ref().as_ref().map(WorkExactRef::from_native),
        "current_exact": current_exact.as_ref().map(WorkExactRef::from_native),
        "effective_decision_rule": effective_rule.as_ref().map(ArrangementDecisionRuleInput::from_native),
        "current_arrangement": Value::Null,
        "current_arrangement_omitted": false,
        "current_commitments": [],
        "current_commitments_omitted": false,
        "proposals": [],
        "proposals_omitted": false,
        "maximum_proposals": null,
        "maximum_projection_bytes": null,
        "quotas_enforced":false,
        "participant_pool_enforced":false,
        "omission_semantics": "Complete available current work-core metadata without count or byte quotas. Unavailable records establish neither absence nor consent. No file body, peer wake or resume is supplied.",
        "consent_rule": "Copy a supplied proposal id and exact reference to accept or decline for yourself. Merely proposing grants no consent; consent_own=true explicitly consents only for the author. Under AffectedPartyAssentV2, consent is required from changed commitment parties, both sides of artifact transfers, and every old and new decision holder when the result rule changes. An unchanged contributor has no additional approval duty. Activation is atomic under the exact formation delegation; every affected acceptance and current base is rechecked. This establishes the arrangement, not peer endorsement of an artifact or task completion.",
        "joining_rule": "When current_exact is present, a new participant must use work command accept_arranged_offer with offer, terms_revision and arrangement=current_exact to accept the supplied invitation and current arrangement explicitly. Responsibility accept is available only before an arrangement exists. Responsibility decline remains available; inspecting an arrangement is not consent."
    });
    let mut proposals = Vec::new();
    // Stable identity order never ranks records using a withheld response.
    for (id, proposal) in &work.arrangement_proposals {
        let Some((mut value, inherited)) = permitted_proposal(state, work_id, id, proposal, a)?
        else {
            projection["proposals_omitted"] = json!(true);
            continue;
        };
        value["base_current"] = json!(proposal.terms.base == current_exact);
        value["required_personal_consents"] =
            json!(work.arrangement_required_consents(&proposal.terms));
        value["formation_assent_policy"] =
            json!(work.arrangement_governance_required(&proposal.terms));
        value["prior_acceptances_current"] =
            json!(proposal.terms.changes.iter().all(|(persona, change)| {
                work.acceptance(persona)
                    .is_some_and(|(id, _)| id == &change.prior_acceptance)
            }));
        proposals.push(value);
        extend_exposure(exposure, &inherited);
    }
    projection["proposals"] = json!(proposals);
    let mut commitments = Vec::new();
    // Include the deciding persona's commitment first, then stable lexical order.
    let participants = work
        .participants
        .keys()
        .filter(|persona| persona.0 == actor)
        .chain(
            work.participants
                .keys()
                .filter(|persona| persona.0 != actor),
        );
    for persona in participants {
        let Some((id, acceptance)) = work.acceptance(persona) else {
            continue;
        };
        let inherited = match retained_acceptance_origin(state, work_id, id, a) {
            Ok(inherited) => inherited,
            Err(_) => {
                projection["current_commitments_omitted"] = json!(true);
                continue;
            }
        };
        let value = json!({"persona": persona, "acceptance_id": id,
            "terms": ArrangementCommitment::from_native(&acceptance.terms), "by": acceptance.by});
        commitments.push(value);
        if let Some(inherited) = inherited {
            extend_exposure(exposure, &inherited);
        }
    }
    // Report every adopted outcome explicitly, including gaps. Participation
    // with an empty outcome set is not accepted ownership. Preserve unavailable
    // provenance as unknown rather than inventing either a gap or an owner.
    let outcome_accountability = work.mandate().outcomes.keys().map(|outcome| {
        let account = match work.ownership(outcome) {
            w::Accountability::Gap(reason) => json!({"status":"gap","reason":reason}),
            w::Accountability::Owner(persona) => {
                match commitments.iter().find(|entry| entry["persona"] == persona.0) {
                    Some(entry) => json!({"status":"accepted_owner","persona":persona,
                        "acceptance_id":entry["acceptance_id"]}),
                    None => json!({"status":"unavailable",
                        "reason":"Current ownership metadata cannot be supplied with its acceptance provenance"}),
                }
            }
        };
        (outcome.0.clone(), account)
    }).collect::<serde_json::Map<String, Value>>();
    projection["outcome_accountability"] = json!(outcome_accountability);
    projection["accountability_meaning"] = json!("Current required outcomes and accepted ownership, not artifact quality or completion. Contribution, file editing and a sufficiency judgment do not accept outcome ownership or supply result decision rights. A gap assigns nobody; participants choose any useful voluntary arrangement.");
    projection["current_commitments"] = json!(commitments);
    if let Some(current_id) = &work.current_arrangement {
        let current = work
            .arrangements
            .get(current_id)
            .ok_or_else(|| held("current arrangement record unavailable"))?;
        let value = json!({"id": current_id, "revision": current.revision,
            "proposal": WorkExactRef::from_native(&current.proposal),
            "by": current.by, "acceptances": current.acceptances, "consents": current.consents});
        projection["current_arrangement"] = value;
    }
    Ok(projection)
}

fn supplied_control(prepared: &Prepared) -> Result<Value> {
    require(
        s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256,
        "arrangement prepared prompt hash mismatch",
    )?;
    let control = request_context::decode(prepared)?.control;
    require(
        control["actor"].as_str() == Some(prepared.request.persona.as_str())
            && control["work_id"].as_str() == Some(prepared.request.work.as_str())
            && control["invocation_id"].as_str() == Some(prepared.request.id.as_str())
            && control["work_version"] == serde_json::to_value(prepared.work_version)?,
        "arrangement proposal supply ownership mismatch",
    )?;
    Ok(control)
}

/// Actual admitted CONTROL is the supply proof. Neither guessed record IDs,
/// same-response proposals, nor a later reconstructed projection can consent.
pub(super) fn require_supplied(
    state: &State,
    prepared: &Prepared,
    proposal: &str,
    exact: &WorkExactRef,
    a: &f::Admission,
) -> Result<w::ExactRef> {
    let work = state
        .board
        .work(&w::WorkId(prepared.request.work.clone()))?;
    let control = supplied_control(prepared)?;
    let supplied = control["arrangements"]["proposals"]
        .as_array()
        .ok_or_else(|| held("exact arrangement proposal was not supplied"))?;
    let id = w::RecordId(proposal.to_owned());
    let current = work
        .arrangement_proposals
        .get(&id)
        .ok_or_else(|| held("arrangement proposal unavailable"))?;
    let native = exact.native()?;
    require(
        current.exact(&id) == native,
        "arrangement proposal exact identity changed",
    )?;
    let expected = proposal_projection(&id, current);
    require(
        permitted_proposal(state, &prepared.request.work, &id, current, a)?.is_some(),
        "arrangement proposal original lineage unavailable",
    )?;
    require(
        supplied.iter().any(|record| {
            record["id"] == expected["id"]
                && record["exact"] == expected["exact"]
                && record["terms"] == expected["terms"]
                && record["by"] == expected["by"]
        }),
        "exact arrangement proposal was not supplied",
    )?;
    Ok(native)
}

/// Joining an established arrangement requires both exact records actually
/// supplied to this actor. It does not reuse Responsibility as implicit assent.
pub(super) fn require_offer_supplied(
    state: &State,
    prepared: &Prepared,
    offer: &str,
    terms_revision: &str,
    arrangement: &WorkExactRef,
    a: &f::Admission,
) -> Result<(w::CommitmentTerms, w::ExactRef)> {
    let work = state
        .board
        .work(&w::WorkId(prepared.request.work.clone()))?;
    let control = supplied_control(prepared)?;
    let exact = arrangement.native()?;
    require(
        work.arrangement_ref().as_ref() == Some(&exact)
            && control["arrangements"]["current_exact"] == serde_json::to_value(arrangement)?,
        "joining requires the exact supplied current arrangement",
    )?;
    require_current_origin(state, &prepared.request.work, a)?;
    let id = w::RecordId(offer.into());
    let current = work
        .offers
        .get(&id)
        .ok_or_else(|| held("arranged offer unavailable"))?;
    require(
        current.to.0 == prepared.request.persona && offer_revision(work, &id)? == terms_revision,
        "arranged offer terms or actor mismatch",
    )?;
    let supplied = control["own_pending_offers"]
        .as_array()
        .ok_or_else(|| held("exact arranged offer was not supplied"))?;
    let terms = serde_json::to_value(&current.terms)?;
    require(
        supplied.iter().any(|item| {
            item["id"].as_str() == Some(offer)
                && item["terms_revision"].as_str() == Some(terms_revision)
                && item["terms"] == terms
        }),
        "exact arranged offer was not supplied",
    )?;
    Ok((current.terms.clone(), exact))
}
