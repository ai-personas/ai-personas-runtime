//! Optional, ephemeral transport observations for local Studio windows.
//! A progress file is never part of an archive, decision, prompt or wake event.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub struct Progress {
    pub stream_bytes: u64,
    pub observed_at: i64,
}

fn address(root: &Path, call: &str) -> PathBuf {
    root.join(format!(
        ".stream-{}",
        crate::store::sha256_bytes(call.as_bytes())
    ))
}

pub fn read(root: &Path, call: &str) -> Option<Progress> {
    serde_json::from_slice(&std::fs::read(address(root, call)).ok()?).ok()
}

mod message_receipt;
pub use message_receipt::inspect as message_receipt;

pub(super) struct Publisher {
    path: PathBuf,
    bytes: u64,
    last: Option<Instant>,
}

impl Publisher {
    pub fn new(root: &Path, call: &str) -> Self {
        Self {
            path: address(root, call),
            bytes: 0,
            last: None,
        }
    }

    pub fn observe(&mut self, bytes: u64) {
        if bytes <= self.bytes
            || self
                .last
                .is_some_and(|at| at.elapsed() < Duration::from_millis(400))
        {
            return;
        }
        self.last = Some(Instant::now());
        let progress = Progress {
            stream_bytes: bytes,
            observed_at: super::now(),
        };
        // An unavailable display must never interrupt an authorized call.
        if let Ok(mut file) = tempfile::NamedTempFile::new_in(self.path.parent().unwrap()) {
            if serde_json::to_writer(&mut file, &progress).is_ok()
                && file.persist(&self.path).is_ok()
            {
                self.bytes = bytes;
            }
        }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
