//! A bounded in-page preview of a single immutable package member.
use super::*;
use super::super::super::data_view;
use ai_personas::neural::issued_files::{Edition, Source};
use std::{
    io::Cursor,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::desktop::html_view as html;
#[path = "archive.rs"]
mod archive;
#[path = "straight_paths.rs"]
mod straight_paths;
pub(in crate::desktop::neural) use archive::Member as ZipFile;
use super::super::pdf_pages as pdf;

// These bounds apply to the inline preview only. The original revision stays
// available in the full file viewer and in the package.
pub(super) const SOURCE_LIMIT: u64 = 24 * 1024 * 1024;
const RASTER_PIXELS: u64 = 24_000_000;
const TEXT_CHARS: usize = 1050;
const TEXT_LINES: usize = 9;

#[derive(Clone, Copy)]
enum Kind {
    Pdf,
    Vector,
    Raster,
    Html,
    Archive,
    Table(bool), // true for TSV
    Text,
    Other,
}

pub(super) struct Format {
    pub label: String,
    kind: Kind,
}

impl Format {
    pub fn identify(path: &str, media: &str) -> Self {
        let ext = Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = media
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let kind = match (ext.as_str(), mime.as_str()) {
            ("zip", _) | (_, "application/zip") => Kind::Archive,
            ("pdf", _) | (_, "application/pdf") => Kind::Pdf,
            ("svg", _) | (_, "image/svg+xml") => Kind::Vector,
            ("csv", _) | (_, "text/csv") => Kind::Table(false),
            ("tsv", _) | (_, "text/tab-separated-values") => Kind::Table(true),
            ("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico", _)
            | (
                _,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/tiff"
                | "image/bmp" | "image/x-icon",
            ) => Kind::Raster,
            _ if print_preview::is_html(path, media) => {
                Kind::Html
            }
            (
                "md" | "markdown" | "txt" | "json" | "yaml" | "yml" | "xml" | "net"
                | "gbr" | "drl" | "kicad_sch" | "kicad_pcb" | "cir" | "spice"
                | "py" | "rs" | "sh" | "js" | "ts" | "toml" | "sha256",
                _,
            ) => Kind::Text,
            // Text-encoded geometry and digital traces need the full file
            // decoder, not a text/plain excerpt with the records cut off.
            ("stl" | "obj" | "gltf" | "glb" | "vcd", _) => Kind::Other,
            (_, mime) if mime.starts_with("text/") || mime == "application/json" => Kind::Text,
            _ => Kind::Other,
        };
        let label = match (kind, ext.as_str()) {
            (Kind::Pdf, _) => "PDF document".into(),
            (Kind::Vector, _) => "SVG drawing".into(),
            (Kind::Raster, _) => "Raster image".into(),
            (Kind::Html, _) => "HTML document".into(),
            (Kind::Archive, _) => "ZIP archive".into(),
            (Kind::Table(false), _) => "CSV table".into(),
            (Kind::Table(true), _) => "TSV table".into(),
            (Kind::Text, "md" | "markdown") => "Markdown".into(),
            (Kind::Text, "gbr") => "Gerber source".into(),
            (Kind::Text, "drl") => "Drill source".into(),
            (Kind::Other, "vcd") => "Digital logic trace".into(),
            (Kind::Text, "py") => "Python source".into(),
            (Kind::Text, "txt" | "") => "Text file".into(),
            (_, "") => {
                if mime.is_empty() {
                    "File".into()
                } else {
                    mime.into()
                }
            }
            (_, ext) => format!("{} file", ext.to_uppercase()),
        };
        Self { label, kind }
    }
    pub fn visual(&self) -> bool {
        matches!(self.kind, Kind::Vector | Kind::Raster | Kind::Pdf)
    }
    pub fn inline(&self) -> bool {
        !matches!(self.kind, Kind::Other)
    }
    pub fn html(&self) -> bool {
        matches!(self.kind, Kind::Html)
    }
    pub fn archive(&self) -> bool {
        matches!(self.kind, Kind::Archive)
    }
}

struct Job {
    root: PathBuf,
    edition: String,
    reference: String,
    file_path: String,
    file_digest: String,
    path: String,
    kind: Kind,
    page: u32,
    serial: u64,
    ctx: egui::Context,
    member: Option<archive::Member>,
}
enum Content {
    Image(egui::ColorImage, Option<String>, Option<straight_paths::Drawing>),
    Pdf(egui::ColorImage, u32),
    Html(html::Preview),
    Archive(archive::Listing),
    Table(Vec<Vec<String>>),
    Studio(Arc<tempfile::TempDir>, PathBuf, String),
    Text(String, bool),
}
struct Identity {
    source: Source,
    member: Option<(String, usize, String)>,
}
impl Identity {
    fn show(&self, ui: &mut Ui, inner_open: bool) {
        let s = &self.source;
        egui::CollapsingHeader::new(if inner_open { "Verified enclosing archive" } else { "Verified file identity" })
            .id_salt(("issued-file-identity", &s.edition, &s.file))
            .show(ui, |ui| {
                ui.weak("Checked against the issuing decision, the original file write and the retained bytes. Not a quality verdict or physical receipt.");
                let mut details = format!("Edition: {}\nIssue decision: {}\nFile: {}\nPath: {}\nContent revision: {}\nWriter: {}\nWrite decision: {}\nBytes: {}\nSHA-256: {}", s.edition, s.issuing_decision, s.file, s.path, s.content_revision, s.content_author, s.content_decision, s.bytes, s.sha256);
                if let Some((path, bytes, hash)) = &self.member {
                    details.push_str(&format!("\nZIP member: {path}\nMember bytes: {bytes}\nMember SHA-256: {hash}"));
                }
                ui.add(egui::Label::new(RichText::new(&details).small().monospace()).wrap().selectable(true));
                if ui.small_button("Copy file identity").clicked() { ui.ctx().copy_text(details); }
            });
    }
}
type Answer = (u64, ai_personas::neural::Result<(Identity, Content)>);

struct Worker {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Answer>,
}

impl Worker {
    fn new(latest: Arc<AtomicU64>) -> Self {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (out, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(first) = jobs.recv() {
                let job = jobs.try_iter().last().unwrap_or(first);
                let cancelled = || latest.load(Ordering::Relaxed) != job.serial;
                if cancelled() {
                    continue;
                }
                let answer = load(&job, cancelled);
                if cancelled() {
                    continue;
                }
                if out.send((job.serial, answer)).is_err() {
                    break;
                }
                job.ctx.request_repaint();
            }
        });
        Self { tx, rx }
    }
}

