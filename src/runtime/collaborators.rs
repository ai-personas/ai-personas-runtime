//! Optional current-work peer intentions, with original information lineage.
//! This snapshot supplies neither another persona's context nor execution state.
use super::*;

const MAX_RECORDS: usize = 8;
const MAX_BYTES: usize = 4096;

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
    let maximum = MAX_BYTES.min(state.config.context_bytes / 16);
    let mut projection = json!({
        "profile":"current-work-collaborators-v1",
        "records":[],
        "records_omitted":false,
        "meaning":"Snapshot at this input's context_at_unix. Each last_authored_continuation is that peer's accepted intention, not observed execution, results, readiness, agreement or a new commitment. It may change while your call runs. Choose your own contribution and whether to communicate or agree on shared edits. This supplies no peer file body or mutation authority. Omitted entries establish neither absence nor permission."
    });
    if work_inspection::bounded(&projection, maximum).is_none() {
        return Ok(None);
    }
    let mut records = Vec::new();
    for peer in work.participants.keys().filter(|peer| peer.0 != actor) {
        if records.len() == MAX_RECORDS {
            projection["records_omitted"] = json!(true);
            break;
        }
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
            "last_authored_continuation":{"status":"none_retained"}
        });
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
        // Keep each authored intention whole. Do not paraphrase or truncate it
        // into a host-written task assignment or claim of current activity.
        let mut candidate = projection.clone();
        let mut candidate_records = records.clone();
        candidate_records.push(value);
        candidate["records"] = json!(candidate_records);
        if work_inspection::bounded(&candidate, maximum).is_some() {
            records = candidate_records;
            projection = candidate;
            extend_exposure(exposure, &inherited);
        } else {
            projection["records_omitted"] = json!(true);
        }
    }
    Ok(Some(projection))
}
