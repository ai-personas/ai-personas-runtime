//! Shared terminal/desktop journal. One writer holds the interaction lock;
//! other windows may inspect saved progress without changing or replaying it.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::SystemTime,
};

pub type Result<T> = std::result::Result<T, String>;
pub const HELPERS: [&str; 4] = [
    "ai-personas",
    "personas-auth",
    "personas-provider",
    "personas-session",
];
pub fn id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "System randomness is unavailable.".to_owned())?;
    Ok(format!("{prefix}-{}", hex::encode(bytes)))
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn absolute(s: &str) -> bool {
    Path::new(s).is_absolute() && !s.contains('\0')
}
pub fn require(ok: bool, error: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(error.into())
    }
}
pub fn io(error: std::io::Error) -> String {
    format!("Local file access failed: {error}")
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CodexPaths {
    pub executable: String,
    pub host_home: String,
    pub codex_home: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub path: String,
    pub owned: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binary {
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub file: String,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub work: String,
    pub route: Value,
    pub expected_launch: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub version: u32,
    pub operator: Option<String>,
    pub installation: Option<BTreeMap<String, Binary>>,
    pub runtime: Option<Runtime>,
    pub vault: Option<String>,
    pub needs: Vec<Value>,
    pub session: Option<Session>,
    #[serde(default)]
    pub session_history: Vec<Session>,
    pub probes: Vec<Value>,
    pub settings: Option<Document>,
    pub bootstrap: Option<Document>,
    pub setup_authorized: bool,
    pub work_input: Option<Value>,
    pub codex_auth: Option<CodexPaths>,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            version: 1,
            operator: None,
            installation: None,
            runtime: None,
            vault: None,
            needs: vec![],
            session: None,
            session_history: vec![],
            probes: vec![],
            settings: None,
            bootstrap: None,
            setup_authorized: false,
            work_input: None,
            codex_auth: None,
        }
    }
}
impl Progress {
    fn validate(&self) -> Result<()> {
        let err = "Saved progress is invalid. Choose its original folder or another new folder; retained progress was not reset.";
        require(
            self.version == 1 && (!self.setup_authorized || self.bootstrap.is_some()),
            err,
        )?;
        require(self.operator.as_deref().is_none_or(identifier), err)?;
        require(self.runtime.as_ref().is_none_or(|r| absolute(&r.path)), err)?;
        require(self.vault.as_deref().is_none_or(absolute), err)?;
        if let Some(c) = &self.codex_auth {
            require(
                [&c.executable, &c.host_home, &c.codex_home]
                    .iter()
                    .all(|p| absolute(p)),
                err,
            )?;
        }
        if let Some(bins) = &self.installation {
            require(
                bins.len() == HELPERS.len()
                    && HELPERS.iter().all(|name| {
                        bins.get(*name)
                            .is_some_and(|b| absolute(&b.path) && hash(&b.sha256))
                    }),
                err,
            )?;
        }
        for r in [&self.settings, &self.bootstrap].into_iter().flatten() {
            require(
                r.file.ends_with(".json")
                    && identifier(r.file.trim_end_matches(".json"))
                    && hash(&r.sha256),
                err,
            )?;
        }
        let mut ids = BTreeSet::new();
        for s in self.session_history.iter().chain(self.session.iter()) {
            require(
                identifier(&s.id) && identifier(&s.work) && ids.insert(&s.id),
                err,
            )?;
            let reference: ai_personas::store::ObjectRef =
                serde_json::from_value(s.expected_launch.clone()).map_err(|_| err.to_owned())?;
            require(
                reference.id == "runtime-launch-v1"
                    && reference.revision > 0
                    && hash(&reference.sha256),
                err,
            )?;
            let _: ai_personas::launch::session::SessionRoute =
                serde_json::from_value(s.route.clone()).map_err(|_| err.to_owned())?;
        }
        let mut needs = BTreeSet::new();
        for n in &self.needs {
            require(
                n["id"]
                    .as_str()
                    .is_some_and(|s| identifier(s) && needs.insert(s)),
                err,
            )?;
            require(
                n["work"].is_null() || n["work"].as_str().is_some_and(identifier),
                err,
            )?;
            if n["kind"] != "optional_activity" {
                require(
                    n["text"].as_str().is_some_and(|s| !s.trim().is_empty())
                        && n["require_acceptance"].is_boolean()
                        && n["shared"].is_boolean(),
                    err,
                )?;
            }
        }
        if let Some(input) = &self.work_input {
            require(
                input["delivered"].is_boolean()
                    && matches!(
                        input["kind"].as_str(),
                        Some("feedback" | "answer-question" | "resume-participant" | "amend-work")
                    )
                    && input["request"]["work"].as_str().is_some_and(identifier),
                err,
            )?;
        }
        Ok(())
    }
}

