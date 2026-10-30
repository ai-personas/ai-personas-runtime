//! Decode local artifacts on the file worker, never on the UI thread.
use super::*;
use std::io::{Cursor, Read};

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
    let mut options = resvg::usvg::Options {
        font_family: "Inter Variable".into(),
        ..Default::default()
    };
    options.fontdb_mut().load_font_data(include_bytes!("fonts/Inter-Regular.ttf").to_vec());
    options.fontdb_mut().load_font_data(include_bytes!("fonts/DejaVuSansMono.ttf").to_vec());
    options.fontdb_mut().set_sans_serif_family("Inter Variable");
    options.fontdb_mut().set_serif_family("Inter Variable");
    options.fontdb_mut().set_monospace_family("DejaVu Sans Mono");
    if let Source::File(path) = source {
        options.resources_dir = path.parent().map(Path::to_path_buf);
    }
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    let scale = (2400.0 / size.width().max(size.height())).min(maximum_upscale);
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or("SVG preview size is invalid")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(Body::Image(
        egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], pixmap.data()),
        format!("SVG preview · {width} × {height} px"),
    ))
}

pub(super) fn native_drawing(source: &Source, cancel: &native_preview::Cancellation) -> Result<Decoded, String> {
    if cancel.is_cancelled() { return Err("Preview request cancelled.".into()); }
    // Small physical drawing dimensions must not cap the readable page resolution.
    // Reuse the SVG renderer; the derived page still has a 2400-pixel longest side.
    let body = svg(source, &source.bytes()?, f32::INFINITY)?;
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
            Self::Zip { name, .. } => name.clone(),
        }
    }
    pub(super) fn label(&self) -> String {
        match self {
            Self::File(p) => p.display().to_string(),
            Self::Snapshot { .. } => format!("Recorded: {}", self.name()),
            Self::Zip {
                container, name, ..
            } => format!("{} / {name}", container.label()),
        }
    }
    pub(super) fn bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::File(p) => read_file(p),
            Self::Snapshot { path, sha256, .. } => {
                let bytes = read_file(path)?;
                super::super::journal::require(
                    super::super::journal::digest(&bytes) == *sha256,
                    "The preview file changed after export. Refresh the recorded files.",
                )?;
                Ok(bytes)
            }
            Self::Zip {
                container,
                index,
                name,
            } => {
                let bytes = container.bytes()?;
                let mut zip =
                    zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
                let mut entry = zip.by_index(*index).map_err(|e| e.to_string())?;
                if entry.name() != name {
                    return Err("The package changed since it was opened. Return to the file list and select it again.".into());
                }
                read_all(&mut entry)
            }
        }
    }
    fn companion(&self) -> Option<Self> {
        for extension in ["stl", "glb", "obj"] {
            match self {
                Self::File(p) => {
                    let p = p.with_extension(extension);
                    if p.is_file() {
                        return Some(Self::File(p));
                    }
                }
                Self::Zip {
                    container, name, ..
                } => {
                    let name = Path::new(name)
                        .with_extension(extension)
                        .to_string_lossy()
                        .into_owned();
                    let bytes = container.bytes().ok()?;
                    let zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
                    if let Some(index) = zip.index_for_name(&name) {
                        return Some(Self::Zip {
                            container: container.clone(),
                            name,
                            index,
                        });
                    }
                }
                _ => {}
            }
        }
        None
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

pub(super) fn decode(source: &Source, cancel: &native_preview::Cancellation) -> Result<Decoded, String> {
    if cancel.is_cancelled() {
        return Err("Preview request cancelled.".into());
    }
    let name = source.name();
    let ext = extension(Path::new(&name));
    if let Some(route) = native_preview::route(&ext) {
        if let Err(reason) = route.availability() {
            return Ok(Decoded {
                body: Body::Unsupported(format!("{} can render this format inside the workspace. {reason}", route.label)),
                note: String::new(),
            });
        }
        return native_preview::prepare(source, route, cancel).map(|document| Decoded {
            body: Body::Native(document), note: String::new(),
        });
    }
    if matches!(ext.as_str(), "step" | "stp") {
        if let Some(companion) = source.companion() {
            let mut decoded = decode(&companion, cancel)?;
            decoded.note = format!(
                "STEP source · showing companion mesh {}. The STEP geometry itself is not decoded.",
                companion.name()
            );
            return Ok(decoded);
        }
        return Ok(Decoded{body:Body::Unsupported("STEP requires a CAD application. A same-name STL, OBJ, or GLB beside this file will render here.".into()),note:String::new()});
    }
    let bytes = source.bytes()?;
    let mut note = String::new();
    let body = match ext.as_str() {
        "svg" => svg(source, &bytes, 2.0)?,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" => {
            let mut reader = image::ImageReader::new(Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(|e| e.to_string())?;
            reader.no_limits();
            let image = reader.decode().map_err(|e| e.to_string())?;
            let info = format!("{} × {} px", image.width(), image.height());
            let image = if image.width().max(image.height()) > 4096 {
                note = "Preview resized to 4096 pixels on its longest side.".into();
                image.thumbnail(4096, 4096)
            } else {
                image
            };
            if ext == "gif" {
                note = "First frame of the animation.".into();
            }
            let rgba = image.to_rgba8();
            Body::Image(
                egui::ColorImage::from_rgba_unmultiplied(
                    [rgba.width() as usize, rgba.height() as usize],
                    rgba.as_raw(),
                ),
                info,
            )
        }
        "stl" | "obj" | "glb" | "gltf" => Body::Mesh(super::super::mesh::load(
            &bytes,
            &ext,
            if let Source::File(p) = source {
                Some(p.as_path())
            } else {
                None
            },
        )?),
        "zip" => {
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
            let mut entries = Vec::new();
            for index in 0..zip.len() {
                let f = zip.by_index(index).map_err(|e| e.to_string())?;
                if !f.is_dir() {
                    entries.push(ArchiveEntry {
                        index,
                        name: f.name().into(),
                        size: f.size(),
                    });
                }
            }
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            Body::Archive(entries)
        }
        "csv" | "tsv" => {
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .delimiter(if ext == "tsv" { b'\t' } else { b',' })
                .from_reader(bytes.as_slice());
            let rows = reader
                .records()
                .map(|r| r.map(|r| r.iter().map(str::to_owned).collect::<Vec<_>>()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            Body::Table(rows, std::str::from_utf8(&bytes).map_err(|e| e.to_string())?.to_owned())
        }
        "json" => {
            let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(value) => {
                    let datasets = data_view::json_datasets(&value);
                    Body::Parameters(value, text.into(), datasets)
                }
                Err(error) => {
                    note = format!("Invalid JSON: {error}. Showing the original source.");
                    Body::Text(text.into(), false)
                }
            }
        }
        "h5" | "hdf5" | "blend" | "brd" | "sch" | "gbr" | "ger" | "gerber"
        | "drl" | "dxf" | "dwg" | "iges" | "igs" | "fcstd" => Body::Unsupported(format!(
            "{ext} has no embedded decoder. Choose an installed application to view the original file. Launching it does not verify its contents.",
        )),
        _ => {
            if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                Body::Unsupported(format!(
                    "No built-in preview for {} files. Open this file in its native application.",
                    if ext.is_empty() { "this format" } else { &ext }
                ))
            } else {
                let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
                Body::Text(text.into(), matches!(ext.as_str(), "md" | "markdown"))
            }
        }
    };
    Ok(Decoded { body, note })
}