#[derive(Default)]
enum View {
    #[default]
    Pending,
    Image(egui::TextureHandle, Option<String>, Option<straight_paths::Drawing>, bool),
    Pdf(egui::TextureHandle),
    Html(html::Preview),
    Archive(archive::Listing),
    Table(Vec<Vec<String>>, data_view::TableView),
    Text(String, bool),
    Studio,
    Note(String),
}

#[derive(Default)]
pub(super) struct Inline {
    gl: Option<Arc<eframe::glow::Context>>,
    studio: Option<artifacts::Viewer>,
    worker: Option<Worker>,
    latest: Arc<AtomicU64>,
    reference: String,
    edition: String,
    file_bytes: Option<u64>,
    identity: Option<Identity>,
    pages: pdf::Pages,
    html_source: bool,
    view: View,
    member: Option<archive::Member>,
    directory: Option<archive::Listing>,
}

impl Drop for Inline {
    fn drop(&mut self) {
        self.latest.fetch_add(1, Ordering::Relaxed);
        self.release();
    }
}

impl Inline {
    pub fn new(gl: Option<&Arc<eframe::glow::Context>>) -> Self {
        let mut inline = Self::default();
        inline.gl = gl.cloned();
        inline
    }

    pub fn release(&mut self) {
        if let Some(mut viewer) = self.studio.take() { viewer.destroy(); }
    }

