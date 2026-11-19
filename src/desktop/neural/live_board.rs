//! Activity-scoped observations, retained events, and human participation.
use super::*;
use std::collections::VecDeque;

const PAGE: usize = 30;
const WIDE: f32 = 728.;

#[derive(Clone, Copy, Default, PartialEq)]
enum View {
    #[default]
    Live,
    Work,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Pane {
    #[default]
    Timeline,
    People,
}

enum WorkSection {
    Deliveries,
    BriefFiles,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Filter {
    #[default]
    Highlights,
    All,
    Messages,
    WithYou,
    Files,
    Tools,
    Problems,
}

impl Filter {
    const OPTIONS: [(Self, &'static str); 7] = [
        (Self::Highlights, "Highlights"),
        (Self::All, "All events"),
        (Self::Messages, "Messages"),
        (Self::WithYou, "With you"),
        (Self::Files, "Files & deliveries"),
        (Self::Tools, "Tools"),
        (Self::Problems, "Problems"),
    ];

    fn includes(self, event: &Value) -> bool {
        let kind = s(event, "kind");
        let tool = s(&event["body"], "tool");
        let is_tool = kind == "tool_result";
        let file = event["tool_outcome"]["file_reference"].is_string();
        let problem = event["tool_outcome"]["problem"] == true
            || matches!(
                kind,
                "provider_failure"
                    | "decision_rejected"
                    | "character_input_unverified"
                    | "uncertain_effect"
                    | "uncertain_job"
                    | "runner_failure"
                    | "recovered_invocation"
            );
        match self {
            Self::Highlights if is_tool => {
                file || problem
                    || matches!(
                        tool,
                        "asset.save"
                            | "asset.import"
                            | "asset.organize"
                            | "asset.register"
                            | "host.command"
                            | "host.start"
                            | "host.cancel"
                            | "command.run"
                            | "compute.run"
                            | "geometry.render"
                    )
            }
            Self::Highlights => kind != "appraisal_selected",
            Self::All => true,
            Self::Messages => matches!(
                kind,
                "message"
                    | "reply"
                    | "user_message"
                    | "initialization_note"
                    | "human_request"
                    | "human_request_progress"
                    | "human_request_withdrawn"
                    | "asset_feedback"
                    | "request_message_sent"
                    | "request_review_sent"
                    | "requester_message"
                    | "requester_review"
                    | "request_reply"
            ),
            Self::WithYou => {
                recipient(event) == "user"
                    || matches!(
                        kind,
                        "user_message"
                            | "human_request_progress"
                            | "user_acceptance"
                            | "requirements_amended"
                    )
                    || event["body"]["author_kind"] == "operator"
                    || event["body"]["recipient_kind"] == "operator"
            }
            Self::Files => {
                file || human_file_count(event) > 0
                    || matches!(
                        kind,
                        "delivery" | "request_delivery" | "package_review" | "user_acceptance" | "asset_feedback"
                    )
                    || (is_tool && matches!(
                        tool,
                        "asset.save" | "asset.register" | "asset.retire" | "geometry.render"
                    ))
            }
            Self::Tools => is_tool,
            Self::Problems => problem,
        }
    }
}

pub(super) struct Board {
    view: View,
    scope: String,
    pane: Pane,
    filter: Filter,
    person: String,
    indexed: usize,
    matches: Vec<usize>,
    paused_at: Option<i64>,
    limit: usize,
    jump_to_latest: bool,
    work_focus: Option<WorkSection>,
    expanded: Option<i64>,
    traces: BTreeMap<String, VecDeque<(i64, u64)>>,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            view: View::Live,
            scope: String::new(),
            pane: Pane::Timeline,
            filter: Filter::Highlights,
            person: String::new(),
            indexed: 0,
            matches: Vec::new(),
            paused_at: None,
            limit: PAGE,
            jump_to_latest: true,
            work_focus: None,
            expanded: None,
            traces: BTreeMap::new(),
        }
    }
}

impl Board {
    pub(super) fn show_activity(&mut self, work: &str) {
        self.select_scope(work);
        self.view = View::Live;
    }

    pub(super) fn take_delivery_focus(&mut self) -> bool {
        self.work_focus
            .take_if(|focus| matches!(focus, WorkSection::Deliveries))
            .is_some()
    }

    pub(super) fn take_files_focus(&mut self) -> bool {
        self.work_focus
            .take_if(|focus| matches!(focus, WorkSection::BriefFiles))
            .is_some()
    }

    fn select_scope(&mut self, work: &str) {
        if self.scope != work {
            *self = Self {
                view: self.view,
                scope: work.into(),
                ..Self::default()
            };
        }
    }

    fn reset_filter(&mut self) {
        self.indexed = 0;
        self.matches.clear();
        self.expanded = None;
        self.limit = PAGE;
        self.jump_to_latest = true;
    }

