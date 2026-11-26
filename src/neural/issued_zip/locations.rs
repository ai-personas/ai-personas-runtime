//! Locate a chosen member in the other archive by its uncompressed fingerprint.
//! Keep the ordinary path-wise comparison intact, including a changed file at
//! the original name. A content match alone does not identify a copy or rename.
use super::{failure, Contents, Entry, Fingerprint, Member, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Baseline,
    Selected,
}

impl Side {
    pub fn other(self) -> Self {
        match self {
            Self::Baseline => Self::Selected,
            Self::Selected => Self::Baseline,
        }
    }
    fn root(self, c: &Contents) -> &str {
        match self {
            Self::Baseline => &c.baseline_root,
            Self::Selected => &c.selected_root,
        }
    }
    fn file(self, m: &Member) -> Option<&Entry> {
        match self {
            Self::Baseline => m.baseline.as_ref(),
            Self::Selected => m.selected.as_ref(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberAddress {
    /// Which of the two chosen archives contains the source member.
    pub side: Side,
    /// Full, original case-sensitive ZIP member path, including any enclosing folders.
    pub path: String,
}

#[derive(Serialize)]
pub struct Locations {
    pub source: MemberAddress,
    pub fingerprint: Fingerprint,
    pub searched: Side,
    pub searched_files: usize,
    /// Fingerprint in the other archive at the original path (null if absent).
    pub at_original_path: Option<Fingerprint>,
    pub matching: usize,
    pub offset: usize,
    pub paths: Vec<String>,
    pub next_offset: Option<usize>,
    pub scope: &'static str,
}

impl Contents {
    pub fn at_original(&self, side: Side, path: &str) -> Option<&Entry> {
        let key = path.strip_prefix(side.root(self))?;
        let i = self
            .members
            .binary_search_by(|m| m.path.as_str().cmp(key))
            .ok()?;
        side.file(&self.members[i])
    }

    /// Scan the already verified, sorted fingerprints on demand. Do not build a
    /// many-to-many list for every member when archives contain duplicate bytes.
    pub fn locate(
        &self,
        source: &MemberAddress,
        offset: usize,
        page_size: usize,
    ) -> Result<Locations> {
        if page_size == 0 {
            return Err(failure("Use a positive location page size"));
        }
        let entry = self.at_original(source.side, &source.path).ok_or_else(|| {
            failure(format!(
                "No regular member at that original path on the chosen side: {}",
                source.path
            ))
        })?;
        let fp = &entry.fingerprint;
        let other = source.side.other();
        let mut paths = Vec::new();
        let (mut searched_files, mut matching) = (0, 0);
        for m in &self.members {
            let Some(candidate) = other.file(m) else {
                continue;
            };
            searched_files += 1;
            if &candidate.fingerprint == fp {
                if matching >= offset && paths.len() < page_size {
                    paths.push(format!("{}{}", other.root(self), m.path));
                }
                matching += 1;
            }
        }
        let end = offset.saturating_add(paths.len());
        Ok(Locations {
            source: source.clone(), fingerprint: fp.clone(), searched: other, searched_files,
            at_original_path: self.at_original(other, &source.path).map(|e| e.fingerprint.clone()),
            matching, offset, paths, next_offset: (end < matching).then_some(end),
            scope: "Exact uncompressed length and SHA-256 in regular files of the other ZIP. Paths are original, not aligned. No member bodies supplied; matches do not prove copying, renaming, authorship or use.",
        })
    }
}