    pub fn clear(&mut self) {
        self.latest.fetch_add(1, Ordering::Relaxed);
        self.reference.clear();
        self.edition.clear();
        self.file_bytes = None;
        self.identity = None;
        if let Some(viewer) = &mut self.studio { viewer.clear_selection(); }
        self.pages = Default::default();
        self.view = View::Pending;
        self.member = None;
        self.directory = None;
    }

    pub fn detected_pdf(&self, reference: &str) -> bool {
        self.reference == reference && self.member.is_none() && self.pages.count.is_some()
    }

    pub fn selected_member(&self) -> Option<&ZipFile> {
        self.member.as_ref()
    }

    /// The embedded reader can descend farther than the delivery’s first
    /// ZIP listing. Its actions must follow the visible file at every depth.
    pub fn viewed_member(&self) -> Result<Option<artifacts::ViewedMember>, String> {
        if !matches!(self.view, View::Studio) { return Ok(None); }
        let Some(entry) = self.studio.as_ref().and_then(|viewer| viewer.viewed_member()) else { return Ok(None) };
        let identity = self.identity.as_ref().ok_or("Missing verified archive identity.")?;
        let original = &identity.source;
        match (&identity.member, &self.member) {
            (Some((path, _, digest)), Some(parent)) if path == &parent.path =>
                entry.in_issued_zip(digest, &original.sha256, &original.path, parent.index, path).map(Some),
            (None, None) => { entry.check_archive(&original.sha256)?; Ok(Some(entry)) }
            _ => Err("The selected archive changed. Reopen the member.".into()),
        }
    }

    fn request(&mut self, ui: &Ui, root: &Path, file: &File) {
        let serial = self.latest.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        ui.ctx().request_repaint();
        self.identity = None;
        self.view = match (file.bytes, &file.digest) {
            (None, Some(digest)) => View::Note(format!("Missing retained file bytes for {} (object:{digest})", file.path)),
            (_, None) => View::Note("The file metadata is unavailable.".into()),
            (Some(n), _) if n > SOURCE_LIMIT => {
                View::Note("This file is too large for an inline preview.".into())
            }
            (Some(_), Some(digest)) => {
                let worker = self
                    .worker
                    .get_or_insert_with(|| Worker::new(self.latest.clone()));
                let job = Job {
                    root: root.into(),
                    edition: self.edition.clone(),
                    reference: file.reference.clone(),
                    file_path: file.path.clone(),
                    file_digest: digest.clone(),
                    path: self.member.as_ref().map_or_else(|| file.path.clone(), |m| m.path.clone()),
                    kind: self.member.as_ref().map_or(file.format.kind, |m| m.kind),
                    page: self.pages.page,
                    serial,
                    ctx: ui.ctx().clone(),
                    member: self.member.clone(),
                };
                if worker.tx.send(job).is_ok() {
                    View::Pending
                } else {
                    View::Note("The file preview worker is unavailable.".into())
                }
            }
        };
    }

