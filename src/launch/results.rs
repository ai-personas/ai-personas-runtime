use super::*;

/// Historical permitted delivery pointer, not a complete authorized request.
/// Current global authority/version is available only through explicit operator inspection.
/// Contains no decision ID, consent value, human statement, or mutable global version.
#[derive(Debug, Serialize)]
pub struct HumanDecisionPointer {
    pub work: String,
    pub release: String,
    pub release_sha256: String,
    pub delivery: String,
    pub delivery_receipt: w::ExactRef,
}

#[derive(Debug, Serialize)]
pub struct Output {
    pub work_id: String,
    pub status: String,
    pub recorded_phase: Option<String>,
    pub focus_release: Option<String>,
    pub completion_supported: Option<bool>,
    /// Latest permitted historical completion; no current support assertion.
    pub recorded_completion: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_activity_ending: Option<Value>,
    /// Current native participation, including decisions made without a reply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_activity: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub persona_creations: Vec<Value>,
    /// Permission-filtered input/disposition history, never proof of compliance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requester_feedback: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_task_amendment: Option<Value>,
    /// Exact permitted receipt for the focus release; not global freshness.
    pub status_delivery: Option<String>,
    pub last_attempt: Option<Value>,
    pub messages: Vec<Message>,
    pub local_deliveries: Vec<Value>,
    pub human_decision_pointers: Vec<HumanDecisionPointer>,
    /// Partial currently permitted facts; never global closure readiness.
    pub closure_evidence: Vec<Value>,
    pub truncated: bool,
    pub stop: Option<String>,
}
pub(super) fn message_permitted(state: &State, message: &Message, now: i64) -> bool {
    let Some(sender) = state.personas.get(&message.sender) else {
        return false;
    };
    let Some(inherited) = message.exposure.as_ref() else {
        return false;
    };
    let mut a = admission(sender, now);
    a.audience = BTreeSet::from([message.recipient.clone()]);
    if let Some(recipient) = state.personas.get(&message.recipient) {
        a.principal = recipient.seed.id.clone();
        a.destination = recipient.seed.destination.clone();
    } else if !state.config.local_recipients.contains(&message.recipient) {
        return false;
    }
    let Ok(mut exposure) = work_exposure(state, &message.work) else {
        return false;
    };
    extend_exposure(&mut exposure, inherited);
    exposure.sources.extend(message.lineage.iter().cloned());
    exposure_permitted(state, &exposure, &a)
}
fn delivery(
    state: &State,
    work_id: &str,
    id: &str,
    body: &Value,
    recipient: &str,
    now: i64,
) -> Result<f::Admission> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    let receipt = work
        .deliveries
        .iter()
        .find(|d| d.id.0 == id)
        .ok_or_else(|| held("delivery receipt absent"))?;
    require(
        body["work"] == work_id
            && body["recipient"] == recipient
            && body["release"] == receipt.release.0
            && receipt.recipient == recipient
            && body["kind"] == "atomic_local_inbox_deposit"
            && body["external_export"] == false
            && receipt.receipt == w::ExactRef::new(id, 1, &serde_json::to_vec(body)?),
        "delivery binding unavailable",
    )?;
    let actor = body["actor"]
        .as_str()
        .ok_or_else(|| held("delivery actor absent"))?;
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("delivery persona absent"))?;
    let mut a = admission(persona, now);
    a.audience = BTreeSet::from([recipient.into()]);
    require(
        state.config.local_recipients.contains(recipient),
        "recipient unavailable",
    )?;
    delivery_evidence::validate(state, work_id, receipt, &a)?;
    Ok(a)
}
pub(super) fn project(app: &mut Runtime, work_id: &str, saved: &Saved) -> Result<Output> {
    let state = app.snapshot()?;
    project_state(&state, work_id, saved, unix_now()?)
}
pub(super) fn project_state(state: &State, work_id: &str, saved: &Saved, now: i64) -> Result<Output> {
    require(saved.works.contains_key(work_id), "unknown launch work")?;
    let mut result = Output {
        work_id: work_id.into(),
        status: "intake-incomplete".into(),
        recorded_phase: None,
        focus_release: None,
        completion_supported: None,
        recorded_completion: None,
        optional_activity_ending: None,
        optional_activity: None,
        persona_creations: Vec::new(),
        requester_feedback: None,
        recorded_task_amendment: None,
        status_delivery: None,
        last_attempt: None,
        messages: Vec::new(),
        local_deliveries: Vec::new(),
        human_decision_pointers: Vec::new(),
        closure_evidence: Vec::new(),
        truncated: false,
        stop: None,
    };
    let Ok(work) = state.board.work(&w::WorkId(work_id.into())) else {
        return Ok(result);
    };
    // The result interface has historical per-record disclosure authority, not
    // an independently retained basis for current global phase/attempt status.
    // Explicit trusted operator status remains a separate interface.
    result.status = "current-state-unestablished".into();
    result.optional_activity_ending = optional_activity::ending_for_recipient(&state,work_id,&saved.operator,now)?;
    result.optional_activity = optional_activity::participation_for_operator(state, work_id, &saved.operator, now)?;
    for creation in state.persona_creations.values().filter(|value| value.work == work_id) {
        let parent = state.personas.get(&creation.parent)
            .ok_or_else(|| held("creation parent absent"))?;
        require(parent.created_by == saved.operator, "creation operator mismatch")?;
        let mut a = admission(parent, now);
        a.audience = BTreeSet::from([saved.operator.clone()]);
        if !exposure_permitted(state, &creation.exposure, &a) {
            continue;
        }
        let child = creation.created.as_ref().map(|created| -> Result<Value> {
            let persona = state.personas.get(&created.persona)
                .ok_or_else(|| held("created persona absent"))?;
            require(persona.birth.as_ref().is_some_and(|origin| origin.receipt == created.receipt),
                "created identity receipt differs")?;
            let mut opportunities = Vec::new();
            for (work, focus) in &persona.focus {
                let board = state.board.work(&w::WorkId(work.clone()))?;
                if !board.participants.contains_key(&w::PersonaId(created.persona.clone())) {
                    continue;
                }
                let continuation = match focus.continuation {
                    None => "no authored choice yet",
                    Some(p::Continuation::Immediate { .. }) => "continuing",
                    Some(p::Continuation::Stop { .. }) => "stopped",
                    Some(p::Continuation::Pause { .. }) => "paused",
                    Some(p::Continuation::Wait { .. }) => "waiting",
                    Some(p::Continuation::ReconsiderAt { .. }) => "scheduled",
                };
                opportunities.push(json!({"work":work,
                    "responsibility_accepted":board.acceptance(&w::PersonaId(created.persona.clone())).is_some(),
                    "continuation":continuation,"phase":board.phase}));
            }
            Ok(json!({"persona":created.persona,"label":persona.seed.label,
                "receipt":created.receipt,"character_accepted":persona.character.is_some(),
                "work_opportunities":opportunities}))
        }).transpose()?;
        result.persona_creations.push(json!({"id":creation.id,"parent":creation.parent,
            "parent_label":parent.seed.label,"invocation":creation.invocation,
            "operation_id":creation.operation.id(),"proposal":creation.proposal,
            "accepted_at_unix":creation.accepted_at_unix,
            "status":if child.is_some() {"created"} else {"pending"},"child":child,
            "creation_implies_work_acceptance":false}));
    }
    if result.optional_activity_ending.is_some() {
        result.status = "optional-activity-ended-as-recorded".into();
    } else if result.optional_activity.is_some() {
        result.status = "optional-activity".into();
    }
    if let Some(amendment) = state.work_meta.get(work_id)
        .and_then(|meta| task_amendment::recorded_for_operator(meta, &saved.operator))
    {
        result.recorded_task_amendment = Some(amendment);
    }
    let completion = closure_readiness::recorded_completion_for_recipient(
        &state,
        work_id,
        &saved.operator,
        now,
    )?;
    let recorded_release = completion["release"].as_str().map(str::to_owned);
    if !completion.is_null() {
        result.status = "completed-as-recorded".into();
        result.recorded_completion = Some(completion);
    }
    let mut permitted_deliveries: Vec<_> = work
        .deliveries
        .iter()
        .rev()
        .filter_map(|receipt| {
            work.releases.get(&receipt.release)?;
            let body = state.local_deliveries.get(&receipt.id.0)?;
            delivery(&state, work_id, &receipt.id.0, body, &saved.operator, now)
                .ok()
                .map(|a| (receipt, body, a))
        })
        .collect();
    // Order only permitted delivered releases by immutable creation revision;
    // a later redelivery of an old release cannot move result focus backwards.
    // Multiple releases from the same transaction use exact release ID as a
    // deterministic tie-break, not a semantic freshness judgment.
    permitted_deliveries.sort_by_key(|(receipt, _, _)| {
        let version = work.releases[&receipt.release].by.version;
        (
            std::cmp::Reverse(version.revision),
            std::cmp::Reverse(version.epoch),
            std::cmp::Reverse(receipt.release.clone()),
        )
    });
    let feedback = requester_feedback::project_for_recipient(
        &state, work_id, &saved.operator, now, 0,
    )?;
    result.requester_feedback = Some(feedback);
    // Choose status before any presentation truncation. This is the latest
    // permitted release creation, not an assertion of global release freshness.
    if let Some((receipt, _, a)) = permitted_deliveries.first() {
        result.status_delivery = Some(receipt.id.0.clone());
        result.focus_release = Some(receipt.release.0.clone());
        let evidence = closure_readiness::record(
            &state,
            work_id,
            &receipt.release.0,
            a,
            &mut Exposure::default(),
        )
        .ok();
        let acceptance_unestablished = evidence.as_ref().is_some_and(|record| {
            record["unestablished_stakeholder_acceptance"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        });
        let recorded_for_focus = recorded_release.as_deref() == Some(receipt.release.0.as_str());
        result.status = if recorded_for_focus {
            "completed-as-recorded"
        } else if acceptance_unestablished {
            "delivered-acceptance-unestablished"
        } else {
            "delivered-current-state-unestablished"
        }
        .into();
        if let Some(evidence) = evidence {
            result.closure_evidence.push(evidence);
        }
        // The target names this exact permitted delivery, never current
        // sufficiency. Explicit human ingress rechecks all current gates.
        if work.mandate().stakeholders.contains(&saved.operator) {
            let target = HumanDecisionPointer {
                work: work_id.into(),
                release: receipt.release.0.clone(),
                release_sha256: digest(&serde_json::to_value(&work.releases[&receipt.release])?)?,
                delivery: receipt.id.0.clone(),
                delivery_receipt: receipt.receipt.clone(),
            };
            result.human_decision_pointers.push(target);
        }
    }
    for (receipt, body, _) in permitted_deliveries {
        debug_assert_eq!(body["release"], receipt.release.0);
        result.local_deliveries.push(body.clone());
    }
    for message in state.messages.values()
        .filter(|m| m.recipient == saved.operator && m.work == work_id)
    {
        if !message_permitted(&state, &message, now) {
            continue;
        }
        result.messages.push(message.clone());
    }
    Ok(result)
}