    fn follow_control(&mut self, ui: &mut Ui, events: &[Value], cursor: i64) -> usize {
        self.index(events);
        let latest = self.matches.len();
        let end = self.paused_at.map_or(latest, |at| {
            self.matches
                .partition_point(|i| events[*i]["event"].as_i64().unwrap_or(0) <= at)
        });
        let new = latest - end;
        let paused = self.paused_at.is_some();
        let show_count_beside = new > 0 && ui.available_size_before_wrap().x >= 190.;
        let label = match (paused, new) {
            (false, _) => "Pause feed".to_owned(),
            (true, n) if n > 0 && !show_count_beside => format!("Latest · {n} new"),
            (true, _) => "Return to latest".to_owned(),
        };
        // Keep the same tab stop and footprint when reading or resuming.
        let toggle = ui
            .add_sized(
                [116., ui.spacing().interact_size.y],
                egui::Button::new(RichText::new(label).small())
                    .truncate()
                    .selected(paused),
            )
            .on_hover_text(if paused {
                "Return to the newest event and follow new arrivals"
            } else {
                "Keep your place while new events arrive"
            });
        if show_count_beside {
            let count = RichText::new(format!("{new} new"))
                .small()
                .color(theme::palette(ui).ink(theme::ACCENT));
            ui.add(egui::Label::new(count).truncate());
        }
        if toggle.clicked() {
            toggle.request_focus();
            if paused {
                self.paused_at = None;
                self.jump_to_latest = true;
                return latest;
            }
            self.paused_at = Some(cursor);
        }
        end
    }

    fn index(&mut self, events: &[Value]) {
        if self.indexed > events.len() {
            self.reset_filter();
        }
        for (offset, event) in events[self.indexed..].iter().enumerate() {
            if event["work"] != self.scope
                || !self.filter.includes(event)
                || !(self.person.is_empty()
                    || event["actor"] == self.person
                    || recipient(event) == self.person)
            {
                continue;
            }
            let index = self.indexed + offset;
            // Current deliveries have a public notice and an addressed package
            // in one transaction. Older activities may contain just the notice.
            if self.filter == Filter::Highlights
                && event["kind"] == "delivery"
                && event["target"] == "*"
                && event["body"]["reference"].is_string()
                && events.get(index + 1).is_some_and(|next| {
                    next["kind"] == "delivery"
                        && next["work"] == self.scope
                        && next["target"] != "*"
                        && next["body"]["reference"] == event["body"]["reference"]
                })
            {
                continue;
            }
            self.matches.push(index);
        }
        self.indexed = events.len();
    }

    fn observe(&mut self, calls: &[Value]) {
        self.traces.retain(|id, _| {
            calls
                .iter()
                .any(|c| c["id"] == *id && c["work"] == self.scope)
        });
        for call in calls.iter().filter(|c| c["work"] == self.scope) {
            let (Some(at), Some(bytes)) =
                (call["observed_at"].as_i64(), call["stream_bytes"].as_u64())
            else {
                continue;
            };
            let samples = self.traces.entry(s(call, "id").into()).or_default();
            match samples.back_mut() {
                Some((time, count)) if *time == at => *count = (*count).max(bytes),
                Some((time, count)) if at < *time || bytes <= *count => {}
                _ => {
                    samples.push_back((at, bytes));
                    if samples.len() > 32 {
                        samples.pop_front();
                    }
                }
            }
        }
    }
}

fn recipient(event: &Value) -> &str {
    if matches!(
        s(event, "kind"),
        "request_message_sent" | "request_review_sent"
    ) {
        s(&event["body"], "to")
    } else {
        s(event, "target")
    }
}

fn human_file_count(event: &Value) -> usize {
    if event["invocation"].is_null()
        && matches!(s(event, "kind"), "user_message" | "requirements_amended")
    {
        event["body"]["attachments"].as_array().map_or(0, Vec::len)
    } else {
        0
    }
}

fn age(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        3600..86400 => format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60),
        _ => format!("{}d {}h", seconds / 86400, (seconds % 86400) / 3600),
    }
}

fn has_recent_bytes(call: &Value, now: i64) -> bool {
    call["stream_bytes"].as_u64().is_some_and(|b| b > 0)
        && call["observed_at"]
            .as_i64()
            .is_some_and(|t| (0..=3).contains(&now.saturating_sub(t)))
}

pub(super) fn call_stage(call: &Value, now: i64) -> &'static str {
    match s(call, "state") {
        "prepared" => "Decision prepared",
        "probe_prepared" => "Assessment prepared",
        "responded" => "Response received",
        "probe_dispatched" if has_recent_bytes(call, now) => "Receiving assessment",
        "dispatched" if has_recent_bytes(call, now) => "Receiving response",
        "probe_dispatched" => "Assessment dispatched",
        "dispatched" => "Decision dispatched",
        _ => "Model call",
    }
}

fn dot(ui: &mut Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10., 12.), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 3.3, color);
}

fn small_text(ui: &mut Ui, text: impl Into<String>) {
    ui.label(
        RichText::new(text)
            .size(12.)
            .color(theme::palette(ui).muted),
    );
}

