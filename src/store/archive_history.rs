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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let limits = Limits {
            total: Resources {
                navigation_bytes: 256 * 1024 * 1024,
                retained_bytes: 256 * 1024 * 1024,
                ..Resources::default()
            },
            protected_closeout: Resources {
                navigation_bytes: 8 * 1024 * 1024,
                retained_bytes: 8 * 1024 * 1024,
                ..Resources::default()
            },
            max_body_bytes: BODY_BYTES,
            max_record_bytes: BODY_BYTES + CONTROL_HEADROOM,
            max_read_bytes: 8 * 1024 * 1024,
            max_page_bytes: 16 * 1024 * 1024,
            max_page_items: 128,
            max_batch_items: 128,
            max_id_bytes: 256,
            max_identity_records: 100_000,
            max_history_events: 100_000,
            busy_timeout_ms: 5000,
        };
        let store =
            Store::open_for_inspection(&directory.path().join("state.sqlite3"), limits).unwrap();
        (directory, store)
    }

    fn pair(invocation: &str, body: Value) -> (DecisionRecord, StoredObject) {
        let archive = StoredObject {
            id: format!("adoption:{}", sha256_bytes(invocation.as_bytes())),
            revision: 1,
            kind: "adopted-persona-response".into(),
            body,
        };
        let decision = DecisionRecord {
            invocation_id: invocation.into(),
            response_hash: sha256_bytes(b"authored response"),
            decision_hash: sha256_bytes(b"store decision"),
            previous_epoch: 0,
            accepted_epoch: 1,
            objects: vec![archive.exact_ref().unwrap()],
            intent_ids: vec![],
            receipt: Receipt {
                sequence: 1,
                epoch: 1,
                subject: EntityId::Invocation(invocation.into()),
                status: ReceiptStatus::Adopted,
                detail: None,
                evidence: None,
            },
        };
        (decision, archive)
    }

    fn insert(store: &mut Store, decision: &DecisionRecord, archive: Option<&StoredObject>) {
        store
            .transaction(|work| {
                work.insert_exact(
                    "decision",
                    &decision.invocation_id,
                    decision,
                    BudgetClass::Ordinary,
                )?;
                if let Some(archive) = archive {
                    work.insert_exact(
                        "revision",
                        &revision_key(&archive.id, archive.revision),
                        archive,
                        BudgetClass::Ordinary,
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn exact_archive_proofs_survive_reopen_and_ignore_mutable_heads() {
        let (directory, mut store) = fixture();
        let expected = pair("call-1", json!({"authored": "original"}));
        insert(&mut store, &expected.0, Some(&expected.1));
        let mut head = expected.1.clone();
        head.revision = 2;
        head.body = json!({"authored": "different head"});
        store
            .transaction(|work| work.insert_exact("object", &head.id, &head, BudgetClass::Ordinary))
            .unwrap();
        let before = store.ledger().unwrap();
        let epoch = store.epoch().unwrap();
        assert_eq!(
            store.history_archives(None).unwrap(),
            Some(vec![expected.clone()])
        );
        let mut expected_ledger = before;
        expected_ledger.ordinary.consumed.navigation_bytes += NAVIGATION_CHARGE;
        assert_eq!(store.ledger().unwrap(), expected_ledger);
        assert_eq!(store.epoch().unwrap(), epoch);
        let limits = store.limits.clone();
        drop(store);
        let mut store =
            Store::open_for_inspection(&directory.path().join("state.sqlite3"), limits).unwrap();
        assert_eq!(store.ledger().unwrap(), expected_ledger);
        assert_eq!(
            store.history_archives(Some(&["call-1".into()])).unwrap(),
            Some(vec![expected])
        );
    }

    #[test]
    fn absent_and_legacy_archives_are_omitted_with_the_same_fixed_charge() {
        let (_directory, mut store) = fixture();
        let complete = pair("complete", json!({"body": true}));
        let missing = pair("missing-archive", json!({"body": true}));
        let (mut legacy, _) = pair("legacy", json!({}));
        legacy.objects.clear();
        insert(&mut store, &complete.0, Some(&complete.1));
        insert(&mut store, &missing.0, None);
        insert(&mut store, &legacy, None);
        let before = store.ledger().unwrap().ordinary.consumed.navigation_bytes;
        assert_eq!(
            store.history_archives(Some(&["absent".into()])).unwrap(),
            Some(vec![])
        );
        assert_eq!(
            store.ledger().unwrap().ordinary.consumed.navigation_bytes - before,
            NAVIGATION_CHARGE
        );
        assert_eq!(
            store
                .history_archives(Some(&[
                    "complete".into(),
                    "complete".into(),
                    "missing-archive".into(),
                    "legacy".into(),
                ]))
                .unwrap(),
            Some(vec![complete])
        );
        assert_eq!(
            store.ledger().unwrap().ordinary.consumed.navigation_bytes - before,
            2 * NAVIGATION_CHARGE
        );
        assert_eq!(store.history_archives(None).unwrap(), None);
    }

    #[test]
    fn malformed_or_unbound_archives_make_the_entire_result_unavailable() {
        for mismatch in [
            "invocation",
            "digest",
            "kind",
            "revision",
            "duplicate",
            "json",
        ] {
            let (_directory, mut store) = fixture();
            let valid = pair("a-valid", json!({}));
            insert(&mut store, &valid.0, Some(&valid.1));
            let (mut decision, mut archive) = pair("z-invalid", json!({}));
            match mismatch {
                "invocation" => decision.invocation_id = "another-call".into(),
                "digest" => archive.body = json!({"tampered": true}),
                "kind" => {
                    archive.kind = "other-kind".into();
                    decision.objects = vec![archive.exact_ref().unwrap()];
                }
                "revision" => decision.objects[0].revision = 2,
                "duplicate" => decision.objects.push(decision.objects[0].clone()),
                _ => {}
            }
            store
                .transaction(|work| {
                    work.insert_exact("decision", "z-invalid", &decision, BudgetClass::Ordinary)?;
                    if mismatch == "json" {
                        work.insert_exact(
                            "revision",
                            &revision_key(&archive.id, 1),
                            &json!({"wrong": "shape"}),
                            BudgetClass::Ordinary,
                        )?;
                    } else {
                        work.insert_exact(
                            "revision",
                            &revision_key(&archive.id, 1),
                            &archive,
                            BudgetClass::Ordinary,
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
            let before = store.ledger().unwrap().ordinary.consumed.navigation_bytes;
            assert_eq!(store.history_archives(None).unwrap(), None, "{mismatch}");
            assert_eq!(
                store.ledger().unwrap().ordinary.consumed.navigation_bytes - before,
                NAVIGATION_CHARGE,
                "{mismatch}"
            );
        }
    }

    #[test]
    fn snapshot_record_overflow_does_not_prevent_bounded_exact_recovery() {
        let (_directory, mut store) = fixture();
        let expected = pair("z-exact", json!({"recoverable": true}));
        insert(&mut store, &expected.0, Some(&expected.1));
        store
            .transaction(|work| {
                for index in 0..MAX_RECORDS {
                    let (mut decision, _) = pair(&format!("earlier-{index:04}"), json!({}));
                    decision.objects.clear();
                    work.insert_exact(
                        "decision",
                        &decision.invocation_id,
                        &decision,
                        BudgetClass::Ordinary,
                    )?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(store.history_archives(None).unwrap(), None);
        assert_eq!(
            store.history_archives(Some(&["z-exact".into()])).unwrap(),
            Some(vec![expected])
        );
    }

    #[test]
    fn aggregate_body_bound_is_all_or_unavailable_while_exact_lookup_remains_bounded() {
        let (_directory, mut store) = fixture();
        let first = pair(
            "a-large",
            json!({"body": "x".repeat(BODY_BYTES as usize / 2)}),
        );
        let second = pair(
            "z-large",
            json!({"body": "y".repeat(BODY_BYTES as usize / 2)}),
        );
        insert(&mut store, &first.0, Some(&first.1));
        insert(&mut store, &second.0, Some(&second.1));
        assert_eq!(store.history_archives(None).unwrap(), None);
        assert_eq!(
            store.history_archives(Some(&["z-large".into()])).unwrap(),
            Some(vec![second])
        );
    }

    #[test]
    fn insufficient_ordinary_budget_never_uses_protected_capacity() {
        let (_directory, mut store) = fixture();
        let mut meta = load_meta(&store.connection).unwrap();
        meta.ordinary.consumed.navigation_bytes = meta.limits.total.navigation_bytes
            - meta.limits.protected_closeout.navigation_bytes
            - NAVIGATION_CHARGE
            + 1;
        store
            .connection
            .execute(
                "UPDATE store_meta SET data=?1 WHERE singleton=1",
                [encode(&meta, META_CAPACITY).unwrap()],
            )
            .unwrap();
        let before = store.ledger().unwrap();
        assert!(matches!(
            store.history_archives(None),
            Err(StoreError::Quota {
                dimension: Resource::NavigationBytes,
                class: BudgetClass::Ordinary,
            })
        ));
        assert_eq!(store.ledger().unwrap(), before);
        assert!(matches!(
            store.history_archives(Some(&vec!["x".into(); MAX_EXACT_IDS + 1])),
            Err(StoreError::Invalid(_))
        ));
        assert_eq!(store.ledger().unwrap(), before);
    }

    #[test]
    fn admitted_charge_survives_a_read_error() {
        let (_directory, mut store) = fixture();
        let before = store.ledger().unwrap().ordinary.consumed.navigation_bytes;
        store
            .connection
            .execute("DROP TABLE store_cells", [])
            .unwrap();
        assert!(matches!(
            store.history_archives(None),
            Err(StoreError::Sqlite(_))
        ));
        assert_eq!(
            store.ledger().unwrap().ordinary.consumed.navigation_bytes - before,
            NAVIGATION_CHARGE
        );
    }
}