/// Reject duplicate object keys instead of silently replacing retained IDs.
pub fn decode(bytes: &[u8]) -> Result<Value> {
    ai_personas::files::parse_json_unique(bytes)
        .map_err(|_| "Invalid JSON; saved data was not changed.".into())
}

pub struct Journal {
    pub root: PathBuf,
    pub read_only: bool,
    directory: File,
    _lock: File,
    pub data: Progress,
}
impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(io)?;
        let root = fs::canonicalize(path).map_err(io)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&root)
            .map_err(io)?;
        let names = || -> Result<Vec<String>> {
            fs::read_dir(&root)
                .map_err(io)?
                .map(|e| {
                    e.map(|e| e.file_name().to_string_lossy().into_owned())
                        .map_err(io)
                })
                .collect()
        };
        let before = names()?;
        require(before.iter().any(|s|s=="progress.json"||s=="interaction.lock") || before.iter().all(|s|s=="personas-account.json.lock"),
            "This folder contains retained files but no progress journal. Choose the original progress folder or a new empty folder.")?;
        let lock = Self::open_at(&directory, "interaction.lock", libc::O_RDWR | libc::O_CREAT)?;
        let read_only = !Self::try_lock(&lock)?;
        Self::same_file(&directory, "interaction.lock", &lock)?;
        let mut j = Self {
            root,
            read_only,
            directory,
            _lock: lock,
            data: Progress::default(),
        };
        if let Some(raw) = j.read("progress.json")? {
            j.data = serde_json::from_value(decode(&raw)?).map_err(|_| {
                "The saved progress format is invalid. It was not reset.".to_owned()
            })?;
            j.data.validate()?;
        } else {
            require(!read_only, "The other launcher is still creating this workspace. Retry when its progress is saved.")?;
            let entries = fs::read_dir(&j.root).map_err(io)?;
            let mut account_lock = None;
            for entry in entries {
                let name = entry
                    .map_err(io)?
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                require(matches!(name.as_str(),"interaction.lock"|"personas-account.json.lock"),
                    "Progress is missing, but retained files exist. Choose the original progress folder or a new empty folder.")?;
                let f = Self::open_at(&j.directory, &name, libc::O_RDWR)?;
                require(
                    f.metadata().map_err(io)?.len() == 0,
                    "A retained lock is not empty. Progress was not reset.",
                )?;
                if name == "personas-account.json.lock" {
                    Self::lock(&f)?;
                    account_lock = Some(f);
                }
            }
            j.save()?;
            drop(account_lock);
        }
        Ok(j)
    }
    fn open_at(dir: &File, name: &str, flags: i32) -> Result<File> {
        require(
            !name.contains('/') && name != "." && name != "..",
            "Invalid journal filename.",
        )?;
        let name = CString::new(name).map_err(|_| "Invalid journal filename.".to_owned())?;
        // SAFETY: dir is live, name is NUL-terminated; returned fd is uniquely owned.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let m = file.metadata().map_err(io)?;
        require(
            m.is_file() && m.nlink() == 1,
            "Journal entries must be regular files with one link.",
        )?;
        Ok(file)
    }
    fn lock(file: &File) -> Result<()> {
        require(Self::try_lock(file)?, "This progress folder is in use by another launcher.")
    }
    fn try_lock(file: &File) -> Result<bool> {
        // SAFETY: live file; nonblocking advisory lock shared with terminal UI.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(false);
            }
            return Err(io(error));
        }
        Ok(true)
    }
    pub fn reload(&mut self) -> Result<()> {
        let raw = self.read("progress.json")?.ok_or("The workspace progress journal is missing.")?;
        let data: Progress = serde_json::from_value(decode(&raw)?)
            .map_err(|_| "The saved progress format is invalid. It was not reset.".to_owned())?;
        data.validate()?;
        require(data.runtime.as_ref().map(|r| &r.path) == self.data.runtime.as_ref().map(|r| &r.path)
            && data.operator == self.data.operator,
            "The workspace binding changed in the other launcher. Reopen it to inspect the new runtime.")?;
        self.data = data;
        Ok(())
    }
    fn same_file(dir: &File, name: &str, file: &File) -> Result<()> {
        let linked = Self::open_at(dir, name, libc::O_RDONLY)?
            .metadata()
            .map_err(io)?;
        let held = file.metadata().map_err(io)?;
        require(
            linked.dev() == held.dev() && linked.ino() == held.ino(),
            "The journal lock changed while opening it. Retry.",
        )
    }
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        // Determine absence without treating permission errors as missing state.
        match fs::symlink_metadata(self.root.join(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
            _ => {}
        }
        let mut f = Self::open_at(&self.directory, name, libc::O_RDONLY)?;
        let mut bytes = vec![];
        f.read_to_end(&mut bytes).map_err(io)?;
        Ok(Some(bytes))
    }
    pub fn write(&self, name: &str, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|_| "Could not serialize local progress.".to_owned())?;
        self.write_bytes(name, &bytes)
    }
    fn write_bytes(&self, name: &str, bytes: &[u8]) -> Result<()> {
        require(!self.read_only, "This window is viewing progress. The other launcher owns workspace controls.")?;
        require(
            !name.contains('/') && name != "." && name != "..",
            "Invalid journal filename.",
        )?;
        let temp = id("write")?;
        let mut f = Self::open_at(
            &self.directory,
            &temp,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        let temp_c = CString::new(temp.as_str()).unwrap();
        let name_c = CString::new(name).map_err(|_| "Invalid journal filename.".to_owned())?;
        let result = (|| {
            f.write_all(bytes).map_err(io)?;
            f.sync_all().map_err(io)?;
            // SAFETY: both names are relative to the held directory descriptor.
            if unsafe {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    temp_c.as_ptr(),
                    self.directory.as_raw_fd(),
                    name_c.as_ptr(),
                )
            } < 0
            {
                return Err(io(std::io::Error::last_os_error()));
            }
            self.directory.sync_all().map_err(io)
        })();
        if result.is_err() {
            unsafe { libc::unlinkat(self.directory.as_raw_fd(), temp_c.as_ptr(), 0) };
        }
        result
    }
    pub fn save(&self) -> Result<()> {
        self.data.validate()?;
        self.write("progress.json", &self.data)
    }
    pub fn keep(&mut self, kind: &str, value: &Value) -> Result<PathBuf> {
        let bytes = serde_json::to_vec(value).map_err(|_| "Invalid configuration.".to_owned())?;
        let sha256 = digest(&bytes);
        let file = format!("{kind}-{sha256}.json");
        if let Some(previous) = self.read(&file)? {
            require(previous == bytes, "Saved configuration changed.")?;
        } else {
            self.write_bytes(&file, &bytes)?;
        }
        let doc = Some(Document {
            file: file.clone(),
            sha256,
        });
        match kind {
            "settings" => self.data.settings = doc,
            "bootstrap" => self.data.bootstrap = doc,
            _ => return Err("Unknown configuration document.".into()),
        }
        self.save()?;
        Ok(self.root.join(file))
    }
    pub fn document(&self, doc: &Document) -> Result<(PathBuf, Value)> {
        let bytes = self
            .read(&doc.file)?
            .ok_or("Saved configuration is missing.")?;
        require(
            digest(&bytes) == doc.sha256,
            "Saved configuration changed. It was not replaced.",
        )?;
        Ok((self.root.join(&doc.file), decode(&bytes)?))
    }
    pub fn pending_control(&self) -> Result<Option<Value>> {
        let Some(bytes) = self.read("work-control.json")? else {
            return Ok(None);
        };
        let value = decode(&bytes)?;
        require(
            value["delivered"].is_boolean()
                && matches!(value["kind"].as_str(), Some("stop-work" | "resume-work")),
            "Invalid saved work control.",
        )?;
        Ok((value["delivered"] == false).then_some(value))
    }
}