impl App {
    pub(super) fn live_workspace_selected(&mut self, ui: &mut Ui) -> bool {
        self.live_board.select_scope(&self.work);
        let available = ui.available_width();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.live_board.view, View::Live, "Live activity");
            ui.selectable_value(&mut self.live_board.view, View::Work, "Work & deliveries");
            let mut earlier = |ui: &mut Ui| {
                if ui
                    .add_enabled(!self.work.is_empty(), egui::Button::new("Earlier work…"))
                    .on_hover_text("Inspect files and deliveries from earlier activities")
                    .clicked()
                {
                    let work = self.work.clone();
                    self.open_sources(ui.ctx(), &work, None);
                }
            };
            if available >= 450. {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), earlier);
            } else {
                earlier(ui);
            }
        });
        ui.add_space(7.);
        self.live_board.view == View::Live
    }

    pub(super) fn studio_live(&mut self, ui: &mut Ui) {
        if self.work.is_empty() {
            if self.snapshot.status.is_null() {
                ui.weak("Loading activities…");
            } else {
                self.activity_overview(ui);
            }
            return;
        }
        let wide = ui.available_width() >= WIDE;
        let now = ai_personas::neural::now();
        self.live_board.observe(&self.snapshot.active);
        self.live_heading(ui, now);
        let people = self.live_people();

        egui::TopBottomPanel::bottom("live-message-composer")
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                top: 8,
                ..Default::default()
            }))
            .default_height(103.)
            .resizable(false)
            .show_separator_line(false)
            .show_inside(ui, |ui| self.live_composer(ui));
        // The event and people panes share the space above the pinned composer.
        // Their contents can grow between frames, including while reading a reply.
        ui.set_clip_rect(ui.clip_rect().intersect(ui.available_rect_before_wrap()));
        if wide {
            let width = (ui.available_width() * 0.30).clamp(231., 326.);
            egui::SidePanel::right("live-people-panel")
                .exact_width(width)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    left: 12,
                    ..Default::default()
                }))
                .show_inside(ui, |ui| self.live_roster(ui, &people, now));
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.live_board.pane, Pane::Timeline, "Timeline");
                ui.selectable_value(
                    &mut self.live_board.pane,
                    Pane::People,
                    format!("People & tools  {}", people.len()),
                );
            });
            ui.add_space(3.);
        }
        if wide || self.live_board.pane == Pane::Timeline {
            self.live_timeline(ui, &people, wide);
        } else {
            self.live_roster(ui, &people, now);
        }
    }

    fn live_heading(&mut self, ui: &mut Ui, now: i64) {
        let p = theme::palette(ui);
        let ended = self
            .records("work")
            .iter()
            .any(|r| r["body"]["id"] == self.work && r["body"]["phase"] == "dissolved");
        let following = self
            .following
            .as_ref()
            .is_some_and(|(work, _)| *work == self.work);
        let calls: Vec<_> = self
            .snapshot
            .active
            .iter()
            .filter(|c| c["work"] == self.work)
            .collect();
        let effects: Vec<_> = self
            .snapshot
            .active_effects
            .iter()
            .filter(|e| e["work"] == self.work)
            .collect();
        let running = effects
            .iter()
            .filter(|e| e["state"] == "dispatched")
            .count();
        let queued = effects.iter().filter(|e| e["state"] == "pending").count();
        let uncertain = effects.iter().any(|e| e["state"] == "uncertain");
        let blocked = self.records("controls").iter().find(|control| {
            let scope = s(control, "scope");
            control["state"] != "running"
                && (matches!(scope, "admission" | "global") || scope.strip_prefix("work:") == Some(self.work.as_str()))
        }).map(|c| (s(c,"state").to_owned(),s(c,"reason").to_owned()));
        let stream = calls
            .iter()
            .filter(|c| has_recent_bytes(c, now))
            .max_by_key(|c| c["observed_at"].as_i64());
        let (state, color) = if let Some(call) = stream {
            (
                format!(
                    "{} · {} received",
                    self.label(s(call, "actor")),
                    size(call["stream_bytes"].as_u64().unwrap_or(0))
                ),
                p.ink(TEAL),
            )
        } else if !calls.is_empty() {
            let call = calls[0];
            (
                if calls.len() == 1 {
                    format!(
                        "{} · {}",
                        self.label(s(call, "actor")),
                        call_stage(call, now)
                    )
                } else {
                    format!("{} model calls", calls.len())
                },
                p.ink(TEAL),
            )
        } else if running > 0 {
            (
                format!(
                    "{running} {} running",
                    if running == 1 { "tool" } else { "tools" }
                ),
                p.ink(TEAL),
            )
        } else if uncertain {
            ("Tool outcome uncertain".into(), p.ink(theme::ACCENT))
        } else if queued > 0 {
            (
                format!(
                    "{queued} {} queued",
                    if queued == 1 { "tool" } else { "tools" }
                ),
                p.muted,
            )
        } else if ended {
            ("Activity ended".into(), p.muted)
        } else if let Some((control, _)) = &blocked {
            (format!("Scheduling {control}"), p.ink(theme::ACCENT))
        } else if following {
            ("Listening for activity".into(), p.ink(TEAL))
        } else {
            ("No calls in progress".into(), p.muted)
        };
        // The workspace context bar already names this activity on short windows.
        if ui.available_height() >= 410. {
            ui.add(egui::Label::new(RichText::new(self.task_title()).size(23.).strong()).truncate())
                .on_hover_text(self.task_title());
        }
        ui.horizontal_wrapped(|ui| {
            dot(ui, color);
            let status = ui.label(RichText::new(state).size(13.).color(color));
            if let Some((_, reason)) = &blocked { status.on_hover_text(reason); }
            ui.separator();
            self.live_shortcuts(ui);
            if following {
                if ui
                    .small_button("Finish run")
                    .on_hover_text("Let the current decisions finish and stop listening")
                    .clicked()
                {
                    self.finish_following();
                }
            } else if !ended && blocked.is_none() && !self.running && self.snapshot.active.is_empty() && running == 0 {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Run activity").small())
                    .clicked()
                {
                    self.follow_activity(ui.ctx());
                }
            }
        });
        ui.add_space(7.);
    }

    fn live_shortcuts(&mut self, ui: &mut Ui) {
        let requests: Vec<_> = self
            .records("human_requests")
            .iter()
            .filter(|r| {
                r["body"]["work"] == self.work
                    && matches!(s(&r["body"], "state"), "open" | "in_progress")
            })
            .collect();
        let mut open = None;
        let mut inbox = false;
        let tint = if requests.is_empty() {
            theme::palette(ui).muted
        } else {
            theme::palette(ui).ink(theme::ACCENT)
        };
        ui.menu_button(
            RichText::new(format!("Needs you  {}", requests.len()))
                .size(13.)
                .color(tint),
            |ui| {
                if requests.is_empty() {
                    ui.weak("No open requests in this activity.");
                }
                for request in &requests {
                    let body = &request["body"];
                    if ui
                        .button(format!(
                            "{} · {}",
                            self.label(s(body, "requester")),
                            excerpt(s(body, "title"), 54)
                        ))
                        .on_hover_text(s(body, "action"))
                        .clicked()
                    {
                        open = Some(s(&request["reference"], "id").to_owned());
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Open workspace inbox").clicked() {
                    inbox = true;
                    ui.close();
                }
            },
        );
        if let Some(id) = open {
            self.show_human_request(&id);
        } else if inbox {
            self.inbox.list();
            self.layout.editor = Editor::Inbox;
        }
        let n = self
            .records("deliveries")
            .iter()
            .filter(|r| r["body"]["work"] == self.work)
            .count();
        if ui
            .small_button(format!("Packages  {n}"))
            .on_hover_text("Go to this activity’s delivered work")
            .clicked()
        {
            self.live_board.view = View::Work;
            self.live_board.work_focus = Some(WorkSection::Deliveries);
        }
        let files = self
            .records("work")
            .iter()
            .map(|record| &record["body"])
            .find(|work| work["id"] == self.work)
            .map_or(0, |work| {
                let original = work["materials"].as_array().map_or(0, Vec::len);
                let added: usize = work["amendments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|amendment| amendment["attachments"].as_array())
                    .map(Vec::len)
                    .sum();
                original + added
            });
        if files > 0
            && ui.small_button(format!("Brief files · {files}"))
                .on_hover_text("Open the files supplied with this brief and its amendments")
                .clicked()
        {
            self.live_board.view = View::Work;
            self.live_board.work_focus = Some(WorkSection::BriefFiles);
        }
    }

    fn live_people(&self) -> Vec<String> {
        let mut people: BTreeSet<_> = self
            .records("participation")
            .iter()
            .filter(|r| r["body"]["work"] == self.work)
            .map(|r| s(&r["body"], "persona").to_owned())
            .collect();
        for item in self
            .snapshot
            .active
            .iter()
            .chain(&self.snapshot.active_effects)
            .filter(|a| a["work"] == self.work)
        {
            people.insert(s(item, "actor").into());
        }
        let mut people: Vec<_> = people.into_iter().collect();
        people.sort_by_cached_key(|id| {
            let active = self
                .snapshot
                .active
                .iter()
                .chain(&self.snapshot.active_effects)
                .any(|o| o["work"] == self.work && o["actor"] == *id);
            (!active, self.label(id).to_lowercase())
        });
        people
    }

    fn live_roster(&mut self, ui: &mut Ui, people: &[String], now: i64) {
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.panel)
            .corner_radius(7)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::glyph(ui, Icon::People, p.muted);
                    ui.label(RichText::new("People & tools").size(16.).strong());
                    small_text(ui, people.len().to_string());
                });
                ui.add_space(4.);
                egui::ScrollArea::vertical()
                    .id_salt(("live-roster", &self.work))
                    .min_scrolled_height(0.)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if people.is_empty() {
                            ui.weak("No participants in this activity.");
                        }
                        for person in people {
                            ui.push_id(person, |ui| {
                                ui.separator();
                                ui.add_space(3.);
                                self.live_person(ui, person, now);
                                ui.add_space(4.);
                            });
                        }
                    });
            });
    }

    fn live_person(&mut self, ui: &mut Ui, person: &str, now: i64) {
        let p = theme::palette(ui);
        let name = self.label(person).to_owned();
        let color = p.ink(self.color(person));
        let can_message = self
            .records("participation")
            .iter()
            .any(|r| r["body"]["work"] == self.work && r["body"]["persona"] == person);
        ui.horizontal(|ui| {
            if avatar(ui, &name, color, 26.)
                .on_hover_text("Open persona")
                .clicked()
            {
                self.show_person(person);
            }
            let width = (ui.available_width() - 34.).max(35.);
            ui.allocate_ui_with_layout(
                egui::vec2(width, 24.),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(width);
                    if ui
                        .add(
                            egui::Label::new(RichText::new(&name).strong())
                                .truncate()
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text(format!("{name} · open persona"))
                        .clicked()
                    {
                        self.show_person(person);
                    }
                },
            );
            if can_message
                && theme::icon_button(ui, Icon::Conversation, &format!("Message {name}")).clicked()
            {
                self.address_in_live(person);
            }
        });
        let calls = self
            .snapshot
            .active
            .iter()
            .filter(|c| c["work"] == self.work && c["actor"] == person);
        let effects = self
            .snapshot
            .active_effects
            .iter()
            .filter(|e| e["work"] == self.work && e["actor"] == person);
        let mut occupied = false;
        let mut open = None;
        for call in calls {
            occupied = true;
            ui.horizontal_wrapped(|ui| {
                dot(ui, p.ink(TEAL));
                ui.label(
                    RichText::new(call_stage(call, now))
                        .size(13.)
                        .strong()
                        .color(p.ink(TEAL)),
                );
                if let Some(at) = call["created"].as_i64() {
                    ui.label(
                        RichText::new(age(now.saturating_sub(at)))
                            .size(12.)
                            .color(p.muted),
                    )
                    .on_hover_text(format!("Prepared at {}", time_label(at)));
                }
            });
            if let (Some(bytes), Some(at)) =
                (call["stream_bytes"].as_u64(), call["observed_at"].as_i64())
            {
                let elapsed = now.saturating_sub(at);
                let when = if elapsed <= 0 {
                    "just now".into()
                } else {
                    format!("{} ago", age(elapsed))
                };
                small_text(ui, format!("{} B received · {when}", number(bytes)));
                if let Some(samples) = self.live_board.traces.get(s(call, "id")) {
                    draw_trace(ui, samples, p.ink(TEAL));
                }
            } else if matches!(s(call, "state"), "dispatched" | "probe_dispatched") {
                small_text(ui, "No response bytes observed yet");
            }
        }
        for effect in effects {
            occupied = true;
            let (state, color) = match s(effect, "state") {
                "dispatched" => ("Tool running", p.ink(TEAL)),
                "pending" => ("Tool queued", p.muted),
                "uncertain" => ("Outcome uncertain", p.ink(theme::ACCENT)),
                _ => ("Tool status", p.muted),
            };
            ui.horizontal_wrapped(|ui| {
                dot(ui, color);
                ui.label(RichText::new(state).size(13.).strong().color(color));
            });
            let reference = effect["job"].as_str().or(effect["operation"].as_str());
            if let Some(reference) = reference {
                if ui
                    .link(RichText::new(s(effect, "tool")).size(13.))
                    .on_hover_text("Inspect the retained job or tool request")
                    .clicked()
                {
                    open = Some(reference.to_owned());
                }
            } else {
                small_text(ui, s(effect, "tool"));
            }
            if let Some(start) = effect["started"].as_i64() {
                small_text(
                    ui,
                    format!("Started {} ago", age(now.saturating_sub(start))),
                );
            }
            if !effect["cancellation"].is_null() {
                small_text(ui, "Cancellation requested");
            }
        }
        if let Some(reference) = open {
            self.open_resource(ui.ctx(), reference);
        }
        if !occupied {
            let part = self
                .records("participation")
                .iter()
                .map(|r| &r["body"])
                .find(|b| b["work"] == self.work && b["persona"] == person)
                .unwrap_or(&Value::Null);
            let state = self.persona_state(person);
            ui.label(RichText::new(state).size(13.).strong().color(p.muted));
            let continuation = &part["continuation"];
            let reason = continuation["reason"]
                .as_str()
                .or(continuation["purpose"].as_str())
                .unwrap_or("");
            let still_here = part["accepted"] == true
                && part["departure"].is_null()
                && part["closed_by_activity"].is_null();
            let open = still_here
                && !self
                    .records("work")
                    .iter()
                    .any(|r| r["body"]["id"] == self.work && r["body"]["phase"] == "dissolved");
            if open && !reason.is_empty() {
                let preview = preview_text(reason, 100);
                ui.add(egui::Label::new(RichText::new(preview).size(12.).color(p.muted)).wrap())
                    .on_hover_text(reason);
            }
            if open && matches!(s(continuation, "kind"), "wait" | "waiting") {
                let names: Vec<_> = continuation["on_events"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|v| v.replace('_', " "))
                    .collect();
                let mut triggers = if names.is_empty() {
                    "any new arrival".into()
                } else {
                    names.join(", ")
                };
                if let Some(at) = continuation["wake_at_unix"].as_i64() {
                    let relative = if at > now {
                        format!("in {}", age(at - now))
                    } else {
                        format!("{} ago", age(now.saturating_sub(at)))
                    };
                    triggers.push_str(&format!(" or {} ({relative})", time_label(at)));
                }
                small_text(ui, format!("Wake conditions · {triggers}"));
            }
            if let Some(last) = self
                .character_decisions
                .iter()
                .rev()
                .find(|d| d["work"] == self.work && d["actor"] == person)
            {
                let stamp = s(last, "accepted_utc");
                let date = if last["accepted"]
                    .as_i64()
                    .is_some_and(|t| t.div_euclid(86400) == now.div_euclid(86400))
                {
                    stamp.get(11..16)
                } else {
                    stamp.get(..16)
                };
                let label = date.map_or_else(
                    || "Latest decision".to_owned(),
                    |t| format!("Latest decision · {t} UTC"),
                );
                if text_action(ui, &label).on_hover_text(stamp).clicked() {
                    self.open_resource(ui.ctx(), format!("experience:{}@1", s(last, "invocation")));
                }
            }
        }
    }
}

