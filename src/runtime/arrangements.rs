//! Bounded persona-authored arrangement inputs and admitted proposal supply.
//! These projections inherit the work core's current permission checks. They
//! grant neither authority nor resources and never change peer continuation.
use super::*;

const MAX_CHANGES: usize = 32;
const MAX_AUTHORITY_HOLDERS: usize = 16;
const MAX_PROPOSALS: usize = 32;
const MAX_COMMITMENTS: usize = 256;
const MAX_CONTROL_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_ORIGINS: usize = MAX_PROPOSALS * 256;
const MAX_ACCEPTANCE_ORIGINS: usize = MAX_PROPOSALS * MAX_CHANGES;
const MAX_ORIGIN_BYTES: usize = 2 * 1024 * 1024;

/// Host-retained immutable origins. Empty historical maps establish no lineage.
/// The maps are separately bounded; no later work exposure backfills a record.
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

    fn validate_bounds(&self) -> Result<()> {
        require(
            self.delegations.len() <= 1
                && self.proposals.len() <= MAX_PROPOSALS
                && self.responses.len() <= MAX_RESPONSE_ORIGINS
                && self.activations.len() <= MAX_PROPOSALS
                && self.acceptances.len() <= MAX_ACCEPTANCE_ORIGINS,
            "arrangement origin count limit exceeded",
        )?;
        require(
            serde_json::to_vec(self)?.len() <= MAX_ORIGIN_BYTES,
            "arrangement origin retention limit exceeded",
        )
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
        result.validate_bounds()?;
        Ok(result)
    }
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
    origins.validate_bounds()?;
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

pub(super) const DUTIES: &str = "Persona arrangements: CONTROL.arrangements reports bounded current work-core metadata; omitted records establish neither absence nor permission. A PersonaArranged mandate delegates only the listed eligible persona pool and supplies no initial owner or decision rule. An accepted participant within that pool may propose exact voluntary commitment changes and an eligible Designated or Quorum rule. ProposeArrangement.consent_own is required: false records the proposal alone; true explicitly records only the proposer's own consent to those exact authored terms and can activate the proposal if every required consent and authorization is then satisfied. For a separate accept or decline action, copy the proposal id and exact {id,revision,digest} from this admitted CONTROL; digest is lowercase hexadecimal. Work exact identities are distinct from file ExactRefs and ArtifactKeys. Consent is only your own, bound to the exact proposal and current accepted responsibility. Copy delegation=CONTROL.arrangements.delegation_exact in every dynamic proposal. The requester's immutable AffectedPartyAssentV1 delegation governs arrangement changes: a result-rule change requires every actual current accepted contributor's exact consent, including later joiners, plus every affected duty or artifact holder. A result-decider role alone grants no amendment authority. The core rechecks current base, prior acceptances, authority and preserved duty coverage at atomic activation. Fixed legacy decision rules permit duty-only arrangements with decision_rule=null. An arrangement transfers no file-source ownership, selects no body, supplies no resource or resume permit and wakes no peer. Once current_exact is present, joining requires accept_arranged_offer with the exact supplied offer, terms_revision and current arrangement; a Responsibility accept cannot imply assent to an arrangement. Before formation, existing responsibility operations retain their exact offer semantics.";

/// Exact work-record identity. This is not a file reference or an ArtifactKey.
/// digest is exactly 64 lowercase hexadecimal characters, not a byte array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkExactRef {
    #[schemars(length(min = 1, max = 256))]
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
        #[schemars(length(min = 1, max = 128))]
        persona: String,
    },
    Quorum {
        #[schemars(length(min = 1, max = 16))]
        #[schemars(extend("items" = {"type": "string", "minLength": 1, "maxLength": 128}))]
        eligible: BTreeSet<String>,
        #[schemars(range(min = 1, max = 16))]
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
    /// Nonblank, at most 16384 UTF-8 bytes.
    #[schemars(length(min = 1, max = 16384))]
    pub exact_terms: String,
    #[schemars(length(max = 256))]
    #[schemars(extend("items" = {"type": "string", "minLength": 1, "maxLength": 256}))]
    pub outcomes: BTreeSet<String>,
    #[schemars(length(max = 256))]
    #[schemars(extend("items" = {"type": "string", "minLength": 1, "maxLength": 256}))]
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
    #[schemars(length(min = 1, max = 256))]
    pub prior_acceptance: String,
    pub terms: ArrangementCommitment,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTransferInput {
    #[schemars(length(min = 1, max = 128))]
    pub from: String,
    #[schemars(length(min = 1, max = 128))]
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
    /// Only changed commitments: at most 32 persona keys, each naming its exact
    /// current acceptance and genuinely different successor terms. Omit people
    /// whose terms stay unchanged; their commitments are retained automatically.
    /// Required consent is computed separately: a decision-rule change still
    /// requires every accepted participant's consent, including omitted people.
    #[schemars(extend("maxProperties" = 32,
        "propertyNames" = {"type": "string", "minLength": 1, "maxLength": 128}))]
    pub changes: BTreeMap<String, CommitmentChangeInput>,
    /// Null preserves the current rule. Fixed legacy mandates cannot replace it.
    pub decision_rule: Option<ArrangementDecisionRuleInput>,
    /// Explicit work-artifact ownership only; never transfers file-source ownership.
    #[schemars(length(max = 32))]
    pub artifact_transfers: Vec<ArtifactTransferInput>,
    /// Nonblank, at most 4096 UTF-8 bytes.
    #[schemars(length(min = 1, max = 4096))]
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
        Some("accept_arranged_offer") => object(&value["arrangement"])?,
        _ => (),
    }
    Ok(())
}

