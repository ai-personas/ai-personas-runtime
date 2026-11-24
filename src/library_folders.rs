//! Project logical filenames into folders without touching the filesystem.
//! Directory rows are derived from the supplied paths, never inferred from
//! file type. File indices remain distinct even when names coincide.
use std::collections::BTreeMap;

pub enum Entry {
    Folder { name: String, files: usize },
    File(usize),
}

/// Names for newly retained files must also work in portable delivery trees.
/// Browsing an existing archive is different: show its actual names, do not rename them.
pub(crate) fn portable_path(path: &str) -> bool {
    crate::files::canonical_relative_path(path)
        && path.split('/').all(|part| {
            !part.ends_with([' ', '.'])
                && !part.contains(['<', '>', '"', '|', '?', '*'])
                && !reserved_device(part)
        })
}

fn reserved_device(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

pub fn split(path: &str) -> (&str, &str) {
    if crate::files::canonical_relative_path(path) {
        path.rsplit_once('/').unwrap_or(("", path))
    } else {
        ("", path)
    }
}

/// `Some("")` is the root. `None` lists matching files across folders (search).
/// A non-portable archive name is a literal root entry, never a navigation instruction.
pub fn entries<'a>(
    directory: Option<&str>,
    paths: impl IntoIterator<Item = (usize, &'a str)>,
) -> Vec<Entry> {
    let mut folders = BTreeMap::new();
    let mut files = Vec::new();
    for (index, path) in paths {
        if let Some(dir) = directory {
            if crate::files::canonical_relative_path(path) {
                let tail = if dir.is_empty() {
                    path
                } else {
                    let Some(rest) = path.strip_prefix(dir).and_then(|p| p.strip_prefix('/'))
                    else {
                        continue;
                    };
                    rest
                };
                if let Some((child, _)) = tail.split_once('/') {
                    *folders.entry(child).or_insert(0) += 1;
                } else {
                    files.push((tail, index));
                }
            } else if dir.is_empty() {
                files.push((path, index));
            }
        } else {
            files.push((path, index));
        }
    }
    files.sort_unstable();
    folders
        .into_iter()
        .map(|(name, files)| Entry::Folder {
            name: name.into(),
            files,
        })
        .chain(files.into_iter().map(|(_, index)| Entry::File(index)))
        .collect()
}

pub fn child(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}
