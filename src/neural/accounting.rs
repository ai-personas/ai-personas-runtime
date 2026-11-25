//! Operator measurements from immutable requests, provider receipts and local tool results.
mod receipts;
use super::{context, failure, store, Result};
use receipts::Call;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const USAGE: &[&str] = &[
    "input_tokens",
    "cached_input_tokens",
    "cache_write_input_tokens",
    "cache_write_30m_input_tokens",
    "cache_write_12h_input_tokens",
    "output_tokens",
    "total_tokens",
    "actual_cost_microusd",
];

#[derive(Default)]
struct Measure {
    sum: u64,
    reported: usize,
    missing: usize,
}

impl Measure {
    fn observe(&mut self, amount: Option<u64>) {
        if let Some(n) = amount {
            self.sum += n;
            self.reported += 1;
        } else {
            self.missing += 1;
        }
    }
    fn report(&self) -> Value {
        json!({"sum":(self.reported>0).then_some(self.sum),"reported":self.reported,
            "missing":self.missing,"complete":self.reported>0 && self.missing==0})
    }
}

pub(super) fn usage_totals<'a>(
    records: impl IntoIterator<Item = &'a Value>,
) -> BTreeMap<&'static str, Value> {
    let mut totals = BTreeMap::<_, Measure>::new();
    for &field in USAGE {
        totals.insert(field, Measure::default());
    }
    for usage in records {
        for (field, measure) in totals.iter_mut() {
            measure.observe(usage[*field].as_u64());
        }
    }
    totals
        .into_iter()
        .map(|(key, measure)| (key, measure.report()))
        .collect()
}

#[derive(Default)]
struct Coverage {
    path: Option<String>,
    supplied: u64,
    appearances: usize,
    whole: usize,
    ranges: Vec<(u64, u64)>,
}

impl Coverage {
    fn add(&mut self, start: u64, end: u64, whole: bool) {
        self.supplied += end - start;
        self.appearances += 1;
        self.whole += usize::from(whole);
        self.ranges.push((start, end));
    }
    fn report(mut self, reference: String) -> Value {
        self.ranges.sort_unstable();
        self.ranges.dedup();
        let mut unique_bytes = 0;
        let mut covered_to = 0;
        for &(start, end) in &self.ranges {
            unique_bytes += end.saturating_sub(start.max(covered_to));
            covered_to = covered_to.max(end);
        }
        json!({"reference":reference,"path":self.path,"appearances":self.appearances,"whole":self.whole,
            "distinct_ranges":self.ranges.len(),"text_bytes_with_repeats":self.supplied,
            "unique_covered_bytes":unique_bytes})
    }
}

struct Input {
    data: Value,
    digest: String,
    images: Vec<(String, String)>,
    mode: &'static str,
}

fn frozen_input(db: &Connection, call: &Call) -> Result<Input> {
    let mut value: Value = store::object(db, &call.snapshot)?;
    if value["purpose"] == "isolated_character_assessment" {
        let prompt = value["prompt"]
            .as_str()
            .ok_or_else(|| failure("Assessment prompt is missing"))?;
        if !prompt.ends_with(&serde_json::to_string(&value["data"])?) {
            return Err(failure(
                "Retained assessment data differs from its frozen prompt",
            ));
        }
        let digest = crate::store::sha256_bytes(prompt.as_bytes());
        let images = value
            .get("image_blobs")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        return Ok(Input {
            data: value["data"].take(),
            digest,
            images,
            mode: "assessment",
        });
    }
    let snapshot: context::Snapshot = serde_json::from_value(value)?;
    if snapshot.invocation != call.id || snapshot.work != call.work || snapshot.actor != call.actor
    {
        return Err(failure(
            "The indexed input belongs to another call, activity or persona",
        ));
    }
    let digest = crate::store::sha256_bytes(context::prompt(db, &snapshot)?.as_bytes());
    let mode = if snapshot.model.is_none() {
        "initialization"
    } else {
        "decision"
    };
    Ok(Input {
        data: snapshot.data,
        images: snapshot.image_blobs,
        digest,
        mode,
    })
}