fn unique_ids(raw: &Value, values: &BTreeSet<String>, maximum: usize, label: &str) -> Result<()> {
    require(
        values.len() <= maximum
            && raw
                .as_array()
                .is_some_and(|items| items.len() == values.len()),
        "arrangement set must be bounded and unique",
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
            require(
                terms.changes.len() <= MAX_CHANGES,
                "too many arrangement commitment changes",
            )?;
            require(
                terms.artifact_transfers.len() <= MAX_CHANGES,
                "too many arrangement artifact transfers",
            )?;
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
                    256,
                    "arrangement.outcome",
                )?;
                unique_ids(
                    &raw["review_roles"],
                    &change.terms.review_roles,
                    256,
                    "arrangement.review_role",
                )?;
            }
            match &terms.decision_rule {
                Some(ArrangementDecisionRuleInput::Designated { persona }) => identity(persona)?,
                Some(ArrangementDecisionRuleInput::Quorum { eligible, minimum }) => {
                    unique_ids(
                        &value["terms"]["decision_rule"]["eligible"],
                        eligible,
                        MAX_AUTHORITY_HOLDERS,
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

/// Invoked only after the existing work-core policy and lineage checks. Count
/// and byte omissions are explicit; a missing record never proves absence.
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
    let maximum = MAX_CONTROL_BYTES.min(state.config.context_bytes / 4);
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
        "maximum_proposals": MAX_PROPOSALS,
        "maximum_projection_bytes": maximum,
        "omission_semantics": "Bounded current permitted work-core metadata only. Omitted records establish neither absence nor consent nor permission. No history traversal, file body, peer wake, resume, or resources are supplied.",
        "consent_rule": "Copy a supplied proposal id and exact reference to accept or decline for yourself. Merely proposing grants no consent; consent_own=true explicitly consents only for the author. Activation is atomic only after all required personal consents under the immutable formation delegation; every acceptance and current base is rechecked."
    });
    let mut bytes = serde_json::to_vec(&projection)?.len();
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
        let size = serde_json::to_vec(&value)?.len() + 1;
        if proposals.len() < MAX_PROPOSALS
            && bytes.checked_add(size).is_some_and(|sum| sum <= maximum)
        {
            proposals.push(value);
            extend_exposure(exposure, &inherited);
            bytes += size;
        } else {
            projection["proposals_omitted"] = json!(true);
        }
    }
    projection["proposals"] = json!(proposals);
    let mut commitments = Vec::new();
    // Consent proposals take precedence over general ownership metadata. Include
    // the deciding persona's commitment first, then stable lexical participant order.
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
        let size = serde_json::to_vec(&value)?.len() + 1;
        if commitments.len() < MAX_COMMITMENTS
            && bytes.checked_add(size).is_some_and(|sum| sum <= maximum)
        {
            commitments.push(value);
            if let Some(inherited) = inherited {
                extend_exposure(exposure, &inherited);
            }
            bytes += size;
        } else {
            projection["current_commitments_omitted"] = json!(true);
        }
    }
    projection["current_commitments"] = json!(commitments);
    if let Some(current_id) = &work.current_arrangement {
        let current = work
            .arrangements
            .get(current_id)
            .ok_or_else(|| held("current arrangement record unavailable"))?;
        let value = json!({"id": current_id, "revision": current.revision,
            "proposal": WorkExactRef::from_native(&current.proposal),
            "by": current.by, "acceptances": current.acceptances, "consents": current.consents});
        let size = serde_json::to_vec(&value)?.len();
        if bytes.checked_add(size).is_some_and(|sum| sum <= maximum) {
            projection["current_arrangement"] = value;
        } else {
            projection["current_arrangement_omitted"] = json!(true);
        }
    }
    // Count actual serialized UTF-8, including keys, escaping and delimiters.
    require(
        serde_json::to_vec(&projection)?.len() <= maximum,
        "bounded arrangement control metadata exceeds its allowance",
    )?;
    Ok(projection)
}

fn supplied_control(prepared: &Prepared) -> Result<Value> {
    require(
        s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256,
        "arrangement prepared prompt hash mismatch",
    )?;
    let control = prepared
        .prompt
        .split_once("\nCONTROL\n")
        .and_then(|(_, tail)| tail.split_once("\nQUALIFIED_CONTEXT\n"))
        .ok_or_else(|| held("arrangement proposal supply unavailable"))?
        .0;
    let control: Value = serde_json::from_str(control)?;
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
    require(
        supplied.len() <= MAX_PROPOSALS,
        "arrangement supplied proposal bound exceeded",
    )?;
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