pub fn candidates() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("PERSONAS_LAUNCHER_HOME") {
        return vec![expand(&PathBuf::from(path))];
    }
    let mut paths = vec![];
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME").filter(|p| Path::new(p).is_absolute()) {
        paths.push(PathBuf::from(xdg).join("ai-personas-launcher"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".local/state/ai-personas-launcher"));
    }
    let mut seen = BTreeSet::new();
    paths.retain(|p| seen.insert(fs::canonicalize(p).unwrap_or_else(|_| p.clone())));
    paths
}

#[derive(Clone)]
pub struct SavedWorkspace {
    pub root: PathBuf,
    pub title: String,
    pub summary: String,
    pub tasks: usize,
    pub sessions: usize,
    pub ready: bool,
    pub issue: Option<String>,
    pub updated: SystemTime,
}

/// Discovery reads journal metadata only. It neither locks a workspace nor
/// opens its runtime, credentials, or provider connection.
pub fn discover(current: &Path) -> Vec<SavedWorkspace> {
    let mut paths = candidates();
    paths.push(current.to_owned());
    let mut state_dirs = vec![];
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME").filter(|p| Path::new(p).is_absolute()) {
        state_dirs.push(PathBuf::from(xdg));
    }
    if let Some(home) = std::env::var_os("HOME") {
        state_dirs.push(PathBuf::from(home).join(".local/state"));
    }
    for parent in state_dirs {
        if let Ok(entries) = fs::read_dir(parent) {
            paths.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
    let mut seen = BTreeSet::new();
    let mut found = vec![];
    for path in paths {
        if !path.join("progress.json").is_file() {
            continue;
        }
        let root = fs::canonicalize(&path).unwrap_or(path);
        if !seen.insert(root.clone()) {
            continue;
        }
        let title = root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut item = SavedWorkspace {
            updated: fs::metadata(root.join("progress.json"))
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH),
            root,
            title,
            summary: String::new(),
            tasks: 0,
            sessions: 0,
            ready: false,
            issue: None,
        };
        let read = || -> Result<Progress> {
            let directory = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(&item.root)
                .map_err(io)?;
            let mut file = Journal::open_at(&directory, "progress.json", libc::O_RDONLY)?;
            let mut raw = vec![];
            file.read_to_end(&mut raw).map_err(io)?;
            let p: Progress = serde_json::from_value(decode(&raw)?).map_err(|_| {
                "The saved progress format is unavailable in this version.".to_owned()
            })?;
            p.validate()?;
            Ok(p)
        };
        match read() {
            Ok(p) => {
                item.tasks = p.needs.iter().filter(|n| n["work"].is_string()).count();
                item.sessions = p.session_history.len() + usize::from(p.session.is_some());
                item.summary = p
                    .needs
                    .iter()
                    .rev()
                    .find_map(|n| n["text"].as_str())
                    .unwrap_or(if p.needs.is_empty() {
                        "No tasks yet"
                    } else {
                        "Exploration and conversation"
                    })
                    .chars()
                    .take(240)
                    .collect();
                if let Some(runtime) = &p.runtime {
                    let db = Path::new(&runtime.path).join("state.sqlite3");
                    item.ready = db.is_file();
                    for path in [db, Path::new(&runtime.path).join("state.sqlite3-wal")] {
                        if let Ok(modified) = fs::metadata(path).and_then(|m| m.modified()) {
                            item.updated = item.updated.max(modified);
                        }
                    }
                }
            }
            Err(error) => {
                // Do not mistake another application's progress file for ours.
                if !item.title.contains("personas") && item.root != current {
                    continue;
                }
                item.issue = Some(error);
            }
        }
        found.push(item);
    }
    found.sort_by(|a, b| {
        b.ready
            .cmp(&a.ready)
            .then_with(|| b.updated.cmp(&a.updated))
            .then_with(|| a.root.cmp(&b.root))
    });
    found
}

pub fn expand(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}
pub fn request(kind: &str, body: Value) -> Value {
    json!({"kind":kind,"request":body,"delivered":false})
}
