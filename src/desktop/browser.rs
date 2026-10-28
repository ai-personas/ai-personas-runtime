//! Workspace browser: locations, a compact file-style list, and selection details.
use super::*;

impl App {
    pub(super) fn library_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("library-locations")
            .default_width(195.0)
            .width_range(160.0..=270.0)
            .frame(theme::panel(ctx))
            .show(ctx, |ui| {
                theme::header(ui, Icon::Folder, "Locations", "");
                ui.add_space(8.0);
                let saved = self.choices.iter().filter(|w| w.ready).count();
                let unfinished = self.choices.len() - saved;
                for (scope, label, count) in [
                    (LibraryScope::All, "All workspaces", saved),
                    (
                        LibraryScope::Recent,
                        "Last 7 days",
                        self.choices
                            .iter()
                            .filter(|w| {
                                w.ready
                                    && w.updated.elapsed().unwrap_or_default().as_secs() < 604800
                            })
                            .count(),
                    ),
                    (LibraryScope::Unfinished, "Unfinished setup", unfinished),
                ] {
                    let text = format!("{label}   {count}");
                    if ui
                        .add_sized(
                            [ui.available_width(), 27.0],
                            egui::Button::new(text).selected(self.library_scope == scope),
                        )
                        .clicked()
                    {
                        self.library_scope = scope;
                    }
                }
                ui.add_space(12.0);
                ui.separator();
                ui.label(RichText::new("WORKSPACE ACTIONS").small().weak());
                if ui
                    .add_enabled(!self.busy, theme::primary("New workspace"))
                    .clicked()
                {
                    self.new_workspace();
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Open another folder…"))
                    .clicked()
                {
                    self.dialog = Some(Dialog::OpenWorkspace);
                }
                if self.view.progress.is_some() {
                    ui.separator();
                    ui.label(RichText::new("CURRENT WORKSPACE").small().weak());
                    ui.add(
                        egui::Label::new(
                            self.view
                                .root
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                        )
                        .wrap(),
                    );
                    if ui.button("Return to workspace").clicked() {
                        self.choose_progress = false;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("Local workspaces").small().weak());
                    ui.add(
                        egui::Label::new("Saved tasks and session history stay on this computer.")
                            .wrap()
                            .selectable(false),
                    );
                });
            });
    }
    pub(super) fn workspace_details(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("workspace-details")
            .default_width(255.0)
            .width_range(205.0..=350.0)
            .frame(theme::panel(ctx))
            .show(ctx, |ui| {
                theme::header(ui, Icon::Workspaces, "Workspace properties", "");
                let Some(w) = self
                    .choices
                    .iter()
                    .find(|w| Some(&w.root) == self.browse_selected.as_ref())
                    .cloned()
                else {
                    ui.label("Select a workspace to inspect its saved work.");
                    return;
                };
                egui::ScrollArea::vertical()
                    .id_salt("workspace-properties")
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        theme::glyph(ui, Icon::Folder, theme::ACCENT);
                        ui.add(
                            egui::Label::new(RichText::new(&w.title).size(14.0).strong()).wrap(),
                        );
                        theme::badge(
                            ui,
                            if w.ready {
                                "SAVED WORKSPACE"
                            } else {
                                "SETUP INCOMPLETE"
                            },
                            if w.ready { theme::GREEN } else { theme::ACCENT },
                        );
                        ui.add_space(8.0);
                        if ui
                            .add_enabled(
                                !self.busy,
                                theme::primary(if w.ready {
                                    "Open workspace"
                                } else {
                                    "Review setup"
                                })
                                .min_size(egui::vec2(ui.available_width(), 28.0)),
                            )
                            .clicked()
                        {
                            self.progress = w.root.to_string_lossy().into_owned();
                            self.open();
                        }
                        ui.separator();
                        theme::property(ui, "Tasks", &w.tasks.to_string());
                        theme::property(ui, "Sessions", &w.sessions.to_string());
                        theme::property(ui, "Updated", &updated(&w));
                        ui.separator();
                        ui.label(RichText::new("LATEST TASK").small().weak());
                        ui.add(egui::Label::new(&w.summary).wrap().selectable(true));
                        if let Some(issue) = &w.issue {
                            ui.colored_label(theme::ACCENT, issue);
                        }
                        ui.add_space(8.0);
                        ui.collapsing("Storage location", |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(w.root.display().to_string()).small(),
                                )
                                .wrap()
                                .selectable(true),
                            );
                            if ui.small_button("Copy path").clicked() {
                                ui.ctx().copy_text(w.root.display().to_string());
                            }
                        });
                    });
            });
    }
    pub(super) fn new_workspace(&mut self) {
        match journal::id("personas-workspace") {
            Ok(name) => {
                let base = journal::candidates()
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| PathBuf::from(&self.progress));
                self.progress = base.with_file_name(name).to_string_lossy().into_owned();
                self.open();
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub(super) fn workspaces(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui);
        theme::header(ui, Icon::Workspaces, "Workspace browser", "LOCAL");
        self.banner(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 34.0).max(80.0);
            ui.add_sized(
                [width, 26.0],
                egui::TextEdit::singleline(&mut self.workspace_filter)
                    .hint_text("Search workspaces or tasks…"),
            );
            if ui
                .add_enabled_ui(!self.busy, |ui| {
                    theme::icon_button(ui, Icon::Refresh, "Refresh workspace list")
                })
                .inner
                .clicked()
            {
                self.discover();
            }
        });
        let query = self.workspace_filter.trim().to_lowercase();
        let choices: Vec<_> = self
            .choices
            .iter()
            .filter(|w| {
                let scope = match self.library_scope {
                    LibraryScope::All => w.ready,
                    LibraryScope::Recent => {
                        w.ready && w.updated.elapsed().unwrap_or_default().as_secs() < 604800
                    }
                    LibraryScope::Unfinished => !w.ready,
                };
                scope
                    && (query.is_empty()
                        || w.title.to_lowercase().contains(&query)
                        || w.summary.to_lowercase().contains(&query)
                        || w.root.to_string_lossy().to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        if !choices
            .iter()
            .any(|w| Some(&w.root) == self.browse_selected.as_ref())
        {
            let selected = choices.first().map(|w| w.root.clone());
            if selected != self.browse_selected {
                self.browse_selected = selected;
                ui.ctx().request_repaint();
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match self.library_scope {
                    LibraryScope::All => "All workspaces",
                    LibraryScope::Recent => "Last 7 days",
                    LibraryScope::Unfinished => "Unfinished setup",
                })
                .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} workspace{} · newest first",
                        choices.len(),
                        if choices.len() == 1 { "" } else { "s" }
                    ))
                    .small()
                    .weak(),
                );
            });
        });
        let width = ui.available_width();
        let wide = width >= 460.0;
        let counts = if wide { 115.0 } else { 0.0 };
        let name_width = (width - counts - 91.0).max(110.0);
        let (head, _) = ui.allocate_exact_size(egui::vec2(width, 27.0), egui::Sense::hover());
        ui.painter().rect_filled(head, 0, p.header);
        let font = egui::FontId::proportional(11.0);
        ui.painter().text(
            egui::pos2(head.left() + 7.0, head.center().y),
            egui::Align2::LEFT_CENTER,
            "Name",
            font.clone(),
            p.muted,
        );
        if wide {
            ui.painter().text(
                egui::pos2(head.left() + name_width + 22.0, head.center().y),
                egui::Align2::CENTER_CENTER,
                "Tasks",
                font.clone(),
                p.muted,
            );
            ui.painter().text(
                egui::pos2(head.left() + name_width + 79.0, head.center().y),
                egui::Align2::CENTER_CENTER,
                "Sessions",
                font.clone(),
                p.muted,
            );
        }
        ui.painter().text(
            egui::pos2(head.right() - 7.0, head.center().y),
            egui::Align2::RIGHT_CENTER,
            "Updated",
            font,
            p.muted,
        );
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Finding workspaces…");
            });
        }
        if choices.is_empty() && !self.busy {
            ui.add_space(30.0);
            ui.label("No matching workspaces");
            ui.label(RichText::new("Try another search, or open a saved progress folder.").weak());
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("workspace-file-list")
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, w) in choices.iter().enumerate() {
                    let selected = self.browse_selected.as_ref() == Some(&w.root);
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 31.0),
                        egui::Sense::click(),
                    );
                    let fill = if selected {
                        p.selection
                    } else if response.hovered() {
                        p.hover
                    } else if index % 2 == 0 {
                        p.canvas
                    } else {
                        p.panel
                    };
                    ui.painter().rect_filled(rect, 0, fill);
                    if selected {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
                            0,
                            theme::BLUE,
                        );
                    }
                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 15.0, rect.center().y),
                        egui::vec2(14.0, 14.0),
                    );
                    theme::paint_icon(
                        ui.painter(),
                        icon_rect,
                        Icon::Folder,
                        if w.ready { theme::ACCENT } else { p.muted },
                    );
                    let text_rect = egui::Rect::from_min_max(
                        rect.min + egui::vec2(30.0, 0.0),
                        egui::pos2(rect.left() + name_width - 5.0, rect.bottom()),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(text_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            ui.add(
                                egui::Label::new(&w.title)
                                    .truncate()
                                    .selectable(false)
                                    .sense(egui::Sense::hover()),
                            );
                        },
                    );
                    if wide {
                        ui.painter().text(
                            egui::pos2(rect.left() + name_width + 22.0, rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            w.tasks.to_string(),
                            egui::FontId::monospace(11.5),
                            p.muted,
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + name_width + 79.0, rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            w.sessions.to_string(),
                            egui::FontId::monospace(11.5),
                            p.muted,
                        );
                    }
                    ui.painter().text(
                        egui::pos2(rect.right() - 7.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        updated(w),
                        egui::FontId::proportional(11.0),
                        p.muted,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            ui.is_enabled(),
                            selected,
                            &w.title,
                        )
                    });
                    let response = response.on_hover_text(format!(
                        "{}\n{} tasks · {} sessions\n{}",
                        w.title,
                        w.tasks,
                        w.sessions,
                        w.root.display()
                    ));
                    if response.clicked() {
                        self.browse_selected = Some(w.root.clone());
                    }
                    if !self.busy
                        && (response.double_clicked()
                            || (response.has_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    {
                        self.progress = w.root.to_string_lossy().into_owned();
                        self.open();
                    }
                }
            });
    }
}
fn updated(w: &journal::SavedWorkspace) -> String {
    let age = w.updated.elapsed().unwrap_or_default().as_secs();
    if age < 60 {
        "Just now".into()
    } else if age < 3600 {
        format!("{} min ago", age / 60)
    } else if age < 86400 {
        format!("{} hr ago", age / 3600)
    } else {
        format!("{} days ago", age / 86400)
    }
}
