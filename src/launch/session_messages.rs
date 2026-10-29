//! Requester session messages with their exact session attribution.
//! Session metadata supplies attribution, never disclosure or completion rights.
use super::*;

#[derive(Debug, Serialize)]
pub struct SessionMessageScope {
    pub work: String,
    pub steps_before: u64,
    pub steps_after: u64,
}

#[derive(Debug, Serialize)]
pub struct SessionMessages {
    pub session: SessionView,
    /// Absent when retained metadata cannot establish a complete call range.
    pub message_scope: Option<SessionMessageScope>,
    pub messages: Vec<Message>,
    pub message_sender_labels: BTreeMap<String, String>,
    pub truncated: bool,
    pub inference_performed: bool,
}

/// Read-only apart from existing navigation accounting. The original exclusive
/// session owner retained both counters; ordinary launch reservations use the
/// canonical call identity derived from each counter. Missing terminal bounds
/// cannot be reconstructed from later launch state or timestamps.
pub fn session_messages(root: &Path, operator: &str, key: &str) -> BoundaryResult<SessionMessages> {
    valid_id(key)?;
    project_view(root, operator, |snapshot, records| {
        let session = project_session(records, operator, key)?;
        project(&snapshot.state, operator, session, records)
    })
}

fn project(
    state: &State,
    operator: &str,
    session: SessionView,
    records: &s::RuntimeInspection<'_, '_>,
) -> BoundaryResult<SessionMessages> {
    let scope = match &session {
        SessionView::Finished { claim, terminal } => Some(SessionMessageScope {
            work: claim.request.work.clone(),
            steps_before: claim.steps_before,
            steps_after: terminal.outcome.progress().0,
        }),
        SessionView::NotRecorded | SessionView::OutcomeUnrecorded { .. } => None,
    };
    let mut result = SessionMessages { session, message_scope: scope, messages: Vec::new(),
        message_sender_labels: BTreeMap::new(),
        truncated: false, inference_performed: false };
    let Some(scope) = &result.message_scope else { return Ok(result); };
    if scope.steps_before == scope.steps_after { return Ok(result); }
    let calls: BTreeSet<_> = (scope.steps_before + 1..=scope.steps_after)
        .map(|step| format!("call-{step:08}")).collect();
    let now = unix_now()?;
    result.messages = state.messages.values().filter(|message| {
        calls.contains(&message.invocation)
            && message.work == scope.work && message.recipient == operator
            && results::message_permitted(state, message, now)
    }).cloned().collect();
    results::order_messages(&mut result.messages, records)?;
    result.message_sender_labels = results::message_sender_labels(state, &result.messages);
    Ok(result)
}
