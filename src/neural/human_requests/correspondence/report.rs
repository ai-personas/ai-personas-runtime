use super::{exact, failure, through, Connection, Result, Value};
use serde_json::json;

/// An operator's report on the original request, not on a delivery subsequently
/// attached to it. Receiving an edition or holding a request isn't a finding.
/// Require the explicit parent as well as the request and operator editions.
pub(in crate::neural) fn source(
    db: &Connection,
    requester: &str,
    work: &str,
    event: &str,
    notice: &Value,
) -> Result<(String, Value)> {
    let reference = notice["human_request"]
        .as_str()
        .ok_or_else(|| failure("Missing reported request edition"))?;
    let reported = exact(db, reference)?;
    if reported.reference.revision < 2
        || reported.body["requester"] != requester
        || reported.body["work"] != work
        || !notice["delivery"].is_null()
    {
        return Err(failure(
            "The report does not belong to this requester and activity",
        ));
    }
    let history = through(db, &reported)?;
    let issue = history.origin.detail;
    let entry = history.entries.last().filter(|e| {
        history.unverified == 0
            && e.kind == "human_report"
            && e.binding == "request_revision_and_origin"
            && e.request == reference
            && e.event.reference() == event
    });
    let Some(entry) = entry else {
        return Err(failure(
            "No verified report to the original request at this event",
        ));
    };
    if !history.origin.choice.finished_before(entry.event.seq)
        || entry.event.body(db)? != *notice
        || notice["reply_to"] != issue["experience"]
    {
        return Err(failure("The report differs from its request or recorded event"));
    }
    Ok((
        history.origin.choice.accepted.snapshot.invocation,
        json!({"endpoint":{"kind":"operation","id":history.origin.operation,"tool":"human.request"},
        "request":issue["reference"],"publication":issue["experience"],"asked":issue["action"],
        "return_requested":issue["return_requested"],"response_base":format!("human_request:{}@{}",reported.reference.id,reported.reference.revision-1),
        "reported_request":reference,"operator_action":entry.action,"reported_by":entry.event.author,
        "report_body":format!("object:{}",entry.event.digest)}),
    ))
}
