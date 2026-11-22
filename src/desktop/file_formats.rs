//! Prefer positive signatures. Only infer unmarked text formats when a filename
//! provides no more specific meaning (a JSON fixture can also be source code).
use std::{fs::File, io::Read};

pub(super) fn generic(ext: &str) -> bool {
    matches!(ext, "" | "bin" | "data" | "dat" | "blob" | "tmp")
}

pub(super) fn identify(header: &[u8], ext: &str, file: &File) -> String {
    let signature = if header.starts_with(b"PK\x03\x04") || header.starts_with(b"PK\x05\x06") {
        zip_type(file, ext)
    } else if header.starts_with(b"%PDF-")
        || (generic(ext) && ai_personas::pdf_preview::is_pdf(header))
    {
        "pdf"
    } else if header.starts_with(b"RIFF") {
        match header.get(8..12) {
            Some(b"WEBP") => "webp",
            Some(b"WAVE") => "wav",
            Some(b"AVI ") => "avi",
            _ => ext,
        }
    } else if header.get(4..8) == Some(b"ftyp") {
        match header.get(8..12) {
            Some(b"avif" | b"avis") => "avif",
            Some(b"M4A ") => "m4a",
            Some(b"heic" | b"heix" | b"mif1") => "heic",
            _ => "mp4",
        }
    } else if header.get(257..262) == Some(b"ustar") {
        "tar"
    } else if let Some((_, kind)) = [
        (&b"\x89PNG\r\n\x1a\n"[..], "png"),
        (b"\xff\xd8\xff", "jpeg"),
        (b"GIF87a", "gif"),
        (b"GIF89a", "gif"),
        (b"II*\0", "tiff"),
        (b"MM\0*", "tiff"),
        (b"OTTO", "otf"),
        (b"ttcf", "ttc"),
        (b"wOFF", "woff"),
        (b"wOF2", "woff2"),
        (b"8BPS", "psd"),
        (b"qoif", "qoi"),
        (b"DDS ", "dds"),
        (b"farbfeld", "ff"),
        (b"#?RADIANCE", "hdr"),
        (b"BM", "bmp"),
        (b"\0asm", "wasm"),
        (b"glTF", "glb"),
        (b"MThd", "mid"),
        (b"OggS", "ogg"),
        (b"fLaC", "flac"),
        (b"ID3", "mp3"),
        (b"\x1a\x45\xdf\xa3", "mkv"),
        (b"\x89HDF\r\n\x1a\n", "h5"),
        (b"{\\rtf", "rtf"),
        (b"SQLite format 3\0", "sqlite"),
        (b"7z\xbc\xaf\x27\x1c", "7z"),
        (b"Rar!\x1a\x07", "rar"),
    ]
    .iter()
    .find(|(magic, _)| header.starts_with(magic))
    {
        kind
    } else {
        let start = header
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(header)
            .trim_ascii_start();
        let lead = String::from_utf8_lossy(&start[..start.len().min(2048)]).to_ascii_lowercase();
        if lead.starts_with("<svg") || (lead.starts_with("<?xml") && lead.contains("<svg")) {
            "svg"
        } else if lead.starts_with("<!doctype html") || lead.starts_with("<html") {
            "html"
        } else if generic(ext) && (start.starts_with(b"{") || start.starts_with(b"[")) {
            "json"
        } else {
            ext
        }
    };
    signature.to_owned()
}

fn zip_type<'a>(file: &File, ext: &'a str) -> &'a str {
    if let Ok(mut zip) = zip::ZipArchive::new(file) {
        if zip.index_for_name("[Content_Types].xml").is_some() {
            for (name, kind) in [
                ("word/document.xml", "docx"),
                ("xl/workbook.xml", "xlsx"),
                ("xl/workbook.bin", "xlsb"),
                ("ppt/presentation.xml", "pptx"),
            ] {
                if zip.index_for_name(name).is_some() {
                    return kind;
                }
            }
        }
        if let Ok(member) = zip.by_name("mimetype") {
            let mut mime = String::new();
            if member.take(256).read_to_string(&mut mime).is_ok() {
                match mime.trim() {
                    "application/vnd.oasis.opendocument.text" => return "odt",
                    "application/vnd.oasis.opendocument.spreadsheet" => return "ods",
                    "application/vnd.oasis.opendocument.presentation" => return "odp",
                    _ => {}
                }
            }
        }
    }
    if matches!(
        ext,
        "fcstd" | "docx" | "pptx" | "xlsx" | "odt" | "ods" | "odp"
    ) {
        ext
    } else {
        "zip"
    }
}

