//! Publish an executor's scope only for the lifetime of its kernel lock. A new
//! executor publishes a fresh, already locked inode, so a read never delays
//! scheduling and a crashed process cannot leave an apparently live session.
use crate::neural::{failure, id, now, store, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::File,
    io,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

pub(super) const FILE_NAME: &str = ".runner-session.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunScope {
    Workspace,
    ActivityTree {
        work: String,
    },
    Selection {
        #[serde(skip_serializing_if = "Option::is_none")]
        work: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        persona: Option<String>,
    },
}

impl RunScope {
    pub(crate) fn selected(work: Option<&str>, persona: Option<&str>, from: Option<&str>) -> Self {
        if let Some(root) = from {
            Self::ActivityTree { work: root.into() }
        } else if work.is_none() && persona.is_none() {
            Self::Workspace
        } else {
            Self::Selection {
                work: work.map(str::to_owned),
                persona: persona.map(str::to_owned),
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunnerSession {
    pub id: String,
    pub scope: RunScope,
    pub watch: bool,
    pub started_at: i64,
    pub process_id: u32,
}

#[derive(Default, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunnerObservation {
    #[default]
    Unobserved,
    Observed {
        session: RunnerSession,
        /// Current admitted work by persona, read from this executor's nonce.
        workers: BTreeMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_requested_by: Option<String>,
    },
    Unavailable {
        reason: String,
    },
}

impl RunnerObservation {
    pub fn has_worker(&self, work: &str, persona: &str) -> bool {
        matches!(self, Self::Observed { workers, .. } if workers.get(persona).is_some_and(|w| w == work))
    }
}

pub(crate) struct SessionOwner {
    pub id: String,
    path: PathBuf,
    _marker: File,
    workers_path: PathBuf,
    last_workers: BTreeMap<String, String>,
}

impl SessionOwner {
    pub fn publish(root: &Path, scope: RunScope, watch: bool) -> Result<Self> {
        let path = root.join(FILE_NAME);
        let mut temp = tempfile::NamedTempFile::new_in(root)?;
        let session = RunnerSession {
            id: id("run")?,
            scope,
            watch,
            started_at: now(),
            process_id: std::process::id(),
        };
        if unsafe { libc::flock(temp.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        serde_json::to_writer(temp.as_file_mut(), &session)?;
        let marker = temp.persist(&path)?;
        Ok(Self {
            id: session.id,
            path,
            _marker: marker,
            workers_path: root.join(".runner-workers.json"),
            last_workers: BTreeMap::new(),
        })
    }

    pub fn update_workers(&mut self, workers: &BTreeMap<String, String>) {
        if self.last_workers == *workers {
            return;
        }
        let published = (|| -> Result<()> {
            let mut temp = tempfile::NamedTempFile::new_in(self.workers_path.parent().unwrap())?;
            serde_json::to_writer(&mut temp, &(&self.id, workers))?;
            temp.persist(&self.workers_path)?;
            Ok(())
        })();
        if published.is_ok() {
            self.last_workers.clone_from(workers);
        } else {
            let _ = std::fs::remove_file(&self.workers_path);
        }
    }
}

pub(crate) fn finish_requested(db: &Connection, session_id: Option<&str>) -> Result<bool> {
    let Some(id) = session_id else {
        return Ok(false);
    };
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM heads WHERE kind='run_finish' AND id=?1)",
        [id],
        |r| r.get(0),
    )?)
}

impl Drop for SessionOwner {
    fn drop(&mut self) {
        // The primary runner.lock still belongs to this executor.
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(&self.workers_path);
    }
}

fn locked_by_owner(file: &File) -> io::Result<bool> {
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
            return Ok(false);
        }
        let err = io::Error::last_os_error();
        match err.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => return Ok(true),
            _ => return Err(err),
        }
    }
}

fn current(root: &Path) -> Result<Option<RunnerSession>> {
    let file = match File::open(root.join(FILE_NAME)) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if !locked_by_owner(&file)? {
        return Ok(None);
    }
    let session = serde_json::from_reader(&file)?;
    Ok(locked_by_owner(&file)?.then_some(session))
}

pub fn observe_runner(root: &Path, db: &Connection) -> RunnerObservation {
    let read = || -> Result<_> {
        let Some(session) = current(root)? else {
            return Ok(RunnerObservation::Unobserved);
        };
        let finish_requested_by = store::head(db, "run_finish", &session.id)?
            .and_then(|r| r.body["requested_by"].as_str().map(str::to_owned));
        let workers = std::fs::File::open(root.join(".runner-workers.json"))
            .ok()
            .and_then(|file| {
                serde_json::from_reader::<_, (String, BTreeMap<String, String>)>(file).ok()
            })
            .filter(|(id, _)| *id == session.id)
            .map(|(_, workers)| workers)
            .unwrap_or_default();
        Ok(RunnerObservation::Observed {
            session,
            workers,
            finish_requested_by,
        })
    };
    read().unwrap_or_else(|e| RunnerObservation::Unavailable {
        reason: e.to_string(),
    })
}

pub(crate) fn request_finish(
    root: &Path,
    db: &Connection,
    session_id: &str,
    operator: &str,
) -> Result<Value> {
    if let Some(existing) = store::head(db, "run_finish", session_id)? {
        return Ok(existing.body);
    }
    let session = current(root)?
        .filter(|runner| runner.id == session_id)
        .ok_or_else(|| {
            failure("This runner has finished or changed. Inspect the current runner.")
        })?;
    let request = json!({"session": session.id, "scope": session.scope, "requested_by": operator, "requested_at": now()});
    store::write(db, "run_finish", session_id, 0, &request, None)?;
    Ok(request)
}
