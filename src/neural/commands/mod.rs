//! Install portable commands and run them against an exact file tree.
//! Installations, executions and registered output assets have separate identities.
mod wasi;
pub use wasi::worker as wasi_worker;

use super::{failure, store, workspace, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncWriteExt;

const NGSPICE_URL: &str = "https://cdn.jsdelivr.net/npm/@o.z/ngspice-wasm@0.0.0/ngspice.js";
const NGSPICE_SHA: &str = "b285fc2d5b19135ed9b775ab41a6ceeb9bb75482bc6b3a44956cc6c959406b93";
const NGSPICE_WORKER: &str = include_str!("ngspice.mjs");

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, clap::ValueEnum, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Ngspice,
    Wasi,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Install {
    /// ngspice installs the published simulator. wasi installs a retained Preview 1 command.
    #[arg(value_enum)]
    pub engine: Engine,
    /// Exact asset or object. Required for WASI. Optional offline copy for ngspice.
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// Exact installation revision from command.install or command.history.
    pub installation: String,
    /// Arguments after argv[0], each passed literally.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Relative project paths mapped to exact asset revisions or byte objects.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Relative regular files to retain after the process finishes.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Optional exact asset or object for standard input. Otherwise the input is empty.
    pub stdin: Option<String>,
}

fn relative(path: &str) -> Result<()> {
    if path.contains(['\0', '\\']) || path.split('/').any(|c| matches!(c, "" | "." | "..")) {
        return Err(failure(format!(
            "Choose a relative project file path: {path:?}"
        )));
    }
    Ok(())
}

fn check_run_paths(run: &Run) -> Result<()> {
    let mut files = BTreeSet::new();
    for name in run.files.keys().chain(&run.outputs) {
        relative(name)?;
        files.insert(Path::new(name));
    }
    for path in &files {
        if path
            .ancestors()
            .skip(1)
            .any(|parent| files.contains(parent))
        {
            return Err(failure("A project file cannot also be a directory"));
        }
    }
    if run.arguments.iter().any(|s| s.contains('\0')) {
        return Err(failure("Command arguments cannot contain null bytes"));
    }
    Ok(())
}

pub(super) fn validate_install(db: &Connection, input: &Install) -> Result<String> {
    match (input.engine, input.source.as_deref()) {
        (Engine::Ngspice, None) => Ok(NGSPICE_SHA.to_owned()),
        (Engine::Wasi, None) => Err(failure("Select a retained WASI Preview 1 command module")),
        (_, Some(reference)) => {
            let digest = workspace::byte_digest(db, reference)?;
            if input.engine == Engine::Ngspice && digest != NGSPICE_SHA {
                return Err(failure(
                    "The selected bytes differ from the published ngspice module",
                ));
            }
            Ok(digest)
        }
    }
}

fn installation(db: &Connection, reference: &str) -> Result<(store::Record, Engine, String)> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != "installation" || record.body["format"] != "portable-command-v1" {
        return Err(failure(
            "Choose an exact portable command installation revision",
        ));
    }
    let engine: Engine = serde_json::from_value(record.body["engine"].clone())?;
    let digest = record.body["program"]
        .as_str()
        .and_then(|s| s.strip_prefix("object:"))
        .ok_or_else(|| failure("Installation has no recorded program"))?
        .to_owned();
    if engine == Engine::Ngspice && digest != NGSPICE_SHA {
        return Err(failure(
            "The installation differs from the pinned ngspice program",
        ));
    }
    Ok((record, engine, digest))
}

