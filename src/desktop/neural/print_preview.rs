//! Print the selected immutable HTML revision and inspect the resulting PDF.
use super::*;
use ai_personas::{
    document_print::{self, Resource},
    neural::{self, failure, store},
};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn is_html(path: &str, media: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mime = media.split(';').next().unwrap_or("").trim();
    ["html", "htm", "xhtml"]
        .iter()
        .any(|value| ext.eq_ignore_ascii_case(value))
        || ["text/html", "application/xhtml+xml"]
            .iter()
            .any(|value| mime.eq_ignore_ascii_case(value))
}

#[derive(Clone)]
pub(super) struct Source {
    pub path: String,
    pub reference: String,
    pub package: Option<String>,
}

#[derive(Clone)]
pub(super) struct Document {
    pub pdf: Arc<Vec<u8>>,
    pub pages: u32,
    renderer: String,
    companions: usize,
}

fn asset(db: &Connection, reference: &str) -> neural::Result<(String, String)> {
    let file = store::exact(db, reference)?;
    if file.reference.kind != "asset" || file.reference.handle() != reference {
        return Err(failure("Select an exact file revision."));
    }
    let path = file.body["path"]
        .as_str()
        .ok_or_else(|| failure("This revision has no retained filename."))?;
    let blob = file.body["blob"]
        .as_str()
        .ok_or_else(|| failure("This revision has no retained bytes."))?;
    Ok((path.into(), blob.into()))
}

fn public_bytes(db: &Connection, digest: &str) -> neural::Result<Vec<u8>> {
    let (bytes, metadata_only) = data::preview_bytes(db, digest)?;
    if metadata_only {
        return Err(failure(
            "Provider receipt metadata cannot be used as a print file.",
        ));
    }
    Ok(bytes)
}

fn print(root: &Path, source: &Source, stopped: &AtomicBool) -> neural::Result<Document> {
    let cancelled = || stopped.load(Ordering::Relaxed);
    let db = data::open(root)?;
    let edition = source.package.as_deref().map(|r| neural::human_requests::Materials::open(&db, r)).transpose()?;
    let bytes = if let Some(edition) = &edition {
        let file = edition.read(&source.reference)?;
        if file.path != source.path { return Err(failure("The filename differs from the selected issued file.")); }
        file.bytes
    } else if let Some(blob) = source.reference.strip_prefix("object:") {
        public_bytes(&db, blob)?
    } else {
        let (path, blob) = asset(&db, &source.reference)?;
        if path != source.path { return Err(failure("The selected filename differs from its exact revision.")); }
        public_bytes(&db, &blob)?
    };
    if bytes.starts_with(b"%PDF-") {
        return Err(failure("This revision already contains a PDF. Open the file to view its pages."));
    }
    let mut resources = Vec::new();
    if let Some(edition) = &edition {
        for member in edition.members()?.iter().filter(|r| **r != source.reference) {
            if cancelled() { return Err(failure("Print preview cancelled.")); }
            let file = edition.read(member)?;
            resources.push(Resource { path: file.path, bytes: file.bytes });
        }
    }
    drop(db);
    let result = if source.package.is_some() {
        document_print::print_html_at_with_control(&bytes, &source.path, &resources, cancelled)
    } else {
        document_print::print_html_with_control(&bytes, &[], cancelled)
    }
    .map_err(failure)?;
    Ok(Document {
        pdf: Arc::new(result.pdf),
        pages: result.pages,
        renderer: result.renderer,
        companions: resources.len(),
    })
}

enum Output {
    Document(Document),
    Page(egui::ColorImage),
}

type Job = Box<dyn FnOnce() + Send>;

/// Serialize print operations across selections. Closing Studio drains the
/// cancelled queue and reaps the running renderer before the process exits.
#[derive(Default)]
pub(super) struct Worker {
    thread: Option<(mpsc::Sender<Job>, std::thread::JoinHandle<()>)>,
}

impl Worker {
    fn queue(&mut self) -> mpsc::Sender<Job> {
        let (queue, _) = self.thread.get_or_insert_with(|| {
            let (queue, jobs) = mpsc::channel::<Job>();
            let worker = std::thread::spawn(move || {
                for job in jobs {
                    job();
                }
            });
            (queue, worker)
        });
        queue.clone()
    }

    pub(super) fn shutdown(&mut self) {
        if let Some((queue, worker)) = self.thread.take() {
            drop(queue);
            let _ = worker.join();
        }
    }
}

struct Task {
    stop: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<Output, String>>,
}

