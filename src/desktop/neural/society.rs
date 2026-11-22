use super::*;
use home::short_text;
use home_live::Observation;

mod cursor;
mod history;
mod map;
use history::{through, History, Identity, Scope};

#[derive(Default)]
pub(super) struct Society {
    history: History,
    view: View,
}

impl Society {
    pub(super) fn observe(&mut self, status: &Value, events: &[Value], start: usize) {
        self.history.observe(status, events, start);
    }
}

#[derive(Default)]
struct View {
    // Exclusive contribution count. New arrivals cannot move a held point.
    held: Option<usize>,
    scope: Scope,
    sender_page: usize,
    recipient_page: usize,
    record_page: usize,
}

enum Action {
    Event(usize),
    Activity(String),
    Profile(String),
}

impl App {
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
                }
            }
            Some(Action::Activity(work)) => self.open_home_activity(&work),
            Some(Action::Profile(person)) => self.show_person(&person),
            None => {}
        }
    }
}

struct WorkLine {
    state: String,
    work: String,
    note: String,
}

fn current_work(app: &App, id: &str) -> Vec<WorkLine> {
    let now = ai_personas::neural::now();
    let mut lines = Vec::new();
    let projects = app.observed_projects(id);
    for (work, records) in &projects {
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
        if projects.contains_key(work) {
            continue;
        }
        let continuation = &part["continuation"];
        let rank = match app.home_membership_state(part) {
            "Resume requested" | "Ready to continue" => 0,
            "Participating" | "Invited" | "Waiting" => 1,
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
            "Request update".into()
        }
        "user_message" if event["body"]["delivery"].is_string() => "Package feedback".into(),
        "message" | "requester_message" | "user_message" => "Message".into(),
        "human_request" => "Human request".into(),
        "tool_result" | "invitation_response" | "user_acceptance" => app.event_heading(event),
        kind => title(kind),
    }
}

