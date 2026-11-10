use super::*;
impl App {
    pub(super) fn header(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        egui::TopBottomPanel::top("studio-menu")
            .frame(
                egui::Frame::new()
                    .fill(p.field)
                    .inner_margin(egui::Margin::symmetric(14, 9)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    theme::glyph(ui, Icon::Workspaces, theme::ACCENT);
                    ui.label(
                        RichText::new("PERSONAS")
                            .size(15.)
                            .strong()
                            .extra_letter_spacing(2.),
                    );
                    ui.label(RichText::new("STUDIO").size(11.).color(p.muted));
                    ui.add_space(20.);
                    for (editor, label) in [
                        (Editor::Studio, "Workspace"),
                        (Editor::Conversations, "Conversations"),
                        (Editor::Artifacts, "Artifacts"),
                        (Editor::Character, "Character Lab"),
                    ] {
                        if ui
                            .add(
                                egui::Button::new(label)
                                    .selected(self.layout.editor == editor)
                                    .frame(self.layout.editor == editor),
                            )
                            .clicked()
                        {
                            self.layout.editor = editor;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::icon_button(ui, Icon::Settings, "Workspace settings").clicked() {
                            self.dialog = Some(Dialog::Settings);
                        }
                        if ui.button("+ Task").clicked() {
                            self.dialog = Some(Dialog::Task);
                        }
                    });
                });
            });
        egui::TopBottomPanel::top("studio-context")
            .frame(
                egui::Frame::new()
                    .fill(p.header)
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    theme::glyph(ui, Icon::Task, p.muted);
                    ui.add(
                        egui::Label::new(RichText::new(excerpt(self.task_title(), 65)).strong())
                            .truncate(),
                    );
                    let active = self.snapshot.active.len();
                    if active > 0 {
                        theme::badge(ui, &format!("{active} active"), TEAL);
                    }
                    let tools = self
                        .snapshot
                        .active_effects
                        .iter()
                        .filter(|e| e["state"] == "dispatched")
                        .count();
                    if tools > 0 {
                        let label = if tools == 1 {
                            "1 tool running".to_owned()
                        } else {
                            format!("{tools} tools running")
                        };
                        theme::badge(ui, &label, theme::ACCENT);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("Run controls", |ui| {
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Enable scheduling"))
                                .clicked()
                            {
                                self.command(
                                    ctx,
                                    Command::Control {
                                        scope: "global".into(),
                                        state: "running".into(),
                                        reason: "User enabled scheduling from desktop".into(),
                                    },
                                );
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    !self.running && !self.busy && active == 0,
                                    egui::Button::new("Continue eligible work"),
                                )
                                .clicked()
                            {
                                self.command(ctx, Command::Run { watch: false });
                                ui.close();
                            }
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Pause all work"))
                                .clicked()
                            {
                                self.command(
                                    ctx,
                                    Command::Control {
                                        scope: "global".into(),
                                        state: "paused".into(),
                                        reason: "User paused the desktop workspace".into(),
                                    },
                                );
                                ui.close();
                            }
                            ui.weak("Explicitly stopped participants stay stopped until resumed.");
                        });
                        if ui.button("Write message").clicked() {
                            self.dialog = Some(Dialog::Message);
                        }
                        if theme::icon_button(ui, Icon::People, "Toggle inspector").clicked() {
                            if ctx.content_rect().width() > 1100. {
                                self.layout.inspector = !self.layout.inspector;
                            } else {
                                self.inspector_window = !self.inspector_window;
                            }
                        }
                        if theme::icon_button(ui, Icon::Folder, "Toggle outliner").clicked() {
                            self.layout.outliner = !self.layout.outliner;
                        }
                    });
                });
                for control in self
                    .records("controls")
                    .iter()
                    .filter(|c| c["state"] != "running")
                {
                    ui.colored_label(
                        theme::ACCENT,
                        format!(
                            "{} · {} — {}",
                            s(control, "scope"),
                            s(control, "state"),
                            s(control, "reason")
                        ),
                    );
                }
                if let Some(error) = self.error.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(Color32::LIGHT_RED, error);
                        if ui.small_button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                }
                if !self.notice.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(TEAL, &self.notice);
                        if ui.small_button("×").clicked() {
                            self.notice.clear();
                        }
                    });
                }
            });
    }
    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        egui::TopBottomPanel::bottom("studio-status")
            .frame(
                egui::Frame::new()
                    .fill(p.field)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(8., 8.), egui::Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        3.,
                        if self.snapshot.active.is_empty()
                            && !self
                                .snapshot
                                .active_effects
                                .iter()
                                .any(|e| e["state"] == "dispatched")
                        {
                            p.muted
                        } else {
                            TEAL
                        },
                    );
                    ui.small(if self.closing {
                        "Waiting for owned work to settle"
                    } else if self.snapshot.status.is_null() {
                        "Loading workspace…"
                    } else if self.snapshot.active.is_empty()
                        && !self
                            .snapshot
                            .active_effects
                            .iter()
                            .any(|e| e["state"] == "dispatched")
                    {
                        "Workspace connected"
                    } else {
                        "Live workspace"
                    });
                    ui.separator();
                    ui.small(format!(
                        "{} accepted decisions",
                        number(
                            self.snapshot.status["invocations"]["accepted"]
                                .as_u64()
                                .unwrap_or(0)
                        )
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .selectable_label(self.layout.activity, "Activity")
                            .clicked()
                        {
                            self.layout.activity = !self.layout.activity;
                        }
                        ui.small(format!("{} artifacts", self.snapshot.assets.len()));
                        ui.separator();
                        ui.small(format!("{} personas", self.records("personas").len()));
                    });
                });
            });
    }
    pub(super) fn outliner(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        let panel = egui::SidePanel::left("studio-outliner")
            .resizable(true)
            .default_width(self.layout.left_width)
            .width_range(170.0..=360.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(12))
            .show(ctx, |ui| {
                eyebrow(ui, "Outliner");
                ui.add_space(12.);
                egui::ScrollArea::vertical()
                    .id_salt("outliner-scroll")
                    .show(ui, |ui| {
                        section(ui, "Tasks", &self.records("work").len().to_string());
                        for task in self.records("work").to_vec() {
                            let b = &task["body"];
                            let id = s(b, "id");
                            if ui
                                .add_sized(
                                    [ui.available_width(), 32.],
                                    egui::Button::new(excerpt(s(b, "original_need"), 32))
                                        .selected(id == self.work)
                                        .truncate(),
                                )
                                .on_hover_text(s(b, "original_need"))
                                .clicked()
                            {
                                self.work = id.into();
                                self.conversation_page = 0;
                            }
                        }
                        if self.records("work").is_empty() {
                            ui.weak("Create a task to begin.");
                        }
                        ui.add_space(20.);
                        section(ui, "Personas", &self.records("personas").len().to_string());
                        for record in self.records("personas").to_vec() {
                            let b = &record["body"];
                            let id = s(b, "id");
                            let label = s(b, "label");
                            let color = self.color(id);
                            let state = self.persona_state(id);
                            let response = egui::Frame::new()
                                .fill(if self.persona == id {
                                    p.selection.gamma_multiply(0.65)
                                } else {
                                    p.panel
                                })
                                .corner_radius(5)
                                .inner_margin(6)
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        if avatar(ui, label, color, 28.).clicked() {
                                            self.persona = id.into();
                                        }
                                        ui.vertical(|ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(label).strong(),
                                                    )
                                                    .frame(false),
                                                )
                                                .clicked()
                                            {
                                                self.persona = id.into();
                                            }
                                            ui.label(RichText::new(state).size(11.).color(p.muted));
                                        });
                                    });
                                })
                                .response;
                            let response = response.interact(egui::Sense::click());
                            if response.clicked() {
                                self.persona = id.into();
                            }
                            if response.double_clicked() {
                                self.persona = id.into();
                                self.layout.editor = Editor::Character;
                            }
                            ui.add_space(4.);
                        }
                        ui.add_space(20.);
                        section(ui, "Library", "");
                        if ui
                            .add_sized(
                                [ui.available_width(), 30.],
                                egui::Button::new(format!(
                                    "Retained artifacts   {}",
                                    self.snapshot.assets.len()
                                ))
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.layout.editor = Editor::Artifacts;
                            self.artifact_mode = 0;
                            self.artifact_scope = 0;
                        }
                        if ui
                            .add_sized(
                                [ui.available_width(), 30.],
                                egui::Button::new(format!(
                                    "Deliveries   {}",
                                    self.records("deliveries")
                                        .iter()
                                        .filter(|d| d["body"]["work"] == self.work)
                                        .count()
                                ))
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.layout.editor = Editor::Artifacts;
                            self.artifact_mode = 0;
                            self.artifact_scope = 1;
                        }
                        if ui
                            .add_sized(
                                [ui.available_width(), 30.],
                                egui::Button::new("Workspace files").frame(false),
                            )
                            .clicked()
                        {
                            self.layout.editor = Editor::Artifacts;
                            self.artifact_mode = 1;
                            self.files.workspace_mode();
                        }
                        ui.add_space(20.);
                        ui.separator();
                        ui.add_space(7.);
                        eyebrow(ui, "Saved workspace");
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    self.root.file_name().unwrap_or_default().to_string_lossy(),
                                )
                                .small()
                                .weak(),
                            )
                            .truncate(),
                        )
                        .on_hover_text(self.root.display().to_string());
                        if ui.small_button("Copy location").clicked() {
                            ctx.copy_text(self.root.display().to_string());
                        }
                    });
            });
        self.layout.left_width = panel.response.rect.width();
    }
    pub(super) fn persona_state(&self, id: &str) -> String {
        if let Some(active) = self.snapshot.active.iter().find(|a| a["actor"] == id) {
            return if active["work"] == self.work {
                "Model call in progress"
            } else {
                "Working on another task"
            }
            .into();
        }
        if let Some(effect) = self
            .snapshot
            .active_effects
            .iter()
            .find(|e| e["actor"] == id)
        {
            return match s(effect, "state") {
                "uncertain" => "Tool outcome uncertain",
                "pending" => "Tool queued",
                _ if effect["work"] != self.work => "Running a tool on another task",
                _ => "Tool run in progress",
            }
            .into();
        }
        let p = self.participation(id);
        if p.is_null() {
            return "No task participation".into();
        }
        match s(&p["continuation"], "kind") {
            "ready" => "Ready to continue",
            "wait" | "waiting" => "Waiting",
            "stop" | "stopped" => "Stopped on this task",
            _ => "Participating",
        }
        .into()
    }
    pub(super) fn studio(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .id_salt("studio-overview")
            .show(ui, |ui| {
                eyebrow(ui, "Shared workspace");
                ui.add_space(5.);
                ui.heading(self.task_title());
                ui.add_space(12.);
                let peers = self
                    .events
                    .iter()
                    .filter(|e| {
                        e["work"] == self.work && e["kind"] == "message" && e["actor"] != "user"
                    })
                    .count();
                ui.horizontal_wrapped(|ui| {
                    for (value, label, color) in [
                        (self.snapshot.active.len(), "active calls", TEAL),
                        (
                            self.snapshot.assets.len(),
                            "retained artifacts",
                            theme::ACCENT,
                        ),
                        (peers, "peer messages", PURPLE),
                        (
                            self.records("deliveries")
                                .iter()
                                .filter(|d| d["body"]["work"] == self.work)
                                .count(),
                            "deliveries",
                            Color32::from_rgb(121, 166, 230),
                        ),
                    ] {
                        egui::Frame::new()
                            .fill(theme::palette(ui).header)
                            .corner_radius(5)
                            .inner_margin(egui::Margin::symmetric(12, 7))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(value.to_string()).size(22.).color(color),
                                    );
                                    ui.small(label);
                                });
                            });
                    }
                });
                ui.add_space(18.);
                section(ui, "Collaboration", "Select a persona to inspect");
                self.collaboration_map(ui);
                ui.add_space(16.);
                let messages: Vec<Value> = self
                    .events
                    .iter()
                    .rev()
                    .filter(|e| {
                        e["work"] == self.work && matches!(s(e, "kind"), "message" | "reply")
                    })
                    .take(3)
                    .cloned()
                    .collect();
                let assets: Vec<Value> = self
                    .snapshot
                    .assets
                    .iter()
                    .filter(|a| a["body"]["retired"] != true && a["current"] != false)
                    .take(6)
                    .cloned()
                    .collect();
                let column_gap = ui.spacing().item_spacing.x;
                ui.spacing_mut().item_spacing.x = 18.;
                ui.columns(2, |cols| {
                    section(&mut cols[0], "Recent conversations", "Live exchanges");
                    for message in &messages {
                        card(&cols[0]).show(&mut cols[0], |ui| {
                            self.message_heading(ui, message);
                            ui.add_space(6.);
                            ui.add(
                                egui::Label::new(excerpt(s(&message["body"], "text"), 230))
                                    .wrap()
                                    .halign(egui::Align::Min),
                            );
                            if ui.small_button("Open conversation →").clicked() {
                                self.layout.editor = Editor::Conversations;
                                self.conversation_persona = s(message, "actor").into();
                                self.conversation_page = 0;
                            }
                        });
                        cols[0].add_space(8.);
                    }
                    if messages.is_empty() {
                        cols[0]
                            .weak("Messages will appear as personas exchange ideas and results.");
                    }
                    section(&mut cols[1], "Recent artifacts", "Exact retained files");
                    for asset in &assets {
                        self.artifact_row(&mut cols[1], asset, false);
                        cols[1].add_space(5.);
                    }
                    if cols[1].button("Browse all artifacts →").clicked() {
                        self.layout.editor = Editor::Artifacts;
                        self.artifact_mode = 0;
                    }
                });
                ui.spacing_mut().item_spacing.x = column_gap;
                ui.add_space(16.);
                section(ui, "Intended outcomes", "Authored by the personas");
                for outcome in self
                    .records("outcomes")
                    .to_vec()
                    .into_iter()
                    .filter(|o| o["body"]["work"] == self.work)
                {
                    let b = &outcome["body"];
                    ui.collapsing(
                        format!(
                            "{} · {}",
                            self.label(s(b, "owner")),
                            excerpt(s(b, "description"), 70)
                        ),
                        |ui| {
                            ui.label(s(b, "description"));
                            for item in b["remaining"].as_array().into_iter().flatten() {
                                ui.label(format!("• {}", item.as_str().unwrap_or("")));
                            }
                        },
                    );
                }
            });
    }
    fn collaboration_map(&mut self, ui: &mut Ui) {
        let people: Vec<Value> = self.records("personas").to_vec();
        let n = people.len();
        if n == 0 {
            ui.weak("Personas appear here after the workspace loads.");
            return;
        }
        let height = if n <= 6 {
            230.
        } else {
            (n as f32 * 35.).max(230.)
        };
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        let p = ui.painter_at(rect);
        let colors = theme::palette(ui);
        p.rect_filled(rect, 6, colors.field);
        p.rect_stroke(
            rect,
            6,
            Stroke::new(1., colors.border),
            egui::StrokeKind::Inside,
        );
        for x in (0..rect.width() as i32).step_by(24) {
            for y in (0..height as i32).step_by(24) {
                p.circle_filled(
                    rect.min + egui::vec2(x as f32, y as f32),
                    0.7,
                    colors.muted.gamma_multiply(0.13),
                );
            }
        }
        let radius_x = (rect.width() / 2. - 80.).max(10.);
        let center = rect.center() - egui::vec2(0., 3.);
        let positions: Vec<_> = (0..n)
            .map(|i| {
                let angle =
                    std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                center + egui::vec2(angle.cos() * radius_x, angle.sin() * (height / 2. - 42.))
            })
            .collect();
        let mut links = std::collections::BTreeMap::<(usize, usize), usize>::new();
        for e in self
            .events
            .iter()
            .filter(|e| e["work"] == self.work && e["kind"] == "message")
        {
            if let (Some(a), Some(b)) = (
                people.iter().position(|v| v["body"]["id"] == e["actor"]),
                people.iter().position(|v| v["body"]["id"] == e["target"]),
            ) {
                if a != b {
                    *links.entry((a, b)).or_default() += 1;
                }
            }
        }
        for ((a, b), count) in links {
            let direction = (positions[b] - positions[a]).normalized();
            let start = positions[a] + direction * 38.;
            let end = positions[b] - direction * 43.;
            let selected =
                people[a]["body"]["id"] == self.persona || people[b]["body"]["id"] == self.persona;
            let color = if selected {
                TEAL.gamma_multiply(0.42)
            } else {
                colors.muted.gamma_multiply(0.17)
            };
            p.line_segment(
                [start, end],
                Stroke::new(0.8 + (count as f32).ln_1p() / 4., color),
            );
            let normal = egui::vec2(-direction.y, direction.x);
            p.add(egui::Shape::convex_polygon(
                vec![
                    end,
                    end - direction * 7. + normal * 3.,
                    end - direction * 7. - normal * 3.,
                ],
                color,
                Stroke::NONE,
            ));
        }
        for (i, person) in people.iter().enumerate() {
            let b = &person["body"];
            let id = s(b, "id");
            let label = s(b, "label");
            let color = self.color(id);
            let box_rect = egui::Rect::from_center_size(positions[i], egui::vec2(120., 50.));
            let response = ui.interact(
                box_rect,
                ui.id().with(("persona-node", id)),
                egui::Sense::click(),
            );
            p.rect_filled(
                box_rect,
                6,
                if response.hovered() {
                    colors.hover
                } else {
                    colors.panel
                },
            );
            p.rect_stroke(
                box_rect,
                6,
                Stroke::new(
                    if self.persona == id { 2. } else { 1. },
                    if self.persona == id {
                        color
                    } else {
                        colors.border
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let dot = box_rect.left_center() + egui::vec2(17., 0.);
            p.circle_filled(dot, 5., color);
            p.text(
                box_rect.min + egui::vec2(30., 10.),
                egui::Align2::LEFT_TOP,
                label,
                FontId::proportional(15.),
                colors.text,
            );
            p.text(
                box_rect.min + egui::vec2(30., 30.),
                egui::Align2::LEFT_TOP,
                if self.snapshot.active.iter().any(|a| a["actor"] == id) {
                    "INFERENCE"
                } else if self
                    .snapshot
                    .active_effects
                    .iter()
                    .any(|a| a["actor"] == id)
                {
                    "TOOL"
                } else {
                    "PERSONA"
                },
                FontId::proportional(9.),
                colors.muted,
            );
            if response.clicked() {
                self.persona = id.into();
            }
            if response.double_clicked() {
                self.layout.editor = Editor::Character;
            }
            response.on_hover_text(format!(
                "{}\n{}\nArrows show actual saved peer messages.",
                label,
                self.persona_state(id)
            ));
        }
    }
    fn message_heading(&mut self, ui: &mut Ui, event: &Value) {
        let actor = s(event, "actor");
        let target = s(event, "target");
        let from = self.label(actor).to_owned();
        let to = if target == "*" {
            "Everyone"
        } else if target == "user" {
            "You"
        } else {
            self.label(target)
        }
        .to_owned();
        ui.horizontal_wrapped(|ui| {
            if avatar(ui, &from, self.color(actor), 24.).clicked() {
                self.persona = actor.into();
            }
            ui.label(RichText::new(from).strong().color(self.color(actor)));
            ui.weak("→");
            ui.strong(to);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(time_label(event["created"].as_i64().unwrap_or(0)))
                        .size(11.)
                        .weak(),
                );
            });
        });
    }
    pub(super) fn conversations(&mut self, ui: &mut Ui) {
        eyebrow(ui, "Conversation editor");
        ui.horizontal(|ui| {
            ui.heading("Ideas, questions & handoffs");
        });
        ui.add_space(10.);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.conversation_filter)
                        .hint_text("Search conversations…")
                        .desired_width(230.),
                )
                .changed()
            {
                self.conversation_page = 0;
            }
            let selected = if self.conversation_persona.is_empty() {
                "All personas"
            } else {
                self.label(&self.conversation_persona)
            }
            .to_owned();
            egui::ComboBox::from_id_salt("conversation-person")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(
                            &mut self.conversation_persona,
                            String::new(),
                            "All personas",
                        )
                        .changed()
                    {
                        self.conversation_page = 0;
                    }
                    for p in self.records("personas").to_vec() {
                        if ui
                            .selectable_value(
                                &mut self.conversation_persona,
                                s(&p["body"], "id").into(),
                                s(&p["body"], "label"),
                            )
                            .changed()
                        {
                            self.conversation_page = 0;
                        }
                    }
                });
            for (i, label) in [
                "All exchanges",
                "Between personas",
                "With you",
                "Deliveries",
            ]
            .iter()
            .enumerate()
            {
                if ui
                    .selectable_value(&mut self.conversation_channel, i, *label)
                    .changed()
                {
                    self.conversation_page = 0;
                }
            }
        });
        ui.add_space(10.);
        let query = self.conversation_filter.to_lowercase();
        let events: Vec<Value> = self
            .events
            .iter()
            .rev()
            .filter(|e| {
                let kind = s(e, "kind");
                let actor = s(e, "actor");
                let target = s(e, "target");
                e["work"] == self.work
                    && matches!(
                        kind,
                        "message" | "reply" | "delivery" | "initialization_note"
                    )
                    && !(kind == "delivery" && target == "*")
                    && (self.conversation_persona.is_empty()
                        || actor == self.conversation_persona
                        || target == self.conversation_persona)
                    && match self.conversation_channel {
                        1 => kind == "message" && actor != "user" && target != "user",
                        2 => actor == "user" || target == "user",
                        3 => kind == "delivery",
                        _ => true,
                    }
                    && (query.is_empty()
                        || e["body"].to_string().to_lowercase().contains(&query)
                        || self.label(actor).to_lowercase().contains(&query)
                        || self.label(target).to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        let pages = events.len().div_ceil(40).max(1);
        self.conversation_page = self.conversation_page.min(pages - 1);
        ui.horizontal(|ui| {
            ui.small(format!("{} exchanges · newest first", events.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.conversation_page + 1 < pages,
                        egui::Button::new("Older →"),
                    )
                    .clicked()
                {
                    self.conversation_page += 1;
                }
                ui.small(format!("{} / {pages}", self.conversation_page + 1));
                if ui
                    .add_enabled(self.conversation_page > 0, egui::Button::new("← Newer"))
                    .clicked()
                {
                    self.conversation_page -= 1;
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt((
                "peer-conversations",
                self.work.clone(),
                self.conversation_page,
                self.conversation_persona.clone(),
                self.conversation_channel,
            ))
            .show(ui, |ui| {
                if events.is_empty() {
                    ui.add_space(40.);
                    ui.heading("No exchanges match this view");
                    ui.weak("Choose another persona, channel or search term.");
                }
                for event in events.iter().skip(self.conversation_page * 40).take(40) {
                    ui.push_id(event["event"].as_i64().unwrap_or(0), |ui| {
                        card(ui).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.message_heading(ui, event);
                            ui.add_space(9.);
                            if let Some(text) = event["body"]["text"].as_str() {
                                document::markdown(ui, text);
                            } else if event["kind"] == "delivery" {
                                let body = &event["body"]["record"];
                                ui.heading(s(body, "title"));
                                ui.label(s(body, "supported_use"));
                                theme::badge(ui, s(body, "scope"), theme::ACCENT);
                                for gap in body["remaining"].as_array().into_iter().flatten() {
                                    ui.label(format!("• {}", gap.as_str().unwrap_or("")));
                                }
                            }
                            let mut refs: Vec<String> = event["body"]["references"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect();
                            refs.extend(
                                event["body"]["record"]["artifacts"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|v| v.as_str().map(String::from)),
                            );
                            if !refs.is_empty() {
                                ui.add_space(7.);
                                ui.horizontal_wrapped(|ui| {
                                    for reference in refs {
                                        let name = self
                                            .snapshot
                                            .assets
                                            .iter()
                                            .find(|a| a["reference"] == reference)
                                            .map(|a| {
                                                s(&a["body"], "path")
                                                    .rsplit('/')
                                                    .next()
                                                    .unwrap_or("Resource")
                                                    .to_owned()
                                            })
                                            .unwrap_or_else(|| excerpt(&reference, 35));
                                        if ui.small_button(name).on_hover_text(&reference).clicked()
                                        {
                                            self.open_resource(ui.ctx(), reference);
                                        }
                                    }
                                });
                            }
                        });
                    });
                    ui.add_space(12.);
                }
            });
    }
    fn artifact_row(&mut self, ui: &mut Ui, asset: &Value, compact: bool) {
        let b = &asset["body"];
        let path = s(b, "path");
        let filename = path.rsplit('/').next().unwrap_or(path);
        let extension = std::path::Path::new(path)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_uppercase();
        let selected = asset["reference"] == self.selected_reference;
        let response = egui::Frame::new()
            .fill(if selected {
                theme::palette(ui).selection.gamma_multiply(0.7)
            } else {
                theme::palette(ui).header
            })
            .inner_margin(if compact { 7 } else { 10 })
            .corner_radius(4)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::glyph(ui, Icon::Files, theme::ACCENT);
                    ui.add(egui::Label::new(RichText::new(filename).strong()).truncate());
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if extension.is_empty() {
                            "FILE"
                        } else {
                            &extension
                        })
                        .size(10.)
                        .color(theme::ACCENT),
                    );
                    ui.small(self.label(s(b, "owner")));
                    if let Some(version) = asset["revision"].as_u64() {
                        ui.label(RichText::new(format!("v{version}")).size(10.).weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(size(asset["size"].as_u64().unwrap_or(0)));
                    });
                });
            })
            .response
            .interact(egui::Sense::click())
            .on_hover_text(format!("{}\n{}", path, s(asset, "reference")));
        if response.clicked() {
            self.open_resource(ui.ctx(), s(asset, "reference").into());
        }
    }
    pub(super) fn artifacts(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Artifacts");
            ui.add_space(10.);
            ui.selectable_value(&mut self.artifact_mode, 0, "Retained library");
            if ui
                .selectable_value(&mut self.artifact_mode, 1, "Workspace files")
                .changed()
            {
                self.files.workspace_mode();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export deliveries…").clicked() {
                    self.export_deliveries(ui.ctx());
                }
                ui.menu_button("Open reference", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.reference_input)
                            .hint_text("asset:…@revision or object:…")
                            .desired_width(330.),
                    );
                    if ui.button("Open exact resource").clicked() {
                        let reference = self.reference_input.trim().to_owned();
                        self.open_resource(ui.ctx(), reference);
                        ui.close();
                    }
                });
            });
        });
        ui.add_space(8.);
        if self.artifact_mode == 1 {
            let directory = self.snapshot.directory.clone();
            if directory.as_os_str().is_empty() {
                ui.weak("The workspace location is unavailable.");
            } else {
                self.files
                    .ui(ui, &directory, "editable-workspace-files", None);
            }
            return;
        }
        ui.small(
            "Browse exact retained versions. A retained artifact may still be intermediate work.",
        );
        ui.add_space(8.);
        if self.artifact_scope == 1 {
            let deliveries: Vec<_> = self
                .records("deliveries")
                .iter()
                .filter(|d| d["body"]["work"] == self.work)
                .cloned()
                .collect();
            ui.collapsing(
                format!(
                    "{} delivery records and their remaining work",
                    deliveries.len()
                ),
                |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(230.)
                        .show(ui, |ui| {
                            for d in deliveries {
                                let b = &d["body"];
                                ui.strong(s(b, "title"));
                                ui.label(format!(
                                    "{} · {} · {}",
                                    self.label(s(b, "owner")),
                                    s(b, "scope"),
                                    if b["user_accepted"] == true {
                                        "Accepted by you"
                                    } else {
                                        "Not accepted"
                                    }
                                ));
                                ui.label(s(b, "supported_use"));
                                for gap in b["remaining"].as_array().into_iter().flatten() {
                                    ui.small(format!("• {}", gap.as_str().unwrap_or("")));
                                }
                                ui.separator();
                            }
                        });
                },
            );
        }
        egui::SidePanel::left("retained-artifact-list").resizable(true).default_width(255.).width_range(180.0..=420.0).frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0,4))).show_inside(ui,|ui|{
            ui.add(egui::TextEdit::singleline(&mut self.asset_filter).hint_text("Search name, path or cues…").desired_width(f32::INFINITY));
            egui::ComboBox::from_id_salt("artifact-scope").selected_text(["Current artifacts","Delivered for this task","Retired artifacts"][self.artifact_scope]).width(ui.available_width()).show_ui(ui,|ui|{for(i,label)in["Current artifacts","Delivered for this task","Retired artifacts"].iter().enumerate(){ui.selectable_value(&mut self.artifact_scope,i,*label);}});
            let owner=if self.asset_owner.is_empty(){"All personas"}else{self.label(&self.asset_owner)}.to_owned();
            egui::ComboBox::from_id_salt("artifact-owner").selected_text(owner).width(ui.available_width()).show_ui(ui,|ui|{ui.selectable_value(&mut self.asset_owner,String::new(),"All personas");for p in self.records("personas").to_vec(){ui.selectable_value(&mut self.asset_owner,s(&p["body"],"id").into(),s(&p["body"],"label"));}});
            let query=self.asset_filter.to_lowercase();let delivered=self.delivered_refs();
            let mut assets:Vec<Value>=self.snapshot.assets.iter().filter(|a|{
                let b=&a["body"];(self.asset_owner.is_empty()||b["owner"]==self.asset_owner)&&(query.is_empty()||s(b,"path").to_lowercase().contains(&query)||b["cues"].to_string().to_lowercase().contains(&query))&&match self.artifact_scope{1=>delivered.contains(s(a,"reference")),2=>b["retired"]==true,_=>b["retired"]!=true&&a["current"]!=false}
            }).cloned().collect();
            // An exact older delivered revision remains accessible even when a
            // newer head exists. Its receipt supplies the exact content on click.
            if self.artifact_scope==1{for reference in delivered{if !assets.iter().any(|a|a["reference"]==reference)&&query.is_empty()&&self.asset_owner.is_empty(){assets.push(serde_json::json!({"reference":reference,"body":{"path":reference,"owner":"","cues":[]},"size":0}));}}}
            ui.small(format!("{} files",assets.len()));ui.add_space(5.);
            egui::ScrollArea::vertical().id_salt("retained-artifacts").show(ui,|ui|{for asset in &assets{self.artifact_row(ui,asset,true);ui.add_space(4.);}if assets.is_empty(){ui.weak("No artifacts match these filters.");}});
        });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::palette(ui).field)
                    .inner_margin(12),
            )
            .show_inside(ui, |ui| {
                if self.preview_pending {
                    ui.label("Loading exact retained bytes…");
                } else if self.selected_reference.is_empty() {
                    ui.add_space(50.);
                    theme::glyph(ui, Icon::Files, theme::ACCENT);
                    ui.heading("Select an artifact");
                    ui.label("Preview drawings, images, documents, data, 3D models and packages.");
                    ui.weak("The inspector shows its owner, exact version and qualifications.");
                } else {
                    ui.horizontal(|ui| {
                        if ui.button("Save file…").clicked() {
                            self.save_artifact(ui.ctx());
                        }
                        if ui.small_button("Copy reference").clicked() {
                            ui.ctx().copy_text(self.selected_reference.clone());
                        }
                    });
                    ui.separator();
                    self.viewer.preview_ui(ui);
                }
            });
    }
    pub(super) fn activity_dock(&mut self, ctx: &egui::Context) {
        let panel = egui::TopBottomPanel::bottom("studio-activity-dock")
            .resizable(true)
            .default_height(self.layout.bottom_height)
            .height_range(100.0..=420.0)
            .frame(theme::panel(ctx))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Activity");
                    for (index, label) in ["All", "Tool results", "Problems"].into_iter().enumerate() {
                        ui.selectable_value(&mut self.activity_filter, index, label);
                    }
                    ui.weak("Process outcomes are separate from artifact quality.");
                });
                let active_effects: Vec<Value> = self.snapshot.active_effects.iter().filter(|e| e["work"] == self.work).cloned().collect();
                for effect in &active_effects {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(theme::ACCENT, self.label(s(effect, "actor")));
                        ui.label(s(effect, "tool"));
                        ui.strong(match s(effect, "state") {
                            "pending" => "Queued",
                            "dispatched" => "Running",
                            "uncertain" => "Outcome uncertain",
                            state => state,
                        });
                        if ui.small_button("Inspect request").clicked() {
                            self.open_resource(ctx, s(effect, "operation").into());
                        }
                    });
                }
                let rows: Vec<Value> = self
                    .events
                    .iter()
                    .rev()
                    .filter(|e| e["work"] == self.work && match self.activity_filter {
                        1 => e["kind"] == "tool_result",
                        2 => e["tool_outcome"]["problem"] == true || matches!(s(e, "kind"), "provider_failure" | "decision_rejected" | "uncertain_effect"),
                        _ => true,
                    })
                    .cloned()
                    .collect();
                egui::ScrollArea::vertical()
                    .id_salt(("activity-events", self.work.as_str(), self.activity_filter))
                    .auto_shrink([false, false])
                    .show_rows(ui, 28., rows.len(), |ui, range| {
                        for i in range {
                            let e = &rows[i];
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(time_label(e["created"].as_i64().unwrap_or(0)))
                                        .monospace()
                                        .size(11.)
                                        .weak(),
                                );
                                ui.label(
                                    RichText::new(self.label(s(e, "actor")))
                                        .size(12.)
                                        .color(self.color(s(e, "actor"))),
                                );
                                if e["kind"] == "tool_result" {
                                    ui.label(RichText::new(s(&e["body"], "tool")).size(12.));
                                    let result = &e["tool_outcome"];
                                    ui.label(RichText::new(s(result, "label")).size(12.).color(if result["problem"] == true { Color32::LIGHT_RED } else { TEAL }));
                                    if let Some(ms) = result["elapsed_milliseconds"].as_u64() {
                                        ui.weak(format!("{:.1}s", ms as f64 / 1000.));
                                    }
                                    if ui.small_button("Inspect result").on_hover_text("Open the exact retained result, including its input and observations.").clicked() {
                                        self.open_resource(ctx, s(result, "result").into());
                                    }
                                    ui.add(egui::Label::new(RichText::new(s(&e["body"], "operation")).size(11.).weak()).truncate());
                                    return;
                                }
                                ui.label(RichText::new(title(s(e, "kind"))).size(12.));
                                let detail = ["text", "reason", "error"].into_iter().map(|key| s(&e["body"], key)).find(|text| !text.is_empty()).unwrap_or("");
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(excerpt(detail, 180))
                                            .size(12.)
                                            .weak(),
                                    )
                                    .truncate(),
                                );
                                if ui.small_button("Inspect event").clicked() {
                                    self.open_resource(ctx, format!("object:{}", s(e, "digest")));
                                }
                            });
                        }
                    });
            });
        self.layout.bottom_height = panel.response.rect.height();
    }
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.dialog.as_ref().map(|d| match d {
            Dialog::Task => 0,
            Dialog::Message => 1,
            Dialog::Settings => 2,
        }) else {
            return;
        };
        let mut open = true;
        egui::Window::new(["New task","Message the workspace","Workspace settings"][kind]).id(egui::Id::new("studio-dialog")).open(&mut open).default_width(540.).resizable(true).show(ctx,|ui|{match kind{
            0=>{ui.label("What would you like to work on?");ui.add(egui::TextEdit::multiline(&mut self.need).desired_rows(5).desired_width(f32::INFINITY));ui.add_space(10.);ui.strong("Invite personas");for p in self.records("personas").to_vec(){let b=&p["body"];let id=s(b,"id");let mut selected=self.invited.contains(id);if ui.checkbox(&mut selected,s(b,"label")).changed(){if selected{self.invited.insert(id.into());}else{self.invited.remove(id);}}}if ui.add_enabled(!self.busy&&!self.need.trim().is_empty()&&!self.invited.is_empty(),theme::primary("Create task")).clicked(){let need=std::mem::take(&mut self.need);self.command(ctx,Command::Request{need,invite:self.invited.iter().cloned().collect()});self.dialog=None;}},
            1=>{ui.strong(self.task_title());ui.add_space(8.);egui::ComboBox::from_id_salt("message-recipient").selected_text(self.label(&self.persona)).show_ui(ui,|ui|{for p in self.records("personas").to_vec(){ui.selectable_value(&mut self.persona,s(&p["body"],"id").into(),s(&p["body"],"label"));}});ui.add(egui::TextEdit::multiline(&mut self.message).hint_text("Feedback, clarification or a reason to resume…").desired_rows(6).desired_width(f32::INFINITY));let enabled=!self.busy&&!self.message.trim().is_empty()&&!self.work.is_empty()&&!self.persona.is_empty();ui.horizontal_wrapped(|ui|{if ui.add_enabled(enabled,theme::primary("Send message")).clicked(){let text=std::mem::take(&mut self.message);self.command(ctx,Command::Message{work:self.work.clone(),persona:self.persona.clone(),text});self.dialog=None;}if ui.add_enabled(enabled,egui::Button::new("Resume participant")).clicked(){let reason=std::mem::take(&mut self.message);self.command(ctx,Command::Resume{work:self.work.clone(),persona:self.persona.clone(),reason});self.dialog=None;}if ui.add_enabled(enabled,egui::Button::new("Amend task")).clicked(){let text=std::mem::take(&mut self.message);self.command(ctx,Command::Amend{work:self.work.clone(),text});self.dialog=None;}});},
            _=>{ui.strong("Appearance");let changed=ui.checkbox(&mut self.layout.dark,"Dark workspace").changed()|ui.add(egui::Slider::new(&mut self.layout.scale,theme::MIN_ZOOM..=theme::MAX_ZOOM).text("Interface scale").step_by(0.05)).changed();if changed{theme::apply(ctx,self.layout.dark,self.layout.scale);self.save_layout();}ui.separator();ui.strong("Panels");ui.checkbox(&mut self.layout.outliner,"Outliner");ui.checkbox(&mut self.layout.inspector,"Inspector");ui.checkbox(&mut self.layout.activity,"Activity dock");ui.small("Drag panel boundaries to resize. Double-click a persona to open Character Lab.");ui.horizontal(|ui|{if ui.button("Reset layout").clicked(){self.layout=Layout::default();theme::apply(ctx,self.layout.dark,self.layout.scale);self.save_layout();}if ui.button("Save layout").clicked(){self.save_layout();self.notice="Workspace layout saved".into();}});ui.separator();ui.strong("Workspace location");ui.label(self.root.display().to_string());if ui.button("Copy location").clicked(){ctx.copy_text(self.root.display().to_string());}}
        }});
        if !open {
            self.dialog = None;
            self.save_layout();
        }
    }
}