pub(super) fn looks_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return true;
    }
    !bytes
        .iter()
        .any(|b| matches!(b, 0..=7 | 14..=26 | 28..=31 | 127))
        && match std::str::from_utf8(bytes) {
            Ok(_) => true,
            Err(e) => e.error_len().is_none(),
        }
}

pub(super) fn text(bytes: &[u8]) -> Option<String> {
    if let Some((rest, le)) = bytes
        .strip_prefix(&[0xff, 0xfe])
        .map(|s| (s, true))
        .or_else(|| bytes.strip_prefix(&[0xfe, 0xff]).map(|s| (s, false)))
    {
        if rest.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|b| {
                if le {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        return String::from_utf16(&units).ok();
    }
    if !looks_text(bytes) {
        return None;
    }
    std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .ok()
        .map(str::to_owned)
}

pub(super) fn structured(ext: &str) -> bool {
    matches!(
        ext,
        "svg"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "tif"
            | "tiff"
            | "ico"
            | "stl"
            | "obj"
            | "glb"
            | "gltf"
            | "wasm"
            | "json"
            | "csv"
            | "tsv"
            | "html"
            | "htm"
            | "xhtml"
            | "docx"
            | "pptx"
            | "xlsx"
            | "xls"
            | "xlsm"
            | "xlsb"
            | "xlam"
            | "odt"
            | "odp"
            | "ods"
            | "pgm"
            | "ppm"
            | "pbm"
            | "pnm"
            | "pam"
            | "tga"
            | "hdr"
            | "qoi"
            | "dds"
            | "ff"
            | "mid"
            | "midi"
            | "wav"
    )
}

/// Local pages can refer to adjacent assets. Only explicitly referenced local
/// paths within the document folder enter the printer's virtual filesystem.
pub(super) fn html_resources(
    source: &super::Source,
    html: &[u8],
) -> Result<Vec<ai_personas::document_print::Resource>, String> {
    use scraper::{Html, Selector};
    let super::Source::File(path) = source else {
        return Ok(Vec::new());
    };
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    let base = url::Url::from_file_path(&path).map_err(|_| "Invalid document path.")?;
    let doc = Html::parse_document(std::str::from_utf8(html).map_err(|e| e.to_string())?);
    let selector =
        Selector::parse("img[src], link[rel=stylesheet][href]").expect("static selector");
    let mut seen = std::collections::BTreeSet::new();
    let mut resources = Vec::new();
    let mut total = 0;
    for element in doc.select(&selector) {
        let Some(value) = element.attr("src").or_else(|| element.attr("href")) else {
            continue;
        };
        if value.starts_with('/') || url::Url::parse(value).is_ok() {
            continue;
        }
        let Some(file) = base.join(value).ok().and_then(|u| u.to_file_path().ok()) else {
            continue;
        };
        let Some(relative) = file.strip_prefix(parent).ok().and_then(|p| p.to_str()) else {
            continue;
        };
        let Ok(real) = file.canonicalize() else {
            continue;
        };
        if !real.starts_with(parent) || !real.is_file() || !seen.insert(relative.to_owned()) {
            continue;
        }
        total += real.metadata().map_err(|e| e.to_string())?.len();
        if total > 64 * 1024 * 1024 {
            return Err("The page’s linked resources exceed the print preview size.".into());
        }
        resources.push(ai_personas::document_print::Resource {
            path: relative.to_owned(),
            bytes: std::fs::read(real).map_err(|e| e.to_string())?,
        });
    }
    Ok(resources)
}
