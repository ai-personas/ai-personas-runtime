//! Application chrome and the resizable task, inspector, and history editors.
use super::*;

impl App {
    pub(super) fn top(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(ctx.style().visuals.dark_mode);
        let can_switch = !self.busy && self.view.running.is_none() && !self.view.login;
        egui::TopBottomPanel::top("application-menu")
            .frame(
                egui::Frame::new()
                    .fill(p.field)
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    theme::glyph(ui, Icon::People, theme::ACCENT);
                    ui.label(RichText::new("PERSONAS").size(12.0).strong());
                    ui.add_space(10.0);
                    ui.menu_button("Workspaces", |ui| {
                        if ui
                            .add_enabled(can_switch, egui::Button::new("Browse workspaces"))
                            .clicked()
                        {
                            self.discover();
                            ui.close();
                        }
                        if ui
                            .add_enabled(can_switch, egui::Button::new("New workspace"))
                            .clicked()
                        {
                            self.new_workspace();
                            ui.close();
                        }
                        if ui
                            .add_enabled(can_switch, egui::Button::new("Open another folder…"))
                            .clicked()
                        {
                            self.dialog = Some(Dialog::OpenWorkspace);
                            ui.close();
                        }
                    });
                    ui.add_enabled_ui(self.view.ready && !self.choose_progress, |ui| {
                        ui.menu_button("Tasks", |ui| {
                            if ui
                                .add_enabled(can_switch && !self.view.read_only, egui::Button::new("New task…"))
                                .clicked()
                            {
                                self.text.clear();
                                self.dialog = Some(Dialog::Task);
                                ui.close();
                            }
                            for (tab, label) in [
                                (5, "Workspace overview"),
                                (0, "Conversation"),
                                (2, "Shared questions"),
                                (3, "Persona activity"),
                                (4, "Session history"),
                            ] {
                                if ui.button(label).clicked() {
                                    self.tab = tab;
                                    ui.close();
                                }
                            }
                            ui.separator();
                            ui.checkbox(&mut self.show_session_strip, "Show session history panel");
                        });
                        ui.menu_button("Personas", |ui| {
                            if ui.button("Properties & execution…").clicked() {
                                self.show_persona_properties = true;
                                ui.close();
                            }
                            ui.separator();
                            let participants: Vec<_> = self.view.inspection["settings"]
                                ["persona_labels"]
                                .as_object()
                                .into_iter()
                                .flatten()
                                .map(|(id, label)| json!({"persona":id,"label":label}))
                                .collect();
                            for person in participants {
                                if ui
                                    .add_enabled(
                                        !self.busy,
                                        egui::Button::new(format!(
                                            "{} · Profile & development",
                                            s(&person, "label")
                                        )),
                                    )
                                    .clicked()
                                {
                                    let id = s(&person, "persona").to_owned();
                                    self.inspected_persona = Some(id.clone());
                                    self.dialog = Some(Dialog::Development);
                                    self.send(Action::Development(id));
                                    ui.close();
                                }
                            }
                        });
                        if ui
                            .selectable_label(self.tab == 1, "Produced files")
                            .clicked()
                        {
                            self.tab = 1;
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Settings").clicked() {
                            self.show_preferences = !self.show_preferences;
                        }
                        if self.view.progress.is_some()
                            && ui
                                .add_enabled(
                                    !self.busy && !self.view.login,
                                    egui::Button::new("Account").frame(false),
                                )
                                .clicked()
                        {
                            self.dialog = Some(Dialog::Account);
                            if self.view.account.is_null() {
                                self.send(Action::Models);
                            }
                        }
                    });
                });
            });
        egui::TopBottomPanel::top("workspace-tabs")
            .frame(
                egui::Frame::new()
                    .fill(p.header)
                    .inner_margin(egui::Margin::symmetric(9, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can_switch,
                            egui::Button::new("Workspaces").selected(self.choose_progress),
                        )
                        .clicked()
                    {
                        self.discover();
                    }
                    if self.view.progress.is_some()
                        && ui
                            .selectable_label(
                                !self.choose_progress && self.tab != 5,
                                if self.view.ready {
                                    "Workspace"
                                } else {
                                    "Setup"
                                },
                            )
                            .clicked()
                    {
                        self.choose_progress = false;
                        self.tab = 0;
                    }
                    if !self.choose_progress {
                        if self.view.ready
                            && ui.selectable_label(self.tab == 5, "Overview").clicked()
                        {
                            self.tab = 5;
                        }
                        ui.separator();
                        let title = self
                            .view
                            .root
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();
                        ui.add(egui::Label::new(RichText::new(title).small().weak()).truncate())
                            .on_hover_text(self.view.root.display().to_string());
                    }
                });
            });
    }
    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(ctx.style().visuals.dark_mode);
        egui::TopBottomPanel::bottom("status-bar")
            .frame(
                egui::Frame::new()
                    .fill(p.field)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let busy = self.busy || self.view.loading_models || self.view.login;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        3.0,
                        if busy || self.view.running.is_some() {
                            theme::ACCENT
                        } else {
                            theme::GREEN
                        },
                    );
                    let status = if self.closing {
                        "Waiting for the session outcome…"
                    } else if self.view.loading_models {
                        "Discovering account models…"
                    } else if self.view.login {
                        "Waiting for sign-in…"
                    } else if self.busy {
                        "Loading saved records…"
                    } else if self.view.running.is_some() {
                        "Session running"
                    } else if self.view.read_only {
                        "Viewing live progress"
                    } else {
                        "Ready"
                    };
                    ui.label(RichText::new(status).size(11.0));
                    if let Some(since) = self.running_since {
                        ui.label(
                            RichText::new(format!(
                                "{}:{:02}",
                                since.elapsed().as_secs() / 60,
                                since.elapsed().as_secs() % 60
                            ))
                            .monospace()
                            .size(11.0),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("Personas {}", env!("CARGO_PKG_VERSION")))
                                .size(10.5)
                                .color(p.muted),
                        );
                        if ui.available_width() > 260.0 {
                            ui.separator();
                            ui.label(
                                RichText::new(if self.choose_progress {
                                    "Click to inspect · Double-click to open"
                                } else {
                                    "Drag panel borders to resize"
                                })
                                .size(10.5)
                                .color(p.muted),
                            );
                        }
                    });
                });
            });
    }
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("task-outliner")
            .default_width(220.0)
            .width_range(175.0..=350.0)
            .frame(theme::panel(ctx))
            .show(ctx, |ui| {
                let works = models_at(&self.view.inspection, "works").to_vec();
                theme::header(ui, Icon::Task, "Task outliner", &works.len().to_string());
                ui.add(
                    egui::TextEdit::singleline(&mut self.task_filter)
                        .hint_text("Filter tasks…")
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy && !self.view.read_only && self.view.running.is_none(),
                            egui::Button::new("New task"),
                        )
                        .clicked()
                    {
                        self.dialog = Some(Dialog::Task);
                        self.text.clear();
                    }
                    ui.add_enabled_ui(!self.busy && !self.view.read_only && self.view.running.is_none(), |ui| {
                        ui.menu_button("Add", |ui| {
                            if ui
                                .button("Start exploration")
                                .on_hover_text("Invite voluntary conversation and exploration.")
                                .clicked()
                            {
                                self.send(Action::NewTask {
                                    text: String::new(),
                                    optional: true,
                                });
                                ui.close();
                            }
                        });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    theme::glyph(ui, Icon::Folder, theme::ACCENT);
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                self.view
                                    .root
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy(),
                            )
                            .small(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(self.view.root.display().to_string());
                });
                let filter = self.task_filter.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("task-outliner-list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        for work in works.iter().filter(|w| {
                            filter.is_empty()
                                || s(w, "need").to_lowercase().contains(&filter)
                                || s(w, "id").contains(&filter)
                        }) {
                            let id = s(work, "id").to_owned();
                            let title = if work["optional_activity"] == true {
                                "Exploration & conversation".into()
                            } else {
                                short(s(work, "need"), 55)
                            };
                            let selected = self.view.selected.as_ref() == Some(&id);
                            let response = ui.add_enabled(
                                !self.busy,
                                egui::Button::new(format!(
                                    "{title}\n{} · {}",
                                    id,
                                    s(work, "phase")
                                ))
                                .selected(selected)
                                .wrap()
                                .min_size(egui::vec2(ui.available_width(), 54.0)),
                            );
                            if response.on_hover_text(s(work, "need")).clicked() {
                                self.inspected_persona = None;
                                self.conversation_persona = None;
                                self.conversation_route = None;
                                self.conversation_page = 0;
                                self.tab = 0;
                                self.send(Action::Select(id));
                            }
                        }
                        if works.is_empty() {
                            ui.label(RichText::new("No tasks yet").weak());
                        }
                    });
            });
    }
    pub(super) fn people(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("persona-inspector")
            .default_width(250.0)
            .width_range(210.0..=400.0)
            .frame(theme::panel(ctx))
            .show(ctx, |ui| self.persona_properties(ui));
    }
    pub(super) fn people_popup(&mut self, ctx: &egui::Context) {
        if !self.show_persona_properties {
            return;
        }
        let mut open = true;
        egui::Window::new("Persona properties")
            .open(&mut open)
            .default_width(300.0)
            .max_height((ctx.content_rect().height() - 80.0).max(240.0))
            .vscroll(true)
            .show(ctx, |ui| self.persona_properties(ui));
        self.show_persona_properties = open;
    }
    fn persona_properties(&mut self, ui: &mut Ui) {
        let work = self.selected().cloned().unwrap_or(Value::Null);
        let participants = models_at(&work, "participants");
        if !participants
            .iter()
            .any(|p| p["persona"].as_str() == self.inspected_persona.as_deref())
        {
            self.inspected_persona = participants.first().map(|p| s(p, "persona").to_owned());
        }
        theme::header(
            ui,
            Icon::People,
            "Personas",
            &participants.len().to_string(),
        );
        egui::ScrollArea::vertical()
            .id_salt("persona-outliner")
            .max_height(175.0)
            .show(ui, |ui| {
                for p in participants {
                    let selected = p["persona"].as_str() == self.inspected_persona.as_deref();
                    let color = if !p["active_invocation"].is_null() {
                        theme::GREEN
                    } else {
                        theme::palette(ui).muted
                    };
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 3.0, color);
                        if ui
                            .add_sized(
                                [ui.available_width(), 24.0],
                                egui::Button::new(s(p, "label")).selected(selected),
                            )
                            .clicked()
                        {
                            self.inspected_persona = Some(s(p, "persona").into());
                        }
                    });
                }
            });
        ui.separator();
        theme::header(ui, Icon::Settings, "Properties", "PERSONA");
        let Some(persona) = participants
            .iter()
            .find(|p| p["persona"].as_str() == self.inspected_persona.as_deref())
        else {
            ui.label(RichText::new("Select a task to inspect its personas.").weak());
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("persona-properties")
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(s(persona, "label")).size(15.0).strong());
                ui.label(RichText::new(s(persona, "persona")).small().weak());
                let continuation = theme::continuation_label(s(persona, "continuation"));
                theme::badge(
                    ui,
                    &continuation.to_uppercase(),
                    if persona["resume_candidate"] == true {
                        theme::ACCENT
                    } else {
                        theme::GREEN
                    },
                );
                egui::CollapsingHeader::new("Model")
                    .default_open(true)
                    .show(ui, |ui| {
                        let selection = &self.view.inspection["settings"]["persona_models"]
                            [s(persona, "persona")];
                        theme::property(ui, "Model", s(selection, "model"));
                        theme::property(ui, "Reasoning", s(selection, "effort"));
                    });
                egui::CollapsingHeader::new("Execution")
                    .default_open(true)
                    .show(ui, |ui| {
                        theme::property(ui, "State", &continuation);
                        if !persona["active_invocation"].is_null() {
                            theme::property(ui, "Call", s(&persona["active_invocation"], "state"));
                        }
                        if let Some(held) = persona["failed_attempt_hold"].as_str() {
                            ui.colored_label(theme::ACCENT, format!("Needs attention: {held}"));
                        }
                        for command in models_at(persona, "pending_host_commands") {
                            theme::property(ui, s(command, "operation"), s(command, "state"));
                        }
                        if persona["resume_candidate"] == true
                            && ui
                                .add_enabled(!self.busy && !self.view.read_only, egui::Button::new("Resume participant"))
                                .clicked()
                        {
                            self.send(Action::ResumePersona {
                                work: s(&work, "id").into(),
                                persona: s(persona, "persona").into(),
                            });
                        }
                    });
                egui::CollapsingHeader::new("Character & development")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(
                                "Inspect the persona's retained profile and development.",
                            )
                            .small()
                            .weak(),
                        );
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Profile & development"))
                            .clicked()
                        {
                            self.dialog = Some(Dialog::Development);
                            self.send(Action::Development(s(persona, "persona").into()));
                        }
                    });
            });
    }
    pub(super) fn workspace(&mut self, ui: &mut Ui) {
        if self.tab == 5 {
            egui::Frame::new().inner_margin(12).show(ui, |ui| {
                self.banner(ui);
                egui::ScrollArea::vertical()
                    .id_salt("workspace-overview-scroll")
                    .show(ui, |ui| self.overview(ui));
            });
            return;
        }
        let Some(work) = self.selected().cloned() else {
            theme::header(ui, Icon::Conversation, "Workspace", "");
            self.banner(ui);
            ui.add_space(40.0);
            ui.heading("Start with a task");
            ui.label("Create a task in the outliner, or invite your personas to explore.");
            return;
        };
        let id = s(&work, "id").to_owned();
        let p = theme::palette(ui);
        egui::TopBottomPanel::top("work-editor-header")
            .frame(egui::Frame::new().fill(p.panel).inner_margin(10))
            .show_inside(ui, |ui| {
                self.banner(ui);
                self.pending(ui);
                ui.horizontal(|ui| {
                    theme::glyph(
                        ui,
                        if work["optional_activity"] == true {
                            Icon::Conversation
                        } else {
                            Icon::Task
                        },
                        theme::ACCENT,
                    );
                    ui.label(
                        RichText::new(if work["optional_activity"] == true {
                            "Exploration & conversation"
                        } else {
                            "Task workspace"
                        })
                        .size(13.0)
                        .strong(),
                    );
                    theme::badge(
                        ui,
                        &s(&work, "phase").to_uppercase(),
                        if work["phase"] == "open" {
                            theme::GREEN
                        } else {
                            p.muted
                        },
                    );
                });
                ui.add(egui::Label::new(s(&work, "need")).truncate())
                    .on_hover_text(s(&work, "need"));
                ui.collapsing("Task brief", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("task-brief")
                        .max_height(110.0)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(s(&work, "need")).wrap().selectable(true));
                        });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(!self.view.read_only, |ui| {
                    if work["phase"] == "paused" {
                        if ui
                            .add_enabled(
                                !self.busy && self.view.running.is_none(),
                                theme::primary("Resume work"),
                            )
                            .clicked()
                        {
                            self.send(Action::Control {
                                work: id.clone(),
                                action: "resume".into(),
                            });
                        }
                    } else if work["phase"] == "open"
                        && self.view.running.is_none()
                        && ui
                            .add_enabled(!self.busy, theme::primary("Start / Continue"))
                            .clicked()
                    {
                        self.send(Action::Run(id.clone()));
                    }
                    if work["phase"] == "open"
                        && ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new("Pause").wrap_mode(egui::TextWrapMode::Extend),
                            )
                            .clicked()
                    {
                        self.send(Action::Control {
                            work: id.clone(),
                            action: "pause".into(),
                        });
                    }
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new("Feedback").wrap_mode(egui::TextWrapMode::Extend),
                        )
                        .on_hover_text("Give feedback for the next eligible decision")
                        .clicked()
                    {
                        self.dialog = Some(Dialog::Feedback);
                        self.text.clear();
                        self.replaced.clear();
                        self.feedback_persona = models_at(&work, "participants")
                            .first()
                            .map(|p| s(p, "persona").to_owned())
                            .unwrap_or_default();
                    }
                    egui::containers::menu::MenuButton::from_button(
                        egui::Button::new("Task").wrap_mode(egui::TextWrapMode::Extend),
                    )
                    .ui(ui, |ui| {
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Amend task"))
                            .clicked()
                        {
                            self.dialog = Some(Dialog::Amend);
                            self.send(Action::AmendSnapshot(id.clone()));
                            ui.close();
                        }
                        if work["phase"] != "ended"
                            && ui
                                .add_enabled(!self.busy, egui::Button::new("Cancel task"))
                                .clicked()
                        {
                            self.send(Action::Control {
                                work: id.clone(),
                                action: "cancel".into(),
                            });
                            ui.close();
                        }
                    });
                    });
                    ui.add_enabled_ui(!self.busy, |ui| {
                        if theme::icon_button(ui, Icon::Refresh, "Refresh task records").clicked() {
                            self.send(Action::Refresh);
                        }
                    });
                });
            });
        if self.show_session_strip && self.tab != 1 && self.tab != 5 {
            self.session_strip(ui, &id);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.canvas).inner_margin(10))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    theme::glyph(
                        ui,
                        [
                            Icon::Conversation,
                            Icon::Files,
                            Icon::Questions,
                            Icon::Activity,
                            Icon::Sessions,
                            Icon::Workspaces,
                        ][self.tab.min(5)],
                        p.muted,
                    );
                    for (i, label) in [
                        "Conversation",
                        "Produced files",
                        "Questions",
                        "Activity",
                        "Sessions",
                        "Overview",
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.tab, i, *label);
                    }
                });
                ui.separator();
                if self.tab == 1 {
                    self.files(ui, &id);
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt(("editor-content", &id, self.tab))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        0 => self.conversation(ui),
                        2 => self.questions(ui, &id),
                        3 => self.activity(ui, &work),
                        5 => self.overview(ui),
                        _ => self.sessions(ui, &id),
                    });
            });
    }
    fn session_strip(&mut self, ui: &mut Ui, work: &str) {
        let sessions: Vec<_> = self
            .view
            .progress
            .as_ref()
            .map(|p| {
                p.session_history
                    .iter()
                    .chain(p.session.iter())
                    .filter(|s| s.work == work)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let p = theme::palette(ui);
        let compact = ui.ctx().content_rect().height() < 700.0;
        egui::TopBottomPanel::bottom("session-history-strip")
            .default_height(if compact { 38.0 } else { 104.0 })
            .height_range(if compact { 38.0..=38.0 } else { 65.0..=240.0 })
            .resizable(!compact)
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .inner_margin(8)
                    .stroke(egui::Stroke::new(1.0_f32, p.border)),
            )
            .show_inside(ui, |ui| {
                let wide = ui.available_width() > 370.0;
                ui.horizontal(|ui| {
                    theme::glyph(ui, Icon::Sessions, p.muted);
                    ui.label(RichText::new("Session history").size(12.0).strong());
                    if wide {
                        ui.label(
                            RichText::new(format!("{} retained", sessions.len()))
                                .small()
                                .weak(),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Hide").clicked() {
                            self.show_session_strip = false;
                        }
                        if ui
                            .small_button(if wide { "View history" } else { "History" })
                            .clicked()
                        {
                            self.tab = 4;
                        }
                    });
                });
                if compact {
                    return;
                }
                ui.separator();
                if sessions.is_empty() {
                    ui.label(
                        RichText::new("Sessions will appear here after work starts.")
                            .small()
                            .weak(),
                    );
                } else {
                    egui::ScrollArea::horizontal()
                        .id_salt("session-strip-scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, session) in sessions.iter().enumerate() {
                                    if i > 0 {
                                        ui.label(RichText::new("—").weak());
                                    }
                                    let selected =
                                        self.view.selected_session.as_deref() == Some(&session.id);
                                    let is_current = i + 1 == sessions.len();
                                    let label = format!(
                                        "{:02}   {}",
                                        i + 1,
                                        if is_current {
                                            "Latest session"
                                        } else {
                                            "Earlier session"
                                        }
                                    );
                                    if ui
                                        .add_enabled(
                                            !self.busy,
                                            egui::Button::new(label).selected(selected),
                                        )
                                        .on_hover_text(&session.id)
                                        .clicked()
                                    {
                                        self.tab = 4;
                                        self.send(Action::SelectSession(session.id.clone()));
                                    }
                                }
                            });
                        });
                    ui.label(
                        RichText::new("Select a session to inspect its outcome and replies.")
                            .small()
                            .weak(),
                    );
                }
            });
    }
}
