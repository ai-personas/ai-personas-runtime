//! Persistent local mock boundaries. No network, subprocess, helper or model.
#![cfg(unix)]
use ai_personas::{mock_effects as m, store as s, work as w};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::TempDir;

// Exercise the executable's exact entry point in-process, without spawning it.
#[path = "../examples/mock_effects_fixture.rs"]
mod executable_fixture;

fn database(limits: s::Limits) -> (TempDir, PathBuf, s::Store, Vec<s::ObjectRef>) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("mock.sqlite3");
    let mut store = s::Store::open(&path, limits).unwrap();
    let mut pins = Vec::new();
    for (id, kind) in [
        ("authority", "mock-authority"),
        ("input-source", "mock-input"),
    ] {
        let object = s::StoredObject {
            id: id.into(),
            revision: 1,
            kind: kind.into(),
            body: json!({"mock_only":true,"enabled":true,"exact":"initial"}),
        };
        pins.push(object.exact_ref().unwrap());
        store.compare_and_swap(None, object).unwrap();
    }
    (directory, path, store, pins)
}
fn attach(store: &mut s::Store) -> m::MockHarness<'_> {
    m::MockHarness::attach(
        store,
        "fixture-mock-connection".into(),
        executable_fixture::bounds(),
    )
    .unwrap()
}
fn access(mock: &m::MockHarness<'_>, pins: &[s::ObjectRef]) -> m::HostAdmission {
    m::HostAdmission::from_trusted_host(
        mock.epoch().unwrap(),
        "fixture-actor".into(),
        "fixture-work".into(),
        pins.to_vec(),
        executable_fixture::rights(),
    )
}
fn proposal(
    mock: &m::MockHarness<'_>,
    pins: &[s::ObjectRef],
    id: &str,
) -> (m::IntentSpec, Vec<u8>) {
    let request = executable_fixture::request(id);
    let invocation = request.invocation_id.clone();
    let spec = m::IntentSpec::new(request, pins.to_vec(), 64);
    let bytes = executable_fixture::response(&spec, &invocation, mock.epoch().unwrap());
    (spec, bytes)
}
fn submit(mock: &mut m::MockHarness<'_>, pins: &[s::ObjectRef], id: &str) -> m::AdmissionReceipt {
    let (spec, bytes) = proposal(mock, pins, id);
    let admission = access(mock, pins);
    mock.submit_fixture(&admission, &spec, &bytes).unwrap()
}
fn dispatch(mock: &mut m::MockHarness<'_>, pins: &[s::ObjectRef], id: &str) -> m::MockView {
    let admission = access(mock, pins);
    mock.dispatch(&admission, id).unwrap()
}
fn inspect(mock: &mut m::MockHarness<'_>, pins: &[s::ObjectRef], id: &str) -> m::MockView {
    let admission = access(mock, pins);
    mock.inspect(&admission, id).unwrap()
}
fn finish(
    mock: &mut m::MockHarness<'_>,
    pins: &[s::ObjectRef],
    id: &str,
    outcome: m::MockOutcome,
    text: &str,
) -> m::MockView {
    let admission = access(mock, pins);
    mock.simulate_terminal(&admission, id, outcome, text)
        .unwrap()
}

#[test]
fn executable_entry_performs_only_the_persistent_mock_fixture() {
    executable_fixture::main().unwrap();
    assert_eq!(
        ai_personas::runtime::readiness()["strict_live_ready"],
        false
    );
}

