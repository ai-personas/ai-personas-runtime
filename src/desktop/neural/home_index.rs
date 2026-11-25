use super::{activity_title, inbox, s, BTreeMap, Value};
use std::cmp::Reverse;

/// A current self-declaration, not a persona membership or an outside-step claim.
/// Keep the operator's literal name; display search may fold it, identity must not.
pub(super) struct HumanRole {
    pub operator: String,
    pub label: &'static str,
    pub note: String,
    pub reference: String,
    pub brief: String,
}

pub(super) struct Activity {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub phase: String,
    pub members: Vec<String>,
    pub brief: String,
    pub roles: Vec<HumanRole>,
    pub deliveries: Vec<usize>,
    pub latest_utc: Option<String>,
    pub open_requests: usize,
}

/// Index the committed snapshot once. Repainting a streaming call only reads
/// these lists and the small changing set of active calls and tool effects.
#[derive(Default)]
pub(super) struct Index {
    pub generation: u64,
    pub latest_asset_receipt: i64,
    pub activities: Vec<Activity>,
    pub participations: BTreeMap<String, Vec<usize>>,
    pub requests: Vec<usize>,
    pub deliveries: Vec<usize>,
    positions: BTreeMap<String, usize>,
    last_event: BTreeMap<String, (i64, Option<String>)>,
    tool_results: BTreeMap<String, BTreeMap<String, i64>>,
    issued: BTreeMap<String, i64>,
    receiving_checks: BTreeMap<String, Vec<i64>>,
    request_events: BTreeMap<String, i64>,
    request_origins: BTreeMap<String, i64>,
}

impl Index {
    pub(super) fn delivery_issue(&self, reference: &str) -> Option<i64> {
        self.issued.get(reference).copied()
    }

    pub(super) fn receiving_checks(&self, delivery: &str) -> &[i64] {
        self.receiving_checks.get(delivery).map(Vec::as_slice).unwrap_or_default()
    }

    pub(super) fn request_event(&self, reference: &str) -> Option<i64> {
        self.request_events.get(reference).copied()
    }

    pub(super) fn request_origin(&self, reference: &str) -> Option<i64> {
        self.request_origins.get(reference).copied()
    }

    pub(super) fn latest_event(&self, work: &str) -> Option<i64> {
        self.last_event.get(work).map(|(seq, _)| *seq)
    }

    pub(super) fn latest_tool_event(&self, work: &str, person: &str) -> Option<i64> {
        self.tool_results.get(work)?.get(person).copied()
    }

    pub fn activity(&self, id: &str) -> Option<&Activity> {
        self.activities.get(*self.positions.get(id)?)
    }

