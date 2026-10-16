//! Stage 1 fixtures: protected temporary Stores and in-process synthetic HTTPS.
//! No environment credential reads, network, sign-in, external binaries or grants.
use super::*;
use futures_util::future::BoxFuture;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const OP: &str = "fixture-human";
fn value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}
fn fixture() -> (tempfile::TempDir, Runtime) {
    let tmp = tempfile::tempdir().unwrap();
    let working = tmp.path().join("working");
    fs::create_dir(&working).unwrap();
    let settings = serde_json::from_value(json!({
        "mode":"platform_api_key","model":"fixture-model","effort":"high",
        "credentials":{"kind":"api_key","key_env":{"env":"FIXTURE_UNUSED_KEY"},"account_env":{"env":"FIXTURE_UNUSED_ACCOUNT"}},
        "evidence":{"mode":"platform_api_key","model":"fixture-model","efforts":["high"],
            "account_env":{"env":"FIXTURE_UNUSED_ACCOUNT"},"account_sha256":s::sha256_bytes(b"fixture-account"),
            "transport_profile":"platform_responses_v2","catalog_evidence_sha256":"1".repeat(64),
            "structured_output_evidence_sha256":"2".repeat(64)},
        "limits":{"timeout_ms":1000,"max_prompt_bytes":196608,"max_schema_bytes":65536,
            "max_schema_nodes":10000,"max_schema_depth":64,"max_request_bytes":400000,
            "max_response_bytes":32768,"max_stream_bytes":1048576,"max_event_bytes":131072,"max_events":10000}
    })).unwrap();
    let app = setup(
        &tmp.path().join("state"),
        OP,
        Setup {
            cohort: 2,
            owner: 1,
            destination: "fixture-local".into(),
            provider: settings,
            working_directory: working,
            share_cohort: true,
            max_steps: 1,
            max_considerations: 8,
            max_work_attempts: 4,
            max_total_steps: 8,
            max_works: 4,
        },
        true,
    )
    .unwrap();
    (tmp, app)
}
fn need(key: &str) -> NeedRequest {
    NeedRequest {
        id: key.into(),
        operator: OP.into(),
        need: "Exact human request: café\n'\"; $(not-executed)".into(),
        require_acceptance: false,
    }
}
fn admit(app: &mut Runtime) -> String {
    intake_opened(app, &need("need-1")).unwrap().receipt.work
}
fn run_request(work: &str, id: &str) -> SessionRequest {
    SessionRequest {
        id: id.into(),
        operator: OP.into(),
        work: work.into(),
        route: SessionRoute::SavedEnvironment,
    }
}
fn new_claim(app: &mut Runtime, request: &SessionRequest) -> Claimed {
    match begin(app, request).unwrap() {
        Begin::New(c) => c,
        Begin::Existing(_) => panic!("fixture expected a fresh claim"),
    }
}
fn heads(app: &mut Runtime) -> (Value, Value) {
    (value(&app.load().unwrap().0), value(&load(app).unwrap().0))
}
fn assert_funding_preserved(before: &s::LedgerReport, after: &s::LedgerReport) {
    assert_eq!(before.limits, after.limits);
    assert_eq!(before.frozen, after.frozen);
    assert_eq!(before.ordinary.reserved, after.ordinary.reserved);
    assert_eq!(before.ordinary.uncertain, after.ordinary.uncertain);
    assert_eq!(before.closeout, after.closeout);
    assert_eq!(
        before.ordinary.consumed.attempts,
        after.ordinary.consumed.attempts
    );
    assert_eq!(
        before.ordinary.consumed.input_bytes,
        after.ordinary.consumed.input_bytes
    );
    assert_eq!(
        before.ordinary.consumed.output_bytes,
        after.ordinary.consumed.output_bytes
    );
    // Ordinary metered navigation and receipt retention may increase, never reset.
    assert!(after.ordinary.consumed.navigation_bytes >= before.ordinary.consumed.navigation_bytes);
}

