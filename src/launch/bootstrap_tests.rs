use super::*;

fn fixture() -> (tempfile::TempDir, Request) {
    let tmp = tempfile::tempdir().unwrap();
    let working = tmp.path().join("working");
    fs::create_dir(&working).unwrap();
    let provider = serde_json::from_value(json!({
        "mode":"platform_api_key","model":"fixture-model","effort":"high",
        "credentials":{"kind":"api_key","key_env":{"env":"UNUSED_FIXTURE_KEY"},"account_env":{"env":"UNUSED_FIXTURE_ACCOUNT"}},
        "evidence":{"mode":"platform_api_key","model":"fixture-model","efforts":["high"],
            "account_env":{"env":"UNUSED_FIXTURE_ACCOUNT"},"account_sha256":"1".repeat(64),
            "transport_profile":"platform_responses_v2","catalog_evidence_sha256":"2".repeat(64),
            "structured_output_evidence_sha256":"3".repeat(64)},
        "limits":{"timeout_ms":1000,"max_prompt_bytes":196608,"max_schema_bytes":65536,
            "max_schema_nodes":10000,"max_schema_depth":64,"max_request_bytes":524288,
            "max_response_bytes":32768,"max_stream_bytes":1048576,"max_event_bytes":131072,"max_events":10000}
    })).unwrap();
    let request = Request {
        id: "fixture-bootstrap".into(),
        operator: "fixture-human".into(),
        allow_direct_provider: true,
        setup: Setup {
            cohort: 2,
            owner: 1,
            destination: "fixture".into(),
            provider,
            working_directory: working,
            share_cohort: true,
            max_steps: 1,
            max_considerations: 16,
            max_work_attempts: 4,
            max_total_steps: 8,
            max_works: 2,
        },
    };
    (tmp, request)
}

#[test]
fn atomic_bootstrap_lost_response_reconciles_without_reset_after_work_intake() {
    let (tmp, request) = fixture();
    let root = tmp.path().join("state");
    // The runtime transaction committed, but its response was lost to the caller.
    assert!(apply_with(&root, &request, || Err(invalid("fixture lost response"))).is_err());
    let mut app = Runtime::open_for_launch_inspection(&root).unwrap();
    let state = app.snapshot().unwrap();
    assert_eq!(state.personas.len(), 2);
    let (_, saved) = load(&mut app).unwrap();
    for id in people(2) {
        assert_eq!(
            serde_json::to_value(&state.personas[&id].seed).unwrap(),
            serde_json::to_value(seed_for(&saved, &request.setup.provider, &id)).unwrap()
        );
    }
    drop(app);
    let admitted = session::intake(
        &root,
        &session::NeedRequest {
            id: "pending".into(),
            operator: request.operator.clone(),
            need: "Exact fixture need".into(),
            require_acceptance: true,
        },
    )
    .unwrap();
    let mut app = Runtime::open_for_launch_inspection(&root).unwrap();
    let heads = (app.load().unwrap().0, load(&mut app).unwrap().0);
    let funding = app.ledger().unwrap();
    drop(app);
    assert!(apply(&root, &request).unwrap().replayed);
    let mut app = Runtime::open_for_launch_inspection(&root).unwrap();
    assert_eq!(app.load().unwrap().0, heads.0);
    assert_eq!(load(&mut app).unwrap().0, heads.1);
    assert!(app
        .snapshot()
        .unwrap()
        .work_meta
        .contains_key(&admitted.receipt.work));
    let after = app.ledger().unwrap();
    assert_eq!(funding.limits, after.limits);
    assert_eq!(funding.ordinary.reserved, after.ordinary.reserved);
    assert_eq!(funding.ordinary.uncertain, after.ordinary.uncertain);
    assert_eq!(funding.closeout, after.closeout);
}

#[test]
fn bootstrap_denial_changed_binding_and_existing_legacy_store_fail_closed() {
    let (tmp, mut request) = fixture();
    let root = tmp.path().join("state");
    request.allow_direct_provider = false;
    assert!(apply(&root, &request).is_err());
    assert!(!root.exists());
    request.allow_direct_provider = true;
    apply(&root, &request).unwrap();
    request.setup.max_total_steps += 1;
    assert!(apply(&root, &request).is_err());
    let old = tmp.path().join("legacy");
    drop(setup(&old, &request.operator, request.setup.clone(), true).unwrap());
    assert!(apply(&old, &request).is_err());
}

#[test]
fn bootstrap_empty_database_interruption_and_concurrent_owner_are_safe() {
    let (tmp, request) = fixture();
    let root = private_tree(&tmp.path().join("state")).unwrap();
    let store =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let owner = store
        .try_execution_owner_without_recovery()
        .unwrap()
        .unwrap();
    assert!(apply(&root, &request).is_err());
    drop(owner);
    drop(store); // Simulate interruption after SQLite schema creation, before setup.
    assert!(!apply(&root, &request).unwrap().replayed);
    assert!(apply(&root, &request).unwrap().replayed);
}

#[test]
fn bootstrap_transaction_does_not_adopt_a_nonempty_nonruntime_store() {
    let (tmp, request) = fixture();
    let root = private_tree(&tmp.path().join("state")).unwrap();
    let mut store =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let foreign = s::StoredObject {
        id: "fixture-unrelated".into(),
        revision: 1,
        kind: "fixture".into(),
        body: json!({"preserve":true}),
    };
    store
        .compare_and_swap_batch(vec![s::Change {
            expected_revision: None,
            object: foreign.clone(),
            depends_on: Vec::new(),
        }])
        .unwrap();
    drop(store);
    assert!(apply(&root, &request).is_err());
    let mut store =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    assert_eq!(store.read(&foreign.id).unwrap(), Some(foreign));
    assert!(store.read(STATE_ID).unwrap().is_none());
    assert!(store.read(ID).unwrap().is_none());
    assert!(store.read(RECEIPT).unwrap().is_none());
}
