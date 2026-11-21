//! Run local production tools against a fresh tree of exact project files.
//! The accepted outbox operation owns dispatch. Receipts keep output bytes
//! separate from their later registration, qualification and delivery.
use super::{check_run_paths, content, store_output_bytes, Origin};
use crate::neural::{failure, now, store, workspace, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, Write},
    os::unix::{
        fs::PermissionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// Local executable name on PATH or path. Arguments are literal, not shell syntax.
    pub program: String,
    /// workspace starts in the configured shared project, where edits persist and
    /// outputs name real project files. The default scratch tree stages exact files.
    #[serde(default, skip_serializing_if = "Directory::is_scratch")]
    pub directory: Directory,
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Relative filenames in the new project directory mapped to exact asset revisions or byte objects.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Relative regular files to capture after exit, even after a nonzero exit.
    /// In workspace mode these may already exist; capture does not prove authorship.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Exact asset revision or byte object for standard input. Empty if omitted.
    pub stdin: Option<String>,
    /// Supply complete UTF-8 text from these streams to your following decision.
    /// Omitted streams provide an excerpt and an exact object address for selective reads.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub return_text: BTreeSet<Stream>,
    /// Your optional deadline, in seconds after this process starts. A timeout
    /// ends its local process group and returns available diagnostics and files.
    /// Omit for no deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Directory {
    #[default]
    Scratch,
    Workspace,
}

