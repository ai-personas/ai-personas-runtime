//! Render one page from retained PDF bytes, using the local Poppler installation.
use std::{
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const PAGE_EDGE: u32 = 2400;

pub struct Page {
    pub png: Vec<u8>,
    pub page_count: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct RenderError {
    message: String,
    pub page_count: Option<u32>,
}
impl RenderError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            page_count: None,
        }
    }
    fn counted(mut self, page_count: u32) -> Self {
        self.page_count = Some(page_count);
        self
    }
}
impl fmt::Display for RenderError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(&self.message)
    }
}
impl std::error::Error for RenderError {}
impl From<io::Error> for RenderError {
    fn from(e: io::Error) -> Self {
        Self::new(format!("PDF rendering I/O error: {e}"))
    }
}

/// Identify a candidate PDF by its header. Its page tree is checked by Poppler.
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|s| s == b"%PDF-")
}

fn executable(name: &str) -> Result<PathBuf, RenderError> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path).filter(|dir| dir.is_absolute()) {
            let path = dir.join(name);
            if let Ok(metadata) = path.metadata() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                if metadata.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    Err(RenderError::new(format!(
        "The Poppler renderer {name} is unavailable on PATH"
    )))
}

// Drain all diagnostics to avoid stalling Poppler. Only the tail is needed for
// its page count / errors, even for documents carrying very large metadata.
fn tail(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    const KEEP: usize = 16 * 1024;
    let mut ring = [0u8; KEEP];
    let mut pos = 0;
    let mut wrapped = false;
    loop {
        let n = pipe.read(&mut ring[pos..])?;
        if n == 0 {
            break;
        }
        pos += n;
        if pos == KEEP {
            pos = 0;
            wrapped = true;
        }
    }
    let mut kept = Vec::with_capacity(if wrapped { KEEP } else { pos });
    if wrapped {
        kept.extend_from_slice(&ring[pos..]);
    }
    kept.extend_from_slice(&ring[..pos]);
    Ok(kept)
}

#[cfg(target_os = "linux")]
pub(crate) fn deny_network(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // The input PDF never supplies an executable or URI. Also deny outbound
    // sockets in the renderer, including when the installer uses a PATH wrapper.
    let ins = |code: u16, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    let ld = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
    let eq = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
    let ret = (libc::BPF_RET | libc::BPF_K) as u16;
    let filter = [
        ins(ld, 0, 0, 0),
        ins(eq, 1, 0, libc::SYS_socket as u32),
        ins(eq, 0, 1, libc::SYS_connect as u32),
        ins(ret, 0, 0, libc::SECCOMP_RET_ERRNO | libc::EPERM as u32),
        ins(ret, 0, 0, libc::SECCOMP_RET_ALLOW),
    ];
    // SAFETY: only prctl and errno inspection run in the forked child. The BPF
    // program and its pointer remain valid until the kernel copies the filter.
    unsafe {
        command.pre_exec(move || {
            let program = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_ptr().cast_mut(),
            };
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(
                    libc::PR_SET_SECCOMP,
                    libc::SECCOMP_MODE_FILTER,
                    &program,
                    0,
                    0,
                ) != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn run(
    name: &str,
    args: &[&str],
    folder: &Path,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, RenderError> {
    if cancelled() {
        return Err(RenderError::new("PDF rendering was cancelled"));
    }
    let mut command = Command::new(executable(name)?);
    command
        .current_dir(folder)
        .env_clear()
        .env("LC_ALL", "C")
        .env("XDG_CACHE_HOME", folder)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(target_os = "linux")]
    deny_network(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| RenderError::new(format!("Could not start {name}: {e}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    std::thread::scope(|threads| {
        let out = threads.spawn(move || tail(stdout));
        let err = threads.spawn(move || tail(stderr));
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if !cancelled() => std::thread::sleep(Duration::from_millis(80)),
                result => {
                    // The unreaped child still owns the PID used for its group.
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    #[cfg(not(unix))]
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(result.err().map_or_else(
                        || RenderError::new("PDF rendering was cancelled"),
                        RenderError::from,
                    ));
                }
            }
        }?;
        let output = out
            .join()
            .map_err(|_| RenderError::new("Could not read Poppler output"))??;
        let diagnostic = err
            .join()
            .map_err(|_| RenderError::new("Could not read Poppler diagnostics"))??;
        if status.success() {
            return Ok(output);
        }
        let diagnostics = String::from_utf8_lossy(&diagnostic);
        if diagnostics.to_ascii_lowercase().contains("password") {
            return Err(RenderError::new("The PDF is password locked"));
        }
        Err(RenderError::new(format!(
            "{name} could not read or render this PDF ({status})"
        )))
    })
}

pub fn render(bytes: &[u8], page: u32) -> Result<Page, RenderError> {
    render_with_control(bytes, page, || false)
}

/// A true control callback stops the active subprocess and drops the staging directory.
pub fn render_with_control(
    bytes: &[u8],
    page: u32,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Page, RenderError> {
    if page == 0 {
        return Err(RenderError::new("PDF page numbers start at 1"));
    }
    if !is_pdf(bytes) {
        return Err(RenderError::new("The retained bytes have no PDF header"));
    }
    if cancelled() {
        return Err(RenderError::new("PDF rendering was cancelled"));
    }
    let stage = tempfile::Builder::new().prefix("personas-pdf-").tempdir()?;
    fs::write(stage.path().join("document.pdf"), bytes)?;
    let info = run("pdfinfo", &["document.pdf"], stage.path(), &mut cancelled)?;
    // Standard page information follows all document-supplied metadata in pdfinfo.
    let page_count = String::from_utf8_lossy(&info)
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("Pages:"))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| RenderError::new("Poppler could not establish the PDF page count"))?;
    if page > page_count {
        return Err(
            RenderError::new(format!("Page {page} is outside this {page_count}-page PDF"))
                .counted(page_count),
        );
    }
    let mut rasterize = || -> Result<Page, RenderError> {
        let number = page.to_string();
        run(
            "pdftoppm",
            &[
                "-f",
                &number,
                "-l",
                &number,
                "-singlefile",
                "-scale-to",
                &PAGE_EDGE.to_string(),
                "-png",
                "document.pdf",
                "page",
            ],
            stage.path(),
            &mut cancelled,
        )?;
        let file = fs::File::open(stage.path().join("page.png"))?;
        // PNG overhead fits comfortably above the maximum 4-byte-per-pixel raster.
        let output_bound = u64::from(PAGE_EDGE).pow(2) * 5 + 65536;
        if file.metadata()?.len() > output_bound {
            return Err(RenderError::new(
                "The renderer returned oversized raster data",
            ));
        }
        let mut png = Vec::new();
        file.take(output_bound + 1).read_to_end(&mut png)?;
        if png.len() < 24
            || png.len() as u64 > output_bound
            || !png.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        {
            return Err(RenderError::new("The renderer did not return a valid PNG"));
        }
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        if !(1..=PAGE_EDGE).contains(&width) || !(1..=PAGE_EDGE).contains(&height) {
            return Err(RenderError::new(
                "The renderer returned invalid page dimensions",
            ));
        }
        resvg::tiny_skia::Pixmap::decode_png(&png)
            .map_err(|_| RenderError::new("The renderer returned an incomplete PNG"))?;
        Ok(Page {
            png,
            page_count,
            width,
            height,
        })
    };
    rasterize().map_err(|e| e.counted(page_count))
}
