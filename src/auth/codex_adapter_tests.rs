//! Synthetic DirectProvider integration tests; no real credentials or network.
use super::*;
use futures_util::future::BoxFuture;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Clone)]
struct QueueSource {
    statuses: Arc<Mutex<VecDeque<Result<AuthStatusSnapshot, p::Failure>>>>,
    calls: Arc<AtomicUsize>,
    cleanup_complete: Arc<std::sync::atomic::AtomicBool>,
}
impl QueueSource {
    fn new(statuses: Vec<AuthStatusSnapshot>) -> Self {
        Self {
            statuses: Arc::new(Mutex::new(statuses.into_iter().map(Ok).collect())),
            calls: Arc::new(AtomicUsize::new(0)),
            cleanup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}
impl AuthorizedAuthStatusSource for QueueSource {
    fn acquire_bounded(&self, timeout: std::time::Duration) -> Result<AuthStatusSnapshot, p::Failure> {
        assert_eq!(timeout, std::time::Duration::from_secs(5));
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.statuses.lock().unwrap().pop_front().ok_or(p::Failure::CredentialMissing)?;
        if matches!(&result, Err(p::Failure::Timeout)) {
            // Synthetic source confirms cleanup before returning timeout.
            self.cleanup_complete.store(true, Ordering::SeqCst);
        }
        result
    }
}

struct FixedClock(AtomicU64);
impl p::Clock for FixedClock {
    fn unix_seconds(&self) -> Result<u64, p::Failure> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Body(VecDeque<Vec<u8>>);
impl p::HttpBody for Body {
    fn read(&mut self, maximum: usize) -> BoxFuture<'_, Result<Option<Vec<u8>>, p::Failure>> {
        Box::pin(async move {
            let Some(mut chunk) = self.0.pop_front() else { return Ok(None) };
            if chunk.len() > maximum {
                let remainder = chunk.split_off(maximum);
                self.0.push_front(remainder);
            }
            Ok(Some(chunk))
        })
    }
}

struct Transport {
    posts: AtomicUsize,
    sse: Vec<u8>,
}
impl p::HttpTransport for Transport {
    fn post(&self, request: p::HttpRequest) -> BoxFuture<'_, Result<p::HttpResponse, p::Failure>> {
        self.posts.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.mode, p::AccessMode::ChatgptOAuth);
        assert_eq!(
            request.account_header.as_ref().map(p::Secret::expose),
            Some("synthetic-account")
        );
        let body = Body(self.sse.chunks(37).map(|chunk| chunk.to_vec()).collect());
        Box::pin(async move {
            Ok(p::HttpResponse { status: 200, is_unencoded_sse: true, body: Box::new(body) })
        })
    }
}

fn synthetic_token(account: &str, exp: i64) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
        "exp": exp,
        "https://api.openai.com/auth": {"chatgpt_account_id": account}
    })).unwrap());
    format!("{header}.{payload}.synthetic-signature")
}
fn status(account: &str, exp: i64) -> AuthStatusSnapshot {
    AuthStatusSnapshot {
        auth_method: Some(CodexAuthMethod::Chatgpt),
        auth_token: Some(synthetic_token(account, exp)),
    }
}
fn reference() -> p::CredentialReference {
    p::CredentialReference::OAuth {
        grant: p::OAuthGrant::TraditionalChatgpt,
        token_env: p::SecretReference { env: "SYNTHETIC_CODEX_TOKEN".into() },
        account_env: p::SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() },
        // Legacy environment-backed OAuth pins the expiry in saved settings.
        // Normal fixture tokens therefore use this same expiry (500).
        expires_at_unix: 500,
    }
}
fn settings(account: &str) -> p::DirectSettings {
    let account_env = p::SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() };
    p::DirectSettings {
        mode: p::AccessMode::ChatgptOAuth,
        model: "fixture-model".into(),
        effort: "high".into(),
        credentials: reference(),
        evidence: p::CapabilityEvidence {
            mode: p::AccessMode::ChatgptOAuth,
            model: "fixture-model".into(),
            efforts: ["high".into()].into(),
            account_env: p::AccountBindingReference::Environment(account_env),
            account_sha256: account_digest(account),
            transport_profile: p::AccessMode::ChatgptOAuth.profile().into(),
            catalog_evidence_sha256: "a".repeat(64),
            structured_output_evidence_sha256: Some("b".repeat(64)),
        },
        limits: p::Limits {
            timeout_ms: 1_000,
            max_prompt_bytes: 4_096,
            max_schema_bytes: 4_096,
            max_schema_nodes: 1_024,
            max_schema_depth: 16,
            max_request_bytes: 16_384,
            max_response_bytes: 4_096,
            max_stream_bytes: 2_000_000,
            max_event_bytes: 8_192,
            max_events: 4_096,
        },
    }
}
fn sse() -> Vec<u8> {
    let created = json!({
        "type":"response.created","sequence_number":0,
        "response":{"id":"resp_synthetic","model":"fixture-model","status":"in_progress","output":[]}
    });
    let completed = json!({
        "type":"response.completed","sequence_number":1,
        "response":{"id":"resp_synthetic","model":"fixture-model","status":"completed",
            "output":[{"id":"m1","type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":"{}"}]}]}
    });
    format!("data: {created}\n\ndata: {completed}\n\n").into_bytes()
}
fn provider(statuses: Vec<AuthStatusSnapshot>, now: u64)
    -> (p::DirectProvider, QueueSource, Arc<Transport>)
{
    let source = QueueSource::new(statuses);
    let credentials = CodexCredentialSource::new(source.clone(), account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(
        settings("synthetic-account"),
        Arc::new(credentials),
        transport.clone(),
        Arc::new(FixedClock(AtomicU64::new(now))),
    ).unwrap();
    (provider, source, transport)
}
fn prepare(provider: &p::DirectProvider) -> Result<Box<dyn p::PreparedCall>, p::ProviderError> {
    use p::ModelProvider;
    provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    })
}

