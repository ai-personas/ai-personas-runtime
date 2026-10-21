//! Bounded pending-result excerpts; immutable results and explicit reads stay full.
use super::*;

const PENDING_TEXT_BYTES: usize = 8 * 1024;
const FORMAT: &str = "host-command-pending-segments-v2";

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

fn prefix(value: &mut Value, limit: usize) -> Option<(Value, usize)> {
    let original = value.as_str()?;
    let length = original.len();
    let end = prefix_end(original, limit);
    let metadata = json!({"retained_utf8_bytes":length,"retained_utf8_sha256":s::sha256_bytes(original.as_bytes()),
        "start_byte":0,"end_byte":end,"complete":end == length});
    *value = Value::String(original[..end].to_owned());
    Some((metadata, end))
}

fn prefix_end(text: &str, limit: usize) -> usize {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn suffix_start(text: &str, limit: usize) -> usize {
    let mut start = text.len().saturating_sub(limit);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    start
}

/// Split fairly, giving unused space to the other stream or segment.
fn split_budget(limit: usize, left: usize, right: usize) -> (usize, usize) {
    let left_initial = left.min(limit.div_ceil(2));
    let right = right.min(limit - left_initial);
    (left.min(limit - right), right)
}

fn project_stream(
    capture: &h::OutputCapture,
    limit: usize,
    name: &str,
    fields: &mut serde_json::Map<String, Value>,
) -> Result<Value> {
    require(
        capture.segments.len() <= 2,
        "unsupported host output segment count",
    )?;
    let mut value = serde_json::to_value(capture)?;
    let budgets = match capture.segments.as_slice() {
        [] => Vec::new(),
        [one] => vec![one.text.len().min(limit)],
        [first, last] => {
            let (first, last) = split_budget(limit, first.text.len(), last.text.len());
            vec![first, last]
        }
        _ => unreachable!(),
    };
    for (index, (segment, budget)) in capture.segments.iter().zip(budgets).enumerate() {
        let text = &segment.text;
        let ranges = if text.len() <= budget {
            vec![(0, text.len())]
        } else if capture.segments.len() == 1 {
            vec![
                (0, prefix_end(text, budget.div_ceil(2))),
                (suffix_start(text, budget / 2), text.len()),
            ]
        } else if index == 0 {
            vec![(0, prefix_end(text, budget))]
        } else {
            vec![(suffix_start(text, budget), text.len())]
        };
        let complete = ranges.len() == 1 && ranges[0] == (0, text.len());
        let excerpts: Vec<_> = ranges
            .into_iter()
            .filter(|(start, end)| start < end || text.is_empty())
            .map(|(start, end)| json!({"start_byte":start,"end_byte":end,"text":&text[start..end]}))
            .collect();
        fields.insert(format!("result.{name}.segments.{index}.text"), json!({
            "retained_utf8_bytes":text.len(),
            "retained_utf8_sha256":s::sha256_bytes(text.as_bytes()),
            "complete":complete,
            "range_coordinates":"text_excerpts use byte offsets within this retained segment's decoded UTF-8 text"
        }));
        let projected = value["segments"][index].as_object_mut().unwrap();
        projected.remove("text");
        projected.insert("text_excerpts".into(), json!(excerpts));
    }
    Ok(value)
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
    let captures = payload
        .get("result")
        .and_then(Value::as_object)
        .map(|result| -> Result<(h::OutputCapture, h::OutputCapture)> {
            Ok((
                serde_json::from_value(result["stdout"].clone())?,
                serde_json::from_value(result["stderr"].clone())?,
            ))
        })
        .transpose()?;
    let command_bytes = payload["inputs"]["command"].as_str().map_or(0, str::len);
    let stdin_bytes = payload["inputs"]["stdin"].as_str().map_or(0, str::len);
    let minimum_inputs = command_bytes.min(text_limit / 8) + stdin_bytes.min(text_limit / 8);
    let output_bytes = captures.as_ref().map_or(0, |(stdout, stderr)| {
        stdout
            .retained_text_bytes()
            .saturating_add(stderr.retained_text_bytes())
    });
    // Preserve the existing output allowance. Only space the captured streams
    // cannot use extends command/stdin prefixes, which may now fit in full.
    let input_limit = text_limit - output_bytes.min(text_limit - minimum_inputs);
    let (command_limit, stdin_limit) = split_budget(input_limit, command_bytes, stdin_bytes);
    let mut fields = serde_json::Map::new();
    let mut input_bytes = 0;
    if let Some(inputs) = payload.get_mut("inputs").and_then(Value::as_object_mut) {
        for (name, limit) in [("command", command_limit), ("stdin", stdin_limit)] {
            if let Some(value) = inputs.get_mut(name) {
                if let Some((metadata, bytes)) = prefix(value, limit) {
                    input_bytes += bytes;
                    fields.insert(format!("inputs.{name}"), metadata);
                }
            }
        }
    }
    if let Some((stdout, stderr)) = captures {
        let result = payload["result"].as_object_mut().unwrap();
        let (out_limit, err_limit) = split_budget(
            text_limit - input_bytes,
            stdout.retained_text_bytes(),
            stderr.retained_text_bytes(),
        );
        result.insert(
            "stdout".into(),
            project_stream(&stdout, out_limit, "stdout", &mut fields)?,
        );
        result.insert(
            "stderr".into(),
            project_stream(&stderr, err_limit, "stderr", &mut fields)?,
        );
    }
    payload["context_projection"] = json!({
        "format":FORMAT,"fields":fields,"text_limit_bytes":text_limit,
        "retained_observation_payload_sha256":digest(&observation.payload)?,
        "retained_result":{"source":source,"body_supplied":false},
        "meaning":"Execution status and metadata are observed. Command/stdin keep prefixes, using spare text space when captured output is short. Each stdout/stderr segment retains its raw log offsets, digest and lossy_utf8 flag; text_excerpts give separately bounded beginning/end text with offsets within that segment's retained decoded UTF-8 text. Gaps are omitted output: never concatenate excerpts as if contiguous or complete. capture_complete describes the runner's captured ranges, while fields.complete describes text supplied here. bytes_observed=null means capture was unavailable. Read the exact retained result for its captured segments or stdout.log_path/stderr.log_path for more host output; reading a result never repeats the command."
    });
    Ok(Some(reference))
}
