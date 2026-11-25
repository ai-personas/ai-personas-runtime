//! Read one verified ZIP as data. Member names are labels, never host paths.
use super::PreviewCancellation;
use ai_personas::neural::{failure, issued_files::{Edition, Source}, issued_zip, store, Result};
use eframe::egui::{Color32, ColorImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

pub type Bounds = [[f64; 2]; 2];
#[derive(Deserialize)]
pub struct Info {
    pub path: String,
    pub function: String,
    pub polarity: String,
    pub objects: usize,
    pub bounds: Option<Bounds>,
    pub round_hits: Vec<(f64, usize)>,
    pub warnings: Vec<String>,
}
#[derive(Deserialize)]
struct Parsed {
    renderer: String,
    bounds: Bounds,
    layers: Vec<Info>,
}
#[derive(Serialize)]
struct Input<'a> {
    path: &'a str,
    format: &'static str,
}
pub struct Plot {
    pub renderer: String,
    pub bounds: Bounds,
    pub layers: Vec<(Info, String, ColorImage)>,
}

pub fn format(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "drl" | "xln" => Some("Excellon"),
        "gbr" | "ger" | "gerber" | "gtl" | "gbl" | "gts" | "gbs" | "gto" | "gbo" | "gtp"
        | "gbp" | "gko" | "gml" | "gm1" => Some("Gerber"),
        _ if ext
            .strip_prefix('g')
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())) =>
        {
            Some("Gerber")
        }
        _ => None,
    }
}

fn active(cancel: &PreviewCancellation) -> Result<()> {
    if cancel.is_cancelled() {
        Err(failure("CAM inspection closed"))
    } else {
        Ok(())
    }
}

pub struct Archive {
    pub source: Source,
    pub paths: Vec<String>,
    bytes: Vec<u8>,
}
impl Archive {
    pub fn open(
        root: &Path,
        edition: &str,
        file: &str,
        cancel: &PreviewCancellation,
    ) -> Result<Self> {
        active(cancel)?;
        let db = store::open_inspection(root)?;
        let (source, bytes) = Edition::open(&db, edition)?.read(file, None)?;
        drop(db);
        Self::from_verified(source, bytes, cancel)
    }

    fn from_verified(
        source: Source,
        bytes: Vec<u8>,
        cancel: &PreviewCancellation,
    ) -> Result<Self> {
        let mut zip = issued_zip::directory(&bytes)?;
        let mut paths = Vec::new();
        for i in 0..zip.len() {
            active(cancel)?;
            let entry = zip.by_index_raw(i)?;
            if entry.is_file()
                && matches!(entry.unix_mode().unwrap_or(0) & 0o170000, 0 | 0o100000)
                && format(entry.name()).is_some()
            {
                paths.push(entry.name().to_owned());
            }
        }
        paths.sort();
        Ok(Self {
            source,
            bytes,
            paths,
        })
    }

    fn stage(
        &self,
        dir: &Path,
        paths: &[String],
        cancel: &PreviewCancellation,
    ) -> Result<Vec<String>> {
        let mut zip = issued_zip::directory(&self.bytes)?;
        let mut inputs = Vec::new();
        let mut hashes = Vec::new();
        let mut buffer = [0; 32 * 1024];
        for (i, path) in paths.iter().enumerate() {
            active(cancel)?;
            // Choose only exact, regular members of the verified directory.
            if self.paths.binary_search(path).is_err() {
                return Err(failure(format!("Not a recognized CAM member: {path}")));
            }
            let mut entry = zip.by_name(path)?;
            let kind =
                format(path).ok_or_else(|| failure(format!("Unknown layer format: {path}")))?;
            inputs.push(Input { path, format: kind });
            let mut staged = File::create(dir.join(format!("{i}.cam")))?;
            let mut hash = Sha256::new();
            let mut size = 0u64;
            loop {
                active(cancel)?;
                let n = entry.read(&mut buffer)?;
                if n == 0 {
                    break;
                } // reads through EOF for CRC
                size = size
                    .checked_add(n as u64)
                    .filter(|n| *n <= entry.size())
                    .ok_or_else(|| failure(format!("Unexpected member length: {path}")))?;
                hash.update(&buffer[..n]);
                staged.write_all(&buffer[..n])?;
            }
            if size != entry.size() {
                return Err(failure(format!("Incomplete member: {path}")));
            }
            hashes.push(hex::encode(hash.finalize()));
        }
        std::fs::write(dir.join("inputs.json"), serde_json::to_vec(&inputs)?)?;
        Ok(hashes)
    }

    pub fn plot(&self, paths: &[String], cancel: &PreviewCancellation) -> Result<Plot> {
        let folder = tempfile::Builder::new().prefix("studio-cam-").tempdir()?;
        let hashes = self.stage(folder.path(), paths, cancel)?;
        crate::desktop::artifacts::cam_layers(folder.path(), cancel).map_err(failure)?;
        let parsed: Parsed =
            serde_json::from_slice(&std::fs::read(folder.path().join("plots.json"))?)?;
        if paths.is_empty() || !parsed.layers.iter().map(|l| &l.path).eq(paths) {
            return Err(failure(
                "The renderer did not return every selected layer in order",
            ));
        }
        let [[l, b], [r, t]] = parsed.bounds;
        let (w, h) = (r - l, t - b);
        if !(w.is_finite() && h.is_finite() && w > 0. && h > 0.) {
            return Err(failure("No finite two-dimensional CAM extent"));
        }
        // One viewBox in mm and one transform for ALL layers. No per-file
        // fitting, bottom mirroring, or guessing offsets to make lands line up.
        let header = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><g transform="matrix(1 0 0 -1 {} {t})">"#,
            -l
        );
        // Scale display detail rather than dropping input files. Source bytes
        // and the parser's vector bounds remain independent of raster size.
        let max_side = 1400_f64.min((12_000_000. / paths.len() as f64).sqrt());
        let scale = max_side / w.max(h);
        let (pw, ph) = (
            (w * scale).round().max(1.) as u32,
            (h * scale).round().max(1.) as u32,
        );
        let mut layers = Vec::new();
        for (i, (info, hash)) in parsed.layers.into_iter().zip(hashes).enumerate() {
            active(cancel)?;
            let mut svg = header.as_bytes().to_vec();
            File::open(folder.path().join(format!("{i}.geometry")))?.read_to_end(&mut svg)?;
            svg.extend_from_slice(b"</g></svg>");
            let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
            let mut pix = resvg::tiny_skia::Pixmap::new(pw, ph)
                .ok_or_else(|| failure("Invalid CAM raster size"))?;
            let transform = resvg::tiny_skia::Transform::from_scale(
                pw as f32 / tree.size().width(),
                ph as f32 / tree.size().height(),
            );
            resvg::render(&tree, transform, &mut pix.as_mut());
            // White draws, black clears on this layer ONLY. Premultiplied red
            // is the remaining coverage even at an antialiased erased edge.
            let mask = ColorImage::new(
                [pw as usize, ph as usize],
                pix.data()
                    .chunks_exact(4)
                    .map(|p| Color32::from_white_alpha(p[0]))
                    .collect(),
            );
            layers.push((info, hash, mask));
        }
        Ok(Plot {
            renderer: parsed.renderer,
            bounds: parsed.bounds,
            layers,
        })
    }
}
