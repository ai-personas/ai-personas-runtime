//! Find earlier occasions the owner explicitly considered for character maintenance,
//! even if their verdict was no change. A cue is an address, not an observation.
use super::{metadata, VISIBLE};
use crate::neural::{context::Snapshot, failure, provider_evidence, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "by", rename_all = "snake_case")]
enum Channel {
    OwnTool {
        tool: String,
    },
    Correspondent {
        author: String,
        event_kind: String,
        persona_authored: bool,
    },
}

fn channel(
    owner: &str,
    kind: &str,
    author: &str,
    target: &str,
    invoked: bool,
    tool: Option<&str>,
) -> Option<Channel> {
    if kind == "tool_result" {
        let tool = tool?;
        return (invoked && author == owner && target == owner)
            .then(|| Channel::OwnTool { tool: tool.into() });
    }
    ((target == owner || target == "*") && !(invoked && author == owner)).then(|| {
        Channel::Correspondent {
            author: author.into(),
            event_kind: kind.into(),
            persona_authored: invoked,
        }
    })
}

#[derive(Clone, Serialize)]
struct Pair {
    reference: String,
    considered: String,
}

#[derive(Default)]
struct Occasions {
    arrivals: BTreeSet<i64>,
    count: usize,
    first: Option<Pair>,
    latest: Option<Pair>,
}

