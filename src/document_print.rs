//! Print static HTML and its exact, explicitly supplied resources with WeasyPrint.
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub struct Resource {
    /// A UTF-8 path in the virtual project, never a host filesystem path.
    pub path: String,
    pub bytes: Vec<u8>,
}

pub struct Printed {
    pub pdf: Vec<u8>,
    pub pages: u32,
    pub renderer: String,
}

const WORKER: &str = include_str!("document_print.py");
const CANCELLED: &str = "Document printing was cancelled";
const INSTALL: &str = "Use the packaged document printer or set PERSONAS_PRINT_PYTHON to an absolute Python interpreter with WeasyPrint 70 and its Pango libraries installed.";

/// File and directory names share one virtual tree, like members of a delivery.
pub fn validate_resource_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let mut files = BTreeSet::new();
    for path in paths {
        if path.contains('\\')
            || path.chars().any(char::is_control)
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
            || path
                .split('/')
                .next()
                .is_some_and(|part| part.contains(':'))
        {
            return Err("Print resource names must be relative paths with ordinary file and directory components".into());
        }
        if !files.insert(path) {
            return Err(format!("Duplicate print resource path: {path}"));
        }
    }
    for path in &files {
        let mut parent = *path;
        while let Some((directory, _)) = parent.rsplit_once('/') {
            if files.contains(directory) {
                return Err(format!(
                    "Print resource {directory} is also used as a directory"
                ));
            }
            parent = directory;
        }
    }
    Ok(())
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    metadata.is_file()
}

fn interpreter() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("PERSONAS_PRINT_PYTHON") {
        let path = PathBuf::from(value);
        return if path.is_absolute() && executable(&path) {
            Ok(path)
        } else {
            Err(format!(
                "The configured print interpreter is unavailable. {INSTALL}"
            ))
        };
    }
    if let Some(parent) = env::current_exe().ok().as_deref().and_then(Path::parent) {
        let path = parent.join("document-printer/bin/python3");
        if executable(&path) {
            return Ok(path);
        }
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("python3"))
        .find(|path| executable(path))
        .ok_or_else(|| format!("No local print interpreter was found. {INSTALL}"))
}

fn io_error(error: io::Error) -> String {
    format!("Document printing I/O error: {error}")
}

/// The document's directory is the virtual root. Resource names match its URLs.
pub fn print_html_with_control(
    source: &[u8],
    resources: &[Resource],
    cancelled: impl FnMut() -> bool,
) -> Result<Printed, String> {
    print(source, None, resources, cancelled)
}

/// Resolve URLs against the document's exact path within a supplied package.
pub fn print_html_at_with_control(
    source: &[u8],
    source_path: &str,
    resources: &[Resource],
    cancelled: impl FnMut() -> bool,
) -> Result<Printed, String> {
    print(source, Some(source_path), resources, cancelled)
}