fn resource_row(
    value: &Value,
    via: &str,
    metadata: Option<&Value>,
    coverage: &mut BTreeMap<String, Coverage>,
) -> Result<Option<Value>> {
    let Some(reference) = value["reference"].as_str() else {
        return Ok(None);
    };
    let mut row = json!({"reference":reference,"via":via});
    if let Some(text) = value["text"].as_str() {
        let start = value["range"][0]
            .as_u64()
            .ok_or_else(|| failure("Missing source range"))?;
        let end = value["range"][1]
            .as_u64()
            .ok_or_else(|| failure("Missing source range"))?;
        if end.checked_sub(start) != Some(text.len() as u64) {
            return Err(failure(
                "Frozen source range differs from the supplied UTF-8 bytes",
            ));
        }
        let whole = start == 0 && Some(end) == value["bytes"].as_u64();
        let covered = coverage.entry(reference.to_owned()).or_default();
        if covered.path.is_none() {
            covered.path = metadata
                .or_else(|| value.get("metadata"))
                .and_then(|m| m["path"].as_str())
                .map(str::to_owned);
        }
        covered.add(start, end, whole);
        row["kind"] = json!("text");
        row["range"] = json!([start, end]);
        row["text_bytes"] = json!(text.len());
        row["whole"] = json!(whole);
    } else if value.get("record").is_some() {
        row["kind"] = json!("structured_record");
        row["record_json_bytes"] = json!(serde_json::to_vec(&value["record"])?.len());
    } else if value["image_blob"].is_string() {
        row["kind"] = json!("image_reference");
    } else if value["binary"] == true {
        row["kind"] = json!("binary_metadata");
    } else {
        return Ok(None);
    }
    Ok(Some(row))
}

#[derive(Default, Serialize)]
struct ToolUse {
    results: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    scanned_text_bytes: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scanned_revisions: Option<Value>,
}

fn tool_results(db: &Connection, work: &str) -> Result<BTreeMap<String, ToolUse>> {
    let mut stmt = db.prepare(
        "SELECT o.bytes FROM events e JOIN objects o ON e.digest=o.digest
        WHERE e.work=?1 AND e.kind='tool_result' ORDER BY e.seq",
    )?;
    let rows = stmt.query_map([work], |row| row.get::<_, Vec<u8>>(0))?;
    let mut counts = BTreeMap::<String, ToolUse>::new();
    let mut scans = Measure::default();
    let mut revisions = Measure::default();
    for bytes in rows {
        let event: Value = serde_json::from_slice(&bytes?)?;
        let Some(tool) = event["tool"].as_str() else {
            continue;
        };
        counts.entry(tool.into()).or_default().results += 1;
        if matches!(tool, "asset.find" | "asset.search") {
            let result = event["result"]
                .as_str()
                .and_then(|r| r.strip_prefix("object:"))
                .map(|digest| store::object::<Value>(db, digest))
                .transpose()?
                .unwrap_or(Value::Null);
            if tool == "asset.find" {
                scans.observe(result["scanned_text_bytes"].as_u64());
            } else {
                revisions.observe(result["scanned_revisions"].as_u64());
            }
        }
    }
    if let Some(search) = counts.get_mut("asset.find") {
        search.scanned_text_bytes = Some(scans.report());
    }
    if let Some(search) = counts.get_mut("asset.search") {
        search.scanned_revisions = Some(revisions.report());
    }
    Ok(counts)
}

/// Group actual accepted-call reports, not the personas' current settings. Calls
/// are in preparation order: keep the last matching receipt as a drill-down.
fn reasoning_receipts(calls: &[Value]) -> Vec<Value> {
    let mut groups = BTreeMap::new();
    for call in calls
        .iter()
        .filter(|c| c["state"] == "accepted" && c["input_evidence"] == "verified_completed_input")
    {
        let key = (
            call["actor"].as_str(),
            call["transport_profile"].as_str(),
            call["reported_model"].as_str(),
            call["reported_effort"].as_str(),
        );
        let entry = groups.entry(key).or_insert((0usize, call));
        entry.0 += 1;
        entry.1 = call;
    }
    groups.into_iter().map(|((actor, transport, model, effort), (count, last))| json!({
        "actor":actor, "transport_profile":transport, "reported_model":model, "reported_effort":effort,
        "calls":count, "last_invocation":last["id"], "last_receipt":last["receipt"],
    })).collect()
}

