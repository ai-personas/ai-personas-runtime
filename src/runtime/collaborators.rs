//! Current-work peer accounts, pending commands and latest result/file cues.
//! This supplies neither private context nor proof that a process is running.
use super::*;

fn native_asset_save(
    state: &State,
    observation: &Observation,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Option<Value> {
    let invocation = observation.invocation.as_deref()?;
    let operation = observation.payload["operation_id"].as_str()?;
    let wire: p::ExactRef = serde_json::from_value(observation.payload["source"].clone()).ok()?;
    let reference = file_reference(&wire).ok()?;
    let stored = record(state, &reference).ok()?;
    if observation.id != observation_id(invocation, "accepted_file_write", operation)
        || observation.payload["accepted_head"] != reference.revision
        || observation.payload["metadata_only"] != true
        || stored.record.scope != f::Scope::Assets
        || stored.record.author != observation.actor
    {
        return None;
    }
    let (kind, author, call, id) = match stored.origin.as_ref()? {
        RevisionOrigin::PersonaInvocation { persona, invocation_id, operation_id } =>
            ("file_write", persona, invocation_id, operation_id),
        RevisionOrigin::NativeFileEdit { persona, invocation_id, operation_id, .. } =>
            ("file_edit", persona, invocation_id, operation_id),
        RevisionOrigin::HostFileImport { persona, invocation_id, operation_id, .. } =>
            ("file_import", persona, invocation_id, operation_id),
        _ => return None,
    };
    if author != &observation.actor || call != invocation || id != operation {
        return None;
    }
    let mut inherited = observation.exposure.as_ref()?.clone();
    inherited.sources.extend(observation.sources.iter().cloned());
    inherited.sources.extend(observation.qualified_roots.iter().cloned());
    inherited.sources.push(reference.clone());
    if !exposure_permitted(state, &inherited, a) {
        return None;
    }
    let (receipt, _) = history::records::observation_identity(observation)?;
    let source = state.sources.get(&reference.source)?;
    let value = json!({
        "invocation":invocation,"operation":operation,"save_kind":kind,
        "actor":observation.actor,"work":observation.work,"receipt":receipt,
        "file_reference":wire_reference(&reference),"path":stored.record.path,
        "file_revision_is_current_head":source.head == reference.revision,
        "source_retired":source.retired,
        "receipt_body_supplied_by_cue":false,"file_body_supplied_by_cue":false
    });
    extend_exposure(exposure, &inherited);
    Some(value)
}

/// Exact supporting-file saves from one accepted invocation. The same receipt
/// checks apply to a peer's latest work and the actor's recent other work.
pub(super) fn native_asset_saves(
    state: &State,
    actor: &str,
    work: &str,
    invocation: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> (Vec<Value>, bool) {
    let mut saves = Vec::new();
    let mut omitted = false;
    for (id, observation) in state.observations.iter().filter(|(_, observation)| {
        observation.work == work && observation.actor == actor
            && observation.invocation.as_deref() == Some(invocation)
            && observation.kind == "accepted_file_write"
            && observation.payload["scope"] == "assets"
    }) {
        // Bind the atomic save receipt to the immutable revision's origin,
        // never a planned write or an inferred host workspace path.
        match (observation.id == *id).then(||
            native_asset_save(state, observation, a, exposure)).flatten()
        {
            Some(saved) => saves.push(saved),
            None => omitted = true,
        }
    }
    (saves, omitted)
}

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
                "citation_lookup":judgment_citations::project(state, judgment, a, exposure),
                "meaning":"Historical accepted persona judgment based on information supplied then. A different mandate does not establish completion of the current request; the same mandate does not resolve later feedback or results. This supplies the authored judgment and current citation lookup metadata, not cited bodies, artifact bodies or verified quality. CONTROL.history_retrieval describes selective lookup of cited retained records, including other personas' records. Missing origin establishes neither invocation nor mandate. Decide afresh using the current request and observations."})
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
    // The roster stays within this work. Its judgments can cite retained receipts
    // from other work; citation lookup supplies identities, not private context.
    let mut projection = json!({
        "profile":"current-work-collaborators-v9",
        "work":work_id,
        "records":[],
        "records_omitted":false,
        "meaning":"Snapshot of this runtime's named work at this input's context_at_unix. Persona, invocation and work identifiers are local to this runtime; matching labels elsewhere do not establish the same participant. Each last_attributed_work_judgment and last_authored_continuation is the peer's own accepted account or intention, not verified execution, quality, readiness, agreement or a new commitment. pending_host_commands names accepted operations whose complete result has not been published. latest_host_command_results links published receipts for commands from that peer's latest accepted work-judgment invocation, even before the peer has received or discussed those results. These exact history references and recorded execution states supply neither result bodies nor a quality verdict. Choose a useful receipt through CONTROL.input_history_choices or exact history lookup. latest_native_asset_saves names supporting files actually saved by that same latest work-judgment invocation, binding each exact file revision to its accepted save receipt and native path. A saved draft is not a delivery, shared candidate or quality judgment; file_import establishes imported bytes, not content authorship. The current-head and retirement fields describe this admission snapshot. latest_message_file_citations names exact native files attached to actual messages from that same invocation, with original sender, recipient and message identity. Choose an exact native read, or use current input_file_choices handles for next-context selection or citation. Native paths name retained files, not host workspace paths; use their exact references instead of assuming a host copy exists. These cues supply neither message nor file bodies, establish no recipient attention or quality, and choose no shared candidate. Earlier messages and save receipts remain discoverable in history; other native files remain discoverable by path and text. working_directory is the retained launch directory when recorded, not a restriction on accessed paths, file ownership, a live process identifier or proof that a command is running. A command may change directory or access other paths. Native parent and child work relationships do not imply parent or sibling filesystem directories. Host-wide listings and matching filenames or local IDs may belong to other runtimes. Establish the source work and participant connection before attributing a host file or export to a peer or relying on it as this work's output. Shared host files can change while your call runs, including after a peer's judgment. Choose useful work, communication, coordination and inspection yourself. These records supply no command bodies, cited evidence or peer artifact bodies. Communicate sends a message without resuming a stopped peer. When useful, CONTROL.need_capability can invite existing personas to new work; acceptance remains their choice. Empty or omitted entries do not establish that files are unchanged or safe to overwrite."
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
            "pending_host_commands_omitted":false,
            "latest_host_command_results":[],
            "latest_host_command_results_omitted":false,
            "latest_native_asset_saves":[],
            "latest_native_asset_saves_omitted":false,
            "latest_message_file_citations":[],
            "latest_message_file_citations_omitted":false
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
        let mut published = Vec::new();
        let latest_invocation = persona.focus.get(work_id)
            .and_then(|focus| focus.judgment_origin.as_ref())
            .map(|origin| origin.invocation.as_str());
        if let Some(invocation) = latest_invocation {
            let (saves, omitted) = native_asset_saves(
                state, &peer.0, work_id, invocation, a, &mut inherited,
            );
            value["latest_native_asset_saves"] = json!(saves);
            value["latest_native_asset_saves_omitted"] = json!(omitted);
        }
        let mut citations = Vec::new();
        for (id, message) in state.messages.iter().filter(|(_, message)| {
            message.work == work_id && message.sender == peer.0
                && latest_invocation == Some(message.invocation.as_str())
                && !message.file_references.is_empty()
        }) {
            // Only immutable message formats that bind their attachments can
            // supply this metadata. Never derive citations from authored prose
            // or the unadopted operations in an archived Decision.
            let complete = message.record_format.as_deref() == Some(history::MESSAGE_RECORD_V3)
                || (message.record_format.is_none() && message.thread_work.is_some());
            let Some(source) = message.exposure.as_ref().filter(|source| {
                message.id == *id && complete && exposure_permitted(state, source, a)
                    && lineage_permitted(state, &message.lineage, a)
            }) else {
                value["latest_message_file_citations_omitted"] = json!(true);
                continue;
            };
            let Some(sha256) = history::message_identity(message) else {
                value["latest_message_file_citations_omitted"] = json!(true);
                continue;
            };
            citations.push(json!({
                "message_id":message.id,"sha256":sha256,
                "work":message.work,"invocation":message.invocation,
                "sender":message.sender,"recipient":message.recipient,
                "file_references":message.file_references,
                "local_deposit_only":message.local_deposit_only,
                "message_body_supplied_by_cue":false,"file_bodies_supplied_by_cue":false
            }));
            extend_exposure(&mut inherited, source);
            inherited.sources.extend(message.lineage.iter().cloned());
        }
        value["latest_message_file_citations"] = json!(citations);
        for (intent, job) in state.host_command_jobs.iter().filter(|(_, job)| {
            job.work == work_id && job.actor == peer.0
        }) {
            let complete = job.observation.is_some() && job.result_source.is_some();
            if complete && latest_invocation != Some(job.invocation.as_str()) {
                continue;
            }
            if !exposure_permitted(state, &job.exposure, a) {
                value[if complete { "latest_host_command_results_omitted" }
                    else { "pending_host_commands_omitted" }] = json!(true);
                continue;
            }
            if complete {
                if let Some(mut result) = judgment_citations::published_host_command(state, intent, &mut inherited) {
                    result["intent"] = json!(intent);
                    result["operation"] = json!(job.operation);
                    result["working_directory"] = json!(job.host_working_directory);
                    published.push(result);
                    extend_exposure(&mut inherited, &job.exposure);
                } else {
                    value["latest_host_command_results_omitted"] = json!(true);
                }
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
        value["latest_host_command_results"] = json!(published);
        // Keep each authored intention whole. Do not paraphrase or truncate it
        // into a host-written task assignment or claim of current activity.
        records.push(value);
        extend_exposure(exposure, &inherited);
    }
    projection["records"] = json!(records);
    Ok(Some(projection))
}
