//! Attributed requester feedback within an existing work, without a new need.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackInput {
    pub id: String,
    pub work: String,
    pub persona: String,
    pub text: String,
}

impl FeedbackInput {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.id, &self.work, &self.persona] {
            identity(value)?;
        }
        require(self.id.len() <= 96, "feedback id exceeds 96 bytes")?;
        require(
            !self.text.trim().is_empty() && self.text.len() <= 16_384,
            "feedback text must be nonblank and at most 16384 bytes",
        )
    }
}

pub fn feedback(
    app: &mut Runtime,
    operator: &str,
    input: &FeedbackInput,
    now: i64,
) -> Result<Value> {
    input.validate()?;
    identity(operator)?;
    require(now >= 0, "negative feedback time")?;
    let _owner = app.store.acquire_execution_owner()?;
    let (old, mut state) = app.load()?;
    let (_, saved) = load(app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    require(
        saved.works.contains_key(&input.work) && !need_proposals::bound(&state, &input.work),
        "feedback requires an existing standalone saved work",
    )?;
    let meta = state
        .work_meta
        .get(&input.work)
        .ok_or_else(|| held("work unavailable"))?;
    require(
        meta.intake_operator == operator,
        "feedback requires the work's requester",
    )?;
    let persona = state
        .personas
        .get(&input.persona)
        .ok_or_else(|| held("persona unavailable"))?;
    require(
        persona.created_by == operator
            && persona.focus.contains_key(&input.work)
            && persona.seed.audience.contains(operator)
            && persona.seed.grants.communicate_with.contains(operator),
        "feedback recipient is outside this requester's work",
    )?;
    let exposure = work_exposure(&state, &input.work)?;
    require(
        exposure_permitted(&state, &exposure, &admission(persona, now)),
        "feedback work is not currently permitted for this persona",
    )?;
    let id = bounded_composite_id("operator-feedback", &[operator, &input.id]);
    let payload = json!({
        "id":input.id,"sender":operator,"recipient":input.persona,
        "work":input.work,"text":input.text,
        "origin":"local_operator_input","independently_authenticated":false,
        "changes_mandate_or_authority":false
    });
    let replayed = if let Some(existing) = state.observations.get(&id) {
        require(
            existing.kind == "received_local_operator_feedback"
                && existing.actor == input.persona
                && existing.work == input.work
                && existing.invocation.is_none()
                && existing.payload == payload,
            "feedback identity conflict",
        )?;
        true
    } else {
        require(
            active_invocations(&state).is_empty(),
            "active invocation blocks feedback",
        )?;
        app.store.require_terminal_invocations()?;
        state.observations.insert(
            id.clone(),
            Observation {
                id: id.clone(),
                actor: input.persona.clone(),
                work: input.work.clone(),
                invocation: None,
                kind: "received_local_operator_feedback".into(),
                at_unix: now,
                payload,
                sources: Vec::new(),
                exposure: Some(exposure),
                qualified_roots: Vec::new(),
            },
        );
        state
            .personas
            .get_mut(&input.persona)
            .unwrap()
            .focus
            .get_mut(&input.work)
            .unwrap()
            .pending_observations
            .push(id.clone());
        state.operator_events.push(json!({
            "kind":"work_feedback","operator":operator,"observation":id,
            "work":input.work,"recipient":input.persona,"at_unix":now
        }));
        app.save(&old, &state)?;
        false
    };
    Ok(json!({
        "observation":id,"work":input.work,"persona":input.persona,"replayed":replayed,
        "supply":"pending fresh permitted admission","persona_resumed":false,
        "inference_performed":false,"budget_reset":false
    }))
}
