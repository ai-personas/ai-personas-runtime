//! Explicit local opportunity registration and episode admission. No timer,
//! background inference, topic assignment or resource extension is implied.
use super::super::optional_activity as a;
use super::*;

pub use a::OpportunityRequest as OptionalActivityRequest;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalActivityStart {
    pub id: String,
    pub operator: String,
    pub opportunity: w::ExactRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalActivityRevocation {
    pub operator: String,
    pub opportunity: w::ExactRef,
    pub reason: String,
}

fn authorized(
    app: &mut Runtime,
    operator: &str,
) -> Result<(s::StoredObject, State, s::StoredObject, Saved)> {
    let (head, state) = app.load()?;
    let (launch, saved) = load(app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    a::bounds(&state)?;
    Ok((head, state, launch, saved))
}

fn inspectable(state: &State, opportunity: &a::Opportunity, operator: &str, now: i64) -> bool {
    opportunity.request.operator == operator
        && opportunity.request.participants.iter().any(|id| {
            state.personas.get(id).is_some_and(|persona| {
                let mut view = admission(persona, now);
                view.audience = BTreeSet::from([operator.into()]);
                persona.seed.audience.contains(operator)
                    && persona.seed.grants.communicate_with.contains(operator)
                    && exposure_permitted(state, &opportunity.exposure, &view)
            })
        })
}

pub fn optional_activity_snapshot(app: &mut Runtime, operator: &str, now: i64) -> Result<Value> {
    let (head, state, launch, saved) = authorized(app, operator)?;
    let mut opportunities = Vec::new();
    for opportunity in state.optional_activities.values() {
        if !inspectable(&state, opportunity, operator, now) {
            opportunities.push(json!({"status":"withheld_unavailable"}));
            continue;
        }
        let mut episodes = Vec::new();
        for episode in opportunity.episodes.values() {
            let admissions: Vec<_> = episode.admissions.iter().map(|record| {
                let permitted = record.exposure.as_ref().is_some_and(|exposure| {
                    state.personas.get(&record.actor).is_some_and(|persona| {
                        let mut view = admission(persona, now);
                        view.audience = BTreeSet::from([operator.into()]);
                        persona.seed.audience.contains(operator)
                            && persona.seed.grants.communicate_with.contains(operator)
                            && exposure_permitted(&state, exposure, &view)
                    })
                });
                if permitted {
                    json!({"invocation":record.invocation,"actor":record.actor,"at_unix":record.at_unix})
                } else {
                    json!({"status":"withheld_unavailable"})
                }
            }).collect();
            let ending = episode.ended.as_ref().filter(|ending| {
                opportunity.request.participants.iter().any(|id| {
                    let Some(persona) = state.personas.get(id) else {
                        return false;
                    };
                    let mut view = admission(persona, now);
                    view.audience = BTreeSet::from([operator.into()]);
                    exposure_permitted(&state, &ending.exposure, &view)
                })
            });
            episodes.push(json!({"trigger":episode.trigger,"work":episode.work,
                "started_at_unix":episode.started_at_unix,"expires_at_unix":episode.expires_at_unix,
                "allocation":episode.allocation,
                "admissions":admissions,
                "recorded_ending":ending.map(|e| json!({"invocation":e.invocation,"at_unix":e.at_unix,"reason":e.reason})),
                "ending_unavailable_is_not_absence":true}));
        }
        opportunities.push(
            json!({"reference":opportunity.reference,"request":opportunity.request,
            "remaining":opportunity.remaining,"revoked_at_unix":opportunity.revoked_at_unix,
            "environment_currently_enabled":opportunity.revoked_at_unix.is_none()
                && opportunity.request.not_before_unix <= now && now < opportunity.request.expires_at_unix,
            "start_eligibility_requires_fresh_admission":true,
            "episodes":episodes,"blocked_starts":opportunity.blocked_starts}),
        );
    }
    Ok(
        json!({"runtime":head.exact_ref()?,"launch":launch.exact_ref()?,"operator":operator,
        "steps":saved.steps,"max_total_steps":saved.setup.max_total_steps,
        "work_count":saved.works.len(),"max_works":saved.setup.max_works,
        "navigation_per_call":state.config.navigation_per_call,
        "configured_per_episode_ceiling":WorkAllocation::configured(&state.config),
        "opportunities":opportunities,"inference_performed":false,
        "funding":"Logical allocations inside existing launch, work and Store ceilings. This inspection reserves nothing and guarantees no future provider capacity."}),
    )
}

pub fn grant_optional_activity(
    app: &mut Runtime,
    request: &OptionalActivityRequest,
    now: i64,
) -> Result<Value> {
    request.validate()?;
    require(
        now >= 0 && request.expires_at_unix > now,
        "optional opportunity must expire in the future",
    )?;
    let _owner = app.store.acquire_execution_owner()?;
    let (head, mut state, _, saved) = authorized(app, &request.operator)?;
    if let Some(existing) = state.optional_activities.get(&request.id) {
        require(
            existing.request == *request,
            "optional opportunity identity conflict",
        )?;
        require(
            inspectable(&state, existing, &request.operator, now),
            "optional opportunity unavailable",
        )?;
        return Ok(
            json!({"reference":existing.reference,"replayed":true,"inference_performed":false}),
        );
    }
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks optional opportunity registration",
    )?;
    app.store.require_terminal_invocations()?;
    let provider = saved.execution_provider()?;
    require(
        request.model == provider.model
            && request.effort == provider.effort
            && request.destination == saved.setup.destination,
        "optional opportunity model, effort or destination mismatch",
    )?;
    request.per_episode.require_within_config(&state.config)?;
    require(
        request.allocation.attempts <= saved.setup.max_total_steps.saturating_sub(saved.steps)
            && saved.works.len() < saved.setup.max_works as usize,
        "optional opportunity exceeds existing launch capacity; no extension is granted",
    )?;
    let policy = current_policy(&state, &request.policy_id)?;
    let mut exposure = Exposure {
        sources: Vec::new(),
        policies: vec![scoped_policy(
            "optional-activity",
            &request.id,
            &digest(request)?,
            policy,
        )],
    };
    for reference in &request.sources {
        require(
            reference.lifetime.is_none(),
            "opportunity source references have no selection lifetime",
        )?;
        let reference = file_reference(reference)?;
        record(&state, &reference)?;
        exposure.sources.push(reference);
    }
    let exposure = expanded_exposure(&state, &exposure)?;
    let cohort: BTreeSet<_> = people(saved.setup.cohort).into_iter().collect();
    require(
        request.participants.is_subset(&cohort),
        "optional participants must belong to the saved cohort",
    )?;
    for id in &request.participants {
        lifecycle::require_active(&state, id)?;
        let persona = state
            .personas
            .get(id)
            .ok_or_else(|| held("optional participant absent"))?;
        require(
            persona.created_by == request.operator
                && persona.seed.requested_model == request.model
                && persona.seed.requested_effort == request.effort
                && persona.seed.destination == request.destination
                && exposure_permitted(&state, &exposure, &admission(persona, now)),
            "optional participant identity, model or source permission unavailable",
        )?;
    }
    let reference = request.reference()?;
    let opportunity = a::Opportunity {
        request: request.clone(),
        reference: reference.clone(),
        exposure,
        remaining: request.allocation,
        episodes: BTreeMap::new(),
        blocked_starts: BTreeMap::new(),
        revoked_at_unix: None,
    };
    require(
        inspectable(&state, &opportunity, &request.operator, now),
        "operator cannot inspect optional opportunity",
    )?;
    state
        .optional_activities
        .insert(request.id.clone(), opportunity);
    a::bounds(&state)?;
    app.store.commit_intake_delegation_batch(
        vec![s::Change {
            expected_revision: Some(head.revision),
            object: state_object(&head, &state)?,
            depends_on: Vec::new(),
        }],
        s::Resources {
            attempts: request.allocation.attempts,
            input_bytes: 1,
            output_bytes: 1,
            navigation_bytes: 1,
            retained_bytes: 1,
        },
    )?;
    Ok(
        json!({"reference":reference,"replayed":false,"inference_performed":false,
        "logical_allowance":request.allocation,"store_invocation_reserved":false,"automatic_recurrence":false}),
    )
}

pub fn optional_activity_snapshot_at(root: &Path, operator: &str, now: i64) -> Result<Value> {
    let mut app = Runtime::open_for_launch_inspection(root)?;
    optional_activity_snapshot(&mut app, operator, now)
}

fn input(
    saved: &Saved,
    request: &OptionalActivityRequest,
    activity: &w::OptionalActivity,
    id: &str,
) -> Result<NeedInput> {
    let prose = "An optional opportunity is available. Choose whether to engage in self-directed exploration, reflection or social exchange, what to pursue, whom to approach among eligible participants, and when to stop. There is no assigned topic, artifact, review, delivery or productivity requirement. Other personas may decline or remain silent. Existing character, source policies and permissions remain controlling.";
    let boundaries = "Local optional episode only, within its exact opportunity, participant pool, expiry and shared finite allowance. No automatic recurrence, new funding, new participants, inherited tool authority or nested intake delegation. Eligible foreground work takes priority.";
    let authorization = serde_json::to_string(&json!({"kind":"authorized_optional_episode",
        "opportunity":activity.opportunity,"episode":activity.episode,"expires_at_unix":activity.expires_at_unix,
        "authorization_reference":request.authorization_reference,"operator":saved.operator}))?;
    let need = w::Need {
        origin: w::Origin::Human(saved.operator.clone()),
        request: prose.into(),
        supplied: BTreeSet::new(),
        boundaries: work_ref(id, "boundaries", boundaries)?,
        optional_activity: Some(activity.clone()),
    };
    let delegation = w::FormationDelegation {
        revision: 1,
        issuer: w::Origin::Human(saved.operator.clone()),
        authorization: work_ref(id, "authority", &authorization)?,
        need: w::formation_need_ref(&w::WorkId(id.into()), &need),
        mandate_revision: 1,
        allocable_outcomes: BTreeSet::new(),
        allocable_review_roles: BTreeSet::new(),
        assent: w::ArrangementAssentPolicy::AffectedPartyAssentV1,
    };
    Ok(NeedInput {
        id: id.into(),
        origin: NeedOrigin::OptionalOpportunity {
            opportunity: activity.opportunity.clone(),
            episode: activity.episode.clone(),
        },
        request: prose.into(),
        boundaries: boundaries.into(),
        authorization,
        assumptions: "Engagement, an artifact, a character edit and later benefit are all optional and unestablished.".into(),
        terms: "Voluntary optional activity; no assigned outcome or delivery duty.".into(),
        policy_id: request.policy_id.clone(),
        outcomes: BTreeMap::new(),
        review_roles: BTreeMap::new(),
        delivery_to: BTreeSet::new(),
        stakeholders: BTreeSet::new(),
        decision_rule: w::DecisionRule::PersonaArranged {
            eligible: request.participants.iter().cloned().map(w::PersonaId).collect(),
            delegation,
        },
        invitations: request.participants.iter().enumerate().map(|(index, id)| Invitation {
            id: format!("{}-optional-{index}", activity.episode),
            to: id.clone(),
            terms: w::CommitmentTerms {
                mandate_revision: 1,
                exact_terms: "Invitation to consider optional activity; engagement and conversation are voluntary. No artifact, review or delivery responsibility is assigned.".into(),
                outcomes: BTreeSet::new(),
                review_roles: BTreeSet::new(),
            },
        }).collect(),
    })
}

fn blocked_start(
    app: &mut Runtime,
    head: &s::StoredObject,
    state: &mut State,
    opportunity: &str,
    request: &OptionalActivityStart,
    now: i64,
    reason: &RuntimeError,
) -> Result<Value> {
    let record = json!({"trigger":request.id,"at_unix":now,"status":"blocked",
        "reason":bounded_detail(&reason.to_string(),512),"episode_admitted":false});
    let starts = &mut state
        .optional_activities
        .get_mut(opportunity)
        .unwrap()
        .blocked_starts;
    require(
        starts.contains_key(&request.id) || starts.len() < 64,
        "optional blocked-start retention bound",
    )?;
    starts.insert(request.id.clone(), record.clone());
    a::bounds(state)?;
    app.save(head, state)?;
    Ok(record)
}

fn admitted_replay(episode: &a::Episode) -> Value {
    json!({"status":"already_admitted","work":episode.work,"trigger":episode.trigger,
        "replayed":true,"inference_performed":false,"resume_authorized":false})
}

pub fn start_optional_activity(
    app: &mut Runtime,
    request: &OptionalActivityStart,
    now: i64,
) -> Result<Value> {
    identity(&request.id)?;
    identity(&request.operator)?;
    require(
        request.id.len() <= 96 && now >= 0,
        "optional trigger identity or clock invalid",
    )?;
    let _owner = app.store.acquire_execution_owner()?;
    let (head, mut state, launch_head, mut saved) = authorized(app, &request.operator)?;
    let opportunity_id = state
        .optional_activities
        .iter()
        .find(|(_, o)| o.reference == request.opportunity)
        .map(|(id, _)| id.clone())
        .ok_or_else(|| held("optional opportunity unavailable"))?;
    let opportunity = state.optional_activities[&opportunity_id].clone();
    require(
        inspectable(&state, &opportunity, &request.operator, now),
        "optional opportunity unavailable",
    )?;
    if let Some(episode) = opportunity.episodes.get(&request.id) {
        return Ok(admitted_replay(episode));
    }
    let grant = &opportunity.request;
    let reason = (|| -> Result<()> {
        require(
            opportunity.revoked_at_unix.is_none()
                && now >= grant.not_before_unix
                && now < grant.expires_at_unix,
            "optional opportunity is not currently enabled",
        )?;
        require(
            active_invocations(&state).is_empty(),
            "active invocation requires completion or reconciliation",
        )?;
        app.store.require_terminal_invocations()?;
        require(
            opportunity.episodes.len() < grant.max_episodes as usize,
            "optional recurrence allowance exhausted",
        )?;
        require(
            opportunity.episodes.values().all(|episode| {
                episode.ended.is_some()
                    || state
                        .board
                        .work(&w::WorkId(episode.work.clone()))
                        .is_ok_and(|work| matches!(work.phase, w::Phase::Ended { .. }))
            }),
            "the previous optional episode has not ended",
        )?;
        if let Some(previous) = opportunity
            .episodes
            .values()
            .map(|e| e.started_at_unix)
            .max()
        {
            require(
                now.checked_sub(previous)
                    .and_then(|v| u64::try_from(v).ok())
                    .is_some_and(|elapsed| elapsed >= grant.minimum_interval_seconds),
                "optional recurrence is not yet due",
            )?;
        }
        need_proposals::subtract(opportunity.remaining, grant.per_episode)?;
        require(
            !a::foreground_eligible(&state, now)?,
            "eligible foreground work has priority",
        )?;
        require(
            saved.steps < saved.setup.max_total_steps
                && saved.works.len() < saved.setup.max_works as usize
                && grant.per_episode.attempts <= saved.setup.max_total_steps - saved.steps,
            "optional episode exceeds remaining launch capacity",
        )?;
        let provider = saved.execution_provider()?;
        require(
            provider.model == grant.model
                && provider.effort == grant.effort
                && saved.setup.destination == grant.destination,
            "optional inference binding changed",
        )?;
        for id in &grant.participants {
            lifecycle::require_active(&state, id)?;
            let persona = state
                .personas
                .get(id)
                .ok_or_else(|| held("optional participant unavailable"))?;
            require(
                persona.failed_attempt_hold.is_none()
                    && persona.active_invocation.is_none()
                    && persona.seed.requested_model == grant.model
                    && persona.seed.requested_effort == grant.effort
                    && persona.seed.destination == grant.destination
                    && exposure_permitted(&state, &opportunity.exposure, &admission(persona, now)),
                "optional participant is held, busy or lacks current permission",
            )?;
        }
        Ok(())
    })()
    .err();
    if let Some(reason) = reason {
        // A blocked trigger retains no work, invitation, consumed recurrence or
        // episode allocation. Explicit reconsideration may reuse the same key.
        return blocked_start(
            app,
            &head,
            &mut state,
            &opportunity_id,
            request,
            now,
            &reason,
        );
    }
    let admission = (|| -> Result<Value> {
        let work_id = format!("work-{:08}", saved.works.len() + 1);
        let expires_at_unix = now
            .checked_add(
                i64::try_from(grant.episode_seconds)
                    .map_err(|_| invalid("optional duration overflow"))?,
            )
            .ok_or_else(|| invalid("optional deadline overflow"))?
            .min(grant.expires_at_unix);
        let activity = w::OptionalActivity {
            opportunity: opportunity.reference.clone(),
            episode: request.id.clone(),
            expires_at_unix: expires_at_unix as u64,
        };
        let input = input(&saved, grant, &activity, &work_id)?;
        let mut exposure = opportunity.exposure.clone();
        exposure.policies.push(scoped_policy(
            "work-need",
            &work_id,
            &digest(&input)?,
            current_policy(&state, &input.policy_id)?,
        ));
        let mut next = intake::construct_host_need(
            &state,
            &request.operator,
            &input,
            grant.per_episode,
            w::Origin::Human(request.operator.clone()),
            exposure,
            Some(json!({"kind":"operator_enabled_optional_activity",
            "opportunity":activity.opportunity,"episode":activity.episode,
            "notice_authorship":"runtime availability notice, not a quoted human task or persona purpose",
            "authorization_reference":grant.authorization_reference,"operator":request.operator})),
            Some(activity),
        )?;
        next.work_meta
            .get_mut(&work_id)
            .unwrap()
            .repair_attempts_declared = Some(grant.per_episode.repair_attempts);
        let current = next.optional_activities.get_mut(&opportunity_id).unwrap();
        current.remaining = need_proposals::subtract(current.remaining, grant.per_episode)?;
        current.episodes.insert(
            request.id.clone(),
            a::Episode {
                trigger: request.id.clone(),
                work: work_id.clone(),
                started_at_unix: now,
                expires_at_unix,
                allocation: grant.per_episode,
                admissions: Vec::new(),
                ended: None,
            },
        );
        a::bounds(&next)?;
        saved.works.insert(work_id.clone(), None);
        let mut launch = launch_head.clone();
        launch.revision = increment(launch.revision)?;
        launch.body["works"] = serde_json::to_value(&saved.works)?;
        app.store.commit_intake_delegation_batch(
            vec![
                s::Change {
                    expected_revision: Some(head.revision),
                    object: state_object(&head, &next)?,
                    depends_on: Vec::new(),
                },
                s::Change {
                    expected_revision: Some(launch_head.revision),
                    object: launch,
                    depends_on: Vec::new(),
                },
            ],
            s::Resources {
                attempts: grant.per_episode.attempts,
                input_bytes: 1,
                output_bytes: 1,
                navigation_bytes: 1,
                retained_bytes: 1,
            },
        )?;
        Ok(
            json!({"status":"episode_admitted","work":work_id,"trigger":request.id,"opportunity":opportunity.reference,
        "expires_at_unix":expires_at_unix,"allocation":grant.per_episode,"logical_allocation_reserved":true,
        "store_invocation_reserved":false,"inference_performed":false,"replayed":false}),
        )
    })();
    match admission {
        Ok(value) => Ok(value),
        Err(reason) => {
            // A construction or atomic persistence failure is still a blocked
            // start. First inspect the durable state, never refund or describe
            // a committed episode as unadmitted after an ambiguous return.
            let (current_head, mut current) = app.load()?;
            if let Some(episode) = current
                .optional_activities
                .get(&opportunity_id)
                .and_then(|opportunity| opportunity.episodes.get(&request.id))
            {
                return Ok(admitted_replay(episode));
            }
            require(
                current_head.exact_ref()? == head.exact_ref()?,
                "optional start state changed; inspect before reconsidering",
            )?;
            blocked_start(
                app,
                &current_head,
                &mut current,
                &opportunity_id,
                request,
                now,
                &reason,
            )
        }
    }
}

pub fn revoke_optional_activity(
    app: &mut Runtime,
    request: &OptionalActivityRevocation,
    now: i64,
) -> Result<Value> {
    identity(&request.operator)?;
    text(&request.reason, 4096)?;
    require(
        !request.reason.trim().is_empty() && now >= 0,
        "optional revocation reason or clock invalid",
    )?;
    let _owner = app.store.acquire_execution_owner()?;
    let (head, mut state, _, _) = authorized(app, &request.operator)?;
    let id = state
        .optional_activities
        .iter()
        .find(|(_, o)| o.reference == request.opportunity)
        .map(|(id, _)| id.clone())
        .ok_or_else(|| held("optional opportunity unavailable"))?;
    require(
        inspectable(
            &state,
            &state.optional_activities[&id],
            &request.operator,
            now,
        ),
        "optional opportunity unavailable",
    )?;
    if let Some(at) = state.optional_activities[&id].revoked_at_unix {
        return Ok(json!({"reference":request.opportunity,"revoked_at_unix":at,"replayed":true}));
    }
    state
        .optional_activities
        .get_mut(&id)
        .unwrap()
        .revoked_at_unix = Some(now);
    state.operator_events.push(
        json!({"kind":"optional_activity_revoked","opportunity":request.opportunity,
        "operator":request.operator,"reason":request.reason,"at_unix":now}),
    );
    app.save(&head, &state)?;
    Ok(
        json!({"reference":request.opportunity,"revoked_at_unix":now,"replayed":false,
        "incurred_charges_preserved":true,"new_discretionary_activity_authorized":false}),
    )
}
