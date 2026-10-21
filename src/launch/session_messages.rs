//! Fresh permitted messages from the exact call range of a retained session.
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
    pub truncated: bool,
    pub inference_performed: bool,
}

/// Read-only apart from existing navigation accounting. The original exclusive
/// session owner retained both counters; ordinary launch reservations use the
/// canonical call identity derived from each counter. Missing terminal bounds
/// cannot be reconstructed from later launch state or timestamps.
pub fn session_messages(root: &Path, operator: &str, key: &str) -> BoundaryResult<SessionMessages> {
    valid_id(key)?;
    let (mut app, snapshot) = open_view(root, operator)?;
    let result = project(&mut app, &snapshot.state, operator, key)?;
    snapshot.unchanged(&app)?;
    Ok(result)
}

fn project(app: &mut Runtime, state: &State, operator: &str, key: &str) -> BoundaryResult<SessionMessages> {
    let session = read_session(app, operator, key)?;
    let scope = match &session {
        SessionView::Finished { claim, terminal } => match &terminal.outcome {
            SessionEnd::Returned { steps_after, .. } => {
                // Every retained native foreground setup limits one session to
                // at most 128 reservations, including unsuccessful admissions.
                check(steps_after.checked_sub(claim.steps_before).is_some_and(|n| n <= 128),
                    BoundaryError::InvalidState)?;
                Some(SessionMessageScope { work: claim.request.work.clone(),
                    steps_before: claim.steps_before, steps_after: *steps_after })
            }
            SessionEnd::Failed { .. } => None,
        },
        SessionView::NotRecorded | SessionView::OutcomeUnrecorded { .. } => None,
    };
    let mut result = SessionMessages { session, message_scope: scope, messages: Vec::new(),
        truncated: false, inference_performed: false };
    let Some(scope) = &result.message_scope else { return Ok(result); };
    if scope.steps_before == scope.steps_after { return Ok(result); }
    let calls: BTreeMap<_, _> = (scope.steps_before + 1..=scope.steps_after)
        .map(|step| (format!("call-{step:08}"), step)).collect();
    let now = unix_now()?;
    let mut candidates: Vec<_> = state.messages.values().filter_map(|message| {
        let step = calls.get(&message.invocation)?;
        (message.work == scope.work && message.recipient == operator
            && results::message_permitted(&state, message, now)).then_some((*step, message))
    }).collect();
    // Keep the most recent whole records that fit, then display them in call
    // order. Earlier work history consumes none of this session's output bound.
    candidates.sort_by(|(step_a, a), (step_b, b)| (step_b, &b.id).cmp(&(step_a, &a.id)));
    let mut left = MAX_VIEW_BYTES.checked_sub(serde_json::to_vec(&result)?.len() + 1024)
        .ok_or(BoundaryError::Capacity)?;
    for (_, message) in candidates {
        if result.messages.len() == 128 {
            result.truncated = true;
            break;
        }
        let size = serde_json::to_vec(message)?.len() + 1;
        if size > left {
            result.truncated = true;
            continue;
        }
        left -= size;
        result.messages.push(message.clone());
    }
    result.messages.reverse();
    Ok(result)
}
