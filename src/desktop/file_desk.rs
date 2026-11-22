//! A local inspection window. The workspace keeps receiving updates while it is open.
use super::{artifacts::Viewer, design, theme};
use eframe::egui::{self, Ui};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};
use theme::Icon;

fn request_id() -> egui::Id {
    egui::Id::new("studio/open-file-viewer")
}

pub fn request(ctx: &egui::Context, choose_files: bool) {
    ctx.data_mut(|d| d.insert_temp(request_id(), choose_files));
    ctx.request_repaint();
}

fn open_shortcut(ctx: &egui::Context) -> bool {
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
}

fn dropped(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    })
}

pub struct Host {
    app: Box<dyn eframe::App>,
    desk: Option<Desk>,
    gl: Option<Arc<eframe::glow::Context>>,
    visible: bool,
    initial_files: Vec<PathBuf>,
}

impl Host {
    pub fn new(
        app: Box<dyn eframe::App>,
        cc: &eframe::CreationContext<'_>,
        files: Vec<PathBuf>,
    ) -> Self {
        Self {
            app,
            gl: cc.gl.clone(),
            desk: None,
            visible: !files.is_empty(),
            initial_files: files,
        }
    }
}

impl eframe::App for Host {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
        let action = ctx.data_mut(|d| d.remove_temp::<bool>(request_id()));
        let mut files = std::mem::take(&mut self.initial_files);
        files.extend(dropped(ctx));
        let choose = open_shortcut(ctx) || action == Some(true);
        let reveal = action.is_some() || choose || !files.is_empty();
        self.visible |= reveal;
        if !self.visible {
            return;
        }
        let id = egui::ViewportId::from_hash_of("studio-file-viewer");
        let desk = self
            .desk
            .get_or_insert_with(|| Desk::new(ctx, self.gl.as_ref()));
        desk.open(files);
        if choose {
            desk.choose(ctx);
        }
        let window = egui::ViewportBuilder::default()
            .with_title("File viewer · Persona Studio")
            .with_inner_size([1100., 740.])
            .with_min_inner_size([460., 330.]);
        let close = ctx.show_viewport_immediate(id, window, |ctx, _| {
            if reveal {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            desk.open(dropped(ctx));
            if open_shortcut(ctx) {
                desk.choose(ctx);
            }
            desk.ui(ctx);
            ctx.input(|i| i.viewport().close_requested())
        });
        if close {
            self.visible = false;
            if let Some(mut desk) = self.desk.take() {
                desk.viewer.destroy();
            }
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(desk) = &mut self.desk {
            desk.viewer.destroy();
        }
        self.app.on_exit(gl);
    }
}

struct Desk {
    viewer: Viewer,
    files: Vec<PathBuf>,
    selected: Option<PathBuf>,
    picker: Option<mpsc::Receiver<Vec<PathBuf>>>,
    path_field: Option<String>,
    message: Option<String>,
}

impl Desk {
    fn new(ctx: &egui::Context, gl: Option<&Arc<eframe::glow::Context>>) -> Self {
        Self {
            viewer: Viewer::new(ctx, gl),
            files: vec![],
            selected: None,
            picker: None,
            path_field: None,
            message: None,
        }
    }

    fn choose(&mut self, ctx: &egui::Context) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.picker = Some(rx);
        std::thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title("Open files in Persona Studio")
                .pick_files()
                .unwrap_or_default();
            let _ = tx.send(files);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
    }

