//! Native host commands. Commands inherit the host account and environment.
//! There is no sandbox, command allowlist, installation gate, or default timeout.
use crate::store::sha256_bytes;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const PROFILE: &str = "host.command.v1";
pub const OUTPUT_BYTES: usize = 16 * 1024;
pub const SHELL: &str = "/bin/bash";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    /// Shell command or script. Tools and packages may be installed and used directly.
    /// Encode this string once as JSON; backslashes in shell or regex code need JSON escaping.
    pub command: String,
    /// Host working directory. Omission uses the directory shown by the interface.
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Optional bytes for the command's standard input, encoded as UTF-8.
    /// For a Python script, use command "python3 -" and place its code here as one JSON string.
    /// Do not JSON-encode the script a second time inside this string.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Optional command timeout chosen by the persona. Omission means no timeout.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSegment {
    /// Offsets in the observed host log, not in the decoded text.
    pub start_byte: u64,
    pub end_byte: u64,
    pub raw_sha256: String,
    pub text: String,
    /// Invalid UTF-8, including a character cut at a capture boundary, is
    /// replaced in text. The raw range and digest still describe the log bytes.
    pub lossy_utf8: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputCapture {
    /// None means capture was unavailable; it is not an observed empty stream.
    pub bytes_observed: Option<u64>,
    pub capture_complete: bool,
    pub segments: Vec<OutputSegment>,
    /// A live host path, not an immutable retained asset reference.
    pub log_path: Option<String>,
}

impl OutputCapture {
    fn unavailable() -> Self {
        Self {
            bytes_observed: None,
            capture_complete: false,
            segments: Vec::new(),
            log_path: None,
        }
    }

    pub fn retained_text_bytes(&self) -> usize {
        self.segments.iter().map(|segment| segment.text.len()).sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub succeeded: bool,
    pub process_started: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub failure: Option<String>,
    pub stdout: OutputCapture,
    pub stderr: OutputCapture,
    pub working_directory: String,
    pub shell: String,
    pub shell_sha256: Option<String>,
    pub runner_artifact_sha256: Option<String>,
    pub effective_uid: u32,
    pub inherited_environment_sha256: String,
    pub elapsed_milliseconds: u64,
}

pub fn revision() -> String {
    sha256_bytes(
        &[
            include_bytes!("host_command.rs").as_slice(),
            include_bytes!("../Cargo.toml").as_slice(),
            include_bytes!("../Cargo.lock").as_slice(),
        ]
        .concat(),
    )
}

fn file_hash(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = File::open(path).ok()?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65_536];
    loop {
        let count = file.read(&mut bytes).ok()?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    Some(hex::encode(hash.finalize()))
}

fn captured(path: &Path) -> std::io::Result<OutputCapture> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let ranges = if length == 0 {
        Vec::new()
    } else if length <= OUTPUT_BYTES as u64 {
        vec![(0, length)]
    } else {
        let half = (OUTPUT_BYTES / 2) as u64;
        vec![(0, half), (length - half, length)]
    };
    let mut segments = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0; (end - start) as usize];
        // Bound each read by the observed length even if a background writer
        // appends more output. A shortened file fails capture instead of
        // claiming that an unobserved range was read.
        file.read_exact(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let lossy_utf8 = matches!(&text, std::borrow::Cow::Owned(_));
        segments.push(OutputSegment {
            start_byte: start,
            end_byte: end,
            raw_sha256: sha256_bytes(&bytes),
            text: text.into_owned(),
            lossy_utf8,
        });
    }
    Ok(OutputCapture {
        bytes_observed: Some(length),
        capture_complete: length <= OUTPUT_BYTES as u64,
        segments,
        log_path: Some(path.to_string_lossy().into_owned()),
    })
}

/// Run once, after native intent dispatch. Output capture limits the retained
/// excerpt, never the command. Full stdout/stderr stay available as host files.
pub fn execute(input: &Inputs, directory: &Path) -> Outcome {
    use std::os::unix::ffi::OsStrExt;
    let started = Instant::now();
    let mut environment: Vec<_> = std::env::vars_os().collect();
    environment.sort();
    let mut environment_bytes = Vec::new();
    for (key, value) in environment {
        for part in [key, value] {
            let bytes = part.as_os_str().as_bytes();
            environment_bytes.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            environment_bytes.extend_from_slice(bytes);
        }
    }
    let mut outcome = Outcome {
        succeeded: false,
        process_started: false,
        exit_code: None,
        signal: None,
        timed_out: false,
        failure: None,
        stdout: OutputCapture::unavailable(),
        stderr: OutputCapture::unavailable(),
        working_directory: directory.to_string_lossy().into_owned(),
        shell: SHELL.into(),
        shell_sha256: file_hash(Path::new(SHELL)),
        runner_artifact_sha256: std::env::current_exe()
            .ok()
            .and_then(|path| file_hash(&path)),
        // SAFETY: geteuid has no arguments or memory access requirements.
        effective_uid: unsafe { libc::geteuid() },
        inherited_environment_sha256: sha256_bytes(&environment_bytes),
        elapsed_milliseconds: 0,
    };
    let run = (|| -> std::io::Result<()> {
        let logs = tempfile::Builder::new()
            .prefix("personas-host-command-")
            .tempdir()?
            .keep();
        let stdout_path = logs.join("stdout.log");
        let stderr_path = logs.join("stderr.log");
        let output = |path: &PathBuf| OpenOptions::new().write(true).create_new(true).open(path);
        let stdout = output(&stdout_path)?;
        let stderr = output(&stderr_path)?;
        outcome.stdout.log_path = Some(stdout_path.to_string_lossy().into_owned());
        outcome.stderr.log_path = Some(stderr_path.to_string_lossy().into_owned());
        let mut stdin = tempfile::tempfile()?;
        stdin.write_all(input.stdin.as_deref().unwrap_or("").as_bytes())?;
        stdin.seek(SeekFrom::Start(0))?;
        let mut child = Command::new(SHELL)
            .arg("-c")
            .arg(&input.command)
            .current_dir(directory)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .process_group(0)
            .spawn()?;
        outcome.process_started = true;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if input
                .timeout_seconds
                .is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
            {
                outcome.timed_out = true;
                // The timeout is explicitly chosen in this command's input.
                // SAFETY: this child established its own process group at spawn.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                break child.wait()?;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        outcome.exit_code = status.code();
        outcome.signal = status.signal();
        outcome.succeeded = status.success() && !outcome.timed_out;
        outcome.stdout = captured(&stdout_path)?;
        outcome.stderr = captured(&stderr_path)?;
        Ok(())
    })();
    if let Err(error) = run {
        outcome.succeeded = false;
        outcome.failure = Some(error.to_string());
    }
    outcome.elapsed_milliseconds = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    outcome
}
