use super::*;
use eframe::egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use theme::Icon;

impl App {
    pub(super) fn library(&mut self, ctx: &egui::Context) {
        let wide = ctx.content_rect().width() >= 920.;
        let page = if self.create_mode {
            "New workspace"
        } else {
            "Workspace library"
        };
        if wide {
            design::rail(
                ctx,
                "library-rail",
                "Local library",
                "On this computer",
                |ui| {
                    design::rail_group(ui, "Studio");
                    if design::rail_link(ui, Icon::Workspaces, "Workspaces", !self.create_mode)
                        .clicked()
                    {
                        self.create_mode = false;
                    }
                    if design::rail_link(ui, Icon::Files, "File viewer", false).clicked() {
                        file_desk::request(ctx, false);
                    }
                    design::rail_group(ui, "Workspace actions");
                    ui.add_enabled_ui(!self.busy, |ui| {
                        if design::rail_link(ui, Icon::Folder, "Open folder…", false).clicked() {
                            self.choose_workspace(ctx);
                        }
                        if design::rail_link(ui, Icon::Search, "Open by path…", false).clicked() {
                            self.open_path = true;
                        }
                        if design::rail_link(ui, Icon::Refresh, "Restore archive…", false).clicked()
                        {
                            self.error = None;
                            self.restore_desk = Some(restore::Form::default());
                        }
                    });
                },
            );
        }
        egui::TopBottomPanel::top("library-header")
            .frame(
                egui::Frame::new()
                    .fill(theme::Palette::new(self.library_dark).panel)
                    .inner_margin(egui::Margin::symmetric(26, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    design::breadcrumb(ui, if wide { "Studio" } else { "Persona Studio" }, page);
                    if wide {
                        design::action_at_right(ui, |ui| self.appearance(ui, ctx));
                    } else {
                        design::action_at_right(ui, |ui| {
                            if ui.button("File viewer").clicked() {
                                file_desk::request(ctx, false);
                            }
                            self.appearance(ui, ctx);
                            ui.menu_button("Workspace actions", |ui| {
                                if ui.button("Open by path…").clicked() {
                                    self.open_path = true;
                                    ui.close();
                                }
                                if ui.button("Restore archive…").clicked() {
                                    self.error = None;
                                    self.restore_desk = Some(restore::Form::default());
                                    ui.close();
                                }
                            });
                        });
                    }
                });
            });
        let pad = if wide { 30 } else { 16 };
        egui::CentralPanel::default()
            .frame(design::page_frame(ctx, pad))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("library-content", self.create_mode))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width().min(1340.);
                        let indent = ((ui.available_width() - width) * 0.5).max(0.);
                        ui.horizontal_top(|ui| {
                            if indent > 1. {
                                ui.add_space(indent - ui.spacing().item_spacing.x);
                            }
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                if self.create_mode {
                                    if ui
                                        .add_enabled(
                                            !self.busy,
                                            egui::Button::new("←  Workspaces").frame(false),
                                        )
                                        .clicked()
                                    {
                                        self.create_mode = false;
                                    }
                                    ui.add_space(6.);
                                    ui.label(design::display("Create a workspace", 44.));
                                    ui.add_space(12.);
                                    self.connection_status(ui);
                                    ui.add_enabled_ui(!self.busy, |ui| self.setup(ui, ctx));
                                } else {
                                    self.welcome(ui, ctx);
                                    if self.restore_desk.is_none() { self.connection_status(ui); }
                                }
                            });
                        });
                    });
            });
        self.show_restore_desk(ctx);
        if self.open_path {
            let mut open = true;
            egui::Window::new("Open workspace by path")
                .open(&mut open)
                .collapsible(false)
                .default_width(470.)
                .show(ctx, |ui| {
                    field(ui, "Workspace folder", &mut self.existing_root);
                    if ui
                        .add_enabled(
                            !self.busy && !self.existing_root.trim().is_empty(),
                            theme::primary("Open workspace"),
                        )
                        .clicked()
                    {
                        self.open(
                            ctx,
                            journal::expand(&PathBuf::from(self.existing_root.trim())),
                        );
                        self.open_path = false;
                    }
                });
            self.open_path &= open;
        }
    }

    fn appearance(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.menu_button("Appearance", |ui| {
            for (dark, label) in [(true, "Graphite"), (false, "Paper")] {
                if ui
                    .selectable_label(self.library_dark == dark, label)
                    .clicked()
                {
                    self.library_dark = dark;
                    theme::apply(ctx, dark, 1.);
                    ui.close();
                }
            }
        });
    }

    fn welcome(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        design::cover(
            ui,
            "Workspace library",
            "Your workspaces",
            "Pick up a conversation, review a delivery or start a new workspace.",
            |ui| {
                if ui
                    .add_enabled(!self.busy, theme::primary("+  New workspace"))
                    .clicked()
                {
                    self.create_mode = true;
                    self.error = None;
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Open folder…"))
                    .clicked()
                {
                    self.choose_workspace(ctx);
                }
            },
        );
        ui.add_space(22.);
        ui.horizontal_wrapped(|ui| {
            design::section_heading(ui, "Saved workspaces", &self.saved.len().to_string());
            if ui.available_width() > 220. {
                design::action_at_right(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.library_filter)
                            .desired_width(185.)
                            .hint_text("Find a workspace…"),
                    );
                });
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut self.library_filter)
                        .desired_width(125.)
                        .hint_text("Find a workspace…"),
                );
            }
        });
        ui.add_space(9.);
        let query = self.library_filter.trim().to_lowercase();
        let matches: Vec<_> = self
            .saved
            .iter()
            .filter(|entry| {
                entry.path.to_string_lossy().to_lowercase().contains(&query)
                    || entry
                        .people
                        .iter()
                        .any(|(_, name)| name.to_lowercase().contains(&query))
            })
            .collect();
        if matches.is_empty() {
            ui.weak(if self.library_read.is_some() {
                "Reading saved workspaces…"
            } else if self.saved.is_empty() {
                "Workspaces you create or open will appear here."
            } else {
                "No workspaces match your search."
            });
        }
        let columns = if ui.available_width() >= 940. && matches.len() != 4 {
            3
        } else if ui.available_width() >= 560. {
            2
        } else {
            1
        };
        let mut selected = None;
        for group in matches.chunks(columns) {
            ui.columns(columns, |cells| {
                for (cell, root) in cells.iter_mut().zip(group) {
                    cell.add_enabled_ui(!self.busy, |ui| {
                        if workspace_tile(ui, root).clicked() {
                            selected = Some(root.path.clone());
                        }
                    });
                }
            });
            ui.add_space(6.);
        }
        ui.add_space(16.);
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.header)
            .corner_radius(9)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    theme::glyph(ui, Icon::Files, p.accent);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Inspect a file").strong().size(18.));
                        ui.small("Drop files anywhere in Studio to open them in the file viewer.");
                    });
                    let button = |ui: &mut Ui| {
                        if ui.button("Choose files…").clicked() {
                            file_desk::request(ctx, true);
                        }
                    };
                    if ui.available_width() > 130. {
                        design::action_at_right(ui, button);
                    } else {
                        button(ui);
                    }
                });
            });
        if let Some(root) = selected {
            self.open(ctx, root);
        }
    }

    fn choose_workspace(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open a workspace")
            .pick_folder()
        {
            self.open(ctx, path);
        }
    }

    pub(super) fn restore(&mut self, ctx: &egui::Context, archive: PathBuf, root: PathBuf, working_directory: Option<PathBuf>) {
        self.error = None;
        self.busy = true;
        self.cancellable = false;
        self.error = None;
        let (tx, ctx, operator) = (self.tx.clone(), ctx.clone(), self.operator.clone());
        std::thread::spawn(move || {
            let result = (|| {
                tokio::runtime::Runtime::new()
                    .map_err(|e| e.to_string())?
                    .block_on(cli::execute(
                        &root,
                        &operator,
                        cli::Command::Restore {
                            archive,
                            sha256: None,
                            working_directory,
                        },
                    ))
                    .map_err(|e| e.to_string())?;
                root.canonicalize().map_err(|e| e.to_string())
            })();
            let _ = tx.send(Update::Opened(result));
            ctx.request_repaint();
        });
    }
}

