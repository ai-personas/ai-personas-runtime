//! Decode local artifacts on the file worker, never on the UI thread.
use super::*;
use std::io::{Cursor, Read};

pub(super) const MAX_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("This path is not a regular file.".into());
    }
    limited_read(&mut file)
}
fn limited_read(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(
            "This file exceeds the 128 MiB preview limit. Open it in its native application."
                .into(),
        );
    }
    Ok(bytes)
}
impl Source {
    pub(super) fn name(&self) -> String {
        match self {
            Self::File(p) => p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Self::Memory { name, .. } | Self::Zip { name, .. } => name.clone(),
        }
    }
    pub(super) fn label(&self) -> String {
        match self {
            Self::File(p) => p.display().to_string(),
            Self::Memory { name, .. } => format!("Delivered: {name}"),
            Self::Zip {
                container, name, ..
            } => format!("{} / {name}", container.label()),
        }
    }
    pub(super) fn bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::File(p) => read_file(p),
            Self::Memory { bytes, .. } => Ok(bytes.as_ref().clone()),
            Self::Zip {
                container, index, ..
            } => {
                let bytes = container.bytes()?;
                let mut zip =
                    zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
                let mut entry = zip.by_index(*index).map_err(|e| e.to_string())?;
                if entry.size() > MAX_BYTES {
                    return Err("This archive entry exceeds the 128 MiB preview limit.".into());
                }
                limited_read(&mut entry)
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

pub(super) fn scan(root: &Path) -> Result<Catalog, String> {
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
            if name.starts_with('.') || matches!(name.as_ref(), "__pycache__" | "node_modules") {
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
                let Ok(meta) = entry.metadata() else {
                    issues += 1;
                    continue;
                };
                files.push(Entry {
                    path: path.strip_prefix(&root).unwrap_or(&path).into(),
                    size: meta.len(),
                    source: Source::File(path),
                    details: None,
                });
            }
            if files.len() >= 10_000 {
                return Ok(Catalog{files,note:"Showing the first 10,000 files. Hidden files and cache folders are omitted.".into()});
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let note = if issues > 0 {
        format!("{issues} folders or files could not be read. Hidden files and cache folders are omitted.")
    } else {
        "Files in this runtime’s working folder; shared across its tasks.".into()
    };
    Ok(Catalog { files, note })
}

pub(super) fn delivered(deliveries: Vec<serde_json::Value>) -> Result<Catalog, String> {
    let mut files = Vec::new();
    for (i, d) in deliveries.iter().enumerate() {
        for (n, a) in d["body"]["artifacts"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let raw = a["qualified_bundle_json"]
                .as_str()
                .ok_or("Missing delivery qualifications")?;
            if super::super::journal::digest(raw.as_bytes())
                != a["qualified_bundle_sha256"].as_str().unwrap_or("")
            {
                return Err("Delivery qualification digest differs.".into());
            }
            let bundle: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
            let bytes: Vec<u8> =
                serde_json::from_value(a["bytes"].clone()).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err("A delivered file exceeds the 128 MiB preview limit. Use Export recorded files to inspect it.".into());
            }
            if super::super::journal::digest(&bytes)
                != a["reference"]["digest"].as_str().unwrap_or("")
            {
                return Err("Delivered file digest differs.".into());
            }
            // A preview label is resolved only from the exact source in the
            // verified qualification bundle, never from a similar filename.
            let source = &a["source"];
            let entry = bundle["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|e| {
                    let reference = &e["reference"];
                    reference["source"] == source["source"]
                        && reference["revision"] == source["revision"]
                });
            let name=entry.and_then(|e|e["path"].as_str()).ok_or("The delivered file's exact source path is unavailable. Use Export recorded files to inspect its retained record.")?.to_owned();
            let path = PathBuf::from(format!("delivery-{}/artifact-{}/{}", i + 1, n + 1, name));
            let mut details = a.clone();
            if let Some(o) = details.as_object_mut() {
                o.remove("bytes");
            }
            files.push(Entry{path,size:bytes.len() as u64,source:Source::Memory{name,bytes:Arc::new(bytes)},details:Some(serde_json::to_string_pretty(&serde_json::json!({"actor":d["body"]["actor"],"release":d["body"]["release"],"artifact":details})).unwrap_or_default())});
        }
    }
    Ok(Catalog{files,note:"Exact delivered bytes with recorded source qualifications. Export also includes eligible message citations.".into()})
}

pub(super) fn decode(source: &Source) -> Result<Decoded, String> {
    let name = source.name();
    let ext = extension(Path::new(&name));
    if matches!(ext.as_str(), "step" | "stp") {
        if let Some(companion) = source.companion() {
            let mut decoded = decode(&companion)?;
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
        "svg" => {
            let mut options = resvg::usvg::Options::default();
            options.font_family = "Inter Variable".into();
            options
                .fontdb_mut()
                .load_font_data(include_bytes!("fonts/Inter-Regular.ttf").to_vec());
            options
                .fontdb_mut()
                .load_font_data(include_bytes!("fonts/DejaVuSansMono.ttf").to_vec());
            options.fontdb_mut().set_sans_serif_family("Inter Variable");
            options.fontdb_mut().set_serif_family("Inter Variable");
            options
                .fontdb_mut()
                .set_monospace_family("DejaVu Sans Mono");
            if let Source::File(p) = source {
                options.resources_dir = p.parent().map(Path::to_path_buf);
            }
            let tree = resvg::usvg::Tree::from_data(&bytes, &options).map_err(|e| e.to_string())?;
            let size = tree.size();
            let scale = (2400.0 / size.width().max(size.height())).min(2.0);
            let w = (size.width() * scale).ceil().max(1.0) as u32;
            let h = (size.height() * scale).ceil().max(1.0) as u32;
            let mut pixmap =
                resvg::tiny_skia::Pixmap::new(w, h).ok_or("SVG preview size is invalid")?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            Body::Image(
                egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data()),
                format!("SVG · {} × {}", size.width(), size.height()),
            )
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" => {
            let mut reader = image::ImageReader::new(Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(|e| e.to_string())?;
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(256 * 1024 * 1024);
            limits.max_image_width = Some(16384);
            limits.max_image_height = Some(16384);
            reader.limits(limits);
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
            for index in 0..zip.len().min(10000) {
                let f = zip.by_index(index).map_err(|e| e.to_string())?;
                if !f.is_dir() {
                    entries.push(ArchiveEntry {
                        index,
                        name: f.name().into(),
                        size: f.size(),
                    });
                }
            }
            if zip.len() > 10000 {
                note = "Showing the first 10,000 archive entries.".into();
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
                .take(2001)
                .map(|r| r.map(|r| r.iter().map(str::to_owned).collect::<Vec<_>>()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let mut rows = rows;
            if rows.len() > 2000 {
                rows.truncate(2000);
                note = "Showing the first 2,000 rows.".into();
            }
            Body::Table(rows)
        }
        _ => {
            if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                Body::Unsupported(format!(
                    "No built-in preview for {} files. Open this file in its native application.",
                    if ext.is_empty() { "this format" } else { &ext }
                ))
            } else {
                let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
                let text = if text.len() > 512 * 1024 {
                    let mut end = 512 * 1024;
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    note = "Showing the first 512 KiB of text.".into();
                    text[..end].to_owned()
                } else if ext == "json" {
                    serde_json::from_str::<serde_json::Value>(text)
                        .ok()
                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                        .unwrap_or_else(|| text.into())
                } else {
                    text.into()
                };
                Body::Text(text, matches!(ext.as_str(), "md" | "markdown"))
            }
        }
    };
    Ok(Decoded { body, note })
}
