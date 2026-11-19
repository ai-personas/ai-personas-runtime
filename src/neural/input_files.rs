//! A supplied filename and its immutable bytes travel together in operator
//! requests. Only that manifest is recorded in the activity or shown in context.
use super::{failure, store, workspace, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
}

impl Material {
    fn new(name: String, bytes: usize, digest: String, asset: Option<String>) -> Self {
        Self {
            name,
            bytes: bytes as u64,
            reference: format!("object:{digest}"),
            sha256: digest,
            asset,
        }
    }

    pub(super) fn selected(db: &Connection, name: &str, reference: &str) -> Result<Self> {
        if name.is_empty() || name.contains(['/', '\\', '\0']) || matches!(name, "." | "..") {
            return Err(failure("Supply a filename for this source"));
        }
        let digest = workspace::byte_digest(db, reference)?;
        let len = super::provider_evidence::resource_bytes(db, &digest)?.len();
        Ok(Self::new(
            name.into(),
            len,
            digest,
            reference.starts_with("asset:").then(|| reference.into()),
        ))
    }
}

#[derive(Clone, Serialize)]
pub struct CapturedFile {
    #[serde(flatten)]
    material: Material,
    #[serde(skip)]
    content: Arc<[u8]>,
}

impl fmt::Debug for CapturedFile {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.material.fmt(out)
    }
}

impl CapturedFile {
    pub fn read(path: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| failure("Supply a file with a UTF-8 filename"))?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut file = options
            .open(path)
            .map_err(|e| failure(format!("Could not open {name}: {e}")))?;
        if !file.metadata()?.is_file() {
            return Err(failure(format!("{name} is not a regular file")));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| failure(format!("Could not read {name}: {e}")))?;
        let material = Material::new(
            name.into(),
            bytes.len(),
            crate::store::sha256_bytes(&bytes),
            None,
        );
        Ok(Self {
            material,
            content: bytes.into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.material.name
    }
    pub fn len(&self) -> usize {
        self.content.len()
    }
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
    pub fn sha256(&self) -> &str {
        &self.material.sha256
    }

    fn retain(&self, db: &Connection) -> Result<Material> {
        let bytes = self.content.as_ref();
        let inserted = store::insert_bytes(db, self.sha256(), "application/octet-stream", bytes)?;
        if !inserted
            && !db.query_row(
                "SELECT bytes=?2 FROM objects WHERE digest=?1",
                params![self.sha256(), bytes],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(failure(
                "A retained attachment differs from its content address",
            ));
        }
        Ok(self.material.clone())
    }
}

#[derive(Clone, Default, Debug, clap::Args, Serialize)]
pub struct InputFiles {
    /// Include the exact contents of a local file. Repeat to supply several in order.
    #[arg(long = "attach", value_name = "FILE")]
    #[serde(skip)]
    paths: Vec<PathBuf>,
    #[arg(skip)]
    #[serde(rename = "attach")]
    captured: Vec<CapturedFile>,
}

impl From<Vec<CapturedFile>> for InputFiles {
    fn from(captured: Vec<CapturedFile>) -> Self {
        Self {
            paths: Vec::new(),
            captured,
        }
    }
}

impl InputFiles {
    pub fn is_empty(&self) -> bool {
        self.captured.is_empty() && self.paths.is_empty()
    }

    pub(super) fn capture_paths(&mut self) -> Result<()> {
        let batch = self
            .paths
            .iter()
            .map(|p| CapturedFile::read(p))
            .collect::<Result<Vec<_>>>()?;
        self.captured.extend(batch);
        self.paths.clear();
        Ok(())
    }

    pub(super) fn retain(&self, db: &Connection) -> Result<Vec<Material>> {
        self.captured.iter().map(|file| file.retain(db)).collect()
    }
}