fn workspace_tile(ui: &mut Ui, entry: &SavedWorkspace) -> egui::Response {
    let root = &entry.path;
    let p = theme::palette(ui);
    let name = root.file_name().unwrap_or_default().to_string_lossy();
    let (r, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 122.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Open {name}"),
        )
    });
    let stroke = if response.hovered() || response.has_focus() {
        p.accent
    } else {
        p.border
    };
    ui.painter().rect(
        r,
        9,
        p.panel,
        Stroke::new(1., stroke),
        egui::StrokeKind::Inside,
    );
    let colors = [
        (0xE8BC9E, 0x784830),
        (0xC9DACA, 0x3D6552),
        (0xC9D9DF, 0x446372),
        (0xDDD0E1, 0x705375),
    ];
    let hash = name
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let (surface, ink) = colors[hash % colors.len()];
    let rgb = |n: u32| Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8);
    let stamp = Rect::from_min_size(r.min + egui::vec2(15., 14.), Vec2::splat(32.));
    let base = if ui.visuals().dark_mode {
        p.panel.lerp_to_gamma(rgb(surface), 0.20)
    } else {
        rgb(surface)
    };
    let line = if ui.visuals().dark_mode {
        rgb(surface)
    } else {
        rgb(ink)
    };
    ui.painter().rect_filled(stamp, 8, base);
    theme::paint_icon(ui.painter(), stamp.shrink(7.), Icon::Folder, line);
    let kind = if entry.editable {
        "Persona workspace"
    } else {
        "Session journal"
    };
    let recent = entry
        .opened
        .and_then(|date| date.elapsed().ok())
        .map(|age| match age.as_secs() / 86400 {
            0 => "Opened today".to_owned(),
            1 => "Opened yesterday".to_owned(),
            n => format!("Opened {n} days ago"),
        })
        .unwrap_or_else(|| "On this computer".into());
    ui.painter().text(
        egui::pos2(stamp.right() + 9., stamp.center().y),
        Align2::LEFT_CENTER,
        kind,
        FontId::proportional(12.),
        p.muted,
    );
    ui.painter().text(
        egui::pos2(r.right() - 16., r.top() + 30.),
        Align2::RIGHT_CENTER,
        "↗",
        FontId::proportional(17.),
        p.muted,
    );
    let galley = |text: RichText, width| {
        egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            width,
            egui::TextStyle::Body,
        )
    };
    ui.painter().galley(
        Pos2::new(r.left() + 15., r.top() + 55.),
        galley(
            RichText::new(name.as_ref()).size(18.).strong(),
            r.width() - 30.,
        ),
        p.text,
    );
    let mut left = r.left() + 15.;
    let bottom = r.bottom() - 23.;
    for (id, label) in entry.people.iter().take(3) {
        let mark = Rect::from_center_size(egui::pos2(left + 10., bottom), Vec2::splat(24.));
        design::persona_mark(ui, mark, label, neural::persona_color(id));
        left += 20.;
    }
    if !entry.people.is_empty() {
        left += 10.;
    }
    let names = entry
        .people
        .iter()
        .take(3)
        .map(|(_, name)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let more = entry.people.len().saturating_sub(3);
    let byline = if more > 0 {
        format!("{names} +{more}")
    } else {
        names
    };
    let location = if byline.is_empty() {
        recent
    } else {
        format!("{byline}   ·   {recent}")
    };
    let line = galley(
        RichText::new(location).size(12.).color(p.muted),
        (r.right() - left - 12.).max(1.),
    );
    ui.painter()
        .galley(egui::pos2(left, bottom - line.size().y / 2.), line, p.muted);
    response
        .on_hover_text(root.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