impl Directory {
    fn is_scratch(&self) -> bool {
        *self == Self::Scratch
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Run {
    pub(crate) fn validate(&self, db: &Connection) -> Result<()> {
        if self.program.trim().is_empty() || self.program.contains('\0') {
            return Err(failure("Name the local executable to run"));
        }
        if self.timeout_seconds == Some(0) {
            return Err(failure(
                "Choose a positive timeout_seconds or omit the deadline",
            ));
        }
        check_run_paths(&self.arguments, &self.files, &self.outputs)?;
        if self.directory == Directory::Workspace && !self.files.is_empty() {
            return Err(failure("Workspace mode operates on existing project files. Use scratch to stage a file map, or omit files to work in the shared directory."));
        }
        for reference in self.files.values().chain(&self.stdin) {
            workspace::byte_digest(db, reference)?;
        }
        Ok(())
    }
}

fn executable(program: &str, cwd: &Path) -> Result<PathBuf> {
    let accessible = |path: &Path| {
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = if program.contains('/') {
        Some(cwd.join(program)).filter(|p| accessible(p))
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| cwd.join(dir).join(program))
            .find(|p| accessible(p))
    };
    path.ok_or_else(|| {
        failure(format!(
            "Local executable {program:?} was not found or is not executable"
        ))
    })
}

fn read_stream(mut file: File) -> Result<Vec<u8>> {
    let end = file.metadata()?.len();
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(end).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != end {
        return Err(failure("Output changed while capturing the observed range"));
    }
    Ok(bytes)
}

fn stream(db: &Connection, bytes: &[u8]) -> Result<Value> {
    let mut v = store_output_bytes(db, bytes)?;
    if let Ok(text) = std::str::from_utf8(bytes) {
        let end = text.char_indices().nth(480).map_or(text.len(), |(i, _)| i);
        v["excerpt"] = json!(&text[..end]);
        v["excerpt_complete"] = json!(end == bytes.len());
    }
    Ok(v)
}

/// Render only the actor's selected streams into the *following input*. The
/// immutable execution receipt retains the intent and byte objects, not a
/// second copy of every log. This projection never dispatches the command.
pub(crate) fn project_streams(db: &Connection, result: &mut Value) -> Result<()> {
    let run = &mut result["result"];
    if run["format"] != "local-project-run-v1" {
        return Ok(());
    }
    let selected: BTreeSet<Stream> = run["intent"]
        .get("return_text")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();
    for mode in selected {
        let name = match mode {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
        };
        let observed = &mut run[name];
        let reference = observed["object"]
            .as_str()
            .and_then(|r| r.strip_prefix("object:"))
            .ok_or_else(|| failure("Missing local stream reference"))?;
        let bytes = store::bytes(db, reference)?;
        let length = bytes.len();
        if observed["bytes"].as_u64() != Some(length as u64) {
            return Err(failure(
                "Local stream length differs from the execution receipt",
            ));
        }
        match String::from_utf8(bytes) {
            Ok(text) => {
                *observed = json!({"object":observed["object"],"reference":observed["object"],
                    "bytes":length,"range":[0,length],"text":text});
            }
            Err(_) => {
                observed["text_unavailable"] = json!("Stream is not UTF-8; exact bytes retained")
            }
        }
    }
    // In the shared tree the actor needs the actual command alongside the
    // result (e.g. which relative path produced a diagnostic). Keep one copy.
    if let Some(obj) = run.as_object_mut() {
        obj.remove("intent");
        if obj["host"]["directory_mode"] != "workspace" {
            obj.remove("arguments");
        }
    }
    Ok(())
}

/// Only called by the runner after pending → dispatched. In particular there
/// is no command retry on an error, cancellation or interrupted process.
pub(crate) fn run(
    root: &Path,
    shared_directory: &Path,
    origin: Origin<'_>,
    input: Run,
    mut stop: impl FnMut() -> bool,
) -> Result<Value> {
    let mut workspace = store::Store::open(root)?;
    input.validate(&workspace.db)?;
    let scratch = input
        .directory
        .is_scratch()
        .then(|| {
            tempfile::Builder::new()
                .prefix("personas-project-")
                .tempdir()
        })
        .transpose()?;
    let project = scratch
        .as_ref()
        .map_or(shared_directory, tempfile::TempDir::path);
    let mut files = BTreeMap::new();
    for (path, reference) in &input.files {
        let (digest, bytes) = content(&workspace.db, reference)?;
        let destination = project.join(path);
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| failure("Missing project directory"))?,
        )?;
        File::create_new(destination)?.write_all(&bytes)?;
        files.insert(
            path,
            json!({"reference":reference,"object":format!("object:{digest}"),"bytes":bytes.len()}),
        );
    }
    let mut stdin = tempfile::tempfile()?;
    let stdin_source = if let Some(reference) = &input.stdin {
        let (digest, bytes) = content(&workspace.db, reference)?;
        stdin.write_all(&bytes)?;
        stdin.rewind()?;
        json!({"reference":reference,"object":format!("object:{digest}"),"bytes":bytes.len()})
    } else {
        Value::Null
    };
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let started = now();
    let mut host = json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "effective_uid":unsafe { libc::geteuid() },"working_directory":project,"directory_mode":input.directory,
        "inherited_environment_sha256":crate::host_command::environment_sha256()});
    let mut process_started = false;
    let mut cancelled = false;
    let mut timed_out = false;
    let terminal = (|| -> Result<_> {
        if !project.is_dir() {
            return Err(failure(format!(
                "Working directory is unavailable: {}",
                project.display()
            )));
        }
        let path = executable(&input.program, project)?;
        // Keep the invoked name (a multicall executable can inspect argv[0]).
        host["executable"] = json!(path);
        host["resolved_executable"] = json!(path.canonicalize()?);
        if let Ok(mut file) = File::open(&path) {
            let mut hash = Sha256::new();
            std::io::copy(&mut file, &mut hash)?;
            host["executable_sha256_before_launch"] = json!(hex::encode(hash.finalize()));
        }
        if stop() {
            cancelled = true;
            return Err(failure("Stopped before launch"));
        }
        let mut child = Command::new(path)
            .args(&input.arguments)
            .current_dir(project)
            .stdin(stdin)
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?)
            .process_group(0)
            .spawn()?;
        process_started = true;
        let launched = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                outcome => {
                    if outcome.is_ok() {
                        cancelled = stop();
                        timed_out = !cancelled
                            && input
                                .timeout_seconds
                                .is_some_and(|s| launched.elapsed().as_secs() >= s);
                        if !cancelled && !timed_out {
                            std::thread::sleep(Duration::from_millis(80));
                            continue;
                        }
                    }
                    // The child is unreaped; the process group still belongs to this launch.
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.kill();
                    let status = child.wait()?;
                    outcome?;
                    return Ok(status);
                }
            }
        }
    })();
    let (exit, signal, error) = match terminal {
        Ok(status) => (
            status.code(),
            status.signal(),
            if timed_out {
                Some(format!(
                    "The selected {} s process deadline elapsed",
                    input.timeout_seconds.unwrap_or_default()
                ))
            } else {
                cancelled.then(|| "Stopped by workspace control".to_owned())
            },
        ),
        Err(error) => (None, None, Some(error.to_string())),
    };
    let stdout = read_stream(stdout)?;
    let stderr = read_stream(stderr)?;
    let mut outputs = BTreeMap::new();
    let mut file_errors = BTreeMap::new();
    if process_started {
        for name in &input.outputs {
            match workspace::read_file(project, name) {
                Ok((_, data)) => {
                    outputs.insert(name, data);
                }
                Err(e) => {
                    file_errors.insert(name, e.to_string());
                }
            }
        }
    }
    let missing: Vec<_> = input
        .outputs
        .iter()
        .filter(|p| !outputs.contains_key(p))
        .collect();
    workspace.transaction(|db| {
        let outputs = outputs.iter().map(|(name, bytes)| Ok((name, store_output_bytes(db, bytes)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let ok = process_started && exit == Some(0) && error.is_none() && !cancelled;
        let id = format!("local-{}", crate::store::sha256_bytes(&serde_json::to_vec(&origin)?));
        let body = json!({"format":"local-project-run-v1","engine":"local","origin":"persona","actor":origin.actor,
            "work":origin.work,"invocation":origin.invocation,"operation":origin.operation,"intent":input,
            "argv0":input.program,"arguments":input.arguments,"files_supplied":files,"stdin":stdin_source,"host":host,
            "process_started":process_started,"process_succeeded":ok,"exit_code":exit,"signal":signal,"cancelled":cancelled,"timed_out":timed_out,
            "error":error,"outputs":outputs,"missing_outputs":missing,"file_errors":file_errors,
            "stdout":stream(db,&stdout)?,"stderr":stream(db,&stderr)?,"started":started,"finished":now(),
            "outputs_complete":missing.is_empty(),"success":ok && missing.is_empty()});
        let r = store::write(db, "command_run", &id, 0, &body, origin.invocation)?;
        Ok(json!({"receipt":r.handle(),"result":body}))
    })
}