pub(super) fn validate_run(db: &Connection, run: &Run) -> Result<()> {
    check_run_paths(run)?;
    installation(db, &run.installation)?;
    for reference in run.files.values().chain(&run.stdin) {
        workspace::byte_digest(db, reference)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Invocation {
    program: String,
    name: String,
    arguments: Vec<String>,
    files: BTreeMap<String, String>,
    outputs: Vec<String>,
    stdin: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Outcome {
    exit_code: Option<i32>,
    error: Option<String>,
    stdout: String,
    stderr: String,
    files: BTreeMap<String, String>,
    file_errors: BTreeMap<String, String>,
    host: Value,
}

async fn execute(
    engine: Engine,
    payload: &Invocation,
    stopped: &mut impl FnMut() -> bool,
) -> Result<Outcome> {
    // The parent owns the scratch directory through cancellation. Both workers
    // consume their input over pipes and report only requested output bytes.
    let scratch = tempfile::tempdir()?;
    let executable = match engine {
        Engine::Ngspice => workspace::node_executable()?,
        #[cfg(target_os = "linux")]
        Engine::Wasi => PathBuf::from("/proc/self/exe"),
        #[cfg(not(target_os = "linux"))]
        Engine::Wasi => std::env::current_exe()?,
    };
    let host_path = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.clone());
    let mut command = tokio::process::Command::new(&executable);
    match engine {
        Engine::Ngspice => {
            command.args([
                "--permission",
                "--no-warnings",
                "--experimental-vm-modules",
                "--input-type=module",
                "-e",
                NGSPICE_WORKER,
            ]);
        }
        Engine::Wasi => {
            command.arg("wasi-worker");
        }
    }
    let input = serde_json::to_vec(payload)?;
    let mut result: Outcome = workspace::controlled(
        async {
            let mut child = command
                .env_clear()
                .current_dir(scratch.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| failure("Missing command worker input"))?;
            let writer = async move { stdin.write_all(&input).await };
            let (sent, output) = tokio::join!(writer, child.wait_with_output());
            let output = output?;
            if !output.status.success() {
                return Err(failure(format!(
                    "Command worker exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            sent?;
            Ok(serde_json::from_slice(&output.stdout)?)
        },
        stopped,
    )
    .await?;
    result.host["executable"] = json!(host_path);
    if engine == Engine::Ngspice {
        result.host["worker_sha256"] = json!(crate::store::sha256_bytes(NGSPICE_WORKER.as_bytes()));
    }
    Ok(result)
}

pub async fn install(
    root: &Path,
    actor: &str,
    call: Option<&str>,
    input: &Install,
    operator_file: Option<&Path>,
    mut stopped: impl FnMut() -> bool,
) -> Result<Value> {
    let mut db = store::Store::open(root)?;
    let local = operator_file
        .map(|path| -> Result<Vec<u8>> {
            use std::{io::Read, os::unix::fs::OpenOptionsExt};
            if call.is_some() || input.source.is_some() {
                return Err(failure("Choose one operator file or one retained source"));
            }
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)?;
            if !file.metadata()?.is_file() {
                return Err(failure("Choose a regular program file"));
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .transpose()?;
    let digest = match &local {
        Some(bytes) => {
            let digest = crate::store::sha256_bytes(bytes);
            if input.engine == Engine::Ngspice && digest != NGSPICE_SHA {
                return Err(failure(
                    "The file differs from the published ngspice package",
                ));
            }
            digest
        }
        None => validate_install(&db.db, input)?,
    };
    let id = match input.engine {
        Engine::Ngspice => "ngspice-wasm-0.0.0".to_owned(),
        Engine::Wasi => format!("wasi-{digest}"),
    };
    let existing = |c: &Connection| -> Result<Option<Value>> {
        let Some(rec) = store::head(c, "installation", &id)? else {
            return Ok(None);
        };
        let (record, engine, stored) = installation(c, &rec.reference.handle())?;
        if stored != digest || engine != input.engine {
            return Err(failure("Conflicting retained installation"));
        }
        store::bytes(c, &stored)?;
        Ok(Some(
            json!({"installation":record.reference.handle(),"record":record.body,"already_installed":true}),
        ))
    };
    if let Some(result) = existing(&db.db)? {
        return Ok(result);
    }
    let local_source = format!("object:{digest}");
    let (bytes, source) = if let Some(bytes) = local {
        (bytes, local_source.as_str())
    } else if let Some(reference) = &input.source {
        (store::bytes(&db.db, &digest)?, reference.as_str())
    } else {
        let bytes = workspace::controlled(
            async {
                Ok(reqwest::get(NGSPICE_URL)
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?
                    .to_vec())
            },
            &mut stopped,
        )
        .await?;
        (bytes, NGSPICE_URL)
    };
    if crate::store::sha256_bytes(&bytes) != digest {
        return Err(failure("Command package digest mismatch"));
    }
    let name = if input.engine == Engine::Wasi {
        input
            .source
            .as_deref()
            .filter(|r| r.starts_with("asset:"))
            .map(|r| store::exact(&db.db, r))
            .transpose()?
            .and_then(|r| {
                r.body["path"]
                    .as_str()
                    .and_then(|s| Path::new(s).file_name())
                    .and_then(|s| s.to_str())
                    .map(str::to_owned)
            })
            .or_else(|| {
                operator_file
                    .and_then(|p| p.file_name()?.to_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "command.wasm".into())
    } else {
        "ngspice".into()
    };
    let probe = match input.engine {
        Engine::Wasi => wasi::validate(&bytes)?,
        Engine::Ngspice => {
            let report = execute(
                input.engine,
                &Invocation {
                    program: STANDARD.encode(&bytes),
                    name: name.clone(),
                    arguments: vec!["-v".into()],
                    files: BTreeMap::new(),
                    outputs: vec![],
                    stdin: String::new(),
                },
                &mut stopped,
            )
            .await?;
            let stdout = String::from_utf8(STANDARD.decode(report.stdout)?)?;
            let version = stdout.lines().find(|line| line.contains("ngspice-"));
            if report.exit_code != Some(0) || report.error.is_some() || version.is_none() {
                return Err(failure(format!(
                    "ngspice version probe failed: {:?} {}",
                    report.error, stdout
                )));
            }
            json!({"kind":"executed_version","arguments":["-v"],"exit_code":report.exit_code,"version":version,"stdout":stdout,
                "stderr":String::from_utf8_lossy(&STANDARD.decode(report.stderr)?),"host":report.host})
        }
    };
    db.transaction(|c|{
        if let Some(result)=existing(c)? {return Ok(result);}
        let program=store::put_bytes(c,if input.engine==Engine::Ngspice{"text/javascript"}else if bytes.starts_with(b"\0asm"){"application/wasm"}else{"text/plain"},&bytes)?;
        let body=json!({"format":"portable-command-v1","engine":input.engine,"name":name,"program":format!("object:{program}"),
            "bytes":bytes.len(),"source":source,"installed_by":actor,"origin":if call.is_some(){"persona"}else{"operator"},
            "invocation":call,"operator_file":operator_file,"installed_at":super::now(),"probe":probe});
        let reference=store::write(c,"installation",&id,0,&body,call)?;
        Ok(json!({"installation":reference.handle(),"record":body,"already_installed":false}))
    })
}

#[derive(Clone, Serialize)]
pub struct Origin<'a> {
    pub actor: &'a str,
    pub work: Option<&'a str>,
    pub invocation: Option<&'a str>,
    /// The persona operation ID or an explicit operator request ID.
    pub operation: Option<&'a str>,
}

fn content(db: &Connection, reference: &str) -> Result<(String, Vec<u8>)> {
    let digest = workspace::byte_digest(db, reference)?;
    let bytes = store::bytes(db, &digest)?;
    Ok((digest, bytes))
}

fn stored_output(db: &Connection, encoded: &str) -> Result<Value> {
    let bytes = STANDARD.decode(encoded)?;
    let media = if std::str::from_utf8(&bytes).is_ok() {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    };
    let digest = store::put_bytes(db, media, &bytes)?;
    Ok(json!({"object":format!("object:{digest}"),"bytes":bytes.len()}))
}

pub async fn run(
    root: &Path,
    origin: Origin<'_>,
    input: &Run,
    mut stopped: impl FnMut() -> bool,
) -> Result<Value> {
    let mut db = store::Store::open(root)?;
    validate_run(&db.db, input)?;
    let (record, engine, digest) = installation(&db.db, &input.installation)?;
    let id = if origin.operation.is_some() {
        format!(
            "command-{}",
            crate::store::sha256_bytes(&serde_json::to_vec(&origin)?)
        )
    } else {
        super::id("command")?
    };
    let intent = serde_json::to_value(input)?;
    let existing = |c: &Connection| -> Result<Option<Value>> {
        let Some(r) = store::head(c, "command_run", &id)? else {
            return Ok(None);
        };
        if r.body["intent"] != intent {
            return Err(failure(
                "This command request ID already names a different invocation",
            ));
        }
        Ok(Some(
            json!({"receipt":r.reference.handle(),"result":r.body}),
        ))
    };
    if let Some(previous) = existing(&db.db)? {
        return Ok(previous);
    }
    let mut files = BTreeMap::new();
    let mut mounted = BTreeMap::new();
    for (name, reference) in &input.files {
        let (digest, bytes) = content(&db.db, reference)?;
        files.insert(name.clone(), STANDARD.encode(&bytes));
        mounted.insert(
            name,
            json!({"reference":reference,"object":format!("object:{digest}"),"bytes":bytes.len()}),
        );
    }
    let (stdin, stdin_source) = if let Some(reference) = &input.stdin {
        let (digest, bytes) = content(&db.db, reference)?;
        (
            STANDARD.encode(&bytes),
            json!({"reference":reference,"object":format!("object:{digest}"),"bytes":bytes.len()}),
        )
    } else {
        (String::new(), Value::Null)
    };
    let invocation = Invocation {
        program: STANDARD.encode(store::bytes(&db.db, &digest)?),
        name: record.body["name"]
            .as_str()
            .ok_or_else(|| failure("Installation lacks a command name"))?
            .to_owned(),
        arguments: input.arguments.clone(),
        files,
        outputs: input.outputs.clone(),
        stdin,
    };
    let started = super::now();
    let run = execute(engine, &invocation, &mut stopped).await?;
    db.transaction(|c|{
        if let Some(previous)=existing(c)? {return Ok(previous);}
        let mut outputs=BTreeMap::new();
        for (name,data) in &run.files {
            if !input.outputs.contains(name) {return Err(failure("Worker returned an unrequested output"));}
            outputs.insert(name,stored_output(c,data)?);
        }
        let missing:Vec<_>=input.outputs.iter().filter(|name| !outputs.contains_key(name)).collect();
        let process_succeeded=run.exit_code==Some(0)&&run.error.is_none();
        let body=json!({"format":"portable-command-run-v1","origin":if origin.invocation.is_some(){"persona"}else{"operator"},
            "actor":origin.actor,"work":origin.work,"invocation":origin.invocation,"operation":origin.operation,
            "intent":intent,"installation":input.installation,"program":format!("object:{digest}"),"engine":engine,
            "argv0":invocation.name,"arguments":input.arguments,"files_supplied":mounted,"stdin":stdin_source,
            "outputs":outputs,"missing_outputs":missing,"file_errors":run.file_errors,"stdout":stored_output(c,&run.stdout)?,
            "stderr":stored_output(c,&run.stderr)?,"host":run.host,"exit_code":run.exit_code,"error":run.error,
            "started":started,"finished":super::now(),"process_succeeded":process_succeeded,
            "outputs_complete":missing.is_empty(),"success":process_succeeded&&missing.is_empty()});
        let r=store::write(c,"command_run",&id,0,&body,origin.invocation)?;
        Ok(json!({"receipt":r.handle(),"result":body}))
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct History {
    /// Optional exact installation, command receipt, supplied file, stdout, stderr or output reference.
    pub reference: Option<String>,
    #[serde(default)]
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    #[serde(default = "default_page")]
    #[arg(long, default_value_t = 20)]
    pub page_size: usize,
}
fn default_page() -> usize {
    20
}

pub fn history(db: &Connection, query: &History) -> Result<Value> {
    let mut reference = query.reference.clone();
    let mut digest = None;
    if let Some(r) = query.reference.as_deref() {
        if r.starts_with("asset:") || r.starts_with("object:") {
            digest = Some(format!("object:{}", workspace::byte_digest(db, r)?));
        } else {
            reference = Some(store::exact(db, r)?.reference.handle());
        }
    }
    let matches = |v: &Value| {
        query.reference.is_none()
            || v.as_str()
                .is_some_and(|s| Some(s) == reference.as_deref() || Some(s) == digest.as_deref())
    };
    let selected = if let Some(receipt) = reference
        .as_deref()
        .filter(|r| r.starts_with("command_run:"))
    {
        Some(json!({"receipt":receipt,"result":store::exact(db,receipt)?.body}))
    } else {
        None
    };
    let mut total = 0;
    let mut runs = Vec::new();
    let mut statement = db.prepare(
        "SELECT id,revision,digest FROM versions WHERE kind='command_run' ORDER BY rowid DESC",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let rev: i64 = row.get(1)?;
        let handle = format!("command_run:{id}@{rev}");
        let body: Value = store::object(db, &row.get::<_, String>(2)?)?;
        let file_match = ["files_supplied", "outputs"].iter().any(|&k| {
            body[k]
                .as_object()
                .into_iter()
                .flatten()
                .any(|(_, f)| matches(&f["reference"]) || matches(&f["object"]))
        });
        if !file_match
            && ![
                json!(handle),
                body["installation"].clone(),
                body["program"].clone(),
                body["stdin"]["object"].clone(),
                body["stdout"]["object"].clone(),
                body["stderr"]["object"].clone(),
            ]
            .iter()
            .any(matches)
        {
            continue;
        }
        if total >= query.offset && runs.len() < query.page_size {
            runs.push(json!({"receipt":handle,"actor":body["actor"],"origin":body["origin"],"work":body["work"],
                "installation":body["installation"],"engine":body["engine"],"arguments":body["arguments"],
                "exit_code":body["exit_code"],"success":body["success"],"outputs":body["outputs"],"finished":body["finished"]}));
        }
        total += 1;
    }
    let installations: Vec<_> = store::list(db, "installation")?
        .into_iter()
        .filter(|r| r.body["format"] == "portable-command-v1")
        .map(|r| {
            json!({"reference":r.reference.handle(),"name":r.body["name"],"engine":r.body["engine"],
            "program":r.body["program"],"installed_by":r.body["installed_by"]})
        })
        .collect();
    Ok(
        json!({"installations":installations,"runs":runs,"total":total,"offset":query.offset,"selected":selected}),
    )
}
