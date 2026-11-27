use super::*;
use home::short_text;
use home_live::Observation;

mod activity;
mod cursor;
mod history;
mod map;
mod reader;
use history::{through, Archive, History, Identity, Scope};

#[derive(Default)]
pub(super) struct Society {
    history: Archive,
    view: View,
    return_to: Option<Source>,
}

impl Society {
    pub(super) fn observe(&mut self, status: &Value, events: &[Value], start: usize) {
        self.history.observe(status, events, start);
    }
}

#[derive(Default)]
struct View {
    // Absolute event position, shared by every activity and stable as new work arrives.
    held: Option<usize>,
    activity: Option<String>,
    query: String,
    scope: Scope,
    direction: Direction,
    kind: RecordKind,
    reading: bool,
    expanded: Option<usize>,
    sender_page: usize,
    recipient_page: usize,
    record_page: usize,
    reset_record_scroll: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Direction {
    #[default]
    Both,
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RecordKind {
    #[default]
    All,
    Addressed,
    Shared,
    Individual,
}

enum Source {
    Activity(String),
    Profile(String),
    File(String),
    Package(String),
}

enum Action {
    Event(usize),
    Activity(String),
    Profile(String),
    File(String),
    Package(String),
}

impl App {
    pub(super) fn open_roster_profile(&mut self, id: &str) {
        let from_society = self.viewing_society()
            || matches!(&self.home.society.return_to, Some(Source::Profile(origin)) if self.layout.editor == Editor::People && *origin == self.persona);
        self.show_person(id);
        if from_society {
            self.home.society.return_to = Some(Source::Profile(id.to_owned()));
        }
    }

    pub(super) fn society_view(&mut self, ui: &mut Ui) {
        let mut view = std::mem::take(&mut self.home.society.view);
        let action = view.show(ui, self, &self.home.society.history);
        self.home.society.view = view;
        match action {
            Some(Action::Event(index)) => {
                let event = &self.events[index];
                let work = s(event, "work").to_owned();
                if let Some(seq) = event["event"].as_i64() {
                    self.open_home_event(&work, seq);
                    self.home.society.return_to = Some(Source::Activity(work));
                }
            }
            Some(Action::Activity(work)) => {
                self.open_home_activity(&work);
                self.home.society.return_to = Some(Source::Activity(work));
            }
            Some(Action::Profile(person)) => {
                self.show_person(&person);
                self.home.society.return_to = Some(Source::Profile(person));
            }
            Some(Action::File(reference)) => {
                self.open_resource(ui.ctx(), reference.clone());
                self.home.society.return_to = Some(Source::File(reference));
            }
            Some(Action::Package(reference)) => {
                self.open_issued_package(ui.ctx(), &reference);
                self.home.society.return_to = Some(if self.layout.editor == Editor::Delivery {
                    Source::Package(reference)
                } else {
                    Source::File(reference)
                });
            }
            None => {}
        }
    }

    pub(super) fn society_return(&mut self, ui: &mut Ui) {
        let same = match &self.home.society.return_to {
            Some(Source::Activity(work)) => {
                self.layout.editor == Editor::Studio && self.work == *work
            }
            Some(Source::Profile(person)) => {
                self.layout.editor == Editor::People && self.persona == *person
            }
            Some(Source::File(reference)) => {
                self.layout.editor == Editor::Artifacts && self.selected_reference == *reference
            }
            Some(Source::Package(reference)) => {
                self.layout.editor == Editor::Delivery && self.layout.delivery == *reference
            }
            None => return,
        };
        if !same {
            self.home.society.return_to = None;
        } else if ui.link("← Back to society").clicked() {
            self.home.society.return_to = None;
            self.layout.editor = Editor::Home;
        }
    }

    pub(super) fn society_activity_picker(&mut self, ui: &mut Ui) {
        let society = &mut self.home.society;
        society
            .view
            .activity_picker(ui, &society.history, &self.home_index);
    }

    pub(super) fn society_reading(&self) -> bool {
        self.home.society.view.reading
    }
}

struct WorkLine {
    state: String,
    work: String,
    note: String,
}

fn current_work(app: &App, id: &str, selected: Option<&str>) -> Vec<WorkLine> {
    let now = ai_personas::neural::now();
    let mut lines = Vec::new();
    let projects = app.observed_projects(id);
    for (work, records) in &projects {
        if selected.is_some_and(|id| id != *work) {
            continue;
        }
        let worker = app.runner_observation.has_worker(work, id);
        if records.is_empty() {
            lines.push(WorkLine {
                state: "Worker admitted".into(),
                work: (*work).into(),
                note: String::new(),
            });
        }
        for &record in records {
            let (state, note) = match record {
                Observation::Call(call) => (live_board::call_stage(call, now, worker), ""),
                Observation::UnresolvedCall(call) => (live_board::call_stage(call, now, false), ""),
                Observation::Tool(tool) => {
                    (live_board::effect_stage(tool, worker), s(tool, "tool"))
                }
            };
            lines.push(WorkLine {
                state: state.into(),
                work: (*work).into(),
                note: note.into(),
            });
        }
    }
    lines.sort_by_key(|line| !app.runner_observation.has_worker(&line.work, id));
    let parts = app.records("participation");
    let mut memberships = Vec::new();
    for &index in app.home_index.participations.get(id).into_iter().flatten() {
        let part = &parts[index]["body"];
        let work = s(part, "work");
        if projects.contains_key(work) || selected.is_some_and(|id| id != work) {
            continue;
        }
        let continuation = &part["continuation"];
        let rank = match app.home_membership_state(part) {
            "Resume requested" | "Ready to continue" => 0,
            "Participating" | "Invited" | "Chose to wait" => 1,
            "Stopped" => 2,
            _ => 3,
        };
        memberships.push((
            rank,
            WorkLine {
                state: app.membership_summary(part),
                work: work.into(),
                note: continuation["purpose"]
                    .as_str()
                    .or(continuation["reason"].as_str())
                    .unwrap_or_default()
                    .into(),
            },
        ));
    }
    // Retain revision order among equally available activities. A later closed
    // membership should not hide another activity where the person can still act.
    memberships.sort_by_key(|(rank, _)| *rank);
    lines.extend(memberships.into_iter().map(|(_, line)| line));
    lines
}

fn work_title<'a>(app: &'a App, work: &'a str) -> &'a str {
    app.home_index
        .activity(work)
        .map(|a| a.title.as_str())
        .unwrap_or(work)
}

fn event_detail(event: &Value) -> &str {
    let text = live_board::event_text(event);
    if !text.is_empty() {
        return text;
    }
    let body = &event["body"];
    event["tool_outcome"]["file_path"]
        .as_str()
        .or(event["tool_outcome"]["label"].as_str())
        .or(body["verdict"].as_str())
        .or(body["state"].as_str())
        .unwrap_or_default()
}

fn record_label(app: &App, event: &Value) -> String {
    match s(event, "kind") {
        "message" | "requester_message" | "user_message"
            if event["body"]["human_request"].is_string() =>
        {
            if event["body"]["action"] == "contribute" { "Contributor observation" } else { "Request update" }.into()
        }
        "user_message" if event["body"]["delivery"].is_string() => "Package feedback".into(),
        "message" | "requester_message" | "user_message" => "Message".into(),
        "human_request" => "Human request".into(),
        "human_request_packet" => "Field packet".into(),
        "human_role_changed" | "tool_result" | "invitation_response" | "user_acceptance" => app.event_heading(event),
        kind => title(kind),
    }
}

impl View {
    fn restart_reader(&mut self) {
        self.record_page = 0;
        self.expanded = None;
        self.reset_record_scroll = true;
    }

    fn focus_activity(&mut self, work: Option<String>) {
        self.activity = work;
        self.query.clear();
        self.sender_page = 0;
        self.recipient_page = 0;
        self.restart_reader();
    }
    fn select(&mut self, scope: Scope) {
        self.scope = scope;
        self.direction = Direction::Both;
        self.restart_reader();
    }

    fn show(&mut self, ui: &mut Ui, app: &App, archive: &Archive) -> Option<Action> {
        let mut action = None;
        let history = archive.history(self.activity.as_deref());
        let previous = self.held;
        let end = self.time_cursor(ui, app, &history);
        if self.held != previous {
            self.restart_reader();
        }
        ui.add_space(7.);
        let width = ui.available_width();
        let split = width >= 930.;
        if split {
            let gap = 12.;
            let left = ((width - gap) * 0.60).min(width - 360.).max(540.);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.vertical(|ui| {
                    ui.set_width(left);
                    ui.spacing_mut().item_spacing.x = 6.;
                    self.map_panel(ui, app, archive, &history, end, true);
                });
                ui.vertical(|ui| {
                    ui.set_width(width - left - gap);
                    ui.spacing_mut().item_spacing.x = 6.;
                    self.reader(ui, app, &history, end, true, &mut action);
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.reading, false, "Exchange map");
                ui.selectable_value(&mut self.reading, true, "Read record");
            });
            ui.add_space(4.);
            if self.reading {
                self.reader(ui, app, &history, end, false, &mut action);
            } else if self.map_panel(ui, app, archive, &history, end, false) {
                self.reading = true;
            }
        }
        action
    }

    fn map_panel(
        &mut self,
        ui: &mut Ui,
        app: &App,
        archive: &Archive,
        h: &History,
        end: usize,
        split: bool,
    ) -> bool {
        let p = theme::palette(ui);
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let members = self.activity.as_deref().and_then(|id| app.home_index.activity(id));
            let people = h.nodes.iter().enumerate().filter(|(n,id)| {
                matches!(id, Identity::Persona(person) if self.person_visible(h, *n, person, members, end))
            }).count();
            ui.horizontal_wrapped(|ui| {
                ui.label(design::display("Exchange map", 28.));
                let mut count = counted(people as u64, "persona", "personas");
                if members.is_none() {
                    let activities = if self.held.is_some() { archive.activity_count(end) } else { app.home_index.activities.len() };
                    count.push_str(&format!(" · {}", counted(activities as u64, "activity", "activities")));
                }
                ui.small(count);
                if self.held.is_none() {
                    if let ai_personas::neural::live::RunnerObservation::Observed { workers, .. } = &app.runner_observation {
                        let active = workers.values().filter(|work| self.activity.as_ref().is_none_or(|id| id == *work)).count();
                        if active > 0 { ui.label(RichText::new(format!("{active} working now")).small().color(p.ink(TEAL))); }
                    }
                }
            });
            if let Some(a) = members {
                ui.add(egui::Label::new(RichText::new(&a.title).small().color(p.muted)).truncate()).on_hover_text(&a.title);
            }
            self.map(ui, app, h, end, split)
        }).inner
    }

    fn person_visible(
        &self,
        h: &History,
        n: usize,
        id: &str,
        activity: Option<&home_index::Activity>,
        end: usize,
    ) -> bool {
        activity.is_none_or(|a| {
            self.held.is_none() && a.members.iter().any(|m| m == id) || h.time.involved(n, end)
        })
    }
}

fn surface(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(p.panel)
        .corner_radius(8)
        .stroke(Stroke::new(1., p.border))
        .inner_margin(12)
}
