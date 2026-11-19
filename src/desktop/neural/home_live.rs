use super::*;
use ai_personas::neural::live::RunnerObservation;

#[derive(Clone, Copy)]
pub(super) enum Observation<'a> {
    Call(&'a Value),
    UnresolvedCall(&'a Value),
    Tool(&'a Value),
}

impl<'a> Observation<'a> {
    fn record(self) -> &'a Value {
        match self {
            Self::Call(record) | Self::UnresolvedCall(record) | Self::Tool(record) => record,
        }
    }

    fn display_order(self, runner: &RunnerObservation) -> u8 {
        let row = self.record();
        match self {
            Self::UnresolvedCall(_) => 3,
            Self::Tool(tool) if tool["state"] == "uncertain" => 3,
            Self::Tool(tool) if tool["state"] == "pending" => 1,
            _ if runner.has_worker(s(row, "work"), s(row, "actor")) => 0,
            _ => 2,
        }
    }

    fn unresolved(self) -> bool {
        matches!(self, Self::UnresolvedCall(_))
            || matches!(self, Self::Tool(tool) if tool["state"] == "uncertain")
    }
}

enum Action {
    Profile(String),
    Activity(String),
    Message { work: String, person: String },
    Resource { work: String, reference: String },
}

#[derive(Default)]
struct Person<'a> {
    name: &'a str,
    registered: bool,
    work: BTreeMap<&'a str, Vec<Observation<'a>>>,
}

fn membership_state(part: &Value, ended: bool) -> &'static str {
    if ended || part["closed_by_activity"].is_string() {
        return "Activity ended";
    }
    if part["departure"].is_string() {
        return "Left activity";
    }
    if part["declined"] == true {
        return "Invitation declined";
    }
    if part["accepted"] != true {
        return "Invited";
    }
    match s(&part["continuation"], "kind") {
        "ready" => "Chose to continue",
        "wait" | "waiting" => "Chose to wait",
        "stop" | "stopped" => "Stopped",
        _ => "Participating",
    }
}

impl App {
    fn observed_people(&self, selected: Option<&str>) -> (BTreeMap<&str, Person<'_>>, usize) {
        let includes = |actor: &str| selected.is_none_or(|id| actor == id);
        let mut people = BTreeMap::new();
        for record in self.records("personas") {
            let person = &record["body"];
            let id = s(person, "id");
            if !includes(id) {
                continue;
            }
            let name = person["label"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(id);
            people.insert(
                id,
                Person {
                    name,
                    registered: true,
                    ..Person::default()
                },
            );
        }
        for observation in self
            .snapshot
            .active
            .iter()
            .map(Observation::Call)
            .chain(
                self.snapshot
                    .uncertain_calls
                    .iter()
                    .map(Observation::UnresolvedCall),
            )
            .chain(self.snapshot.active_effects.iter().map(Observation::Tool))
        {
            let record = observation.record();
            let actor = s(record, "actor");
            if !includes(actor) {
                continue;
            }
            let person = people.entry(actor).or_insert_with(|| Person {
                name: actor,
                ..Person::default()
            });
            person
                .work
                .entry(s(record, "work"))
                .or_default()
                .push(observation);
        }
        // Admission can precede preparation of the first call or tool row.
        let mut admissions = 0;
        if let RunnerObservation::Observed { workers, .. } = &self.runner_observation {
            for (id, work) in workers {
                if !includes(id) {
                    continue;
                }
                let person = people.entry(id.as_str()).or_insert_with(|| Person {
                    name: id,
                    ..Person::default()
                });
                if person
                    .work
                    .entry(work)
                    .or_default()
                    .iter()
                    .all(|o| o.unresolved())
                {
                    admissions += 1;
                }
            }
        }
        (people, admissions)
    }

    pub(super) fn observed_projects(&self, actor: &str) -> BTreeMap<&str, Vec<Observation<'_>>> {
        self.observed_people(Some(actor))
            .0
            .remove(actor)
            .map(|person| person.work)
            .unwrap_or_default()
    }

