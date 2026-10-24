//! Primary-selected pending host result context with exact source attribution.
use super::*;
use schemars::JsonSchema;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResultContext {
    Complete,
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
    let mut references = vec![reference.clone()];
    let supplied = matches!(choice, ResultContext::Complete);
    let mut fields = vec![reference_text(
        &mut payload["inputs"],
        "command",
        &reference,
        "/inputs/command".into(),
        supplied,
    )?];
    if payload["inputs"]["stdin"].is_string() {
        fields.push(reference_text(
            &mut payload["inputs"],
            "stdin",
            &reference,
            "/inputs/stdin".into(),
            supplied,
        )?);
    }
    for stream in ["stdout", "stderr"] {
        let segments = payload["result"][stream]["segments"]
            .as_array_mut()
            .ok_or_else(|| invalid("pending host capture segments missing"))?;
        for (index, segment) in segments.iter_mut().enumerate() {
            fields.push(reference_text(
                segment,
                "text",
                &reference,
                format!("/result/{stream}/segments/{index}/text"),
                supplied,
            )?);
        }
    }
    references.extend(fields.iter().cloned());
    let (format, meaning) = match choice {
        ResultContext::Complete => (
            "host-command-pending-complete-v2",
            "All retained command, literal stdin and captured output text is supplied now. Each text field also has an exact decoded-string reference, byte count, hash and supplied=true. Its input-file handle can select only that field for later context, without copying the whole command record or rewriting the output. Asset stdin remains an exact input receipt. Output segments preserve their observed log offsets, digest and lossy_utf8 flag; older partial captures remain partial. The whole result reference names a separate retained document whose exact body is not supplied merely by this projection. Reading or selecting a result never repeats the command.",
        ),
        ResultContext::Reference => (
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
    Ok(Projection {
        references,
        supplied_text: if supplied { fields } else { Vec::new() },
    })
}

/// Decorate a pending text field, removing its text only for reference mode.
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

/// Inspect only the current native complete projection. Authored JSON, nested
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
        if observation["kind"] != "host_command_result"
            || observation["actor"] != prepared.request.persona
            || observation["work"] != prepared.request.work
            || !observation["id"]
                .as_str()
                .is_some_and(|id| prepared.supplied_observations.iter().any(|i| i == id))
            || payload["context_projection"]["format"] != "host-command-pending-complete-v2"
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
        append(&payload["inputs"], "command", "/inputs/command".into())?;
        if payload["inputs"]["stdin"].is_string() {
            append(&payload["inputs"], "stdin", "/inputs/stdin".into())?;
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
