//! Preserve the persona's program and literal arguments. A receiving launcher
//! can supply different installed tools; an outer local launcher still applies.
use super::*;

const RECEIVER: &str = "AI_PERSONAS_RECEIVING_LAUNCHER";

pub(in crate::neural) fn receiving_launcher() -> Option<PathBuf> {
    std::env::var_os(RECEIVER).map(PathBuf::from)
}

fn executable(program: &str, cwd: &Path) -> Result<PathBuf> {
    let accessible = |path: &Path| {
        path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = if program.contains('/') {
        Some(cwd.join(program)).filter(|p| accessible(p))
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| cwd.join(dir).join(program))
            .find(|p| accessible(p))
    };
    path.ok_or_else(|| failure(format!("Local executable {program:?} was not found or is not executable")))
}

fn digest(file: &mut File) -> Result<String> {
    let mut hash = Sha256::new();
    std::io::copy(file, &mut hash)?;
    Ok(hex::encode(hash.finalize()))
}

fn observe_launcher(path: &Path, setting: &str) -> Result<Value> {
    if !path.is_absolute() {
        return Err(failure(format!("{setting} must name an absolute executable path")));
    }
    let resolved = path.canonicalize()?;
    let mut file = File::options().read(true).custom_flags(libc::O_NONBLOCK).open(&resolved)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(failure(format!("{setting} does not name an executable file")));
    }
    Ok(json!({"path":path.to_string_lossy(),"resolved":resolved.to_string_lossy(),"sha256_before_launch":digest(&mut file)?}))
}

pub(super) fn prepare(
    program: &str, cwd: &Path, outer: Option<&Path>, receiver: Option<&Path>, host: &mut Value,
) -> Result<Command> {
    let target = if let Some(receiver) = receiver {
        let record = observe_launcher(receiver, RECEIVER)?;
        host["resolved_executable"] = record["resolved"].clone();
        host["executable_sha256_before_launch"] = record["sha256_before_launch"].clone();
        host["receiving_launcher"] = record;
        receiver.to_owned()
    } else {
        let target = executable(program, cwd)?;
        host["resolved_executable"] = json!(target.canonicalize()?);
        if let Ok(mut file) = File::open(&target) {
            host["executable_sha256_before_launch"] = json!(digest(&mut file)?);
        }
        target
    };
    host["executable"] = json!(target);
    let mut cmd = if let Some(outer) = outer {
        host["launcher"] = observe_launcher(outer, "AI_PERSONAS_LOCAL_LAUNCHER")?;
        let mut cmd = Command::new(outer);
        cmd.arg(target);
        cmd
    } else {
        Command::new(target)
    };
    if receiver.is_some() {
        // Resolve inside the selected toolchain, not against the author's PATH.
        cmd.arg(program);
    }
    Ok(cmd)
}
