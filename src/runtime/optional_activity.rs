//! Bounded optional episodes. An absent opportunity never authorizes activity.
//! Host grants describe an environment and limits, not topics or required output.
use super::*;

pub(crate) const MAX_OPPORTUNITIES: usize = 32;
pub(crate) const MAX_EPISODES: usize = 32;
pub(crate) const MAX_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpportunityRequest {
    pub id: String,
    pub operator: String,
    pub authorization_reference: String,
    pub authorize_optional_activity: bool,
    pub participants: BTreeSet<String>,
    pub policy_id: String,
    pub sources: Vec<p::ExactRef>,
    pub not_before_unix: i64,
    pub expires_at_unix: i64,
    pub episode_seconds: u64,
    pub minimum_interval_seconds: u64,
    pub max_episodes: u32,
    pub allocation: WorkAllocation,
    pub per_episode: WorkAllocation,
    pub model: String,
    pub effort: String,
    pub destination: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Opportunity {
    pub request: OpportunityRequest,
    pub reference: w::ExactRef,
    pub exposure: Exposure,
    pub remaining: WorkAllocation,
    pub episodes: BTreeMap<String, Episode>,
    pub blocked_starts: BTreeMap<String, Value>,
    pub revoked_at_unix: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Episode {
    pub trigger: String,
    pub work: String,
    pub started_at_unix: i64,
    pub expires_at_unix: i64,
    pub allocation: WorkAllocation,
    #[serde(default)]
    pub admissions: Vec<AdmissionRecord>,
    pub ended: Option<Ending>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdmissionRecord {
    pub invocation: String,
    pub actor: String,
    pub at_unix: i64,
    pub exposure: Option<Exposure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ending {
    pub invocation: String,
    pub at_unix: i64,
    pub exposure: Exposure,
    pub reason: String,
}

impl OpportunityRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        identity(&self.id)?;
        identity(&self.operator)?;
        identity(&self.policy_id)?;
        identity(&self.model)?;
        identity(&self.effort)?;
        identity(&self.destination)?;
        text(&self.authorization_reference, 4096)?;
        require(
            self.authorize_optional_activity
                && !self.authorization_reference.trim().is_empty()
                && self.id.len() <= 96
                && (1..=8).contains(&self.participants.len())
                && self.sources.len() <= 16
                && self.not_before_unix >= 0
                && self.expires_at_unix > self.not_before_unix
                && (1..=86_400).contains(&self.episode_seconds)
                && self.minimum_interval_seconds <= 31_536_000
                && (1..=MAX_EPISODES as u32).contains(&self.max_episodes)
                && (self.max_episodes == 1 || self.minimum_interval_seconds > 0),
            "explicit finite optional-activity opportunity required",
        )?;
        for participant in &self.participants {
            identity(participant)?;
        }
        self.allocation.validate()?;
        self.per_episode.validate()?;
        need_proposals::subtract(self.allocation, self.per_episode)?;
        require(
            self.allocation.attempts <= 4096
                && self.per_episode.attempts <= 128
                && serde_json::to_vec(self)?.len() <= 32_768,
            "optional-activity allocation or request bound",
        )
    }
    pub(crate) fn reference(&self) -> Result<w::ExactRef> {
        Ok(w::ExactRef::new(
            format!("optional-activity:{}", self.id),
            1,
            &serde_json::to_vec(self)?,
        ))
    }
}

pub(crate) fn bounds(state: &State) -> Result<()> {
    require(
        state.optional_activities.len() <= MAX_OPPORTUNITIES
            && serde_json::to_vec(&state.optional_activities)?.len() <= MAX_BYTES,
        "optional-activity retention bound",
    )
}

pub(crate) fn bound(state: &State, work: &str) -> bool {
    state
        .board
        .work(&w::WorkId(work.into()))
        .is_ok_and(|work| work.need.optional_activity.is_some())
}

fn binding<'a>(state: &'a State, work: &str) -> Result<(&'a Opportunity, &'a Episode)> {
    let activity = state
        .board
        .work(&w::WorkId(work.into()))?
        .need
        .optional_activity
        .as_ref()
        .ok_or_else(|| held("optional-activity binding absent"))?;
    let opportunity = state
        .optional_activities
        .values()
        .find(|opportunity| opportunity.reference == activity.opportunity)
        .ok_or_else(|| held("optional-activity opportunity unavailable"))?;
    let episode = opportunity
        .episodes
        .get(&activity.episode)
        .ok_or_else(|| held("optional-activity episode unavailable"))?;
    require(
        opportunity.reference == opportunity.request.reference()?
            && episode.trigger == activity.episode
            && episode.work == work
            && u64::try_from(episode.expires_at_unix).ok() == Some(activity.expires_at_unix),
        "optional-activity binding mismatch",
    )?;
    Ok((opportunity, episode))
}

/// Inspect the complete bounded root, never just one scheduler page. No work
/// identity or private payload is included in the resulting availability reason.
pub(crate) fn foreground_eligible(state: &State, now: i64) -> Result<bool> {
    bounds(state)?;
    require(
        state.board.works().len() <= 128 && state.personas.len() <= 128,
        "optional-activity foreground projection exceeds its bound",
    )?;
    let optional_admissions: BTreeSet<_> = state
        .optional_activities
        .values()
        .flat_map(|o| o.episodes.values())
        .flat_map(|e| &e.admissions)
        .map(|a| (&a.actor, &a.invocation))
        .collect();
    for (id, work) in state.board.works() {
        if work.need.optional_activity.is_some() || !matches!(work.phase, w::Phase::Open) {
            continue;
        }
        let Some(meta) = state.work_meta.get(&id.0) else {
            continue;
        };
        for actor in work.participants.keys() {
            let Some(persona) = state.personas.get(&actor.0) else {
                continue;
            };
            if persona
                .active_invocation
                .as_ref()
                .is_some_and(|call| !optional_admissions.contains(&(&actor.0, call)))
            {
                return Ok(true);
            }
            if persona.failed_attempt_hold.is_some()
                || lifecycle::reason(state, &actor.0).is_some()
                || meta.attempts_left == 0
                || need_proposals::route_reason(state, &id.0, now).is_some()
                || need_proposals::actor_reason(state, &id.0, &actor.0).is_some()
                || tools::require_settled(state, &actor.0, &id.0).is_err()
                || http_tools::require_settled(state, &actor.0, &id.0).is_err()
                || host_tools::require_settled(state, &actor.0, &id.0).is_err()
            {
                continue;
            }
            let a = admission(persona, now);
            if !work_exposure(state, &id.0).is_ok_and(|e| exposure_permitted(state, &e, &a)) {
                continue;
            }
            if continuation_permit(state, &actor.0, &id.0)
                || ordinary_opportunity(state, &actor.0, &id.0, now)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Staged in the same transaction as actual preparation and reservation. A
/// failed preparation creates no admission record and cannot classify an
/// unrelated active invocation as optional.
pub(crate) fn record_admission(
    state: &mut State,
    request: &PrepareRequest,
    now: i64,
) -> Result<()> {
    if !bound(state, &request.work) {
        return Ok(());
    }
    let (opportunity, episode) = binding(state, &request.work)?;
    let key = opportunity.request.id.clone();
    let trigger = episode.trigger.clone();
    require(
        !episode
            .admissions
            .iter()
            .any(|a| a.invocation == request.id)
            && episode.admissions.len() < episode.allocation.attempts as usize,
        "optional episode admission identity or allocation exhausted",
    )?;
    state
        .optional_activities
        .get_mut(&key)
        .unwrap()
        .episodes
        .get_mut(&trigger)
        .unwrap()
        .admissions
        .push(AdmissionRecord {
            invocation: request.id.clone(),
            actor: request.persona.clone(),
            at_unix: now,
            exposure: None,
        });
    bounds(state)
}

/// The provisional marker above is needed during context assembly. Its exact
/// supplied-input restrictions are attached before the admission transaction.
pub(crate) fn seal_admission(state: &mut State, prepared: &Prepared) -> Result<()> {
    if !bound(state, &prepared.request.work) {
        return Ok(());
    }
    let (opportunity, episode) = binding(state, &prepared.request.work)?;
    let key = opportunity.request.id.clone();
    let trigger = episode.trigger.clone();
    let record = state
        .optional_activities
        .get_mut(&key)
        .unwrap()
        .episodes
        .get_mut(&trigger)
        .unwrap()
        .admissions
        .iter_mut()
        .find(|a| a.invocation == prepared.request.id)
        .ok_or_else(|| held("optional admission marker absent"))?;
    require(
        record.exposure.is_none(),
        "optional admission already sealed",
    )?;
    record.exposure = Some(prepared_exposure(prepared)?.clone());
    bounds(state)
}

pub(crate) fn require_current(state: &State, work: &str, actor: &str, now: i64) -> Result<()> {
    if !bound(state, work) {
        return Ok(());
    }
    lifecycle::require_active(state, actor)?;
    let (opportunity, episode) = binding(state, work)?;
    let request = &opportunity.request;
    require(
        opportunity.revoked_at_unix.is_none()
            && episode.ended.is_none()
            && request.not_before_unix <= now
            && now < request.expires_at_unix
            && episode.started_at_unix <= now
            && now < episode.expires_at_unix
            && request.participants.contains(actor),
        "optional-activity opportunity is no longer eligible",
    )?;
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("optional participant unavailable"))?;
    require(
        persona.seed.requested_model == request.model
            && persona.seed.requested_effort == request.effort
            && persona.seed.destination == request.destination
            && exposure_permitted(state, &opportunity.exposure, &admission(persona, now)),
        "optional-activity inference or source authority changed",
    )?;
    require(
        !foreground_eligible(state, now)?,
        "optional activity yields to eligible foreground work",
    )
}

pub(crate) fn route_reason(
    state: &State,
    work: &str,
    actor: &str,
    now: i64,
) -> Option<&'static str> {
    if bound(state, work) && require_current(state, work, actor, now).is_err() {
        Some("optional activity is unavailable under its current scope, expiry or foreground priority")
    } else {
        None
    }
}

pub(crate) fn require_recipient(state: &State, work: &str, recipient: &str) -> Result<()> {
    if !bound(state, work) {
        return Ok(());
    }
    let (opportunity, _) = binding(state, work)?;
    require(
        opportunity.request.participants.contains(recipient)
            || opportunity.request.operator == recipient,
        "optional communication is limited to the enabled participants and operator",
    )
}

/// An already admitted response may record an end after expiry or revocation,
/// but may perform no new discretionary action, communication or semantic edit.
pub(crate) fn require_response(
    state: &State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
) -> Result<()> {
    if !bound(state, &prepared.request.work) {
        return Ok(());
    }
    binding(state, &prepared.request.work)?;
    let closing = decision.operations.is_empty()
        && matches!(&decision.work, p::WorkDisposition::Ongoing { evidence, .. } if evidence.is_empty())
        && matches!(decision.reply, p::ReplyDisposition::None { .. })
        && matches!(
            decision.continuation,
            p::Continuation::Stop { .. } | p::Continuation::Pause { .. }
        )
        && [&decision.fragment_maintenance, &decision.asset_maintenance]
            .iter()
            .all(|m| {
                matches!(
                    m,
                    p::Maintenance::NoChange { .. }
                        | p::Maintenance::Blocked { .. }
                        | p::Maintenance::Deferred { .. }
                )
            })
        && [&decision.next_fragments, &decision.next_assets]
            .iter()
            .all(|s| {
                matches!(
                    s,
                    p::SelectionIntent::Preserve { .. } | p::SelectionIntent::Clear { .. }
                )
            });
    if closing {
        Ok(())
    } else {
        require_current(
            state,
            &prepared.request.work,
            &prepared.request.persona,
            now,
        )
    }
}

pub(crate) fn control(state: &State, work: &str) -> Result<Value> {
    if !bound(state, work) {
        return Ok(Value::Null);
    }
    let (opportunity, episode) = binding(state, work)?;
    Ok(
        json!({"opportunity":opportunity.reference,"episode":episode.trigger,
        "expires_at_unix":episode.expires_at_unix,"participants":opportunity.request.participants,
        "operator":opportunity.request.operator,
        "communication":"Messages and replies may address only the enabled participants and operator, subject to existing source and contact permissions. Availability creates no right to a response.",
        "purpose":"Optional self-directed activity within the existing local permissions. No assigned topic, artifact, review or delivery. Choose whether to engage, whom to approach, what to explore or discuss, and when to stop. Silence or refusal creates no duty. Other participants choose independently.",
        "funding":"One bounded episode allocation shared by all participants, tools and failed attempts. No reset, renewal, new participant allowance or automatic recurrence. Eligible foreground work has priority. Expiry or revocation prevents further discretionary admission.",
        "ending":"Choose Stop when you want to end participation. The host can record the activity ended once all invited participants have stopped and all executions are settled. No release, delivery, CloseGoal, artifact or characteristic edit is required. An ended activity establishes neither improvement nor task fulfillment.",
        "maintenance":"Every response still addresses work, fragment and asset maintenance and both next-context scopes. Character and actual experience shape choices; no change or no engagement is legitimate."}),
    )
}

pub(crate) fn finish_if_stopped(state: &mut State, prepared: &Prepared, now: i64) -> Result<()> {
    let work_id = &prepared.request.work;
    if !bound(state, work_id) {
        return Ok(());
    }
    let (opportunity, episode) = binding(state, work_id)?;
    if episode.ended.is_some() {
        return Ok(());
    }
    let work = state.board.work(&w::WorkId(work_id.clone()))?;
    if !matches!(work.phase, w::Phase::Open) {
        return Ok(());
    }
    // A retained uncertainty observation is not a terminal execution. Keep its
    // original episode and resources until explicit reconciliation or closeout.
    if state
        .computation_jobs
        .values()
        .chain(state.http_fetch_jobs.values())
        .chain(state.host_command_jobs.values())
        .any(|job| {
            &job.work == work_id
                && (!matches!(
                    job.state,
                    s::IntentState::Succeeded | s::IntentState::Failed | s::IntentState::Canceled
                ) || job.observation.is_none()
                    || job.result_source.is_none())
        })
    {
        return Ok(());
    }
    let mut exposure = work_exposure(state, work_id)?;
    for job in state
        .computation_jobs
        .values()
        .chain(state.http_fetch_jobs.values())
        .chain(state.host_command_jobs.values())
    {
        if &job.work == work_id {
            extend_exposure(&mut exposure, &job.exposure);
        }
    }
    for participant in work.participants.keys() {
        let Some(persona) = state.personas.get(&participant.0) else {
            return Ok(());
        };
        let Some(focus) = persona.focus.get(work_id) else {
            return Ok(());
        };
        let Some(authored) = &focus.authored_continuation else {
            return Ok(());
        };
        if persona.active_invocation.is_some()
            || persona.failed_attempt_hold.is_some()
            || !matches!(authored.value, p::Continuation::Stop { .. })
            || tools::require_settled(state, &participant.0, work_id).is_err()
            || http_tools::require_settled(state, &participant.0, work_id).is_err()
            || host_tools::require_settled(state, &participant.0, work_id).is_err()
        {
            return Ok(());
        }
        extend_exposure(&mut exposure, &authored.exposure);
    }
    let opportunity_id = opportunity.request.id.clone();
    let operator = opportunity.request.operator.clone();
    let trigger = episode.trigger.clone();
    let reason = "Every invited participant chose Stop; no assigned deliverable or continuing local execution remains. No behavioral benefit or artifact completion is inferred.";
    let exposure = expanded_exposure(state, &exposure)?;
    // Current lifecycle metadata depends on all of those dispositions too.
    // Preserve their restrictions in generic current-work projections, not only
    // in the dedicated optional-activity inspection.
    state.work_meta.get_mut(work_id).unwrap().exposure = Some(exposure.clone());
    observe(
        state,
        &operator,
        work_id,
        w::RuntimeRight::Governance,
        w::RuntimeOperation::End {
            kind: w::EndingKind::OptionalActivityEnded,
            reason: reason.into(),
        },
    )?;
    state
        .optional_activities
        .get_mut(&opportunity_id)
        .unwrap()
        .episodes
        .get_mut(&trigger)
        .unwrap()
        .ended = Some(Ending {
        invocation: prepared.request.id.clone(),
        at_unix: now,
        exposure,
        reason: reason.into(),
    });
    bounds(state)
}

/// Exact historical ending, permission-checked independently of current global
/// work status. Missing or inaccessible evidence establishes no absence claim.
pub(crate) fn ending_for_recipient(
    state: &State,
    work: &str,
    recipient: &str,
    now: i64,
) -> Result<Option<Value>> {
    if !bound(state, work) {
        return Ok(None);
    }
    let (opportunity, episode) = binding(state, work)?;
    let Some(ending) = &episode.ended else {
        return Ok(None);
    };
    let permitted = opportunity.request.operator == recipient
        && opportunity.request.participants.iter().any(|id| {
            let Some(persona) = state.personas.get(id) else {
                return false;
            };
            let mut view = admission(persona, now);
            view.audience = BTreeSet::from([recipient.into()]);
            persona.seed.audience.contains(recipient)
                && persona.seed.grants.communicate_with.contains(recipient)
                && exposure_permitted(state, &opportunity.exposure, &view)
                && exposure_permitted(state, &ending.exposure, &view)
        });
    Ok(permitted.then(|| json!({"opportunity":opportunity.reference,"episode":episode.trigger,
        "work":work,"invocation":ending.invocation,"at_unix":ending.at_unix,"reason":ending.reason,
        "artifact_completion":false,"behavioral_benefit_established":false,"current_eligibility_established":false})))
}
