//! Identity controls are separate from work state, character and model choice.
//! Legacy absence preserves the existing active state, not a new control grant.
use super::*;

pub(crate) const SLOT_PREFIX: &str = "persona-control:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Active,
    Paused,
    Retired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Notice {
    pub action: String,
    pub operator: String,
    pub reason: String,
    pub runtime_revision: u64,
    pub lifecycle_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub revision: u64,
    pub status: Status,
    pub pause: Option<Notice>,
    pub quarantine: Option<Notice>,
    pub retirement: Option<Notice>,
    pub last: Notice,
}

pub(crate) fn slot_key(persona: &str) -> String {
    format!("{SLOT_PREFIX}{}", s::sha256_bytes(persona.as_bytes()))
}

pub(crate) fn reason(state: &State, actor: &str) -> Option<&'static str> {
    let persona = state.personas.get(actor)?;
    let record = persona.lifecycle.as_ref()?;
    if record.status == Status::Retired {
        Some("persona is retired")
    } else if record.quarantine.is_some() {
        Some("persona is quarantined")
    } else if record.status == Status::Paused {
        Some("persona is paused")
    } else {
        None
    }
}

pub(crate) fn require_active(state: &State, actor: &str) -> Result<()> {
    require(state.personas.contains_key(actor), "persona unavailable")?;
    if let Some(reason) = reason(state, actor) {
        return Err(held(reason));
    }
    Ok(())
}

pub(crate) fn notice(
    action: &str,
    operator: &str,
    reason: &str,
    runtime_revision: u64,
    revision: u64,
) -> Notice {
    Notice {
        action: action.into(),
        operator: operator.into(),
        reason: reason.into(),
        runtime_revision,
        lifecycle_revision: revision,
    }
}

pub(crate) fn clear_routes_object(persona: &mut Value) -> Result<()> {
    for focus in persona["focus"]
        .as_object_mut()
        .ok_or_else(|| held("persona focus unavailable"))?
        .values_mut()
    {
        require(focus["permit"].is_boolean(), "focus permit unavailable")?;
        focus["permit"] = json!(false);
        focus["ordinary_continuation"] = json!(false);
        focus
            .as_object_mut()
            .ok_or_else(|| held("focus unavailable"))?
            .remove("explicit_resume_permit");
    }
    Ok(())
}

pub(crate) fn projection(persona: &Persona) -> Value {
    json!({"status":persona.lifecycle.as_ref().map_or(Status::Active, |r| r.status),
        "quarantined":persona.lifecycle.as_ref().is_some_and(|r| r.quarantine.is_some()),
        "controls_provisioned":persona.lifecycle.is_some(),
        "revision":persona.lifecycle.as_ref().map(|r| r.revision),
        "identity_continues":true,"inference_or_work_resume_granted":false})
}

fn retained(object: &s::StoredObject, actor: &str) -> Result<Record> {
    serde_json::from_value(object.body["personas"][actor]["lifecycle"].clone()).map_err(|_| {
        held("persona lifecycle controls must first be provisioned while ordinary capacity remains")
    })
}

pub(crate) fn control_id(object: &s::StoredObject, control: &s::RuntimeControl) -> Result<String> {
    let record = retained(object, &control.work)?;
    Ok(format!(
        "control:{}",
        digest(&("persona-control-v1", control, record.revision))?
    ))
}

pub(crate) fn control_is_current(
    current: &s::StoredObject,
    recorded: &s::StoredObject,
    control: &s::RuntimeControl,
) -> Result<bool> {
    let now = retained(current, &control.work)?;
    let then = retained(recorded, &control.work)?;
    Ok(match control.action {
        s::RuntimeControlAction::PersonaPause => {
            now.status == Status::Paused && now.pause.is_some() && now.pause == then.pause
        }
        s::RuntimeControlAction::PersonaQuarantine => {
            now.quarantine.is_some() && now.quarantine == then.quarantine
        }
        _ => false,
    })
}

/// Capacity is explicit and opt-in for retained identities. Existing stores do
/// not acquire unfunded closeout requirements merely by opening a newer binary.
pub(crate) fn requirements(object: &s::StoredObject) -> Result<Vec<s::RuntimeControlRequirement>> {
    let personas = object.body["personas"]
        .as_object()
        .ok_or_else(|| held("persona state unavailable"))?;
    let works = object.body["board"]["works"]
        .as_object()
        .ok_or_else(|| held("work board unavailable"))?;
    let mut requirements = Vec::new();
    for (id, persona) in personas {
        let Some(value) = persona.get("lifecycle") else {
            continue;
        };
        let record: Record = serde_json::from_value(value.clone())?;
        let key = slot_key(id);
        require(
            !works.contains_key(&key),
            "persona control namespace conflicts with a retained work identity",
        )?;
        let focuses = persona["focus"]
            .as_object()
            .ok_or_else(|| held("persona focus unavailable"))?
            .len();
        require(
            record.revision > 0,
            "persona lifecycle revision absent",
        )?;
        let pause = record.status == Status::Active;
        let quarantine = record.quarantine.is_none();
        // Three possible copies of an escaped reason, bounded identifiers,
        // scalar counters and every route flag. Double that conservative bound
        // for each still-available independent control.
        let focus_bytes = u64::try_from(focuses)
            .ok()
            .and_then(|count| count.checked_mul(16))
            .ok_or_else(|| held("persona focus accounting overflow"))?;
        let growth = (6_u64 * (6 * 4096 + 4096) + 4096)
            .checked_add(focus_bytes)
            .and_then(|bytes| bytes.checked_mul(u64::from(pause) + u64::from(quarantine)))
            .ok_or_else(|| held("persona control accounting overflow"))?;
        requirements.push(s::RuntimeControlRequirement {
            work: key,
            pause,
            ending: quarantine,
            growth_bytes: growth,
        });
    }
    Ok(requirements)
}

