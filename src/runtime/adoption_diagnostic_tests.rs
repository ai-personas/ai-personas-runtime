//! Synthetic formatter and Store retention checks; no provider or private state.
use super::*;

#[test]
fn adoption_byte_bound_stage_text_is_bounded_and_specific() {
    for (stage, name) in [
        (AdoptionFailureStage::Planning, "planning"),
        (AdoptionFailureStage::Commit, "commit"),
    ] {
        for error in [
            s::StoreError::TooLarge,
            s::StoreError::SerializationByteBound {
                serialized_bytes_at_least: u64::MAX,
                limit_bytes: u64::MAX - 1,
            },
            s::StoreError::RuntimeControlCapacity {
                projected_bytes: u64::MAX,
                limit_bytes: u64::MAX - 1,
            },
        ] {
            let error = RuntimeError::Store(error);
            let reason = adoption_failure_reason(&error, Some(stage));
            assert_eq!(reason, format!("adoption_stage={name} {error}"));
            assert_eq!(bounded_detail(&reason, 180), reason);
            assert!(format!("terminal:{} {reason}", "0".repeat(64)).len() <= 256);
            assert_eq!(adoption_failure_reason(&error, None), error.to_string());
        }
    }
    let error = RuntimeError::ClosureRecordNotSupplied;
    assert_eq!(
        adoption_failure_reason(&error, Some(AdoptionFailureStage::Planning)),
        error.to_string()
    );
}

#[test]
fn commit_byte_bound_failure_rolls_back_and_retains_bounded_diagnostic() {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = directory.path().join("state.sqlite3");
    let mut limits = storage_limits();
    limits.max_body_bytes = 128; // Synthetic fixture only; no production limit is changed.
    let mut store = s::Store::open(&path, limits.clone()).unwrap();
    let original = s::StoredObject {
        id: "fixture-head".into(),
        revision: 1,
        kind: "fixture".into(),
        body: json!({"accepted":true}),
    };
    store.compare_and_swap(None, original.clone()).unwrap();
    let epoch = store.epoch().unwrap();
    let invocation = store
        .prepare_invocation(s::InvocationSpec {
            id: "fixture-call".into(),
            expected_epoch: epoch,
            request: json!({"synthetic":true}),
            reservation: s::Resources {
                attempts: 1,
                input_bytes: 512,
                ..Default::default()
            },
            class: s::BudgetClass::Ordinary,
        })
        .unwrap();
    let id = &invocation.id;
    let hash = s::sha256_bytes(b"synthetic response");
    store.dispatch_invocation(id).unwrap();
    store.record_response(id, &hash).unwrap();
    let responded = store.invocation(id).unwrap();
    let ledger = store.ledger().unwrap();

    let error = store
        .commit_runtime_decision(s::Decision {
            expected_epoch: epoch,
            invocation_id: id.clone(),
            response_hash: hash.clone(),
            changes: vec![s::Change {
                expected_revision: Some(original.revision),
                object: s::StoredObject {
                    revision: 2,
                    body: json!({"private_fixture":"not-for-diagnostics".repeat(128)}),
                    ..original.clone()
                },
                depends_on: Vec::new(),
            }],
            intents: Vec::new(),
        })
        .unwrap_err();
    assert!(matches!(
        &error,
        s::StoreError::SerializationByteBound { limit_bytes: 128, .. }
    ));
    assert_eq!(store.ledger().unwrap(), ledger);
    assert_eq!(store.epoch().unwrap(), epoch);
    assert_eq!(store.invocation(id).unwrap(), responded);

    let reason = adoption_failure_reason(
        &RuntimeError::Store(error),
        Some(AdoptionFailureStage::Commit),
    );
    assert!(reason.starts_with("adoption_stage=commit canonical JSON"));
    assert!(!reason.contains("private_fixture"));
    assert!(!reason.contains("not-for-diagnostics"));
    let detail = format!(
        "terminal:{} {}",
        s::sha256_bytes(b"synthetic terminal"),
        bounded_detail(&reason, 180)
    );
    assert!(detail.len() <= 256);

    // Mirror the existing rejection boundary. Oversized private context must
    // fall back to unknown while preserving the bounded numeric diagnostic.
    store
        .trusted_host()
        .reject_runtime_invocation(id, &detail, json!({"fixture":"x".repeat(8192)}))
        .unwrap();
    store
        .settle(&invocation.reservation, s::Usage::Unknown)
        .unwrap();
    drop(store);

    let mut store = s::Store::open_for_inspection(&path, limits).unwrap();
    let failed = store.invocation(id).unwrap();
    assert_eq!(failed.state, s::InvocationState::Failed);
    assert_eq!(failed.response_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(
        failed.history.last().unwrap().detail.as_deref(),
        Some(detail.as_str())
    );
    assert_eq!(
        failed.runtime_failure_context,
        Some(json!({"kind":"unknown"}))
    );
    assert_eq!(store.epoch().unwrap(), epoch);
    assert_eq!(store.read(&original.id).unwrap(), Some(original));
    assert!(matches!(
        store.retained_decision(id),
        Err(s::StoreError::Missing(_))
    ));
    assert_eq!(
        store.reservation(&invocation.reservation).unwrap().state,
        s::ReservationState::Uncertain
    );
}
