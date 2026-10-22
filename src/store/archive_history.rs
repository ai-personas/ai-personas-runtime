//! Trusted access to complete immutable persona-response archives.

use super::*;

impl Store {
    /// `None` reads the permanent decision archive inventory. Exact requests
    /// use indexed lookups and omit missing records. Present records must match
    /// their immutable proof. Record actual logical read bytes even on failure;
    /// history has no application count, batch-byte or identity-size quota.
    pub(crate) fn history_archives(
        &mut self,
        ids: Option<&[String]>,
    ) -> Result<Option<Vec<(DecisionRecord, StoredObject)>>> {
        if let Some(ids) = ids {
            if ids.iter().any(|id| !valid_id(id)) {
                return Err(StoreError::Invalid(
                    "invalid archive invocation identity".into(),
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
        let mut inspected_bytes = 2 * META_CAPACITY;
        let result = read_archives(&tx, ids, &mut inspected_bytes);
        meta.ordinary.consumed.navigation_bytes = meta
            .ordinary
            .consumed
            .navigation_bytes
            .checked_add(inspected_bytes)
            .ok_or(StoreError::Overflow("archive navigation accounting"))?;
        meta.check_dimension(Resource::NavigationBytes, BudgetClass::Ordinary)?;
        tx.execute(
            "UPDATE store_meta SET data=?1 WHERE singleton=1",
            [encode(&meta, META_CAPACITY)?],
        )?;
        tx.commit()?;
        result
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && !id.chars().any(char::is_control)
}

fn read_archives(
    db: &Connection,
    ids: Option<&[String]>,
    inspected_bytes: &mut u64,
) -> Result<Option<Vec<(DecisionRecord, StoredObject)>>> {
    let keys: Vec<String> = if let Some(ids) = ids {
        // Duplicate references do not multiply actual reads or returned bodies.
        ids.iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    } else {
        let mut statement = db.prepare("SELECT k FROM store_cells WHERE ns='decision' ORDER BY k")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut keys = Vec::new();
        for row in rows {
            let key = row?;
            charge(inspected_bytes, key.len() as u64)?;
            if !valid_id(&key) {
                return Ok(None);
            }
            keys.push(key);
        }
        keys
    };
    let mut archives = Vec::new();
    for key in keys {
        let Some(bytes) = archive_cell(db, "decision", &key, inspected_bytes)? else {
            if ids.is_none() {
                return Ok(None);
            }
            continue;
        };
        let Some(bytes) = bytes else { return Ok(None) };
        let Ok(decision) = decode_json::<DecisionRecord>(&bytes) else {
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
        let Some(bytes) = archive_cell(db, "revision", &revision_key(&id, 1), inspected_bytes)?
        else {
            if ids.is_none() {
                return Ok(None);
            }
            continue;
        };
        let Some(bytes) = bytes else { return Ok(None) };
        let Ok(archive) = decode_json::<StoredObject>(&bytes) else {
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

// Outer None means an absent row; inner None means invalid retained metadata.
fn archive_cell(
    db: &Connection,
    namespace: &str,
    key: &str,
    inspected_bytes: &mut u64,
) -> Result<Option<Option<Vec<u8>>>> {
    let row = db.query_row(
        "SELECT length(CAST(data AS BLOB)), CASE WHEN typeof(data)='blob'
             AND capacity>=length(data) AND lane IN (0,1) AND slots=0
             THEN data ELSE NULL END, capacity
         FROM store_cells WHERE ns=?1 AND k=?2",
        params![namespace, key],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?, row.get::<_, i64>(2)?)),
    ).optional()?;
    charge(inspected_bytes, key.len() as u64)?;
    if let Some((bytes, body, capacity)) = row {
        let physical_bytes = u64::try_from(bytes)
            .map_err(|_| StoreError::Overflow("archive byte length"))?;
        let Some(body) = body else {
            charge(inspected_bytes, physical_bytes)?;
            return Ok(Some(None));
        };
        let decoded = match revision_storage::decode(namespace, body) {
            Ok(decoded) => decoded,
            Err(error) => {
                charge(inspected_bytes, physical_bytes)?;
                return Err(error);
            }
        };
        // Archive navigation measures complete logical records, independently
        // of their physical compression. Retain observed cost on failures too.
        charge(inspected_bytes, decoded.len() as u64)?;
        if u64::try_from(capacity).ok().is_none_or(|capacity| decoded.len() as u64 > capacity) {
            return Ok(Some(None));
        }
        Ok(Some(Some(decoded)))
    } else {
        Ok(None)
    }
}

fn charge(inspected_bytes: &mut u64, bytes: u64) -> Result<()> {
    *inspected_bytes = inspected_bytes.checked_add(bytes)
        .ok_or(StoreError::Overflow("archive navigation accounting"))?;
    Ok(())
}