#[test]
fn keyed_intake_lost_response_returns_exact_receipt_after_restart() {
    let (tmp, mut app) = fixture();
    let first = intake_opened(&mut app, &need("need-1")).unwrap();
    let expected_heads = heads(&mut app);
    let before = app.ledger().unwrap();
    drop(app);
    let repeated = intake(&tmp.path().join("state"), &need("need-1")).unwrap();
    assert!(repeated.replayed);
    assert_eq!(first.receipt, repeated.receipt);
    let mut app = open(&tmp.path().join("state"), OP).unwrap();
    assert_eq!(heads(&mut app), expected_heads);
    assert_funding_preserved(&before, &app.ledger().unwrap());
    assert_eq!(app.snapshot().unwrap().work_meta.len(), 1);
}
#[test]
fn rebound_need_or_acceptance_key_is_rejected_without_mutation() {
    let (_tmp, mut app) = fixture();
    intake_opened(&mut app, &need("need-1")).unwrap();
    let before = heads(&mut app);
    let mut altered = need("need-1");
    altered.need.push(' ');
    assert_eq!(
        intake_opened(&mut app, &altered).unwrap_err(),
        BoundaryError::KeyConflict
    );
    altered = need("need-1");
    altered.require_acceptance = true;
    assert_eq!(
        intake_opened(&mut app, &altered).unwrap_err(),
        BoundaryError::KeyConflict
    );
    altered = need("need-1");
    altered.operator = "other-human".into();
    assert_eq!(
        intake_opened(&mut app, &altered).unwrap_err(),
        BoundaryError::Unauthorized
    );
    assert_eq!(heads(&mut app), before);
}
#[test]
fn intake_capacity_failure_leaves_no_key_or_partial_work() {
    let (_tmp, mut app) = fixture();
    let (head, mut saved) = load(&mut app).unwrap();
    saved.setup.max_works = 1;
    save(&mut app, Some(&head), &saved).unwrap();
    let first = intake_opened(&mut app, &need("need-1")).unwrap();
    let before = heads(&mut app);
    assert!(intake_opened(&mut app, &need("need-2")).is_err());
    assert!(app
        .store
        .read(&record_id(NEED_KIND, "need-2"))
        .unwrap()
        .is_none());
    assert_eq!(heads(&mut app), before);
    assert_eq!(
        intake_opened(&mut app, &need("need-1")).unwrap().receipt,
        first.receipt
    );
}
#[test]
fn concurrent_owner_excludes_intake_and_session_but_not_inspection() {
    let (tmp, mut app) = fixture();
    let work = admit(&mut app);
    let before = heads(&mut app);
    let owner = app
        .store
        .try_execution_owner_without_recovery()
        .unwrap()
        .unwrap();
    let mut other = open(&tmp.path().join("state"), OP).unwrap();
    assert_eq!(
        intake_opened(&mut other, &need("need-2")).unwrap_err(),
        BoundaryError::OwnerBusy
    );
    assert!(matches!(
        begin(&mut other, &run_request(&work, "run-1")),
        Err(BoundaryError::OwnerBusy)
    ));
    assert_eq!(
        inspect_opened(&mut other, OP, None, 1).unwrap().works.len(),
        1
    );
    assert_eq!(heads(&mut app), before);
    drop(owner);
    let claim = new_claim(&mut other, &run_request(&work, "run-1"));
    drop(claim);
}
#[tokio::test]
async fn claimed_session_without_terminal_is_never_replayed() {
    let (tmp, mut app) = fixture();
    let work = admit(&mut app);
    let request = run_request(&work, "run-1");
    drop(new_claim(&mut app, &request));
    let before = heads(&mut app);
    let ledger = app.ledger().unwrap();
    drop(app);
    // The public production entry must short-circuit before even constructing
    // the environment provider. No credentials exist in these fixtures.
    let result = run_session(&tmp.path().join("state"), &request)
        .await
        .unwrap();
    assert!(matches!(result, SessionView::OutcomeUnrecorded { .. }));
    let mut app = open(&tmp.path().join("state"), OP).unwrap();
    assert_eq!(heads(&mut app), before);
    assert_funding_preserved(&ledger, &app.ledger().unwrap());
    let mut changed = request;
    changed.work = "different-work".into();
    assert!(matches!(
        begin(&mut app, &changed),
        Err(BoundaryError::KeyConflict)
    ));
}
#[tokio::test]
async fn terminal_result_survives_lost_stdout_without_driver_reentry() {
    let (tmp, mut app) = fixture();
    let work = admit(&mut app);
    let request = run_request(&work, "run-1");
    let claim = new_claim(&mut app, &request);
    let first = finish(
        &mut app,
        &claim,
        SessionEnd::Returned {
            stop: Some("no_eligible_route".into()),
            driver_failure: None,
            steps_after: 0,
            last_invocation: None,
        },
    )
    .unwrap();
    let before = heads(&mut app);
    drop(claim);
    drop(app);
    let replay = run_session(&tmp.path().join("state"), &request)
        .await
        .unwrap();
    assert_eq!(value(&first), value(&replay));
    let mut app = open(&tmp.path().join("state"), OP).unwrap();
    assert_eq!(heads(&mut app), before);
}