#[test]
fn synthetic_credential_bundle_debug_redacts_bearer_and_account() {
    use p::CredentialSource as _;
    let source = CodexCredentialSource::new(
        QueueSource::new(vec![status("synthetic-account", 500)]),
        account_digest("synthetic-account"),
    ).unwrap();
    let bundle = source.load(
        &reference(),
        &p::SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() },
    ).unwrap();
    let debug = format!("{bundle:?}");
    assert!(!debug.contains("synthetic-signature"));
    assert!(!debug.contains("synthetic-account"));
}

#[tokio::test]
async fn direct_provider_future_expiry_succeeds_with_one_acquisition_per_load() {
    let (provider, source, transport) = provider(
        vec![status("synthetic-account", 500), status("synthetic-account", 500)], 100,
    );
    let prepared = prepare(&provider).unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    let output = prepared.dispatch(p::Cancellation::default()).await.unwrap();
    assert_eq!(output.response, "{}");
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(transport.posts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn direct_provider_expired_token_rejects_before_network() {
    // Preparation sees a future token; reacquisition immediately before dispatch
    // returns a token expiring at now, which the existing provider clock rejects.
    let (provider, source, transport) = provider(
        vec![status("synthetic-account", 500), status("synthetic-account", 100)], 100,
    );
    let prepared = prepare(&provider).unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    let result = prepared.dispatch(p::Cancellation::default()).await;
    assert!(matches!(result, Err(p::ProviderError::Attempt { failure: p::Failure::CredentialExpired, .. })));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn direct_provider_account_switch_rejects_before_network() {
    let (provider, source, transport) = provider(
        vec![status("synthetic-account", 500), status("different-account", 500)], 100,
    );
    let prepared = prepare(&provider).unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    let result = prepared.dispatch(p::Cancellation::default()).await;
    assert!(matches!(result, Err(p::ProviderError::Attempt { failure: p::Failure::AccountMismatch, .. })));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);

}

#[tokio::test]
async fn direct_provider_bounded_acquisition_timeout_rejects_before_network() {
    let source = QueueSource {
        statuses: Arc::new(Mutex::new(VecDeque::from([Err(p::Failure::Timeout)]))),
        calls: Arc::new(AtomicUsize::new(0)),
        cleanup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let credentials = CodexCredentialSource::new(
        source.clone(),
        account_digest("synthetic-account"),
    ).unwrap();
    let provider = p::DirectProvider::new(
        settings("synthetic-account"),
        Arc::new(credentials),
        transport.clone(),
        Arc::new(FixedClock(AtomicU64::new(100))),
    ).unwrap();
    let result = prepare(&provider);
    assert!(matches!(result, Err(p::ProviderError::Rejected(p::Failure::Timeout))));
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert!(source.cleanup_complete.load(Ordering::SeqCst));
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}


#[cfg(unix)]
fn process_watchdog<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || { let _ = tx.send(work()); });
    rx.recv_timeout(std::time::Duration::from_secs(4))
        .expect("outer watchdog: fake auth helper hung")
}

#[cfg(unix)]
fn fake_helper(script: &str) -> ProcessAuthStatusSource {
    use std::ffi::OsString;
    ProcessAuthStatusSource::new(
        std::path::PathBuf::from("/bin/sh"),
        vec![OsString::from("-c"), OsString::from(script)],
    )
}

#[cfg(unix)]
fn synthetic_helper_json(account: &str, exp: i64) -> String {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "exp": exp,
            "https://api.openai.com/auth": {"chatgpt_account_id": account}
        })).unwrap(),
    );
    let token = format!("{header}.{payload}.synthetic-signature");
    serde_json::json!({"authMethod":"chatgpt","authToken":token}).to_string()
}

