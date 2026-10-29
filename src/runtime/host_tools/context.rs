//! Primary-selected pending host result context with exact source attribution.
use super::*;
use schemars::JsonSchema;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResultContext {
    Complete,
    Output,
    Reference,
}

#[derive(Default)]
pub(in crate::runtime) struct Projection {
    pub references: Vec<f::Reference>,
    pub supplied_text: Vec<f::Reference>,
}

/// Expose exact text fields for either context choice. Complete fields are
/// supplied now; neither choice selects any field for a later input.
pub(in crate::runtime) fn pending_projection(
    state: &State,
    observation: &Observation,
    audience: &f::Admission,
    payload: &mut Value,
) -> Result<Projection> {
    project_result(state, observation, audience, payload, None)
}

/// The current reader's rejected historical read is distinct from the original
/// producer's pending-result choice. Only its exact retained rejection permits
/// reference delivery; ordinary historical reads still supply complete bodies.
pub(in crate::runtime) fn historical_recovery_projection(
    state: &State,
    observation: &Observation,
    audience: &f::Admission,
    payload: &mut Value,
    rejection: &provider_context_recovery::Pending,
) -> Result<Projection> {
    let reference = history::records::observation_identity(observation)
        .ok_or_else(|| held("historical result identity unavailable"))?.0;
    require(rejection.historical_results.values().any(|selected| *selected == reference),
        "historical result was not in the rejected input")?;
    project_result(state, observation, audience, payload, Some(rejection))
}

