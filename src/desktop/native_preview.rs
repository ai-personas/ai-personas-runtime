//! Compatible native renderers. Inputs are staged; only derived previews are written.
//! No shell, macros, generated programs, provider calls, or automatic installations.
use super::{loader, Body, Decoded, Source};
use std::{
    ffi::OsString,
    io::{Read, Seek, SeekFrom},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone, Copy)]
enum Kind {
    Pdf,
    Office,
    Board,
    Schematic,
    Video,
    Audio,
}

pub(super) struct Route {
    pub label: &'static str,
    pub extensions: &'static [&'static str],
    kind: Kind,
    tools: &'static [&'static str],
}

// Extend this registry and the matching preparation/page renderer together.
// A registered extension is not a promise that its optional renderer is installed.
pub(super) const ROUTES: &[Route] = &[
    Route {
        label: "PDF pages · Poppler",
        extensions: &["pdf"],
        kind: Kind::Pdf,
        tools: &["pdfinfo", "pdftoppm"],
    },
    Route {
        label: "Office pages · LibreOffice + Poppler",
        extensions: &[
            "doc", "docx", "odt", "rtf", "ppt", "pptx", "odp", "xls", "xlsx", "ods",
        ],
        kind: Kind::Office,
        tools: &["soffice", "pdfinfo", "pdftoppm"],
    },
    Route {
        label: "PCB layers · KiCad CLI 9+",
        extensions: &["kicad_pcb"],
        kind: Kind::Board,
        tools: &["kicad-cli"],
    },
    Route {
        label: "Schematic sheets · KiCad CLI 9+",
        extensions: &["kicad_sch"],
        kind: Kind::Schematic,
        tools: &["kicad-cli"],
    },
    Route {
        label: "Video frame inspection · FFmpeg",
        extensions: &["mp4", "mov", "webm", "avi", "mkv", "m4v"],
        kind: Kind::Video,
        tools: &["ffprobe", "ffmpeg"],
    },
    Route {
        label: "Audio waveform · FFmpeg",
        extensions: &["wav", "mp3", "ogg", "flac", "m4a", "aac"],
        kind: Kind::Audio,
        tools: &["ffmpeg"],
    },
];

pub(super) fn route(extension: &str) -> Option<&'static Route> {
    ROUTES
        .iter()
        .find(|route| route.extensions.contains(&extension))
}

fn executable(name: &str) -> Option<PathBuf> {
    let office_names = ["soffice", "libreoffice"];
    let single_name = [name];
    let candidates: &[&str] = if name == "soffice" {
        &office_names
    } else {
        &single_name
    };
    for folder in std::env::split_paths(&std::env::var_os("PATH")?) {
        // Never discover an executable in the generated file's working directory.
        if !folder.is_absolute() {
            continue;
        }
        for candidate in candidates {
            let path = folder.join(candidate);
            if std::fs::metadata(&path)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            {
                return std::fs::canonicalize(path).ok();
            }
        }
    }
    None
}

impl Route {
    pub fn availability(&self) -> Result<(), String> {
        let missing: Vec<_> = self
            .tools
            .iter()
            .filter(|name| executable(name).is_none())
            .copied()
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Missing renderer: {}. Install it on this computer and reload the preview.",
                missing.join(", ")
            ))
        }
    }
}

enum Pages {
    Pdf(PathBuf),
    Svg(Vec<PathBuf>),
    Board(PathBuf),
    Video(PathBuf, f64),
    Audio(PathBuf),
}

pub(super) struct Document {
    directory: tempfile::TempDir,
    pages: Pages,
    pub labels: Vec<String>,
    pub summary: String,
    pub diagnostics: String,
    pub white_background: bool,
}

#[derive(Default)]
pub(super) struct Cancellation {
    cancelled: AtomicBool,
    process: Mutex<Option<Child>>,
}

impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let mut process = self
            .process
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        Self::stop(&mut process);
    }

    fn stop(process: &mut Option<Child>) {
        if let Some(mut child) = process.take() {
            // Retain the unreaped child while signalling its owned process group.
            // The mutex also prevents a concurrent spawn or reap from reusing its ID.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
        }
    }
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(|value| OsString::from(*value)).collect()
}
fn cancelled(cancel: &Cancellation) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("Preview request cancelled.".into())
    } else {
        Ok(())
    }
}

fn run(
    name: &str,
    arguments: &[OsString],
    folder: &Path,
    cancel: &Cancellation,
) -> Result<String, String> {
    cancelled(cancel)?;
    let executable = executable(name)
        .ok_or_else(|| format!("{name} is not installed or is unavailable on PATH."))?;
    let mut log = tempfile::tempfile().map_err(|e| e.to_string())?;
    {
        let mut process = cancel
            .process
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Closing the viewer must not race a converter starting after cancellation.
        cancelled(cancel)?;
        *process = Some(
            Command::new(executable)
                .args(arguments)
                .current_dir(folder)
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(log.try_clone().map_err(|e| e.to_string())?)
                .stderr(log.try_clone().map_err(|e| e.to_string())?)
                .process_group(0)
                .spawn()
                .map_err(|e| format!("Could not start {name}: {e}"))?,
        );
    }
    let status = loop {
        {
            let mut process = cancel
                .process
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if cancel.is_cancelled() {
                Cancellation::stop(&mut process);
                return Err("Preview request cancelled.".into());
            }
            let Some(child) = process.as_mut() else {
                return Err("Preview request cancelled.".into());
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    *process = None;
                    break status;
                }
                Ok(None) => {}
                Err(error) => {
                    Cancellation::stop(&mut process);
                    return Err(format!("Could not observe {name}: {error}"));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    log.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    // Bound diagnostic display, not document/page/geometry admission.
    let mut bytes = Vec::new();
    log.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let truncated = bytes.len() > 65_536;
    bytes.truncate(65_536);
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        output.push_str("\n[Further renderer diagnostics omitted]");
    }
    if !status.success() {
        return Err(format!("{name} failed ({status}).\n{output}"));
    }
    cancelled(cancel)?;
    Ok(output)
}

fn pdf_pages(path: &Path, folder: &Path, cancel: &Cancellation) -> Result<(usize, String), String> {
    let output = run("pdfinfo", &[path.as_os_str().into()], folder, cancel)?;
    let pages = output
        .lines()
        .find_map(|line| line.strip_prefix("Pages:")?.trim().parse::<usize>().ok())
        .filter(|count| *count > 0)
        .ok_or("PDF page count was unavailable; no substitute page count was inferred.")?;
    Ok((pages, output))
}

pub(super) fn prepare(
    source: &Source,
    route: &Route,
    cancel: &Cancellation,
) -> Result<Arc<Document>, String> {
    route.availability()?;
    cancelled(cancel)?;
    let directory = tempfile::Builder::new()
        .prefix("personas-render-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let folder = directory.path();
    let input_dir = folder.join("input");
    std::fs::create_dir(&input_dir).map_err(|e| e.to_string())?;
    let name = source.name();
    let name = Path::new(&name)
        .file_name()
        .ok_or("The selected file has no filename.")?;
    let input = input_dir.join(name);
    std::fs::write(&input, source.bytes()?).map_err(|e| e.to_string())?;
    cancelled(cancel)?;
    let mut diagnostics = String::new();
    let (pages, labels, summary) = match route.kind {
        Kind::Pdf | Kind::Office => {
            let pdf = if matches!(route.kind, Kind::Office) {
                let profile = folder.join("office-profile");
                std::fs::create_dir_all(profile.join("user")).map_err(|e| e.to_string())?;
                // Document rendering uses an isolated profile, never a running user session.
                // These are LibreOffice's documented configuration properties, not a sandbox.
                std::fs::write(profile.join("user/registrymodifications.xcu"), br#"<?xml version="1.0"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry">
 <item oor:path="/org.openoffice.Office.Common/Security/Scripting">
  <prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop>
  <prop oor:name="DisableMacrosExecution" oor:op="fuse"><value>true</value></prop>
  <prop oor:name="DisableActiveContent" oor:op="fuse"><value>true</value></prop>
  <prop oor:name="BlockUntrustedRefererLinks" oor:op="fuse"><value>true</value></prop>
 </item>
 <item oor:path="/org.openoffice.Office.Common/Load">
  <prop oor:name="UserDefinedSettings" oor:op="fuse"><value>false</value></prop>
 </item>
</oor:items>"#).map_err(|e| e.to_string())?;
                let profile_url = url::Url::from_directory_path(&profile).map_err(|_| "Invalid temporary profile path.")?;
                let mut command = args(&["--headless", "--nologo", "--nodefault", "--nofirststartwizard", "--convert-to", "pdf", "--outdir"]);
                command.push(folder.as_os_str().into());
                command.push(format!("-env:UserInstallation={profile_url}").into());
                command.push(input.as_os_str().into());
                diagnostics.push_str(&run("soffice", &command, folder, cancel)?);
                let mut name = input.file_stem().ok_or("Missing document name.")?.to_os_string();
                name.push(".pdf");
                folder.join(name)
            } else { input.clone() };
            let (count, info) = pdf_pages(&pdf, folder, cancel)?;
            diagnostics.push_str(&info);
            let summary = if matches!(route.kind, Kind::Office) {
                "LibreOffice page rendering; layout depends on installed fonts. Spreadsheets use their print layout. The isolated conversion profile disables macros and active content and blocks untrusted document links. This is a derived preview, not an edit to the original."
            } else { "PDF pages rendered by Poppler. Interactive forms, embedded programs, and annotations are not executed." };
            (Pages::Pdf(pdf), (1..=count).map(|i| format!("Page {i}")).collect(), summary.to_owned())
        }
        Kind::Board => (Pages::Board(input), vec!["Front copper + silkscreen + outline".into(), "Back copper + silkscreen + outline (mirrored)".into(), "Board outline".into()],
            "KiCad PCB SVG rendering of the named layers. Inner copper, 3D component models, electrical checks, and fabrication validation are not included. Open a produced GLB/STL/OBJ separately for 3D geometry.".into()),
        Kind::Schematic => {
            let output = folder.join("sheets");
            std::fs::create_dir(&output).map_err(|e| e.to_string())?;
            let mut command = args(&["sch", "export", "svg", "--output"]);
            command.push(output.as_os_str().into());
            command.push(input.as_os_str().into());
            diagnostics.push_str(&run("kicad-cli", &command, folder, cancel)?);
            let mut files = std::fs::read_dir(&output).map_err(|e| e.to_string())?
                .filter_map(Result::ok).map(|entry| entry.path())
                .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "svg"))
                .collect::<Vec<_>>();
            files.sort();
            if files.is_empty() { return Err("KiCad returned no SVG sheets. No schematic preview was inferred.".into()); }
            let labels = files.iter().map(|path| path.file_stem().unwrap_or_default().to_string_lossy().into_owned()).collect();
            (Pages::Svg(files), labels, "KiCad schematic SVG export. Only the selected file is staged; separate hierarchical sheets, libraries, or linked assets may be unavailable. Exported sheets do not establish electrical correctness or completeness.".into())
        }
        Kind::Video => {
            let mut command = args(&["-v", "error", "-protocol_whitelist", "file,pipe", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"]);
            command.push(input.as_os_str().into());
            let output = run("ffprobe", &command, folder, cancel)?;
            let duration = output.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
                .ok_or("Video duration was unavailable. No frame timeline was inferred.")?;
            let labels = (0..8).map(|i| format!("Frame near {:.2} s", duration * i as f64 / 8.0)).collect();
            (Pages::Video(input, duration), labels, format!("Video inspection: eight sampled timestamps across {duration:.2} seconds, rendered on demand. This is not playback and does not show every frame or play audio."))
        }
        Kind::Audio => (Pages::Audio(input), vec!["Whole-file waveform".into()],
            "Audio waveform rendered by FFmpeg. Amplitude is a visual summary; this view does not play audio, transcribe speech, or assess audio quality.".into()),
    };
    Ok(Arc::new(Document {
        directory,
        pages,
        labels,
        summary,
        diagnostics,
        white_background: !matches!(route.kind, Kind::Board),
    }))
}

pub(super) fn page(
    document: &Document,
    index: usize,
    cancel: &Cancellation,
) -> Result<Decoded, String> {
    cancelled(cancel)?;
    if index >= document.labels.len() {
        return Err("The requested preview page is unavailable.".into());
    }
    let folder = document.directory.path();
    let base = folder.join(format!("page-{index}"));
    let pending = folder.join(format!("pending-{index}"));
    let path = match &document.pages {
        Pages::Svg(paths) => paths[index].clone(),
        Pages::Pdf(input) => {
            let path = base.with_extension("png");
            if !path.is_file() {
                let mut command = args(&[
                    "-f",
                    &(index + 1).to_string(),
                    "-l",
                    &(index + 1).to_string(),
                    "-singlefile",
                    "-scale-to",
                    "2400",
                    "-png",
                ]);
                command.extend([input.as_os_str().into(), pending.as_os_str().into()]);
                run("pdftoppm", &command, folder, cancel)?;
                std::fs::rename(pending.with_extension("png"), &path).map_err(|e| e.to_string())?;
            }
            path
        }
        Pages::Board(input) => {
            let path = base.with_extension("svg");
            if !path.is_file() {
                let layers = [
                    "F.Cu,F.SilkS,Edge.Cuts",
                    "B.Cu,B.SilkS,Edge.Cuts",
                    "Edge.Cuts",
                ][index];
                let mut command = args(&[
                    "pcb",
                    "export",
                    "svg",
                    "--mode-single",
                    "--page-size-mode",
                    "2",
                    "--exclude-drawing-sheet",
                    "--layers",
                    layers,
                    "--output",
                ]);
                command.push(pending.with_extension("svg").into_os_string());
                if index == 1 {
                    command.push("--mirror".into());
                }
                command.push(input.as_os_str().into());
                run("kicad-cli", &command, folder, cancel)?;
                std::fs::rename(pending.with_extension("svg"), &path).map_err(|e| e.to_string())?;
            }
            path
        }
        Pages::Video(input, duration) => {
            let path = base.with_extension("png");
            if !path.is_file() {
                let mut command = args(&[
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-nostdin",
                    "-y",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-ss",
                    &(duration * index as f64 / 8.0).to_string(),
                    "-i",
                ]);
                command.push(input.as_os_str().into());
                command.extend(args(&[
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=1600:1000:force_original_aspect_ratio=decrease",
                    "-update",
                    "1",
                ]));
                command.push(pending.with_extension("png").into_os_string());
                run("ffmpeg", &command, folder, cancel)?;
                std::fs::rename(pending.with_extension("png"), &path).map_err(|e| e.to_string())?;
            }
            path
        }
        Pages::Audio(input) => {
            let path = base.with_extension("png");
            if !path.is_file() {
                let mut command = args(&[
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-nostdin",
                    "-y",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-i",
                ]);
                command.push(input.as_os_str().into());
                command.extend(args(&[
                    "-filter_complex",
                    "showwavespic=s=1600x400:colors=0x4a7cbe:split_channels=1",
                    "-frames:v",
                    "1",
                    "-update",
                    "1",
                ]));
                command.push(pending.with_extension("png").into_os_string());
                run("ffmpeg", &command, folder, cancel)?;
                std::fs::rename(pending.with_extension("png"), &path).map_err(|e| e.to_string())?;
            }
            path
        }
    };
    cancelled(cancel)?;
    let mut decoded = if matches!(&document.pages, Pages::Svg(_) | Pages::Board(_)) {
        loader::native_drawing(&Source::File(path), cancel)?
    } else {
        loader::decode(&Source::File(path), cancel)?
    };
    if !matches!(&decoded.body, Body::Image(..)) {
        return Err("The native renderer did not produce a supported image page.".into());
    }
    decoded.note = document.labels[index].clone();
    Ok(decoded)
}