    pub fn show(&mut self, ui: &mut Ui, root: &Path, edition: &str, file: &File) {
        if self.reference != file.reference || self.edition != edition || self.file_bytes != file.bytes {
            self.clear();
            self.file_bytes = file.bytes;
            self.reference.clone_from(&file.reference);
            self.edition = edition.into();
            self.html_source = false;
            self.request(ui, root, file);
        }
        if self.member.is_some() && ui.link("← Archive contents").clicked() {
            self.latest.fetch_add(1, Ordering::Relaxed);
            self.member = None;
            if let Some(viewer) = &mut self.studio { viewer.clear_selection(); }
            self.pages = Default::default();
            self.html_source = false;
            self.request(ui, root, file);
        }
        let member_key = self.member.as_ref().map(|m| m.key(&file.reference));
        let reference = member_key.as_deref().unwrap_or(&file.reference);
        let (path, kind) = match &self.member {
            Some(m) => {
                ui.add(egui::Label::new(RichText::new(&m.path).strong()).wrap());
                (m.path.clone(), m.kind)
            }
            None => (file.path.clone(), file.format.kind),
        };
        if let Some(worker) = &self.worker {
            for (serial, result) in worker.rx.try_iter() {
                if serial != self.latest.load(Ordering::Relaxed) {
                    continue;
                }
                self.view = match result.map(|(identity, content)| { self.identity = Some(identity); content }) {
                    Ok(Content::Image(pixels, note, paths)) => View::Image(
                        ui.ctx().load_texture(
                            format!("delivery-{reference}"),
                            pixels,
                            egui::TextureOptions::LINEAR,
                        ),
                        note,
                        paths,
                        false,
                    ),
                    Ok(Content::Pdf(pixels, pages)) => {
                        if self.pages.count.is_none() && !matches!(kind, Kind::Pdf) {
                            ui.ctx().request_repaint();
                        }
                        self.pages.count = Some(pages);
                        View::Pdf(ui.ctx().load_texture(
                            format!("delivery-{reference}-page-{}", self.pages.page),
                            pixels,
                            pdf::SAMPLING,
                        ))
                    }
                    Ok(Content::Html(page)) => View::Html(page),
                    Ok(Content::Archive(listing)) => View::Archive(self.directory.take().unwrap_or(listing)),
                    Ok(Content::Table(rows)) => View::Table(rows, Default::default()),
                    Ok(Content::Text(body, truncated)) => View::Text(body, truncated),
                    Ok(Content::Studio(directory, path, sha256)) => {
                        let viewer = self.studio.get_or_insert_with(|| artifacts::Viewer::new(ui.ctx(), self.gl.as_ref()));
                        match viewer.open_snapshot(directory, path, sha256, None) {
                            Ok(()) => View::Studio,
                            Err(e) => View::Note(format!("Preview unavailable. {e}")),
                        }
                    }
                    Err(e) => {
                        if let Some(total) = e
                            .downcast_ref::<ai_personas::pdf_preview::RenderError>()
                            .and_then(|e| e.page_count)
                        {
                            self.pages.count = Some(total);
                        }
                        View::Note(format!("Preview unavailable. {e}"))
                    }
                };
            }
        }
        if let Some(identity) = &self.identity {
            let inner_open = matches!(self.view, View::Studio) && self.studio.as_ref().is_some_and(|v| v.inside_archive());
            identity.show(ui, inner_open);
        }
        let document = file.bytes.is_some_and(|n| n <= SOURCE_LIMIT)
            && (matches!(kind, Kind::Pdf) || self.pages.count.is_some());
        if document && self.pages.navigation(ui, reference) {
            self.request(ui, root, file);
        }
        let p = theme::palette(ui);
        let mut selected = None;
        match &mut self.view {
            View::Pending if document => self.pages.loading(ui),
            View::Pending => {
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    ui.weak("Loading exact file…");
                });
            }
            View::Pdf(texture) => self.pages.show(ui, texture, &path, reference),
            View::Html(page) => page.show(ui, reference, &mut self.html_source),
            View::Archive(listing) => selected = listing.show(ui),
            View::Table(rows, table) => {
                ui.push_id(("issued-table", reference), |ui| {
                    // Keep the inner viewport bounded inside the delivery's own
                    // scrolling page, including when this ZIP has many rows.
                    let area = egui::vec2(ui.available_width(), 310.0_f32.min(ui.ctx().content_rect().height() * 0.46));
                    ui.allocate_ui(area, |ui| table.ui(ui, rows));
                });
            }
            View::Image(texture, note, drawing, centerlines) => {
                if let Some(drawing) = drawing.as_ref() { drawing.controls(ui, centerlines, &path); }
                let available = ui.available_width();
                let height = (available * 0.72)
                    .clamp(145., 350.)
                    .min(ui.ctx().content_rect().height() * 0.48);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(available, height), egui::Sense::hover());
                ui.painter().rect_filled(rect, 5., Color32::WHITE);
                let resolution = texture.size_vec2();
                let area = rect.shrink(6.).size();
                let scale = (area.x / resolution.x).min(area.y / resolution.y).max(0.);
                let bounds = egui::Rect::from_center_size(rect.center(), resolution * scale);
                if let Some(drawing) = drawing.as_ref().filter(|_| *centerlines) {
                    drawing.paint(ui, rect);
                } else { ui.place(
                    bounds,
                    egui::Image::new((texture.id(), resolution))
                        .fit_to_exact_size(bounds.size())
                        .alt_text(format!("Preview of {path}")),
                ); }
                ui.painter().rect_stroke(
                    rect,
                    5.,
                    Stroke::new(1., p.border),
                    egui::StrokeKind::Inside,
                );
                if let Some(note) = note {
                    ui.small(note.as_str());
                }
            }
            View::Text(body, truncated) => {
                ui.small(if *truncated {
                    "Source excerpt"
                } else {
                    "File contents"
                });
                egui::Frame::new()
                    .fill(p.canvas)
                    .stroke(Stroke::new(1., p.border))
                    .corner_radius(5)
                    .inner_margin(9)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if body.is_empty() {
                            ui.weak("Empty file");
                        } else {
                            ui.add(
                                egui::Label::new(RichText::new(body.as_str()).monospace().size(12.)).wrap(),
                            );
                        }
                    });
            }
            View::Studio => {
                if let Some(viewer) = &mut self.studio {
                    let height = (ui.ctx().content_rect().height() * 0.41).clamp(155., 340.);
                    let area = egui::vec2(ui.available_width(), height);
                    ui.push_id(("delivery-studio-preview", reference), |ui| {
                        ui.allocate_ui(area, |ui| viewer.embedded_ui(ui));
                    });
                }
            }
            View::Note(text) => {
                ui.weak(text.as_str());
                if file.digest.is_some() && file.bytes.is_some_and(|n| n <= SOURCE_LIMIT)
                    && ui.small_button(if document { "Retry page" } else { "Retry preview" }).clicked()
                {
                    self.request(ui, root, file);
                }
            }
        }
        if let Some(member) = selected {
            if let View::Archive(listing) = std::mem::take(&mut self.view) {
                self.directory = Some(listing);
            }
            self.member = Some(member);
            self.pages = Default::default();
            self.html_source = false;
            self.request(ui, root, file);
        }
    }
}

