use super::*;

#[derive(Default)]
pub(super) struct Home {
    query: String,
    kind: ActivityFilter,
    activity_page: usize,
    search_generation: Option<u64>,
    matching: Vec<usize>,
    request_page: usize,
    delivery_page: usize,
    pub(super) message_work: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum ActivityFilter {
    #[default]
    All,
    Tasks,
    Conversations,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    People,
    Feed,
    Activities,
    Requests,
    Deliveries,
}

impl Section {
    fn jump_here(self, ui: &Ui, requested: Option<Self>) {
        if requested == Some(self) {
            ui.scroll_to_cursor(Some(egui::Align::Min));
        }
    }
}

enum SelectedDelivery {
    Package(String, String),
    File { file: String, delivery: String, work: String },
    SaveZip(String, String),
    Handoff(String, String, usize),
}

const RECENT_ROWS: usize = 2;
const ACTIVITY_ROWS: usize = 6;

impl App {
    pub(super) fn workspace_home(&mut self, ui: &mut Ui) {
        self.overview_composer(ui);
        let jump = self.home_sections(ui);
        egui::ScrollArea::vertical()
            .id_salt("workspace-overview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.home_heading(ui);
                if self.snapshot.status.is_null() {
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        if self.error.is_some() {
                            ui.label("Workspace data is unavailable.");
                        } else {
                            ui.spinner();
                            ui.label("Opening workspace…");
                        }
                    });
                    return;
                }
                ui.add_space(14.);
                Section::People.jump_here(ui, jump);
                let width = ui.available_width();
                let handoffs =
                    !self.home_index.requests.is_empty() || !self.home_index.deliveries.is_empty();
                if width >= 1080. && handoffs {
                    let gap = 16.;
                    let live_width = (width - gap) * 0.58;
                    let spacing = ui.spacing().item_spacing.x;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.x = spacing;
                            ui.set_width(live_width);
                            self.workspace_live_people(ui);
                            ui.add_space(14.);
                            Section::Feed.jump_here(ui, jump);
                            self.workspace_updates(ui);
                        });
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.x = spacing;
                            ui.set_width((width - gap - live_width).max(1.));
                            self.home_handoffs(ui, jump);
                        });
                    });
                } else {
                    self.workspace_live_people(ui);
                    ui.add_space(14.);
                    self.home_handoffs(ui, jump);
                    ui.add_space(14.);
                    Section::Feed.jump_here(ui, jump);
                    self.workspace_updates(ui);
                }
                ui.add_space(16.);
                Section::Activities.jump_here(ui, jump);
                self.home_activities(ui);
                ui.add_space(8.);
            });
    }

    fn home_handoffs(&mut self, ui: &mut Ui, jump: Option<Section>) {
        let requests = !self.home_index.requests.is_empty();
        let deliveries = !self.home_index.deliveries.is_empty();
        if requests {
            Section::Requests.jump_here(ui, jump);
            self.home_requests(ui);
            ui.add_space(10.);
        }
        if deliveries {
            Section::Deliveries.jump_here(ui, jump);
            self.home_deliveries(ui);
        } else if requests {
            quiet_queue(ui, Icon::Files, "No packages delivered yet.");
        }
        if !requests {
            let state = if deliveries {
                "No open human requests."
            } else {
                "No open human requests or delivery packages yet."
            };
            quiet_queue(ui, Icon::Questions, state);
        }
    }

    fn home_sections(&self, ui: &mut Ui) -> Option<Section> {
        if self.snapshot.status.is_null() {
            return None;
        }
        let mut jump = None;
        ui.horizontal_wrapped(|ui| {
            ui.weak("Jump to");
            let items = [
                (
                    Section::People,
                    "People",
                    Some(self.records("personas").len()),
                ),
                (Section::Feed, "Workspace activity", None),
                (
                    Section::Requests,
                    "Human requests",
                    Some(self.home_index.requests.len()),
                ),
                (
                    Section::Deliveries,
                    "Deliveries",
                    Some(self.home_index.deliveries.len()),
                ),
                (
                    Section::Activities,
                    "Activities",
                    Some(self.home_index.activities.len()),
                ),
            ];
            for (section, title, count) in items {
                if count == Some(0) && matches!(section, Section::Requests | Section::Deliveries) {
                    continue;
                }
                let label = count.map_or_else(|| title.to_owned(), |n| format!("{title} ({n})"));
                if ui.link(label).clicked() {
                    jump = Some(section);
                }
            }
        });
        ui.separator();
        jump
    }

    fn home_heading(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui);
        let mut open = None;
        egui::Frame::new()
            .fill(p.header)
            .stroke(Stroke::new(1., p.border))
            .corner_radius(9)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let recent = self
                    .home_index
                    .activities
                    .first()
                    .is_some_and(|activity| activity.latest_utc.is_some());
                if ui.available_width() >= 740. && recent {
                    ui.columns(2, |columns| {
                        self.home_identity(&mut columns[0]);
                        let activity = &self.home_index.activities[0];
                        let ui = &mut columns[1];
                        eyebrow(ui, "Latest activity");
                        if title_link(ui, &activity.title, &activity.id).clicked() {
                            open = Some(activity.id.clone());
                        }
                        if let Some(time) = &activity.latest_utc {
                            ui.small(time);
                        }
                    });
                } else {
                    self.home_identity(ui);
                }
            });
        if let Some(work) = open {
            self.open_home_activity(&work);
        }
    }

    fn home_identity(&mut self, ui: &mut Ui) {
        let name = self
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let generic = |name: &str| {
            name.is_empty()
                || matches!(
                    name.to_ascii_lowercase().as_str(),
                    "state" | "workspace" | "runtime"
                )
        };
        let title = if generic(name) {
            self.snapshot
                .directory
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !generic(name))
                .unwrap_or("Workspace overview")
        } else {
            name
        }
        .to_owned();
        if title != "Workspace overview" {
            eyebrow(ui, "Workspace overview");
        }
        ui.add(egui::Label::new(RichText::new(&title).size(28.).strong()).wrap())
            .on_hover_text(self.root.display().to_string());
        if !self.snapshot.status.is_null() {
            ui.add_space(4.);
            ui.horizontal_wrapped(|ui| self.workspace_run_controls(ui));
        }
    }

    fn home_activity_title<'a>(&'a self, work: &'a str) -> &'a str {
        self.home_index
            .activity(work)
            .map(|activity| activity.title.as_str())
            .unwrap_or(work)
    }

    fn home_requests(&mut self, ui: &mut Ui) {
        let indices = &self.home_index.requests;
        let mut page = self
            .home
            .request_page
            .min(last_page(indices.len(), RECENT_ROWS));
        let mut selected = None;
        let mut handoff = None;
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            panel_heading(
                ui,
                Icon::Questions,
                theme::ACCENT,
                "Human requests",
                indices.len(),
            );
            if indices.is_empty() {
                ui.weak("No human requests are open or in progress.");
                return;
            }
            let records = self.records("human_requests");
            recent_rows(ui, indices, records, page, |ui, record| {
                let body = &record["body"];
                let id = s(&record["reference"], "id");
                ui.push_id(("home-request", id), |ui| {
                    let (state, color) = if body["state"] == "in_progress" {
                        ("In progress", TEAL)
                    } else {
                        ("Open", theme::ACCENT)
                    };
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(ui, state, color);
                        if let Some(owner) = body["claimed_by"].as_str() {
                            ui.small(format!("Taken by {owner}"));
                        }
                        if ui
                            .small_button("Save handoff…")
                            .on_hover_text("Save the printable request and its attached files")
                            .clicked()
                        {
                            let files = body["artifacts"].as_array().map_or(0, Vec::len);
                            handoff =
                                Some((inbox::handle(record), s(body, "title").to_owned(), files));
                        }
                    });
                    if title_link(ui, s(body, "title"), &inbox::handle(record)).clicked() {
                        selected = Some(id.to_owned());
                    }
                    let action = s(body, "action");
                    if !action.is_empty() && action != s(body, "title") {
                        ui.label(short_text(action, row_limit(ui, 1.5)))
                            .on_hover_text(action);
                    }
                    provenance(
                        ui,
                        "From",
                        self.label(s(body, "requester")),
                        self.home_activity_title(s(body, "work")),
                    );
                });
            });
            pager(ui, &mut page, indices.len(), RECENT_ROWS, "requests");
        });
        self.home.request_page = page;
        if let Some(id) = selected {
            self.show_human_request(&id);
        } else if let Some((reference, title, files)) = handoff {
            self.save_request_handoff(&reference, &title, files);
        }
    }

    fn home_deliveries(&mut self, ui: &mut Ui) {
        let indices = &self.home_index.deliveries;
        let mut page = self
            .home
            .delivery_page
            .min(last_page(indices.len(), RECENT_ROWS));
        let mut selected = None;
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            panel_heading(ui, Icon::Files, TEAL, "Recent deliveries", indices.len());
            if indices.is_empty() {
                ui.weak("No packages have been delivered in this workspace.");
                return;
            }
            let records = self.records("deliveries");
            recent_rows(ui, indices, records, page, |ui, record| {
                let body = &record["body"];
                let reference = inbox::handle(record);
                let work = s(body, "work");
                ui.push_id(("home-delivery", &reference), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (scope, color) = match s(body, "scope") {
                            "partial" => ("Partial delivery", theme::ACCENT),
                            "claimed_complete" => ("Author claims complete", TEAL),
                            _ => ("Delivered", TEAL),
                        };
                        theme::badge(ui, scope, color);
                        if let Some(gaps) = body["remaining"]
                            .as_array()
                            .filter(|items| !items.is_empty())
                        {
                            ui.small(counted(
                                gaps.len() as u64,
                                "remaining item",
                                "remaining items",
                            ))
                            .on_hover_text(
                                gaps.first().and_then(Value::as_str).unwrap_or_default(),
                            );
                        }
                        if let Some(accepted) = self.snapshot.status["delivery_reviews"][&reference]
                            ["accepted"]
                            .as_bool()
                        {
                            ui.small(if accepted {
                                "Accepted"
                            } else {
                                "Changes requested"
                            });
                        }
                        if ui.small_button("Save handoff…")
                            .on_hover_text("Save the printable delivery sheet and its exact files").clicked() {
                            selected = Some(SelectedDelivery::Handoff(reference.clone(), s(body, "title").into(),
                                body["artifacts"].as_array().map_or(0, Vec::len)));
                        }
                        if ui.small_button("Save files ZIP…")
                            .on_hover_text("Save this edition’s files with their original paths").clicked() {
                            selected = Some(SelectedDelivery::SaveZip(
                                reference.clone(),
                                s(body, "title").to_owned(),
                            ));
                        }
                    });
                    if title_link(ui, s(body, "title"), &reference).clicked() {
                        selected = Some(SelectedDelivery::Package(
                            reference.clone(),
                            work.to_owned(),
                        ));
                    }
                    provenance(
                        ui,
                        "By",
                        self.label(s(body, "owner")),
                        self.home_activity_title(work),
                    );
                    let files = body["artifacts"]
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    if !files.is_empty() {
                        let mut file_list = |ui: &mut Ui| {
                            for file in files.iter().take(6).filter_map(Value::as_str) {
                                let path = self
                                    .snapshot
                                    .assets
                                    .iter()
                                    .find(|asset| asset["reference"] == file)
                                    .and_then(|asset| asset["body"]["path"].as_str())
                                    .unwrap_or(file);
                                ui.horizontal_wrapped(|ui| {
                                    theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                                    if ui
                                        .link(excerpt(path, row_limit(ui, 1.4)))
                                        .on_hover_text(format!("{path}\n{file}"))
                                        .clicked()
                                    {
                                        selected = Some(SelectedDelivery::File {
                                            file: file.into(),
                                            delivery: reference.clone(),
                                            work: work.into(),
                                        });
                                    }
                                    if let Some((_, revision)) = file.rsplit_once('@') {
                                        ui.small(format!("rev {revision}"));
                                    }
                                });
                            }
                            if files.len() > 6
                                && ui
                                    .link(format!("Open all {} files in the package", files.len()))
                                    .clicked()
                            {
                                selected = Some(SelectedDelivery::Package(
                                    reference.clone(),
                                    work.to_owned(),
                                ));
                            }
                        };
                        if files.len() == 1 {
                            file_list(ui);
                        } else {
                            ui.collapsing(format!("{} files", files.len()), file_list);
                        }
                    }
                });
            });
            pager(ui, &mut page, indices.len(), RECENT_ROWS, "deliveries");
        });
        self.home.delivery_page = page;
        match selected {
            Some(SelectedDelivery::Package(reference, work)) => {
                self.open_home_delivery(&reference, &work)
            }
            Some(SelectedDelivery::File { file, delivery, work }) => {
                self.open_home_delivery(&delivery, &work);
                self.open_resource(ui.ctx(), file);
            }
            Some(SelectedDelivery::SaveZip(reference, title)) => {
                self.export_package(&reference, &title)
            }
            Some(SelectedDelivery::Handoff(reference, title, files)) => {
                self.save_delivery_handoff(&reference, &title, files)
            }
            None => {}
        }
    }

    fn home_activities(&mut self, ui: &mut Ui) {
        let mut selected = None;
        let mut create = None;
        panel(ui).show(ui, |ui| {
            let activities = &self.home_index.activities;
            ui.set_width(ui.available_width());
            panel_heading(
                ui,
                Icon::Workspaces,
                theme::palette(ui).accent,
                "Activities",
                activities.len(),
            );
            if activities.is_empty() {
                ui.weak("No activities have been created in this workspace.");
                ui.horizontal_wrapped(|ui| {
                    if ui.add(theme::primary("New task…")).clicked() {
                        create = Some(false);
                    }
                    if ui.button("New conversation…").clicked() {
                        create = Some(true);
                    }
                });
                return;
            }
            if activities.len() > ACTIVITY_ROWS
                || !self.home.query.is_empty()
                || self.home.kind != ActivityFilter::All
            {
                ui.horizontal_wrapped(|ui| {
                    let previous = self.home.kind;
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.home.query)
                            .hint_text("Find an activity or participant…")
                            .desired_width(ui.available_width().min(265.)),
                    );
                    for (value, label) in [
                        (ActivityFilter::All, "All"),
                        (ActivityFilter::Tasks, "Tasks"),
                        (ActivityFilter::Conversations, "Conversations"),
                    ] {
                        ui.selectable_value(&mut self.home.kind, value, label);
                    }
                    if search.changed() || self.home.kind != previous {
                        self.home.activity_page = 0;
                        self.home.search_generation = None;
                    }
                });
                ui.add_space(5.);
            }
            let filtered =
                !self.home.query.trim().is_empty() || self.home.kind != ActivityFilter::All;
            if filtered && self.home.search_generation != Some(self.home_index.generation) {
                let query = self.home.query.trim().to_lowercase();
                let people: BTreeSet<_> = self
                    .records("personas")
                    .iter()
                    .filter_map(|record| {
                        let persona = &record["body"];
                        (!query.is_empty() && s(persona, "label").to_lowercase().contains(&query))
                            .then_some(s(persona, "id"))
                    })
                    .collect();
                self.home.matching = activities
                    .iter()
                    .enumerate()
                    .filter_map(|(index, activity)| {
                        let kind = match self.home.kind {
                            ActivityFilter::All => true,
                            ActivityFilter::Tasks => activity.kind == "task",
                            ActivityFilter::Conversations => activity.kind == "social",
                        };
                        let matches = kind
                            && (query.is_empty()
                                || activity.title.to_lowercase().contains(&query)
                                || activity.id.to_lowercase().contains(&query)
                                || activity.members.iter().any(|id| {
                                    people.contains(id.as_str())
                                        || id.to_lowercase().contains(&query)
                                }));
                        matches.then_some(index)
                    })
                    .collect();
                self.home.search_generation = Some(self.home_index.generation);
            }
            let count = if filtered {
                self.home.matching.len()
            } else {
                activities.len()
            };
            self.home.activity_page = self.home.activity_page.min(last_page(count, ACTIVITY_ROWS));
            if count == 0 {
                ui.weak("No activities match this search.");
                if ui.small_button("Clear filters").clicked() {
                    self.home.query.clear();
                    self.home.kind = ActivityFilter::All;
                    self.home.search_generation = None;
                }
            }
            let start = self.home.activity_page * ACTIVITY_ROWS;
            for offset in start..(start + ACTIVITY_ROWS).min(count) {
                let index = if filtered {
                    self.home.matching[offset]
                } else {
                    offset
                };
                let activity = &activities[index];
                if offset > start {
                    divider(ui);
                }
                ui.push_id(("home-activity", &activity.id), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let social = activity.kind == "social";
                        let p = theme::palette(ui);
                        theme::glyph(
                            ui,
                            if social {
                                Icon::Conversation
                            } else {
                                Icon::Task
                            },
                            p.muted,
                        );
                        ui.small(if social { "Conversation" } else { "Task" });
                        let phase = match activity.phase.as_str() {
                            "dissolved" => "Ended",
                            "open" => "Open",
                            other => other,
                        };
                        if !phase.is_empty() {
                            ui.small(format!("· {phase}"));
                        }
                        if let Some(time) = &activity.latest_utc {
                            ui.small(format!("· {time}"));
                        }
                        if activity.open_requests > 0 {
                            let count = counted(
                                activity.open_requests as u64,
                                "human request",
                                "human requests",
                            );
                            ui.label(RichText::new(count).small().color(p.ink(theme::ACCENT)));
                        }
                    });
                    if title_link(ui, &activity.title, &activity.id).clicked() {
                        selected = Some(activity.id.clone());
                    }
                    if !activity.members.is_empty() {
                        let people = activity
                            .members
                            .iter()
                            .map(|id| self.label(id))
                            .collect::<Vec<_>>()
                            .join(", ");
                        ui.horizontal_wrapped(|ui| {
                            theme::glyph(ui, Icon::People, theme::palette(ui).muted);
                            ui.weak(short_text(&people, row_limit(ui, 1.)))
                                .on_hover_text(people);
                        });
                    }
                });
            }
            pager(
                ui,
                &mut self.home.activity_page,
                count,
                ACTIVITY_ROWS,
                "activities",
            );
        });
        if let Some(social) = create {
            self.dialog = Some(Dialog::activity(social));
        }
        if let Some(work) = selected {
            self.open_home_activity(&work);
        }
    }
}

