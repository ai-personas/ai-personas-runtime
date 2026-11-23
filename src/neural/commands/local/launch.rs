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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn receiver_resolves_its_own_program_inside_outer_host() {
        let dir = tempfile::tempdir().unwrap();
        let (outer, receiver) = (dir.path().join("host launcher"), dir.path().join("receiving tools"));
        script(&outer, "export THROUGH_OUTER=observed\nexec \"$@\"");
        script(&receiver, "test \"$1\" = supplier-only-reader\nshift\nprintf '%s\\n' \"$THROUGH_OUTER\" \"$#\" \"$@\"\ncat './issued file'");
        fs::write(dir.path().join("issued file"), b"original bytes").unwrap();
        let mut host = json!({});
        let run = prepare("supplier-only-reader", dir.path(), Some(&outer), Some(&receiver), &mut host)
            .unwrap().args(["a b", "$HOME; other", ""]).current_dir(dir.path()).output().unwrap();
        assert!(run.status.success());
        assert_eq!(run.stdout, b"observed\n3\na b\n$HOME; other\n\noriginal bytes");
        assert_eq!(host["launcher"]["path"], json!(outer));
        assert_eq!(host["executable"], json!(receiver));
        assert_eq!(host["receiving_launcher"]["sha256_before_launch"], json!(crate::store::sha256_bytes(&fs::read(&receiver).unwrap())));
        assert!(prepare("supplier-only-reader", dir.path(), None, None, &mut json!({})).is_err());
    }

    #[test]
    fn configured_broken_route_never_becomes_a_direct_run() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let mut host = json!({});
        assert!(prepare("/bin/true", dir.path(), None, Some(&missing), &mut host).is_err());
        assert!(prepare("/bin/true", dir.path(), None, Some(Path::new("relative")), &mut host).is_err());
        assert!(prepare("/bin/true", dir.path(), Some(&missing), Some(Path::new("/bin/true")), &mut host).is_err());
        let run = prepare("/bin/false", dir.path(), None, None, &mut json!({})).unwrap().output().unwrap();
        assert_eq!(run.status.code(), Some(1));
    }
}
