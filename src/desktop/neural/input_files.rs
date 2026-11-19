use super::{number, size, theme, Icon};
use ai_personas::neural::input_files::CapturedFile;
use eframe::egui::{self, RichText, Ui};
use std::{path::PathBuf, sync::mpsc};

/// Each draft owns its picker. A slow read can finish after the user switches
/// activities, recipients or dialogs without attaching files to another draft.
#[derive(Default)]
pub(super) struct Picker {
    pending: Option<mpsc::Receiver<Result<Vec<CapturedFile>, String>>>,
    entering_path: bool,
    focus_path: bool,
    path: String,
    issue: Option<String>,
}

impl Picker {
    fn visible_rows(ui: &Ui) -> usize {
        let window = ui.ctx().content_rect();
        if window.width() < 830. && window.height() < 730. {
            2
        } else {
            3
        }
    }

    pub(super) fn busy(&self) -> bool {
        self.pending.is_some() || self.entering_path
    }

    pub(super) fn show(&mut self, ui: &mut Ui, files: &mut Vec<CapturedFile>) {
        self.poll(files);
        ui.horizontal(|ui| self.button(ui, files.len()));
        self.details(ui, files);
    }

    pub(super) fn button(&mut self, ui: &mut Ui, file_count: usize) {
        if self.pending.is_some() {
            ui.add(egui::Spinner::new().size(13.));
            ui.small("Adding files…");
            return;
        }
        ui.add_enabled_ui(!self.entering_path, |ui| {
            let label = if file_count > Self::visible_rows(ui) {
                format!("Attach files… ({file_count})")
            } else {
                "Attach files…".to_owned()
            };
            ui.menu_button(label, |ui| {
                if ui.button("Choose files…").clicked() {
                    ui.close();
                    self.read(ui.ctx(), None);
                }
                if ui.button("Enter file path…").clicked() {
                    ui.close();
                    self.entering_path = true;
                    self.focus_path = true;
                }
            });
        });
    }

    pub(super) fn poll(&mut self, files: &mut Vec<CapturedFile>) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("File selection closed before the files could be added.".into())
            }
        };
        self.pending = None;
        match result {
            Ok(batch) => {
                if !batch.is_empty() {
                    self.path.clear();
                }
                for file in batch {
                    if !files.iter().any(|existing| {
                        existing.name() == file.name() && existing.sha256() == file.sha256()
                    }) {
                        files.push(file);
                    }
                }
            }
            Err(reason) => self.issue = Some(reason),
        }
    }

    fn read(&mut self, ctx: &egui::Context, paths: Option<Vec<PathBuf>>) {
        let (sender, receiver) = mpsc::channel();
        self.issue = None;
        let ctx = ctx.clone();
        let worker = std::thread::Builder::new()
            .name("studio-input-files".into())
            .spawn(move || {
                let paths = paths.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("Attach files")
                        .pick_files()
                });
                let result = paths
                    .unwrap_or_default()
                    .iter()
                    .map(|path| {
                        CapturedFile::read(path).map_err(|e| format!("No files added. {e}"))
                    })
                    .collect();
                let _ = sender.send(result);
                ctx.request_repaint();
            });
        match worker {
            Ok(_) => self.pending = Some(receiver),
            Err(reason) => self.issue = Some(format!("Could not start file selection. {reason}")),
        }
    }

    pub(super) fn details(&mut self, ui: &mut Ui, files: &mut Vec<CapturedFile>) {
        if self.entering_path {
            ui.horizontal_wrapped(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.path)
                        .hint_text("Local file path")
                        .desired_width((ui.available_width() - 100.).clamp(60., 430.)),
                );
                if std::mem::take(&mut self.focus_path) {
                    field.request_focus();
                }
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = ui
                    .add_enabled(!self.path.is_empty(), egui::Button::new("Add").small())
                    .clicked();
                if !self.path.is_empty() && (enter || add) {
                    self.entering_path = false;
                    self.read(ui.ctx(), Some(vec![PathBuf::from(&self.path)]));
                }
                if ui.small_button("Cancel").clicked() {
                    self.path.clear();
                    self.entering_path = false;
                }
            });
        }
        if let Some(issue) = &self.issue {
            let dismiss = ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 20.),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let clicked = ui.small_button("Dismiss").clicked();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width().max(1.), 20.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(issue)
                                        .small()
                                        .color(ui.visuals().error_fg_color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(issue);
                        },
                    );
                    clicked
                },
            );
            if dismiss.inner {
                self.issue = None;
            }
        }
        if files.is_empty() {
            return;
        }
        let p = theme::palette(ui);
        let mut remove = None;
        let rows = Self::visible_rows(ui) as f32;
        egui::ScrollArea::vertical()
            .id_salt("attached-files")
            .max_height(rows * 20. + (rows - 1.) * ui.spacing().item_spacing.y)
            .min_scrolled_height(0.)
            .auto_shrink([false, true])
            .show_rows(ui, 20., files.len(), |ui, rows| {
                for index in rows {
                    let file = &files[index];
                    let name = file.name();
                    let bytes = file.len() as u64;
                    let duplicate_name = files
                        .iter()
                        .any(|other| other.name() == name && other.sha256() != file.sha256());
                    ui.push_id((name, file.sha256()), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 20.),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let button = ui.small_button("Remove");
                                button.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        ui.is_enabled(),
                                        if duplicate_name {
                                            format!("Remove {name} ({})", &file.sha256()[..10])
                                        } else {
                                            format!("Remove {name}")
                                        },
                                    )
                                });
                                if button.on_hover_text(format!("Remove {name}")).clicked() {
                                    remove = Some(index);
                                }
                                ui.label(RichText::new(size(bytes)).small().color(p.muted))
                                    .on_hover_text(format!("{} bytes", number(bytes)));
                                if duplicate_name {
                                    ui.label(
                                        RichText::new(&file.sha256()[..10]).small().color(p.muted),
                                    )
                                    .on_hover_text(file.sha256());
                                }
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width().max(1.), 20.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        if ui.available_width() > 140. {
                                            theme::glyph(ui, Icon::Files, p.muted);
                                        }
                                        ui.add(egui::Label::new(name).truncate()).on_hover_text(
                                            format!("{name}\nSHA-256 {}", file.sha256()),
                                        );
                                    },
                                );
                            },
                        );
                    });
                }
            });
        if let Some(index) = remove {
            files.remove(index);
        }
    }
}
