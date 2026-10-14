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
    if !sender
        .seed
        .grants
        .communicate_with
        .contains(&message.recipient)
        || !sender.seed.audience.contains(&message.recipient)
    {
        return false;
    }
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
) -> Result<()> {
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
    Ok(())
}
pub(super) fn project(app: &mut Runtime, work_id: &str, saved: &Saved) -> Result<Output> {
    let messages = app.inbox(&saved.operator, work_id)?;
    let state = app.snapshot()?;
    let now = unix_now()?;
    let mut result = Output {
        work_id: work_id.into(),
        status: "intake-incomplete".into(),
        recorded_phase: None,
        focus_release: None,
        completion_supported: None,
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
    let owner = people(saved.setup.cohort)[saved.setup.owner - 1].clone();
    let mut a = admission(
        state
            .personas
            .get(&owner)
            .ok_or_else(|| held("owner absent"))?,
        now,
    );
    a.audience = BTreeSet::from([saved.operator.clone()]);
    if !exposure_permitted(&state, &work_exposure(&state, work_id)?, &a) {
        result.status = "withheld".into();
        return Ok(result);
    }
    if let Some(id) = saved.works.get(work_id).and_then(Option::as_deref) {
        result.last_attempt = Some(match app.invocation(id) {
            Ok(invocation) => json!({"id": id, "state": invocation.state}),
            Err(RuntimeError::Store(s::StoreError::Missing(_))) => {
                json!({"id": id, "state": "not_admitted"})
            }
            Err(error) => return Err(error),
        });
    }
    // Work phase is a permitted historical record. Re-evaluating its global
    // predicate could disclose inaccessible newer evidence, consent or findings.
    result.completion_supported = if matches!(work.phase, w::Phase::Completed { .. }) {
        None
    } else {
        Some(false)
    };
    result.recorded_phase = Some(
        match &work.phase {
            w::Phase::Completed { .. } => "completed",
            w::Phase::Paused { .. } => "paused",
            w::Phase::Ended { .. } => "ended",
            w::Phase::Open => "open",
        }
        .into(),
    );
    result.status = match &work.phase {
        w::Phase::Completed { .. } => "completed-as-recorded",
        w::Phase::Paused { .. } => "paused",
        w::Phase::Ended { .. } => "ended-unfinished",
        _ => "unfinished",
    }
    .into();
    let mut permitted_deliveries: Vec<_> = work
        .deliveries
        .iter()
        .rev()
        .filter_map(|receipt| {
            work.releases.get(&receipt.release)?;
            let body = state.local_deliveries.get(&receipt.id.0)?;
            delivery(&state, work_id, &receipt.id.0, body, &saved.operator, now)
                .is_ok()
                .then_some((receipt, body))
        })
        .collect();
    // Order only permitted delivered releases by immutable creation revision;
    // a later redelivery of an old release cannot move result focus backwards.
    // Multiple releases from the same transaction use exact release ID as a
    // deterministic tie-break, not a semantic freshness judgment.
    permitted_deliveries.sort_by_key(|(receipt, _)| {
        let version = work.releases[&receipt.release].by.version;
        (
            std::cmp::Reverse(version.revision),
            std::cmp::Reverse(version.epoch),
            std::cmp::Reverse(receipt.release.clone()),
        )
    });
    let mut left = 1024 * 1024 - 4096;
    // Choose status before any presentation truncation. This is the latest
    // permitted release creation, not an assertion of global release freshness.
    if let Some((receipt, _)) = permitted_deliveries.first() {
        result.status_delivery = Some(receipt.id.0.clone());
        result.focus_release = Some(receipt.release.0.clone());
        let evidence = closure_readiness::record(
            &state,
            work_id,
            &receipt.release.0,
            &a,
            &mut Exposure::default(),
        )
        .ok();
        if matches!(work.phase, w::Phase::Open) {
            let acceptance_unestablished = !work.mandate().stakeholders.is_empty()
                && evidence.as_ref().is_none_or(|record| {
                    record["unestablished_stakeholder_acceptance"]
                        .as_array()
                        .is_none_or(|items| !items.is_empty())
                });
            result.status = if acceptance_unestablished {
                "delivered-acceptance-unestablished"
            } else {
                "delivered-unfinished"
            }
            .into();
        }
        if let Some(evidence) = evidence {
            let size = serde_json::to_vec(&evidence)?.len();
            if size <= left {
                left -= size;
                result.closure_evidence.push(evidence);
            } else {
                result.truncated = true;
            }
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
            let size = serde_json::to_vec(&target)?.len();
            if size <= left {
                left -= size;
                result.human_decision_pointers.push(target);
            } else {
                result.truncated = true;
            }
        }
    }
    for (receipt, body) in permitted_deliveries {
        let size = serde_json::to_vec(body)?.len();
        if result.local_deliveries.len() == 128 || size > left {
            result.truncated = true;
            break;
        }
        left -= size;
        debug_assert_eq!(body["release"], receipt.release.0);
        result.local_deliveries.push(body.clone());
    }
    for message in messages {
        if !message_permitted(&state, &message, now) {
            continue;
        }
        let size = serde_json::to_vec(&message)?.len();
        if result.messages.len() == 128 || size > left {
            result.truncated = true;
            break;
        }
        left -= size;
        result.messages.push(message);
    }
    Ok(result)
}