pub(super) fn previous_usage(db: &Connection, actor: &str) -> Result<Option<Value>> {
    Call::previous_usage(db, actor)
}

/// All-society audit and activity views must attribute the same provider report
/// (or the same absence) to the same invocation.
pub(super) fn workspace_calls(db: &Connection) -> Result<Vec<Value>> {
    Ok(Call::list(db, None)?
        .into_iter()
        .map(|call| {
            let (mut row, _) = call.measure(db);
            row["work"] = json!(call.work);
            row
        })
        .collect())
}

#[derive(Default)]
struct Supplied {
    sources: Vec<Value>,
    images: Vec<Value>,
    coverage: BTreeMap<String, Coverage>,
    components: BTreeMap<String, u64>,
    image_bytes: u64,
}

/// Accumulate a call in isolation. A failed reconstruction must not contribute
/// partial source ranges or discard independently attributable measurements.
fn supplied(
    db: &Connection,
    input: Input,
    attempt: &Value,
    out: &mut Supplied,
) -> Result<&'static str> {
    if attempt["response_completed"] != true {
        return Ok("completion_unconfirmed");
    }
    let audit = &attempt["audit"];
    if audit["prompt_sha256"] != input.digest {
        return Ok("prompt_mismatch");
    }
    let expected = context::retained_images(db, &input.images)?
        .iter()
        .map(crate::provider::ImageInput::audit)
        .collect::<Vec<_>>();
    if audit
        .get("image_inputs")
        .cloned()
        .unwrap_or_else(|| json!([]))
        != json!(expected)
    {
        return Ok("image_mismatch");
    }
    let data = &input.data;
    for source in context::supplied_resources(data) {
        out.sources.extend(resource_row(
            source.value,
            source.via,
            source.shared_metadata,
            &mut out.coverage,
        )?);
    }
    if input.mode == "assessment" {
        for value in data["resources"].as_array().into_iter().flatten() {
            out.sources
                .extend(resource_row(value, "assessment", None, &mut out.coverage)?);
        }
    }
    for (digest, mime) in input.images {
        let bytes: i64 = db.query_row(
            "SELECT length(bytes) FROM objects WHERE digest=?1",
            [&digest],
            |r| r.get(0),
        )?;
        out.image_bytes += u64::try_from(bytes)?;
        out.images
            .push(json!({"reference":format!("object:{digest}"),"mime":mime,"bytes":bytes}));
    }
    for key in [
        "available_tools",
        "selected_resources",
        "new_events",
        "baseline_character",
        "last_contextual_evaluation",
        "resources",
    ] {
        if let Some(value) = data.get(key) {
            out.components
                .insert(key.to_owned(), serde_json::to_vec(value)?.len() as u64);
        }
    }
    Ok("verified_completed_input")
}

