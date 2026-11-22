use super::*;

const COMPACT_ARTIFACT_PADDING: i8 = 7;
const ARTIFACT_ROW_GAP: f32 = 4.;
impl App {
    pub(super) fn header(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        let tight = ctx.content_rect().width() < 340.;
        let short_toolbar = tight && ctx.content_rect().height() < 260.;
        if self.wide_navigation(ctx) {
            egui::TopBottomPanel::top("studio-topbar")
                .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(20, 7)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let parent = self.root.file_name().and_then(|s| s.to_str()).unwrap_or("Workspace");
                        let width = (ui.available_width() - 236.).clamp(80., 420.);
                        ui.allocate_ui(egui::vec2(width, 30.), |ui| design::breadcrumb(ui, parent, self.editor_name()));
                        design::action_at_right(ui, |ui| {
                            ui.horizontal(|ui| self.header_actions(ui, ctx, false, false));
                        });
                    });
                });
        } else {
        egui::TopBottomPanel::top("studio-menu")
            .frame(egui::Frame::new().fill(p.field).inner_margin(if tight {
                egui::Margin::symmetric(5, 5)
            } else {
                egui::Margin::symmetric(14, 9)
            }))
            .show(ctx, |ui| {
                let width = ui.available_width();
                if tight {
                    ui.spacing_mut().item_spacing.x = 4.;
                    ui.spacing_mut().button_padding.x = 5.;
                }
                ui.horizontal_wrapped(|ui| {
                    if theme::icon_button(ui, Icon::Workspaces, "Open workspace library").clicked()
                    {
                        self.open_library();
                    }
                    let waiting = self.home_index.requests.len();
                    let inbox_label = if waiting == 0 {
                        "Needs you".to_owned()
                    } else {
                        format!("Needs you · {waiting}")
                    };
                    let pages = [
                        (Editor::Home, "Overview"),
                        (Editor::Studio, "Activity"),
                        (Editor::People, "Personas"),
                        (Editor::Inbox, inbox_label.as_str()),
                        (Editor::Conversations, "Conversations"),
                        (Editor::Artifacts, "Artifacts"),
                        (Editor::Character, "Character Lab"),
                        (Editor::Atlas, "Character Atlas"),
                        (Editor::Usage, "Usage"),
                    ];
                    let active = match self.layout.editor {
                        Editor::Responsibilities | Editor::Agreements | Editor::Delivery => Editor::Studio,
                        other => other,
                    };
                    let current = match self.layout.editor {
                        Editor::Responsibilities => "Responsibility board",
                        Editor::Agreements => "Agreement book",
                        Editor::Delivery => "Delivery",
                        _ => pages.iter().find(|(page, _)| *page == active).unwrap().1,
                    };
                    let short = match self.layout.editor {
                        Editor::Responsibilities => "Duties",
                        Editor::Agreements => "Agreements",
                        Editor::Character => "Character",
                        Editor::Atlas => "Atlas",
                        Editor::Inbox => "Needs you",
                        _ => current,
                    };
                    let selector = format!("{} ↓", if short_toolbar { short } else { current });
                    let show_request = waiting > 0 && active != Editor::Inbox && !short_toolbar;
                    let request_label = if tight {
                        counted(waiting as u64, "request", "requests")
                    } else {
                        inbox_label.clone()
                    };
                    let wordmark = RichText::new("Persona Studio").size(16.).strong().color(p.text);
                    let measure = |text: egui::WidgetText, style| {
                        text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
                            .size()
                            .x
                            .ceil()
                    };
                    let gap = ui.spacing().item_spacing.x;
                    let padding = 2. * ui.spacing().button_padding.x;
                    let button_width =
                        |text: &str| measure(text.into(), egui::TextStyle::Button) + padding;
                    let brand_width = measure(wordmark.clone().into(), egui::TextStyle::Body)
                        + gap;
                    let actions_width = (if tight { 28. } else { button_width("Find…") })
                        + button_width("+ New")
                        + 28.
                        + 2. * gap;
                    let tab_width: f32 =
                        pages.iter().map(|(_, name)| button_width(name) + gap).sum();
                    let selector_width = button_width(&selector);
                    let request_width = button_width(&request_label);
                    let compact_width = selector_width
                        + gap
                        + if show_request {
                            request_width + gap
                        } else {
                            0.
                        };
                    let remaining = ui.available_size_before_wrap().x;
                    let request_first =
                        show_request && selector_width > remaining && request_width <= remaining;
                    let tabs = brand_width + tab_width + actions_width <= remaining;
                    if tabs || brand_width + compact_width + actions_width <= remaining {
                        ui.label(wordmark);
                    }
                    let mut go_to = None;
                    if tabs {
                        for (page, name) in pages {
                            if ui.selectable_label(active == page, name).clicked() {
                                go_to = Some(page);
                            }
                        }
                    } else {
                        let request_button = |ui: &mut Ui| {
                            ui.button(&request_label)
                                .on_hover_text(&inbox_label)
                                .clicked()
                        };
                        if request_first && request_button(ui) {
                            go_to = Some(Editor::Inbox);
                        }
                        let page_menu = ui.menu_button(&selector, |ui| {
                            ui.set_max_width((width - 10.).clamp(1., 180.));
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for (page, name) in pages {
                                    if ui.selectable_label(active == page, name).clicked() {
                                        go_to = Some(page);
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.button("File viewer").clicked() {
                                    file_desk::request(ctx, false);
                                    ui.close();
                                }
                            });
                        })
                        .response;
                        if !self.workspace_find.open {
                            page_menu.on_hover_text(if width < 180. {
                                "Pages"
                            } else {
                                "Choose a Studio page"
                            });
                        }
                        if show_request && !request_first && request_button(ui) {
                            go_to = Some(Editor::Inbox);
                        }
                    }
                    if let Some(page) = go_to {
                        self.layout.editor = page;
                        if page == Editor::Inbox {
                            self.inbox.active_list();
                        }
                    }
                    if short_toolbar {
                        ui.menu_button("More", |ui| {
                            self.header_actions(ui, ctx, false, true);
                        })
                        .response
                        .on_hover_text("Find, create and configure");
                    } else {
                        let remaining = ui.available_size_before_wrap().x;
                        if actions_width <= remaining {
                            ui.add_space(remaining - actions_width);
                        } else {
                            ui.end_row();
                        }
                        self.header_actions(ui, ctx, tight, false);
                    }
                });
            });
        }
        let workspace = self.layout.editor.is_workspace_view();
        let compact = Self::constrained_activity_layout(ctx);
        if workspace && (compact || !self.has_workspace_notices()) {
            return;
        }
        egui::TopBottomPanel::top("studio-context")
            .frame(egui::Frame::new().fill(p.header).inner_margin(if compact {
                egui::Margin::symmetric(5, 2)
            } else {
                egui::Margin::symmetric(14, 6)
            }))
            .show(ctx, |ui| {
                if !workspace {
                    let ended = self
                        .home_index
                        .activity(&self.work)
                        .is_some_and(|a| a.phase == "dissolved");
                    let mut browser = false;
                    let mut inspector = false;
                    if compact {
                        let screen = ctx.content_rect().size();
                        ui.horizontal(|ui| {
                            let menu = egui::containers::menu::MenuButton::new("Activity tools")
                                .config(
                                    egui::containers::menu::MenuConfig::new().close_behavior(
                                        egui::PopupCloseBehavior::CloseOnClickOutside,
                                    ),
                                );
                            let (response, _) = menu.ui(ui, |ui| {
                                ui.set_max_width((screen.x - 16.).clamp(1., 300.));
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                egui::ScrollArea::vertical()
                                    .id_salt("activity-tools")
                                    .max_height((screen.y - 18.).max(1.))
                                    .show(ui, |ui| {
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                self.context_title(ui, ended, true);
                                                ui.separator();
                                                ui.add_enabled_ui(!self.work.is_empty(), |ui| {
                                                    egui::CollapsingHeader::new("Participate")
                                                        .show_unindented(ui, |ui| {
                                                            self.activity_participation_options(
                                                                ui, ended,
                                                            );
                                                        });
                                                });
                                                egui::CollapsingHeader::new("Run controls")
                                                    .show_unindented(ui, |ui| {
                                                        self.activity_run_options(ui, ctx);
                                                    });
                                                if ui.button("Workspace browser").clicked() {
                                                    browser = true;
                                                    ui.close_kind(egui::UiKind::Menu);
                                                }
                                                if ui.button("Inspector").clicked() {
                                                    inspector = true;
                                                    ui.close_kind(egui::UiKind::Menu);
                                                }
                                            },
                                        );
                                    });
                            });
                            response.on_hover_text(self.task_title());
                            if ui.available_width() > 70. {
                                ui.add(egui::Label::new(self.task_title()).truncate())
                                    .on_hover_text(self.task_title());
                            }
                        });
                    } else {
                        let title_row = ui.available_width() < 720.;
                        if title_row {
                            ui.horizontal(|ui| self.context_title(ui, ended, false));
                        }
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.menu_button("Run controls", |ui| {
                                    self.activity_run_options(ui, ctx)
                                });
                                ui.add_enabled_ui(!self.work.is_empty(), |ui| {
                                    ui.menu_button("Participate", |ui| {
                                        self.activity_participation_options(ui, ended)
                                    });
                                });
                                inspector =
                                    theme::icon_button(ui, Icon::People, "Toggle inspector")
                                        .clicked();
                                browser = theme::icon_button(ui, Icon::Folder, "Workspace browser")
                                    .clicked();
                                if !title_row {
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| self.context_title(ui, ended, false),
                                    );
                                }
                            })
                        });
                    }
                    if inspector {
                        if ctx.content_rect().width() > 1100. {
                            self.layout.inspector = !self.layout.inspector;
                        } else {
                            self.inspector_window = !self.inspector_window;
                        }
                    }
                    if browser {
                        if ctx.content_rect().width() < COMPACT_BROWSER_WIDTH {
                            self.compact_browser = !self.compact_browser;
                        } else {
                            self.layout.outliner = !self.layout.outliner;
                        }
                    }
                }
                if !compact {
                    self.workspace_notices(ui);
                }
            });
    }

    pub(super) fn wide_navigation(&self, ctx: &egui::Context) -> bool {
        ctx.content_rect().width() >= 1180.
    }

    fn editor_name(&self) -> &'static str {
        match self.layout.editor {
            Editor::Home => "Overview", Editor::Studio => "Activity", Editor::Delivery => "Delivery",
            Editor::People => "Personas", Editor::Inbox => "Needs you", Editor::Conversations => "Conversations",
            Editor::Artifacts => "Artifacts", Editor::Character => "Character Lab", Editor::Atlas => "Character Atlas",
            Editor::Usage => "Usage", Editor::Responsibilities => "Responsibility board", Editor::Agreements => "Agreement book",
        }
    }

    pub(super) fn navigation_rail(&mut self, ctx: &egui::Context) {
        let operator = self.operator.clone();
        design::rail(ctx, "workspace-rail", &operator, "Workspace operator", |ui| {
            design::rail_group(ui, "Studio");
            if design::rail_link(ui, Icon::Folder, "Workspace library", false).clicked() { self.open_library(); }
            let waiting = self.home_index.requests.len();
            let inbox = if waiting == 0 { "Needs you".into() } else { format!("Needs you  ·  {waiting}") };
            for (group, pages) in [
                ("Workspace", &[(Editor::Home, Icon::Workspaces, "Overview"), (Editor::People, Icon::People, "Personas"),
                    (Editor::Studio, Icon::Activity, "Activity"), (Editor::Conversations, Icon::Conversation, "Conversations"),
                    (Editor::Inbox, Icon::Questions, inbox.as_str())][..]),
                ("Explore", &[(Editor::Artifacts, Icon::Folder, "Artifacts"), (Editor::Character, Icon::Settings, "Character Lab"),
                    (Editor::Atlas, Icon::Map, "Character Atlas"), (Editor::Usage, Icon::Sessions, "Usage")][..]),
            ] {
                design::rail_group(ui, group);
                for &(page, icon, label) in pages {
                    let selected = self.layout.editor == page || (page == Editor::Studio &&
                        matches!(self.layout.editor, Editor::Responsibilities | Editor::Agreements | Editor::Delivery));
                    if design::rail_link(ui, icon, label, selected).clicked() {
                        self.layout.editor = page;
                        if page == Editor::Inbox { self.inbox.active_list(); }
                    }
                }
            }
            if design::rail_link(ui, Icon::Files, "File viewer", false).clicked() { file_desk::request(ctx, false); }
        });
    }

    fn open_library(&mut self) {
        if self.running {
            if let Err(error) = std::env::current_exe().and_then(|exe| {
                std::process::Command::new(exe).arg("--operator").arg(&self.operator).spawn()
            }) {
                self.error = Some(error.to_string());
            }
        } else if !self.busy {
            self.show_library = true;
        }
    }

    fn header_actions(&mut self, ui: &mut Ui, ctx: &egui::Context, tight: bool, in_menu: bool) {
        let find = if tight {
            theme::icon_button(ui, Icon::Search, &self.find_hint(ctx))
        } else {
            ui.button("Find…").on_hover_text(self.find_hint(ctx))
        };
        if find.clicked() {
            self.open_workspace_find(ctx);
            if in_menu {
                ui.close();
            }
        }
        ui.menu_button("+ New", |ui| {
            for (social, label) in [(false, "Task"), (true, "Social conversation")] {
                if ui.button(label).clicked() {
                    self.dialog = Some(Dialog::activity(social));
                    ui.close();
                }
            }
            if ui.button("File viewer…").clicked() { file_desk::request(ctx, false); ui.close(); }
            if ui.button("Persona").clicked() {
                self.dialog = Some(Dialog::persona(&self.persona));
                ui.close();
            }
        });
        let settings = if in_menu {
            ui.button("Settings")
        } else {
            theme::icon_button(ui, Icon::Settings, "Workspace settings")
        };
        if settings.clicked() {
            if in_menu {
                ui.close();
            }
            self.dialog = Some(Dialog::Settings(Default::default()));
        }
    }

    pub(super) fn constrained_activity_layout(ctx: &egui::Context) -> bool {
        let size = ctx.content_rect().size();
        size.x < 320. || (size.x < 720. && size.y < 360.)
    }

    fn activity_run_options(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        self.finish_run_button(ui);
        if self
            .records("controls")
            .iter()
            .any(|c| c["scope"] == "admission" && c["state"] != "running")
            && ui
                .add_enabled(!self.busy, egui::Button::new("Enable new calls"))
                .clicked()
        {
            self.command(
                ctx,
                Command::Control {
                    scope: "admission".into(),
                    state: "running".into(),
                    reason: "User enabled new calls from Studio".into(),
                },
            );
            ui.close_kind(egui::UiKind::Menu);
        }
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
            ui.close_kind(egui::UiKind::Menu);
        }
        if !self.work.is_empty()
            && ui
                .add_enabled(
                    self.can_start_run(),
                    egui::Button::new("Run once until waiting"),
                )
                .clicked()
        {
            self.command(
                ctx,
                Command::Run {
                    watch: false,
                    work: None,
                    persona: None,
                    from_activity: Some(self.work.clone()),
                },
            );
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui
            .add_enabled(
                self.can_start_run(),
                egui::Button::new("Continue all eligible work"),
            )
            .clicked()
        {
            self.command(
                ctx,
                Command::Run {
                    watch: false,
                    work: None,
                    persona: None,
                    from_activity: None,
                },
            );
            ui.close_kind(egui::UiKind::Menu);
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
            ui.close_kind(egui::UiKind::Menu);
        }
        ui.weak("Explicitly stopped participants stay stopped until resumed.");
        for (scope, label) in [
            (format!("work:{}", self.work), "This activity"),
            (
                format!("persona:{}", self.persona),
                "Selected persona across activities",
            ),
        ] {
            if scope.ends_with(':') {
                continue;
            }
            ui.separator();
            ui.small(label);
            ui.horizontal_wrapped(|ui| {
                for (state, action) in [
                    ("running", "Enable"),
                    ("paused", "Pause"),
                    ("canceled", "Cancel"),
                ] {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new(action))
                        .clicked()
                    {
                        self.command(
                            ctx,
                            Command::Control {
                                scope: scope.clone(),
                                state: state.into(),
                                reason: format!(
                                    "Operator selected {action} for {scope} in the desktop"
                                ),
                            },
                        );
                        ui.close_kind(egui::UiKind::Menu);
                    }
                }
            });
        }
    }

    fn activity_participation_options(&mut self, ui: &mut Ui, ended: bool) {
        if ui.button("Agreement book").clicked() {
            self.show_agreements(None);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Responsibility board").clicked() {
            self.layout.editor = Editor::Responsibilities;
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Requested activities").clicked() {
            self.requested_activities
                .open(ui.ctx(), &self.root, &self.work, self.cursor);
            ui.close_kind(egui::UiKind::Menu);
        }
        for (action, label) in [
            (dialogs::Action::Message, "Write message"),
            (dialogs::Action::Amend, "Amend brief"),
            (dialogs::Action::Invite, "Invite a persona"),
        ] {
            if ui
                .add_enabled(
                    !ended || action == dialogs::Action::Message,
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.dialog = Some(self.participation_dialog(action));
                ui.close_kind(egui::UiKind::Menu);
            }
        }
        if ui.add_enabled(!ended, egui::Button::new("Resume participant")).clicked() {
            self.dialog = Some(self.resume_dialog());
            ui.close_kind(egui::UiKind::Menu);
        }
    }

    fn has_workspace_notices(&self) -> bool {
        self.error.is_some()
            || !self.notice.is_empty()
            || self
                .records("controls")
                .iter()
                .any(|c| c["state"] != "running" && !s(c, "scope").starts_with("job:"))
    }

    fn workspace_notices(&mut self, ui: &mut Ui) {
        let mut enable = None;
        for control in self
            .records("controls")
            .iter()
            .filter(|c| c["state"] != "running" && !s(c, "scope").starts_with("job:"))
        {
            let scope = s(control, "scope");
            ui.horizontal_wrapped(|ui| {
                if let Some(id) = scope.strip_prefix("persona:").filter(|_| control["state"] == "paused") {
                    if ui.add_enabled(!self.busy, egui::Button::new(format!("Enable {}", self.label(id))).small())
                        .on_hover_text("Allow this persona’s eligible work to be scheduled again.").clicked() {
                        enable = Some(scope.to_owned());
                    }
                }
                ui.colored_label(theme::ACCENT, format!("{scope} · {} — {}", s(control, "state"), s(control, "reason")));
            });
        }
        if let Some(scope) = enable {
            self.command(ui.ctx(), Command::Control { reason: format!("Operator enabled scheduling for {scope} in Studio"), scope, state: "running".into() });
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
    }

    fn context_title(&self, ui: &mut Ui, ended: bool, full: bool) {
        let p = theme::palette(ui);
        if !full {
            theme::glyph(ui, Icon::Task, p.muted);
        }
        if ended {
            theme::badge(ui, "Activity ended", theme::ACCENT);
        }
        let worker = |e: &Value| {
            e["work"] == self.work
                && self
                    .runner_observation
                    .has_worker(&self.work, s(e, "actor"))
        };
        let calls = self.snapshot.active.iter().filter(|e| worker(e)).count() as u64;
        let tools = self
            .snapshot
            .active_effects
            .iter()
            .filter(|e| e["state"] == "dispatched" && worker(e))
            .count() as u64;
        if calls > 0 {
            theme::badge(ui, &counted(calls, "model call", "model calls"), TEAL);
        }
        if tools > 0 {
            theme::badge(
                ui,
                &counted(tools, "tool running", "tools running"),
                theme::ACCENT,
            );
        }
        let title = egui::Label::new(RichText::new(self.task_title()).strong());
        if full {
            ui.add(title.wrap());
        } else {
            ui.add(title.truncate()).on_hover_text(self.task_title());
        }
    }
    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        let compact = Self::constrained_activity_layout(ctx);
        egui::TopBottomPanel::bottom("studio-status")
            .frame(egui::Frame::new().fill(p.field).inner_margin(if compact {
                egui::Margin::symmetric(5, 2)
            } else {
                egui::Margin::symmetric(12, 4)
            }))
            .show(ctx, |ui| {
                let status = self.runner_status_text();
                let counts = self.snapshot.status.is_object().then(|| {
                    let accepted = self.snapshot.status["invocations"]["accepted"]
                        .as_u64()
                        .unwrap_or(0);
                    [
                        counted(accepted, "accepted decision", "accepted decisions"),
                        counted(self.records("personas").len() as u64, "persona", "personas"),
                        counted(self.snapshot.assets.len() as u64, "artifact", "artifacts"),
                    ]
                });
                if compact {
                    // Keep the scrollable work area in view when several physical pixels
                    // represent one interface point. The menu retains the full observed scope.
                    let state = match status.as_str() {
                        "Workspace connected" => "Connected",
                        "Opening workspace…" => "Opening…",
                        "Waiting for owned work to settle" => "Closing…",
                        "Starting or settling run…" => "Run status",
                        "Runner observation unavailable" => "Status unknown",
                        other => other.split(" · ").next().unwrap_or(other),
                    };
                    let notices = self.has_workspace_notices();
                    let label = if notices {
                        format!("{state} !")
                    } else {
                        format!("{state} ↓")
                    };
                    let screen = ctx.content_rect().size();
                    ui.menu_button(label, |ui| {
                        ui.set_max_width((screen.x - 16.).clamp(1., 260.));
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        egui::ScrollArea::vertical()
                            .id_salt("compact-workspace-status")
                            .max_height((screen.y - 18.).max(1.))
                            .show(ui, |ui| {
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.label(&status);
                                    if ui
                                        .selectable_label(self.layout.activity, "Event log")
                                        .clicked()
                                    {
                                        self.layout.activity = !self.layout.activity;
                                        ui.close();
                                    }
                                    if notices {
                                        self.workspace_notices(ui);
                                    }
                                    if let Some(counts) = &counts {
                                        ui.separator();
                                        for value in counts {
                                            ui.label(value);
                                        }
                                    }
                                });
                            });
                    })
                    .response
                    .on_hover_text(&status);
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(8., 8.), egui::Sense::hover());
                    let working = matches!(
                        &self.runner_observation,
                        ai_personas::neural::live::RunnerObservation::Observed { workers, .. }
                            if !workers.is_empty()
                    );
                    ui.painter().circle_filled(
                        rect.center(),
                        3.,
                        if working { TEAL } else { p.muted },
                    );
                    let mut label = egui::WidgetText::from(RichText::new(&status).small())
                        .into_layout_job(
                            ui.style(),
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                    Arc::make_mut(&mut label).wrap.max_rows = 2;
                    ui.add(egui::Label::new(label).wrap())
                        .on_hover_text(&status);
                    let summary = counts.as_ref().map(|items| items.join("  ·  "));
                    let measure = |text: &str, style| {
                        egui::WidgetText::from(text)
                            .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
                            .size()
                            .x
                    };
                    let button_width = |text| {
                        measure(text, egui::TextStyle::Button) + 2. * ui.spacing().button_padding.x
                    };
                    let remaining = ui.available_size_before_wrap().x;
                    let log_width = button_width("Event log") + 2.;
                    let gap = ui.spacing().item_spacing.x;
                    let inline_width = summary
                        .as_deref()
                        .map(|text| measure(text, egui::TextStyle::Small) + gap);
                    let inline = inline_width.is_some_and(|w| w + log_width <= remaining);
                    let controls_width = log_width
                        + match inline_width {
                            Some(width) if inline => width,
                            Some(_) => button_width("Counts") + gap,
                            None => 0.,
                        };
                    if controls_width <= remaining {
                        ui.add_space(remaining - controls_width);
                    } else {
                        ui.end_row();
                    }
                    if let (Some(values), Some(summary)) = (counts, summary) {
                        if inline {
                            ui.small(summary);
                        } else {
                            ui.menu_button("Counts", |ui| {
                                ui.set_max_width(
                                    (ctx.content_rect().width() - 26.).clamp(1., 240.),
                                );
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.label(&status);
                                    ui.separator();
                                    for value in values {
                                        ui.label(value);
                                    }
                                });
                            });
                        }
                    }
                    if ui
                        .selectable_label(self.layout.activity, "Event log")
                        .clicked()
                    {
                        self.layout.activity = !self.layout.activity;
                    }
                });
            });
    }
    pub(super) fn outliner(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        let compact = ctx.content_rect().width() < COMPACT_BROWSER_WIDTH;
        let left_width = self.layout.left_width;
        let contents = |ui: &mut Ui| {
                if !compact { eyebrow(ui, "Workspace browser"); ui.add_space(12.); }
                egui::ScrollArea::vertical()
                    .id_salt("outliner-scroll")
                    .show(ui, |ui| {
                        section(ui, "Activities", &self.records("work").len().to_string());
                        ui.add(egui::TextEdit::singleline(&mut self.activity_filter_text).hint_text("Find an activity…").desired_width(f32::INFINITY));
                        let query=self.activity_filter_text.to_lowercase();
                        let mut selected = None;
                        egui::ScrollArea::vertical().id_salt("browser-activities").max_height(176.).auto_shrink([false,true]).show(ui,|ui|{
                            for activity in self.home_index.activities.iter().filter(|a|query.is_empty() || a.title.to_lowercase().contains(&query)) {
                                if ui.add_sized([ui.available_width(),28.],egui::Button::new(excerpt(&activity.title,36)).selected(activity.id==self.work).truncate())
                                    .on_hover_text(&activity.title).clicked() {
                                    selected = Some(activity.id.clone());
                                }
                            }
                        });
                        if let Some(id) = selected {
                            self.compact_browser = false;
                            if matches!(self.layout.editor, Editor::Inbox | Editor::Delivery | Editor::Responsibilities) { self.open_home_activity(&id); }
                            else { self.select_activity(&id); }
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
                                            self.show_person(id);
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
                                                self.show_person(id);
                                            }
                                            ui.label(RichText::new(state).size(11.).color(p.muted));
                                        });
                                    });
                                })
                                .response;
                            let response = response.interact(egui::Sense::click());
                            if response.clicked() {
                                self.show_person(id);
                            }
                            if response.double_clicked() {
                                self.show_person(id);
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
                            self.compact_browser = false;
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
                            self.compact_browser = false;
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
                            self.compact_browser = false;
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
        };
        if compact {
            let mut open = true;
            egui::Window::new("Workspace browser").id(egui::Id::new("compact-workspace-browser"))
                .open(&mut open).default_width(260.).default_height(400.).collapsible(false)
                .show(ctx, contents);
            self.compact_browser &= open;
        } else {
            let panel = egui::SidePanel::left("studio-outliner").resizable(true)
                .default_width(left_width).width_range(170.0..=360.0)
                .frame(egui::Frame::new().fill(p.panel).inner_margin(12)).show(ctx, contents);
            self.layout.left_width = panel.response.rect.width();
        }
    }
    pub(super) fn persona_state(&self, id: &str) -> String {
        self.persona_state_in(id, &self.work)
    }

    pub(super) fn persona_state_in(&self, id: &str, work: &str) -> String {
        if let Some(active) = self.snapshot.active.iter().find(|a| a["actor"] == id && a["work"] == work) {
            return live_board::call_stage(active, ai_personas::neural::now(), self.runner_observation.has_worker(work,id)).into();
        }
        if let Some(effect) = self
            .snapshot
            .active_effects
            .iter()
            .filter(|e| e["actor"] == id && e["work"] == work)
            .min_by_key(|e| match s(e, "state") { "dispatched" => 0, "uncertain" => 1, _ => 2 })
        {
            return live_board::effect_stage(effect,self.runner_observation.has_worker(work,id)).into();
        }
        let participation = self.records("participation").iter()
            .map(|r| &r["body"]).find(|p| p["persona"] == id && p["work"] == work);
        if let Some(p) = participation {
            if p["closed_by_activity"].is_string() || self.records("work").iter()
                .any(|w| w["body"]["id"] == work && w["body"]["phase"] == "dissolved") {
                return "Activity ended".into();
            }
            if p["departure"].is_string() { return "Left this activity".into(); }
            if p["declined"] == true { return "Declined invitation".into(); }
        }
        if self.snapshot.active.iter().any(|a| a["actor"] == id && self.runner_observation.has_worker(s(a,"work"),id)) {
            return "Model call on another activity".into();
        }
        if self.snapshot.active_effects.iter().any(|e| e["actor"] == id && e["state"] == "dispatched" && self.runner_observation.has_worker(s(e,"work"),id)) {
            return "Running a tool on another activity".into();
        }
        let Some(p) = participation else { return "No task participation".into(); };
        let resuming = p["pending_resume"].is_object();
        if !resuming && p["accepted"]!=true{return "Invited".into();}
        if resuming || !matches!(s(&p["continuation"], "kind"), "stop" | "stopped") {
            if let Some(control) = self.records("controls").iter().find(|c| {
                let scope = s(c, "scope");
                c["state"] != "running" && (matches!(scope, "global" | "admission")
                    || scope.strip_prefix("persona:") == Some(id)
                    || scope.strip_prefix("work:") == Some(work))
            }) {
                return format!("Scheduling {}", s(control, "state"));
            }
            if let Some(recovery) = self.snapshot.inference_recovery.get(id) {
                let seconds = recovery["not_before_unix"].as_i64().unwrap_or(0)
                    .saturating_sub(ai_personas::neural::now());
                return if seconds > 0 { format!("Inference recovery · {seconds}s") }
                    else { "Inference recovery eligible".into() };
            }
        }
        if resuming { return "Resume requested".into(); }
        match s(&p["continuation"], "kind") {
            "ready" => "Ready to continue",
            "wait" | "waiting" => "Chose to wait",
            "stop" | "stopped" => "Stopped on this task",
            _ => "Participating",
        }
        .into()
    }
    fn peer_exchange(event:&Value)->bool {
        let kind=s(event,"kind");
        let actor=s(event,"actor");
        let recipient=if matches!(kind,"request_message_sent"|"request_review_sent"){s(&event["body"],"to")}else{s(event,"target")};
        if event["invocation"].is_null() || actor==recipient {return false;}
        match kind {
            "message"|"reply"|"delivery"=>recipient!="user" && recipient!="*",
            "asset_feedback"=>event["body"]["author_kind"]=="persona" && event["body"]["recipient_kind"]=="persona",
            "invitation"|"contribution_proposal"=>true,
            _=>requested_activities::label(kind).is_some(),
        }
    }
    pub(super) fn studio(&mut self, ui: &mut Ui) {
        if self.show_activity_view(ui) {
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("studio-overview",self.work.clone()))
            .show(ui, |ui| {
                self.activity_overview(ui);
                if self.work.is_empty() { return; }
                ui.add_space(12.);
                ui.collapsing("Collaboration map",|ui| self.collaboration_map(ui));
                ui.add_space(14.);
                let messages: Vec<Value> = self
                    .events
                    .iter()
                    .rev()
                    .filter(|e| {
                        e["work"] == self.work && matches!(s(e, "kind"), "message" | "reply" | "request_reply" | "requester_message" | "requester_review")
                    })
                    .take(3)
                    .cloned()
                    .collect();
                let assets: Vec<Value> = self
                    .snapshot
                    .assets
                    .iter()
                    .filter(|a| a["body"]["work"] == self.work && a["body"]["retired"] != true && a["current"] != false)
                    .take(6)
                    .cloned()
                    .collect();
                let column_gap = ui.spacing().item_spacing.x;
                ui.spacing_mut().item_spacing.x = 18.;
                ui.columns(2, |cols| {
                    section(&mut cols[0], "Recent conversations", "");
                    for message in &messages {
                        card(&cols[0]).show(&mut cols[0], |ui| {
                            self.message_heading(ui, message);
                            ui.add_space(6.);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min),|ui|{
                                ui.add(egui::Label::new(excerpt(s(&message["body"], "text"), 230)).wrap());
                            });
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
                    section(&mut cols[1], "Created here", "Recent files");
                    if assets.is_empty(){cols[1].weak("Files created in this activity will appear here.");}
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
                let outcomes:Vec<_>=self.records("outcomes").iter().filter(|o|o["body"]["work"]==self.work).cloned().collect();
                for outcome in outcomes {
                    let b = &outcome["body"];
                    let reference=format!("outcome:{}@{}",s(&outcome["reference"],"id"),outcome["reference"]["revision"]);
                    let ownership=self.snapshot.status["outcome_ownership"][&reference].clone();
                    let gap=ownership["state"]=="follow_through_gap";
                    let unowned=ownership["state"]=="unowned";
                    ui.collapsing(
                        format!(
                            "{}{} · {}",
                            if gap{"Follow-through gap · "}else{""},
                            if unowned{"Unowned"}else{self.label(s(b, "owner"))},
                            excerpt(s(b, "description"), 55)
                        ),
                        |ui| {
                            ui.label(s(b, "description"));
                            if unowned{ui.label("The former participant released this duty. A volunteer can explicitly accept it.");}
                            if gap{ui.label("The named owner stopped or has no accepted participation. This outcome remains unfinished.");}
                            if let Some(accepted)=ownership["accepted_handoff"].as_str(){
                                if ui.small_button("Accepted handoff ↗").clicked(){self.open_resource(ui.ctx(),accepted.into());}
                            }
                            for item in b["remaining"].as_array().into_iter().flatten() {
                                ui.label(format!("• {}", item.as_str().unwrap_or("")));
                            }
                        },
                    );
                }
                let offers:Vec<_>=self.records("handoffs").iter().filter(|h|h["work"]==self.work).cloned().collect();
                if !offers.is_empty(){
                    ui.add_space(12.);section(ui,"Responsibility handoffs",&offers.len().to_string());
                    for h in offers{
                        ui.horizontal_wrapped(|ui|{
                            ui.strong(format!("{} → {}",self.label(s(&h,"from")),self.label(s(&h,"to"))));
                            theme::badge(ui,if h["basis_current"]==false{"Original scope changed"}else{s(&h,"state")},PURPLE);
                            if ui.small_button("Inspect handoff ↗").clicked(){self.open_resource(ui.ctx(),s(&h,"reference").into());}
                        });
                        ui.label(h["answer"]["reason"].as_str().unwrap_or(s(&h,"reason")));
                    }
                }
            });
    }
    fn collaboration_map(&mut self, ui: &mut Ui) {
        let people: Vec<Value> = self.records("personas").iter()
            .filter(|p| self.work.is_empty() || !self.participation(s(&p["body"], "id")).is_null()).cloned().collect();
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
                if self.snapshot.active.iter().any(|a| a["actor"] == id && a["work"] == self.work) {
                    "INFERENCE"
                } else if self
                    .snapshot
                    .active_effects
                    .iter()
                    .any(|a| a["actor"] == id && a["work"] == self.work)
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
    pub(super) fn message_heading(&mut self, ui: &mut Ui, event: &Value) {
        let actor = s(event, "actor");
        let target = if matches!(s(event,"kind"),"request_message_sent"|"request_review_sent"){s(&event["body"],"to")}else{s(event,"target")};
        let published = event["kind"] == "asset_feedback";
        let operator = published && event["body"]["author_kind"] == "operator";
        let from = if operator { format!("Operator · {}", s(&event["body"],"author")) } else { self.label(actor).to_owned() };
        let to = if published && event["body"]["recipient_kind"] == "operator" {
            s(&event["body"],"recipient")
        } else if target == "*" {
            "Everyone"
        } else if target == "user" {
            "You"
        } else {
            self.label(target)
        }
        .to_owned();
        ui.horizontal_wrapped(|ui| {
            if avatar(ui, &from, self.color(actor), 24.).clicked() && !operator {
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
                        "message" | "reply" | "user_message" | "delivery" | "initialization_note" | "invitation" | "contribution_proposal" | "asset_feedback" | "request_reply" | "request_delivery" | "request_message_sent" | "request_review_sent" | "requester_message" | "requester_review" | "request_joined" | "request_declined" | "request_left"
                    )
                    && !(kind == "delivery" && target == "*")
                    && (self.conversation_persona.is_empty()
                        || actor == self.conversation_persona
                        || target == self.conversation_persona || e["body"]["to"]==self.conversation_persona)
                    && match self.conversation_channel {
                        1 => Self::peer_exchange(e),
                        2 => kind == "user_message" || target == "user"
                            || (kind == "asset_feedback" && (e["body"]["author_kind"]=="operator" || e["body"]["recipient_kind"]=="operator"))
                            || (kind == "invitation" && e["invocation"].is_null()),
                        3 => matches!(kind,"delivery"|"request_delivery"|"requester_review"|"request_review_sent"),
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
                            match event["kind"].as_str() {
                                Some("invitation") => { theme::badge(ui, "Invitation", TEAL); }
                                Some("contribution_proposal") => { theme::badge(ui, "Contribution proposal", PURPLE); }
                                Some(kind) if requested_activities::label(kind).is_some() => {
                                    let (name,color)=requested_activities::label(kind).unwrap();
                                    theme::badge(ui,name,color);
                                    if ui.small_button("Follow requested activity →").clicked(){
                                        self.requested_activities.open_request(ui.ctx(),&self.root,&self.work,event["body"]["request"].as_str(),self.cursor);
                                    }
                                }
                                _ => {}
                            }
                            if let Some(parent) = event["body"]["reply_to"].as_str() {
                                if ui.small_button("In reply to ↗").on_hover_text(parent).clicked() {
                                    self.open_resource(ui.ctx(),parent.into());
                                }
                            }
                            ui.add_space(9.);
                            if let Some(text) = event["body"]["text"].as_str().or_else(|| event["body"]["contribution"].as_str()).or_else(||event["body"]["reason"].as_str()) {
                                if !text.trim().is_empty() {
                                    document::message_markdown(ui, text);
                                }
                                if event["kind"] == "user_message" && event["invocation"].is_null() {
                                    if let Some(sequence) = event["event"].as_i64() {
                                        self.source_file_links(ui, &event["body"]["attachments"], &format!("experience:event-{sequence}@1"));
                                    }
                                }
                                ui.horizontal_wrapped(|ui| {
                                if event["kind"]=="asset_feedback" && ui.small_button("View revision feedback…").clicked() {
                                    self.asset_feedback.open(ui.ctx(), &self.root, s(&event["body"],"asset"), &self.work);
                                }
                                if !text.is_empty() && ui.small_button("Copy message").clicked() {
                                    ui.ctx().copy_text(text.to_owned());
                                    self.notice="Original message copied".into();
                                }
                                if event["kind"]=="user_message" && event["invocation"].is_null()
                                    && ui.small_button("Delivery status").clicked() {
                                    self.open_message_receipt(ui.ctx(), event["event"].as_i64().unwrap_or_default());
                                }
                                if event["target"]=="user" && matches!(event["kind"].as_str(),Some("reply"|"message"))
                                    && ui.small_button("Reply to this message").clicked() {
                                    self.dialog=Some(dialogs::Dialog::Participation {
                                        work:self.work.clone(),title:self.task_title().into(),action:dialogs::Action::Message,
                                        person:s(event,"actor").into(),text:String::new(),
                                        sources:Default::default(),
                                        reply_to:Some((format!("experience:event-{}@1",event["event"]),excerpt(text,180))),
                                    });
                                }
                                });
                            } else if event["kind"] == "request_delivery" {
                                ui.strong(s(&event["body"],"title"));
                                ui.label(s(&event["body"],"supported_use"));
                                if ui.small_button("Open exact delivery ↗").clicked(){self.open_issued_package(ui.ctx(),s(&event["body"],"delivery"));}
                            } else if event["kind"] == "delivery" {
                                let body = &event["body"]["record"];
                                ui.heading(s(body, "title"));
                                ui.label(s(body, "supported_use"));
                                theme::badge(ui, s(body, "scope"), theme::ACCENT);
                                if let Some(reference) = event["body"]["reference"].as_str() {
                                    if ui.link("Open delivery ↗").clicked() { self.open_issued_package(ui.ctx(), reference); }
                                    self.review_controls(ui, reference, s(body, "title"));
                                }
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
    fn artifact_line_height(ui: &Ui) -> f32 {
        ui.spacing()
            .interact_size
            .y
            .max(ui.text_style_height(&egui::TextStyle::Body))
            .max(ui.text_style_height(&egui::TextStyle::Small))
            .max(16.)
    }
    pub(super) fn artifact_row(&mut self, ui: &mut Ui, asset: &Value, compact: bool) {
        let line_height = Self::artifact_line_height(ui);
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
            .inner_margin(if compact { COMPACT_ARTIFACT_PADDING } else { 10 })
            .corner_radius(4)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_height(line_height);
                    theme::glyph(ui, Icon::Files, theme::ACCENT);
                    ui.add(egui::Label::new(RichText::new(filename).strong()).truncate());
                });
                ui.horizontal(|ui| {
                    ui.set_height(line_height);
                    ui.add(
                        egui::Label::new(RichText::new(if extension.is_empty() {
                            "FILE"
                        } else {
                            &extension
                        })
                        .size(10.)
                        .color(theme::ACCENT)).truncate(),
                    );
                    let owner = s(b, "owner");
                    let author = b["last_content_author"].as_str().unwrap_or(owner);
                    let credit = if author == owner {
                        self.label(author).to_owned()
                    } else {
                        format!("By {} · owner {}", self.label(author), self.label(owner))
                    };
                    ui.add(egui::Label::new(RichText::new(&credit).small()).truncate()).on_hover_text(credit);
                    if let Some(version) = asset["revision"].as_u64() {
                        ui.add(egui::Label::new(RichText::new(format!("v{version}")).size(10.).weak()).truncate());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(size(asset["size"].as_u64().unwrap_or(0))).small()).truncate());
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
        self.delivery_return_link(ui);
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
            if ui.button("Package reviews…").clicked() {
                let delivery=self.selected_reference.starts_with("delivery:").then_some(self.selected_reference.as_str());
                self.package_reviews.open(ui.ctx(),&self.root,&self.work,delivery);
            }
            if ui.button("Search library…").clicked(){
                self.library_search.catalogue(self.asset_filter.clone());
            }
            egui::containers::menu::MenuButton::new("Open reference")
                .config(egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
                .ui(ui, |ui| {
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
            if ui.button("Command bench…").clicked() {
                self.command_bench.open = true;
            }
            if ui.button("Export deliveries…").clicked() {
                self.export_deliveries(ui.ctx());
            }
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
                                ui.label(format!("{} · {}", self.label(s(b, "owner")), title(s(b, "scope"))));
                                ui.label(s(b, "supported_use"));
                                self.delivery_review(ui, &d);
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
            ui.small(format!("{} {}", assets.len(), if assets.len() == 1 { "file" } else { "files" }));ui.add_space(5.);
            if assets.is_empty() {
                ui.weak("No artifacts match these filters.");
            } else {
                // Two single-line lanes and fixed padding keep card heights
                // stable, so offscreen cards need no widget layout or painting.
                let row_height = 2. * Self::artifact_line_height(ui)
                    + ui.spacing().item_spacing.y
                    + 2. * f32::from(COMPACT_ARTIFACT_PADDING)
                    + ARTIFACT_ROW_GAP;
                egui::ScrollArea::vertical()
                    .id_salt("retained-artifacts")
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, assets.len(), |ui, range| {
                        for index in range {
                            let asset = &assets[index];
                            ui.push_id(s(asset, "reference"), |ui| {
                                self.artifact_row(ui, asset, true);
                            });
                            ui.add_space(ARTIFACT_ROW_GAP);
                        }
                    });
            }
        });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::palette(ui).field)
                    .inner_margin(12),
            )
            .show_inside(ui, |ui| {
                if self.preview_pending {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Loading exact retained bytes…");
                        if self.selected_reference.starts_with("asset:") && ui.small_button("File history…").clicked() {
                            self.open_file_history(ui.ctx(), &self.selected_reference.clone());
                        }
                    });
                } else if self.selected_reference.is_empty() {
                    ui.add_space(50.);
                    theme::glyph(ui, Icon::Files, theme::ACCENT);
                    ui.heading("Select an artifact");
                    ui.label("Preview drawings, images, documents, data, 3D models and packages.");
                    ui.weak("The inspector shows its owner, exact version and qualifications.");
                } else {
                    ui.horizontal_wrapped(|ui| {
                        let projected=self.preview_metadata["view"]=="provider_receipt_metadata";
                        if !projected && (self.selected_reference.starts_with("asset:") || self.selected_reference.starts_with("object:"))
                            && print_preview::is_html(s(&self.preview_metadata,"path"), s(&self.preview_metadata, "media")) {
                            let print = ui.button("Print preview").on_hover_text("Render the print layout of this retained HTML");
                            if print.clicked() {
                                let path = self.preview_metadata["path"].as_str().unwrap_or("document.html").to_owned();
                                let reference = self.selected_reference.clone();
                                let package = self.print_package_for(&reference);
                                self.open_print_preview(ui.ctx(), &reference, &path, package, Some(print.id));
                            }
                        }
                        if ui.add_enabled(!self.preview_metadata.is_null(), egui::Button::new(if projected {"Save metadata…"} else {"Save file…"})).clicked() {
                            self.save_artifact(ui.ctx());
                        }
                        if ui.small_button("Copy reference").clicked() {
                            ui.ctx().copy_text(self.selected_reference.clone());
                        }
                        if self.selected_reference.starts_with("asset:") && ui.small_button("File history…").clicked() {
                            self.open_file_history(ui.ctx(), &self.selected_reference.clone());
                        }
                        if self.selected_reference.starts_with("asset:") && ui.small_button("Feedback…").clicked() {
                            self.asset_feedback.open(ui.ctx(), &self.root, &self.selected_reference, &self.work);
                        }
                        let path=s(&self.preview_metadata,"path");
                        if (path.to_ascii_lowercase().ends_with(".stl") || self.preview_metadata["format"]=="stl") && ui.small_button("Measure mesh…").clicked() {
                            self.mesh_profile.open(ui.ctx(),&self.root,&self.operator,&self.selected_reference,&self.work);
                        }
                        if self.selected_reference.starts_with("asset:") && path.to_ascii_lowercase().ends_with(".scad") && ui.button("Render model…").clicked() {
                            self.dialog=Some(Dialog::Render(super::render::RenderDialog::new(path,&self.selected_reference)));
                        }
                        if !projected && (self.selected_reference.starts_with("asset:") || self.selected_reference.starts_with("object:")) {
                            if ui.small_button("Use in command…").clicked() {
                                self.command_bench.mount(&self.selected_reference,path);
                            }
                            if ui.small_button("Find in this version…").clicked() {
                                self.library_search.document(self.selected_reference.clone());
                            }
                            if ui.add_enabled(!self.busy,egui::Button::new("Command receipts…").small()).clicked() {
                                let command=self.command_bench.locate(self.selected_reference.clone());
                                self.command(ui.ctx(),command);
                            }
                            if ui.small_button("Matching registrations…").clicked() {
                                self.library_search.registrations(ui.ctx(),&self.root,self.selected_reference.clone());
                            }
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
                let recoveries: Vec<_> = self.snapshot.inference_recovery.iter()
                    .filter(|(actor, _)| !self.participation(actor).is_null())
                    .map(|(actor, recovery)| (actor.clone(), recovery.clone())).collect();
                for (actor, recovery) in recoveries {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(theme::ACCENT, self.label(&actor));
                        ui.strong(self.persona_state(&actor));
                        ui.weak("Fresh inference only; pauses and stops still apply.");
                        if ui.small_button("Inspect failed attempt").clicked() {
                            self.open_resource(ctx, s(&recovery, "failed_receipt").into());
                        }
                    });
                }
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
                        if let Some(job) = effect["job"].as_str() {
                            if let Some(started) = effect["started"].as_i64() {
                                ui.weak(format!("{}s since launch", ai_personas::neural::now().saturating_sub(started)));
                            }
                            if !effect["cancellation"].is_null() {
                                ui.weak("Cancellation requested");
                            }
                            if ui.small_button("Inspect job").clicked() {
                                self.open_resource(ctx, job.into());
                            }
                        }
                    });
                }
                let rows: Vec<Value> = self
                    .events
                    .iter()
                    .rev()
                    .filter(|e| e["work"] == self.work && match self.activity_filter {
                        1 => e["kind"] == "tool_result",
                        2 => e["tool_outcome"]["problem"] == true || matches!(s(e, "kind"), "provider_failure" | "decision_rejected" | "uncertain_effect" | "uncertain_job"),
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
                                    if matches!(e["body"]["tool"].as_str(),Some("compute.run"|"compute.inspect")) {
                                        if let Some(program)=result["program"].as_str() {
                                            let label=result["program_format"].as_str().map_or("Program ↗".to_owned(),|f|format!("Program .{f} ↗"));
                                            if ui.small_button(label).on_hover_text(program).clicked() {self.open_resource(ctx,program.into());}
                                        }
                                    }
                                    if e["body"]["tool"]=="geometry.render" {
                                        for (key,label) in [("source","Source"),("mesh","Mesh")] {
                                            if let Some(reference)=result[key].as_str() {
                                                if ui.small_button(label).on_hover_text(reference).clicked() {self.open_resource(ctx,reference.into());}
                                            }
                                        }
                                    }
                                    if ui.small_button("Inspect result").on_hover_text("Open the exact retained result, including its input and observations.").clicked() {
                                        self.open_resource(ctx, s(result, "result").into());
                                    }
                                    ui.add(egui::Label::new(RichText::new(s(&e["body"], "operation")).size(11.).weak()).truncate());
                                    return;
                                }
                                let label = if e["kind"] == "control_changed" {
                                    let scope = s(&e["body"], "scope");
                                    let label = match scope {
                                        "global" => "Workspace", "admission" => "Scheduling",
                                        _ if scope.starts_with("work:") => "Task",
                                        _ if scope.starts_with("persona:") => "Persona",
                                        _ => "Background job",
                                    };
                                    format!("{label} · {}", title(s(&e["body"], "state")))
                                } else { title(s(e, "kind")) };
                                ui.label(RichText::new(label).size(12.));
                                let detail = ["text", "reason", "error"].into_iter().map(|key| s(&e["body"], key)).find(|text| !text.is_empty()).unwrap_or("");
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(excerpt(detail, 180))
                                            .size(12.)
                                            .weak(),
                                    )
                                    .truncate(),
                                );
                                let control = e["body"]["control"].as_str().filter(|_| e["kind"] == "control_changed");
                                if ui.small_button(if control.is_some() { "Inspect control" } else { "Inspect event" }).clicked() {
                                    self.open_resource(ctx, control.map(str::to_owned).unwrap_or_else(|| format!("object:{}", s(e, "digest"))));
                                }
                            });
                        }
                    });
            });
        self.layout.bottom_height = panel.response.rect.height();
    }
}