fn recent_rows(
    ui: &mut Ui,
    indices: &[usize],
    records: &[Value],
    page: usize,
    mut row: impl FnMut(&mut Ui, &Value),
) {
    let start = page * RECENT_ROWS;
    let visible = &indices[start..(start + RECENT_ROWS).min(indices.len())];
    if ui.available_width() >= 750. && visible.len() > 1 {
        ui.columns(visible.len(), |columns| {
            for (column, &index) in columns.iter_mut().zip(visible) {
                if let Some(record) = records.get(index) {
                    row(column, record);
                }
            }
        });
    } else {
        for (offset, &index) in visible.iter().enumerate() {
            if let Some(record) = records.get(index) {
                if offset > 0 {
                    divider(ui);
                }
                row(ui, record);
            }
        }
    }
}

fn quiet_queue(ui: &mut Ui, icon: Icon, text: &str) {
    ui.add_space(4.);
    ui.horizontal_wrapped(|ui| {
        theme::glyph(ui, icon, theme::palette(ui).muted);
        ui.small(text);
    });
}

fn panel(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1., p.border))
        .corner_radius(8)
        .inner_margin(14)
}

fn panel_heading(ui: &mut Ui, icon: Icon, color: Color32, name: &str, count: usize) {
    ui.horizontal_wrapped(|ui| {
        theme::glyph(ui, icon, theme::palette(ui).ink(color));
        ui.label(RichText::new(name).size(19.).strong());
        ui.weak(number(count as u64));
    });
    ui.add_space(5.);
}