fn project_result(
    state: &State,
    observation: &Observation,
    audience: &f::Admission,
    payload: &mut Value,
    historical_recovery: Option<&provider_context_recovery::Pending>,
) -> Result<Projection> {
    if observation.kind != "host_command_result" {
        return Ok(Projection::default());
    }
    let intent = observation.payload["intent"]
        .as_str()
        .ok_or_else(|| invalid("pending host result intent missing"))?;
    let job = state
        .host_command_jobs
        .get(intent)
        .ok_or_else(|| invalid("pending host result job missing"))?;
    require(
        job.actor == observation.actor
            && job.work == observation.work
            && observation.invocation.as_deref() == Some(job.invocation.as_str())
            && job.observation.as_deref() == Some(observation.id.as_str()),
        "pending host result identity changed",
    )?;
    let source = job
        .result_source
        .as_ref()
        .ok_or_else(|| invalid("pending host result source missing"))?;
    let reference = file_reference(source)?;
    require(
        lineage_permitted(state, std::slice::from_ref(&reference), audience),
        "pending host result source unavailable",
    )?;
    // Historical observations predate an authored choice and retain their
    // complete projection. New choices are bound by the accepted command input.
    let choice = observation.payload["inputs"]
        .get("result_context")
        .map(|value| serde_json::from_value::<ResultContext>(value.clone()))
        .transpose()?
        .unwrap_or(ResultContext::Complete);
    let pending_recovery = state.personas.get(&observation.actor)
        .and_then(|persona| persona.focus.get(&observation.work))
        .and_then(|focus| focus.provider_context_recoveries.iter().rev()
            .find(|pending| pending.results.contains_key(&observation.id)));
    if let Some(pending) = pending_recovery.filter(|_| historical_recovery.is_none()) {
        let body = &pending.results[&observation.id];
        require(body.source == *source && body.payload_sha256 == digest(&observation.payload)?,
            "context rejection result binding changed")?;
    }
    let recovery = historical_recovery.or(pending_recovery);
    let mut references = vec![reference.clone()];
    let supply_inputs = recovery.is_none() && matches!(choice, ResultContext::Complete);
    let supply_outputs = recovery.is_none()
        && matches!(choice, ResultContext::Complete | ResultContext::Output);
    let mut fields = vec![reference_text(
        &mut payload["inputs"],
        "command",
        &reference,
        "/inputs/command".into(),
        supply_inputs,
    )?];
    if payload["inputs"]["stdin"].is_string() {
        fields.push(reference_text(
            &mut payload["inputs"],
            "stdin",
            &reference,
            "/inputs/stdin".into(),
            supply_inputs,
        )?);
    }
    let mut supplied_text = if supply_inputs { fields.clone() } else { Vec::new() };
    for stream in ["stdout", "stderr"] {
        let segments = payload["result"][stream]["segments"]
            .as_array_mut()
            .ok_or_else(|| invalid("pending host capture segments missing"))?;
        for (index, segment) in segments.iter_mut().enumerate() {
            let reference = reference_text(
                segment,
                "text",
                &reference,
                format!("/result/{stream}/segments/{index}/text"),
                supply_outputs,
            )?;
            if supply_outputs {
                supplied_text.push(reference.clone());
            }
            fields.push(reference);
        }
    }
    references.extend(fields.iter().cloned());
    let (format, meaning) = match (choice, recovery.is_some()) {
        (_, true) => (
            "host-command-pending-provider-context-recovery-v1",
            "The provider rejected an earlier input containing inline pending-result text with an explicit context or input-text length error. This fresh ordinary input supplies execution metadata and exact text references instead of command, literal stdin and output bodies. The earlier result-context choice remains recorded but was not fulfilled by a successful model call. All observed text remains retained; no tool is repeated, optional selection changed, quota imposed or body claimed read. Use ordinary reads or next-context selection to choose useful exact ranges or complete fields. Current character, work facts and required qualifications remain supplied. This reference delivery ends when the pending observation is consumed by an accepted ordinary decision.",
        ),
        (ResultContext::Complete, false) => (
            "host-command-pending-complete-v2",
            "All retained command, literal stdin and captured output text is supplied now. Each text field also has an exact decoded-string reference, byte count, hash and supplied=true. Its input-file handle can select only that field for later context, without copying the whole command record or rewriting the output. Asset stdin remains an exact input receipt. Output segments preserve their observed log offsets, digest and lossy_utf8 flag; older partial captures remain partial. The whole result reference names a separate retained document whose exact body is not supplied merely by this projection. Reading or selecting a result never repeats the command.",
        ),
        (ResultContext::Output, false) => (
            "host-command-pending-output-v1",
            "The primary chose captured stdout and stderr text with execution metadata, without repeating command or literal stdin bodies. Output segments have exact decoded-string references, byte counts, hashes and supplied=true; command and literal stdin have the same metadata with supplied=false. All captured text remains retained and readable through exact references. Asset stdin remains an exact input receipt. Raw offsets, digests, lossy_utf8 and capture completeness describe the original observation. This choice supplies no omitted command body, selects no later context and repeats no command. The whole result reference names a separate retained document, not a body supplied by this projection.",
        ),
        (ResultContext::Reference, false) => (
                "host-command-pending-reference-v2",
                "The primary chose execution metadata and exact references instead of command, literal stdin and output text in this pending context. All observed text remains retained. Each field reference names the complete decoded JSON string; ordinary file reads or next-context selections can supply any range or the complete string, without a text quota. The whole result reference can supply the complete document. Neither reference metadata nor capture_complete establishes that text was supplied to the model. Raw offsets, digests, lossy_utf8 and capture completeness still describe the original observation. Required qualifications remain supplied. Reading a result never repeats the command.",
            ),
    };
    payload["context_projection"] = json!({
        "format":format,
        "text_limit_bytes":null,
        "retained_observation_payload_sha256":digest(&observation.payload)?,
        "retained_result":{"source":source,"body_supplied":false},
        "context_lifetime":{"pending_observation_consumed":"after_this_accepted_decision",
            "automatically_selected_for_later":false,"retain_with":"next_assets"},
        "meaning":meaning
    });
    if let Some(pending) = recovery {
        payload["context_projection"]["provider_rejection"] = json!({
            "invocation":pending.invocation,"terminal":pending.terminal,
            "original_result_context":choice,"original_request_preserved":true,
            "selected_context_changed":false
        });
    }
    if historical_recovery.is_some() {
        payload["context_projection"]["format"] = json!("host-command-historical-provider-context-recovery-v1");
        payload["context_projection"]["context_lifetime"] = json!({
            "pending_history_read_consumed":"after_this_accepted_decision",
            "automatically_selected_for_later":false,"retain_with":"next_assets"
        });
        payload["context_projection"]["meaning"] = json!("The provider rejected the current reader's earlier input containing this complete historical host result. This historical projection supplies execution metadata and exact references instead of command, literal stdin and output text. The original observation, requested read, failed input and captured bytes remain intact. Field byte counts and hashes describe retained text, not supplied bodies. Choose useful fields or measured ranges through ordinary file reads or next-context selection; stdin_asset can pass exact retained bytes to a host command. Original actor, work, invocation, historical status and current required qualifications remain unchanged. This does not repeat the original command, reactivate an event, change a selection or claim successful body supply. The override ends when an accepted decision consumes the current reader's pending history read.");
    }
    Ok(Projection {
        references,
        supplied_text,
    })
}

