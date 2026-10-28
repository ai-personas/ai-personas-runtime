//! Operator projection of retained communication and voluntary responsibility.
//! Opening this view never delivers a message, starts inference, or selects context.
use super::*;

pub fn conversation(root: &Path, operator: &str, work_id: &str) -> Result<Value> {
    identity(operator)?;
    identity(work_id)?;
    inspection::project(root, |snapshot, records| {
        let state = &snapshot.state;
        require(
            snapshot.saved.operator == operator && snapshot.saved.works.contains_key(work_id),
            "conversation inspection requires the saved requester and task",
        )?;
        let meta = state
            .work_meta
            .get(work_id)
            .ok_or_else(|| held("task unavailable"))?;
        require(meta.intake_operator == operator, "task requester differs")?;
        let work = state.board.work(&w::WorkId(work_id.into()))?;
        let now = unix_now()?;
        // Accepted decision epochs order concurrent calls by actual adoption.
        // A subject is not a reply-to link; a deposit does not establish reading.
        let mut epochs = BTreeMap::new();
        let mut ordered = Vec::new();
        for message in state.messages.values().filter(|m| {
            (m.work == work_id || m.thread_work.as_deref() == Some(work_id))
                && results::message_permitted(state, m, now)
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
        let messages: Vec<_> = ordered
            .into_iter()
            .map(|(epoch, message)| {
                let mut value = serde_json::to_value(message).expect("serializable message");
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
            "messages":messages,"participants":participants,
            "arrangement_proposals":work.arrangement_proposals,
            "arrangements":work.arrangements,"current_arrangement":work.current_arrangement,
            "dependencies":work.dependencies,"predecessor":work.predecessor,
            "inference_performed":false,"order":"accepted_decision_epoch_then_message_id",
            "meaning":"Recorded messages and accepted responsibilities. Message counts do not measure agreement, reading, expertise or completion."}),
        )
    })
}