impl Task {
    fn start(
        queue: &mpsc::Sender<Job>,
        ctx: &egui::Context,
        work: impl FnOnce(&AtomicBool) -> neural::Result<Output> + Send + 'static,
    ) -> Self {
        let (tx, result) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let ctx = ctx.clone();
        let _ = queue.send(Box::new(move || {
            if !flag.load(Ordering::Relaxed) {
                let _ = tx.send(work(&flag).map_err(|error| error.to_string()));
                ctx.request_repaint();
            }
        }));
        Self { stop, result }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

enum State {
    Working(Task),
    Ready(egui::TextureHandle),
    Error(String),
    Cancelled,
}

pub(super) struct Preview {
    queue: mpsc::Sender<Job>,
    source: Source,
    on_desk: bool,
    document: Option<Document>,
    pages: pdf_pages::Pages,
    state: State,
    focus: bool,
    trigger: Option<egui::Id>,
    export: Option<u64>,
    saved_to: Option<String>,
}

impl Preview {
    fn request(&mut self, ctx: &egui::Context, root: &Path) {
        self.state = State::Working(if let Some(doc) = &self.document {
            let pdf = doc.pdf.clone();
            let count = doc.pages;
            let page = self.pages.page;
            let max_side = ctx.input(|input| input.max_texture_side);
            Task::start(&self.queue, ctx, move |stop| {
                let (pixels, actual) =
                    pdf_pages::render(&pdf, page, max_side, || stop.load(Ordering::Relaxed))?;
                if actual != count {
                    return Err(failure("The PDF page count differs from the print result."));
                }
                Ok(Output::Page(pixels))
            })
        } else {
            let root = root.to_owned();
            let source = self.source.clone();
            Task::start(&self.queue, ctx, move |stop| {
                print(&root, &source, stop).map(Output::Document)
            })
        });
        ctx.request_repaint();
    }

    fn poll(&mut self, ctx: &egui::Context, root: &Path) {
        let State::Working(task) = &self.state else {
            return;
        };
        match task.result.try_recv() {
            Ok(Ok(Output::Document(doc))) => {
                self.pages.count = Some(doc.pages);
                self.document = Some(doc);
                self.request(ctx, root);
            }
            Ok(Ok(Output::Page(image))) => {
                self.state =
                    State::Ready(ctx.load_texture("printed-html-page", image, pdf_pages::SAMPLING));
            }
            Ok(Err(error)) => self.state = State::Error(error),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.state = State::Error("The print preview worker stopped.".into())
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(super) fn finish_export(&mut self, serial: u64, destination: Option<&str>) {
        if self.export == Some(serial) {
            self.focus = true;
            self.saved_to = destination.map(str::to_owned);
        }
    }

    fn show(&mut self, ctx: &egui::Context, root: &Path, exporting: bool) -> (bool, bool) {
        self.poll(ctx, root);
        let viewport = ctx.content_rect();
        let max_width = (viewport.width() - 30.).max(150.);
        let width = max_width.min(655.);
        let height = (viewport.height() - 100.).clamp(120., 780.);
        let mut open = true;
        let mut close = false;
        let mut save = false;
        egui::Window::new("Print preview")
            .id(egui::Id::new("retained-html-print"))
            .open(&mut open)
            .collapsible(false)
            .resizable([true, false])
            .fade_out(false)
            .min_width(max_width.min(250.))
            .default_width(width)
            .default_height(height)
            .max_width(max_width)
            .max_height(height)
            .default_pos(egui::pos2(
                viewport.center().x - width * 0.5,
                viewport.top() + 26.,
            ))
            .show(ctx, |ui| {
                ui.set_max_height(height);
                egui::ScrollArea::vertical()
                    .id_salt("print-window-scroll")
                    .max_height(height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let top = ui.cursor().top();
                        ui.set_min_width(width.min(ui.available_width()));
                        ui.add(
                            egui::Label::new(RichText::new(&self.source.path).size(21.).strong())
                                .wrap(),
                        );
                        let origin = if self.source.package.is_some() {
                            "Issued package"
                        } else {
                            "Retained file"
                        };
                        let edition = self
                            .source
                            .reference
                            .rsplit_once('@')
                            .map(|(_, n)| format!("Revision {n}"))
                            .unwrap_or_else(|| "Exact object".into());
                        ui.small(format!("{origin}  ·  {edition}"));
                        ui.add_space(4.);
                        ui.horizontal_wrapped(|ui| {
                            save = ui
                                .add_enabled(
                                    self.document.is_some() && !exporting,
                                    theme::primary("Save PDF…"),
                                )
                                .on_hover_text("Save the PDF shown in this preview")
                                .clicked();
                            let dismiss = ui
                                .button("Close preview")
                                .on_hover_text("Close print preview (Esc)");
                            if !exporting && std::mem::take(&mut self.focus) {
                                ctx.move_to_top(ui.layer_id());
                                dismiss.request_focus();
                            }
                            close = dismiss.clicked();
                            if matches!(self.state, State::Working(_))
                                && ui.button("Cancel rendering").clicked()
                            {
                                self.state = State::Cancelled;
                            }
                        });
                        if let Some(destination) = &self.saved_to {
                            ui.add(egui::Label::new(destination).wrap());
                        }
                        let focused = ctx
                            .memory(|m| m.focused())
                            .and_then(|id| ctx.read_response(id));
                        let shortcuts = !exporting
                            && !egui::Popup::is_any_open(ctx)
                            && ctx.top_layer_id() == Some(ui.layer_id())
                            && focused.is_none_or(|w| w.layer_id == ui.layer_id());
                        if shortcuts
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                            })
                        {
                            close = true;
                        }
                        egui::CollapsingHeader::new("Print details")
                            .id_salt(("print-origin", &self.source.reference))
                            .show(ui, |ui| {
                                ui.label(format!("HTML source  ·  {}", self.source.reference));
                                if let Some(package) = &self.source.package {
                                    ui.label(format!("Package  ·  {package}"));
                                }
                                if let Some(doc) = &self.document {
                                    ui.label(format!(
                                        "{}  ·  {} PDF  ·  {}",
                                        doc.renderer,
                                        size(doc.pdf.len() as u64),
                                        counted(
                                            doc.companions as u64,
                                            "companion file",
                                            "companion files"
                                        )
                                    ));
                                }
                            });
                        if self.document.is_some() {
                            ui.add_space(5.);
                            let original = self.pages.page;
                            if shortcuts {
                                ui.input_mut(|i| {
                                    if i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp) {
                                        self.pages.page = self.pages.page.saturating_sub(1).max(1);
                                    }
                                    if i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
                                        self.pages.page = self
                                            .pages
                                            .page
                                            .saturating_add(1)
                                            .min(self.pages.count.unwrap_or(1));
                                    }
                                });
                            }
                            self.pages.navigation(ui, &self.source.reference);
                            if self.pages.page != original {
                                self.request(ctx, root);
                            }
                        }
                        ui.add_space(5.);
                        self.pages.height_limit =
                            Some((height - (ui.cursor().top() - top) - 8.).max(100.));
                        match &self.state {
                            State::Working(_) if self.document.is_some() => self.pages.loading(ui),
                            State::Working(_) => {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spinner();
                                    ui.label("Laying out print pages…");
                                });
                            }
                            State::Ready(texture) => self.pages.show(
                                ui,
                                texture,
                                &self.source.path,
                                &self.source.reference,
                            ),
                            State::Error(reason) => {
                                ui.strong(if self.document.is_some() {
                                    "Page preview unavailable"
                                } else {
                                    "Print layout unavailable"
                                });
                                ui.add(egui::Label::new(reason).wrap());
                                if ui.button("Retry rendering").clicked() {
                                    self.request(ctx, root);
                                }
                            }
                            State::Cancelled => {
                                ui.label("Rendering cancelled.");
                                if ui.button("Resume preview").clicked() {
                                    self.request(ctx, root);
                                }
                            }
                        }
                    });
            });
        (open && !close, save)
    }
}

