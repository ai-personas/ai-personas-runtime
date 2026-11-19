use super::{activity_title, inbox, s, BTreeMap, Value};
use std::cmp::Reverse;

pub(super) struct Activity {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub phase: String,
    pub members: Vec<String>,
    pub latest_utc: Option<String>,
    pub open_requests: usize,
}

/// Index the committed snapshot once. Repainting a streaming call only reads
/// these lists and the small changing set of active calls and tool effects.
#[derive(Default)]
pub(super) struct Index {
    pub generation: u64,
    pub activities: Vec<Activity>,
    pub participations: BTreeMap<String, Vec<usize>>,
    pub requests: Vec<usize>,
    pub deliveries: Vec<usize>,
    positions: BTreeMap<String, usize>,
    last_event: BTreeMap<String, (i64, Option<String>)>,
    issued: BTreeMap<String, i64>,
    request_events: BTreeMap<String, i64>,
}

impl Index {
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
            if event["kind"] == "delivery" {
                if let Some(reference) = body["reference"].as_str() {
                    self.issued.insert(reference.to_owned(), seq);
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
            list.sort_by_key(|i| Reverse(recency(s(&parts[*i]["body"], "work"))));
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
        self.activities = rows("work")
            .iter()
            .map(|r| {
                let body = &r["body"];
                let id = s(body, "id");
                Activity {
                    id: id.into(),
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
