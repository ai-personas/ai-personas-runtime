//! Bounded trusted access to immutable persona-response archives. Authority and
//! disclosure projection remain the runtime's responsibility.

use super::*;

const MAX_RECORDS: usize = 4096;
const MAX_EXACT_IDS: usize = 128;
const BODY_BYTES: u64 = 4 * 1024 * 1024;
// One fixed envelope in either mode: JSON bodies, both metadata encodings, and
// bounded key/length/cell metadata for 4,097 decision and 4,096 revision lookups.
// This is logical navigation accounting, not a physical SQLite I/O guarantee.
const NAVIGATION_CHARGE: u64 = BODY_BYTES
    + 2 * META_CAPACITY
    + (MAX_RECORDS as u64 + 1) * (256 + 32)
    + MAX_RECORDS as u64 * (100 + 32);

impl Store {
    /// `None` obtains an all-or-unavailable bounded snapshot of permanent
    /// decision proofs. Exact requests use only indexed invocation lookups.
    /// Exact requests omit missing decisions/archives. A snapshot with a missing
    /// proven archive, any malformed present record, or an exceeded envelope is
    /// unavailable. Decisions with no persona archive reference are omitted.
    /// No object head, terminal slot, or closeout capacity is substituted.
    ///
    /// The fixed ordinary charge is admitted before archive consultation and
    /// committed even when history is unavailable or a read fails. Resource
    /// exhaustion prevents consultation. This does not change the epoch, any
    /// identity/history allocation, or lifecycle state.
    pub(crate) fn history_archives(
        &mut self,
        ids: Option<&[String]>,
    ) -> Result<Option<Vec<(DecisionRecord, StoredObject)>>> {
        if let Some(ids) = ids {
            if ids.len() > MAX_EXACT_IDS || ids.iter().any(|id| !valid_id(id, &self.limits)) {
                return Err(StoreError::Invalid(
                    "archive invocation lookup bound".into(),
                ));
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut meta = load_meta(&tx)?;
        if meta.limits != self.limits {
            return Err(StoreError::LimitsMismatch);
        }
        meta.ordinary.consumed.navigation_bytes = meta
            .ordinary
            .consumed
            .navigation_bytes
            .checked_add(NAVIGATION_CHARGE)
            .ok_or(StoreError::Overflow("archive navigation envelope"))?;
        meta.check_dimension(Resource::NavigationBytes, BudgetClass::Ordinary)?;
        tx.execute(
            "UPDATE store_meta SET data=?1 WHERE singleton=1",
            [encode(&meta, META_CAPACITY)?],
        )?;
        // Do not propagate the read result until the admitted charge is durable.
        let result = read_archives(&tx, &self.limits, ids);
        tx.commit()?;
        result
    }
}

fn valid_id(id: &str, limits: &Limits) -> bool {
    !id.is_empty() && id.len() <= limits.max_id_bytes as usize && !id.chars().any(char::is_control)
}

fn read_archives(
    db: &Connection,
    limits: &Limits,
    ids: Option<&[String]>,
) -> Result<Option<Vec<(DecisionRecord, StoredObject)>>> {
    let keys: Vec<String> = if let Some(ids) = ids {
        // A duplicate exact request neither multiplies bodies nor reduces the
        // fixed fee. Input length was already bounded before deduplication.
        ids.iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    } else {
        let mut statement = db.prepare(
            "SELECT CASE WHEN length(CAST(k AS BLOB))<=?1 THEN k ELSE NULL END
             FROM store_cells WHERE ns='decision' ORDER BY k LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![limits.max_id_bytes, MAX_RECORDS as i64 + 1],
            |row| row.get::<_, Option<String>>(0),
        )?;
        let mut keys = Vec::new();
        for row in rows {
            let Some(key) = row? else { return Ok(None) };
            if !valid_id(&key, limits) || keys.len() == MAX_RECORDS {
                return Ok(None);
            }
            keys.push(key);
        }
        keys
    };
    let mut remaining = BODY_BYTES;
    let mut archives = Vec::new();
    for key in keys {
        let Some(bytes) = bounded_cell(db, limits, "decision", &key, &mut remaining)? else {
            if ids.is_none() {
                return Ok(None);
            }
            continue;
        };
        let Some(bytes) = bytes else { return Ok(None) };
        let Ok(decision) = serde_json::from_slice::<DecisionRecord>(&bytes) else {
            return Ok(None);
        };
        if decision.invocation_id != key {
            return Ok(None);
        }
        let id = format!("adoption:{}", sha256_bytes(key.as_bytes()));
        let mut references = decision
            .objects
            .iter()
            .filter(|reference| reference.id == id);
        let Some(reference) = references.next() else {
            continue;
        };
        if references.next().is_some()
            || reference.revision != 1
            || hash_valid(&reference.sha256).is_err()
        {
            return Ok(None);
        }
        let Some(bytes) = bounded_cell(
            db,
            limits,
            "revision",
            &revision_key(&id, 1),
            &mut remaining,
        )?
        else {
            if ids.is_none() {
                return Ok(None);
            }
            continue;
        };
        let Some(bytes) = bytes else { return Ok(None) };
        let Ok(archive) = serde_json::from_slice::<StoredObject>(&bytes) else {
            return Ok(None);
        };
        if archive.id != id
            || archive.revision != 1
            || archive.kind != "adopted-persona-response"
            || archive.exact_ref().ok().as_ref() != Some(reference)
        {
            return Ok(None);
        }
        archives.push((decision, archive));
    }
    Ok(Some(archives))
}

// Outer None means an absent indexed row; inner None means a present row whose
// type, allocation, or body length cannot be safely accepted. The SQL guard
// ensures no body larger than the remaining aggregate envelope is materialized.
fn bounded_cell(
    db: &Connection,
    limits: &Limits,
    namespace: &str,
    key: &str,
    remaining: &mut u64,
) -> Result<Option<Option<Vec<u8>>>> {
    let bytes = db
        .query_row(
            "SELECT CASE WHEN typeof(data)='blob' AND length(data)<=?3
                 AND capacity>=length(data) AND capacity<=?4
                 AND lane IN (0,1) AND slots=0 THEN data ELSE NULL END
         FROM store_cells WHERE ns=?1 AND k=?2",
            params![
                namespace,
                key,
                *remaining as i64,
                limits.max_record_bytes as i64
            ],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        )
        .optional()?;
    if let Some(Some(bytes)) = &bytes {
        *remaining -= bytes.len() as u64;
    }
    Ok(bytes)
}