    fn open(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.message = None;
        let mut first = None;
        for path in paths {
            match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => {
                    if !self.files.contains(&path) {
                        self.files.push(path.clone());
                    }
                    first.get_or_insert(path);
                }
                Ok(_) => {
                    self.message = Some(format!(
                        "Choose a regular file. {} is a directory or device.",
                        path.display()
                    ))
                }
                Err(e) => self.message = Some(format!("Could not open {}. {e}", path.display())),
            }
        }
        if let Some(path) = first {
            self.select(path);
        }
    }

    fn select(&mut self, path: PathBuf) {
        self.viewer.open_file(path.clone());
        self.selected = Some(path);
    }

    fn ui(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(paths) => {
                    self.picker = None;
                    self.open(paths);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.picker = None;
                    self.message =
                        Some("File picker closed. Use Open by path to enter a filename.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let p = theme::Palette::new(ctx.style().visuals.dark_mode);
        egui::TopBottomPanel::top("file-window-header")
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    design::breadcrumb(ui, "Studio", "File viewer");
                    if ui.available_width() >= 260. {
                        design::action_at_right(ui, |ui| self.open_controls(ui, ctx));
                    } else {
                        self.open_controls(ui, ctx);
                    }
                });
                if let Some(value) = &mut self.path_field {
                    let mut open = false;
                    ui.horizontal_wrapped(|ui| {
                        ui.label("File path");
                        let response = ui.add(
                            egui::TextEdit::singleline(value)
                                .hint_text("Enter or paste a file path")
                                .desired_width((ui.available_width() - 155.).clamp(140., 550.)),
                        );
                        if !response.has_focus() && value.is_empty() {
                            response.request_focus();
                        }
                        open = (response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            || ui.button("Open").clicked();
                    });
                    if open && !value.trim().is_empty() {
                        let path = super::journal::expand(Path::new(value.trim()));
                        self.open(vec![path]);
                        if self.message.is_none() {
                            self.path_field = None;
                        }
                    }
                }
                if let Some(note) = &self.message {
                    ui.colored_label(p.ink(theme::ACCENT), note);
                }
            });
        if ctx.content_rect().width() >= 760. {
            egui::SidePanel::left("file-shelf")
                .resizable(true)
                .default_width(214.)
                .width_range(160.0..=340.)
                .frame(egui::Frame::new().fill(p.panel).inner_margin(14))
                .show(ctx, |ui| {
                    ui.add_space(4.);
                    ui.label(design::caption(
                        &format!("Open files  /  {}", self.files.len()),
                        p.muted,
                    ));
                    ui.add_space(10.);
                    if self.files.is_empty() {
                        ui.weak("Files you open appear here.");
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| self.file_list(ui));
                });
        }
        egui::CentralPanel::default()
            .frame(design::page_frame(ctx, 18))
            .show(ctx, |ui| {
                if ctx.content_rect().width() < 760. && !self.files.is_empty() {
                    ui.menu_button(format!("Open files  ·  {}  ↓", self.files.len()), |ui| {
                        self.file_list(ui)
                    });
                    ui.add_space(8.);
                }
                if self.selected.is_some() {
                    self.viewer.preview_ui(ui);
                } else {
                    let pad = (ui.available_height() * 0.18).min(120.);
                    ui.add_space(pad);
                    egui::Frame::new()
                        .fill(p.panel)
                        .corner_radius(12)
                        .stroke(egui::Stroke::new(1., p.border))
                        .inner_margin(25)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(44.),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(r, 10, p.selection);
                                theme::paint_icon(
                                    ui.painter(),
                                    r.shrink(11.),
                                    Icon::Files,
                                    p.accent,
                                );
                                ui.add_space(12.);
                                ui.label(design::display("Open a file to inspect", 37.));
                                ui.label("Drop files here or choose them from your computer.");
                                ui.add_space(12.);
                                if ui
                                    .add_enabled(
                                        self.picker.is_none(),
                                        theme::primary("Choose files…"),
                                    )
                                    .clicked()
                                {
                                    self.choose(ctx);
                                }
                            });
                        });
                }
            });
    }

    fn open_controls(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui.button("Open by path…").clicked() {
                self.path_field = if self.path_field.is_some() {
                    None
                } else {
                    Some(String::new())
                };
            }
            if ui
                .add_enabled(self.picker.is_none(), theme::primary("Open files…"))
                .clicked()
            {
                self.choose(ctx);
            }
        });
    }

    fn file_list(&mut self, ui: &mut Ui) {
        let mut choice = None;
        for path in &self.files {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let selected = self.selected.as_ref() == Some(path);
            let p = theme::palette(ui);
            let (r, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 37.), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    name.as_ref(),
                )
            });
            if response.hovered() || response.has_focus() || selected {
                ui.painter()
                    .rect_filled(r, 6, if selected { p.selection } else { p.hover });
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    r,
                    6,
                    egui::Stroke::new(1., p.accent),
                    egui::StrokeKind::Inside,
                );
            }
            let stamp = egui::Rect::from_center_size(
                egui::pos2(r.left() + 19., r.center().y),
                egui::Vec2::splat(26.),
            );
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            design::file_stamp(ui, stamp, &ext);
            let galley = egui::WidgetText::from(egui::RichText::new(name.as_ref()).size(13.))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    (r.width() - 48.).max(1.),
                    egui::TextStyle::Body,
                );
            ui.painter().galley(
                egui::pos2(r.left() + 39., r.center().y - galley.size().y / 2.),
                galley,
                p.text,
            );
            if response
                .on_hover_text(path.display().to_string())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                choice = Some(path.clone());
                ui.close_kind(egui::UiKind::Menu);
            }
        }
        if let Some(path) = choice {
            self.select(path);
        }
    }
}