/// Decorate a pending text field, omitting its body when it was not selected.
/// The sealed result and original observation keep the exact original bytes.
fn reference_text(
    object: &mut Value,
    field: &str,
    result: &f::Reference,
    pointer: String,
    supplied: bool,
) -> Result<f::Reference> {
    let object = object
        .as_object_mut()
        .ok_or_else(|| invalid("pending host text container missing"))?;
    let Some(Value::String(text)) = object.get(field) else {
        return Err(invalid("pending host text missing"));
    };
    let bytes = text.len();
    let sha256 = s::sha256_bytes(text.as_bytes());
    if !supplied {
        object.remove(field);
    }
    let reference = f::Reference::full(
        &result.source,
        &result.revision,
        f::Representation::JsonString { pointer },
    );
    object.insert(
        format!("{field}_reference"),
        serde_json::to_value(wire_reference(&reference))?,
    );
    object.insert(format!("{field}_utf8_bytes"), json!(bytes));
    object.insert(format!("{field}_sha256"), json!(sha256));
    object.insert(format!("{field}_supplied"), json!(supplied));
    Ok(reference)
}

/// Inspect only the current native inline projections. Authored JSON, nested
/// history and reference-only fields are not inline body-supply evidence.
pub(in crate::runtime) fn supplied_fields<'a>(
    control: &'a Value,
    prepared: &Prepared,
) -> Result<Vec<(f::Reference, &'a str, Value)>> {
    let mut fields = Vec::new();
    for observation in control["actual_observations"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let payload = &observation["payload"];
        let complete = payload["context_projection"]["format"]
            == "host-command-pending-complete-v2";
        let output_only = payload["context_projection"]["format"]
            == "host-command-pending-output-v1";
        if observation["kind"] != "host_command_result"
            || observation["actor"] != prepared.request.persona
            || observation["work"] != prepared.request.work
            || !observation["id"]
                .as_str()
                .is_some_and(|id| prepared.supplied_observations.iter().any(|i| i == id))
            || (!complete && !output_only)
        {
            continue;
        }
        let source = file_reference(&serde_json::from_value(
            payload["context_projection"]["retained_result"]["source"].clone(),
        )?)?;
        let mut append = |container: &'a Value, field: &str, pointer: String| -> Result<()> {
            let text = container[field]
                .as_str()
                .ok_or_else(|| held("supplied host text field missing"))?;
            let reference = file_reference(&serde_json::from_value(
                container[format!("{field}_reference")].clone(),
            )?)?;
            require(
                reference
                    == f::Reference::full(
                        &source.source,
                        &source.revision,
                        f::Representation::JsonString {
                            pointer: pointer.clone(),
                        },
                    )
                    && container[format!("{field}_supplied")] == true
                    && container[format!("{field}_utf8_bytes")] == text.len() as u64
                    && container[format!("{field}_sha256")] == s::sha256_bytes(text.as_bytes()),
                "supplied host text metadata mismatch",
            )?;
            fields.push((
                reference,
                text,
                json!({"kind":"pending_host_result_text",
                "observation":observation["id"],"producing_invocation":observation["invocation"],
                "pointer":pointer}),
            ));
            Ok(())
        };
        if complete {
            append(&payload["inputs"], "command", "/inputs/command".into())?;
            if payload["inputs"]["stdin"].is_string() {
                append(&payload["inputs"], "stdin", "/inputs/stdin".into())?;
            }
        }
        for stream in ["stdout", "stderr"] {
            for (index, segment) in payload["result"][stream]["segments"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                append(
                    segment,
                    "text",
                    format!("/result/{stream}/segments/{index}/text"),
                )?;
            }
        }
    }
    Ok(fields)
}
