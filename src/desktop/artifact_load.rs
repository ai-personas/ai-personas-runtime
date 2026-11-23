//! Decode local artifacts on the file worker, never on the UI thread.
use super::*;
use std::io::{Cursor, Read, Seek};
use std::fs::File;

pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("This path is not a regular file.".into());
    }
    read_all(&mut file)
}
fn read_all(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn svg(source: &Source, bytes: &[u8], maximum_upscale: f32) -> Result<Body, String> {
    let resources = match source {
        Source::File(path) => path.parent(),
        _ => None,
    };
    let preview = ai_personas::vector_preview::render(bytes, maximum_upscale, resources)?;
    let pixmap = preview.pixels;
    let (width, height) = (pixmap.width(), pixmap.height());
    Ok(Body::Image(
        egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], pixmap.data()),
        format!("SVG preview · {width} × {height} px"),
    ))
}

pub(super) fn native_drawing(source: &Source, cancel: &native_preview::Cancellation) -> Result<Decoded, String> {
    if cancel.is_cancelled() { return Err("Preview request cancelled.".into()); }
    // Small physical drawing dimensions must not cap the readable page resolution.
    // Reuse the SVG renderer; the derived page still has a 2400-pixel longest side.
    let body = svg(source, &source.bytes(cancel)?, f32::INFINITY)?;
    if cancel.is_cancelled() { return Err("Preview request cancelled.".into()); }
    Ok(Decoded { body, note: String::new() })
}

impl Source {
    pub(super) fn name(&self) -> String {
        match self {
            Self::File(p) | Self::Snapshot { path: p, .. } => p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Self::Member { name, .. } => name.clone(),
        }
    }
    pub(super) fn label(&self) -> String {
        match self {
            Self::File(p) => p.display().to_string(),
            Self::Snapshot { .. } => format!("Recorded: {}", self.name()),
            Self::Member {
                container, name, ..
            } => format!("{} / {name}", container.label()),
        }
    }
    pub(super) fn bytes(&self, cancel: &native_preview::Cancellation) -> Result<Vec<u8>, String> {
        read_all(&mut self.open_input(cancel)?)
    }
    fn companion(&self, cancel: &native_preview::Cancellation) -> Option<Self> {
        for extension in ["stl", "glb", "obj"] {
            match self {
                Self::File(p) => {
                    let p = p.with_extension(extension);
                    if p.is_file() {
                        return Some(Self::File(p));
                    }
                }
                Self::Member {
                    container, format, name, ..
                } => {
                    let name = Path::new(name)
                        .with_extension(extension)
                        .to_string_lossy()
                        .into_owned();
                    let mut file = container.open_input(cancel).ok()?;
                    let entries = format.entries(&mut file, cancel).ok()?;
                    if let Some(entry) = entries.iter().find(|entry| entry.name == name && entry.note.is_none()) {
                        return Some(Self::Member {
                            container: container.clone(),
                            format: *format,
                            name,
                            index: entry.index,
                        });
                    }
                }
                _ => {}
            }
        }
        None
    }
}

impl ArchiveFormat {
    pub(super) fn for_name(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        if name.ends_with(".zip") { return Some(Self::Zip); }
        if [".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tbz", ".tbz2", ".tar.xz", ".txz"]
            .iter().any(|suffix| name.ends_with(suffix)) { return Some(Self::Tar); }
        ["7z","rar","iso","ar","deb","cpio","cab","gz","bz2","xz","zst","lz","lzma","lzh","xar"]
            .contains(&extension(Path::new(&name)).as_str()).then_some(Self::System)
    }

    fn entries(&self, file: &mut File, cancel: &native_preview::Cancellation) -> Result<Vec<ArchiveEntry>, String> {
        file.rewind().map_err(|e|e.to_string())?;
        let mut entries = match self {
            Self::Zip => {
                let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
                let mut entries = Vec::new();
                for index in 0..zip.len() {
                    if cancel.is_cancelled() { return Err("Preview request cancelled.".into()); }
                    let f = zip.by_index_raw(index).map_err(|e| e.to_string())?;
                    if !f.is_dir() {
                        entries.push(ArchiveEntry { index, name: f.name().into(), size: f.size(), note: None });
                    }
                }
                entries
            }
            Self::Tar | Self::System => serde_json::from_reader(native_preview::archive(file, *self, None, cancel)?)
                .map_err(|e| format!("Could not read the TAR directory: {e}"))?,
        };
        entries.sort_by(|a, b| a.name.cmp(&b.name).then(a.index.cmp(&b.index)));
        Ok(entries)
    }
}

