//! Requester inspection and attributed replies to existing shared questions.
use super::*;
#[path = "question_inbox.rs"]
mod inbox;

pub(super) fn deliver(app: &mut Runtime, owner: &s::ExecutionOwner, now: i64) -> Result<usize> {
    inbox::deliver(app, owner, now)
}
pub(super) fn append_deliveries(records: &s::RuntimeInspection<'_, '_>, operator: &str, work: &str, value: &mut Value) -> Result<()> {
    inbox::append_view(records, operator, work, value)
}

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
    app.store.work_input_transaction(None, |runtime, launch, records| {
        let state = Runtime::decode_inspection_state(&runtime)?;
        let saved = decode_saved(&launch)?;
        let mut value = project(&state, &saved, operator, work, now)?;
        inbox::append_view(records, operator, work, &mut value)?;
        Ok::<_, RuntimeError>((value, Vec::new()))
    })?
}
pub fn shared_questions_at(root: &Path, operator: &str, work: &str, now: i64) -> Result<Value> {
    identity(operator)?;
    identity(work)?;
    inspection::project(root, |snapshot, records| {
        let mut value = project(&snapshot.state, &snapshot.saved, operator, work, now)?;
        inbox::append_view(records, operator, work, &mut value)?;
        Ok(value)
    })
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
    inbox::submit(app, operator, input, now)
}