#[cfg(unix)]
#[test]
fn fake_helper_protocol_parses_synthetic_status() {
    let response = synthetic_helper_json("synthetic-account", 500);
    let script = format!("printf '%s' '{}'", response.replace('\'', "'\\''"));
    let source = fake_helper(&script);
    let status = <ProcessAuthStatusSource as AuthorizedAuthStatusSource>::acquire_bounded(
        &source, std::time::Duration::from_secs(1),
    ).unwrap();
    assert_eq!(status.auth_method, Some(CodexAuthMethod::Chatgpt));
    assert!(status.auth_token.unwrap().contains("synthetic-signature"));
}

#[cfg(unix)]
#[tokio::test]
async fn fake_helper_synthetic_token_drives_direct_provider_success() {
    use p::ModelProvider;
    let response = synthetic_helper_json("synthetic-account", 500);
    let script = format!("printf '%s' '{}'", response.replace('\'', "'\\''"));
    let source = fake_helper(&script);
    let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(
        settings("synthetic-account"), Arc::new(credentials), transport.clone(),
        Arc::new(FixedClock(AtomicU64::new(100))),
    ).unwrap();
    let prepared = provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    }).unwrap();
    let output = prepared.dispatch(p::Cancellation::default()).await.unwrap();
    assert_eq!(output.response, "{}");
    assert_eq!(transport.posts.load(Ordering::SeqCst), 1);
}
#[cfg(unix)]
fn stateful_fake_helper(state_path: &std::path::Path, first_json: &str, second_json: &str) -> ProcessAuthStatusSource {
    use std::ffi::OsString;
    let script = format!(
        "if [ -e \"$1\" ]; then printf '%s' '{second}'; printf second > \"$1\"; else printf first > \"$1\"; printf '%s' '{first}'; fi",
        first = first_json, second = second_json,
    );
    ProcessAuthStatusSource::new(
        std::path::PathBuf::from("/bin/sh"),
        vec![OsString::from("-c"), OsString::from(script),
             OsString::from("synthetic-helper"), OsString::from(state_path.as_os_str())],
    )
}

#[cfg(unix)]
#[tokio::test]
async fn fake_helper_account_switch_between_prepare_and_dispatch_rejects_before_post() {
    use p::ModelProvider;
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("helper-state");
    let source = stateful_fake_helper(&state, &synthetic_helper_json("synthetic-account", 500),
        &synthetic_helper_json("different-account", 500));
    let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(settings("synthetic-account"), Arc::new(credentials),
        transport.clone(), Arc::new(FixedClock(AtomicU64::new(100)))).unwrap();
    let prepared = provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    }).unwrap();
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "first");
    let result = prepared.dispatch(p::Cancellation::default()).await;
    assert!(matches!(result, Err(p::ProviderError::Attempt { failure: p::Failure::AccountMismatch, .. })));
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "second");
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn fake_helper_expiry_after_prepare_rejects_before_post() {
    use p::ModelProvider;
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("helper-state");
    let source = stateful_fake_helper(&state, &synthetic_helper_json("synthetic-account", 500),
        &synthetic_helper_json("synthetic-account", 100));
    let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(settings("synthetic-account"), Arc::new(credentials),
        transport.clone(), Arc::new(FixedClock(AtomicU64::new(100)))).unwrap();
    let prepared = provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    }).unwrap();
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "first");
    let result = prepared.dispatch(p::Cancellation::default()).await;
    assert!(matches!(result, Err(p::ProviderError::Attempt { failure: p::Failure::CredentialExpired, .. })));
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "second");
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn fake_helper_malformed_output_maps_to_fixed_redacted_failure() {
    let source = fake_helper("printf 'SYNTHETIC_SECRET_MARKER malformed'; exit 0");
    let error = match <ProcessAuthStatusSource as AuthorizedAuthStatusSource>::acquire_bounded(
        &source, std::time::Duration::from_secs(1),
    ) {
        Ok(_) => panic!("malformed synthetic helper output unexpectedly parsed"),
        Err(error) => error,
    };
    assert_eq!(error, p::Failure::InvalidCredential);
    assert_eq!(error.to_string(), "credential is invalid");
    assert!(!error.to_string().contains("SYNTHETIC_SECRET_MARKER"));
    assert!(!format!("{error:?}").contains("SYNTHETIC_SECRET_MARKER"));
}