    pub fn refresh(&mut self, status: &Value, events: &[Value]) {
        self.generation = self.generation.wrapping_add(1);
        let rows = |key: &str| status[key].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for event in events {
            let (Some(work), Some(seq)) = (event["work"].as_str(), event["event"].as_i64()) else {
                continue;
            };
            self.last_event.insert(
                work.to_owned(),
                (seq, event["created_utc"].as_str().map(str::to_owned)),
            );
            let body = &event["body"];
            // Synchronous operation receipts declare whether the decision
            // initialized the persona. External effects require an ordinary
            // decision and keep their effect ID instead of that flag.
            if event["kind"] == "tool_result"
                && body["context"] == "complete"
                && (body["bootstrap"] == false
                    || (body["bootstrap"].is_null() && body["effect"].is_string()))
                && event["actor"] == event["target"]
                && event["invocation"].is_string()
                && body["tool"].is_string()
                && event["tool_outcome"]["result"].is_string()
            {
                if let Some(person) = event["actor"].as_str() {
                    self.tool_results
                        .entry(work.into())
                        .or_default()
                        .insert(person.into(), seq);
                }
            }
            if event["kind"] == "tool_result" && matches!(
                body["tool"].as_str(),
                Some("asset.save" | "asset.register" | "asset.import" | "asset.organize")
            ) {
                self.latest_asset_receipt = self.latest_asset_receipt.max(seq);
            }
            if event["kind"] == "delivery" {
                if let Some(reference) = body["reference"].as_str() {
                    self.issued.insert(reference.to_owned(), seq);
                }
            }
            if event["kind"] == "delivery_check" {
                if let Some(reference) = body["delivery"].as_str() {
                    self.receiving_checks.entry(reference.into()).or_default().push(seq);
                }
            }
            if matches!(s(event, "kind"), "human_request" | "human_request_packet") && event["target"] == "user" {
                if let Some(reference) = body["reference"].as_str() {
                    self.request_origins.entry(reference.to_owned()).or_insert(seq);
                }
            }
            if let Some(reference) = body["human_request"].as_str().or_else(|| {
                body["reference"]
                    .as_str()
                    .filter(|r| r.starts_with("human_request:"))
            }) {
                self.request_events.insert(reference.to_owned(), seq);
            }
        }
        let parts = rows("participation");
        self.participations.clear();
        let mut members = BTreeMap::<&str, Vec<String>>::new();
        for (index, part) in parts.iter().enumerate() {
            let body = &part["body"];
            let (person, work) = (s(body, "persona"), s(body, "work"));
            self.participations
                .entry(person.to_owned())
                .or_default()
                .push(index);
            members.entry(work).or_default().push(person.to_owned());
        }
        let recency = |work: &str| self.last_event.get(work).map(|(seq, _)| *seq);
        for list in self.participations.values_mut() {
            list.sort_by_key(|i| Reverse(parts[*i]["write_order"].as_i64()));
        }
        let requests = rows("human_requests");
        let mut counts = BTreeMap::<&str, usize>::new();
        self.requests = requests
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let body = &r["body"];
                if !matches!(s(body, "state"), "open" | "in_progress") {
                    return None;
                }
                *counts.entry(s(body, "work")).or_default() += 1;
                Some(i)
            })
            .collect();
        self.requests.sort_by_cached_key(|i| {
            let r = &requests[*i];
            Reverse((
                r["body"]["updated"].as_i64(),
                self.request_events.get(&inbox::handle(r)).copied(),
            ))
        });
        let deliveries = rows("deliveries");
        self.deliveries = (0..deliveries.len()).collect();
        self.deliveries.sort_by_cached_key(|i| {
            Reverse(self.issued.get(&inbox::handle(&deliveries[*i])).copied())
        });
        let mut by_work = BTreeMap::<&str, Vec<usize>>::new();
        for &i in &self.deliveries {
            by_work
                .entry(s(&deliveries[i]["body"], "work"))
                .or_default()
                .push(i);
        }
        let mut roles = BTreeMap::<&str, Vec<HumanRole>>::new();
        for record in rows("human_task_roles") {
            let b = &record["body"];
            if b["state"] != "active" {
                continue;
            }
            let label = match b["role"].as_str() {
                Some("owner") => "Task owner",
                Some("contributor") => "Contributor",
                _ => continue,
            };
            roles.entry(s(b, "work")).or_default().push(HumanRole {
                operator: s(b, "operator").into(),
                label,
                note: s(b, "note").into(),
                reference: inbox::handle(record),
                brief: s(b, "brief").into(),
            });
        }
        self.activities = rows("work")
            .iter()
            .map(|r| {
                let body = &r["body"];
                let id = s(body, "id");
                let mut people = roles.remove(id).unwrap_or_default();
                if body["phase"] != "open" || body["kind"] == "social" {
                    people.clear();
                }
                people.sort_by(|a, b| {
                    (a.label != "Task owner", &a.operator)
                        .cmp(&(b.label != "Task owner", &b.operator))
                });
                Activity {
                    id: id.into(),
                    brief: inbox::handle(r),
                    roles: people,
                    deliveries: by_work.remove(id).unwrap_or_default(),
                    title: activity_title(body).into(),
                    kind: body["kind"].as_str().unwrap_or("task").into(),
                    phase: s(body, "phase").into(),
                    members: members.remove(id).unwrap_or_default(),
                    latest_utc: self.last_event.get(id).and_then(|(_, utc)| utc.clone()),
                    open_requests: counts.get(id).copied().unwrap_or(0),
                }
            })
            .collect();
        self.activities.sort_by_key(|row| Reverse(recency(&row.id)));
        self.positions = self
            .activities
            .iter()
            .enumerate()
            .map(|(i, row)| (row.id.clone(), i))
            .collect();
    }
}