/// One activity, including rejected decisions and assessment arms. Only receipt-bound
/// completed inputs contribute to the observed source-body and context figures.
pub fn activity(db: &Connection, work: &str) -> Result<Value> {
    let activity = store::required(db, "work", work)?;
    let mut measures = BTreeMap::<&str, Measure>::new();
    for key in [
        "prompt_bytes",
        "request_bytes",
        "response_schema_bytes",
        "elapsed_ms",
    ] {
        measures.insert(key, Measure::default());
    }
    let mut calls = Vec::new();
    let mut states = BTreeMap::<String, usize>::new();
    let mut actors = BTreeMap::<String, usize>::new();
    let mut coverage = BTreeMap::<String, Coverage>::new();
    let mut component_bytes = BTreeMap::<String, u64>::new();
    let mut verified = 0;
    let mut raw_image_bytes = 0u64;
    let mut first: Option<i64> = None;
    let mut last: Option<i64> = None;
    for call in Call::list(db, Some(work))? {
        first.get_or_insert(call.created);
        last = Some(last.map_or(call.updated, |n| n.max(call.updated)));
        *states.entry(call.state.clone()).or_default() += 1;
        *actors.entry(call.actor.clone()).or_default() += 1;
        let (mut row, attempt) = call.measure(db);
        let audit = &attempt["audit"];
        for (field, source) in [
            ("prompt_bytes", &audit["prompt_bytes"]),
            ("request_bytes", &audit["request_bytes"]),
            ("response_schema_bytes", &audit["schema_bytes"]),
            ("elapsed_ms", &attempt["elapsed_ms"]),
        ] {
            measures.get_mut(field).unwrap().observe(source.as_u64());
        }
        row["created"] = json!(call.created);
        row["updated"] = json!(call.updated);
        row["transport_profile"] = audit["transport_profile"].clone();
        let mut observed = Supplied::default();
        let check = frozen_input(db, &call).and_then(|input| {
            row["mode"] = json!(input.mode);
            if row["receipt_problem"].is_string() {
                Ok("receipt_unavailable")
            } else {
                supplied(db, input, &attempt, &mut observed)
            }
        });
        let evidence = match check {
            Ok(label) => label,
            Err(e) => {
                row["input_problem"] = json!(e.to_string());
                "input_unavailable"
            }
        };
        row["input_evidence"] = json!(evidence);
        if evidence == "verified_completed_input" {
            verified += 1;
            raw_image_bytes += observed.image_bytes;
            for (key, bytes) in observed.components {
                *component_bytes.entry(key).or_default() += bytes;
            }
            for (reference, local) in observed.coverage {
                let total = coverage.entry(reference).or_default();
                total.path = total.path.take().or(local.path);
                total.appearances += local.appearances;
                total.supplied += local.supplied;
                total.whole += local.whole;
                total.ranges.extend(local.ranges);
            }
            row["sources"] = json!(observed.sources);
            row["images"] = json!(observed.images);
        } else {
            row["sources"] = json!([]);
            row["images"] = json!([]);
        }
        calls.push(row);
    }
    let count_rows = |sql: &str| -> Result<BTreeMap<String, i64>> {
        let mut statement = db.prepare(sql)?;
        let counts = statement
            .query_map([work], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(counts)
    };
    let events = count_rows("SELECT kind,count(*) FROM events WHERE work=?1 GROUP BY kind")?;
    let effects = count_rows("SELECT state,count(*) FROM outbox WHERE work=?1 GROUP BY state")?;
    let revisions=count_rows("SELECT v.kind,count(*) FROM versions v JOIN invocations i ON i.id=v.invocation WHERE i.work=?1 GROUP BY v.kind")?;
    let mut totals = measures
        .into_iter()
        .map(|(key, value)| (key, value.report()))
        .collect::<BTreeMap<_, _>>();
    totals.extend(usage_totals(calls.iter().map(|c| &c["usage"])));
    let sources = coverage
        .into_iter()
        .map(|(reference, ranges)| ranges.report(reference))
        .collect::<Vec<_>>();
    Ok(
        json!({"work":work,"work_reference":activity.reference.handle(),"call_count":calls.len(),"states":states,"actors":actors,
        "verified_completed_inputs":verified,"calls_without_verified_input":calls.len()-verified,
        "first_call_created":first,"last_call_updated":last,"recorded_call_span_seconds":last.zip(first).map(|(b,a)|b-a),
        "events_by_kind":events,"effects_by_state":effects,"decision_revisions_by_kind":revisions,
        "totals":totals,"tool_results":tool_results(db,work)?,"verified_component_json_bytes":component_bytes,
        "verified_image_bytes_with_repeats":raw_image_bytes,"text_sources":sources,"reasoning_receipts":reasoning_receipts(&calls),"calls":calls}),
    )
}