struct Source;
impl provider::CredentialSource for Source {
    fn load(
        &self,
        _: &provider::CredentialReference,
        _: &provider::SecretReference,
    ) -> std::result::Result<provider::CredentialBundle, provider::Failure> {
        Ok(provider::CredentialBundle {
            credential: provider::Credential::ApiKey {
                key: provider::Secret::new("DO_NOT_DISCLOSE_SYNTHETIC_SECRET".into()),
                account: provider::Secret::new("fixture-account".into()),
            },
            evidence_account: provider::Secret::new("fixture-account".into()),
        })
    }
}
#[derive(Clone)]
enum TransportMode {
    RejectedDecision,
    Pending(Arc<tokio::sync::Notify>),
    LocalFailure,
}
struct Transport {
    calls: Arc<AtomicUsize>,
    mode: TransportMode,
}
struct Body(Option<Vec<u8>>);
impl provider::HttpBody for Body {
    fn read(
        &mut self,
        maximum: usize,
    ) -> BoxFuture<'_, std::result::Result<Option<Vec<u8>>, provider::Failure>> {
        Box::pin(async move {
            match self.0.take() {
                Some(mut bytes) if bytes.len() > maximum => {
                    let tail = bytes.split_off(maximum);
                    self.0 = Some(tail);
                    Ok(Some(bytes))
                }
                value => Ok(value),
            }
        })
    }
}
impl provider::HttpTransport for Transport {
    fn post(
        &self,
        _: provider::HttpRequest,
    ) -> BoxFuture<'_, std::result::Result<provider::HttpResponse, provider::Failure>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.mode {
                TransportMode::Pending(entered) => {
                    // Signal only after this transport future is actually polled.
                    // notify_one retains a permit even if the waiter polls later.
                    entered.notify_one();
                    std::future::pending().await
                }
                TransportMode::LocalFailure => Err(provider::Failure::Network),
                TransportMode::RejectedDecision => {
                    let carrier = json!({"decision_json":"{}"}).to_string();
                    let created = json!({"type":"response.created","response":{"id":"resp_fixture","model":"fixture-model","status":"in_progress","output":[]}});
                    let completed = json!({"type":"response.completed","response":{"id":"resp_fixture","model":"fixture-model","status":"completed","output":[
                        {"id":"message_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":carrier}]}]}});
                    Ok(provider::HttpResponse {
                        status: 200,
                        is_unencoded_sse: true,
                        body: Box::new(Body(Some(
                            format!("data: {created}\n\ndata: {completed}\n\n").into_bytes(),
                        ))),
                    })
                }
            }
        })
    }
}
// Exact production OwnedLive, DirectProvider and Runtime admission/dispatch;
// only the CredentialSource and HTTPS transport are synthetic fixtures.
async fn fixture_run(
    app: &mut Runtime,
    request: &SessionRequest,
    calls: Arc<AtomicUsize>,
    mode: TransportMode,
) -> BoundaryResult<SessionView> {
    let claim = match begin(app, request)? {
        Begin::Existing(view) => return Ok(*view),
        Begin::New(claim) => claim,
    };
    let (_, saved) = load(app)?;
    let (head, _) = app.load()?;
    let provider = provider::DirectProvider::new(
        saved.execution_provider()?,
        Arc::new(Source),
        Arc::new(Transport { calls, mode }),
        Arc::new(provider::SystemClock),
    )
    .map_err(RuntimeError::from)?;
    let mut driver = OwnedLive {
        owner: &claim.owner,
        provider,
        binding: execution_binding(&saved)?,
        config: head.body["config"].clone(),
    };
    let result = drive(app, &request.work, &mut driver).await;
    let end = end_of_run(app, &request.work, result)?;
    finish(app, &claim, end)
}
#[tokio::test]
async fn rejected_complete_response_retains_hold_and_never_dispatches_twice() {
    let (tmp, mut app) = fixture();
    let work = admit(&mut app);
    let request = run_request(&work, "run-1");
    let calls = Arc::new(AtomicUsize::new(0));
    let first = fixture_run(
        &mut app,
        &request,
        calls.clone(),
        TransportMode::RejectedDecision,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let inspection = inspect_opened(&mut app, OP, None, 1).unwrap();
    assert_eq!(
        inspection.works[0].last_attempt.as_ref().unwrap().status,
        AttemptStatus::CompleteResponseRejected
    );
    assert!(inspection.active_invocations_present);
    assert_eq!(
        inspection.works[0]
            .last_attempt
            .as_ref()
            .unwrap()
            .reservation_state,
        s::ReservationState::Uncertain
    );
    assert!(!value(&inspection)
        .to_string()
        .contains("DO_NOT_DISCLOSE_SYNTHETIC_SECRET"));
    let before = heads(&mut app);
    let ledger = app.ledger().unwrap();
    drop(app);
    let mut app = open(&tmp.path().join("state"), OP).unwrap();
    assert_eq!(
        value(
            &fixture_run(
                &mut app,
                &request,
                calls.clone(),
                TransportMode::RejectedDecision
            )
            .await
            .unwrap()
        ),
        value(&first)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(heads(&mut app), before);
    assert_funding_preserved(&ledger, &app.ledger().unwrap());
}
#[tokio::test]
async fn abandoned_dispatched_session_inspection_does_not_recover_or_replay() {
    // The deadline bounds the async test, not the time needed to reach dispatch.
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let (tmp, mut app) = fixture();
        let work = admit(&mut app);
        let request = run_request(&work, "run-1");
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let mode = TransportMode::Pending(entered.clone());
        let mut running = Box::pin(fixture_run(&mut app, &request, calls.clone(), mode.clone()));
        tokio::select! {
            biased;
            _ = entered.notified() => {},
            result = &mut running => panic!("fixture run ended before cancellation: {result:?}"),
        }
        // Cancel by dropping the whole run only after observing actual transport
        // entry. This releases its Store owner without fabricating a terminal.
        drop(running);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let inv = load(&mut app).unwrap().1.works[&work].clone().unwrap();
        assert_eq!(
            app.invocation(&inv).unwrap().state,
            s::InvocationState::Dispatched
        );
        let before = heads(&mut app);
        let ledger = app.ledger().unwrap();
        drop(app);
        let mut app = open(&tmp.path().join("state"), OP).unwrap();
        let view = inspect_opened(&mut app, OP, None, 1).unwrap();
        assert_eq!(
            view.works[0].last_attempt.as_ref().unwrap().status,
            AttemptStatus::DispatchedUnresolved
        );
        assert!(matches!(
            fixture_run(&mut app, &request, calls.clone(), mode.clone())
                .await
                .unwrap(),
            SessionView::OutcomeUnrecorded { .. }
        ));
        assert!(matches!(
            begin(&mut app, &run_request(&work, "fresh-key")),
            Err(BoundaryError::Blocked)
        ));
        assert_eq!(
            app.invocation(&inv).unwrap().state,
            s::InvocationState::Dispatched
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(heads(&mut app), before);
        assert_funding_preserved(&ledger, &app.ledger().unwrap());
        // Simulate an independently performed ordinary recovery, then verify the
        // new inspection recognizes (and does not remove) that retained uncertainty.
        drop(app);
        let mut app = Runtime::open(&tmp.path().join("state")).unwrap();
        let recovered = inspect_opened(&mut app, OP, None, 1).unwrap();
        assert_eq!(
            recovered.works[0].last_attempt.as_ref().unwrap().status,
            AttemptStatus::Uncertain
        );
        assert!(matches!(
            fixture_run(&mut app, &request, calls.clone(), mode)
                .await
                .unwrap(),
            SessionView::OutcomeUnrecorded { .. }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    })
    .await
    .expect("abandoned-dispatch fixture exceeded its overall async deadline");
}
#[tokio::test]
async fn observed_transport_failure_is_not_a_complete_response_or_refund() {
    let (_tmp, mut app) = fixture();
    let work = admit(&mut app);
    let calls = Arc::new(AtomicUsize::new(0));
    let request = run_request(&work, "run-1");
    fixture_run(
        &mut app,
        &request,
        calls.clone(),
        TransportMode::LocalFailure,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let view = inspect_opened(&mut app, OP, None, 1).unwrap();
    let attempt = view.works[0].last_attempt.as_ref().unwrap();
    assert_eq!(attempt.status, AttemptStatus::FailedAttempt);
    assert!(attempt.response_hash.is_none());
    assert_eq!(attempt.reservation_state, s::ReservationState::Uncertain);
    let ledger = app.ledger().unwrap();
    fixture_run(
        &mut app,
        &request,
        calls.clone(),
        TransportMode::LocalFailure,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_funding_preserved(&ledger, &app.ledger().unwrap());
}
#[tokio::test]
async fn persona_stop_and_work_pause_are_distinct_and_not_resumed() {
    let (_tmp, mut app) = fixture();
    let work = admit(&mut app);
    let (head, mut state) = app.load().unwrap();
    for persona in state.personas.values_mut() {
        let focus = persona.focus.get_mut(&work).unwrap();
        focus.permit = false;
        focus.ordinary_continuation = false;
        focus.continuation = Some(p::Continuation::Stop {
            reason: "synthetic persona choice".into(),
        });
    }
    app.save(&head, &state).unwrap();
    let before = heads(&mut app).0;
    let calls = Arc::new(AtomicUsize::new(0));
    fixture_run(
        &mut app,
        &run_request(&work, "run-1"),
        calls.clone(),
        TransportMode::RejectedDecision,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(heads(&mut app).0, before);
    let view = inspect_opened(&mut app, OP, None, 1).unwrap();
    assert_eq!(view.works[0].phase, WorkPhase::Open);
    assert!(view.works[0]
        .participants
        .iter()
        .all(|p| p.continuation == ContinuationStatus::PersonaStop && !p.eligible_candidate));
    app.pause(OP, &work, "explicit fixture pause").unwrap();
    let before = heads(&mut app).0;
    fixture_run(
        &mut app,
        &run_request(&work, "run-2"),
        calls.clone(),
        TransportMode::RejectedDecision,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(heads(&mut app).0, before);
    assert_eq!(
        inspect_opened(&mut app, OP, None, 1).unwrap().works[0].phase,
        WorkPhase::Paused
    );
}
#[test]
fn inspection_is_bounded_authorized_and_marks_saved_expiry_without_credentials() {
    let (_tmp, mut app) = fixture();
    admit(&mut app);
    intake_opened(&mut app, &need("need-2")).unwrap();
    assert!(matches!(
        inspect_opened(&mut app, OP, None, 0),
        Err(BoundaryError::InvalidRequest)
    ));
    assert!(matches!(
        inspect_opened(&mut app, "other-human", None, 1),
        Err(BoundaryError::Unauthorized)
    ));
    let view = inspect_opened(&mut app, OP, None, 1).unwrap();
    assert_eq!(view.works.len(), 1);
    assert!(view.next_after.is_some());
    assert!(view.works[0]
        .participants
        .iter()
        .any(|p| p.eligible_candidate));
    let (head, mut saved) = load(&mut app).unwrap();
    saved.setup.provider["mode"] = json!("siwc_oauth");
    saved.setup.provider["credentials"] = value(&provider::CredentialReference::OAuth {
        grant: provider::OAuthGrant::SiwcTokenSharing,
        token_env: provider::SecretReference {
            env: "FIXTURE_UNUSED_TOKEN".into(),
        },
        account_env: provider::SecretReference {
            env: "FIXTURE_UNUSED_ACCOUNT".into(),
        },
        expires_at_unix: 1,
    });
    saved.setup.provider["evidence"]["mode"] = json!("siwc_oauth");
    saved.setup.provider["evidence"]["transport_profile"] =
        json!("siwc_documented_public_responses_v2");
    saved.execution_provider().unwrap();
    save(&mut app, Some(&head), &saved).unwrap();
    let before = heads(&mut app);
    let view = inspect_opened(&mut app, OP, None, 1).unwrap();
    assert_eq!(
        view.settings.credential_status,
        CredentialStatus::SavedExpiryElapsed
    );
    assert!(!view.settings.credential_resolved);
    assert!(!view.settings.capability_verified);
    assert_eq!(heads(&mut app), before);
}
#[test]
fn malformed_receipts_and_invalid_keys_fail_closed() {
    let (tmp, mut app) = fixture();
    let work = admit(&mut app);
    let request = run_request(&work, "run-1");
    app.store
        .compare_and_swap_batch(vec![s::Change {
            expected_revision: None,
            object: s::StoredObject {
                id: record_id(CLAIM_KIND, &request.id),
                revision: 1,
                kind: CLAIM_KIND.into(),
                body: json!({"version":99,"untrusted":"DO_NOT_ECHO"}),
            },
            depends_on: vec![],
        }])
        .unwrap();
    let before = heads(&mut app);
    assert!(matches!(
        begin(&mut app, &request),
        Err(BoundaryError::InvalidState)
    ));
    assert_eq!(heads(&mut app), before);
    let mut bad = need("bad key");
    assert!(matches!(
        intake(&tmp.path().join("absent"), &bad),
        Err(BoundaryError::InvalidRequest)
    ));
    bad.id = "valid-key".into();
    bad.need = " ".into();
    assert!(matches!(
        intake(&tmp.path().join("absent"), &bad),
        Err(BoundaryError::InvalidRequest)
    ));
    assert!(!tmp.path().join("absent").exists());
    let encoded = value(&error_code(&RuntimeError::Invalid("DO_NOT_ECHO".into()))).to_string();
    assert!(!encoded.contains("DO_NOT_ECHO"));
}
#[test]
fn frozen_terminal_write_keeps_claim_unknown_and_does_not_replenish() {
    let (_tmp, mut app) = fixture();
    let work = admit(&mut app);
    let request = run_request(&work, "run-1");
    let claim = new_claim(&mut app, &request);
    // Finite Store overrun fixture, never mutate real metadata or forgive usage.
    let id = s::ReservationId::standalone("fixture-overrun");
    app.store
        .reserve(
            "fixture-overrun",
            s::Resources {
                attempts: 1,
                ..Default::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    let _ = app.store.settle(
        &id,
        s::Usage::Measured(s::Resources {
            attempts: 2,
            ..Default::default()
        }),
    );
    assert!(app.ledger().unwrap().frozen);
    let before = app.ledger().unwrap();
    assert_eq!(
        finish(
            &mut app,
            &claim,
            SessionEnd::Failed {
                code: BoundaryError::Blocked
            }
        )
        .unwrap_err(),
        BoundaryError::TerminalUnconfirmed
    );
    assert!(matches!(
        read_session(&mut app, OP, &request.id).unwrap(),
        SessionView::OutcomeUnrecorded { .. }
    ));
    assert_funding_preserved(&before, &app.ledger().unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn native_expiry_failure_is_retained_without_opening_vault_or_replay() {
    let (tmp, mut app) = fixture();
    let work = admit(&mut app);
    let (head, mut saved) = load(&mut app).unwrap();
    saved.setup.provider["mode"] = json!("siwc_oauth");
    saved.setup.provider["credentials"] = value(&provider::CredentialReference::OAuth {
        grant: provider::OAuthGrant::SiwcTokenSharing,
        token_env: provider::SecretReference {
            env: crate::auth::ACCESS_TOKEN_ENV.into(),
        },
        account_env: provider::SecretReference {
            env: crate::auth::ACCOUNT_BINDING_ENV.into(),
        },
        expires_at_unix: 1,
    });
    saved.setup.provider["evidence"]["mode"] = json!("siwc_oauth");
    saved.setup.provider["evidence"]["account_env"] =
        json!({"env":crate::auth::ACCOUNT_BINDING_ENV});
    saved.setup.provider["evidence"]["transport_profile"] =
        json!("siwc_documented_public_responses_v2");
    saved.execution_provider().unwrap();
    save(&mut app, Some(&head), &saved).unwrap();
    let before = heads(&mut app);
    drop(app);
    let vault = tmp.path().join("not-created-vault");
    let mut request = run_request(&work, "native-1");
    request.route = SessionRoute::NativeSiwc {
        vault: vault.clone(),
    };
    let first = run_session(&tmp.path().join("state"), &request)
        .await
        .unwrap();
    assert!(matches!(&first, SessionView::Finished { terminal, .. }
        if terminal.outcome == (SessionEnd::Failed { code: BoundaryError::CredentialExpired })));
    assert!(!vault.exists());
    let replay = run_session(&tmp.path().join("state"), &request)
        .await
        .unwrap();
    assert_eq!(value(&first), value(&replay));
    let mut app = open(&tmp.path().join("state"), OP).unwrap();
    assert_eq!(heads(&mut app), before);
    request.route = SessionRoute::SavedEnvironment;
    assert!(matches!(
        begin(&mut app, &request),
        Err(BoundaryError::KeyConflict)
    ));
}
