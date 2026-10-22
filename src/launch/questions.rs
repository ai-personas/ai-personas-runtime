//! Requester inspection and attributed replies to existing shared questions.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionAnswerInput {
    pub work: String,
    pub reply: QuestionReplyInput,
}

impl QuestionAnswerInput {
    pub fn validate(&self) -> Result<()> {
        identity(&self.work)?;
        require(self.reply.question_work.as_deref().unwrap_or(&self.work) == self.work,
            "reply names a different question work")?;
        questions::validate_reply(&self.reply)
    }
}

pub fn shared_questions(app: &mut Runtime, operator: &str, work: &str, now: i64) -> Result<Value> {
    identity(operator)?;
    identity(work)?;
    let (_, saved) = load(app)?;
    let (_, state) = app.load()?;
    project(&state, &saved, operator, work, now)
}
pub fn shared_questions_at(root: &Path, operator: &str, work: &str, now: i64) -> Result<Value> {
    identity(operator)?;
    identity(work)?;
    inspection::project(root, |snapshot, _| project(&snapshot.state, &snapshot.saved, operator, work, now))
}
pub(super) fn project(state: &State, saved: &Saved, operator: &str, work: &str, now: i64) -> Result<Value> {
    admission_extension::authorize(state, saved, operator)?;
    Ok(
        json!({"work":work,"shared_questions":questions::operator_view(&state,operator,work,now)?,
        "inference_performed":false,"persona_resumed":false}),
    )
}

pub fn answer_question(
    app: &mut Runtime,
    operator: &str,
    input: &QuestionAnswerInput,
    now: i64,
) -> Result<Value> {
    input.validate()?;
    identity(operator)?;
    require(now >= 0, "negative question reply time")?;
    let _owner = app.store.acquire_execution_owner()?;
    let (_, saved) = load(app)?;
    let (old, mut state) = app.load()?;
    admission_extension::authorize(&state, &saved, operator)?;
    require(
        app.invocation_leases_quiescent(&state)?,
        "unfinished invocation blocks question reply",
    )?;
    app.store.require_quiescent_invocations()?;
    let events_before: usize = state.board.works().values().map(|work| work.events.len()).sum();
    let replayed = questions::operator_reply(&mut state, operator, &input.work, &input.reply, now)?;
    let waits_satisfied = state.board.works().values().map(|work| work.events.len()).sum::<usize>()
        .saturating_sub(events_before);
    if !replayed {
        state.operator_events.push(json!({"kind":"shared_question_reply","operator":operator,
            "work":input.work,"question":input.reply.question,"reply":input.reply.id,"at_unix":now}));
        app.save(&old, &state)?;
    }
    Ok(
        json!({"work":input.work,"question":input.reply.question,"reply":input.reply.id,
        "replayed":replayed,"inference_performed":false,"persona_resumed":false,
        "existing_waits_satisfied":waits_satisfied,
        "changes_mandate_or_consent":false,"budget_reset":false}),
    )
}
