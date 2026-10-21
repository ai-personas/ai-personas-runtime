//! Pure need construction after the caller has established creation authority
//! and resolved the exact origin and exposure. Allocations are logical ceilings;
//! construction neither reserves Store resources nor persists a candidate.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkAllocation {
    pub attempts: u64,
    pub navigation: f::NavigationLimits,
    pub repair_attempts: u64,
}

impl WorkAllocation {
    pub(super) fn configured(config: &Config) -> Self {
        Self {
            attempts: config.max_work_attempts,
            navigation: config.navigation_per_work,
            repair_attempts: config.max_repair_attempts,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        require(
            self.attempts > 0 && self.repair_attempts <= self.attempts,
            "need allocation requires positive attempts and a repair suballowance within those attempts",
        )
    }

    pub(super) fn require_within_config(&self, config: &Config) -> Result<()> {
        self.validate()?;
        let available = config.navigation_per_work;
        let requested = self.navigation;
        require(
            self.attempts <= config.max_work_attempts
                && self.repair_attempts <= config.max_repair_attempts
                && requested.calls <= available.calls
                && requested.scanned_bytes <= available.scanned_bytes
                && requested.inspected_revisions <= available.inspected_revisions
                && requested.matches <= available.matches
                && requested.result_bytes <= available.result_bytes,
            "explicit host allocation exceeds existing configured need ceilings; this API grants no cap extension or delegated funding",
        )
    }
}

/// These are the existing host-intake structural checks. Origin authentication
/// remains in Runtime::add_need_allocated, including the retained adopted bytes,
/// exact authored request and current exposure checks for PersonaResponse.
pub(super) fn validate_input(operator: &str, input: &NeedInput) -> Result<()> {
    identity(operator)?;
    identity(&input.id)?;
    require(!input.id.starts_with(lifecycle::SLOT_PREFIX), "reserved persona control namespace")?;
    text(&input.request, 32_768)?;
    for value in [
        &input.boundaries,
        &input.authorization,
        &input.assumptions,
        &input.terms,
    ] {
        text(value, 16_384)?;
    }
    for invitation in &input.invitations {
        p::check_id(&invitation.id, "invitations.id")?;
    }
    Ok(())
}

/// The caller supplies an authenticated origin and host-derived exposure.
/// This helper is deliberately private: none of these arguments are a public
/// JSON authority assertion. The actual host operator remains the creation and
/// formation-delegation issuer even when exact need authorship is persona-origin.
///
/// This preserves existing host-intake invitation permits. A delegated child
/// constructor must separately implement its initial-recipient scheduling gate;
/// this helper alone does not implement delegated intake.
pub(super) fn construct_host_need(
    current: &State,
    operator: &str,
    input: &NeedInput,
    allocation: WorkAllocation,
    origin: w::Origin,
    need_exposure: Exposure,
    authored_origin: Option<Value>,
    optional_activity: Option<w::OptionalActivity>,
) -> Result<State> {
    validate_input(operator, input)?;
    allocation.validate()?;
    let wid = w::WorkId(input.id.clone());
    require(
        !current.board.works().contains_key(&wid) && !current.work_meta.contains_key(&input.id),
        "need identity already exists in the board or resource accounts",
    )?;
    let authority = work_ref(&input.id, "authority", &input.authorization)?;
    let mandate = mandate(input, 1, authority.clone())?;
    let context = w::RuntimeContext::from_runtime(
        wid,
        w::Version {
            revision: 0,
            epoch: 0,
        },
        authority,
        w::Origin::Human(operator.into()),
        w::RuntimeRight::NewAuthorization,
    );
    let mut next = current.clone();
    next.board.create(
        &context,
        w::Need {
            origin,
            request: input.request.clone(),
            supplied: BTreeSet::new(),
            boundaries: work_ref(&input.id, "boundaries", &input.boundaries)?,
            optional_activity,
        },
        mandate,
    )?;
    for invitation in &input.invitations {
        observe(
            &mut next,
            operator,
            &input.id,
            w::RuntimeRight::Governance,
            w::RuntimeOperation::Invite {
                id: w::RecordId(invitation.id.clone()),
                to: w::PersonaId(invitation.to.clone()),
                terms: invitation.terms.clone(),
            },
        )?;
        next.personas
            .get_mut(&invitation.to)
            .ok_or_else(|| held("invited persona absent"))?
            .focus
            .entry(input.id.clone())
            .or_insert_with(|| Focus {
                permit: true,
                ..Focus::default()
            });
    }
    let arrangement_origins = arrangements::Origins::for_need(&next, &input.id, &need_exposure)?;
    next.work_meta.insert(
        input.id.clone(),
        WorkMeta {
            original: input.clone(),
            amendments: Vec::new(),
            intake_operator: operator.into(),
            attempts_left: allocation.attempts,
            navigation_left: allocation.navigation,
            repair_attempts_left: allocation.repair_attempts,
            repair_attempts_declared: None,
            artifact_sources: BTreeMap::new(),
            human_evidence: BTreeMap::new(),
            model_evidence: BTreeMap::new(),
            completion_exposures: BTreeMap::new(),
            arrangement_origins,
            local_dependencies: BTreeMap::new(),
            message_waits: BTreeMap::new(),
            questions: BTreeMap::new(),
            exposure: Some(need_exposure),
            authored_origin,
        },
    );
    next.operator_events
        .push(json!({"kind":"need_intake","operator":operator,"work":input.id}));
    Ok(next)
}

/// Exact typed scope construction shared by initial intake and authorized
/// amendments. Revision one preserves the original reference identities.
pub(super) fn mandate(input: &NeedInput, revision: u64, authority: w::ExactRef) -> Result<w::Mandate> {
    let reference = |label: &str, value: &str| -> Result<w::ExactRef> {
        text(value, 32_768)?;
        Ok(w::ExactRef::new(format!("{}/{label}", input.id), revision, value.as_bytes()))
    };
    Ok(w::Mandate {
        revision,
        terms: input.terms.clone(),
        authority,
        assumptions: reference("assumptions", &input.assumptions)?,
        outcomes: input
            .outcomes
            .iter()
            .map(|(id, outcome)| {
                Ok((
                    w::OutcomeId(id.clone()),
                    w::RequiredOutcome {
                        description: outcome.description.clone(),
                        criterion: reference(
                            &format!("criterion/{id}"),
                            &outcome.criterion,
                        )?,
                        required_reviews: outcome
                            .required_reviews
                            .iter()
                            .cloned()
                            .map(w::RoleId)
                            .collect(),
                    },
                ))
            })
            .collect::<Result<_>>()?,
        review_roles: input
            .review_roles
            .iter()
            .map(|(id, role)| {
                Ok((
                    w::RoleId(id.clone()),
                    w::ReviewRole {
                        terms: role.terms.clone(),
                        funding: reference(&format!("funding/{id}"), &role.funding)?,
                        policy: reference(&format!("review-policy/{id}"), &role.policy)?,
                        independence: role.independence,
                    },
                ))
            })
            .collect::<Result<_>>()?,
        decision_rule: input.decision_rule.clone(),
        delivery_to: input.delivery_to.clone(),
        stakeholders: input.stakeholders.clone(),
    })
}
