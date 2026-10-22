//! Complete pending host results with exact retained-source attribution.
use super::*;

/// Return a metadata-only exact result reference after its current permission
/// checks. Its body is not added to the supplied-body set or selected implicitly.
pub(in crate::runtime) fn pending_projection(
    state: &State,
    observation: &Observation,
    audience: &f::Admission,
    payload: &mut Value,
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
    payload["context_projection"] = json!({
        "format":"host-command-pending-complete-v1",
        "text_limit_bytes":null,
        "retained_observation_payload_sha256":digest(&observation.payload)?,
        "retained_result":{"source":source,"body_supplied":false},
        "meaning":"The complete retained command, stdin and captured output are supplied. Each output segment preserves its observed log offsets, digest and lossy_utf8 flag. Older partial captures remain partial; capture_complete describes the original observation. The result reference names a separate retained document and does not imply its exact body was supplied. Reading a result never repeats the command."
    });
    Ok(Some(reference))
}
