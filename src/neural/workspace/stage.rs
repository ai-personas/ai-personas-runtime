//! Publish a complete input file into a shared tree without replacing a peer’s
//! work. The directory descriptors remain anchored while the name is installed.
use super::{descriptor_path, open_relative};
use crate::neural::{failure, Result};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::{self, Write},
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::{Component, Path},
};

pub(crate) fn input(root: &File, name: &str, digest: &str, bytes: &[u8]) -> Result<&'static str> {
    let path = Path::new(name);
    let mut parts = path.components().peekable();
    let mut parent = root.try_clone()?;
    let leaf = loop {
        let Some(Component::Normal(part)) = parts.next() else {
            return Err(failure("Choose a relative project input file"));
        };
        if parts.peek().is_none() {
            break Path::new(part);
        }
        let c = CString::new(part.as_bytes())?;
        // A concurrent creator may win; open_relative refuses symlinks.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) } != 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::AlreadyExists {
                return Err(e.into());
            }
        }
        parent = open_relative(&parent, Path::new(part))?;
        if !parent.metadata()?.is_dir() {
            return Err(failure(format!("Not an input directory: {part:?}")));
        }
    };
    let existing = || -> Result<bool> {
        let mut file = match open_relative(&parent, leaf) {
            Ok(file) => file,
            Err(e)
                if e.downcast_ref::<io::Error>()
                    .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                return Ok(false)
            }
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        if meta.is_file() && meta.len() == bytes.len() as u64 {
            let mut actual = Sha256::new();
            io::copy(&mut file, &mut actual)?;
            if hex::encode(actual.finalize()) == digest {
                return Ok(true);
            }
        }
        Err(failure(format!("Input {name:?} already exists and is not the selected bytes; choose a new name. Existing entry left untouched.")))
    };
    if existing()? {
        return Ok("present_identical");
    }
    let dir = descriptor_path(&parent);
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    match temp.persist_noclobber(dir.join(leaf)) {
        Ok(_) => {
            parent.sync_all().map_err(|e| {
                failure(format!(
                    "Published {name:?}, but could not sync its directory: {e}"
                ))
            })?;
            Ok("created")
        }
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists && existing()? => {
            Ok("present_identical")
        }
        Err(e) => Err(e.error.into()),
    }
}
