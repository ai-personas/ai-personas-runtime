//! Capture a persona-selected tree as reviewable bytes, not an automatic issue.
//! Every file is read through an anchored descriptor. The manifest describes
//! the bytes actually read, not a simultaneous snapshot of a changing project.
use super::{directory_entries, open_relative};
use crate::neural::{failure, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    /// Named project-relative files or directories. Directories include all descendant regular files, including hidden files. No globs.
    pub paths: Vec<String>,
    /// Exact paths or subtrees to leave out. Each must occur below a selection.
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn check_path(name: &str) -> Result<()> {
    if !crate::files::canonical_relative_path(name)
        || name.split('/').any(|c| c.ends_with([' ', '.']))
    {
        return Err(failure(format!(
            "Use a canonical relative file or directory name, not {name:?}"
        )));
    }
    Ok(())
}

impl Bundle {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.paths.is_empty() {
            return Err(failure(
                "Choose files or named project directories for the ZIP",
            ));
        }
        for name in self.paths.iter().chain(&self.exclude) {
            check_path(name)?;
        }
        for name in &self.exclude {
            if !self.paths.iter().any(|p| Path::new(name).starts_with(p))
                || self.paths.iter().any(|p| Path::new(p).starts_with(name))
            {
                return Err(failure(format!(
                    "Exclude a member below a selected directory, not {name:?}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub(crate) struct Member {
    path: String,
    bytes: u64,
    sha256: String,
}

pub(crate) struct Capture {
    pub bytes: Vec<u8>,
    pub members: Vec<Member>,
}

pub(crate) fn capture(
    root: &Path,
    input: Bundle,
    mut stopped: impl FnMut() -> bool,
) -> Result<Capture> {
    input.validate()?;
    let root = File::open(root)?;
    let mut pending: BTreeSet<String> = input.paths.into_iter().collect();
    let mut exclusions: BTreeSet<String> = input.exclude.into_iter().collect();
    let mut members = Vec::new();
    let mut names = BTreeMap::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut buffer = vec![0u8; 64 * 1024];
    let mut check = || {
        if stopped() {
            Err(failure("Bundle capture interrupted by control"))
        } else {
            Ok(())
        }
    };
    while let Some(name) = pending.pop_first() {
        check()?;
        if exclusions.remove(&name) {
            continue;
        }
        check_path(&name)?;
        let mut file = open_relative(&root, Path::new(&name))
            .map_err(|e| failure(format!("Cannot capture {name:?}: {e}")))?;
        let metadata = file.metadata()?;
        if metadata.is_dir() {
            for entry in directory_entries(&file)? {
                check()?;
                let part = entry?
                    .file_name()
                    .into_string()
                    .map_err(|_| failure(format!("Non-UTF-8 member under {name:?}")))?;
                pending.insert(format!("{name}/{part}"));
            }
            continue;
        }
        if !metadata.is_file() {
            return Err(failure(format!("Not a regular file: {name:?}")));
        }
        // Independently selected leaves can have conflicting ancestor spellings.
        for ancestor in Path::new(&name)
            .ancestors()
            .filter_map(Path::to_str)
            .filter(|p| !p.is_empty())
        {
            let is_file = ancestor == name;
            let (previous, was_file) = names
                .entry(ancestor.to_lowercase())
                .or_insert_with(|| (ancestor.to_owned(), is_file));
            if previous != ancestor || *was_file != is_file {
                return Err(failure(format!(
                    "Conflicting ZIP paths {previous:?} and {ancestor:?}"
                )));
            }
        }
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(if metadata.permissions().mode() & 0o111 != 0 {
                0o755
            } else {
                0o644
            })
            .large_file(metadata.len() >= u32::MAX as u64);
        zip.start_file(&name, options)?;
        let mut hash = Sha256::new();
        let mut bytes = 0;
        loop {
            check()?;
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            zip.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            bytes += n as u64;
        }
        members.push(Member {
            path: name,
            bytes,
            sha256: hex::encode(hash.finalize()),
        });
    }
    if let Some(missing) = exclusions.first() {
        return Err(failure(format!(
            "Excluded path {missing:?} was not found in the selection"
        )));
    }
    if members.is_empty() {
        return Err(failure("The selected tree has no regular files to capture"));
    }
    let bytes = zip.finish()?.into_inner();
    check()?;
    Ok(Capture { bytes, members })
}