/// Join a citation to its exact event and to the recorded owner/input/output of
/// the decision that considered it. No developmental question or edit required.
fn pairs() -> String {
    format!("FROM invocations i JOIN objects reply ON reply.digest=i.response
        CROSS JOIN json_each(CAST(reply.bytes AS TEXT),'$.considered_experience') cite
        JOIN events e ON e.seq=CAST(substr(cite.value,length('experience:event-')+1) AS INTEGER)
            AND cite.value='experience:event-'||e.seq||'@1'
        JOIN objects envelope ON envelope.digest=e.digest
        JOIN heads u ON u.kind='character_use' AND u.id=i.id JOIN objects binding ON binding.digest=u.digest
        WHERE i.actor=?1 AND i.state='accepted' AND {VISIBLE}
            AND json_extract(CAST(binding.bytes AS TEXT),'$.response')='object:'||i.response
            AND json_extract(CAST(binding.bytes AS TEXT),'$.snapshot')='object:'||i.snapshot
            AND json_extract(CAST(binding.bytes AS TEXT),'$.owner')=?1")
}

/// Only accepted, active choices by this owner, and only exact event citations
/// in their considered_experience. Inspect only the earliest and latest matching
/// choices for the automatic cards, not a lifetime of bodies. Each channel is descriptive:
/// the same tool or correspondent can mean quite different things on two occasions.
pub(in crate::neural) fn cues(
    db: &Connection,
    owner: &str,
    work: &str,
    events: &[Value],
) -> Result<Vec<Value>> {
    let mut matches: BTreeMap<Channel, Occasions> = BTreeMap::new();
    for e in events {
        let (Some(kind), Some(author), Some(target), Some(seq)) = (
            e["kind"].as_str(),
            e["actor"].as_str(),
            e["target"].as_str(),
            e["event"].as_i64(),
        ) else {
            continue;
        };
        if let Some(key) = channel(
            owner,
            kind,
            author,
            target,
            e["invocation"].is_string(),
            e["body"]["tool"].as_str(),
        ) {
            matches.entry(key).or_default().arrivals.insert(seq);
        }
    }
    if matches.is_empty() {
        return Ok(vec![]);
    }
    // Read the persona's own accepted declarations, not keywords in notes,
    // proposals, assessment samples or another person's copy of an experience.
    let sql = format!(
        "SELECT DISTINCT i.rowid,i.id,e.seq,e.kind,e.actor,e.target,e.invocation IS NOT NULL,
        CASE WHEN e.kind='tool_result' THEN json_extract(CAST(envelope.bytes AS TEXT),'$.tool') END
        {} ORDER BY i.rowid,e.seq",
        pairs()
    );
    let mut query = db.prepare_cached(&sql)?;
    let mut rows = query.query(params![owner])?;
    while let Some(row) = rows.next()? {
        let key = channel(
            owner,
            row.get_ref(3)?.as_str()?,
            row.get_ref(4)?.as_str()?,
            row.get_ref(5)?.as_str()?,
            row.get(6)?,
            row.get::<_, Option<String>>(7)?.as_deref(),
        );
        let Some(group) = key.and_then(|k| matches.get_mut(&k)) else {
            continue;
        };
        let seq: i64 = row.get(2)?;
        if group.arrivals.contains(&seq) {
            continue;
        }
        let call: String = row.get(1)?;
        let pair = Pair {
            reference: format!("experience:{call}@1"),
            considered: format!("experience:event-{seq}@1"),
        };
        group.first.get_or_insert_with(|| pair.clone());
        group.latest = Some(pair);
        group.count += 1;
    }
    // Failure to reconstruct optional history must not invent a past choice
    // or block unrelated present work. A deliberate read still fails closed.
    let offered = |pair: &Pair| {
        card(db, owner, work, pair).unwrap_or_else(|_| json!({
        "status":"unavailable","reason":"The historical choice and observation could not be verified. No facts from that pair are supplied."}))
    };
    let mut cues = Vec::new();
    for (channel, group) in matches {
        let Some(first) = group.first else { continue };
        let mut cue = json!({"match":channel,"arrivals":group.arrivals.iter().map(|n|format!("experience:event-{n}@1")).collect::<Vec<_>>(),
            "prior_decision_event_pairs":group.count,"first":first,"first_choice":offered(&first)});
        if let Some(latest) = group.latest.filter(|_| group.count > 1) {
            cue["latest_choice"] = offered(&latest);
            cue["latest"] = json!(latest);
        }
        cues.push(cue);
    }
    Ok(cues)
}

/// Show facts of the earlier choice immediately. Do not automatically supply
/// its brief, event text, stdout, tool arguments, private rationale from another
/// activity, images, numerical hypothesis, or any later outcome. These are not
/// full observations for a trial. A tool call is a choice, not a successful effect.
fn card(db: &Connection, owner: &str, work: &str, pair: &Pair) -> Result<Value> {
    let prior = material(db, owner, &pair.reference, &pair.considered, false)?;
    let choice = &prior["choice_then"];
    let observation = &prior["observation_then"];
    let tools = choice["operations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|op| &op["tool"])
        .collect::<Vec<_>>();
    let mut card = json!({"choice_work":prior["work"],"appraisal_used":choice["basis"],"work_status":choice["judgment"]["status"],
        "tools_chosen":tools,"maintenance":choice["maintenance"]["kind"],"continuation":choice["continuation"]["kind"]});
    if prior["work"] == work
        && (observation["work"] == work || observation["record"]["work"] == work)
    {
        if let Some(reason) = choice["maintenance"]["reason"].as_str() {
            card["own_reason_then"] = json!(reason);
        }
    }
    let e = &observation["event"];
    if e["kind"] == "tool_result" && e["body"]["tool"] == "command.local" {
        let run = &e["supplied_result"]["result"];
        if run["format"] == "local-project-run-v1" {
            card["process_observed_then"] = json!({"started":run["process_started"].as_bool(),"exit_code":run["exit_code"].as_i64(),
                "signal":run["signal"].as_i64(),"timed_out":run["timed_out"].as_bool(),"cancelled":run["cancelled"].as_bool()});
        }
    }
    Ok(card)
}

/// Deliberate search, unlike automatic choice cards. Match the actual
/// historical observation, not the later interpretation or an unread stream.
/// Only one source snapshot and one page of result addresses need stay in memory.
pub(in crate::neural) fn search(
    db: &Connection,
    owner: &str,
    query: &crate::neural::tools::ExperienceQuery,
) -> Result<Value> {
    let regex = query.expression()?;
    let literal = query.query.to_lowercase();
    fn find(value: &Value, test: &impl Fn(&str) -> bool) -> bool {
        match value {
            Value::String(s) => test(s),
            Value::Array(a) => a.iter().any(|v| find(v, test)),
            Value::Object(o) => o.values().any(|v| find(v, test)),
            _ => false,
        }
    }
    let sql=format!("SELECT DISTINCT i.rowid,i.id,i.snapshot,i.work,e.seq,e.work,e.kind,e.actor {} AND (?2 IS NULL OR i.work=?2) AND (?3 IS NULL OR e.actor=?3) AND (?4 IS NULL OR e.kind=?4) ORDER BY i.rowid DESC,e.seq DESC", pairs());
    let mut q = db.prepare_cached(&sql)?;
    let mut rows = q.query(params![owner, query.work, query.author, query.kind])?;
    let mut previous = None::<Snapshot>;
    let mut items = Vec::new();
    let mut total = 0;
    while let Some(row) = rows.next()? {
        let call = row.get_ref(1)?.as_str()?;
        let work = row.get_ref(3)?.as_str()?;
        if previous.as_ref().is_none_or(|s| s.invocation != call) {
            previous = Some(crate::neural::store::object(db, row.get_ref(2)?.as_str()?)?);
        }
        let s = previous.as_ref().expect("read above");
        if s.actor != owner || s.invocation != call || s.work != work {
            return Err(failure("Consideration has a different source identity"));
        }
        if s.model.is_none() || !s.data["context_recovery"].is_null() {
            continue;
        }
        let event = format!("experience:event-{}@1", row.get::<_, i64>(4)?);
        let Some(observed) = previous_observation(s, &event) else {
            continue;
        };
        if !literal.is_empty() || regex.is_some() {
            let body = observed.get("event").unwrap_or(&observed["record"]);
            let found = find(body, &|s| {
                regex
                    .as_ref()
                    .map_or_else(|| s.to_lowercase().contains(&literal), |r| r.is_match(s))
            });
            if !found {
                continue;
            }
        }
        if total >= query.offset && items.len() < query.page_size {
            items.push(json!({"selection":{"reference":format!("experience:{call}@1"),"considered":event},"work":work,
                "event_work":row.get_ref(5)?.as_str()?,"kind":row.get_ref(6)?.as_str()?,"author":row.get_ref(7)?.as_str()?}));
        }
        total += 1;
    }
    Ok(
        json!({"items":items,"total":total,"offset":query.offset,"body_supplied":false,
        "scope":"Event observations you explicitly cited for maintenance. Select a pair to read the earlier observation and your choice together. The work filter selects the considering decision; author and kind select its cited event."}),
    )
}

pub(in crate::neural) fn previous_observation(input: &Snapshot, event: &str) -> Option<Value> {
    let events = || input.data["new_events"].as_array().into_iter().flatten();
    if let Some(e) = events().find(|e| e["experience"] == event) {
        return Some(
            json!({"via":"inbox","at_decision":input.invocation,"work":input.work,"event":e}),
        );
    }
    if let Some(record) = whole_read(input, event) {
        // An experience read may have supplied only the event envelope, not the
        // underlying command streams. Never open those objects on its behalf.
        return Some(
            json!({"via":"experience_read","at_decision":input.invocation,"record":record}),
        );
    }
    input.data["selected_resources"].as_array().into_iter().flatten()
        .find(|v| authentic_selection(input, v) && v["considered"] == event)
        .and_then(|v| v.get("observation_then"))
        .cloned()
        .or_else(|| crate::neural::development::deliberation::observed(input, event).cloned())
}

/// Did this input actually contain this particular event envelope? A reference
/// on an application is not its outcome. Reading an envelope is not reading
/// every file or command stream it names.
pub(in crate::neural) fn contains_event(s: &Snapshot, record: &Value) -> bool {
    let Some(reference) = record["reference"].as_str() else { return false };
    if !s.data["context_recovery"].is_null() { return false; }
    let Some(observed) = previous_observation(s, reference) else { return false };
    if let Some(read) = observed.get("record") {
        return read == record;
    }
    let e = &observed["event"];
    observed["work"] == record["work"]
        && e["experience"] == reference
        && e["event"].as_i64().is_some_and(|n| reference == format!("experience:event-{n}@1"))
        && e["created"] == record["time"]
        && e["digest"].as_str().is_some_and(|d| record["source"] == format!("object:{d}"))
        && ["kind", "actor", "target", "invocation", "body"].iter().all(|k| e[*k] == record[*k])
}

/// The record supplied to this input, not a discovery pointer, excerpt or
/// working note. A full experience read can come from selection or a tool.
pub(super) fn whole_read<'a>(input: &'a Snapshot, reference: &str) -> Option<&'a Value> {
    let reads = input.data["new_events"].as_array().into_iter().flatten()
        .filter(|e| e["kind"] == "tool_result"
            && matches!(e["body"]["tool"].as_str(), Some("asset.read" | "persona.read")))
        .filter_map(|e| e.get("supplied_result"));
    input.data["selected_resources"].as_array().into_iter().flatten().chain(reads)
        .find(|v| v["reference"] == reference && v["record"]["reference"] == reference
            && v["record"]["body"].is_object())
        .map(|v| &v["record"])
}

