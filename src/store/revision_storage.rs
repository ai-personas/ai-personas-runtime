//! Physical encoding of immutable revision cells. Exact references and Store
//! accounting continue to describe the complete canonical JSON bytes.
use super::*;
use std::borrow::Cow;
use std::io::Write;

const MAGIC: &[u8; 8] = b"AIPRZ001";
const HEADER_BYTES: usize = 8 + 8 + 32;

/// Compress only new immutable revisions, and only when the entire stored
/// representation is smaller. Heads and control cells remain ordinary JSON.
pub(super) fn encode<'a>(namespace: &str, data: &'a [u8]) -> Result<Cow<'a, [u8]>> {
    if namespace != "revision" {
        return Ok(Cow::Borrowed(data));
    }
    let length = u64::try_from(data.len())
        .map_err(|_| StoreError::Overflow("revision byte length"))?;
    let mut header = Vec::with_capacity(HEADER_BYTES);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&length.to_le_bytes());
    header.extend_from_slice(&Sha256::digest(data));
    let mut encoder = zstd::stream::Encoder::new(header, 3)?;
    encoder.include_checksum(true)?;
    encoder.set_pledged_src_size(Some(length))?;
    encoder.write_all(data)?;
    let encoded = encoder.finish()?;
    if encoded.len() < data.len() {
        Ok(Cow::Owned(encoded))
    } else {
        Ok(Cow::Borrowed(data))
    }
}

/// Restore exact bytes before JSON decoding, identity validation or logical
/// accounting. The recorded length is an integrity claim, not a size quota.
/// Existing plain revisions remain their original immutable representation.
pub(super) fn decode(namespace: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    if namespace != "revision" || !data.starts_with(b"AIPR") {
        return Ok(data);
    }
    if data.len() < HEADER_BYTES || &data[..8] != MAGIC {
        return Err(StoreError::Corrupt("revision compression header".into()));
    }
    let length = u64::from_le_bytes(data[8..16].try_into().expect("checked header"));
    let capacity = usize::try_from(length)
        .map_err(|_| StoreError::Overflow("decoded revision byte length"))?;
    let decoded = zstd::bulk::decompress(&data[HEADER_BYTES..], capacity)
        .map_err(|error| StoreError::Corrupt(format!("compressed revision: {error}")))?;
    if decoded.len() != capacity || Sha256::digest(&decoded)[..] != data[16..HEADER_BYTES] {
        return Err(StoreError::Corrupt("revision byte length or digest".into()));
    }
    Ok(decoded)
}
