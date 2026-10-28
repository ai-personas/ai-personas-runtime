//! Explicit imports preserve actual host bytes, separate from persona authorship.
use super::*;
use std::os::unix::fs::OpenOptionsExt;

pub(super) const INTERFACE: &str = "host-file-import-v1";

pub(super) fn read(
    prepared: &Prepared,
    operation: &str,
    path: &str,
    expected_sha256: Option<&str>,
    format: p::FileImportFormat,
    operation_index: usize,
) -> Result<(Vec<u8>, RevisionOrigin)> {
    let interface = prepared
        .host_command_interface
        .as_ref()
        .ok_or_else(|| held("file_import requires the ordinary host interface"))?;
    require(
        prepared.request.mode == Mode::Ordinary,
        "file_import requires an ordinary decision",
    )?;
    let control = request_context::decode(prepared)?.control;
    require(
        control["file_import"]["interface"] == INTERFACE,
        "file_import was not offered in this admission",
    )?;
    let requested = Path::new(path);
    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        Path::new(&interface.default_working_directory).join(requested)
    };
    // Do not block opening a FIFO before discovering that it is not a regular
    // file. Host commands remain available for arbitrary devices and streams.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&resolved)
        .map_err(|e| held(format!("file_import operations[{operation_index}].host_path: {e}")))?;
    require(
        file.metadata()
            .map_err(|e| held(format!("file_import operations[{operation_index}].host_path: {e}")))?
            .is_file(),
        &format!("file_import operations[{operation_index}].host_path: requires a regular file"),
    )?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| held(format!("file_import operations[{operation_index}].host_path: {e}")))?;
    require(
        format == p::FileImportFormat::Bytes || std::str::from_utf8(&bytes).is_ok(),
        &format!("file_import operations[{operation_index}].format: utf8 requires UTF-8 content; bytes were not replaced"),
    )?;
    let sha256 = s::sha256_bytes(&bytes);
    require(
        expected_sha256.is_none_or(|expected| expected == sha256),
        &format!("file_import operations[{operation_index}].expected_sha256: does not match the bytes read; observed_bytes={}; observed_sha256={sha256}. This rejected import retained no native asset. The observation does not establish later host-file contents.", bytes.len()),
    )?;
    let origin = RevisionOrigin::HostFileImport {
        persona: prepared.request.persona.clone(),
        invocation_id: prepared.request.id.clone(),
        operation_id: operation.into(),
        host_path: resolved.to_string_lossy().into_owned(),
        bytes: bytes.len() as u64,
        sha256,
    };
    Ok((bytes, origin))
}