fn print(
    source: &[u8],
    path: Option<&str>,
    resources: &[Resource],
    mut cancelled: impl FnMut() -> bool,
) -> Result<Printed, String> {
    if cancelled() {
        return Err(CANCELLED.into());
    }
    validate_resource_paths(
        path.into_iter()
            .chain(resources.iter().map(|r| r.path.as_str())),
    )?;
    let python = interpreter()?;
    let stage = tempfile::Builder::new()
        .prefix("personas-print-")
        .tempdir()
        .map_err(io_error)?;
    // No user-supplied path is ever opened. An anonymous file carries a JSON
    // length manifest followed by the exact HTML and binary resource bodies.
    let mut input = tempfile::tempfile_in(&stage).map_err(io_error)?;
    let lengths: Vec<_> = resources.iter().map(|r| (&r.path, r.bytes.len())).collect();
    serde_json::to_writer(&mut input, &(path, source.len(), lengths)).map_err(|e| e.to_string())?;
    input.write_all(b"\n").map_err(io_error)?;
    for bytes in std::iter::once(source).chain(resources.iter().map(|r| r.bytes.as_slice())) {
        for chunk in bytes.chunks(1024 * 1024) {
            if cancelled() {
                return Err(CANCELLED.into());
            }
            input.write_all(chunk).map_err(io_error)?;
        }
    }
    input.rewind().map_err(io_error)?;
    let mut output = tempfile::tempfile_in(&stage).map_err(io_error)?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-B", "-X", "utf8", "-c", WORKER])
        .current_dir(&stage)
        .env_clear()
        .env("HOME", stage.path())
        .env("TMPDIR", stage.path())
        .env("XDG_CACHE_HOME", stage.path())
        .stdin(input)
        .stdout(output.try_clone().map_err(io_error)?)
        .stderr(Stdio::null());
    // Homebrew installs its libraries outside the macOS loader's default path.
    #[cfg(target_os = "macos")]
    command.env(
        "DYLD_FALLBACK_LIBRARY_PATH",
        "/opt/homebrew/lib:/usr/local/lib:/usr/lib",
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(target_os = "linux")]
    crate::pdf_preview::deny_network(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start the local print engine: {e}. {INSTALL}"))?;
    let status = loop {
        let stopped = cancelled();
        match if stopped { Ok(None) } else { child.try_wait() } {
            Ok(Some(status)) => break status,
            Ok(None) if !stopped => std::thread::sleep(Duration::from_millis(60)),
            result => {
                // The leader has not been reaped. Stop its whole renderer group.
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                #[cfg(not(unix))]
                let _ = child.kill();
                let _ = child.wait();
                return Err(result
                    .err()
                    .map(io_error)
                    .unwrap_or_else(|| CANCELLED.into()));
            }
        }
    };
    if status.code() == Some(69) {
        return Err(format!(
            "The local WeasyPrint runtime is unavailable. {INSTALL}"
        ));
    }
    output.rewind().map_err(io_error)?;
    let mut stream = BufReader::new(output);
    let mut line = String::new();
    stream.read_line(&mut line).map_err(io_error)?;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Report {
        Printed { pages: u32, renderer: String },
        Failed { error: String },
    }
    let report: Report = serde_json::from_str(&line).map_err(|_| {
        format!("The local print engine returned no valid result ({status}). {INSTALL}")
    })?;
    match report {
        Report::Failed { error } => Err(error),
        Report::Printed { pages, renderer }
            if status.success() && pages > 0 && renderer.starts_with("WeasyPrint ") =>
        {
            let mut pdf = Vec::new();
            stream.read_to_end(&mut pdf).map_err(io_error)?;
            if cancelled() {
                return Err(CANCELLED.into());
            }
            if !complete_pdf(&pdf) {
                return Err("The print engine returned an incomplete PDF".into());
            }
            Ok(Printed {
                pdf,
                pages,
                renderer,
            })
        }
        _ => Err(format!(
            "The local print engine could not produce a paginated PDF ({status})"
        )),
    }
}

// WeasyPrint emits a cross-reference table or stream. Its final startxref
// must point back into the body independently of the worker's page report.
pub(crate) fn complete_pdf(pdf: &[u8]) -> bool {
    let Some(body) = pdf.trim_ascii_end().strip_suffix(b"%%EOF") else {
        return false;
    };
    let mut lines = body.trim_ascii_end().rsplit(|b| *b == b'\n');
    let offset = lines
        .next()
        .and_then(|s| std::str::from_utf8(s.trim_ascii()).ok())
        .and_then(|s| s.parse::<usize>().ok());
    if !pdf.starts_with(b"%PDF-") || lines.next().is_none_or(|s| s.trim_ascii() != b"startxref") {
        return false;
    }
    let Some(xref) = offset.filter(|n| *n > 0).and_then(|n| body.get(n..)) else {
        return false;
    };
    let mut object = xref.splitn(2, |b| *b == b'\n');
    let first = object.next().unwrap_or_default().trim_ascii();
    if first == b"xref" {
        return true;
    }
    let mut fields = first
        .split(|b| b.is_ascii_whitespace())
        .filter(|s| !s.is_empty());
    let number = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    if !fields.next().is_some_and(number)
        || !fields.next().is_some_and(number)
        || fields.next() != Some(b"obj")
        || fields.next().is_some()
    {
        return false;
    }
    let object = object.next().unwrap_or_default();
    let Some(end) = object.windows(7).position(|w| w == b"\nstream") else {
        return false;
    };
    let dictionary = object[..end].trim_ascii();
    dictionary.starts_with(b"<<")
        && dictionary.ends_with(b">>")
        && dictionary.windows(5).any(|w| w == b"/XRef")
}