fn draw_trace(ui: &mut Ui, samples: &VecDeque<(i64, u64)>, color: Color32) {
    let (Some(&(first_time, first_bytes)), Some(&(last_time, last_bytes))) =
        (samples.front(), samples.back())
    else {
        return;
    };
    if last_time <= first_time || last_bytes <= first_bytes {
        return;
    }
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 23.), egui::Sense::hover());
    let area = rect.shrink2(egui::vec2(2., 3.));
    let painter = ui.painter();
    painter.hline(
        area.x_range(),
        area.bottom(),
        Stroke::new(1., theme::palette(ui).hover),
    );
    let points = samples
        .iter()
        .map(|(time, bytes)| {
            let x = (*time - first_time) as f32 / (last_time - first_time) as f32;
            let y = (*bytes - first_bytes) as f32 / (last_bytes - first_bytes) as f32;
            egui::pos2(
                area.left() + x * area.width(),
                area.bottom() - y * area.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.6, color)));
    painter.circle_filled(area.right_top(), 2., color);
    response.on_hover_text(format!(
        "Observed response-stream bytes · {} to {} B in {}",
        number(first_bytes),
        number(last_bytes),
        age(last_time - first_time)
    ));
}

fn preview_text(source: &str, limit: usize) -> String {
    let mut text = String::new();
    let mut chars = 0;
    for word in source.split_whitespace() {
        if !text.is_empty() {
            if chars == limit {
                text.push('…');
                break;
            }
            text.push(' ');
            chars += 1;
        }
        for c in word.chars() {
            if chars == limit {
                text.push('…');
                return text;
            }
            text.push(c);
            chars += 1;
        }
    }
    text
}

