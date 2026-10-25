//! Current-work peer accounts and accepted commands awaiting result publication.
//! This supplies neither private context nor proof that a process is running.
use super::*;

pub(super) fn last_judgment(
    state: &State,
    focus: &Focus,
    a: &f::Admission,
    current_mandate: u64,
    exposure: &mut Exposure,
) -> Value {
    match (&focus.last_judgment, &focus.judgment_exposure) {
        (Some(judgment), Some(inherited)) if exposure_permitted(state, inherited, a) => {
            extend_exposure(exposure, inherited);
            let mandate_relation = match &focus.judgment_origin {
                Some(origin) if origin.mandate_revision == current_mandate => "same_mandate",
                Some(_) => "different_mandate",
                None => "origin_unrecorded",
            };
            json!({"value":judgment,"origin":focus.judgment_origin,
                "current_mandate_revision":current_mandate,
                "mandate_relation":mandate_relation,
                "meaning":"Historical accepted persona judgment based on information supplied then. A different mandate does not establish completion of the current request; the same mandate does not resolve later feedback or results. This supplies the authored judgment, not its cited evidence, artifact bodies or verified execution. CONTROL.history_retrieval describes selective lookup of cited retained records, including other personas' records. Missing origin establishes neither invocation nor mandate. Decide afresh using the current request and observations."})
        }
        (None, _) => Value::Null,
        _ => json!({"status":"withheld_unavailable","body_supplied":false}),
    }
}

pub(super) fn context(
    state: &State,
    work_id: &str,
    actor: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Option<Value>> {
    let work = state.board.work(&w::WorkId(work_id.into()))?;
    if !work.participants.keys().any(|peer| peer.0 != actor) {
        return Ok(None);
    }
    // The caller already checked this work's policy and complete core lineage.
    // Never inspect other tasks, private selections, prompts or in-flight calls.
    let mut projection = json!({
        "profile":"current-work-collaborators-v4",
        "work":work_id,
        "records":[],
        "records_omitted":false,
        "meaning":"Snapshot of this runtime's named work at this input's context_at_unix. Persona, invocation and work identifiers are local to this runtime; matching labels elsewhere do not establish the same participant. Each last_attributed_work_judgment and last_authored_continuation is the peer's own accepted account or intention, not verified execution, quality, readiness, agreement or a new commitment. pending_host_commands names accepted operations whose complete result has not been published. working_directory is the retained launch directory when recorded, not a restriction on accessed paths, file ownership, a live process identifier or proof that a command is running. A command may change directory or access other paths. Host-wide process listings and similarly named files may belong to other work or runtimes; they are not this collaborator snapshot. Establish a current-work connection before attributing them to a peer or relying on them as this work's output. Shared host files can change while your call runs, including after a peer's judgment. Choose useful work, communication, coordination and inspection yourself. These records supply no command bodies, cited evidence or peer artifact bodies. Communicate sends a message without resuming a stopped peer. When useful, CONTROL.need_capability can invite existing personas to new work; acceptance remains their choice. Empty or omitted entries do not establish that files are unchanged or safe to overwrite."
    });
    let mut records = Vec::new();
    for peer in work.participants.keys().filter(|peer| peer.0 != actor) {
        let Some(persona) = state.personas.get(&peer.0) else {
            projection["records_omitted"] = json!(true);
            continue;
        };
        let mut inherited = Exposure::default();
        // A persona-proposed identity can carry creation-source restrictions,
        // including on its label, before it has authored any continuation.
        if let Some(origin) = &persona.birth {
            if !exposure_permitted(state, &origin.exposure, a) {
                projection["records_omitted"] = json!(true);
                continue;
            }
            extend_exposure(&mut inherited, &origin.exposure);
        }
        let acceptance = work.acceptance(peer).map(|(id, _)| id);
        if let Some(id) = acceptance {
            match arrangements::retained_acceptance_origin(state, work_id, id, a) {
                Ok(Some(origin)) => extend_exposure(&mut inherited, &origin),
                Ok(None) => {},
                Err(_) => {
                    projection["records_omitted"] = json!(true);
                    continue;
                }
            }
        }
        let mut value = json!({
            "persona":peer,"label":persona.seed.label,
            "current_acceptance":acceptance,
            "last_attributed_work_judgment":null,
            "last_authored_continuation":{"status":"none_retained"},
            "pending_host_commands":[],
            "pending_host_commands_omitted":false
        });
        if let Some(focus) = persona.focus.get(work_id) {
            value["last_attributed_work_judgment"] =
                last_judgment(state, focus, a, work.current_mandate, &mut inherited);
        }
        if let Some(record) = persona.focus.get(work_id)
            .and_then(|focus| focus.authored_continuation.as_ref())
        {
            if exposure_permitted(state, &record.exposure, a) {
                value["last_authored_continuation"] = json!({
                    "invocation":record.invocation,"value":record.value,
                    "body_supplied":true
                });
                extend_exposure(&mut inherited, &record.exposure);
            } else {
                value["last_authored_continuation"] = json!({
                    "status":"withheld_unavailable","body_supplied":false
                });
            }
        }
        let mut pending = Vec::new();
        for (intent, job) in state.host_command_jobs.iter().filter(|(_, job)| {
            job.work == work_id && job.actor == peer.0
                && (job.observation.is_none() || job.result_source.is_none())
        }) {
            if !exposure_permitted(state, &job.exposure, a) {
                value["pending_host_commands_omitted"] = json!(true);
                continue;
            }
            // The marker records acceptance and result publication. Its saved
            // intent state can lag dispatch, so do not present it as live state.
            pending.push(json!({
                "intent":intent,"invocation":job.invocation,"operation":job.operation,
                "working_directory":job.host_working_directory,
                "status":"awaiting_result_publication"
            }));
            extend_exposure(&mut inherited, &job.exposure);
        }
        value["pending_host_commands"] = json!(pending);
        // Keep each authored intention whole. Do not paraphrase or truncate it
        // into a host-written task assignment or claim of current activity.
        records.push(value);
        extend_exposure(exposure, &inherited);
    }
    projection["records"] = json!(records);
    Ok(Some(projection))
}
