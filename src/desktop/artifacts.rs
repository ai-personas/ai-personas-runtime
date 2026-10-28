//! Produced-file browser, with decoding isolated from the foreground UI.
use super::{mesh, theme};
use eframe::{egui, glow};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

#[path = "artifact_load.rs"]
mod loader;
pub(super) use loader::read_file;

#[derive(Clone)]
enum Source {
    File(PathBuf),
    Memory {
        name: String,
        bytes: Arc<Vec<u8>>,
    },
    Zip {
        container: Arc<Source>,
        index: usize,
        name: String,
    },
}
struct Entry {
    path: PathBuf,
    size: u64,
    source: Source,
    details: Option<String>,
}
struct Catalog {
    files: Vec<Entry>,
    note: String,
}
struct ArchiveEntry {
    index: usize,
    name: String,
    size: u64,
}
enum Body {
    Image(egui::ColorImage, String),
    Text(String, bool),
    Table(Vec<Vec<String>>),
    Mesh(Arc<mesh::Mesh>),
    Archive(Vec<ArchiveEntry>),
    Unsupported(String),
}
struct Decoded {
    body: Body,
    note: String,
}
enum Request {
    Scan(u64, PathBuf),
    Delivered(u64, Vec<serde_json::Value>),
    Load(u64, Source),
}
enum Response {
    Catalog(u64, Result<Catalog, String>),
    Preview(u64, Result<Decoded, String>),
}

