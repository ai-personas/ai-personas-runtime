//! Align only the enclosing folder(s) explicitly chosen by the reader. Never
//! discard a file to make two trees look alike or infer renames from a digest.
use super::{failure, Difference, Entry, Member, Result};
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) type Files = BTreeMap<String, Entry>;

#[derive(Serialize)]
pub struct Contents {
    pub baseline_enclosing_folder: String,
    pub selected_enclosing_folder: String,
    pub baseline_root: String,
    pub selected_root: String,
    pub members: Vec<Member>,
    pub scope: &'static str,
}

/// Return the deepest complete folder prefix shared by every file. The full
/// archive is always selectable, including for empty or non-portable trees.
fn enclosing(files: &Files) -> &str {
    if !files
        .keys()
        .all(|p| crate::files::canonical_relative_path(p))
    {
        return "";
    }
    let (Some((first, _)), Some((last, _))) = (files.first_key_value(), files.last_key_value())
    else {
        return "";
    };
    // Sorted endpoints bound the common prefix. Slice only just after a slash,
    // never at a byte mismatch in the middle of a UTF-8 scalar.
    let length = first
        .bytes()
        .zip(last.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    first.as_bytes()[..length]
        .iter()
        .rposition(|b| *b == b'/')
        .map_or("", |i| &first[..=i])
}
fn selected_root(side: &str, root: &str, folder: &str) -> Result<()> {
    if root.is_empty() || (root.ends_with('/') && folder.starts_with(root)) {
        Ok(())
    } else {
        Err(failure(format!("Choose a complete enclosing {side} folder prefix ending in '/' (available: {folder:?}), or an empty root for every original path")))
    }
}

pub(super) fn compare(
    left: Files,
    mut right: Files,
    base: &str,
    selected: &str,
) -> Result<Contents> {
    let baseline_enclosing_folder = enclosing(&left).to_owned();
    let selected_enclosing_folder = enclosing(&right).to_owned();
    selected_root("baseline", base, &baseline_enclosing_folder)?;
    selected_root("selected", selected, &selected_enclosing_folder)?;
    let mut members = Vec::new();
    for (mut path, a) in left {
        path.drain(..base.len());
        let b = if selected.is_empty() {
            right.remove(&path)
        } else {
            right.remove(&format!("{selected}{path}"))
        };
        let difference = match &b {
            Some(b) if b.fingerprint == a.fingerprint => Difference::Same,
            Some(_) => Difference::Changed,
            None => Difference::OnlyBaseline,
        };
        members.push(Member {
            path,
            difference,
            baseline: Some(a),
            selected: b,
        });
    }
    members.extend(right.into_iter().map(|(mut path, b)| {
        path.drain(..selected.len());
        Member {
            path,
            difference: Difference::OnlySelected,
            baseline: None,
            selected: Some(b),
        }
    }));
    members.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(Contents { members, baseline_enclosing_folder, selected_enclosing_folder, baseline_root:base.into(),selected_root:selected.into(),
        scope:"All regular files, by uncompressed bytes and exact case-sensitive path relative to each chosen enclosing folder. Prepend that side's root to recover the original member path. No rename inference, nested unpacking, or comparison of folder entries, metadata, rendered appearance or design quality." })
}