/// A pointer in a cue, arbitrary JSON in an asset, or a recovery card is not a
/// supplied pair. A historical working choice is not its whole input either.
pub(in crate::neural) fn authentic_selection(input: &Snapshot, item: &Value) -> bool {
    input.development_version >= 7
        && input.data["context_recovery"].is_null()
        && item["view"] == "own_considered_experience"
        && item["owner"] == input.actor
        && item["observation_then"].is_object()
        && item["choice_then"].is_object()
        && input.data["participation"]["selection"]["references"]
            .as_array()
            .is_some_and(|selections| {
                selections.iter().any(|s| {
                    s["reference"] == item["reference"] && s["considered"] == item["considered"]
                })
            })
}

pub(in crate::neural) fn supplied(input: &Snapshot, reference: &str) -> bool {
    input.data["selected_resources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| {
            authentic_selection(input, item)
                && (item["reference"] == reference
                    || item["considered"] == reference
                    || (input.development_version >= 8
                        && item["action_before_observation"]["owner"] == input.actor
                        && item["action_before_observation"]["decision"] == reference
                        && item["action_before_observation"]["outcome"] == item["considered"]
                        && item["action_before_observation"]["performed"].is_object()))
        })
}

/// A current-choice alias can only refer to a pair this very response authors.
/// Validation happens inside adoption; the later read checks the accepted input.
pub(in crate::neural) fn current(
    input: &Snapshot,
    considered: &[String],
    event: &str,
) -> Result<String> {
    if input.development_version < 7
        || input.model.is_none()
        || !input.data["context_recovery"].is_null()
        || !event.starts_with("experience:event-")
        || !considered.iter().any(|e| e == event)
        || previous_observation(input, event).is_none()
    {
        return Err(failure("To keep a considered observation from this decision, cite its observed event in considered_experience"));
    }
    Ok(format!("experience:{}@1", input.invocation))
}

