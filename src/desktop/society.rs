//! Linked task/persona views built only from retained runtime observations.
use super::*;
use std::collections::BTreeMap;

impl App {
    pub(super) fn overview(&mut self, ui: &mut Ui) {
        let works = models_at(&self.view.inspection, "works").to_vec();
        let mut people = BTreeMap::new();
        if let Some(labels) = self.view.inspection["settings"]["persona_labels"].as_object() {
            for (id, label) in labels {
                people.insert(id.clone(), label.as_str().unwrap_or(id).to_owned());
            }
        }
        for work in &works {
            for person in models_at(work, "participants") {
                people.insert(
                    s(person, "persona").to_owned(),
                    s(person, "label").to_owned(),
                );
            }
        }
        theme::header(
            ui,
            Icon::Workspaces,
            "Workspace overview",
            "TASKS × PERSONAS",
        );
        ui.horizontal_wrapped(|ui| {
            theme::badge(ui, &format!("{} tasks", works.len()), theme::BLUE);
            theme::badge(ui, &format!("{} personas", people.len()), theme::ACCENT);
            theme::badge(
                ui,
                &format!(
                    "{} open",
                    works.iter().filter(|w| w["phase"] == "open").count()
                ),
                theme::GREEN,
            );
        });
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.overview_filter)
                .hint_text("Find a task or persona…")
                .desired_width(f32::INFINITY),
        );
        ui.weak("Select a task to follow its conversation and outputs. Select a persona to inspect their character.");
        let filter = self.overview_filter.to_lowercase();
        let mut selected = None;
        let mut profile = None;
        egui::ScrollArea::horizontal()
            .id_salt("society-matrix")
            .show(ui, |ui| {
                egui::Grid::new("society-matrix-grid")
                    .striped(true)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        ui.strong("Task / current phase");
                        for (id, label) in &people {
                            if ui.button(label).on_hover_text(id).clicked() {
                                profile = Some(id.clone());
                            }
                        }
                        ui.end_row();
                        for work in &works {
                            let participants = models_at(work, "participants");
                            if !filter.is_empty()
                                && !s(work, "need").to_lowercase().contains(&filter)
                                && !s(work, "id").to_lowercase().contains(&filter)
                                && !participants
                                    .iter()
                                    .any(|p| s(p, "label").to_lowercase().contains(&filter))
                            {
                                continue;
                            }
                            ui.vertical(|ui| {
                                ui.set_max_width(230.0);
                                if ui
                                    .add(egui::Button::new(short(s(work, "need"), 90)).wrap())
                                    .on_hover_text(s(work, "need"))
                                    .clicked()
                                {
                                    selected = Some(s(work, "id").to_owned());
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        s(work, "id"),
                                        s(work, "phase")
                                    ))
                                    .small()
                                    .weak(),
                                );
                            });
                            for id in people.keys() {
                                if let Some(person) =
                                    participants.iter().find(|p| s(p, "persona") == id)
                                {
                                    let label = if !person["active_invocation"].is_null() {
                                        "Working".into()
                                    } else {
                                        theme::continuation_label(s(person, "continuation"))
                                            .to_owned()
                                    };
                                    if ui
                                        .button(label)
                                        .on_hover_text("Open this task and inspect the participant")
                                        .clicked()
                                    {
                                        selected = Some(s(work, "id").to_owned());
                                        self.inspected_persona = Some(id.clone());
                                    }
                                } else {
                                    ui.weak("—");
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(10.0);
        ui.weak("Participation and execution state are shown as recorded. Open tasks and stopped personas can coexist with delivered results.");
        if let Some(id) = selected {
            self.tab = 0;
            self.conversation_persona = None;
            self.conversation_route = None;
            self.conversation_page = 0;
            self.send(Action::Select(id));
        }
        if let Some(id) = profile {
            self.dialog = Some(Dialog::Development);
            self.send(Action::Development(id));
        }
    }

    pub(super) fn conversation(&mut self, ui: &mut Ui) {
        let work = self.view.selected.clone().unwrap_or_default();
        if self.view.conversation["work"].as_str() != Some(&work) {
            if !self.busy && self.error.is_none() {
                self.send(Action::Conversation(work));
            }
            if self.busy {
                ui.spinner();
            }
            if self.error.is_some() && ui.button("Retry conversation").clicked() {
                self.error = None;
            }
            return;
        }
        let snapshot = self.view.conversation.clone();
        let all = models_at(&snapshot, "messages");
        let participants = models_at(&snapshot, "participants");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.conversation_mode, 0, "Timeline");
            ui.selectable_value(&mut self.conversation_mode, 1, "Collaboration");
            ui.separator();
            ui.label(
                RichText::new(format!("{} messages", all.len()))
                    .small()
                    .weak(),
            );
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.conversation_filter)
                .hint_text("Search messages, subjects, or people…")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("conversation-channel")
                .selected_text(
                    ["All communication", "Between personas", "To you"][self.conversation_channel],
                )
                .show_ui(ui, |ui| {
                    for (n, label) in ["All communication", "Between personas", "To you"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut self.conversation_channel, n, *label);
                    }
                });
            egui::ComboBox::from_id_salt("conversation-person")
                .selected_text(
                    self.conversation_persona
                        .as_deref()
                        .map(|id| self.persona_label(id))
                        .unwrap_or_else(|| "Everyone".into()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.conversation_persona, None, "Everyone");
                    for p in participants {
                        ui.selectable_value(
                            &mut self.conversation_persona,
                            Some(s(p, "persona").to_owned()),
                            s(p, "label"),
                        );
                    }
                });
            if ui.small_button("Clear filters").clicked() {
                self.conversation_filter.clear();
                self.conversation_persona = None;
                self.conversation_route = None;
                self.conversation_page = 0;
                self.conversation_channel = 0;
            }
        });
        let query = self.conversation_filter.to_lowercase();
        if let Some((from, to)) = self.conversation_route.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} → {}",
                    self.persona_label(&from),
                    self.persona_label(&to)
                ));
                if ui.small_button("Show all paths").clicked() {
                    self.conversation_route = None;
                }
            });
        }
        let operator = self
            .view
            .progress
            .as_ref()
            .and_then(|p| p.operator.as_deref())
            .unwrap_or("");
        let messages: Vec<_> =
            all.iter()
                .filter(|m| {
                    let channel = match self.conversation_channel {
                        1 => participants.iter().any(|p| p["persona"] == m["recipient"]),
                        2 => s(m, "recipient") == operator,
                        _ => true,
                    };
                    channel
                        && self.conversation_route.as_ref().is_none_or(|(from, to)| {
                            s(m, "sender") == from && s(m, "recipient") == to
                        })
                        && self
                            .conversation_persona
                            .as_ref()
                            .is_none_or(|id| m["sender"] == *id || m["recipient"] == *id)
                        && (query.is_empty()
                            || ["text", "subject", "sender", "recipient", "id"]
                                .iter()
                                .any(|key| s(m, key).to_lowercase().contains(&query))
                            || self
                                .persona_label(s(m, "sender"))
                                .to_lowercase()
                                .contains(&query)
                            || self
                                .persona_label(s(m, "recipient"))
                                .to_lowercase()
                                .contains(&query))
                })
                .cloned()
                .collect();
        ui.separator();
        if self.conversation_mode == 0 {
            ui.weak(format!(
                "{} shown · accepted call order · arrows identify the actual recipient",
                messages.len()
            ));
            ui.weak("Messages from the same call are ordered by record ID.");
            if messages.is_empty() {
                ui.label(if all.is_empty() {
                    "No messages have been recorded for this task yet."
                } else {
                    "No messages match these filters."
                });
            }
            const PAGE: usize = 50;
            let pages = messages.len().div_ceil(PAGE).max(1);
            self.conversation_page = self.conversation_page.min(pages - 1);
            if pages > 1 {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.conversation_page > 0,
                            egui::Button::new("Earlier messages"),
                        )
                        .clicked()
                    {
                        self.conversation_page -= 1;
                    }
                    ui.label(format!("Page {} / {pages}", self.conversation_page + 1));
                    if ui
                        .add_enabled(
                            self.conversation_page + 1 < pages,
                            egui::Button::new("Later messages"),
                        )
                        .clicked()
                    {
                        self.conversation_page += 1;
                    }
                });
            }
            let begin = (self.conversation_page * PAGE).min(messages.len());
            let end = (begin + PAGE).min(messages.len());
            self.messages(ui, &messages[begin..end]);
        } else {
            theme::header(
                ui,
                Icon::People,
                "Communication paths",
                "RECORDED EXCHANGES",
            );
            let mut paths: BTreeMap<(String, String), usize> = BTreeMap::new();
            for m in &messages {
                *paths
                    .entry((s(m, "sender").into(), s(m, "recipient").into()))
                    .or_default() += 1;
            }
            if paths.is_empty() {
                ui.weak("Communication paths appear when messages are recorded.");
            } else {
                egui::CollapsingHeader::new("Connection map")
                    .default_open(true)
                    .show(ui, |ui| self.communication_map(ui, &paths));
            }
            for ((from, to), count) in paths {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(self.persona_label(&from)).clicked() {
                        self.conversation_persona = Some(from.clone());
                        self.conversation_route = None;
                    }
                    ui.label("→");
                    if ui.button(self.persona_label(&to)).clicked() {
                        self.conversation_persona = Some(to.clone());
                        self.conversation_route = None;
                    }
                    if ui
                        .small_button(format!(
                            "{count} {}",
                            if count == 1 { "message" } else { "messages" }
                        ))
                        .clicked()
                    {
                        self.conversation_mode = 0;
                        self.conversation_route = Some((from.clone(), to.clone()));
                        self.conversation_page = 0;
                    }
                });
            }
            ui.add_space(10.0);
            theme::header(ui, Icon::Task, "Responsibilities", "ACCEPTED CONTRIBUTIONS");
            for person in participants {
                ui.push_id(s(person, "persona"), |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(s(person, "label"));
                        theme::badge(
                            ui,
                            if person["responsibility_accepted"] == true {
                                "Accepted responsibility"
                            } else {
                                "No current acceptance"
                            },
                            if person["responsibility_accepted"] == true {
                                theme::GREEN
                            } else {
                                theme::palette(ui).muted
                            },
                        );
                    });
                    if !person["acceptance"].is_null() {
                        ui.collapsing("Scope and acceptance record", |ui| {
                            json_view(ui, &person["acceptance"])
                        });
                    }
                });
            }
            for (key, label) in [
                ("arrangement_proposals", "Proposed arrangements"),
                ("arrangements", "Accepted arrangements"),
                ("dependencies", "Dependencies"),
            ] {
                let value = &snapshot[key];
                if value.as_object().is_some_and(|v| !v.is_empty())
                    || value.as_array().is_some_and(|v| !v.is_empty())
                {
                    ui.collapsing(label, |ui| json_view(ui, value));
                }
            }
            ui.add_space(8.0);
            ui.weak("A message records communication; acceptance records responsibility. Neither establishes that work is complete.");
        }
    }

    fn communication_map(&mut self, ui: &mut Ui, paths: &BTreeMap<(String, String), usize>) {
        let ids: std::collections::BTreeSet<_> = paths
            .keys()
            .flat_map(|(from, to)| [from.clone(), to.clone()])
            .collect();
        // Dense populations remain inspectable as rows below the map. Selecting
        // a persona focuses the real incident paths, without hiding records.
        if ids.len() > 12 {
            ui.weak("Select a persona below to focus their connections. The complete directed list remains available.");
            return;
        }
        let colors = theme::palette(ui);
        let width = ui.available_width().max(220.0);
        let height = if width > 600.0 { 320.0 } else { 280.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 3, colors.field);
        let radius = egui::vec2((width / 2.0 - 80.0).max(35.0), height / 2.0 - 40.0);
        let nodes: BTreeMap<_, _> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let angle = std::f32::consts::TAU * i as f32 / ids.len() as f32
                    - std::f32::consts::FRAC_PI_2;
                let center =
                    rect.center() + egui::vec2(angle.cos() * radius.x, angle.sin() * radius.y);
                (
                    id.clone(),
                    egui::Rect::from_center_size(center, egui::vec2(112.0, 36.0)),
                )
            })
            .collect();
        let mut route = None;
        for ((from, to), count) in paths {
            let a = nodes[from].center();
            let b = nodes[to].center();
            let direction = (b - a).normalized();
            if from == to {
                continue;
            }
            let normal = egui::vec2(-direction.y, direction.x);
            let control = (a + b.to_vec2()) * 0.5 + normal * 40.0;
            // Meet the actual rectangular node boundary along the curve's
            // tangent so that drawing the node cannot cover the arrowhead.
            let boundary = |node: egui::Rect, toward: egui::Pos2| {
                let delta = toward - node.center();
                let extent = node.expand(7.0).size() * 0.5;
                let distance = (extent.x / delta.x.abs()).min(extent.y / delta.y.abs());
                node.center() + delta * distance
            };
            let start = boundary(nodes[from], control);
            let end = boundary(nodes[to], control);
            let middle = (start + end.to_vec2()) * 0.25 + control.to_vec2() * 0.5;
            let hit = ui.interact(
                egui::Rect::from_center_size(middle, egui::vec2(24.0, 18.0)),
                ui.id().with(("connection", from, to)),
                egui::Sense::click(),
            );
            let stroke = egui::Stroke::new(
                if hit.hovered() { 2.0_f32 } else { 1.0_f32 },
                if hit.hovered() {
                    theme::ACCENT
                } else {
                    colors.muted.gamma_multiply(0.55)
                },
            );
            painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke(
                [start, control, end],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            let tangent = (end - control).normalized();
            let side = egui::vec2(-tangent.y, tangent.x);
            painter.line_segment([end, end - tangent * 7.0 + side * 3.0], stroke);
            painter.line_segment([end, end - tangent * 7.0 - side * 3.0], stroke);
            painter.rect_filled(hit.rect, 3, colors.field);
            painter.text(
                middle,
                egui::Align2::CENTER_CENTER,
                count,
                egui::FontId::monospace(10.0),
                colors.text,
            );
            if hit.clicked() {
                route = Some((from.clone(), to.clone()));
            }
            hit.on_hover_text(format!(
                "{} → {} · {count} recorded messages. Click to read.",
                self.persona_label(from),
                self.persona_label(to)
            ));
        }
        for (id, node) in &nodes {
            let response = ui.interact(
                *node,
                ui.id().with(("connection-persona", id)),
                egui::Sense::click(),
            );
            let selected = self.conversation_persona.as_ref() == Some(id);
            painter.rect_filled(
                *node,
                4,
                if selected || response.hovered() {
                    colors.selection
                } else {
                    colors.panel
                },
            );
            painter.rect_stroke(
                *node,
                4,
                egui::Stroke::new(1.0_f32, theme::BLUE),
                egui::StrokeKind::Inside,
            );
            let label = self.persona_label(id);
            painter.text(
                node.center(),
                egui::Align2::CENTER_CENTER,
                short(&label, 15),
                egui::FontId::proportional(12.0),
                colors.text,
            );
            if response.clicked() {
                self.conversation_persona = Some(id.clone());
                self.conversation_route = None;
            }
            response.on_hover_text(format!("{label} · focus communication"));
        }
        if let Some(pair) = route {
            self.conversation_route = Some(pair);
            self.conversation_page = 0;
            self.conversation_mode = 0;
        }
        ui.weak("Select a person to focus; select a count to read that directed exchange.");
    }
}