fn load(job: &Job, cancelled: impl Fn() -> bool) -> ai_personas::neural::Result<(Identity, Content)> {
    let db = data::open(&job.root)?;
    let (source, bytes) = Edition::open(&db, &job.edition)?.read(&job.reference, Some(SOURCE_LIMIT))?;
    if source.path != job.file_path || source.sha256 != job.file_digest {
        return Err(ai_personas::neural::failure("The file differs from the selection on the delivery desk."));
    }
    drop(db);
    let (member, bytes) = match &job.member {
        Some(entry) => {
            let bytes = entry.contents(&bytes)?;
            (Some((entry.path.clone(), bytes.len(), ai_personas::store::sha256_bytes(&bytes))), bytes)
        }
        None => (None, bytes),
    };
    Ok((Identity { source, member }, render(job, bytes, cancelled)?))
}

fn render(job: &Job, bytes: Vec<u8>, cancelled: impl Fn() -> bool) -> ai_personas::neural::Result<Content> {
    let pdf_header = match job.kind {
        Kind::Other => ai_personas::pdf_preview::is_pdf(&bytes),
        Kind::Text | Kind::Html | Kind::Table(_) => bytes.starts_with(b"%PDF-"),
        _ => false,
    };
    let kind = if pdf_header {
        Kind::Pdf
    } else if job.member.is_some() && matches!(job.kind, Kind::Archive) {
        Kind::Other
    } else { job.kind };
    match kind {
        Kind::Pdf => {
            let max_side = job.ctx.input(|i| i.max_texture_side);
            let (image, count) = pdf::render(&bytes, job.page, max_side, cancelled)?;
            Ok(Content::Pdf(image, count))
        }
        Kind::Vector => {
            let (drawing, tree) = ai_personas::vector_preview::render_with_tree(&bytes, 1.5, None)
                .map_err(ai_personas::neural::failure)?;
            let pixmap = drawing.pixels;
            let (width, height) = (pixmap.width(), pixmap.height());
            let image = image::RgbaImage::from_raw(width, height, pixmap.take())
                .ok_or_else(|| ai_personas::neural::failure("Invalid SVG raster."))?;
            let image = sized_thumbnail(image, 1280);
            let pixels = egui::ColorImage::from_rgba_premultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            let omitted = drawing.omitted_external_images;
            let note = (omitted > 0).then(|| {
                format!(
                    "{} omitted from this standalone drawing.",
                    counted(omitted as u64, "external image", "external images")
                )
            });
            Ok(Content::Image(pixels, note, straight_paths::Drawing::inspect(&tree, &bytes)))
        }
        Kind::Raster => {
            let reader =
                || image::ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format();
            let (w, h) = reader()?.into_dimensions()?;
            if u64::from(w) * u64::from(h) > RASTER_PIXELS {
                return Err(ai_personas::neural::failure(
                    "The image dimensions exceed the inline preview size.",
                ));
            }
            let reader = reader()?;
            let note = (reader.format() == Some(image::ImageFormat::Gif))
                .then(|| "First frame of the animation.".into());
            let image = sized_thumbnail(reader.decode()?.to_rgba8(), 1280);
            Ok(Content::Image(
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                note,
                None,
            ))
        }
        Kind::Html => Ok(Content::Html(html::Preview::read(&bytes)?)),
        Kind::Archive => Ok(Content::Archive(archive::Listing::read(&bytes)?)),
        Kind::Table(tsv) => Ok(Content::Table(data_view::delimited_rows(std::str::from_utf8(&bytes)?, tsv)?)),
        Kind::Text => {
            if bytes.contains(&0) {
                return Err(ai_personas::neural::failure(
                    "The file contains binary data.",
                ));
            }
            let content = std::str::from_utf8(&bytes)?;
            let mut lines = 0;
            let mut end = content.len();
            for (count, (at, ch)) in content.char_indices().enumerate() {
                if count == TEXT_CHARS || lines == TEXT_LINES {
                    end = at;
                    break;
                }
                if ch == '\n' {
                    lines += 1;
                }
            }
            Ok(Content::Text(
                content[..end].trim_end_matches(['\n', '\r']).into(),
                end < content.len(),
            ))
        }
        Kind::Other => {
            use sha2::{Digest, Sha256};
            let directory = Arc::new(tempfile::tempdir()?);
            let name = Path::new(&job.path).file_name().unwrap_or_else(|| std::ffi::OsStr::new("file"));
            let path = directory.path().join(name);
            let sha256 = hex::encode(Sha256::digest(&bytes));
            std::fs::write(&path, bytes)?;
            Ok(Content::Studio(directory, path, sha256))
        }
    }
}

fn sized_thumbnail(image: image::RgbaImage, maximum: u32) -> image::RgbaImage {
    let (w, h) = image.dimensions();
    let longest = w.max(h);
    if longest <= maximum {
        return image;
    }
    let length = |n| ((u64::from(n) * u64::from(maximum)) / u64::from(longest)).max(1) as u32;
    image::imageops::thumbnail(&image, length(w), length(h))
}
