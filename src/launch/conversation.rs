//! Operator projection of retained communication and voluntary responsibility.
//! Opening this view never delivers a message, starts inference, or selects context.
use super::*;

pub fn conversation(root: &Path, operator: &str, work_id: &str) -> Result<Value> {
    identity(operator)?;
    identity(work_id)?;
    inspection::project(root, |snapshot, records| {
        project(snapshot, records, operator, work_id)
    })
}

pub(super) fn project(
    snapshot: &inspection::Snapshot,
    records: &s::RuntimeInspection<'_, '_>,
    operator: &str,
    work_id: &str,
) -> Result<Value> {
    project_messages(snapshot, records, operator, work_id, false)
}

pub(super) fn project_display(
    snapshot: &inspection::Snapshot,
    records: &s::RuntimeInspection<'_, '_>,
    operator: &str,
    work_id: &str,
) -> Result<Value> {
    project_messages(snapshot, records, operator, work_id, true)
}

/// Displayed text, attribution, and exact citations. The complete retained
/// message (including its potentially large provenance graph) remains available
/// through conversation/history inspection; it is not rendered by the desktop.
pub(super) fn display_message(message: &Message) -> Value {
    json!({"id":message.id,"sender":message.sender,"recipient":message.recipient,
        "work":message.work,"thread_work":message.thread_work,"invocation":message.invocation,
        "subject":message.subject,"text":message.text,"file_references":message.file_references,
        "history_references":message.history_references,"local_deposit_only":message.local_deposit_only})
}

fn project_messages(
    snapshot: &inspection::Snapshot,
    records: &s::RuntimeInspection<'_, '_>,
    operator: &str,
    work_id: &str,
    display: bool,
) -> Result<Value> {
        let state = &snapshot.state;
        require(
            snapshot.saved.operator == operator && snapshot.saved.works.contains_key(work_id),
            "conversation inspection requires the saved requester and task",
        )?;
        let work = state.board.work(&w::WorkId(work_id.into()))?;
        // Accepted decision epochs order concurrent calls by actual adoption.
        // A subject is not a reply-to link; a deposit does not establish reading.
        let mut epochs = BTreeMap::new();
        let mut ordered = Vec::new();
        for message in state.messages.values().filter(|m| {
            m.work == work_id || m.thread_work.as_deref() == Some(work_id)
        }) {
            let epoch = match epochs.get(&message.invocation) {
                Some(epoch) => *epoch,
                None => {
                    let epoch = records.decision(&message.invocation)?.accepted_epoch;
                    epochs.insert(message.invocation.clone(), epoch);
                    epoch
                }
            };
            ordered.push((epoch, message));
        }
        ordered.sort_by(|(a_epoch, a), (b_epoch, b)| (a_epoch, &a.id).cmp(&(b_epoch, &b.id)));
        let persona_labels: BTreeMap<_, _> = ordered.iter()
            .flat_map(|(_, message)| [&message.sender, &message.recipient])
            .filter_map(|id| state.personas.get(id).map(|persona| (id.clone(), persona.seed.label.clone())))
            .collect();
        let messages: Vec<_> = ordered
            .into_iter()
            .map(|(epoch, message)| {
                let mut value = if display { display_message(message) }
                    else { serde_json::to_value(message).expect("serializable message") };
                value["accepted_epoch"] = epoch.into();
                value
            })
            .collect();
        let participants: Vec<_> = work
            .participants
            .keys()
            .filter_map(|id| {
                let persona = state.personas.get(&id.0)?;
                let acceptance = work.acceptance(id);
                Some(json!({"persona":id,"label":persona.seed.label,
                "responsibility_accepted":acceptance.is_some(),
                "acceptance":acceptance.map(|(id, value)| json!({"id":id,"record":value}))}))
            })
            .collect();
        Ok(
            json!({"work":work_id,"epoch":snapshot.epoch,"snapshot_atomic":true,
            "messages":messages,"persona_labels":persona_labels,"participants":participants,
            "arrangement_proposals":work.arrangement_proposals,
            "arrangements":work.arrangements,"current_arrangement":work.current_arrangement,
            "dependencies":work.dependencies,"predecessor":work.predecessor,
            "inference_performed":false,"order":"accepted_decision_epoch_then_message_id",
            "message_projection":if display { "desktop_text_and_citations" } else { "complete_record" },
            "meaning":"Recorded messages and accepted responsibilities. Message counts do not measure agreement, reading, expertise or completion."}),
        )
}