fn divider(ui: &mut Ui) {
    ui.add_space(3.);
    ui.separator();
    ui.add_space(3.);
}

fn row_limit(ui: &Ui, lines: f32) -> usize {
    (ui.available_width() / 8. * lines).clamp(20., 140.) as usize
}

pub(super) fn short_text(value: &str, limit: usize) -> String {
    let mut chars = value
        .split_whitespace()
        .flat_map(|word| std::iter::once(' ').chain(word.chars()))
        .skip(1);
    let mut text: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    text
}

fn title_link(ui: &mut Ui, title: &str, reference: &str) -> egui::Response {
    ui.link(
        RichText::new(short_text(title, row_limit(ui, 1.8)))
            .color(theme::palette(ui).accent)
            .size(17.)
            .strong(),
    )
    .on_hover_ui(|ui| {
        ui.label(title);
        ui.small(reference);
    })
}

fn provenance(ui: &mut Ui, role: &str, person: &str, work: &str) {
    let limit = row_limit(ui, 1.4);
    let line = format!(
        "{role} {} · {}",
        short_text(person, limit),
        short_text(work, limit)
    );
    ui.small(short_text(&line, limit)).on_hover_ui(|ui| {
        ui.label(format!("{role} {person}"));
        ui.label(work);
    });
}

fn last_page(total: usize, size: usize) -> usize {
    total.saturating_sub(1) / size
}

pub(super) fn pager(ui: &mut Ui, page: &mut usize, total: usize, size: usize, noun: &str) {
    if total <= size {
        return;
    }
    ui.add_space(6.);
    ui.horizontal_wrapped(|ui| {
        let start = *page * size;
        let first = start + 1;
        let last = (start + size).min(total);
        ui.small(if first == last {
            format!("{first} of {total}")
        } else {
            format!("{first}–{last} of {total}")
        });
        let mut go = |direction: &str, enabled: bool| {
            let response = ui.add_enabled(enabled, egui::Button::new(direction).small());
            let label = format!("{direction} {noun}");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label)
            });
            response.on_hover_text(label).clicked()
        };
        if go("Previous", *page > 0) {
            *page -= 1;
        }
        if go("Next", *page < last_page(total, size)) {
            *page += 1;
        }
    });
}
