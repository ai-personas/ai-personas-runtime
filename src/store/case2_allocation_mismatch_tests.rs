//! Synthetic reproduction of Store::Work::insert's explicit-allocation check.
//!
//! This is a local Store unit test, not a runtime/provider fixture. It changes
//! no production limits and uses no credentials, provider calls, or live state.

use super::*;
use serde_json::json;

// Same value as src/store/archive_history.rs's private BODY_BYTES constant.
const CASE2_BODY_BYTES: u64 = 4 * 1024 * 1024;

fn fixture() -> (tempfile::TempDir, Store) {
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
        max_body_bytes: CASE2_BODY_BYTES,
        max_record_bytes: CASE2_BODY_BYTES + CONTROL_HEADROOM,
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

fn read_object(store: &mut Store, id: &str) -> Result<StoredObject> {
    store.transaction(|work| work.required("object", id))
}

#[test]
fn case2_explicit_allocation_mismatch_is_too_large_and_rolls_back() {
    let (_directory, mut store) = fixture();
    let head = StoredObject {
        id: "stable-head".into(),
        revision: 1,
        kind: "fixture-head".into(),
        body: json!({"value": "original"}),
    };
    store
        .transaction(|work| {
            work.insert_exact("object", &head.id, &head, BudgetClass::Ordinary)?;
            Ok(())
        })
        .unwrap();

    let candidate = StoredObject {
        id: "candidate".into(),
        revision: 1,
        kind: "fixture-candidate".into(),
        body: json!({"payload": "synthetic allocation mismatch"}),
    };
    // Measure the exact canonical encoding used by Work::insert, without
    // changing Store's configured max_record_bytes.
    let encoded = encode(&candidate, u64::MAX).unwrap();
    let encoded_len = encoded.len() as u64;
    assert!(encoded_len < store.limits.max_record_bytes);

    let ledger_before = store.ledger().unwrap();
    let epoch_before = store.epoch().unwrap();
    let head_before = read_object(&mut store, &head.id).unwrap();

    // Mutate an existing head and epoch *inside the same transaction* before
    // requesting an allocation one byte too small. The insert's canonical
    // record still fits max_record_bytes; only its explicit allocation fails.
    let error = store
        .transaction(|work| {
            let mut changed = head_before.clone();
            changed.body["value"] = json!("rollback");
            work.update("object", &head.id, &changed)?;
            work.bump_epoch()?;
            work.insert(
                "object",
                &candidate.id,
                &candidate,
                Some(encoded_len - 1),
                BudgetClass::Ordinary,
                0,
            )?;
            Ok(())
        })
        .unwrap_err();
    assert!(matches!(error, StoreError::TooLarge), "got {error:?}");

    // The failed transaction must roll back all earlier writes in its closure.
    assert_eq!(store.ledger().unwrap(), ledger_before);
    assert_eq!(store.epoch().unwrap(), epoch_before);
    assert_eq!(read_object(&mut store, &head.id).unwrap(), head_before);
    assert!(matches!(
        store.transaction(|work| work.get::<StoredObject>("object", &candidate.id)),
        Ok(None)
    ));

    // The same object fits when its explicit allocation equals its canonical
    // serialized length. This is the positive control for the allocation gate.
    store
        .transaction(|work| {
            work.insert(
                "object",
                &candidate.id,
                &candidate,
                Some(encoded_len),
                BudgetClass::Ordinary,
                0,
            )
        })
        .unwrap();
    assert_eq!(read_object(&mut store, &candidate.id).unwrap(), candidate);
    // Ordinary insertion does not itself bump the semantic epoch.
    assert_eq!(store.epoch().unwrap(), epoch_before);
}
