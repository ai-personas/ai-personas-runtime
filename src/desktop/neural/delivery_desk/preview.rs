//! A bounded in-page preview of a single immutable package member.
use super::*;
use ai_personas::neural::store;
use std::{io::Cursor, path::Path};

// These bounds apply to the inline preview only. The original revision stays
// available in the full file viewer and in the package.
pub(super) const SOURCE_LIMIT: u64 = 24 * 1024 * 1024;
const RASTER_PIXELS: u64 = 24_000_000;
const TEXT_CHARS: usize = 1050;
const TEXT_LINES: usize = 9;

#[derive(Clone, Copy)]
enum Kind {
    Vector,
    Raster,
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
        let mime = media.split(';').next().unwrap_or("").trim();
        let kind = match (ext.as_str(), mime) {
            ("svg", _) | (_, "image/svg+xml") => Kind::Vector,
            ("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico", _)
            | (
                _,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/tiff"
                | "image/bmp" | "image/x-icon",
            ) => Kind::Raster,
            (
                "md" | "markdown" | "txt" | "json" | "csv" | "tsv" | "yaml" | "yml" | "xml" | "net"
                | "gbr" | "drl",
                _,
            ) => Kind::Text,
            (_, mime) if mime.starts_with("text/") || mime == "application/json" => Kind::Text,
            _ => Kind::Other,
        };
        let label = match (kind, ext.as_str()) {
            (Kind::Vector, _) => "SVG drawing".into(),
            (Kind::Raster, _) => "Raster image".into(),
            (Kind::Text, "md" | "markdown") => "Markdown".into(),
            (Kind::Text, "gbr") => "Gerber source".into(),
            (Kind::Text, "drl") => "Drill source".into(),
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
        matches!(self.kind, Kind::Vector | Kind::Raster)
    }
    pub fn inline(&self) -> bool {
        !matches!(self.kind, Kind::Other)
    }
}

struct Job {
    root: PathBuf,
    reference: String,
    kind: Kind,
    ctx: egui::Context,
}
enum Content {
    Image(egui::ColorImage, Option<String>),
    Text(String, bool),
}
type Answer = (String, Result<Content, String>);

struct Worker {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Answer>,
}

impl Worker {
    fn new() -> Self {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (out, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(first) = jobs.recv() {
                let job = jobs.try_iter().last().unwrap_or(first);
                let answer = load(&job).map_err(|e| e.to_string());
                if out.send((job.reference, answer)).is_err() {
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
    Image(egui::TextureHandle, Option<String>),
    Text(String, bool),
    Note(String),
}

#[derive(Default)]
pub(super) struct Inline {
    worker: Option<Worker>,
    reference: String,
    view: View,
}

impl Inline {
    pub fn clear(&mut self) {
        self.reference.clear();
        self.view = View::Pending;
    }

    pub fn show(&mut self, ui: &mut Ui, root: &Path, file: &File) {
        if self.reference != file.reference {
            self.reference.clone_from(&file.reference);
            self.view = match file.bytes {
                None => View::Note("The file metadata is unavailable.".into()),
                Some(_) if !file.format.inline() => {
                    View::Note("Use the full file viewer for this format.".into())
                }
                Some(n) if n > SOURCE_LIMIT => {
                    View::Note("This file is too large for an inline preview.".into())
                }
                Some(_) => {
                    let worker = self.worker.get_or_insert_with(Worker::new);
                    if worker
                        .tx
                        .send(Job {
                            root: root.into(),
                            reference: file.reference.clone(),
                            kind: file.format.kind,
                            ctx: ui.ctx().clone(),
                        })
                        .is_ok()
                    {
                        View::Pending
                    } else {
                        View::Note("The file preview worker is unavailable.".into())
                    }
                }
            };
        }
        if let Some(worker) = &self.worker {
            for (reference, result) in worker.rx.try_iter() {
                if reference != self.reference {
                    continue;
                }
                self.view = match result {
                    Ok(Content::Image(pixels, note)) => View::Image(
                        ui.ctx().load_texture(
                            format!("delivery-{reference}"),
                            pixels,
                            egui::TextureOptions::LINEAR,
                        ),
                        note,
                    ),
                    Ok(Content::Text(body, truncated)) => View::Text(body, truncated),
                    Err(e) => View::Note(format!("Preview unavailable. {e}")),
                };
            }
        }
        let p = theme::palette(ui);
        match &self.view {
            View::Pending => {
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    ui.weak("Loading exact file…");
                });
            }
            View::Image(texture, note) => {
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
                ui.place(
                    bounds,
                    egui::Image::new((texture.id(), resolution))
                        .fit_to_exact_size(bounds.size())
                        .alt_text(format!("Preview of {}", file.path)),
                );
                ui.painter().rect_stroke(
                    rect,
                    5.,
                    Stroke::new(1., p.border),
                    egui::StrokeKind::Inside,
                );
                if let Some(note) = note {
                    ui.small(note);
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
                                egui::Label::new(RichText::new(body).monospace().size(12.)).wrap(),
                            );
                        }
                    });
            }
            View::Note(text) => {
                ui.weak(text);
            }
        }
    }
}

fn load(job: &Job) -> ai_personas::neural::Result<Content> {
    let db = data::open(&job.root)?;
    let asset = store::exact(&db, &job.reference)?;
    if asset.reference.kind != "asset" || asset.reference.handle() != job.reference {
        return Err(ai_personas::neural::failure(
            "The selected revision is not a retained file.",
        ));
    }
    let digest = asset.body["blob"]
        .as_str()
        .ok_or_else(|| ai_personas::neural::failure("This revision has no retained bytes."))?;
    let size: i64 = db.query_row(
        "SELECT length(bytes) FROM objects WHERE digest=?1",
        [digest],
        |r| r.get(0),
    )?;
    if u64::try_from(size)? > SOURCE_LIMIT {
        return Err(ai_personas::neural::failure(
            "This file is too large for an inline preview.",
        ));
    }
    let bytes = store::bytes(&db, digest)?;
    drop(db);
    match job.kind {
        Kind::Vector => {
            let drawing = ai_personas::vector_preview::render(&bytes, 1.5, None)
                .map_err(ai_personas::neural::failure)?;
            let pixmap = drawing.pixels;
            let (width, height) = (pixmap.width(), pixmap.height());
            let image = image::RgbaImage::from_raw(width, height, pixmap.take())
                .ok_or_else(|| ai_personas::neural::failure("Invalid SVG raster."))?;
            let image = thumbnail(image);
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
            Ok(Content::Image(pixels, note))
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
            let image = thumbnail(reader.decode()?.to_rgba8());
            Ok(Content::Image(
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                note,
            ))
        }
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
        Kind::Other => Err(ai_personas::neural::failure(
            "Use the full file viewer for this format.",
        )),
    }
}

fn thumbnail(image: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = image.dimensions();
    let longest = w.max(h);
    if longest <= 1280 {
        return image;
    }
    let length = |n| ((u64::from(n) * 1280) / u64::from(longest)).max(1) as u32;
    image::imageops::thumbnail(&image, length(w), length(h))
}