/// Deliberate retrieval of one pair. Reconstruct the accepted response and
/// immutable input; an index entry cannot grant read or update authority.
pub(in crate::neural) fn read(
    db: &Connection,
    owner: &str,
    decision: &str,
    event: &str,
) -> Result<Value> {
    material(db, owner, decision, event, true)
}

fn material(
    db: &Connection,
    owner: &str,
    decision: &str,
    event: &str,
    verify_prompt: bool,
) -> Result<Value> {
    let choice = metadata(db, Some(owner), decision)?;
    let call = choice["invocation"]
        .as_str()
        .filter(|_| choice["kind"] == "accepted_decision")
        .ok_or_else(|| failure("Recall requires your own accepted experience:call-ID@1"))?;
    if !event.starts_with("experience:event-") {
        return Err(failure(
            "Choose one exact event you considered in that decision",
        ));
    }
    let original = metadata(db, Some(owner), event)?;
    let accepted = provider_evidence::accepted(db, owner, call, verify_prompt)?;
    let s = &accepted.snapshot;
    let r = &accepted.response;
    if s.model.is_none()
        || !s.data["context_recovery"].is_null()
        || !r["considered_experience"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == event))
    {
        return Err(failure(
            "That ordinary decision did not cite this event in considered_experience",
        ));
    }
    let observation = previous_observation(s, event)
        .ok_or_else(|| failure("This event was not an observed experience in the cited input"))?;
    // Do not attach an authentic past observation to a replaced journal row.
    // A deliberate event read and an inbox projection use different envelopes.
    let same_event = if let Some(e) = observation.get("event") {
        observation["work"] == original["work"]
            && e["experience"] == original["reference"]
            && e["digest"]
                .as_str()
                .is_some_and(|d| original["source"] == format!("object:{d}"))
            && e["created"] == original["time"]
            && ["actor", "target", "kind", "invocation"]
                .iter()
                .all(|k| e[k] == original[k])
    } else {
        let record = &observation["record"];
        [
            "reference",
            "source",
            "work",
            "actor",
            "target",
            "kind",
            "time",
            "invocation",
        ]
        .iter()
        .all(|k| record[k] == original[k])
    };
    if !same_event {
        return Err(failure(
            "The event does not match the observation in the accepted decision",
        ));
    }
    let operations = r["operations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|op| json!({"id":op["id"],"tool":op["tool"]}))
        .collect::<Vec<_>>();
    let (basis, evaluation) = if s.data["last_contextual_evaluation"]["outputs"].is_object() {
        ("contextual", &s.data["last_contextual_evaluation"])
    } else {
        ("baseline", &s.data["baseline_character"])
    };
    let mut view = json!({"reference":decision,"considered":event,"owner":owner,"work":s.work,"view":"own_considered_experience",
        "source_response":choice["source"],"response_binding":accepted.binding,
        "observation_then":observation,"choice_then":{"judgment":r["work"],"operations":operations,
        "continuation":r["continuation"],"maintenance":r["persona_maintenance"],"package":s.model,"epoch":s.epoch,
        "basis":basis,"evaluation":evaluation["reference"]},
        "scope":"The earlier observation as supplied in JSON and your choice, not a current result or its later consequence. Referenced images and files are not attached by recall. Read the decision separately for later outcome addresses."});
    if verify_prompt {
        // Only on deliberate retrieval: do not automatically replay earlier prose in other work.
        match super::action::origin(
            db,
            owner,
            original["work"]
                .as_str()
                .ok_or_else(|| failure("Missing event work"))?,
            event,
        ) {
            Ok(Some(action)) => view["action_before_observation"] = action,
            Ok(None) => {}
            Err(_) => view["action_origin_status"] = json!("unavailable"),
        }
    }
    Ok(view)
}
