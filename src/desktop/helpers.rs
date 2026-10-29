//! Immutable copies of one helper installation. Concurrent rebuilds cannot
//! replace the executable underneath an active desktop session.
use super::journal::{self, Binary, CodexPaths, Result, HELPERS};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub type Cancel = Arc<AtomicBool>;
pub fn cancel() -> Cancel {
    Arc::new(AtomicBool::new(false))
}
#[derive(Clone)]
pub struct Helpers {
    copies: Arc<tempfile::TempDir>,
    pub installation: BTreeMap<String, Binary>,
}
#[derive(Clone)]
pub struct ResultFiles {
    helpers: Helpers,
    args: Vec<String>,
    pub revision: String,
}
impl ResultFiles {
    pub fn export(&self, destination: &Path) -> Result<Value> {
        let mut args = self.args.clone();
        args.extend(["--export-directory".into(), destination.to_string_lossy().into_owned()]);
        self.helpers.call("personas-session", &args)
    }
}
impl Helpers {
    pub fn discover(
        explicit: Option<&Path>,
        saved: Option<&BTreeMap<String, Binary>>,
        inspection: Option<&[String]>,
    ) -> Result<Self> {
        let checked = |candidate: Result<Self>| -> Result<Self> {
            let helpers = candidate?;
            if let Some(args) = inspection {
                helpers.call("personas-session", args)?;
            }
            Ok(helpers)
        };
        if let Some(folder) = explicit {
            return checked(Self::from_folder(folder));
        }
        let mut errors = Vec::new();
        // Prefer the complete desktop bundle, but verify that it can inspect
        // this runtime before replacing the installation saved by its launcher.
        let exe = std::env::current_exe().map_err(journal::io)?;
        if let Some(folder) = exe
            .parent()
            .filter(|p| HELPERS.iter().all(|n| p.join(n).is_file()))
        {
            match checked(Self::from_folder(folder)) {
                Ok(helpers) => return Ok(helpers),
                Err(error) => errors.push(format!("Bundled helpers: {error}")),
            }
        }
        if let Some(mapping) = saved {
            match checked(Self::pin(mapping.clone())) {
                Ok(helpers) => return Ok(helpers),
                Err(error) => errors.push(format!("Saved helpers: {error}")),
            }
        }
        if let Some(path) = find_executable("ai-personas") {
            if let Some(folder) = path.parent() {
                match checked(Self::from_folder(folder)) {
                    Ok(helpers) => return Ok(helpers),
                    Err(error) => errors.push(format!("PATH helpers: {error}")),
                }
            }
        }
        if errors.is_empty() {
            Err("Runtime helpers were not found. Select the folder containing the four Personas helpers, or install the complete desktop bundle.".into())
        } else {
            Err(format!("No available helper installation could open this workspace. Select its matching helpers. {}", errors.join(" ")))
        }
    }
    fn from_folder(folder: &Path) -> Result<Self> {
        let mut mapping = BTreeMap::new();
        for name in HELPERS {
            let path = fs::canonicalize(folder.join(name))
                .map_err(|_| format!("The selected installation is missing {name}."))?;
            mapping.insert(
                name.into(),
                Binary {
                    path: path.to_string_lossy().into_owned(),
                    sha256: String::new(),
                },
            );
        }
        Self::pin(mapping)
    }
    fn pin(mut mapping: BTreeMap<String, Binary>) -> Result<Self> {
        let copies = tempfile::Builder::new()
            .prefix("personas-helpers-")
            .tempdir()
            .map_err(journal::io)?;
        for name in HELPERS {
            let binary = mapping
                .get_mut(name)
                .ok_or("The saved helper installation is incomplete.")?;
            let path = fs::canonicalize(&binary.path).map_err(journal::io)?;
            let mut input = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
                .open(path)
                .map_err(journal::io)?;
            let before = input.metadata().map_err(journal::io)?;
            journal::require(
                before.is_file() && before.permissions().mode() & 0o111 != 0,
                "A runtime helper is not an executable file.",
            )?;
            let target = copies.path().join(name);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(&target)
                .map_err(journal::io)?;
            let mut sha = Sha256::new();
            let mut buf = [0u8; 65536];
            loop {
                let n = input.read(&mut buf).map_err(journal::io)?;
                if n == 0 {
                    break;
                }
                sha.update(&buf[..n]);
                output.write_all(&buf[..n]).map_err(journal::io)?;
            }
            let after = input.metadata().map_err(journal::io)?;
            journal::require(
                before.len() == after.len() && before.modified().ok() == after.modified().ok(),
                "Runtime helpers changed while loading. Retry after the build finishes.",
            )?;
            let digest = hex::encode(sha.finalize());
            journal::require(
                binary.sha256.is_empty() || binary.sha256 == digest,
                "Saved runtime helpers have changed. Select the complete installation to use.",
            )?;
            binary.sha256 = digest;
            output.sync_all().map_err(journal::io)?;
            fs::set_permissions(target, fs::Permissions::from_mode(0o500)).map_err(journal::io)?;
        }
        Ok(Self {
            copies: Arc::new(copies),
            installation: mapping,
        })
    }
    pub fn call(&self, name: &str, args: &[String]) -> Result<Value> {
        self.call_events(name, args, None, |_| {})
    }
    pub fn result_files(&self, args: Vec<String>, revision: String) -> ResultFiles {
        ResultFiles { helpers: self.clone(), args, revision }
    }
    pub fn call_events(
        &self,
        name: &str,
        args: &[String],
        stop: Option<(Cancel, i32)>,
        event: impl Fn(Value) + Send + 'static,
    ) -> Result<Value> {
        journal::require(HELPERS.contains(&name), "Unknown runtime helper.")?;
        let mut child = Command::new(self.copies.path().join(name))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|_| format!("Could not start {name}. Check the selected installation."))?;
        let stdout = child.stdout.take().ok_or("Helper output is unavailable.")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Helper diagnostics are unavailable.")?;
        let out = thread::spawn(move || {
            let mut bytes = vec![];
            BufReader::new(stdout)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let err = thread::spawn(move || {
            let mut code = None;
            // Never retain or forward raw diagnostics: authentication libraries
            // can include sensitive material. Only project known event fields.
            for line in BufReader::new(stderr)
                .split(b'\n')
                .map_while(std::result::Result::ok)
            {
                if let Ok(value) = journal::decode(&line) {
                    if value["event"] == "authorization" {
                        if let Some(url) = value["url"].as_str().filter(|s| {
                            url::Url::parse(s).is_ok_and(|u| {
                                u.scheme() == "https"
                                    && u.host_str().is_some()
                                    && u.username().is_empty()
                                    && u.password().is_none()
                            })
                        }) {
                            event(
                                serde_json::json!({"event":"authorization","url":url,"user_code":value["user_code"].as_str()}),
                            );
                        }
                    }
                    if let Some(s) = value["code"]
                        .as_str()
                        .filter(|s| s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
                    {
                        code = Some(s.to_owned());
                    }
                }
            }
            code
        });
        let mut signaled = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Err(e) => {
                    break Err(journal::io(e));
                }
                Ok(None) => {}
            }
            if let Some((cancel, signal)) = &stop {
                if !signaled && cancel.load(Ordering::Acquire) {
                    // SAFETY: try_wait established the owned child is live. The
                    // signal requests graceful recording; it is never a timeout.
                    unsafe { libc::kill(child.id() as i32, *signal) };
                    signaled = true;
                }
            }
            thread::sleep(Duration::from_millis(25));
        };
        let raw = out
            .join()
            .map_err(|_| "Helper output reader failed.")?
            .map_err(journal::io)?;
        let code = err.join().map_err(|_| "Helper event reader failed.")?;
        if !status?.success() {
            return Err(if signaled {
                "The operation was interrupted. Its saved identity was retained.".into()
            } else {
                friendly(code.as_deref().unwrap_or("helper_unavailable"))
            });
        }
        let value = journal::decode(&raw)?;
        if name == "personas-session" {
            journal::require(value["version"]==1&&value["ok"]==true,"The runtime helper returned an unexpected response. Retained requests were not replayed.")?;
            Ok(value["result"].clone())
        } else if name == "personas-provider" {
            journal::require(
                value["status"] == "ok",
                "The account helper returned an unexpected response.",
            )?;
            Ok(value["data"].clone())
        } else {
            Ok(value)
        }
    }
}
fn friendly(code: &str) -> String {
    let explanation=match code {
        "credential_unavailable"|"credential_expired"=>"Sign in to the selected account, then try again.",
        "catalogue_unavailable"=>"Available models could not be read. Check your Codex installation and login, then Refresh models. Manual model entry is also available.",
        "owner_busy"=>"Another session owns this runtime. Wait for it to finish, then Refresh.",
        "blocked"=>"The runtime cannot proceed from its current state. Review the task's activity and retained session outcome.",
        "key_conflict"=>"A saved request has different contents. Its identity was retained; it was not replaced.",
        "unauthorized"=>"The operator or account does not match this workspace. Select its original settings.",
        "invalid_request"=>"Check the entered values and the selected workspace.",
        _=>"The helper could not complete the operation. Saved progress is retained; Retry inspects it again.",
    };
    format!("{explanation} ({code})")
}
pub fn find_executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| executable(p))
}
fn executable(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
pub fn codex(saved: Option<&CodexPaths>) -> Option<CodexPaths> {
    if let Some(saved) = saved {
        return Some(saved.clone());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let mut paths = vec![];
    if let Some(p) = find_executable("codex") {
        paths.push(p)
    }
    for relative in [
        ".local/bin/codex",
        ".npm-global/bin/codex",
        ".cargo/bin/codex",
        ".codex/packages/standalone/current/bin/codex",
    ] {
        paths.push(home.join(relative))
    }
    for path in ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"] {
        paths.push(path.into())
    }
    // Node version managers may not be initialized by a desktop login shell.
    if let Ok(versions) = fs::read_dir(home.join(".nvm/versions/node")) {
        let mut versions: Vec<_> = versions
            .flatten()
            .map(|v| v.path().join("bin/codex"))
            .collect();
        versions.sort();
        versions.reverse();
        paths.extend(versions);
    }
    let executable = paths
        .into_iter()
        .find(|p| executable(p))
        .and_then(|p| fs::canonicalize(p).ok())?;
    let state = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    Some(CodexPaths {
        executable: executable.to_string_lossy().into_owned(),
        host_home: home.to_string_lossy().into_owned(),
        codex_home: journal::expand(&state).to_string_lossy().into_owned(),
    })
}
pub fn codex_args(c: &CodexPaths) -> Vec<String> {
    vec![
        "--codex-executable".into(),
        c.executable.clone(),
        "--host-home".into(),
        c.host_home.clone(),
        "--codex-home".into(),
        c.codex_home.clone(),
    ]
}