enum EventAction {
    Open(String),
    SourceFile {
        source: String,
        object: String,
        name: String,
    },
    Profile(String),
    Address(String),
    Reply(String, String, String),
    Request(String),
    Receipt(i64),
    Expand(Option<i64>),
}

impl App {
    fn live_timeline(&mut self, ui: &mut Ui, people: &[String], wide: bool) {
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.field)
            .corner_radius(7)
            .inner_margin(11)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if wide {
                    ui.horizontal(|ui| {
                        theme::glyph(ui, Icon::Activity, p.accent);
                        ui.label(RichText::new("Timeline").size(17.).strong());
                    });
                    ui.add_space(3.);
                }
                let before = (self.live_board.filter, self.live_board.person.clone());
                let end = ui.horizontal_wrapped(|ui| {
                    let name = Filter::OPTIONS
                        .iter()
                        .find(|(f, _)| *f == self.live_board.filter)
                        .unwrap()
                        .1;
                    egui::ComboBox::from_id_salt("live-event-type")
                        .width(124.)
                        .selected_text(name)
                        .show_ui(ui, |ui| {
                            for (filter, name) in Filter::OPTIONS {
                                ui.selectable_value(&mut self.live_board.filter, filter, name);
                            }
                        });
                    let person = if self.live_board.person.is_empty() {
                        "Everyone".to_owned()
                    } else {
                        self.label(&self.live_board.person).to_owned()
                    };
                    egui::ComboBox::from_id_salt("live-event-person")
                        .width(105.)
                        .selected_text(excerpt(&person, 18))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.live_board.person,
                                String::new(),
                                "Everyone",
                            );
                            for id in people {
                                let name = self.label(id).to_owned();
                                ui.selectable_value(&mut self.live_board.person, id.clone(), name);
                            }
                        });
                    if before.0 != self.live_board.filter || before.1 != self.live_board.person {
                        self.live_board.reset_filter();
                    }
                    self.live_board.follow_control(ui, &self.events, self.cursor)
                }).inner;
                ui.separator();
                let start = end.saturating_sub(self.live_board.limit);
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("live-event-rows", &self.work))
                    .min_scrolled_height(0.)
                    .auto_shrink([false, false]);
                if std::mem::take(&mut self.live_board.jump_to_latest) {
                    scroll = scroll.vertical_scroll_offset(0.);
                }
                let mut action = None;
                let result = scroll.show(ui, |ui| {
                    if end == 0 {
                        ui.add_space(14.);
                        ui.strong("No events in this view");
                        if self.live_board.filter == Filter::All
                            && self.live_board.person.is_empty()
                        {
                            ui.weak("Activity will appear here when the runtime records an event.");
                        } else if ui.link("Show all events").clicked() {
                            self.live_board.filter = Filter::All;
                            self.live_board.person.clear();
                            self.live_board.reset_filter();
                        }
                        return;
                    }
                    let mut previous_date = None;
                    for i in self.live_board.matches[start..end].iter().rev() {
                        let event = &self.events[*i];
                        let date = s(event, "created_utc").get(..10).unwrap_or("");
                        if !date.is_empty() && previous_date != Some(date) {
                            ui.add_space(4.);
                            small_text(ui, format!("{date}   UTC"));
                            ui.add_space(1.);
                            previous_date = Some(date);
                        }
                        ui.push_id(event["event"].as_i64(), |ui| {
                            if let Some(selected) = self.timeline_event(ui, event) {
                                action = Some(selected);
                            }
                        });
                        ui.separator();
                    }
                    if start > 0
                        && ui
                            .button(format!("Show {} earlier", PAGE.min(start)))
                            .clicked()
                    {
                        self.live_board.limit = self.live_board.limit.saturating_add(PAGE);
                    }
                });
                if self.live_board.paused_at.is_none() && result.state.offset.y > 32. {
                    self.live_board.paused_at = Some(self.cursor);
                }
                match action {
                    Some(EventAction::Open(reference)) => self.open_resource(ui.ctx(), reference),
                    Some(EventAction::SourceFile { source, object, name }) => {
                        self.open_named_resource(ui.ctx(), object, Some((source, name)))
                    }
                    Some(EventAction::Profile(person)) => self.show_person(&person),
                    Some(EventAction::Address(person)) => self.address_in_live(&person),
                    Some(EventAction::Reply(person, reference, quote)) => {
                        self.reply_in_live(&person, &reference, &quote)
                    }
                    Some(EventAction::Request(id)) => self.show_human_request(&id),
                    Some(EventAction::Receipt(seq)) => self.open_message_receipt(ui.ctx(), seq),
                    Some(EventAction::Expand(seq)) => {
                        if seq.is_some() {
                            self.live_board.paused_at.get_or_insert(self.cursor);
                        }
                        self.live_board.expanded = seq;
                    }
                    None => {}
                }
            });
    }

    fn timeline_event(&self, ui: &mut Ui, event: &Value) -> Option<EventAction> {
        let p = theme::palette(ui);
        let seq = event["event"].as_i64().unwrap_or(0);
        let kind = s(event, "kind");
        let body = &event["body"];
        let result = &event["tool_outcome"];
        let actor = s(event, "actor");
        let persona = event["invocation"].is_string() && actor != "runtime";
        let who = if persona {
            self.label(actor).to_owned()
        } else if actor == "runtime" {
            "Runtime".into()
        } else {
            let operator = actor.strip_prefix("operator:").unwrap_or(actor);
            if operator == self.operator || operator == "user" {
                "You".into()
            } else {
                format!("Operator {operator}")
            }
        };
        let tint = if persona {
            p.ink(self.color(actor))
        } else if actor == "runtime" {
            p.muted
        } else {
            p.accent
        };
        let heading = self.event_heading(event);
        let reference = || format!("experience:event-{seq}@1");
        let mut action = None;
        ui.add_space(3.);
        ui.horizontal_top(|ui| {
            if persona {
                if avatar(ui, &who, tint, 26.)
                    .on_hover_text(format!("Open {who}"))
                    .clicked()
                {
                    action = Some(EventAction::Profile(actor.into()));
                }
            } else {
                let (r, _) = ui.allocate_exact_size(egui::vec2(26., 26.), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 13., p.panel);
                theme::paint_icon(
                    ui.painter(),
                    r.shrink(5.),
                    if actor == "runtime" {
                        Icon::Activity
                    } else {
                        Icon::People
                    },
                    tint,
                );
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let time_width = 62.;
                    let info_width =
                        (ui.available_width() - time_width - ui.spacing().item_spacing.x).max(26.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(info_width, 0.),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            ui.set_min_width(info_width);
                            ui.label(RichText::new(&who).size(14.).strong().color(tint))
                                .on_hover_text(actor);
                            ui.label(RichText::new(heading).size(13.).color(p.muted));
                        },
                    );
                    let stamp = time_label(event["created"].as_i64().unwrap_or(0));
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(stamp.trim_end_matches(" UTC"))
                                    .monospace()
                                    .size(11.)
                                    .color(p.muted),
                            )
                            .small()
                            .frame(false),
                        )
                        .on_hover_text(format!("{} · open saved event", s(event, "created_utc")))
                        .clicked()
                    {
                        action = Some(EventAction::Open(reference()));
                    }
                });
                let text = event_text(event);
                let expanded = self.live_board.expanded == Some(seq);
                let limit = if ui.available_width() < 400. {
                    145
                } else {
                    220
                };
                let full_text = text.lines().nth(1).is_some() || text.chars().nth(limit).is_some();
                if !text.is_empty() {
                    if expanded {
                        document::message_markdown(ui, text);
                    } else {
                        let key = egui::Id::new(("timeline-message-preview", kind, s(event, "digest"), limit));
                        let preview = ui.ctx().data_mut(|cache| {
                            cache.get_temp_mut_or_insert_with(key, || document::excerpt(text, limit)).clone()
                        });
                        ui.add(
                            egui::Label::new(RichText::new(preview).size(14.))
                                .wrap(),
                        );
                    }
                }
                if let Some(file) = result["file_reference"].as_str() {
                    let path = result["file_path"].as_str().unwrap_or(file);
                    ui.horizontal(|ui| {
                        theme::glyph(ui, Icon::Files, p.accent);
                        if ui
                            .add(
                                egui::Label::new(RichText::new(path).color(p.accent).size(13.))
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text(format!("{path}\n{file}"))
                            .clicked()
                        {
                            action = Some(EventAction::Open(file.into()));
                        }
                    });
                }
                if human_file_count(event) > 0 {
                    let source = reference();
                    if let Some((object, name)) =
                        super::overview::source_file_links(ui, &body["attachments"], &source)
                    {
                        action = Some(EventAction::SourceFile { source, object, name });
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if full_text
                        && text_action(ui, if expanded { "Show less" } else { "Read more" })
                            .clicked()
                    {
                        action = Some(EventAction::Expand(if expanded { None } else { Some(seq) }));
                    }
                    if persona && event["target"] == "user" && matches!(kind, "message" | "reply") {
                        if text_action(ui, "Reply").clicked() {
                            action = Some(EventAction::Reply(
                                actor.into(),
                                reference(),
                                document::excerpt(text, 180),
                            ));
                        }
                    } else if persona
                        && matches!(
                            kind,
                            "message" | "reply" | "request_reply" | "requester_message"
                        )
                        && self.records("participation").iter().any(|r| {
                            r["body"]["work"] == self.work && r["body"]["persona"] == actor
                        })
                        && text_action(ui, &format!("Message {who}")).clicked()
                    {
                        action = Some(EventAction::Address(actor.into()));
                    }
                    if kind == "user_message"
                        && event["invocation"].is_null()
                        && text_action(ui, "Delivery status").clicked()
                    {
                        action = Some(EventAction::Receipt(seq));
                    }
                    if let Some(parent) = body["reply_to"].as_str() {
                        if text_action(ui, "In reply to")
                            .on_hover_text(parent)
                            .clicked()
                        {
                            action = Some(EventAction::Open(parent.into()));
                        }
                    }
                    if matches!(
                        kind,
                        "human_request"
                            | "human_request_progress"
                            | "human_request_withdrawn"
                            | "user_message"
                    ) {
                        if let Some(id) = body["human_request"]
                            .as_str()
                            .or(body["reference"].as_str())
                            .and_then(|r| r.strip_prefix("human_request:"))
                            .and_then(|r| r.rsplit_once('@'))
                            .map(|(id, _)| id)
                        {
                            if text_action(ui, "Open request").clicked() {
                                action = Some(EventAction::Request(id.into()));
                            }
                        }
                    }
                    if kind == "tool_result" {
                        if let Some(result) = result["result"].as_str() {
                            if text_action(ui, "Tool receipt").clicked() {
                                action = Some(EventAction::Open(result.into()));
                            }
                        }
                    }
                    if matches!(kind, "request_delivery" | "delivery") {
                        if let Some(package) =
                            body["delivery"].as_str().or(body["reference"].as_str())
                        {
                            if text_action(ui, "Open package").clicked() {
                                action = Some(EventAction::Open(package.into()));
                            }
                        }
                    }
                });
            });
        });
        action
    }

    fn event_heading(&self, event: &Value) -> String {
        let kind = s(event, "kind");
        let body = &event["body"];
        let target = match recipient(event) {
            "user" => "you",
            "*" => "everyone",
            who => self.label(who),
        };
        if kind == "user_message" && s(body, "text").trim().is_empty() {
            let files = human_file_count(event);
            if files > 0 {
                return format!("sent {} to {target}", counted(files as u64, "file", "files"));
            }
        }
        match kind {
            "message" | "user_message" | "requester_message" | "request_message_sent" => {
                format!("sent a message to {target}")
            }
            "reply" | "request_reply" => format!("replied to {target}"),
            "initialization_note" => "recorded an initialization note".into(),
            "human_request" => "requested your help".into(),
            "human_request_progress" => "updated a request".into(),
            "human_request_withdrawn" => "withdrew a request".into(),
            "invitation" => format!("invited {target}"),
            "invitation_response" if body["accept"] == true => "accepted the invitation".into(),
            "invitation_response" if body["accept"] == false => "declined the invitation".into(),
            "commitment_changed" => "updated a commitment".into(),
            "contribution_proposal" => format!("proposed a contribution to {target}"),
            "participation_left" => "left the activity".into(),
            "outcome_changed" => "updated an outcome".into(),
            "delivery" if event["target"] == "*" => "announced a delivery".into(),
            "delivery" => format!("delivered a package to {target}"),
            "request_delivery" => format!("returned a delivery to {target}"),
            "package_review" => "reviewed a package".into(),
            "user_acceptance" if body["decision"]["accepted"] == true => {
                "accepted a delivery".into()
            }
            "user_acceptance" if body["decision"]["accepted"] == false => {
                "requested delivery changes".into()
            }
            "asset_feedback" => "shared feedback on a file".into(),
            "provider_failure" => format!("reported a provider failure for {target}"),
            "decision_rejected" => format!("rejected a decision from {target}"),
            "requirements_amended" => "amended the brief".into(),
            "activity_dissolved" => "ended the activity".into(),
            "activity_reopened" => "reopened the activity".into(),
            "tool_result" => {
                let result = &event["tool_outcome"];
                let tool = s(body, "tool");
                match (
                    tool,
                    result["file_reference"].is_string(),
                    s(result, "label"),
                ) {
                    ("asset.register", true, _) => "registered a file".into(),
                    ("asset.save", true, _) => "saved a file revision".into(),
                    ("asset.import", true, _) => "imported a file".into(),
                    ("asset.organize", true, _) => "updated a file record".into(),
                    (_, true, _) => format!("retained a file with {tool}"),
                    (_, _, "" | "Recorded") => format!("used {tool}"),
                    (_, _, result) => format!("{tool} · {result}"),
                }
            }
            _ => requested_activities::label(kind)
                .map_or_else(|| title(kind), |(name, _)| name.into()),
        }
    }
}

fn event_text(event: &Value) -> &str {
    let body = &event["body"];
    if event["kind"] == "delivery" && body["record"]["title"].is_string() {
        return s(&body["record"], "title");
    }
    if event["kind"] == "user_acceptance" {
        return s(&body["decision"], "reason");
    }
    if event["kind"] == "tool_result" {
        return s(&event["tool_outcome"], "error");
    }
    [
        "text",
        "title",
        "description",
        "commitment",
        "contribution",
        "reason",
        "action",
        "error",
        "criterion",
    ]
    .into_iter()
    .find_map(|key| body[key].as_str().filter(|v| !v.is_empty()))
    .unwrap_or("")
}

fn text_action(ui: &mut Ui, name: &str) -> egui::Response {
    ui.link(RichText::new(name).size(12.))
}
