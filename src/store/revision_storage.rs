//! Physical encoding of immutable revision cells. Exact references and Store
//! accounting continue to describe the complete canonical JSON bytes.
use super::*;
use std::borrow::Cow;
use std::io::Write;

mod chunks;

const MAGIC: &[u8; 8] = b"AIPRZ001";
const HEADER_BYTES: usize = 8 + 8 + 32;

pub(super) fn initialize(db: &Connection) -> Result<()> {
    chunks::initialize(db)
}

/// Large immutable revisions share content-defined chunks in this transaction.
/// Small revisions keep independent compression when it saves space. Heads and
/// control cells remain ordinary JSON; neither representation changes identity.
pub(super) fn encode<'a>(db: &Connection, namespace: &str, data: &'a [u8]) -> Result<Cow<'a, [u8]>> {
    if namespace != "revision" {
        return Ok(Cow::Borrowed(data));
    }
    if data.len() >= chunks::MINIMUM_REVISION_BYTES {
        return chunks::encode(db, data).map(Cow::Owned);
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
pub(super) fn decode(db: &Connection, namespace: &str, data: Vec<u8>, allocation: u64) -> Result<Vec<u8>> {
    if namespace != "revision" || !data.starts_with(b"AIPR") {
        return Ok(data);
    }
    if data.starts_with(chunks::MAGIC) {
        return chunks::decode(db, &data, allocation);
    }
    if data.len() < HEADER_BYTES || &data[..8] != MAGIC {
        return Err(StoreError::Corrupt("revision compression header".into()));
    }
    let length = u64::from_le_bytes(data[8..16].try_into().expect("checked header"));
    // Allocation records the complete bytes retained for this immutable cell.
    // Check that identity metadata before decompression can allocate output.
    // This is not a configured Store limit; new cells grow with their data.
    if length > allocation {
        return Err(StoreError::Corrupt("revision length exceeds recorded allocation".into()));
    }
    let capacity = usize::try_from(length)
        .map_err(|_| StoreError::Overflow("decoded revision byte length"))?;
    let decoded = zstd::bulk::decompress(&data[HEADER_BYTES..], capacity)
        .map_err(|error| StoreError::Corrupt(format!("compressed revision: {error}")))?;
    if decoded.len() != capacity || Sha256::digest(&decoded)[..] != data[16..HEADER_BYTES] {
        return Err(StoreError::Corrupt("revision byte length or digest".into()));
    }
    Ok(decoded)
}