#[test]
fn complete_validation_and_explicit_rights_precede_all_store_access() {
    let (_directory, _path, mut store, pins) = database(executable_fixture::limits());
    let mut mock = attach(&mut store);
    let (spec, bytes) = proposal(&mock, &pins, "rejected");
    let admission = access(&mock, &pins);
    let before = mock.ledger().unwrap();
    let mut missing: Value = serde_json::from_slice(&bytes).unwrap();
    missing.as_object_mut().unwrap().remove("next_assets");
    assert!(matches!(
        mock.submit_fixture(&admission, &spec, &serde_json::to_vec(&missing).unwrap()),
        Err(m::MockError::Protocol(_))
    ));
    let mut tail: Value = serde_json::from_slice(&bytes).unwrap();
    tail["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"unknown","id":"tail"}));
    assert!(matches!(
        mock.submit_fixture(&admission, &spec, &serde_json::to_vec(&tail).unwrap()),
        Err(m::MockError::Protocol(_))
    ));
    let mut unsupported: Value = serde_json::from_slice(&bytes).unwrap();
    unsupported["operations"][0]["capability_id"] = json!("native.effect");
    assert!(matches!(
        mock.submit_fixture(
            &admission,
            &spec,
            &serde_json::to_vec(&unsupported).unwrap()
        ),
        Err(m::MockError::Unsupported(_))
    ));
    let denied = m::HostAdmission::from_trusted_host(
        mock.epoch().unwrap(),
        "fixture-actor".into(),
        "fixture-work".into(),
        pins.clone(),
        m::Rights::default(),
    );
    assert!(matches!(
        mock.submit_fixture(&denied, &spec, &bytes),
        Err(m::MockError::Denied)
    ));
    assert_eq!(mock.ledger().unwrap(), before);
    assert!(mock.dispatch(&admission, "rejected").is_err());
    assert_eq!(mock.ledger().unwrap().ordinary.consumed.attempts, 0);
}

#[test]
fn exact_request_and_response_survive_reopen_with_an_actual_event_receipt() {
    let (_directory, path, mut store, pins) = database(executable_fixture::limits());
    let (admitted, terminal, raw) = {
        let mut mock = attach(&mut store);
        let (spec, compact) = proposal(&mock, &pins, "roundtrip");
        let raw = format!(" \n{}\n ", std::str::from_utf8(&compact).unwrap()).into_bytes();
        let admission = access(&mock, &pins);
        let admitted = mock.submit_fixture(&admission, &spec, &raw).unwrap();
        let pending = dispatch(&mut mock, &pins, "roundtrip");
        assert_eq!(pending.state, m::MockState::Pending);
        assert_eq!(pending.observed_mock_executions, Some(1));
        assert!(pending.terminal.is_none());
        let terminal = finish(
            &mut mock,
            &pins,
            "roundtrip",
            m::MockOutcome::Completed,
            "exact \\n λ\n",
        )
        .terminal
        .unwrap();
        assert_eq!(terminal.receipt().status, s::ReceiptStatus::Succeeded);
        assert_eq!(
            terminal.receipt().subject,
            s::EntityId::Intent("roundtrip".into())
        );
        assert_eq!(
            terminal.receipt().evidence.as_ref(),
            Some(terminal.evidence())
        );
        (admitted, terminal, raw)
    };
    drop(store);
    let mut store = s::Store::open(&path, executable_fixture::limits()).unwrap();
    let request = store.read_exact(&admitted.request).unwrap();
    assert_eq!(
        request.body["response_json"].as_str().unwrap().as_bytes(),
        raw
    );
    assert_eq!(
        request.body["request"]["connection"],
        "fixture-mock-connection"
    );
    assert_eq!(
        request.body["request"]["inputs"],
        executable_fixture::request("roundtrip").inputs
    );
    let evidence = store.read_exact(terminal.evidence()).unwrap();
    assert_eq!(
        evidence.body["request"],
        serde_json::to_value(&admitted.request).unwrap()
    );
    assert_eq!(evidence.body["result"], "exact \\n λ\n");
    assert_eq!(evidence.body["mock_only"], true);
    let intent = store.intent("roundtrip").unwrap();
    assert_eq!(
        intent
            .history
            .iter()
            .filter(|r| r.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
    let reservation = store.reservation(&intent.reservation).unwrap();
    assert_eq!(reservation.state, s::ReservationState::Settled);
    assert_eq!(
        reservation.measured.unwrap().output_bytes,
        "exact \\n λ\n".len() as u64
    );
    let mut mock = attach(&mut store);
    let restored = inspect(&mut mock, &pins, "roundtrip").terminal.unwrap();
    assert_eq!(restored.receipt(), terminal.receipt());
    assert_eq!(restored.evidence(), terminal.evidence());
    // This exercises the actual work event carrier. The parent scheduler must
    // supply a registered key and its own current work epoch/authority.
    let event = w::Event {
        key: w::EventKey {
            dependency: admitted.dependency_reference().unwrap(),
            epoch: 1,
        },
        observation: restored.event_observation().unwrap(),
    };
    assert_eq!(event.key.dependency.id, admitted.request.id);
    assert_eq!(event.observation.id, terminal.evidence().id);
    assert_eq!(event.observation.revision, terminal.evidence().revision);
    assert_eq!(
        hex::encode(event.observation.digest),
        terminal.evidence().sha256
    );
    let decoded: w::Event = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn exact_replay_never_executes_twice_and_changed_identity_content_conflicts() {
    let (_directory, _path, mut store, pins) = database(executable_fixture::limits());
    let mut mock = attach(&mut store);
    let (spec, bytes) = proposal(&mock, &pins, "once");
    let admission = access(&mock, &pins);
    let first = mock.submit_fixture(&admission, &spec, &bytes).unwrap();
    dispatch(&mut mock, &pins, "once");
    let before = mock.ledger().unwrap();
    let epoch = mock.epoch().unwrap();
    let admission = access(&mock, &pins);
    let replay = mock.submit_fixture(&admission, &spec, &bytes).unwrap();
    assert_eq!(first.decision, replay.decision);
    assert_eq!(
        dispatch(&mut mock, &pins, "once").observed_mock_executions,
        Some(1)
    );
    assert_eq!(mock.epoch().unwrap(), epoch);
    assert_eq!(
        mock.ledger().unwrap().ordinary.consumed.attempts,
        before.ordinary.consumed.attempts
    );
    assert_eq!(
        mock.ledger().unwrap().permanent_records,
        before.permanent_records
    );
    for field in ["inputs", "request", "actor", "work", "connection"] {
        let mut changed = executable_fixture::request("once");
        match field {
            "inputs" => changed.inputs.push('x'),
            "request" => changed.request_text.push('x'),
            "actor" => changed.actor.push('x'),
            "work" => changed.work.push('x'),
            "connection" => changed.connection.push('x'),
            _ => unreachable!(),
        }
        let invocation = changed.invocation_id.clone();
        let changed = m::IntentSpec::new(changed, pins.clone(), 64);
        let response = executable_fixture::response(&changed, &invocation, epoch);
        let admission = access(&mock, &pins);
        assert!(
            mock.submit_fixture(&admission, &changed, &response)
                .is_err(),
            "{field}"
        );
    }
    let padded = format!(" {}", std::str::from_utf8(&bytes).unwrap());
    let admission = access(&mock, &pins);
    assert!(matches!(
        mock.submit_fixture(&admission, &spec, padded.as_bytes()),
        Err(m::MockError::Store(s::StoreError::Conflict(_)))
    ));
    let original = finish(
        &mut mock,
        &pins,
        "once",
        m::MockOutcome::Completed,
        "terminal",
    )
    .terminal
    .unwrap();
    let repeated = finish(
        &mut mock,
        &pins,
        "once",
        m::MockOutcome::Completed,
        "terminal",
    )
    .terminal
    .unwrap();
    assert_eq!(original.receipt(), repeated.receipt());
    let admission = access(&mock, &pins);
    assert!(mock
        .simulate_terminal(&admission, "once", m::MockOutcome::Failed, "different")
        .is_err());
    assert_eq!(
        inspect(&mut mock, &pins, "once")
            .terminal
            .unwrap()
            .receipt(),
        original.receipt()
    );
}

#[test]
fn owner_loss_conserves_uncertainty_and_late_receipt_reconciles_without_retry() {
    let (_directory, path, mut store, pins) = database(executable_fixture::limits());
    {
        let mut mock = attach(&mut store);
        submit(&mut mock, &pins, "lost-owner");
        dispatch(&mut mock, &pins, "lost-owner");
    }
    drop(store);
    let mut inspection =
        s::Store::open_for_inspection(&path, executable_fixture::limits()).unwrap();
    assert_eq!(
        inspection.intent("lost-owner").unwrap().state,
        s::IntentState::Dispatched
    );
    drop(inspection);
    let mut store = s::Store::open(&path, executable_fixture::limits()).unwrap();
    let uncertain = store.intent("lost-owner").unwrap();
    assert_eq!(uncertain.state, s::IntentState::Uncertain);
    let hold = store.reservation(&uncertain.reservation).unwrap();
    assert_eq!(hold.state, s::ReservationState::Uncertain);
    assert_eq!(hold.measured, None);
    let before = store.ledger().unwrap();
    assert_eq!(before.ordinary.uncertain.output_bytes, 64);
    assert_eq!(
        before.ordinary.uncertain.input_bytes,
        uncertain.intent.reservation.input_bytes
    );
    {
        let mut mock = attach(&mut store);
        for _ in 0..2 {
            let view = dispatch(&mut mock, &pins, "lost-owner");
            assert_eq!(view.state, m::MockState::EffectUnknown);
            assert_eq!(view.observed_mock_executions, Some(1));
            assert!(view.terminal.is_none());
        }
        assert_eq!(
            mock.ledger().unwrap().ordinary.uncertain,
            before.ordinary.uncertain
        );
        assert_eq!(
            mock.ledger().unwrap().ordinary.consumed.attempts,
            before.ordinary.consumed.attempts
        );
        let late = finish(
            &mut mock,
            &pins,
            "lost-owner",
            m::MockOutcome::Completed,
            "late exact observation",
        );
        assert_eq!(late.state, m::MockState::Completed);
        assert_eq!(late.observed_mock_executions, Some(1));
        assert_eq!(mock.ledger().unwrap().ordinary.uncertain.output_bytes, 0);
    }
    let record = store.intent("lost-owner").unwrap();
    assert!(record.reconciliation.is_some());
    assert_eq!(
        record
            .history
            .iter()
            .filter(|r| r.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
    assert_eq!(
        record.reconciliation.unwrap().uncertain_receipt,
        uncertain.history.last().unwrap().sequence
    );
    assert_eq!(
        store
            .reservation(&record.reservation)
            .unwrap()
            .measured
            .unwrap()
            .output_bytes,
        22
    );
}

#[test]
fn cancellation_request_and_late_terminal_outcomes_remain_distinct() {
    for outcome in [
        m::MockOutcome::Completed,
        m::MockOutcome::Failed,
        m::MockOutcome::CanceledToObservedExtent,
    ] {
        let (_directory, path, mut store, pins) = database(executable_fixture::limits());
        let terminal = {
            let mut mock = attach(&mut store);
            submit(&mut mock, &pins, "cancel");
            dispatch(&mut mock, &pins, "cancel");
            let admission = access(&mock, &pins);
            assert!(mock
                .simulate_terminal(
                    &admission,
                    "cancel",
                    m::MockOutcome::CanceledToObservedExtent,
                    "closed to the stated mock extent"
                )
                .is_err());
            assert!(inspect(&mut mock, &pins, "cancel").terminal.is_none());
            let before = mock.ledger().unwrap().ordinary.reserved;
            let admission = access(&mock, &pins);
            let requested = mock
                .request_cancel(
                    &admission,
                    "cancel",
                    "Stop requested for this mock execution.",
                )
                .unwrap();
            assert_eq!(requested.state, m::MockState::CancelRequested);
            assert!(requested.cancel_requested);
            assert!(requested.terminal.is_none());
            assert_eq!(mock.ledger().unwrap().ordinary.reserved, before);
            let admission = access(&mock, &pins);
            mock.request_cancel(
                &admission,
                "cancel",
                "Stop requested for this mock execution.",
            )
            .unwrap();
            let admission = access(&mock, &pins);
            assert!(mock
                .request_cancel(&admission, "cancel", "A conflicting request.")
                .is_err());
            let completed = finish(
                &mut mock,
                &pins,
                "cancel",
                outcome,
                "The explicitly observed mock extent.",
            );
            assert!(completed.cancel_requested);
            let terminal = completed.terminal.unwrap();
            assert_eq!(terminal.outcome(), outcome);
            terminal
        };
        drop(store);
        let mut store = s::Store::open(&path, executable_fixture::limits()).unwrap();
        let evidence = store.read_exact(terminal.evidence()).unwrap();
        assert!(evidence.body["cancellation"].is_object());
        assert_eq!(
            evidence.body["result"],
            "The explicitly observed mock extent."
        );
        let mut mock = attach(&mut store);
        let restored = inspect(&mut mock, &pins, "cancel");
        assert!(restored.cancel_requested);
        assert_eq!(restored.terminal.unwrap().receipt(), terminal.receipt());
    }
}

#[test]
fn pause_and_source_changes_cannot_be_rebased_into_dispatch_authority() {
    for changed_id in ["authority", "input-source"] {
        let (_directory, path, mut store, pins) = database(executable_fixture::limits());
        let mut peer = s::Store::open_for_inspection(&path, executable_fixture::limits()).unwrap();
        let mut mock = attach(&mut store);
        submit(&mut mock, &pins, "fenced");
        let stale = access(&mock, &pins);
        let old = peer.read(changed_id).unwrap().unwrap();
        let mut changed = old.clone();
        changed.revision += 1;
        changed.body = json!({"mock_only":true,"enabled":false,"exact":"changed"});
        let changed_ref = changed.exact_ref().unwrap();
        peer.compare_and_swap(Some(old.revision), changed).unwrap();
        assert!(matches!(
            mock.dispatch(&stale, "fenced"),
            Err(m::MockError::Store(s::StoreError::StaleEpoch { .. }))
        ));
        let current_with_old_pins = access(&mock, &pins);
        assert!(matches!(
            mock.dispatch(&current_with_old_pins, "fenced"),
            Err(m::MockError::Store(s::StoreError::RevisionConflict(_)))
        ));
        let new_pins: Vec<_> = pins
            .iter()
            .map(|p| {
                if p.id == changed_id {
                    changed_ref.clone()
                } else {
                    p.clone()
                }
            })
            .collect();
        let paused = m::HostAdmission::from_trusted_host(
            mock.epoch().unwrap(),
            "fixture-actor".into(),
            "fixture-work".into(),
            new_pins.clone(),
            m::Rights {
                observe: true,
                ..m::Rights::default()
            },
        );
        assert!(matches!(
            mock.dispatch(&paused, "fenced"),
            Err(m::MockError::Denied)
        ));
        // Even a caller claiming renewed dispatch cannot replace the original
        // intent's exact authority/source dependencies with the new revisions.
        let rebased = access(&mock, &new_pins);
        assert!(mock.dispatch(&rebased, "fenced").is_err());
        let view = mock.inspect(&paused, "fenced").unwrap();
        assert_eq!(view.state, m::MockState::Admitted);
        assert_eq!(view.observed_mock_executions, Some(0));
        assert!(peer
            .intent("fenced")
            .unwrap()
            .history
            .iter()
            .all(|r| r.status != s::ReceiptStatus::Dispatched));
    }
}

#[test]
fn result_bounds_preserve_holds_and_failed_effects_do_not_complete_other_effects() {
    let (_directory, _path, mut store, pins) = database(executable_fixture::limits());
    let mut mock = attach(&mut store);
    submit(&mut mock, &pins, "first");
    dispatch(&mut mock, &pins, "first");
    let admission = access(&mock, &pins);
    mock.simulate_unknown(&admission, "first").unwrap();
    let first_hold = mock.ledger().unwrap().ordinary.uncertain;
    submit(&mut mock, &pins, "second");
    dispatch(&mut mock, &pins, "second");
    let admission = access(&mock, &pins);
    let before = mock.ledger().unwrap();
    assert!(matches!(
        mock.simulate_terminal(
            &admission,
            "second",
            m::MockOutcome::Completed,
            &"x".repeat(65)
        ),
        Err(m::MockError::TooLarge(_))
    ));
    assert_eq!(
        mock.ledger().unwrap().ordinary.reserved,
        before.ordinary.reserved
    );
    assert_eq!(mock.ledger().unwrap().ordinary.uncertain, first_hold);
    assert!(inspect(&mut mock, &pins, "second").terminal.is_none());
    let failure = finish(
        &mut mock,
        &pins,
        "second",
        m::MockOutcome::Failed,
        "Observed mock failure.",
    );
    assert_eq!(failure.state, m::MockState::Failed);
    assert_eq!(
        failure.terminal.unwrap().receipt().status,
        s::ReceiptStatus::Failed
    );
    assert_eq!(
        inspect(&mut mock, &pins, "first").state,
        m::MockState::EffectUnknown
    );
    assert_eq!(mock.ledger().unwrap().ordinary.uncertain, first_hold);
}

#[test]
fn finite_attempt_quota_and_unknown_accounting_survive_repeated_admission_and_reopen() {
    let mut limits = executable_fixture::limits();
    limits.total.attempts = 5;
    let (_directory, path, mut store, pins) = database(limits.clone());
    {
        let mut mock = attach(&mut store);
        submit(&mut mock, &pins, "unknown");
        dispatch(&mut mock, &pins, "unknown");
        let admission = access(&mock, &pins);
        mock.simulate_unknown(&admission, "unknown").unwrap();
        submit(&mut mock, &pins, "independent");
        dispatch(&mut mock, &pins, "independent");
        finish(
            &mut mock,
            &pins,
            "independent",
            m::MockOutcome::Completed,
            "observed",
        );
        let (spec, response) = proposal(&mock, &pins, "unfunded");
        for _ in 0..2 {
            let admission = access(&mock, &pins);
            assert!(matches!(
                mock.submit_fixture(&admission, &spec, &response),
                Err(m::MockError::Store(s::StoreError::Quota {
                    dimension: s::Resource::Attempts,
                    ..
                }))
            ));
            assert_eq!(mock.ledger().unwrap().ordinary.consumed.attempts, 5);
            assert_eq!(mock.ledger().unwrap().ordinary.uncertain.output_bytes, 64);
        }
        assert!(inspect_missing(&mut mock, &pins, "unfunded"));
    }
    drop(store);
    let mut store = s::Store::open(&path, limits).unwrap();
    assert_eq!(store.ledger().unwrap().ordinary.consumed.attempts, 5);
    assert_eq!(store.ledger().unwrap().ordinary.uncertain.output_bytes, 64);
    assert_eq!(
        store.invocation("fixture-decision-unfunded").unwrap().state,
        s::InvocationState::Responded
    );
    assert!(matches!(
        store.intent("unfunded"),
        Err(s::StoreError::Missing(_))
    ));
    let mut mock = attach(&mut store);
    assert_eq!(
        dispatch(&mut mock, &pins, "unknown").state,
        m::MockState::EffectUnknown
    );
    assert_eq!(
        inspect(&mut mock, &pins, "independent").state,
        m::MockState::Completed
    );
    assert_eq!(mock.ledger().unwrap().ordinary.consumed.attempts, 5);
}
fn inspect_missing(mock: &mut m::MockHarness<'_>, pins: &[s::ObjectRef], id: &str) -> bool {
    let admission = access(mock, pins);
    matches!(
        mock.inspect(&admission, id),
        Err(m::MockError::Store(s::StoreError::Missing(_)))
    )
}

#[test]
fn evidence_capacity_failure_precedes_mock_dispatch() {
    let mut limits = executable_fixture::limits();
    // Two initial objects use four permanent cells. Invocation+decision+request+
    // intent use seven more. Only one of the three evidence slots then fits the
    // ordinary share (13 total minus one protected identity).
    limits.max_identity_records = 13;
    let (_directory, path, mut store, pins) = database(limits);
    let mut peer = s::Store::open_for_inspection(&path, {
        let mut limits = executable_fixture::limits();
        limits.max_identity_records = 13;
        limits
    })
    .unwrap();
    let mut mock = attach(&mut store);
    submit(&mut mock, &pins, "no-slots");
    let admission = access(&mock, &pins);
    assert!(matches!(
        mock.dispatch(&admission, "no-slots"),
        Err(m::MockError::Store(s::StoreError::IdentityLimit))
    ));
    let record = peer.intent("no-slots").unwrap();
    assert_eq!(record.state, s::IntentState::Admitted);
    assert!(record
        .history
        .iter()
        .all(|r| r.status != s::ReceiptStatus::Dispatched));
    assert_eq!(
        inspect(&mut mock, &pins, "no-slots").observed_mock_executions,
        Some(0)
    );
    assert_eq!(
        peer.reservation(&record.reservation).unwrap().state,
        s::ReservationState::Reserved
    );
}

fn mock_navigation_limits() -> s::Limits {
    let mut limits = executable_fixture::limits();
    limits.total.navigation_bytes = 1_000_000;
    limits.protected_closeout.navigation_bytes = 256_000;
    limits
}

fn closeout_access(mock: &m::MockHarness<'_>, pins: &[s::ObjectRef]) -> m::HostAdmission {
    m::HostAdmission::from_trusted_host(
        mock.epoch().unwrap(),
        "fixture-actor".into(),
        "fixture-work".into(),
        pins.to_vec(),
        m::Rights {
            cancel: true,
            observe: true,
            ..m::Rights::default()
        },
    )
}

fn hold_mock_ordinary_navigation(store: &mut s::Store) -> s::ReservationRecord {
    let report = store.ledger().unwrap();
    let remaining = report.limits.total.navigation_bytes
        - report.limits.protected_closeout.navigation_bytes
        - report.ordinary.exposure().unwrap().navigation_bytes;
    assert!(remaining > 0);
    store
        .reserve(
            "ordinary-navigation-hold",
            s::Resources {
                navigation_bytes: remaining,
                ..s::Resources::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap()
}

// Test-only durable inspection works even when both navigation accounts are full.
// It neither changes accounting nor supplies observations to the mock harness.
fn retained_mock_cells(path: &std::path::Path) -> Vec<(String, String, Vec<u8>)> {
    let connection = rusqlite::Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT ns,k,data FROM store_cells ORDER BY ns,k")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap();
    rows.collect::<std::result::Result<Vec<_>, _>>().unwrap()
}

#[test]
fn postdispatch_cancellation_and_terminal_observations_use_finite_closeout_navigation() {
    for outcome in [
        m::MockOutcome::Completed,
        m::MockOutcome::Failed,
        m::MockOutcome::CanceledToObservedExtent,
    ] {
        let limits = mock_navigation_limits();
        let (_directory, path, mut store, pins) = database(limits.clone());
        let mut peer = s::Store::open_for_inspection(&path, limits.clone()).unwrap();
        let mut mock = attach(&mut store);
        let admitted = submit(&mut mock, &pins, "nav-cancel");
        dispatch(&mut mock, &pins, "nav-cancel");
        let dispatched = peer.intent("nav-cancel").unwrap();
        let hold = hold_mock_ordinary_navigation(&mut peer);
        let before = mock.ledger().unwrap();
        let epoch = mock.epoch().unwrap();
        let ordinary = access(&mock, &pins);
        assert!(matches!(
            mock.inspect(&ordinary, "nav-cancel"),
            Err(m::MockError::Store(s::StoreError::Quota {
                dimension: s::Resource::NavigationBytes,
                class: s::BudgetClass::Ordinary,
            }))
        ));
        assert_eq!(mock.ledger().unwrap(), before);

        let admission = closeout_access(&mock, &pins);
        let requested = mock
            .request_cancel(&admission, "nav-cancel", "Stop requested.")
            .unwrap();
        assert_eq!(requested.state, m::MockState::CancelRequested);
        assert_eq!(requested.observed_mock_executions, Some(1));
        assert!(requested.cancel_requested);
        assert!(requested.terminal.is_none());
        assert_eq!(mock.epoch().unwrap(), epoch);
        assert_eq!(mock.ledger().unwrap().ordinary, before.ordinary);

        let admission = closeout_access(&mock, &pins);
        let completed = mock
            .simulate_terminal(&admission, "nav-cancel", outcome, "Observed mock extent.")
            .unwrap();
        assert_eq!(completed.observed_mock_executions, Some(1));
        assert!(completed.cancel_requested);
        assert!(completed.unapplied_terminal.is_none());
        assert_eq!(
            completed.state,
            match outcome {
                m::MockOutcome::Completed => m::MockState::Completed,
                m::MockOutcome::Failed => m::MockState::Failed,
                m::MockOutcome::CanceledToObservedExtent => m::MockState::CanceledToObservedExtent,
            }
        );
        let terminal = completed.terminal.unwrap();
        assert_eq!(terminal.outcome(), outcome);
        let admission = closeout_access(&mock, &pins);
        let replay = mock
            .simulate_terminal(&admission, "nav-cancel", outcome, "Observed mock extent.")
            .unwrap()
            .terminal
            .unwrap();
        assert_eq!(replay.receipt(), terminal.receipt());
        assert_eq!(replay.evidence(), terminal.evidence());
        let after = mock.ledger().unwrap();
        assert_eq!(after.ordinary.consumed.attempts, 2);
        assert_eq!(
            after.ordinary.consumed.attempts,
            before.ordinary.consumed.attempts
        );
        assert_eq!(
            after.ordinary.consumed.navigation_bytes,
            before.ordinary.consumed.navigation_bytes
        );
        assert_eq!(
            after.ordinary.reserved.navigation_bytes,
            hold.amount.navigation_bytes
        );
        assert!(
            after.closeout.consumed.navigation_bytes > before.closeout.consumed.navigation_bytes
        );
        assert!(
            after.closeout.exposure().unwrap().navigation_bytes
                <= limits.protected_closeout.navigation_bytes
        );
        assert_eq!(after.permanent_records, before.permanent_records);
        assert_eq!(
            after.allocated_history_events,
            before.allocated_history_events
        );

        let record = peer.trusted_host().intent("nav-cancel").unwrap();
        assert!(record.history.starts_with(&dispatched.history));
        assert_eq!(
            record
                .history
                .iter()
                .filter(|r| r.status == s::ReceiptStatus::Dispatched)
                .count(),
            1
        );
        if outcome == m::MockOutcome::CanceledToObservedExtent {
            assert_eq!(record.history.len(), dispatched.history.len() + 2);
            let uncertain = &record.history[dispatched.history.len()];
            assert_eq!(uncertain.status, s::ReceiptStatus::Uncertain);
            assert_eq!(
                record.reconciliation.as_ref().unwrap().uncertain_receipt,
                uncertain.sequence
            );
        } else {
            assert_eq!(record.history.len(), dispatched.history.len() + 1);
            assert!(record.reconciliation.is_none());
        }
        let usage = peer
            .trusted_host()
            .reservation(&record.reservation)
            .unwrap();
        assert_eq!(usage.state, s::ReservationState::Settled);
        assert_eq!(
            usage.measured.unwrap(),
            s::Resources {
                attempts: 1,
                input_bytes: dispatched.intent.reservation.input_bytes,
                output_bytes: "Observed mock extent.".len() as u64,
                ..s::Resources::default()
            }
        );
        assert_eq!(peer.trusted_host().reservation(&hold.id).unwrap(), hold);
        let evidence = peer.trusted_host().read_exact(terminal.evidence()).unwrap();
        assert_eq!(
            evidence.body["request"],
            serde_json::to_value(&admitted.request).unwrap()
        );
        assert_eq!(evidence.body["result"], "Observed mock extent.");
        let start_ref: s::ObjectRef =
            serde_json::from_value(evidence.body["started"].clone()).unwrap();
        let start = peer.trusted_host().read_exact(&start_ref).unwrap();
        assert_eq!(
            start.body["dispatch"],
            serde_json::to_value(dispatched.history.last().unwrap()).unwrap()
        );
    }
}

#[test]
fn owner_loss_and_late_observation_preserve_unknown_holds_without_ordinary_navigation() {
    let limits = mock_navigation_limits();
    let (_directory, path, mut store, pins) = database(limits.clone());
    let mut peer = s::Store::open_for_inspection(&path, limits.clone()).unwrap();
    let hold = {
        let mut mock = attach(&mut store);
        submit(&mut mock, &pins, "nav-recovery");
        dispatch(&mut mock, &pins, "nav-recovery");
        let hold = hold_mock_ordinary_navigation(&mut peer);
        peer.settle(&hold.id, s::Usage::Unknown).unwrap();
        hold
    };
    drop(peer);
    drop(store);
    let mut store = s::Store::open(&path, limits.clone()).unwrap();
    let uncertain = store.trusted_host().intent("nav-recovery").unwrap();
    assert_eq!(uncertain.state, s::IntentState::Uncertain);
    let usage = store
        .trusted_host()
        .reservation(&uncertain.reservation)
        .unwrap();
    assert_eq!(usage.state, s::ReservationState::Uncertain);
    assert!(usage.measured.is_none());
    let before = store.ledger().unwrap();
    assert_eq!(before.ordinary.uncertain.output_bytes, 64);
    assert_eq!(
        before.ordinary.uncertain.navigation_bytes,
        hold.amount.navigation_bytes
    );
    let terminal = {
        let mut mock = attach(&mut store);
        let admission = closeout_access(&mock, &pins);
        let recovered = mock.inspect_closeout(&admission, "nav-recovery").unwrap();
        assert_eq!(recovered.state, m::MockState::EffectUnknown);
        assert_eq!(recovered.observed_mock_executions, Some(1));
        assert!(recovered.terminal.is_none());
        let requested = mock
            .request_cancel(&admission, "nav-recovery", "Stop requested after recovery.")
            .unwrap();
        assert_eq!(requested.state, m::MockState::EffectUnknown);
        assert!(requested.cancel_requested);
        assert!(requested.terminal.is_none());
        let still_unknown = mock.simulate_unknown(&admission, "nav-recovery").unwrap();
        assert_eq!(still_unknown.state, m::MockState::EffectUnknown);
        assert_eq!(mock.ledger().unwrap().ordinary, before.ordinary);
        let terminal = mock
            .simulate_terminal(
                &admission,
                "nav-recovery",
                m::MockOutcome::Completed,
                "Late exact observation.",
            )
            .unwrap()
            .terminal
            .unwrap();
        let admission = closeout_access(&mock, &pins);
        let recovered = mock.inspect_closeout(&admission, "nav-recovery").unwrap();
        assert_eq!(recovered.observed_mock_executions, Some(1));
        assert_eq!(recovered.terminal.unwrap().receipt(), terminal.receipt());
        let after = mock.ledger().unwrap();
        assert_eq!(
            after.ordinary.consumed.attempts,
            before.ordinary.consumed.attempts
        );
        assert_eq!(
            after.ordinary.consumed.navigation_bytes,
            before.ordinary.consumed.navigation_bytes
        );
        assert_eq!(
            after.ordinary.uncertain,
            s::Resources {
                navigation_bytes: hold.amount.navigation_bytes,
                ..s::Resources::default()
            }
        );
        assert!(
            after.closeout.exposure().unwrap().navigation_bytes
                <= limits.protected_closeout.navigation_bytes
        );
        terminal
    };
    let record = store.trusted_host().intent("nav-recovery").unwrap();
    assert!(record.history.starts_with(&uncertain.history));
    assert_eq!(record.history.len(), uncertain.history.len() + 1);
    assert_eq!(record.history.last().unwrap(), terminal.receipt());
    assert_eq!(
        record.reconciliation.as_ref().unwrap().uncertain_receipt,
        uncertain.history.last().unwrap().sequence
    );
    assert_eq!(
        record
            .history
            .iter()
            .filter(|r| r.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
    let navigation = store.trusted_host().reservation(&hold.id).unwrap();
    assert_eq!(navigation.state, s::ReservationState::Uncertain);
    assert!(navigation.measured.is_none());
    assert_eq!(store.ledger().unwrap().ordinary.consumed.attempts, 2);
}

#[test]
fn closeout_navigation_keeps_current_authority_source_and_exact_admission_gates() {
    for changed_id in ["authority", "input-source"] {
        let limits = mock_navigation_limits();
        let (_directory, path, mut store, pins) = database(limits.clone());
        let mut peer = s::Store::open_for_inspection(&path, limits).unwrap();
        let mut mock = attach(&mut store);
        let admitted = submit(&mut mock, &pins, "nav-gates");
        dispatch(&mut mock, &pins, "nav-gates");
        let stale = access(&mock, &pins);
        let old = peer.read(changed_id).unwrap().unwrap();
        let mut changed = old.clone();
        changed.revision += 1;
        changed.body = json!({"mock_only":true,"enabled":false,"exact":"changed"});
        let changed_ref = changed.exact_ref().unwrap();
        peer.compare_and_swap(Some(old.revision), changed).unwrap();
        let new_pins: Vec<_> = pins
            .iter()
            .map(|pin| {
                if pin.id == changed_id {
                    changed_ref.clone()
                } else {
                    pin.clone()
                }
            })
            .collect();
        let exact_request = peer.read_exact(&admitted.request).unwrap();
        let mut newer_request = exact_request.clone();
        newer_request.revision += 1;
        newer_request.body["request"]["inputs"] = json!("different current head");
        peer.compare_and_swap(Some(exact_request.revision), newer_request)
            .unwrap();
        hold_mock_ordinary_navigation(&mut peer);
        let before = mock.ledger().unwrap();
        let denied = m::HostAdmission::from_trusted_host(
            mock.epoch().unwrap(),
            "fixture-actor".into(),
            "fixture-work".into(),
            new_pins.clone(),
            m::Rights::default(),
        );
        assert!(matches!(
            mock.request_cancel(&denied, "nav-gates", "Stop requested."),
            Err(m::MockError::Denied)
        ));
        assert!(matches!(
            mock.simulate_terminal(&denied, "nav-gates", m::MockOutcome::Completed, "Observed."),
            Err(m::MockError::Denied)
        ));
        assert!(matches!(
            mock.inspect_closeout(&denied, "nav-gates"),
            Err(m::MockError::Denied)
        ));
        assert!(matches!(
            mock.simulate_terminal(&stale, "nav-gates", m::MockOutcome::Completed, "Observed."),
            Err(m::MockError::Store(s::StoreError::StaleEpoch { .. }))
        ));
        assert_eq!(mock.ledger().unwrap(), before);
        let old_pins = closeout_access(&mock, &pins);
        assert!(matches!(
            mock.request_cancel(&old_pins, "nav-gates", "Stop requested."),
            Err(m::MockError::Store(s::StoreError::RevisionConflict(_)))
        ));
        for (actor, work) in [
            ("other-actor", "fixture-work"),
            ("fixture-actor", "other-work"),
        ] {
            let foreign = m::HostAdmission::from_trusted_host(
                mock.epoch().unwrap(),
                actor.into(),
                work.into(),
                new_pins.clone(),
                m::Rights {
                    cancel: true,
                    observe: true,
                    ..m::Rights::default()
                },
            );
            assert!(matches!(
                mock.simulate_terminal(
                    &foreign,
                    "nav-gates",
                    m::MockOutcome::Completed,
                    "Observed."
                ),
                Err(m::MockError::Binding(_))
            ));
        }
        let fresh = closeout_access(&mock, &new_pins);
        let pending = mock.inspect_closeout(&fresh, "nav-gates").unwrap();
        assert_eq!(pending.state, m::MockState::Pending);
        assert!(pending.terminal.is_none());
        assert!(!pending.cancel_requested);
        mock.request_cancel(&fresh, "nav-gates", "Stop requested.")
            .unwrap();
        let terminal = mock
            .simulate_terminal(&fresh, "nav-gates", m::MockOutcome::Completed, "Observed.")
            .unwrap()
            .terminal
            .unwrap();
        assert_eq!(
            mock.ledger().unwrap().ordinary.consumed.navigation_bytes,
            before.ordinary.consumed.navigation_bytes
        );
        assert_eq!(
            mock.ledger().unwrap().ordinary.consumed.attempts,
            before.ordinary.consumed.attempts
        );
        assert_eq!(
            peer.trusted_host().read_exact(&admitted.request).unwrap(),
            exact_request
        );
        let evidence = peer.trusted_host().read_exact(terminal.evidence()).unwrap();
        assert_eq!(
            evidence.body["request"],
            serde_json::to_value(&admitted.request).unwrap()
        );
    }
}

#[test]
fn ordinary_admission_dispatch_and_inspection_never_borrow_closeout_navigation() {
    let limits = mock_navigation_limits();
    let (_directory, path, mut store, pins) = database(limits.clone());
    let mut peer = s::Store::open_for_inspection(&path, limits).unwrap();
    let mut mock = attach(&mut store);
    submit(&mut mock, &pins, "nav-admitted");
    hold_mock_ordinary_navigation(&mut peer);
    let (spec, bytes) = proposal(&mock, &pins, "nav-new");
    let admission = access(&mock, &pins);
    let before = mock.ledger().unwrap();
    let cells = retained_mock_cells(&path);
    assert!(matches!(
        mock.submit_fixture(&admission, &spec, &bytes),
        Err(m::MockError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Ordinary,
        }))
    ));
    assert!(matches!(
        mock.dispatch(&admission, "nav-admitted"),
        Err(m::MockError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Ordinary,
        }))
    ));
    assert!(matches!(
        mock.inspect(&admission, "nav-admitted"),
        Err(m::MockError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Ordinary,
        }))
    ));
    assert_eq!(mock.ledger().unwrap(), before);
    assert_eq!(retained_mock_cells(&path), cells);
    let admission = closeout_access(&mock, &pins);
    assert!(matches!(
        mock.inspect_closeout(&admission, "nav-admitted"),
        Err(m::MockError::Binding("closeout requires retained dispatch"))
    ));
    assert!(matches!(
        mock.request_cancel(&admission, "nav-admitted", "Stop requested."),
        Err(m::MockError::Binding("closeout requires retained dispatch"))
    ));
    assert!(matches!(
        mock.simulate_unknown(&admission, "nav-admitted"),
        Err(m::MockError::Binding("closeout requires retained dispatch"))
    ));
    assert!(matches!(
        mock.simulate_terminal(
            &admission,
            "nav-admitted",
            m::MockOutcome::Completed,
            "Observed."
        ),
        Err(m::MockError::Binding("closeout requires retained dispatch"))
    ));
    assert_eq!(mock.ledger().unwrap().ordinary, before.ordinary);
    assert_eq!(retained_mock_cells(&path), cells);
}

#[test]
fn insufficient_closeout_navigation_preserves_unresolved_records_and_all_holds() {
    for unknown_hold in [false, true] {
        let limits = mock_navigation_limits();
        let (_directory, path, mut store, pins) = database(limits.clone());
        let mut peer = s::Store::open_for_inspection(&path, limits.clone()).unwrap();
        let mut mock = attach(&mut store);
        submit(&mut mock, &pins, "nav-refused");
        dispatch(&mut mock, &pins, "nav-refused");
        let admission = closeout_access(&mock, &pins);
        mock.simulate_unknown(&admission, "nav-refused").unwrap();
        let ordinary = hold_mock_ordinary_navigation(&mut peer);
        let report = peer.ledger().unwrap();
        let remaining = limits.total.navigation_bytes
            - report.ordinary.exposure().unwrap().navigation_bytes
            - report.closeout.exposure().unwrap().navigation_bytes;
        let closeout = peer
            .reserve(
                "closeout-navigation-hold",
                s::Resources {
                    navigation_bytes: remaining,
                    ..s::Resources::default()
                },
                s::BudgetClass::Closeout,
            )
            .unwrap();
        if unknown_hold {
            peer.settle(&ordinary.id, s::Usage::Unknown).unwrap();
            peer.settle(&closeout.id, s::Usage::Unknown).unwrap();
        }
        let before = mock.ledger().unwrap();
        let cells = retained_mock_cells(&path);
        let epoch = mock.epoch().unwrap();
        let admission = closeout_access(&mock, &pins);
        assert!(matches!(
            mock.request_cancel(&admission, "nav-refused", "Stop requested."),
            Err(m::MockError::Store(s::StoreError::Quota {
                dimension: s::Resource::NavigationBytes,
                class: s::BudgetClass::Closeout,
            }))
        ));
        assert!(matches!(
            mock.simulate_terminal(
                &admission,
                "nav-refused",
                m::MockOutcome::Completed,
                "Late observation."
            ),
            Err(m::MockError::Store(s::StoreError::Quota {
                dimension: s::Resource::NavigationBytes,
                class: s::BudgetClass::Closeout,
            }))
        ));
        assert!(matches!(
            mock.inspect_closeout(&admission, "nav-refused"),
            Err(m::MockError::Store(s::StoreError::Quota {
                dimension: s::Resource::NavigationBytes,
                class: s::BudgetClass::Closeout,
            }))
        ));
        assert_eq!(mock.epoch().unwrap(), epoch);
        assert_eq!(mock.ledger().unwrap(), before);
        assert_eq!(retained_mock_cells(&path), cells);
        drop(mock);
        drop(peer);
        drop(store);
        let store = s::Store::open(&path, limits).unwrap();
        assert_eq!(store.ledger().unwrap(), before);
        assert_eq!(retained_mock_cells(&path), cells);
    }
}

#[test]
fn navigation_failure_after_cancellation_uncertainty_keeps_unapplied_evidence() {
    let limits = mock_navigation_limits();
    let (_directory, path, mut store, pins) = database(limits.clone());
    let mut peer = s::Store::open_for_inspection(&path, limits.clone()).unwrap();
    let mut mock = attach(&mut store);
    let admitted = submit(&mut mock, &pins, "nav-partial");
    dispatch(&mut mock, &pins, "nav-partial");
    let admission = closeout_access(&mock, &pins);
    mock.request_cancel(&admission, "nav-partial", "Stop requested.")
        .unwrap();

    // Leave exactly enough navigation for the initial current prerequisites,
    // retained intent/admission, and three slots. The subsequent intent read
    // must refuse after the cancellation transition records uncertainty.
    let mut initial_bytes = 0u64;
    for pin in &pins {
        let current = peer.read(&pin.id).unwrap();
        initial_bytes += serde_json::to_vec(&current).unwrap().len() as u64;
    }
    let dispatched = peer.intent("nav-partial").unwrap();
    initial_bytes += serde_json::to_vec(&dispatched).unwrap().len() as u64;
    let request = peer.read_exact(&admitted.request).unwrap();
    initial_bytes += serde_json::to_vec(&request).unwrap().len() as u64;
    let subject = s::EntityId::Intent("nav-partial".into());
    for kind in ["started", "cancel", "terminal"] {
        let id = format!("mock-{kind}:{}", s::sha256_bytes(b"nav-partial"));
        let slot = peer
            .trusted_host()
            .read_terminal_evidence(&subject, &id)
            .unwrap();
        initial_bytes += serde_json::to_vec(&slot).unwrap().len() as u64;
    }
    hold_mock_ordinary_navigation(&mut peer);
    let report = peer.ledger().unwrap();
    let remaining = limits.total.navigation_bytes
        - report.ordinary.exposure().unwrap().navigation_bytes
        - report.closeout.exposure().unwrap().navigation_bytes;
    assert!(remaining > initial_bytes);
    peer.reserve(
        "closeout-navigation-hold",
        s::Resources {
            navigation_bytes: remaining - initial_bytes,
            ..s::Resources::default()
        },
        s::BudgetClass::Closeout,
    )
    .unwrap();
    let before = mock.ledger().unwrap();
    let epoch = mock.epoch().unwrap();
    let admission = closeout_access(&mock, &pins);
    assert!(matches!(
        mock.simulate_terminal(
            &admission,
            "nav-partial",
            m::MockOutcome::CanceledToObservedExtent,
            "Observed stopping extent."
        ),
        Err(m::MockError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Closeout,
        }))
    ));
    assert_eq!(mock.epoch().unwrap(), epoch + 1);
    let after = mock.ledger().unwrap();
    assert_eq!(
        after.closeout.consumed.navigation_bytes,
        before.closeout.consumed.navigation_bytes + initial_bytes
    );
    assert_eq!(
        after.ordinary.consumed.navigation_bytes,
        before.ordinary.consumed.navigation_bytes
    );
    assert_eq!(
        after.ordinary.consumed.attempts,
        before.ordinary.consumed.attempts
    );
    assert_eq!(
        after.ordinary.uncertain.input_bytes,
        dispatched.intent.reservation.input_bytes
    );
    assert_eq!(after.ordinary.uncertain.output_bytes, 64);
    assert_eq!(after.ordinary.reserved.input_bytes, 0);
    assert_eq!(after.ordinary.reserved.output_bytes, 0);
    let cells = retained_mock_cells(&path);
    let record: s::IntentRecord = serde_json::from_slice(
        &cells
            .iter()
            .find(|(ns, key, _)| ns == "intent" && key == "nav-partial")
            .unwrap()
            .2,
    )
    .unwrap();
    assert_eq!(record.state, s::IntentState::Uncertain);
    assert!(record.reconciliation.is_none());
    assert!(record.history.starts_with(&dispatched.history));
    assert_eq!(record.history.len(), dispatched.history.len() + 1);
    assert_eq!(
        record.history.last().unwrap().status,
        s::ReceiptStatus::Uncertain
    );
    let reservation: s::ReservationRecord = serde_json::from_slice(
        &cells
            .iter()
            .find(|(ns, key, _)| ns == "reservation" && key == "i:nav-partial")
            .unwrap()
            .2,
    )
    .unwrap();
    assert_eq!(reservation.state, s::ReservationState::Uncertain);
    assert!(reservation.measured.is_none());
    let terminal_id = format!("mock-terminal:{}", s::sha256_bytes(b"nav-partial"));
    let slot: Value = serde_json::from_slice(
        &cells
            .iter()
            .find(|(ns, key, _)| ns == "terminal_evidence" && key == &terminal_id)
            .unwrap()
            .2,
    )
    .unwrap();
    let evidence: s::StoredObject = serde_json::from_value(slot["object"].clone()).unwrap();
    assert_eq!(
        evidence.body["request"],
        serde_json::to_value(&admitted.request).unwrap()
    );
    assert_eq!(evidence.body["outcome"], "canceled_to_observed_extent");
    assert_eq!(evidence.body["result"], "Observed stopping extent.");
    assert!(evidence.body["cancellation"].is_object());
    let admission = closeout_access(&mock, &pins);
    assert!(matches!(
        mock.inspect_closeout(&admission, "nav-partial"),
        Err(m::MockError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Closeout,
        }))
    ));
    assert!(matches!(
        mock.simulate_terminal(
            &admission,
            "nav-partial",
            m::MockOutcome::CanceledToObservedExtent,
            "Observed stopping extent."
        ),
        Err(m::MockError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Closeout,
        }))
    ));
    assert_eq!(mock.ledger().unwrap(), after);
    assert_eq!(retained_mock_cells(&path), cells);
    assert_eq!(mock.epoch().unwrap(), epoch + 1);
}

