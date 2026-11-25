//! Optional, ephemeral observations for local Studio windows.
//! A progress file is never part of an archive, decision, prompt or wake event.
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Serialize, Deserialize)]
pub struct ReplyDraft {
    pub text: String,
    pub truncated: bool,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Progress {
    pub stream_bytes: u64,
    pub observed_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invocation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_draft: Option<ReplyDraft>,
}

fn address(root: &Path, call: &str) -> PathBuf {
    root.join(format!(
        ".stream-{}",
        crate::store::sha256_bytes(call.as_bytes())
    ))
}

pub fn read(root: &Path, call: &str) -> Option<Progress> {
    let file = File::open(address(root, call)).ok()?;
    let mut progress: Progress = serde_json::from_reader(&file).ok()?;
    if progress.reply_draft.is_some()
        && (progress.invocation.as_deref() != Some(call)
            || !session::locked_by_owner(&file).unwrap_or(false))
    {
        progress.reply_draft = None;
    }
    Some(progress)
}

pub(super) fn remove(root: &Path, call: &str) {
    let _ = std::fs::remove_file(address(root, call));
}

mod message_receipt;
pub use message_receipt::{inspect as message_receipt, participation_terms};
pub(super) use message_receipt::operator_answer;
mod session;
pub(crate) use session::{finish_requested, request_finish, SessionOwner};
pub use session::{observe_runner, RunScope, RunnerObservation, RunnerSession};

pub(super) struct Publisher {
    path: PathBuf,
    invocation: String,
    published: Progress,
    revision: u64,
    last: Option<Instant>,
    // A reader may display a draft only while this process holds its exact inode.
    owner: Option<File>,
}

impl Publisher {
    pub fn new(root: &Path, call: &str) -> Self {
        Self {
            path: address(root, call),
            invocation: call.to_owned(),
            published: Progress::default(),
            revision: 0,
            last: None,
            owner: None,
        }
    }

    pub fn observe(&mut self, cancellation: &crate::provider::Cancellation) {
        let bytes = cancellation.stream_bytes();
        let change = cancellation.public_reply_since(self.revision);
        if change.is_none()
            && (bytes <= self.published.stream_bytes
                || self
                    .last
                    .is_some_and(|at| at.elapsed() < Duration::from_millis(400)))
        {
            return;
        }
        let (revision, draft) = match change {
            Some((revision, draft)) => {
                if draft.is_none() {
                    // Withdraw even if a replacement file cannot be written.
                    self.owner = None;
                }
                (
                    revision,
                    draft.map(|(text, truncated)| ReplyDraft { text, truncated }),
                )
            }
            None => (self.revision, self.published.reply_draft.clone()),
        };
        self.last = Some(Instant::now());
        let progress = Progress {
            stream_bytes: bytes,
            observed_at: if bytes > self.published.stream_bytes {
                super::now()
            } else {
                self.published.observed_at
            },
            invocation: draft.is_some().then(|| self.invocation.clone()),
            reply_draft: draft,
        };
        // An unavailable display must never interrupt an authorized call.
        if let Ok(mut file) = tempfile::NamedTempFile::new_in(self.path.parent().unwrap()) {
            let has_draft = progress.reply_draft.is_some();
            if has_draft
                && unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
            {
                return;
            }
            if serde_json::to_writer(&mut file, &progress).is_ok() {
                if let Ok(published) = file.persist(&self.path) {
                    self.owner = has_draft.then_some(published);
                    self.published = progress;
                    self.revision = revision;
                }
            }
        }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
