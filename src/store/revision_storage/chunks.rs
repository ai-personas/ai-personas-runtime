//! Shared physical bytes for immutable revisions. Manifests do not reference
//! other manifests, so reading an old revision never walks a delta chain.
use super::*;

pub(super) const MAGIC: &[u8; 8] = b"AIPRC001";
pub(super) const MINIMUM_REVISION_BYTES: usize = 64 * 1024;
const MINIMUM_CHUNK_BYTES: usize = 4 * 1024;
const MAXIMUM_CHUNK_BYTES: usize = 64 * 1024;
const DESCRIPTOR_BYTES: usize = 32 + 8;
const CUT_MASK: u64 = (1 << 14) - 1;
const GEAR: [u64; 256] = gear_table();

// This fixed rolling hash chooses boundaries, not identities. SHA-256 binds
// every actual chunk and the complete reconstructed revision separately.
const fn gear_table() -> [u64; 256] {
    let mut table = [0; 256];
    let mut seed = 0x243f6a8885a308d3u64;
    let mut i = 0;
    while i < table.len() {
        seed = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = seed;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        table[i] = value ^ (value >> 31);
        i += 1;
    }
    table
}

pub(super) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS store_revision_chunks (
             sha256 BLOB PRIMARY KEY CHECK(length(sha256)=32),
             raw_bytes INTEGER NOT NULL CHECK(raw_bytes>0),
             encoding INTEGER NOT NULL CHECK(encoding IN (0,1)),
             data BLOB NOT NULL
         ) WITHOUT ROWID;
         CREATE TRIGGER IF NOT EXISTS immutable_revision_chunks
         BEFORE UPDATE ON store_revision_chunks
         BEGIN SELECT RAISE(ABORT,'immutable revision chunk'); END;
         CREATE TRIGGER IF NOT EXISTS permanent_revision_chunks
         BEFORE DELETE ON store_revision_chunks
         BEGIN SELECT RAISE(ABORT,'retained revision chunk'); END;",
    )?;
    Ok(())
}

fn raw_chunk(digest: &[u8], length: usize, encoding: i64, data: Vec<u8>) -> Result<Vec<u8>> {
    let raw = match encoding {
        0 => data,
        1 => zstd::bulk::decompress(&data, length)
            .map_err(|error| StoreError::Corrupt(format!("compressed revision chunk: {error}")))?,
        _ => return Err(StoreError::Corrupt("revision chunk encoding".into())),
    };
    if raw.len() != length || Sha256::digest(&raw)[..] != *digest {
        return Err(StoreError::Corrupt(
            "revision chunk length or digest".into(),
        ));
    }
    Ok(raw)
}

pub(super) fn encode(db: &Connection, data: &[u8]) -> Result<Vec<u8>> {
    // These writes use the caller's existing SQLite transaction. Failure in a
    // later cell or metadata write rolls back new chunks with the entire commit.
    let mut lookup = db.prepare_cached(
        "SELECT raw_bytes,encoding,data FROM store_revision_chunks WHERE sha256=?1",
    )?;
    let mut insert = db.prepare_cached(
        "INSERT INTO store_revision_chunks(sha256,raw_bytes,encoding,data) VALUES(?1,?2,?3,?4)",
    )?;
    let mut validated = BTreeSet::new();
    let mut manifest = Vec::new();
    manifest.extend_from_slice(MAGIC);
    manifest.extend_from_slice(
        &u64::try_from(data.len())
            .map_err(|_| StoreError::Overflow("revision byte length"))?
            .to_le_bytes(),
    );
    manifest.extend_from_slice(&Sha256::digest(data));
    let mut start = 0;
    let mut rolling = 0u64;
    for (index, byte) in data.iter().enumerate() {
        rolling = rolling.wrapping_shl(1).wrapping_add(GEAR[*byte as usize]);
        let end = index + 1;
        let length = end - start;
        if end != data.len()
            && (length < MINIMUM_CHUNK_BYTES
                || (rolling & CUT_MASK != 0 && length < MAXIMUM_CHUNK_BYTES))
        {
            continue;
        }
        let chunk = &data[start..end];
        let digest: [u8; 32] = Sha256::digest(chunk).into();
        if validated.insert(digest) {
            let stored: Option<(i64, i64, Vec<u8>)> = lookup
                .query_row([digest.as_slice()], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .optional()?;
            if let Some((stored_length, encoding, stored)) = stored {
                if usize::try_from(stored_length).ok() != Some(length)
                    || raw_chunk(&digest, length, encoding, stored)? != chunk
                {
                    return Err(StoreError::Corrupt(
                        "revision chunk identity conflict".into(),
                    ));
                }
            } else {
                let compressed = zstd::bulk::compress(chunk, 3)?;
                let (encoding, stored) = if compressed.len() < chunk.len() {
                    (1, compressed.as_slice())
                } else {
                    (0, chunk)
                };
                insert.execute(params![digest.as_slice(), length as i64, encoding, stored])?;
            }
        }
        manifest.extend_from_slice(&digest);
        manifest.extend_from_slice(&(length as u64).to_le_bytes());
        start = end;
        rolling = 0;
    }
    Ok(manifest)
}

pub(super) fn decode(db: &Connection, manifest: &[u8], allocation: u64) -> Result<Vec<u8>> {
    if manifest.len() <= HEADER_BYTES
        || &manifest[..8] != MAGIC
        || (manifest.len() - HEADER_BYTES) % DESCRIPTOR_BYTES != 0
    {
        return Err(StoreError::Corrupt("revision chunk manifest".into()));
    }
    let length = u64::from_le_bytes(manifest[8..16].try_into().expect("checked header"));
    if length > allocation {
        return Err(StoreError::Corrupt(
            "revision length exceeds recorded allocation".into(),
        ));
    }
    let capacity = usize::try_from(length)
        .map_err(|_| StoreError::Overflow("decoded revision byte length"))?;
    let mut decoded = Vec::with_capacity(capacity);
    let mut lookup = db.prepare_cached(
        "SELECT raw_bytes,encoding,data FROM store_revision_chunks WHERE sha256=?1",
    )?;
    for descriptor in manifest[HEADER_BYTES..].chunks_exact(DESCRIPTOR_BYTES) {
        let digest = &descriptor[..32];
        let expected = u64::from_le_bytes(descriptor[32..].try_into().expect("checked descriptor"));
        let expected = usize::try_from(expected)
            .map_err(|_| StoreError::Overflow("revision chunk byte length"))?;
        if expected == 0 || expected > capacity - decoded.len() {
            return Err(StoreError::Corrupt("revision chunk lengths".into()));
        }
        let (stored_length, encoding, data): (i64, i64, Vec<u8>) = lookup
            .query_row([digest], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .optional()?
            .ok_or_else(|| StoreError::Corrupt("revision chunk missing".into()))?;
        if usize::try_from(stored_length).ok() != Some(expected) {
            return Err(StoreError::Corrupt("revision chunk recorded length".into()));
        }
        decoded.extend_from_slice(&raw_chunk(digest, expected, encoding, data)?);
    }
    if decoded.len() != capacity || Sha256::digest(&decoded)[..] != manifest[16..HEADER_BYTES] {
        return Err(StoreError::Corrupt("revision byte length or digest".into()));
    }
    Ok(decoded)
}