#[test]
fn terminal_persistence_failure_is_inspectable_and_exact_retry_only_finishes_observation() {
    let (_directory, path, mut store, pins) = database(executable_fixture::limits());
    let fault = rusqlite::Connection::open(&path).unwrap();
    let mut mock = attach(&mut store);
    submit(&mut mock, &pins, "terminal-fault");
    dispatch(&mut mock, &pins, "terminal-fault");
    fault
        .execute_batch(
            "CREATE TRIGGER mock_terminal_fault BEFORE UPDATE ON store_cells
         WHEN NEW.ns='intent' AND NEW.k='terminal-fault'
           AND json_extract(NEW.data,'$.state')='Succeeded'
         BEGIN SELECT RAISE(ABORT,'injected mock terminal transition'); END;",
        )
        .unwrap();
    let admission = access(&mock, &pins);
    let before = mock.ledger().unwrap().ordinary.reserved;
    let error = mock
        .simulate_terminal(
            &admission,
            "terminal-fault",
            m::MockOutcome::Completed,
            "retained exact outcome",
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("injected mock terminal transition"));
    let view = inspect(&mut mock, &pins, "terminal-fault");
    assert_eq!(view.state, m::MockState::Pending);
    assert!(view.terminal.is_none());
    assert!(view.unapplied_terminal.is_some());
    assert_eq!(mock.ledger().unwrap().ordinary.reserved, before);
    assert_eq!(
        dispatch(&mut mock, &pins, "terminal-fault").observed_mock_executions,
        Some(1)
    );
    let admission = access(&mock, &pins);
    assert!(mock
        .simulate_terminal(
            &admission,
            "terminal-fault",
            m::MockOutcome::Completed,
            "different bytes"
        )
        .is_err());
    fault
        .execute_batch("DROP TRIGGER mock_terminal_fault")
        .unwrap();
    let finished = finish(
        &mut mock,
        &pins,
        "terminal-fault",
        m::MockOutcome::Completed,
        "retained exact outcome",
    );
    assert_eq!(finished.state, m::MockState::Completed);
    assert_eq!(finished.observed_mock_executions, Some(1));
    assert_eq!(
        finished.terminal.unwrap().evidence(),
        view.unapplied_terminal.as_ref().unwrap()
    );
}