impl App {
    pub(super) fn open_print_preview(
        &mut self,
        ctx: &egui::Context,
        reference: &str,
        path: &str,
        package: Option<String>,
        trigger: Option<egui::Id>,
    ) {
        let mut preview = Preview {
            queue: self.print_worker.queue(),
            source: Source {
                path: path.into(),
                reference: reference.into(),
                package,
            },
            on_desk: self.layout.editor == Editor::Delivery,
            document: None,
            pages: Default::default(),
            state: State::Cancelled,
            focus: true,
            trigger,
            export: None,
            saved_to: None,
        };
        preview.request(ctx, &self.root);
        self.print_preview = Some(preview);
    }

    pub(super) fn print_preview_window(&mut self, ctx: &egui::Context) {
        let Some(mut preview) = self.print_preview.take() else {
            return;
        };
        let source = &preview.source;
        let active = if preview.on_desk {
            self.layout.editor == Editor::Delivery
                && source.package.as_deref() == Some(self.layout.delivery.as_str())
                && self.delivery_desk.selected_file() == Some(source.reference.as_str())
        } else {
            self.layout.editor == Editor::Artifacts
                && self.artifact_mode == ArtifactView::Retained
                && self.selected_reference == source.reference
        };
        if !active {
            return;
        }
        let (open, save) = preview.show(ctx, &self.root, self.export_dialog.is_some());
        if !open {
            if let Some(id) = preview
                .trigger
                .filter(|id| ctx.read_response(*id).is_some())
            {
                ctx.memory_mut(|m| m.request_focus(id));
            }
            return;
        }
        if save {
            if let Some(document) = &preview.document {
                self.save_print_pdf(&preview.source, document);
                preview.export = Some(self.export_generation);
                preview.saved_to = None;
            }
        }
        self.print_preview = Some(preview);
    }
}