impl View {
    fn show(&mut self, ui: &mut Ui, app: &App, history: &History) -> Option<Action> {
        let mut action = None;
        ui.add_space(7.);
        let previous = self.held;
        let end = self.time_cursor(ui, app, history);
        if self.held != previous {
            self.record_page = 0;
        }
        ui.add_space(12.);
        let p = theme::palette(ui);
        let map = egui::Frame::new()
            .fill(p.panel)
            .corner_radius(8)
            .stroke(Stroke::new(1., p.border))
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(design::display("Exchange map", 30.));
                    let activities = if self.held.is_some() {
                        through(&history.work_starts, end).len()
                    } else {
                        app.home_index.activities.len()
                    };
                    ui.small(format!(
                        "{}  ·  {}",
                        counted(app.records("personas").len() as u64, "persona", "personas"),
                        counted(activities as u64, "activity", "activities")
                    ));
                    if self.held.is_none() {
                        if let ai_personas::neural::live::RunnerObservation::Observed {
                            workers,
                            ..
                        } = &app.runner_observation
                        {
                            if !workers.is_empty() {
                                ui.label(
                                    RichText::new(format!("{} working now", workers.len()))
                                        .small()
                                        .color(p.ink(TEAL)),
                                );
                            }
                        }
                    }
                });
                self.map(ui, app, history, end)
            });
        ui.add_space(12.);
        if map.inner {
            ui.scroll_to_cursor(Some(egui::Align::Min));
        }
        self.records(ui, app, history, end, map.response.rect, &mut action);
        action
    }

    fn select(&mut self, scope: Scope) {
        self.scope = scope;
        self.record_page = 0;
    }

    fn records(
        &mut self,
        ui: &mut Ui,
        app: &App,
        history: &History,
        end: usize,
        map: egui::Rect,
        action: &mut Option<Action>,
    ) {
        let p = theme::palette(ui);
        let scope = self.scope;
        let indices = history.matches(scope, end);
        let count = indices.map_or(end, |items| items.len());
        let heading = match (scope.from, scope.to) {
            (Some(a), Some(b)) => format!("{} → {}", history.name(a), history.name(b)),
            (Some(a), None) => format!("From {}", history.name(a)),
            (None, Some(b)) => format!("To {}", history.name(b)),
            (None, None) => "Society record".into(),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(design::display(heading, 26.));
            ui.small(counted(count as u64, "contribution", "contributions"));
            if map.top() < ui.clip_rect().top() && ui.small_button("Back to map").clicked() {
                ui.scroll_to_rect(map, Some(egui::Align::Min));
            }
            for (i, node) in [scope.from, scope.to].into_iter().enumerate() {
                if i == 1 && scope.to == scope.from {
                    continue;
                }
                if let Some(Identity::Persona(id)) = node.map(|n| &history.nodes[n]) {
                    if app
                        .records("personas")
                        .iter()
                        .any(|r| r["body"]["id"] == id.as_str())
                    {
                        if ui
                            .small_button(format!("{}’s profile", app.label(id)))
                            .clicked()
                        {
                            *action = Some(Action::Profile(id.clone()));
                        }
                    }
                }
            }
        });
        if let Some(Identity::Persona(id)) = scope.from.map(|n| &history.nodes[n]) {
            if self.held.is_none() && scope.to.is_none() {
                let lines = current_work(app, id);
                if !lines.is_empty() {
                    egui::CollapsingHeader::new(format!(
                        "Participation and current work ({})",
                        lines.len()
                    ))
                    .default_open(lines.len() <= 3)
                    .id_salt(("society-current-work", id))
                    .show(ui, |ui| {
                        for line in lines {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(&line.state);
                                if ui
                                    .link(short_text(work_title(app, &line.work), 80))
                                    .clicked()
                                {
                                    *action = Some(Action::Activity(line.work));
                                }
                            });
                            if !line.note.is_empty() {
                                ui.weak(short_text(&line.note, 240))
                                    .on_hover_text(&line.note);
                            }
                        }
                    });
                }
            }
        }
        if count == 0 {
            ui.weak(if end == 0 {
                "Contributions appear here when a persona or human takes part in an activity."
            } else {
                "No contributions for this selection by this point."
            });
            return;
        }
        const ROWS: usize = 5;
        self.record_page = self.record_page.min((count - 1) / ROWS);
        for offset in (self.record_page * ROWS..count).take(ROWS) {
            let position = count - offset - 1;
            let mark = history.entries[indices.map_or(position, |items| items[position])];
            let event = &app.events[mark.event];
            let author = &history.nodes[mark.from];
            ui.separator();
            ui.push_id(("society-event", mark.event), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .link(format!("#{}", event["event"]))
                        .on_hover_text("Open this event in its activity")
                        .clicked()
                    {
                        *action = Some(Action::Event(mark.event));
                    }
                    ui.label(
                        RichText::new(history.name(mark.from))
                            .strong()
                            .color(author.color(ui)),
                    );
                    if let Some(to) = mark.to {
                        ui.weak("→");
                        ui.strong(history.name(to));
                    }
                    ui.label(record_label(app, event));
                    ui.small(s(event, "created_utc"));
                });
                let text = event_detail(event);
                if !text.is_empty() {
                    let preview = document::excerpt(
                        &short_text(text, 780),
                        if ui.available_width() > 650. { 165 } else { 92 },
                    );
                    ui.label(preview).on_hover_ui(|ui| {
                        ui.label(document::excerpt(&short_text(text, 2000), 1200));
                    });
                }
                let work = s(event, "work");
                if ui
                    .link(
                        RichText::new(short_text(work_title(app, work), 100))
                            .color(p.muted)
                            .small(),
                    )
                    .on_hover_text(work_title(app, work))
                    .clicked()
                {
                    *action = Some(Action::Activity(work.into()));
                }
            });
        }
        let page = self.record_page;
        home::pager(ui, &mut self.record_page, count, ROWS, "contributions");
        if self.record_page > page && self.held.is_none() {
            self.held = Some(end);
        }
    }
}