pub(super) fn scan(root: &Path, show_hidden: bool) -> Result<Catalog, String> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    let mut folders = vec![root.clone()];
    let mut issues = 0;
    while let Some(dir) = folders.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(d) => d,
            Err(_) => {
                issues += 1;
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                issues += 1;
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !show_hidden
                && (name.starts_with('.')
                    || matches!(name.as_ref(), "__pycache__" | "node_modules"))
            {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                issues += 1;
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file() || (kind.is_symlink() && path.is_file()) {
                let Ok(meta) = std::fs::metadata(&path) else {
                    issues += 1;
                    continue;
                };
                files.push(Entry {
                    path: path.strip_prefix(&root).unwrap_or(&path).into(),
                    size: meta.len(),
                    source: Source::File(path),
                    details: None,
                    citation_message: None,
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let note = if issues > 0 {
        format!("{issues} folders or files could not be read.")
    } else {
        "Files in this runtime’s working folder; shared across its tasks.".into()
    };
    Ok(Catalog { files, note })
}

pub(super) fn recorded(source: super::super::helpers::ResultFiles) -> Result<Catalog, String> {
    use super::super::journal::{decode, digest, require};
    let directory = Arc::new(tempfile::Builder::new().prefix("personas-preview-")
        .tempdir().map_err(|e| e.to_string())?);
    let root = directory.path().join("files");
    let exported = source.export(&root)?;
    let note = "Exact delivered and cited files from replies, with source information. A citation is not a formal release or task completion.";
    if exported["status"] == "no_exportable_files" {
        return Ok(Catalog { files: vec![], note: note.into() });
    }
    require(exported["status"] == "exported" && exported["inference_performed"] == false,
        "The runtime did not return a recorded-file export.")?;
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let manifest = exported["files"].as_array().ok_or("Exported file metadata is missing.")?;
    let read_record = |path: &Path| -> Result<serde_json::Value, String> {
        let entry = manifest.iter().find(|entry| entry["path"].as_str().is_some_and(|p| Path::new(p) == path))
            .ok_or("The preview source record is missing from its export.")?;
        let bytes = read_file(path)?;
        require(entry["sha256"] == digest(&bytes) && entry["bytes"].as_u64() == Some(bytes.len() as u64),
            "The preview source record changed after export.")?;
        decode(&bytes)
    };
    let context = read_record(&root.join("export-context.json"))?;
    let mut files = Vec::new();
    for entry in manifest {
        let path = PathBuf::from(entry["path"].as_str().ok_or("Exported file path is missing.")?);
        let relative = path.strip_prefix(&root).map_err(|_| "Exported file is outside the preview folder.")?;
        require(relative.components().all(|c| matches!(c, std::path::Component::Normal(_))),
            "Exported file path is not relative to its preview folder.")?;
        let Some(content) = path.parent().filter(|p| p.file_name().is_some_and(|n| n == "content")) else {
            continue;
        };
        let group = content.parent().ok_or("The recorded file has no source group.")?;
        let is_delivery = relative.components().next().is_some_and(|part| part.as_os_str().to_string_lossy().starts_with("delivery-"));
        let source = if is_delivery {
            let receipt = read_record(&group.parent().ok_or("The delivery has no receipt folder.")?.join("receipt.json"))?;
            serde_json::json!({"delivery_receipt":receipt["receipt"],"actor":receipt["body"]["actor"],"release":receipt["body"]["release"]})
        } else {
            read_record(&group.join("source.json"))?
        };
        let history = source["kind"] == "requested_cited_history_export";
        let qualifications = read_record(&if history { path.clone() } else { group.join("qualified-bundle.json") })?;
        let sha256 = entry["sha256"].as_str().ok_or("The recorded file digest is missing.")?.to_owned();
        let size = entry["bytes"].as_u64().ok_or("The recorded file size is missing.")?;
        let label = if is_delivery { "Delivery" } else {
            source["citation_message"]["sender_label"].as_str().unwrap_or("Reply")
        }.replace('/', "∕");
        let name = path.file_name().ok_or("The recorded filename is missing.")?.to_string_lossy();
        let display = PathBuf::from(format!("{}. {label} — {name}", files.len() + 1));
        let details = serde_json::to_string_pretty(&serde_json::json!({
            "source": source, "qualifications": qualifications, "export_context": context,
            "content_sha256": sha256, "content_bytes": size,
        })).map_err(|e| e.to_string())?;
        files.push(Entry {
            path: display, size,
            source: Source::Snapshot { path, sha256, _directory: directory.clone() },
            details: Some(details),
            citation_message: source["citation_message"]["id"].as_str().map(str::to_owned),
        });
    }
    Ok(Catalog { files, note: note.into() })
}

pub(super) fn raw(
    source: &Source,
    cancel: &native_preview::Cancellation,
) -> Result<Decoded, String> {
    raw_input(source.open_input(cancel)?, String::new())
}

fn raw_input(file: File, note: String) -> Result<Decoded, String> {
    Ok(Decoded {
        body: Body::Bytes(byte_view::Page::read(Arc::new(file), 0)?),
        note,
    })
}

pub(super) fn decode(
    source: &Source,
    cancel: &native_preview::Cancellation,
) -> Result<Decoded, String> {
    let mut file = source.open_input(cancel)?;
    let name = source.name();
    let named = extension(Path::new(&name));
    let prefix = input::read_at(&file, 0, 8192)?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let format = formats::identify(&prefix, &named, &file);
    let ext = format.as_str();
    let mut reason = String::new();
    if ext == "dst" {
        if len > stitch::MAX_BYTES as u64 {
            return raw_input(file, "Design exceeds the stitch preview limit · paged bytes".into());
        }
        file.rewind().map_err(|e| e.to_string())?;
        let bytes = read_all(&mut (&mut file).take(stitch::MAX_BYTES as u64 + 1))?;
        return match stitch::Plan::read(&bytes, &name, cancel) {
            Ok(plan) => Ok(Decoded { body: Body::Stitches(plan), note: String::new() }),
            Err(reason) => raw_input(file, reason),
        };
    }
    if ext == "sqlite" {
        match database::Page::open(source, &mut file, cancel) {
            Ok(page) => {
                return Ok(Decoded {
                    body: Body::Database(page),
                    note: String::new(),
                })
            }
            Err(error) => return raw_input(file, error),
        }
    }
    if matches!(ext, "xlsx" | "xls" | "xlsb" | "xlsm" | "ods") {
        file.rewind().map_err(|e| e.to_string())?;
        if let Ok(book) = office::Book::workbook(&read_all(&mut file)?) {
            return Ok(Decoded {
                body: Body::Book(book),
                note: String::new(),
            });
        }
    }
    if ext == "wav" {
        file.rewind().map_err(|e| e.to_string())?;
        if let Ok(wave) = sound::Wave::read(std::io::BufReader::new(&mut file), cancel) {
            return Ok(Decoded {
                body: Body::Wave(wave),
                note: String::new(),
            });
        }
    }
    if let Some(route) = native_preview::route(ext) {
        match native_preview::prepare(source, &mut file, ext, route, cancel) {
            Ok(document) => {
                let note = document.summary.clone();
                return Ok(Decoded {
                    body: Body::Native(document),
                    note,
                });
            }
            Err(error) => {
                if cancel.is_cancelled() {
                    return Err("Preview cancelled.".into());
                }
                reason = error;
                if matches!(ext, "step" | "stp") {
                    if let Some(companion) = source.companion(cancel) {
                        if let Ok(mut view) = decode(&companion, cancel) {
                            view.note = format!(
                                "Companion mesh {}. STEP preview unavailable. {reason}",
                                companion.name()
                            );
                            return Ok(view);
                        }
                    }
                }
            }
        }
    }
    let archive = if ext == "zip" {
        Some(ArchiveFormat::Zip)
    } else if ext == "tar" {
        Some(ArchiveFormat::Tar)
    } else {
        ArchiveFormat::for_name(&name).or_else(|| ArchiveFormat::for_name(&format!("file.{ext}")))
    };
    if let Some(archive) = archive {
        match archive.entries(&mut file, cancel) {
            Ok(entries) => {
                return Ok(Decoded {
                    body: Body::Archive(archive, entries),
                    note: String::new(),
                })
            }
            Err(error) => reason = error,
        }
    }
    if cancel.is_cancelled() {
        return Err("Preview cancelled.".into());
    }
    let textual = formats::looks_text(&prefix);
    let known = formats::structured(ext);
    if !known && (!textual || len > 128 * 1024 * 1024) {
        if len > 128 * 1024 * 1024 && reason.is_empty() {
            reason = "Large file · paged inspection".into();
        }
        let mut decoded = raw_input(file, reason)?;
        if let Body::Bytes(page) = &mut decoded.body {
            page.prefer_text = textual;
        }
        return Ok(decoded);
    }
    file.rewind().map_err(|e| e.to_string())?;
    let bytes = read_all(&mut file)?;
    match structured(source, &bytes, ext, cancel) {
        Ok(Some(body)) => {
            return Ok(Decoded {
                body,
                note: String::new(),
            })
        }
        Err(error) => reason = error,
        Ok(None) => {}
    }
    if cancel.is_cancelled() {
        return Err("Preview cancelled.".into());
    }
    if let Some(text) = formats::text(&bytes) {
        return Ok(Decoded {
            body: Body::Text(text, matches!(ext, "md" | "markdown" | "mdown")),
            note: reason,
        });
    }
    raw_input(file, reason)
}

fn structured(
    source: &Source,
    bytes: &[u8],
    ext: &str,
    cancel: &native_preview::Cancellation,
) -> Result<Option<Body>, String> {
    let body = match ext {
        "svg" => svg(source, bytes, 2.0)?,
        "mid" | "midi" => Body::Score(sound::Score::read(bytes)?),
        "docx" | "pptx" | "odt" | "odp" => Body::Book(office::Book::document(bytes, ext)?),
        "html" | "htm" | "xhtml" => {
            let html = formats::text(bytes).ok_or("Could not decode the HTML document.")?;
            Body::Html(super::super::html_view::Document::new(html))
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "pgm"
        | "ppm" | "pbm" | "pnm" | "pam" | "qoi" | "tga" | "hdr" | "dds" | "ff" => {
            let make_reader = || {
                let mut reader = image::ImageReader::new(Cursor::new(bytes));
                if let Some(format) = image::ImageFormat::from_extension(ext) {
                    reader.set_format(format);
                }
                reader.with_guessed_format().map_err(|e| e.to_string())
            };
            let (w, h) = make_reader()?
                .into_dimensions()
                .map_err(|e| e.to_string())?;
            if u64::from(w) * u64::from(h) > 64 * 1024 * 1024 {
                return Err(format!(
                    "{w} × {h} px exceeds the raster preview memory budget."
                ));
            }
            if w.max(h) <= 4096 {
                if let Some(animated) = sequence::animated(bytes, ext, cancel)? {
                    return Ok(Some(animated));
                }
            }
            let decoded = make_reader()?.decode().map_err(|e| e.to_string())?;
            let scaled = w.max(h) > 4096;
            let display = if scaled {
                decoded.thumbnail(4096, 4096)
            } else {
                decoded
            }
            .into_rgba8();
            let info = format!(
                "{w} × {h} px{}",
                if scaled { " · Scaled preview" } else { "" }
            );
            Body::Image(
                egui::ColorImage::from_rgba_unmultiplied(
                    [display.width() as usize, display.height() as usize],
                    display.as_raw(),
                ),
                info,
            )
        }
        "stl" | "obj" | "glb" | "gltf" => Body::Mesh(super::super::mesh::load(
            bytes,
            ext,
            if let Source::File(p) = source {
                Some(p)
            } else {
                None
            },
        )?),
        "wasm" => Body::Wasm(
            ai_personas::computation::module_interface(bytes)
                .map_err(|code| format!("Could not decode the WebAssembly module ({code})."))?,
            bytes.len(),
        ),
        "csv" | "tsv" => {
            let text = formats::text(bytes).ok_or("Could not decode the table as text.")?;
            let rows = data_view::delimited_rows(&text, ext == "tsv").map_err(|e| e.to_string())?;
            Body::Table(rows, text)
        }
        "json" => {
            let text = formats::text(bytes).ok_or("Could not decode JSON as text.")?;
            let value = serde_json::from_str(&text).map_err(|e| format!("Invalid JSON. {e}"))?;
            let datasets = data_view::json_datasets(&value);
            Body::Parameters(value, text, datasets)
        }
        _ => return Ok(None),
    };
    if cancel.is_cancelled() {
        return Err("Preview cancelled.".into());
    }
    Ok(Some(body))
}