    pub(super) fn workspace_live_people(&mut self, ui: &mut Ui) {
        let (people, admissions) = self.observed_people(None);
        let mut people: Vec<_> = people.into_iter().collect();
        // Keep a small team's profile and message targets in a stable place as
        // calls begin and end. Surface current work first in a longer directory.
        let prioritize_work = people.len() > 8;
        people.sort_by_cached_key(|(id, person)| {
            let order = if prioritize_work {
                person
                    .work
                    .iter()
                    .map(|(work, observations)| {
                        if self.runner_observation.has_worker(work, id) {
                            0
                        } else {
                            observations
                                .iter()
                                .map(|o| o.display_order(&self.runner_observation))
                                .min()
                                .unwrap_or(4)
                        }
                    })
                    .min()
                    .unwrap_or(4)
            } else {
                0
            };
            (order, person.name.to_lowercase(), *id)
        });
        let p = theme::palette(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("People & live work").size(20.).strong());
            if !people.is_empty() {
                ui.label(
                    RichText::new(counted(people.len() as u64, "person", "people"))
                        .small()
                        .color(p.muted),
                );
            }
        });
        if people.is_empty() {
            ui.weak("No personas in this workspace.");
            return;
        }
        let effects = &self.snapshot.active_effects;
        let worker = |record: &Value| {
            self.runner_observation
                .has_worker(s(record, "work"), s(record, "actor"))
        };
        let observed_calls = self.snapshot.active.iter().filter(|c| worker(c)).count();
        let count = |state| effects.iter().filter(|e| e["state"] == state).count();
        let running_jobs = effects
            .iter()
            .filter(|e| e["state"] == "dispatched" && e["job"].is_string() && worker(e))
            .count();
        let dispatched_tools = effects
            .iter()
            .filter(|e| e["state"] == "dispatched" && worker(e) && !e["job"].is_string())
            .count();
        let mut summary = Vec::new();
        for (n, singular, plural) in [
            (admissions, "admitted participant", "admitted participants"),
            (observed_calls, "live model call", "live model calls"),
            (
                self.snapshot.active.len() - observed_calls,
                "recorded model call",
                "recorded model calls",
            ),
            (dispatched_tools, "dispatched tool", "dispatched tools"),
            (
                running_jobs,
                "running background job",
                "running background jobs",
            ),
            (count("pending"), "queued tool", "queued tools"),
            (
                count("dispatched") - running_jobs - dispatched_tools,
                "recorded tool dispatch",
                "recorded tool dispatches",
            ),
            (
                self.snapshot.uncertain_calls.len() + count("uncertain"),
                "unresolved outcome",
                "unresolved outcomes",
            ),
        ] {
            if n > 0 {
                summary.push(counted(n as u64, singular, plural));
            }
        }
        let summary = if summary.is_empty() {
            "Model calls and tools are idle".into()
        } else {
            summary.join("  ·  ")
        };
        ui.label(RichText::new(summary).small().color(p.muted));
        ui.add_space(4.);
        let width = ui.available_width();
        let columns = if width >= 970. {
            3
        } else if width >= 650. {
            2
        } else {
            1
        };
        let columns = columns.min(people.len());
        let now = ai_personas::neural::now();
        let mut action = None;
        for group in people.chunks(columns) {
            ui.columns(columns, |cells| {
                for (cell, (id, person)) in cells.iter_mut().zip(group) {
                    cell.scope_builder(egui::UiBuilder::new().id(("home-person", id)), |ui| {
                        self.home_person(ui, id, person, now, &mut action);
                    });
                }
            });
        }
        match action {
            Some(Action::Profile(id)) => self.show_person(&id),
            Some(Action::Activity(work)) => self.open_home_activity(&work),
            Some(Action::Message { work, person }) => self.address_on_overview(work, &person),
            Some(Action::Resource { work, reference }) => {
                self.open_home_activity(&work);
                self.open_resource(ui.ctx(), reference);
            }
            None => {}
        }
    }

    fn home_person(
        &self,
        ui: &mut Ui,
        id: &str,
        person: &Person<'_>,
        now: i64,
        action: &mut Option<Action>,
    ) {
        let p = theme::palette(ui);
        let records = self.records("participation");
        let memberships = self
            .home_index
            .participations
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let member_rows = || memberships.iter().map(|i| &records[*i]["body"]);
        let mut shown = 0;
        let admitted = person.work.keys().any(|work| self.runner_observation.has_worker(work, id));
        let outline = if admitted { p.ink(self.color(id)) } else { p.border };
        let frame = egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(if admitted { 1.6 } else { 1. }, outline))
            .corner_radius(7)
            .inner_margin(10);
        frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                let name = RichText::new(person.name).size(16.).strong();
                if person.registered {
                    let label = format!("Open {}’s profile", person.name);
                    let mark = avatar(ui, person.name, p.ink(self.color(id)), 28.);
                    mark.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
                    });
                    if mark.has_focus() {
                        ui.painter().circle_stroke(
                            mark.rect.center(),
                            15.,
                            Stroke::new(2., p.accent),
                        );
                    }
                    let pointer_click =
                        egui::Sense::click() - egui::Sense::focusable_noninteractive();
                    let title = ui
                        .add(egui::Label::new(name).truncate().sense(pointer_click))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&label);
                    if mark.on_hover_text(label).clicked() || title.clicked() {
                        *action = Some(Action::Profile(id.into()));
                    }
                } else {
                    theme::glyph(ui, Icon::People, p.muted);
                    ui.label(name);
                }
            });
            let mut projects: Vec<_> = person.work.iter().collect();
            if projects.len() > 8 {
                projects.sort_by_key(|(_, observations)| {
                    observations
                        .iter()
                        .map(|o| o.display_order(&self.runner_observation))
                        .min()
                });
            }
            for (index, (work, observations)) in projects.into_iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                let member = self
                    .home_index
                    .activity(work)
                    .is_some_and(|a| a.members.iter().any(|other| other == id));
                shown += usize::from(member);
                self.home_activity_line(ui, work, id, person.name, member, action);
                if let Some(reference) = self.work_observations(ui, id, work, observations, now) {
                    *action = Some(Action::Resource {
                        work: (*work).into(),
                        reference,
                    });
                }
            }
            let mut primary = None;
            if person.work.is_empty() {
                // The index orders participations by the most recent activity
                // event. Prefer an ongoing membership to a historical one.
                primary = member_rows()
                    .find(|part| {
                        matches!(
                            self.home_membership_state(part),
                            "Invited" | "Chose to wait" | "Chose to continue" | "Participating"
                        )
                    })
                    .or_else(|| {
                        member_rows().find(|part| self.home_membership_state(part) == "Stopped")
                    })
                    .or_else(|| member_rows().next());
                if let Some(first) = primary {
                    shown += 1;
                    self.home_membership(ui, first, id, person.name, action);
                } else {
                    ui.label(
                        RichText::new("Idle · No activity participation")
                            .small()
                            .color(p.muted),
                    );
                }
            }
            let others = memberships.len().saturating_sub(shown);
            if others > 0 {
                let label = counted(others as u64, "other activity", "other activities");
                egui::CollapsingHeader::new(RichText::new(label).small())
                    .id_salt("other-activities")
                    .show(ui, |ui| {
                        let primary_work = primary.map(|part| s(part, "work"));
                        let remaining = member_rows().filter(|part| {
                            let work = s(part, "work");
                            !person.work.contains_key(work) && Some(work) != primary_work
                        });
                        for (i, part) in remaining.enumerate() {
                            if i > 0 {
                                ui.separator();
                            }
                            self.home_membership(ui, part, id, person.name, action);
                        }
                    });
            }
        });
    }

    pub(super) fn work_observations(
        &self,
        ui: &mut Ui,
        actor: &str,
        work: &str,
        observations: &[Observation<'_>],
        now: i64,
    ) -> Option<String> {
        let worker = self.runner_observation.has_worker(work, actor);
        if worker && observations.iter().all(|o| o.unresolved()) {
            ui.horizontal(|ui| theme::badge(ui, "Admitted by runner", TEAL));
        }
        let mut resource = None;
        for (n, observation) in observations.iter().enumerate() {
            if n > 0 {
                ui.add_space(4.);
            }
            ui.push_id(("observation", s(observation.record(), "id")), |ui| {
                observation_line(ui, *observation, now, worker, &mut resource);
            });
        }
        resource
    }

    fn home_membership_state(&self, part: &Value) -> &'static str {
        let ended = self
            .home_index
            .activity(s(part, "work"))
            .is_some_and(|w| w.phase == "dissolved");
        membership_state(part, ended)
    }

    fn home_membership(
        &self,
        ui: &mut Ui,
        part: &Value,
        id: &str,
        name: &str,
        action: &mut Option<Action>,
    ) {
        let work = s(part, "work");
        self.home_activity_line(ui, work, id, name, true, action);
        ui.label(
            RichText::new(self.membership_summary(part))
                .small()
                .color(theme::palette(ui).muted),
        );
    }

    pub(super) fn membership_summary(&self, part: &Value) -> String {
        let work = s(part, "work");
        let id = s(part, "persona");
        let state = self.home_membership_state(part);
        let control = self.records("controls").iter().find(|control| {
            let scope = s(control, "scope");
            control["state"] != "running"
                && (matches!(scope, "admission" | "global")
                    || scope.strip_prefix("persona:") == Some(id)
                    || scope.strip_prefix("work:") == Some(work))
        });
        let mut text = state.to_owned();
        if matches!(
            state,
            "Chose to wait" | "Chose to continue" | "Invited" | "Participating"
        ) {
            if let Some(control) = control {
                text.push_str(&format!("  ·  Scheduling {}", s(control, "state")));
            }
        }
        text
    }

    fn home_activity_line(
        &self,
        ui: &mut Ui,
        work: &str,
        id: &str,
        name: &str,
        member: bool,
        action: &mut Option<Action>,
    ) {
        let activity = self.home_index.activity(work);
        let title = activity.map(|a| a.title.as_str()).unwrap_or(work);
        let title = if title.is_empty() {
            "Activity unavailable"
        } else {
            title
        };
        ui.push_id(work, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 20.),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if member && activity.is_some() {
                        let button = ui.small_button("Message");
                        button.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                format!("Message {name} on {title}"),
                            )
                        });
                        if button
                            .on_hover_text(format!("Write to {name} here while following the workspace\n{title}"))
                            .clicked()
                        {
                            *action = Some(Action::Message {
                                work: work.into(),
                                person: id.into(),
                            });
                        }
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width().max(1.), 20.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            if activity.is_some() {
                                if ui
                                    .link(title)
                                    .on_hover_text(format!("{title}\n{work}"))
                                    .clicked()
                                {
                                    *action = Some(Action::Activity(work.into()));
                                }
                            } else {
                                ui.weak(title);
                            }
                        },
                    );
                },
            );
        });
    }
}

