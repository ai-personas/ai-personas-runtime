//! Seekable input for local files and immutable archive members. Never use a
//! member name as an extraction path. The owning handle keeps temporary bytes alive.
use super::{native_preview, ArchiveFormat, Source};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    os::unix::fs::FileExt,
};

pub(super) fn copy(
    reader: &mut impl Read,
    writer: &mut impl Write,
    cancel: &native_preview::Cancellation,
) -> Result<(), String> {
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err("Preview cancelled.".into());
        }
        let n = reader.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
    }
}

impl Source {
    pub(super) fn open_input(&self, cancel: &native_preview::Cancellation) -> Result<File, String> {
        if cancel.is_cancelled() {
            return Err("Preview cancelled.".into());
        }
        match self {
            Self::File(path) | Self::Snapshot { path, .. } => {
                // A nonblocking open also permits identifying a FIFO without waiting for a writer.
                use std::os::unix::fs::OpenOptionsExt;
                let mut file = File::options()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                if !file.metadata().map_err(|e| e.to_string())?.is_file() {
                    return Err("Select a regular file.".into());
                }
                if let Self::Snapshot { sha256, .. } = self {
                    let mut hash = Sha256::new();
                    copy(&mut file, &mut hash, cancel)?;
                    if hex::encode(hash.finalize()) != *sha256 {
                        return Err(
                            "The recorded file changed after export. Refresh the recorded files."
                                .into(),
                        );
                    }
                    file.rewind().map_err(|e| e.to_string())?;
                }
                Ok(file)
            }
            Self::Member {
                container,
                format,
                index,
                name,
            } => {
                let mut input = container.open_input(cancel)?;
                match format {
                    ArchiveFormat::Zip => {
                        let mut archive = ai_personas::neural::issued_zip::file_directory(&input)
                            .map_err(|e| e.to_string())?;
                        let mut member = archive.by_index(*index).map_err(|e| e.to_string())?;
                        if member.name() != name {
                            return Err(
                                "The archive has changed. Reopen it to select this entry.".into()
                            );
                        }
                        if !member.is_file()
                            || !matches!(member.unix_mode().unwrap_or(0) & 0o170000, 0 | 0o100000)
                        {
                            return Err(
                                "Select a regular file in the ZIP; links are not followed.".into(),
                            );
                        }
                        let size = member.size();
                        let mut out = tempfile::tempfile().map_err(|e| e.to_string())?;
                        copy(&mut (&mut member).take(size), &mut out, cancel)?;
                        if out.stream_position().map_err(|e| e.to_string())? != size
                            || member.read(&mut [0]).map_err(|e| e.to_string())? != 0
                        {
                            return Err(
                                "The ZIP member size differs from its directory entry.".into()
                            );
                        }
                        out.rewind().map_err(|e| e.to_string())?;
                        Ok(out)
                    }
                    ArchiveFormat::Tar | ArchiveFormat::System => {
                        native_preview::archive(&mut input, *format, Some((*index, name)), cancel)
                    }
                }
            }
        }
    }
}

pub(super) fn read_at(file: &File, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0; length];
    let mut read = 0;
    while read < length {
        match file.read_at(&mut bytes[read..], offset + read as u64) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    bytes.truncate(read);
    Ok(bytes)
}