/// Deterministic narrowing only. No accepted text, obligation, grant, model,
/// invocation, result, usage hold or unrelated persona is rewritten.
pub(crate) fn control_object(
    old: &s::StoredObject,
    control: &s::RuntimeControl,
) -> Result<s::StoredObject> {
    identity(&control.operator)?;
    identity(&control.work)?;
    text(&control.reason, 4096)?;
    require(
        !control.reason.trim().is_empty(),
        "persona control reason required",
    )?;
    let actor = &control.work;
    let mut record = retained(old, actor)?;
    require(
        old.body["personas"][actor]["created_by"] == control.operator,
        "persona control requires its creating operator",
    )?;
    let next_revision = increment(old.revision)?;
    record.revision = increment(record.revision)?;
    let action = match control.action {
        s::RuntimeControlAction::PersonaPause => {
            require(
                record.status == Status::Active,
                "only an active identity can be paused",
            )?;
            record.status = Status::Paused;
            "pause"
        }
        s::RuntimeControlAction::PersonaQuarantine => {
            require(
                record.quarantine.is_none(),
                "persona is already quarantined",
            )?;
            "quarantine"
        }
        _ => return Err(held("not a persona narrowing control")),
    };
    let notice = notice(
        action,
        &control.operator,
        &control.reason,
        next_revision,
        record.revision,
    );
    if action == "pause" {
        record.pause = Some(notice.clone());
    } else {
        record.quarantine = Some(notice.clone());
    }
    record.last = notice.clone();
    let mut next = old.clone();
    next.revision = next_revision;
    let target = &mut next.body["personas"][actor];
    target["lifecycle"] = serde_json::to_value(record)?;
    target["owner_epoch"] = json!(increment(
        target["owner_epoch"]
            .as_u64()
            .ok_or_else(|| held("persona ownership unavailable"))?
    )?);
    clear_routes_object(target)?;
    next.body["operator_events"]
        .as_array_mut()
        .ok_or_else(|| held("operator history unavailable"))?
        .push(
            json!({"kind":"persona_lifecycle_control","persona":actor,"notice":notice,
            "obligations_resolved":false,"inference_performed":false,"budget_reset":false}),
        );
    Ok(next)
}

/// Retirement is unavailable until accepted work has been transferred,
/// explicitly canceled, or has a currently supported recorded completion. This
/// profile does not authorize retirement with an unresolved ownership block.
pub(crate) fn retirement_blockers(
    state: &State,
    actor: &str,
) -> Result<Vec<(String, &'static str)>> {
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("persona unavailable"))?;
    let mut blockers = Vec::new();
    for (wid, work) in state.board.works() {
        let canceled = matches!(
            work.phase,
            w::Phase::Ended {
                kind: w::EndingKind::Canceled,
                ..
            }
        );
        let optional_ended =
            work.need.optional_activity.is_some() && matches!(work.phase, w::Phase::Ended { .. });
        if work.acceptance(&w::PersonaId(actor.into())).is_some()
            && !canceled
            && !optional_ended
            && !state.board.completion_supported(wid)
        {
            blockers.push((wid.0.clone(), "accepted work requires a transfer, authorized cancellation or supported completion"));
        }
    }
    for (id, focus) in &persona.focus {
        if !focus.pending_requester_feedback.is_empty() || peer_feedback::has_pending(focus) {
            blockers.push((id.clone(), "consequential feedback is unresolved"));
        }
        if maintenance::has_unresolved(actor, id, focus)? {
            blockers.push((id.clone(), "retained maintenance is unresolved"));
        }
    }
    for job in state
        .computation_jobs
        .values()
        .chain(state.http_fetch_jobs.values())
        .chain(state.host_command_jobs.values())
    {
        if job.actor == actor
            && (!matches!(
                job.state,
                s::IntentState::Succeeded | s::IntentState::Failed | s::IntentState::Canceled
            ) || job.observation.is_none()
                || job.result_source.is_none())
        {
            blockers.push((
                job.work.clone(),
                "execution requires settlement or reconciliation",
            ));
        }
    }
    blockers.sort();
    blockers.dedup();
    Ok(blockers)
}