fn observation_line(
    ui: &mut Ui,
    observation: Observation<'_>,
    now: i64,
    worker: bool,
    resource: &mut Option<String>,
) {
    let p = theme::palette(ui);
    let muted = |ui: &mut Ui, text: String| ui.label(RichText::new(text).small().color(p.muted));
    match observation {
        Observation::Call(call) => {
            let state = s(call, "state");
            let badge = if !worker || matches!(state, "prepared" | "probe_prepared") {
                p.muted
            } else {
                TEAL
            };
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, live_board::call_stage(call, now, worker), badge);
            });
            if let (Some(bytes), Some(at)) =
                (call["stream_bytes"].as_u64(), call["observed_at"].as_i64())
            {
                muted(
                    ui,
                    format!(
                        "{} B received  ·  {}  ·  {} ago",
                        number(bytes),
                        time_label(at),
                        live_board::age(now.saturating_sub(at))
                    ),
                )
                .on_hover_text(format!(
                    "Response bytes observed at {}\n{}",
                    time_label(at),
                    s(call, "id")
                ));
            } else if let Some(at) = call["created"].as_i64() {
                let when = format!("Prepared {} ago", live_board::age(now.saturating_sub(at)));
                let text = if matches!(state, "dispatched" | "probe_dispatched") {
                    format!("{when}  ·  No response bytes observed")
                } else {
                    when
                };
                muted(ui, text).on_hover_text(format!(
                    "Prepared at {}\n{}",
                    time_label(at),
                    s(call, "id")
                ));
            }
            live_board::draft::home_reply(ui, call, worker);
        }
        Observation::UnresolvedCall(call) => {
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, live_board::call_stage(call, now, false), theme::ACCENT);
            });
            if let Some(at) = call["updated"].as_i64() {
                muted(
                    ui,
                    format!("Recorded {} ago", live_board::age(now.saturating_sub(at))),
                )
                .on_hover_text(format!("{}\n{}", time_label(at), s(call, "id")));
            }
        }
        Observation::Tool(tool) => {
            let label = live_board::effect_stage(tool, worker);
            let tint = match s(tool, "state") {
                "uncertain" => theme::ACCENT,
                "dispatched" if worker => TEAL,
                _ => p.muted,
            };
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, label, tint);
                if let Some(reference) = tool["job"].as_str().or(tool["operation"].as_str()) {
                    if ui
                        .link(RichText::new(s(tool, "tool")).small())
                        .on_hover_text(reference)
                        .clicked()
                    {
                        *resource = Some(reference.into());
                    }
                } else {
                    muted(ui, s(tool, "tool").into());
                }
            });
            if let Some(at) = tool["started"].as_i64() {
                muted(
                    ui,
                    format!("Started {} ago", live_board::age(now.saturating_sub(at))),
                )
                .on_hover_text(time_label(at));
            }
            if !tool["cancellation"].is_null() {
                muted(ui, "Cancellation requested".into());
            }
        }
    }
}