#[cfg(unix)]
#[test]
fn fake_helper_timeout_rejects_before_provider_post() {
    process_watchdog(|| {
        use p::ModelProvider;
        let source = ProcessAuthStatusSource::with_timeout_cap(
            std::path::PathBuf::from("/bin/sh"),
            vec!["-c".into(), "sleep 5".into()],
            std::time::Duration::from_millis(150),
        );
        let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
        let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
        let provider = p::DirectProvider::new(
            settings("synthetic-account"), Arc::new(credentials), transport.clone(),
            Arc::new(FixedClock(AtomicU64::new(100))),
        ).unwrap();
        let result = provider.prepare(p::ProviderRequest {
            prompt: "synthetic prompt".into(),
            output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            invocation_id: "synthetic-call".into(),
        });
        assert!(matches!(result, Err(p::ProviderError::Rejected(p::Failure::Timeout))));
        assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
    });
}

#[cfg(unix)]
#[tokio::test]
async fn pre_cancelled_dispatch_does_not_reacquire_helper_or_post() {
    use p::ModelProvider;
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("helper-state");
    let response = synthetic_helper_json("synthetic-account", 500);
    let source = stateful_fake_helper(&state, &response, &response);
    let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(settings("synthetic-account"), Arc::new(credentials),
        transport.clone(), Arc::new(FixedClock(AtomicU64::new(100)))).unwrap();
    let prepared = provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    }).unwrap();
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "first");
    let cancellation = p::Cancellation::default();
    cancellation.cancel();
    let result = prepared.dispatch(cancellation).await;
    assert!(matches!(result, Err(p::ProviderError::Attempt { failure: p::Failure::Cancelled, .. })));
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "first");
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
struct CancellingSecondAcquire {
    calls: AtomicUsize,
    cancellation: p::Cancellation,
}
#[cfg(unix)]
impl AuthorizedAuthStatusSource for CancellingSecondAcquire {
    fn acquire_bounded(
        &self,
        timeout: std::time::Duration,
    ) -> Result<AuthStatusSnapshot, p::Failure> {
        assert!(timeout <= std::time::Duration::from_secs(5));
        if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            self.cancellation.cancel();
        }
        Ok(AuthStatusSnapshot {
            auth_method: Some(CodexAuthMethod::Chatgpt),
            auth_token: Some(synthetic_helper_token("synthetic-account", 500)),
        })
    }
}
#[cfg(unix)]
fn synthetic_helper_token(account: &str, exp: i64) -> String {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({"exp": exp, "https://api.openai.com/auth": {"chatgpt_account_id": account}})).unwrap()
    );
    format!("{header}.{payload}.synthetic-signature")
}
#[cfg(unix)]
#[tokio::test]
async fn cancellation_during_second_acquisition_is_rechecked_before_transport() {
    use p::ModelProvider;
    let cancellation = p::Cancellation::default();
    let source = CancellingSecondAcquire { calls: AtomicUsize::new(0), cancellation: cancellation.clone() };
    let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(settings("synthetic-account"), Arc::new(credentials),
        transport.clone(), Arc::new(FixedClock(AtomicU64::new(100)))).unwrap();
    let prepared = provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    }).unwrap();
    assert!(!cancellation.is_cancelled());
    let result = prepared.dispatch(cancellation).await;
    assert!(matches!(result, Err(p::ProviderError::Attempt { failure: p::Failure::Cancelled, .. })));
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn fake_helper_output_overflow_rejects_before_provider_post() {
    use p::ModelProvider;
    let source = ProcessAuthStatusSource::new(
        std::path::PathBuf::from("/bin/sh"),
        vec!["-c".into(), "printf 'SYNTHETIC_SECRET_MARKER%040000d' 1".into()],
    );
    let credentials = CodexCredentialSource::new(source, account_digest("synthetic-account")).unwrap();
    let transport = Arc::new(Transport { posts: AtomicUsize::new(0), sse: sse() });
    let provider = p::DirectProvider::new(
        settings("synthetic-account"), Arc::new(credentials), transport.clone(),
        Arc::new(FixedClock(AtomicU64::new(100))),
    ).unwrap();
    let result = provider.prepare(p::ProviderRequest {
        prompt: "synthetic prompt".into(),
        output_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        invocation_id: "synthetic-call".into(),
    });
    let error = match result {
        Ok(_) => panic!("overflowing helper output unexpectedly prepared"),
        Err(error) => error,
    };
    assert!(matches!(error, p::ProviderError::Rejected(p::Failure::Limit)));
    assert!(!error.to_string().contains("SYNTHETIC_SECRET_MARKER"));
    assert!(!format!("{error:?}").contains("SYNTHETIC_SECRET_MARKER"));
    assert_eq!(transport.posts.load(Ordering::SeqCst), 0);
}

