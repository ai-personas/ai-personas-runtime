use ai_personas::neural::{self, store};
use rusqlite::Connection;
use std::path::Path;

// Display budgets only. The ordinary viewer can open either whole revision.
const INLINE_BYTES: u64 = 1024 * 1024;
const ALIGNMENT_CELLS: usize = 2_000_000;
const LINE_DISPLAY: usize = 360;

pub(super) struct File {
    pub reference: String,
    pub path: String,
    pub bytes: u64,
    pub format: String,
    digest: String,
    binary: bool,
}

pub(super) struct Comparison {
    pub before: File,
    pub after: File,
    pub content: Content,
}

pub(super) enum Content {
    Identical,
    Summary(&'static str),
    Lines {
        rows: Vec<Line>,
        added: usize,
        removed: usize,
        shortened: bool,
    },
    Region {
        before: Vec<Line>,
        after: Vec<Line>,
        before_count: usize,
        after_count: usize,
    },
}

pub(super) struct Line {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
    pub gap: bool,
}

impl File {
    fn load(db: &Connection, reference: &str) -> neural::Result<Self> {
        let record = store::exact(db, reference)?;
        if record.reference.kind != "asset" || record.reference.handle() != reference {
            return Err(neural::failure(
                "An exact retained file revision is required.",
            ));
        }
        let path = record.body["path"].as_str().unwrap_or(reference);
        let digest = record.body["blob"]
            .as_str()
            .ok_or_else(|| neural::failure("This file has no retained byte reference."))?;
        let (media, bytes, header): (String, i64, Vec<u8>) = db.query_row(
            "SELECT media,length(bytes),substr(bytes,1,1024) FROM objects WHERE digest=?",
            [digest],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let detected = ai_personas::neural::context::image_media(&header)
            .or_else(|| ai_personas::pdf_preview::is_pdf(&header).then_some("application/pdf"));
        let media = detected.unwrap_or(&media);
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        let format = if media == "application/octet-stream" {
            if extension.is_empty() {
                "Unspecified format".into()
            } else {
                format!("{extension} file")
            }
        } else {
            media.to_owned()
        };
        let binary = detected.is_some()
            || media.starts_with("audio/")
            || media.starts_with("video/")
            || media.starts_with("image/") && !media.ends_with("+xml")
            || matches!(
                media,
                "application/zip" | "application/pdf" | "application/wasm" | "application/gzip"
            );
        Ok(Self {
            reference: reference.into(),
            path: path.into(),
            bytes: u64::try_from(bytes)?,
            format,
            digest: digest.into(),
            binary,
        })
    }
}

pub(super) fn read(
    db: &Connection,
    previous: &str,
    current: &str,
    verified_change: &str,
) -> neural::Result<Comparison> {
    let before = File::load(db, previous)?;
    let after = File::load(db, current)?;
    let content = match verified_change {
        "unchanged" if before.digest == after.digest => Content::Identical,
        "changed" if before.digest != after.digest => {
            if before.binary || after.binary {
                Content::Summary("The retained bytes changed. Open either revision to inspect the file in its native viewer.")
            } else if before.bytes.max(after.bytes) > INLINE_BYTES {
                Content::Summary("The retained bytes changed. The inline text comparison supports files up to 1 MiB each.")
            } else {
                let old = store::bytes(db, &before.digest)?;
                let new = store::bytes(db, &after.digest)?;
                match (plain_text(&old), plain_text(&new)) {
                    (Some(a), Some(b)) => compare_text(a, b),
                    _ => Content::Summary("The retained bytes changed. A line comparison requires two UTF-8 text files."),
                }
            }
        }
        _ => {
            return Err(neural::failure(
                "The byte evidence for these two revisions could not be verified.",
            ))
        }
    };
    Ok(Comparison {
        before,
        after,
        content,
    })
}

fn plain_text(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    (!text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')))
    .then_some(text)
}

fn display_line(line: &str) -> (String, bool) {
    let (line, ending) = if let Some(s) = line.strip_suffix("\r\n") {
        (s, "  [CRLF]")
    } else if let Some(s) = line.strip_suffix('\n') {
        (s, "")
    } else {
        (line, "  [no final newline]")
    };
    let mut chars = line.chars();
    let mut text: String = chars.by_ref().take(LINE_DISPLAY).collect();
    let shortened = chars.next().is_some();
    if shortened {
        text.push('…');
    }
    text.push_str(ending);
    (text, shortened)
}

fn compare_text(old: &str, new: &str) -> Content {
    // Including the terminator makes a changed line ending or final newline visible.
    let a: Vec<_> = old.split_inclusive('\n').collect();
    let b: Vec<_> = new.split_inclusive('\n').collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (x, y) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if x.len().saturating_add(y.len()) > 18_000
        || (x.len() + 1).saturating_mul(y.len() + 1) > ALIGNMENT_CELLS
    {
        // The outer common lines are proven. Show the remaining region as
        // source excerpts without describing every line in it as an edit.
        let side = |lines: &[&str], old: bool| {
            lines
                .iter()
                .take(12)
                .enumerate()
                .map(|(i, text)| Line {
                    old: old.then_some(prefix + i + 1),
                    new: (!old).then_some(prefix + i + 1),
                    text: display_line(text).0,
                    gap: false,
                })
                .collect()
        };
        return Content::Region {
            before: side(x, true),
            after: side(y, false),
            before_count: x.len(),
            after_count: y.len(),
        };
    }
    // A complete longest common subsequence of the differing region gives
    // true insert/delete counts. The matrix is bounded before allocation.
    let stride = y.len() + 1;
    let mut lengths = vec![0_u32; (x.len() + 1) * stride];
    for i in (0..x.len()).rev() {
        for j in (0..y.len()).rev() {
            lengths[i * stride + j] = if x[i] == y[j] {
                lengths[(i + 1) * stride + j + 1] + 1
            } else {
                lengths[(i + 1) * stride + j].max(lengths[i * stride + j + 1])
            };
        }
    }
    let mut indices = Vec::new();
    for i in prefix.saturating_sub(1)..prefix {
        indices.push((Some(i), Some(i)));
    }
    let (mut i, mut j, mut added, mut removed) = (0, 0, 0, 0);
    while i < x.len() || j < y.len() {
        if i < x.len() && j < y.len() && x[i] == y[j] {
            indices.push((Some(prefix + i), Some(prefix + j)));
            i += 1;
            j += 1;
        } else if i < x.len()
            && (j == y.len() || lengths[(i + 1) * stride + j] >= lengths[i * stride + j + 1])
        {
            indices.push((Some(prefix + i), None));
            i += 1;
            removed += 1;
        } else {
            indices.push((None, Some(prefix + j)));
            j += 1;
            added += 1;
        }
    }
    if suffix > 0 {
        indices.push((Some(a.len() - suffix), Some(b.len() - suffix)));
    }
    let mut rows = Vec::new();
    let mut shortened = false;
    let mut gap = false;
    for (i, &(old, new)) in indices.iter().enumerate() {
        if old.is_some()
            && new.is_some()
            && !indices[i.saturating_sub(1)..(i + 2).min(indices.len())]
                .iter()
                .any(|(x, y)| x.is_none() || y.is_none())
        {
            gap = true;
            continue;
        }
        let text = old.map(|n| a[n]).unwrap_or_else(|| b[new.unwrap()]);
        let (text, clipped) = display_line(text);
        shortened |= clipped;
        rows.push(Line {
            old: old.map(|n| n + 1),
            new: new.map(|n| n + 1),
            text,
            gap,
        });
        gap = false;
    }
    Content::Lines {
        rows,
        added,
        removed,
        shortened,
    }
}