pub struct Viewer {
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Response>,
    serial: u64,
    context: String,
    recorded: bool,
    initialized: bool,
    loading: bool,
    catalog: Catalog,
    folder: PathBuf,
    filter: String,
    kind: usize,
    selected: Option<PathBuf>,
    source: Option<Source>,
    trail: Vec<Source>,
    decoded: Option<Decoded>,
    texture: Option<egui::TextureHandle>,
    image_zoom: f32,
    image_pan: egui::Vec2,
    white_paper: bool,
    source_text: bool,
    archive_filter: String,
    details: Option<String>,
    error: Option<String>,
    model: mesh::Viewport,
}
impl Viewer {
    pub fn new(ctx: &egui::Context, gl: Option<&Arc<glow::Context>>) -> Self {
        let (tx, requests) = mpsc::channel();
        let (responses, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut request) = requests.recv() {
                // Only the latest queued selection needs decoding. Running
                // work can finish, but its generation cannot replace a newer view.
                while let Ok(next) = requests.try_recv() {
                    request = next;
                }
                let response = match request {
                    Request::Scan(id, root) => Response::Catalog(id, loader::scan(&root)),
                    Request::Delivered(id, d) => Response::Catalog(id, loader::delivered(d)),
                    Request::Load(id, source) => Response::Preview(id, loader::decode(&source)),
                };
                if responses.send(response).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            tx,
            rx,
            serial: 0,
            context: String::new(),
            recorded: false,
            initialized: false,
            loading: false,
            catalog: Catalog {
                files: Vec::new(),
                note: String::new(),
            },
            folder: PathBuf::new(),
            filter: String::new(),
            kind: 0,
            selected: None,
            source: None,
            trail: Vec::new(),
            decoded: None,
            texture: None,
            image_zoom: 1.0,
            image_pan: egui::Vec2::ZERO,
            white_paper: true,
            source_text: false,
            archive_filter: String::new(),
            details: None,
            error: None,
            model: mesh::Viewport::new(gl),
        }
    }
    pub fn destroy(&mut self) {
        self.model.destroy();
    }
    fn reset_preview(&mut self) {
        self.decoded = None;
        self.texture = None;
        self.error = None;
        self.image_zoom = 1.0;
        self.image_pan = egui::Vec2::ZERO;
        self.source_text = false;
        self.archive_filter.clear();
        self.model.reset();
    }
    fn load(&mut self, source: Source) {
        self.reset_preview();
        self.serial += 1;
        self.loading = true;
        self.source = Some(source.clone());
        let _ = self.tx.send(Request::Load(self.serial, source));
    }
    fn scan(&mut self, root: &Path, deliveries: &[serde_json::Value]) {
        self.serial += 1;
        self.loading = true;
        self.initialized = true;
        self.catalog.files.clear();
        self.catalog.note.clear();
        self.folder.clear();
        self.selected = None;
        self.source = None;
        self.trail.clear();
        self.details = None;
        self.reset_preview();
        let request = if self.recorded {
            Request::Delivered(self.serial, deliveries.to_vec())
        } else {
            Request::Scan(self.serial, root.into())
        };
        let _ = self.tx.send(request);
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Response::Catalog(id, result) if id == self.serial => {
                    self.loading = false;
                    match result {
                        Ok(c) => self.catalog = c,
                        Err(e) => self.error = Some(e),
                    }
                }
                Response::Preview(id, result) if id == self.serial => {
                    self.loading = false;
                    match result {
                        Ok(decoded) => {
                            if let Body::Image(image, _) = &decoded.body {
                                self.texture = Some(ctx.load_texture(
                                    "artifact-preview",
                                    image.clone(),
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                            self.decoded = Some(decoded);
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                _ => {}
            }
        }
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        root: &Path,
        context: &str,
        deliveries: &[serde_json::Value],
    ) {
        self.poll(ui.ctx());
        if self.context != context {
            self.context = context.into();
            self.initialized = false;
            self.filter.clear();
            self.kind = 0;
        }
        ui.horizontal_wrapped(|ui| {
            let changed = ui
                .selectable_value(&mut self.recorded, false, "Workspace files")
                .changed()
                | ui.selectable_value(&mut self.recorded, true, "Recorded deliveries")
                    .changed();
            if changed {
                self.filter.clear();
                self.kind = 0;
            }
            if changed
                || theme::icon_button(ui, theme::Icon::Refresh, "Refresh file list").clicked()
            {
                self.initialized = false;
            }
            ui.weak(format!("{} files", self.catalog.files.len()));
        });
        if !self.initialized {
            self.scan(root, deliveries);
        }
        ui.add(egui::Label::new(egui::RichText::new(&self.catalog.note).small().weak()).truncate())
            .on_hover_text(if self.recorded {
                self.catalog.note.clone()
            } else {
                format!("{}\n{}", root.display(), self.catalog.note)
            });
        ui.separator();
        let mut open = None;
        egui::SidePanel::left("produced-file-browser").default_width(205.0).width_range(155.0..=350.0).frame(egui::Frame::new().inner_margin(6).fill(theme::palette(ui).panel)).show_inside(ui,|ui|{
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Find files…").desired_width(f32::INFINITY));
            let kinds=["All formats","Images & drawings","3D models","Documents & code","ZIP packages"];
            egui::ComboBox::from_id_salt("artifact-kind").selected_text(kinds[self.kind]).width(ui.available_width()).show_ui(ui,|ui|{for (i,name) in kinds.iter().enumerate(){ui.selectable_value(&mut self.kind,i,*name);}});
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.folder.as_os_str().is_empty(),egui::Button::new("Up")).clicked(){self.folder.pop();}
                if ui.button("Root").clicked(){self.folder.clear();self.filter.clear();self.kind=0;}
                ui.add(egui::Label::new(self.folder.file_name().map(|n|n.to_string_lossy()).unwrap_or("Files".into())).truncate()).on_hover_text(self.folder.display().to_string());
            });
            ui.separator();
            let filtered=!self.filter.trim().is_empty() || self.kind!=0;
            let query=self.filter.to_lowercase();
            let mut folders=BTreeSet::new();
            let mut entries=Vec::new();
            for (i,entry) in self.catalog.files.iter().enumerate() {
                if filtered {
                    if entry.path.to_string_lossy().to_lowercase().contains(&query) && (self.kind==0 || category(&entry.path)==self.kind) {entries.push(i);}
                } else if let Ok(relative)=entry.path.strip_prefix(&self.folder) {
                    let mut parts=relative.components();let first=parts.next();
                    if parts.next().is_some(){if let Some(c)=first{folders.insert(self.folder.join(c));}}
                    else {entries.push(i);}
                }
            }
            egui::ScrollArea::vertical().id_salt("artifact-list").show(ui,|ui|{
                if folders.is_empty() && entries.is_empty() && !self.catalog.files.is_empty() { ui.weak("No files match this search."); }
                for folder in folders {
                    ui.horizontal(|ui|{
                        theme::glyph(ui,theme::Icon::Folder,theme::ACCENT);
                        let name=folder.file_name().unwrap_or_default().to_string_lossy();
                        if ui.add_sized([ui.available_width(),22.0],egui::Button::new(name).right_text("").frame(false).truncate()).clicked(){self.folder=folder;}
                    });
                }
                for index in entries {
                    let e=&self.catalog.files[index];
                    let label=e.path.file_name().unwrap_or_default().to_string_lossy();
                    let selected=self.selected.as_ref()==Some(&e.path);
                    let response=ui.add_sized([ui.available_width(),23.0],egui::Button::new(label).right_text(egui::RichText::new(size_label(e.size)).small().weak()).selected(selected).frame(selected).truncate());
                    if response.on_hover_text(format!("{}\n{}",e.path.display(),size_label(e.size))).clicked(){open=Some(index);}
                }
                if self.catalog.files.is_empty() && !self.loading {ui.weak(if self.recorded{"No recorded deliveries. Check Workspace files for produced work, or export eligible citations."}else{"No visible files in the working folder."});}
            });
        });
        if let Some(index) = open {
            let entry = &self.catalog.files[index];
            self.selected = Some(entry.path.clone());
            self.details = entry.details.clone();
            let source = entry.source.clone();
            self.trail.clear();
            self.load(source);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(8)
                    .fill(theme::palette(ui).canvas),
            )
            .show_inside(ui, |ui| self.preview(ui));
        if self.loading {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    fn preview(&mut self, ui: &mut egui::Ui) {
        let mut back = false;
        ui.horizontal_wrapped(|ui| {
            if !self.trail.is_empty() && ui.button("Back to package").clicked() {
                back = true;
            }
            if let Some(source) = &self.source {
                ui.add(egui::Label::new(egui::RichText::new(source.name()).strong()).truncate())
                    .on_hover_text(source.label());
                ui.menu_button("Actions", |ui| {
                    if ui.button("Copy file location").clicked() {
                        ui.ctx().copy_text(source.label());
                        ui.close();
                    }
                    if let Source::File(path) = source {
                        if ui.button("Open in external application").clicked() {
                            open_external(path);
                            ui.close();
                        }
                        if ui.button("Open containing folder").clicked() {
                            if let Some(parent) = path.parent() {
                                open_external(parent);
                            }
                            ui.close();
                        }
                    }
                });
            }
        });
        if back {
            if let Some(source) = self.trail.pop() {
                self.load(source);
            }
            return;
        }
        if let Some(details) = &self.details {
            ui.collapsing("Source & qualifications", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(170.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(details).monospace())
                                .wrap()
                                .selectable(true),
                        );
                    });
            });
        }
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading preview…");
            });
            return;
        }
        if let Some(e) = &self.error {
            ui.colored_label(theme::ACCENT, e);
            return;
        }
        let Some(decoded) = &self.decoded else {
            ui.add_space(35.0);
            ui.heading("Preview produced files");
            ui.label("Select an image, drawing, document, model, or package.");
            ui.weak("PNG · JPEG · SVG · Markdown · CSV · JSON · STL · OBJ · GLB · ZIP");
            return;
        };
        if !decoded.note.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&decoded.note)
                        .small()
                        .color(theme::ACCENT),
                )
                .wrap(),
            );
        }
        ui.separator();
        let mut archive_entry = None;
        match &decoded.body {
            Body::Image(_, info) => {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Fit").clicked() {
                        self.image_zoom = 1.0;
                        self.image_pan = egui::Vec2::ZERO;
                    }
                    ui.add(
                        egui::Slider::new(&mut self.image_zoom, 0.2..=8.0)
                            .logarithmic(true)
                            .show_value(false)
                            .text("Zoom"),
                    );
                    ui.checkbox(&mut self.white_paper, "White background");
                    ui.weak(info);
                });
                let (rect, response) = ui.allocate_exact_size(
                    ui.available_size().max(egui::vec2(40.0, 40.0)),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter().with_clip_rect(rect);
                painter.rect_filled(
                    rect,
                    0,
                    if self.white_paper {
                        egui::Color32::from_gray(248)
                    } else {
                        egui::Color32::from_gray(46)
                    },
                );
                if response.dragged() {
                    self.image_pan += ui.input(|i| i.pointer.delta());
                }
                if response.hovered() {
                    self.image_zoom = (self.image_zoom
                        * (ui.input(|i| i.smooth_scroll_delta.y) * 0.003).exp())
                    .clamp(0.2, 8.0);
                }
                if response.double_clicked() {
                    self.image_zoom = 1.0;
                    self.image_pan = egui::Vec2::ZERO;
                }
                if let Some(texture) = &self.texture {
                    let size = texture.size_vec2();
                    let factor =
                        (rect.width() / size.x).min(rect.height() / size.y) * self.image_zoom;
                    let target =
                        egui::Rect::from_center_size(rect.center() + self.image_pan, size * factor);
                    painter.image(
                        texture.id(),
                        target,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            }
            Body::Mesh(mesh) => self.model.ui(ui, mesh),
            Body::Text(text, markdown) => {
                if *markdown {
                    ui.checkbox(&mut self.source_text, "Show Markdown source");
                }
                egui::ScrollArea::both()
                    .id_salt(("artifact-text", self.serial))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if *markdown && !self.source_text {
                            ui.set_min_width(ui.available_width());
                            theme::message_body(ui, text);
                        } else {
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).monospace())
                                    .selectable(true)
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                    });
            }
            Body::Table(rows) => {
                ui.weak(format!("{} rows", rows.len().saturating_sub(1)));
                egui::ScrollArea::both()
                    .id_salt(("artifact-table", self.serial))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new(("csv", self.serial))
                            .striped(true)
                            .spacing([18.0, 8.0])
                            .show(ui, |ui| {
                                for (n, row) in rows.iter().enumerate() {
                                    for cell in row {
                                        ui.add(
                                            egui::Label::new(if n == 0 {
                                                egui::RichText::new(cell).strong()
                                            } else {
                                                egui::RichText::new(cell)
                                            })
                                            .selectable(true)
                                            .wrap_mode(egui::TextWrapMode::Extend),
                                        );
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }
            Body::Archive(entries) => {
                ui.label(format!("{} files in this package", entries.len()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.archive_filter)
                        .hint_text("Find a file inside the package…")
                        .desired_width(f32::INFINITY),
                );
                let filter = self.archive_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt(("zip", self.serial))
                    .show(ui, |ui| {
                        for entry in entries
                            .iter()
                            .filter(|e| e.name.to_lowercase().contains(&filter))
                        {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(size_label(entry.size)).small().weak(),
                                );
                                if ui
                                    .add(egui::Button::new(&entry.name).frame(false).truncate())
                                    .on_hover_text(&entry.name)
                                    .clicked()
                                {
                                    archive_entry = Some((entry.index, entry.name.clone()));
                                }
                            });
                        }
                    });
            }
            Body::Unsupported(message) => {
                ui.label(message);
            }
        }
        if let Some((index, name)) = archive_entry {
            if self.trail.len() >= 4 {
                self.error = Some("Nested archive preview is limited to four levels.".into());
            } else if let Some(source) = self.source.clone() {
                self.trail.push(source.clone());
                self.load(Source::Zip {
                    container: Arc::new(source),
                    index,
                    name,
                });
            }
        }
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
fn category(path: &Path) -> usize {
    match extension(path).as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" | "tiff" | "tif" | "ico" => 1,
        "stl" | "obj" | "glb" | "gltf" | "step" | "stp" => 2,
        "zip" => 4,
        _ => 3,
    }
}
fn size_label(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / 1048576.0)
    } else if size >= 1024 {
        format!("{:.1} KiB", size as f64 / 1024.0)
    } else {
        format!("{size} B")
    }
}
fn open_external(path: &Path) {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // Explicit reader action. Previews themselves never open an application.
    let _ = std::process::Command::new(command).arg(path).spawn();
}
