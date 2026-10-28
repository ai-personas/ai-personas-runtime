//! Current own-command metadata across work; command and result bodies stay selective.
use super::*;

pub(super) fn unresolved(state: &State, actor: &str, current_work: &str) -> Vec<(String, Value)> {
    state.host_command_jobs.iter().rev()
        .filter(|(_, job)| job.actor == actor
            && (job.observation.is_none() || job.result_source.is_none()
                || job.state == s::IntentState::Uncertain))
        .map(|(id, job)| {
            let awaiting_publication = job.observation.is_none() || job.result_source.is_none();
            (id.clone(), json!({
                "intent":id,"invocation":job.invocation,"operation":job.operation,
                "work":job.work,"current_work":job.work == current_work,
                "working_directory":job.host_working_directory,
                "state":job.state,
                "status":if awaiting_publication { "awaiting_result_publication" }
                    else { "outcome_uncertain" },
                "automatic_replay":false
            }))
        })
        .collect()
}
