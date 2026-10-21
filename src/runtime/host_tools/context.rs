//! Bounded pending-result excerpts; immutable results and explicit reads stay full.
use super::*;

const PENDING_TEXT_BYTES: usize = 8 * 1024;
const FORMAT: &str = "host-command-pending-excerpts-v1";

pub(in crate::runtime) fn pending_text_limit(state: &State, pending: &[String]) -> usize {
    let results = pending
        .iter()
        .filter(|id| {
            state
                .observations
                .get(*id)
                .is_some_and(|o| o.kind == "host_command_result")
        })
        .count();
    PENDING_TEXT_BYTES / results.max(1)
}

fn excerpt(value: &mut Value, limit: usize, tail: bool) -> Option<Value> {
    let original = value.as_str()?;
    let length = original.len();
    let (start, end) = if tail {
        let mut start = length.saturating_sub(limit);
        while !original.is_char_boundary(start) {
            start += 1;
        }
        (start, length)
    } else {
        let mut end = length.min(limit);
        while !original.is_char_boundary(end) {
            end -= 1;
        }
        (0, end)
    };
    let metadata = json!({"retained_utf8_bytes":length,"retained_utf8_sha256":s::sha256_bytes(original.as_bytes()),
        "start_byte":start,"end_byte":end,"complete":start == 0 && end == length});
    *value = Value::String(original[start..end].to_owned());
    Some(metadata)
}

/// Return a metadata-only exact result reference after its current permission
/// checks. Its body is not added to the supplied-body set or selected implicitly.
pub(in crate::runtime) fn pending_projection(
    state: &State,
    observation: &Observation,
    audience: &f::Admission,
    payload: &mut Value,
    text_limit: usize,
) -> Result<Option<f::Reference>> {
    if observation.kind != "host_command_result" {
        return Ok(None);
    }
    let Some(intent) = observation.payload["intent"].as_str() else {
        return Ok(None);
    };
    let Some(job) = state.host_command_jobs.get(intent) else {
        return Ok(None);
    };
    require(
        job.actor == observation.actor
            && job.work == observation.work
            && observation.invocation.as_deref() == Some(job.invocation.as_str())
            && job.observation.as_deref() == Some(observation.id.as_str()),
        "pending host result identity changed",
    )?;
    let Some(source) = &job.result_source else {
        // A legacy result without a retained readable body keeps its original
        // projection; do not discard its only available result text.
        return Ok(None);
    };
    let reference = file_reference(source)?;
    if !lineage_permitted(state, std::slice::from_ref(&reference), audience) {
        return Ok(None);
    }
    let mut fields = serde_json::Map::new();
    let input_limit = text_limit / 8;
    let stream_limit = (text_limit - input_limit * 2) / 2;
    if let Some(inputs) = payload.get_mut("inputs").and_then(Value::as_object_mut) {
        for name in ["command", "stdin"] {
            if let Some(value) = inputs.get_mut(name) {
                if let Some(metadata) = excerpt(value, input_limit, false) {
                    fields.insert(format!("inputs.{name}"), metadata);
                }
            }
        }
    }
    if let Some(result) = payload.get_mut("result").and_then(Value::as_object_mut) {
        for name in ["stdout", "stderr"] {
            if let Some(value) = result.get_mut(name) {
                if let Some(metadata) = excerpt(value, stream_limit, true) {
                    fields.insert(format!("result.{name}"), metadata);
                }
            }
        }
    }
    payload["context_projection"] = json!({
        "format":FORMAT,"fields":fields,"text_limit_bytes":text_limit,
        "retained_observation_payload_sha256":digest(&observation.payload)?,
        "retained_result":{"source":source,"body_supplied":false},
        "meaning":"Execution status and metadata are observed. Command/stdin prefixes and stdout/stderr tails may be partial; field offsets and hashes describe the retained UTF-8 strings. The original capture truncation flags and observed byte counts still describe execution, not this context projection. Omitted text is unknown here. Read the exact retained result or host logs for more; reading a result never repeats the command."
    });
    Ok(Some(reference))
}
