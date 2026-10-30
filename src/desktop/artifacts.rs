//! Produced-file browser, with decoding isolated from the foreground UI.
use super::{data_view, document, mesh, theme};
use eframe::{egui, glow};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

#[path = "artifact_load.rs"]
mod loader;
#[path = "native_preview.rs"]
mod native_preview;
pub(super) use loader::read_file;

#[derive(Clone)]
enum Source {
    File(PathBuf),
    Snapshot {
        path: PathBuf,
        sha256: String,
        _directory: Arc<tempfile::TempDir>,
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
    citation_message: Option<String>,
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
    Parameters(serde_json::Value, String, Vec<data_view::Dataset>),
    Table(Vec<Vec<String>>, String),
    Mesh(Arc<mesh::Mesh>),
    Archive(Vec<ArchiveEntry>),
    Native(Arc<native_preview::Document>),
    Unsupported(String),
}
struct Decoded {
    body: Body,
    note: String,
}
enum Request {
    Scan(u64, PathBuf, bool),
    Recorded(u64, super::helpers::ResultFiles),
    Load(u64, Source, Arc<native_preview::Cancellation>),
    NativePage(u64, Arc<native_preview::Document>, usize, Arc<native_preview::Cancellation>),
}
enum Response {
    Catalog(u64, Result<Catalog, String>),
    Preview(u64, Result<Decoded, String>),
    NativePage(u64, Result<Decoded, String>),
}
enum ExternalEvent {
    Opened(u64, Result<Option<tempfile::TempDir>, String>),
    Failed(u64, String),
}

pub struct Viewer {
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Response>,
    serial: u64,
    cancel: Arc<native_preview::Cancellation>,
    native_page: usize,
    native_info: String,
    context: String,
    recorded: bool,
    recorded_revision: String,
    initialized: bool,
    loading: bool,
    cancelled: bool,
    catalog: Catalog,
    folder: PathBuf,
    filter: String,
    kind: usize,
    show_hidden: bool,
    selected: Option<PathBuf>,
    source: Option<Source>,
    trail: Vec<Source>,
    decoded: Option<Decoded>,
    native_document: Option<Arc<native_preview::Document>>,
    texture: Option<egui::TextureHandle>,
    image_zoom: f32,
    image_pan: egui::Vec2,
    white_paper: bool,
    source_text: bool,
    data_mode: usize,
    dataset: usize,
    citation_filter: Option<String>,
    citation_request: Option<String>,
    archive_filter: String,
    details: Option<String>,
    error: Option<String>,
    model: mesh::Viewport,
    table: data_view::TableView,
    external_tx: mpsc::Sender<ExternalEvent>,
    external_rx: mpsc::Receiver<ExternalEvent>,
    external_serial: u64,
    external_pending: Option<u64>,
    external_files: Vec<tempfile::TempDir>,
    external_error: Option<String>,
}
impl Viewer {
    pub fn new(ctx: &egui::Context, gl: Option<&Arc<glow::Context>>) -> Self {
        let (tx, requests) = mpsc::channel();
        let (external_tx, external_rx) = mpsc::channel();
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
                    Request::Scan(id, root, hidden) => {
                        Response::Catalog(id, loader::scan(&root, hidden))
                    }
                    Request::Recorded(id, source) => Response::Catalog(id, loader::recorded(source)),
                    Request::Load(id, source, cancel) => {
                        let ext = extension(Path::new(&source.name()));
                        let result = if let Some(route) = native_preview::route(&ext) {
                            native_preview::prepare(&source, route, &cancel).map(|document| {
                                let note = document.summary.clone();
                                Decoded { body: Body::Native(document), note }
                            })
                        } else { loader::decode(&source, &cancel) };
                        Response::Preview(id, result)
                    },
                    Request::NativePage(id, document, page, cancel) => Response::NativePage(id, native_preview::page(&document, page, &cancel)),
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
            cancel: Arc::new(native_preview::Cancellation::default()),
            native_page: 0,
            native_info: String::new(),
            context: String::new(),
            recorded: true,
            recorded_revision: String::new(),
            initialized: false,
            loading: false,
            cancelled: false,
            catalog: Catalog {
                files: Vec::new(),
                note: String::new(),
            },
            folder: PathBuf::new(),
            filter: String::new(),
            kind: 0,
            show_hidden: false,
            selected: None,
            source: None,
            trail: Vec::new(),
            decoded: None,
            native_document: None,
            texture: None,
            image_zoom: 1.0,
            image_pan: egui::Vec2::ZERO,
            white_paper: true,
            source_text: false,
            data_mode: 0,
            dataset: 0,
            citation_filter: None,
            citation_request: None,
            archive_filter: String::new(),
            details: None,
            error: None,
            model: mesh::Viewport::new(gl),
            table: data_view::TableView::default(),
            external_tx,
            external_rx,
            external_serial: 0,
            external_pending: None,
            external_files: vec![],
            external_error: None,
        }
    }
    pub fn destroy(&mut self) {
        self.cancel.cancel();
        self.model.destroy();
    }
    pub fn focus_citation(&mut self, message: String) {
        self.recorded = true;
        self.initialized = false;
        self.filter.clear();
        self.kind = 0;
        self.citation_request = Some(message);
    }
    pub fn deactivate(&mut self, ctx: &egui::Context) {
        if self.initialized {
            self.serial += 1;
            self.initialized = false;
            self.loading = false;
            self.catalog.files.clear();
            self.catalog.note.clear();
            self.selected = None;
            self.source = None;
            self.trail.clear();
            self.details = None;
            self.citation_filter = None;
            self.reset_preview();
        }
        // Drop late snapshots from a closed view; they must not retain exports
        // or replace a newer selection when the file panel is opened again.
        self.poll(ctx);
    }
    fn reset_preview(&mut self) {
        self.cancel.cancel();
        self.cancel = Arc::new(native_preview::Cancellation::default());
        self.native_page = 0;
        self.native_info.clear();
        self.decoded = None;
        self.native_document = None;
        self.texture = None;
        self.error = None;
        self.image_zoom = 1.0;
        self.image_pan = egui::Vec2::ZERO;
        self.source_text = false;
        self.data_mode = 0;
        self.dataset = 0;
        self.cancelled = false;
        self.archive_filter.clear();
        self.model.reset();
        self.table = data_view::TableView::default();
    }
    fn load(&mut self, source: Source) {
        self.reset_preview();
        self.serial += 1;
        self.loading = true;
        self.source = Some(source.clone());
        if self.tx.send(Request::Load(self.serial, source, self.cancel.clone())).is_err() {
            self.loading = false;
            self.error = Some("The preview worker is unavailable. Reopen the desktop to retry.".into());
        }
    }
    fn native_document(&self) -> Option<Arc<native_preview::Document>> {
        self.native_document.clone()
    }
    fn load_native_page(&mut self, document: Arc<native_preview::Document>, page: usize) {
        self.cancel.cancel();
        self.cancel = Arc::new(native_preview::Cancellation::default());
        self.serial += 1;
        self.native_page = page;
        self.loading = true;
        self.cancelled = false;
        self.error = None;
        self.texture = None;
        self.native_info.clear();
        self.image_zoom = 1.0;
        self.image_pan = egui::Vec2::ZERO;
        if self.tx.send(Request::NativePage(self.serial, document, page, self.cancel.clone())).is_err() {
            self.loading = false;
            self.error = Some("The preview worker is unavailable. Reopen the desktop to retry.".into());
        }
    }
    fn scan(&mut self, root: &Path, recorded: Option<&super::helpers::ResultFiles>) {
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
            let Some(source) = recorded else {
                self.loading = false;
                self.error = Some("Recorded files are unavailable until the task records load.".into());
                return;
            };
            self.recorded_revision = source.revision.clone();
            Request::Recorded(self.serial, source.clone())
        } else {
            Request::Scan(self.serial, root.into(), self.show_hidden)
        };
        if self.tx.send(request).is_err() {
            self.loading = false;
            self.error = Some("The file worker is unavailable. Reopen the desktop to retry.".into());
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.external_rx.try_recv() {
            match result {
                ExternalEvent::Opened(id, result) => {
                    if self.external_pending == Some(id) {
                        self.external_pending = None;
                    }
                    match result {
                        Ok(Some(dir)) => self.external_files.push(dir),
                        Ok(None) => {}
                        Err(error) if id == self.external_serial => self.external_error = Some(error),
                        Err(_) => {}
                    }
                }
                ExternalEvent::Failed(id, error) if id == self.external_serial => {
                    self.external_error = Some(error);
                }
                ExternalEvent::Failed(_, _) => {}
            }
        }
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
                            if let Body::Parameters(_, _, datasets) = &decoded.body {
                                self.table = data_view::TableView::with_header(datasets.first().and_then(|d| d.header));
                            }
                            if let Body::Image(image, _) = &decoded.body {
                                self.texture = Some(ctx.load_texture("artifact-preview", image.clone(), egui::TextureOptions::LINEAR));
                            }
                            self.native_document = match &decoded.body { Body::Native(document) => Some(document.clone()), _ => None };
                            self.decoded = Some(decoded);
                            if let Some(document) = self.native_document() {
                                self.white_paper = document.white_background;
                                self.load_native_page(document, 0);
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Response::NativePage(id, result) if id == self.serial => {
                    self.loading = false;
                    match result {
                        Ok(page) => {
                            if let Body::Image(image, info) = &page.body {
                                self.texture = Some(ctx.load_texture("native-page", image.clone(), egui::TextureOptions::LINEAR));
                                self.native_info = info.clone();
                            } else {
                                self.texture = None;
                                self.model.reset();
                                if let Body::Parameters(_, _, datasets) = &page.body {
                                    self.table = data_view::TableView::with_header(datasets.first().and_then(|d| d.header));
                                }
                            }
                            self.decoded = Some(page);
                        }
                        Err(error) => self.error = Some(error),
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
        recorded: Option<&super::helpers::ResultFiles>,
    ) {
        self.poll(ui.ctx());
        if self.context != context {
            self.context = context.into();
            self.initialized = false;
            self.filter.clear();
            self.kind = 0;
            self.citation_filter = None;
        }
        if let Some(message) = self.citation_request.take() {
            self.citation_filter = Some(message);
        }
        if self.recorded && !self.cancelled && recorded.is_some_and(|source| source.revision != self.recorded_revision) {
            self.initialized = false;
        }
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Viewer support", |ui| {
                ui.label("Built in: images/SVG, Markdown/code, JSON/CSV, STL/OBJ/GLB/glTF, ZIP.");
                ui.separator();
                for route in native_preview::ROUTES {
                    ui.strong(route.label);
                    ui.weak(route.extensions.join(", "));
                    match route.availability() {
                        Ok(()) => { ui.label("Executable found; rendering is checked when the file opens."); }
                        Err(reason) => { ui.label(reason); }
                    }
                }
                ui.weak("Unlisted formats have no registered native renderer. No universal format support is implied.");
            });
            let changed = ui
                .selectable_value(&mut self.recorded, false, "Workspace files")
                .changed()
                | ui.selectable_value(&mut self.recorded, true, "Recorded files")
                    .changed();
            if changed {
                self.filter.clear();
                self.kind = 0;
                self.citation_filter = None;
            }
            if changed
                || theme::icon_button(ui, theme::Icon::Refresh, "Refresh file list").clicked()
            {
                self.initialized = false;
            }
            if self.loading && self.source.is_none() {
                ui.weak("Preparing file list…");
            } else if self.error.is_none() && !self.cancelled {
                ui.weak(format!("{} files", self.catalog.files.len()));
            }
            if !self.recorded
                && ui
                    .checkbox(&mut self.show_hidden, "Include hidden & cache files")
                    .changed()
            {
                self.initialized = false;
            }
        });
        if let Some(message) = self.citation_filter.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Exported citations from message {message}"));
                if ui.small_button("Show all recorded files").clicked() {
                    self.citation_filter = None;
                }
            });
        }
        if !self.initialized {
            ui.ctx().forget_all_images();
            self.scan(root, recorded);
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
            let kinds=["All formats","Images & drawings","3D geometry","Documents & code","Data & parameters","ZIP packages","Other formats","Electronics","Audio & video"];
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
                if self.citation_filter.as_ref().is_some_and(|id| entry.citation_message.as_ref() != Some(id)) { continue; }
                if filtered {
                    if entry.path.to_string_lossy().to_lowercase().contains(&query) && (self.kind==0 || category(&entry.path)==self.kind) {entries.push(i);}
                } else if let Ok(relative)=entry.path.strip_prefix(&self.folder) {
                    let mut parts=relative.components();let first=parts.next();
                    if parts.next().is_some(){if let Some(c)=first{folders.insert(self.folder.join(c));}}
                    else {entries.push(i);}
                }
            }
            egui::ScrollArea::vertical().id_salt("artifact-list").show(ui,|ui|{
                if folders.is_empty() && entries.is_empty() && !self.catalog.files.is_empty() { ui.weak("No files match the current search, format, and citation filters."); }
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
                if self.catalog.files.is_empty() && !self.loading && self.error.is_none() && !self.cancelled {ui.weak(if self.recorded{"No delivered or cited files yet. Check Workspace files for other produced work."}else{"No visible files in the working folder."});}
            });
        });
        if let Some(index) = open {
            ui.ctx().forget_all_images();
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
        let mut external = None;
        let mut reload = false;
        ui.horizontal_wrapped(|ui| {
            if !self.trail.is_empty() && ui.button("Back to package").clicked() {
                back = true;
            }
            if let Some(source) = &self.source {
                ui.add(egui::Label::new(egui::RichText::new(source.name()).strong()).truncate())
                    .on_hover_text(source.label());
                ui.menu_button("Actions", |ui| {
                    if ui.button("Reload preview").clicked() {
                        reload = true;
                        ui.close();
                    }
                    if ui.button("Copy file location").clicked() {
                        ui.ctx().copy_text(source.label());
                        ui.close();
                    }
                    if ui.add_enabled(self.external_pending.is_none(), egui::Button::new("Open in external application")).clicked() {
                        external = Some((source.clone(), false));
                        ui.close();
                    }
                    if ui.add_enabled(self.external_pending.is_none(), egui::Button::new("Open with…")).clicked() {
                        external = Some((source.clone(), true));
                        ui.close();
                    }
                    if let Source::File(path) = source {
                        if ui.add_enabled(self.external_pending.is_none(), egui::Button::new("Open containing folder")).clicked() {
                            if let Some(parent) = path.parent() {
                                external = Some((Source::File(parent.into()), false));
                            }
                            ui.close();
                        }
                    }
                });
            }
        });
        if reload {
            if let Some(source) = self.source.clone() {
                self.load(source);
            }
        }
        if let Some((source, choose)) = external.take() {
            self.open_external(ui.ctx(), source, choose);
        }
        if self.external_pending.is_some() {
            ui.weak("Opening externally or waiting for application selection…");
        }
        if let Some(error) = self.external_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme::ACCENT, error);
                if ui.small_button("Dismiss").clicked() {
                    self.external_error = None;
                }
            });
        }
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
        if let Some(document) = self.native_document() {
            let mut page = self.native_page;
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(page > 0, egui::Button::new("Previous")).clicked() { page -= 1; }
                egui::ComboBox::from_id_salt("native-preview-page")
                    .selected_text(&document.labels[page])
                    .show_ui(ui, |ui| {
                        for (index, label) in document.labels.iter().enumerate() {
                            ui.selectable_value(&mut page, index, label);
                        }
                    });
                if ui.add_enabled(page + 1 < document.labels.len(), egui::Button::new("Next")).clicked() { page += 1; }
                ui.weak(format!("{} / {}", page + 1, document.labels.len()));
            });
            ui.add(egui::Label::new(egui::RichText::new(&document.summary).small().weak()).wrap());
            ui.collapsing("Renderer diagnostics", |ui| {
                if !document.diagnostics.trim().is_empty() {
                    egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&document.diagnostics).monospace()).wrap().selectable(true));
                    });
                }
            });
            if page != self.native_page { self.load_native_page(document, page); }
        }
        if self.loading {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(if self.source.is_some() {
                    "Preparing preview…"
                } else if self.recorded {
                    "Preparing recorded file list…"
                } else {
                    "Reading workspace file list…"
                });
                if ui.button("Cancel loading")
                    .on_hover_text("Stop an owned native converter and dismiss this request. Other decoding may finish, but its result will not open.")
                    .clicked()
                {
                    self.cancel.cancel();
                    self.serial += 1;
                    self.loading = false;
                    self.cancelled = true;
                }
            });
            return;
        }
        if self.error.is_some() || self.cancelled {
            if let Some(error) = &self.error {
                ui.colored_label(theme::ACCENT, error);
            } else {
                ui.label("Preview request cancelled.");
            }
            if ui.button("Retry").clicked() {
                if let Some(document) = self.native_document() {
                    self.load_native_page(document, self.native_page);
                } else if let Some(source) = self.source.clone() {
                    self.load(source);
                } else {
                    self.initialized = false;
                }
            }
            return;
        }
        let Some(decoded) = &self.decoded else {
            ui.add_space(35.0);
            ui.heading("Preview produced files");
            ui.label("Select an image, drawing, document, model, or package.");
            ui.weak("Images · documents · data · 3D · electronics · media · packages. Viewer support lists optional renderers.");
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
            Body::Image(_, info) => image_preview(ui, self.texture.as_ref(), &mut self.image_zoom, &mut self.image_pan, &mut self.white_paper, info),
            Body::Native(_) => image_preview(ui, self.texture.as_ref(), &mut self.image_zoom, &mut self.image_pan, &mut self.white_paper, &self.native_info),
            Body::Mesh(mesh) => self.model.ui(ui, mesh),
            Body::Text(text, markdown) => {
                ui.horizontal_wrapped(|ui| {
                    if *markdown {
                        ui.selectable_value(&mut self.source_text, false, "Rendered Markdown");
                        ui.selectable_value(&mut self.source_text, true, "Source");
                    } else {
                        ui.weak("Text / code source");
                    }
                    if ui.small_button("Copy source").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
                if *markdown && !self.source_text && !matches!(&self.source, Some(Source::File(_))) {
                    ui.weak("Local images resolve in workspace Markdown. Recorded and packaged Markdown retain image references without resolving adjacent files.");
                }
                egui::ScrollArea::both()
                    .id_salt(("artifact-text", self.serial))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if *markdown && !self.source_text {
                            ui.set_min_width(ui.available_width());
                            let base = match &self.source {
                                Some(Source::File(path)) => path.parent(),
                                _ => None,
                            };
                            document::markdown_at(ui, text, base);
                        } else {
                            let language = self
                                .source
                                .as_ref()
                                .map(|s| extension(Path::new(&s.name())))
                                .unwrap_or_default();
                            document::code(ui, text, &language);
                        }
                    });
            }
            Body::Table(rows, text) => {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.source_text, false, "Table / plot");
                    ui.selectable_value(&mut self.source_text, true, "Source");
                    if ui.small_button("Copy source").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
                if self.source_text {
                    egui::ScrollArea::both().id_salt(("table-source", self.serial)).show(ui, |ui| document::code(ui, text, "text"));
                } else {
                    self.table.ui(ui, rows);
                }
            }
            Body::Parameters(value, text, datasets) => {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.data_mode, 0, "Parameters");
                    if !datasets.is_empty() {
                        ui.selectable_value(&mut self.data_mode, 1, "Table / plot");
                    }
                    ui.selectable_value(&mut self.data_mode, 2, "JSON source");
                    if ui.small_button("Copy source").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
                if self.data_mode == 1 {
                    let before = self.dataset;
                    let label = |dataset: &data_view::Dataset| {
                        if dataset.pointer.is_empty() { "Document root".to_owned() } else { dataset.pointer.clone() }
                    };
                    if let Some(dataset) = datasets.get(self.dataset) {
                        egui::ComboBox::from_id_salt("json-dataset")
                            .selected_text(label(dataset))
                            .show_ui(ui, |ui| {
                                for (index, dataset) in datasets.iter().enumerate() {
                                    ui.selectable_value(&mut self.dataset, index, label(dataset));
                                }
                            });
                    }
                    if let Some(dataset) = datasets.get(self.dataset) {
                        if before != self.dataset {
                            self.table = data_view::TableView::with_header(dataset.header);
                        }
                        ui.weak("Dataset locations are exact JSON Pointers. Column names and values retain their source meaning; no simulation is executed.");
                        self.table.ui(ui, &dataset.rows);
                    }
                } else {
                    egui::ScrollArea::both()
                        .id_salt(("parameters", self.serial))
                        .show(ui, |ui| {
                            if self.data_mode == 2 {
                                document::code(ui, text, "json");
                            } else {
                                data_view::parameters(ui, value);
                            }
                        });
                }
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
                ui.heading("No embedded preview for this format");
                ui.label(message);
                if let Some(source) = self.source.clone() {
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(self.external_pending.is_none(), egui::Button::new("Open in external application")).clicked() {
                            external = Some((source.clone(), false));
                        }
                        if ui.add_enabled(self.external_pending.is_none(), egui::Button::new("Open with…")).clicked() {
                            external = Some((source, true));
                        }
                    });
                }
            }
        }
        if let Some((source, choose)) = external {
            self.open_external(ui.ctx(), source, choose);
        }
        if let Some((index, name)) = archive_entry {
            if let Some(source) = self.source.clone() {
                self.trail.push(source.clone());
                self.load(Source::Zip {
                    container: Arc::new(source),
                    index,
                    name,
                });
            }
        }
    }
    fn open_external(&mut self, ctx: &egui::Context, source: Source, choose: bool) {
        if self.external_pending.is_some() {
            return;
        }
        self.external_serial += 1;
        let id = self.external_serial;
        self.external_pending = Some(id);
        self.external_error = None;
        let tx = self.external_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Option<tempfile::TempDir>, String> {
                let application = if choose {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose an application to open this file")
                        .pick_file()
                    else {
                        return Ok(None);
                    };
                    Some(path)
                } else {
                    None
                };
                let mut temporary = None;
                let path = match &source {
                    Source::File(path) => path.clone(),
                    _ => {
                        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
                        let name = source.name();
                        let name = Path::new(&name)
                            .file_name()
                            .filter(|n| !n.is_empty())
                            .unwrap_or(std::ffi::OsStr::new("artifact"));
                        let path = dir.path().join(name);
                        std::fs::write(&path, source.bytes()?).map_err(|e| e.to_string())?;
                        temporary = Some(dir);
                        path
                    }
                };
                let command = application.unwrap_or_else(|| {
                    if cfg!(target_os = "macos") {
                        "open".into()
                    } else {
                        "xdg-open".into()
                    }
                });
                let mut child = std::process::Command::new(command)
                    .arg(&path)
                    .spawn()
                    .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
                let errors = tx.clone();
                let repaint = ctx.clone();
                let label = source.label();
                std::thread::spawn(move || {
                    if let Ok(status) = child.wait() {
                        if !status.success() {
                            let _=errors.send(ExternalEvent::Failed(id, format!("The external application for {label} exited with {status}. Choose another application with Open with…")));
                            repaint.request_repaint();
                        }
                    }
                });
                Ok(temporary)
            })();
            let _ = tx.send(ExternalEvent::Opened(id, result));
            ctx.request_repaint();
        });
    }
}
fn image_preview(ui: &mut egui::Ui, texture: Option<&egui::TextureHandle>, zoom: &mut f32, pan: &mut egui::Vec2, white: &mut bool, info: &str) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Fit").clicked() { *zoom = 1.0; *pan = egui::Vec2::ZERO; }
        ui.add(egui::Slider::new(&mut *zoom, 0.2..=8.0).logarithmic(true).show_value(false).text("Zoom"));
        ui.checkbox(&mut *white, "White background");
        ui.weak(info);
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size().max(egui::vec2(40.0, 40.0)), egui::Sense::click_and_drag());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0, egui::Color32::from_gray(if *white { 248 } else { 46 }));
    if response.dragged() { *pan += ui.input(|i| i.pointer.delta()); }
    if response.hovered() { *zoom = (*zoom * (ui.input(|i| i.smooth_scroll_delta.y) * 0.003).exp()).clamp(0.2, 8.0); }
    if response.double_clicked() { *zoom = 1.0; *pan = egui::Vec2::ZERO; }
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let factor = (rect.width() / size.x).min(rect.height() / size.y) * *zoom;
        let target = egui::Rect::from_center_size(rect.center() + *pan, size * factor);
        painter.image(texture.id(), target, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
}
impl Drop for Viewer {
    fn drop(&mut self) { self.cancel.cancel(); }
}
fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
fn category(path: &Path) -> usize {
    match extension(path).as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" | "tiff" | "tif" | "ico" | "dxf" => 1,
        "stl" | "obj" | "glb" | "gltf" | "step" | "stp" | "iges" | "igs" | "blend" | "fcstd" => 2,
        "csv" | "tsv" | "json" | "h5" | "hdf5" | "hdf" => 4,
        "zip" => 5,
        "kicad_pcb" | "kicad_sch" | "brd" | "sch" | "gbr" | "ger" | "gerber" | "gtl" | "gbl" | "gts" | "gbs" | "gto" | "gbo" | "gko" | "gml" | "gm1" | "drl" | "xln" => 7,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "mp4" | "mov" | "webm" | "avi" | "mkv" | "m4v" => 8,
        "bin" | "dwg" => 6,
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
