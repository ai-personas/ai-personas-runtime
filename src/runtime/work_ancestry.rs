//! The requested result survives a persona's decomposition into smaller needs.
//! This is current request context, never an inherited assignment or workflow.
use super::*;

pub(super) fn context(
    state: &State, work_id: &str, audience: &f::Admission, exposure: &mut Exposure,
) -> Result<Option<Value>> {
    let mut current = work_id;
    let mut seen = BTreeSet::from([work_id]);
    let mut records = Vec::new();
    while let Some((episode, child)) = need_proposals::originating_child(state, current) {
        let parent = child.parent_work(episode);
        require(seen.insert(parent), "cyclic requested-result ancestry")?;
        let work = state.board.work(&w::WorkId(parent.into()))?;
        let meta = state.work_meta.get(parent)
            .ok_or_else(|| held("ancestor request metadata absent"))?;
        let inherited = work_exposure(state, parent)?;
        if !exposure_permitted(state, &inherited, audience) {
            records.push(json!({"work":parent,"status":"unavailable","body_supplied":false}));
            break;
        }
        // Never present the original intake as the current request after an
        // amendment. An absent exact effective scope is unknown, not a fallback.
        let input = if work.current_mandate == 1 {
            &meta.original
        } else {
            &meta.amendments.last()
                .filter(|record| record.mandate_revision == work.current_mandate)
                .ok_or_else(|| held("current ancestor request scope unavailable"))?.input
        };
        records.push(json!({
            "work":parent,"child_work":current,"requester":work.need.origin,
            "mandate_revision":work.current_mandate,"work_version":work.version,
            "phase":work.phase,"request":input.request,"outcomes":input.outcomes,
            "boundaries":input.boundaries,"assumptions":input.assumptions,
            "terms":input.terms,"delivery_to":input.delivery_to,
            "body_supplied":true,
            "recorded_artifacts":work.artifacts.len(),"recorded_assessments":work.assessments.len(),
            "recorded_deliveries":work.deliveries.len(),
        }));
        extend_exposure(exposure, &inherited);
        current = parent;
    }
    if records.is_empty() { return Ok(None); }
    records.reverse();
    Ok(Some(json!({
        "profile":"requested-result-ancestry-v1","current_work":work_id,
        "ordering":"outermost request first; immediate parent last","records":records,
        "meaning":"Exact current ancestor requests explain the result this contribution serves. A narrow child report does not establish completion of that result. Counts describe registered records, not all host files or quality. Your current accepted scope remains separate: these records create no assignment, acceptance, resume or change to another work. Judge how your contribution helps the requested result; choose useful implementation, integration, communication or further cooperation yourself. Parent and child phases remain separate. No peer's private working notes or selected context are supplied."
    })))
}
