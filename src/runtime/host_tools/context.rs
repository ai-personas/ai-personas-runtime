//! Primary-selected pending host result context with exact source attribution.
use super::*;
use schemars::JsonSchema;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResultContext {
    Complete,
    Reference,
}

/// Return metadata-only exact references. Their bodies are not added to the
/// supplied-body set or selected implicitly; qualifications remain supplied.
pub(in crate::runtime) fn pending_projection(
    state: &State,
    observation: &Observation,
    audience: &f::Admission,
    payload: &mut Value,
) -> Result<Vec<f::Reference>> {
    if observation.kind != "host_command_result" {
        return Ok(Vec::new());
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
    let (format, meaning) = match choice {
        ResultContext::Complete => (
            "host-command-pending-complete-v1",
            "All retained command, literal stdin and captured output text is supplied. Asset stdin remains an exact input receipt. Each output segment preserves its observed log offsets, digest and lossy_utf8 flag. Older partial captures remain partial; capture_complete describes the original observation. The result reference names a separate retained document and does not imply its exact body was supplied. Reading a result never repeats the command.",
        ),
        ResultContext::Reference => {
            references.push(reference_text(
                &mut payload["inputs"], "command", &reference, "/inputs/command".into(),
            )?);
            if payload["inputs"]["stdin"].is_string() {
                references.push(reference_text(
                    &mut payload["inputs"], "stdin", &reference, "/inputs/stdin".into(),
                )?);
            }
            for stream in ["stdout", "stderr"] {
                let segments = payload["result"][stream]["segments"]
                    .as_array_mut()
                    .ok_or_else(|| invalid("pending host capture segments missing"))?;
                for (index, segment) in segments.iter_mut().enumerate() {
                    references.push(reference_text(
                        segment, "text", &reference,
                        format!("/result/{stream}/segments/{index}/text"),
                    )?);
                }
            }
            (
                "host-command-pending-reference-v1",
                "The primary chose execution metadata and exact references instead of command, literal stdin and output text in this pending context. All observed text remains retained. Each field reference names the complete decoded JSON string; ordinary file reads or next-context selections can supply any range or the complete string, without a text quota. The whole result reference can supply the complete document. Neither reference metadata nor capture_complete establishes that text was supplied to the model. Raw offsets, digests, lossy_utf8 and capture completeness still describe the original observation. Required qualifications remain supplied. Reading a result never repeats the command.",
            )
        }
    };
    payload["context_projection"] = json!({
        "format":format,
        "text_limit_bytes":null,
        "retained_observation_payload_sha256":digest(&observation.payload)?,
        "retained_result":{"source":source,"body_supplied":false},
        "meaning":meaning
    });
    Ok(references)
}

/// Change only the pending projection. The sealed result and original
/// observation retain the complete text, including empty strings.
fn reference_text(
    object: &mut Value,
    field: &str,
    result: &f::Reference,
    pointer: String,
) -> Result<f::Reference> {
    let object = object
        .as_object_mut()
        .ok_or_else(|| invalid("pending host text container missing"))?;
    let Some(Value::String(text)) = object.remove(field) else {
        return Err(invalid("pending host text missing"));
    };
    let reference = f::Reference::full(
        &result.source,
        &result.revision,
        f::Representation::JsonString { pointer },
    );
    object.insert(
        format!("{field}_reference"),
        serde_json::to_value(wire_reference(&reference))?,
    );
    object.insert(format!("{field}_utf8_bytes"), json!(text.len()));
    object.insert(
        format!("{field}_sha256"),
        json!(s::sha256_bytes(text.as_bytes())),
    );
    object.insert(format!("{field}_supplied"), json!(false));
    Ok(reference)
}
