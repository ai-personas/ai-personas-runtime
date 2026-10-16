//! In-process HTTPS/auth fixtures and protected temporary files only.
//! No real sign-in, environment credential load, provider inference, or Runtime.
use super::*;
use futures_util::future::BoxFuture;
use p::{HttpBody, HttpRequest, HttpResponse, HttpTransport};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Mutex,
    },
};

const TOKEN: &str = "stage2-synthetic-access-DO-NOT-USE";
const MODEL: &str = "fixture-model";
struct TestClock(AtomicU64);
impl p::Clock for TestClock {
    fn unix_seconds(&self) -> std::result::Result<u64, p::Failure> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
struct FakeAccess {
    generation: AtomicU64,
    account: Mutex<String>,
    failure: Mutex<Option<Code>>,
    expiry: AtomicU64,
}
impl FakeAccess {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(1),
            account: Mutex::new(hash(b"fixture-account")),
            failure: Mutex::new(None),
            expiry: AtomicU64::new(4_000),
        }
    }
}
impl AccessSource for FakeAccess {
    fn load(&self, _: &LockedVault, now: u64) -> Result<Access> {
        if let Some(code) = *self.failure.lock().unwrap() {
            return Err(code);
        }
        let expiry = self.expiry.load(Ordering::SeqCst);
        if now >= expiry {
            return Err(Code::CredentialExpired);
        }
        Ok(Access {
            account_sha256: self.account.lock().unwrap().clone(),
            credential_sha256: hash(&self.generation.load(Ordering::SeqCst).to_le_bytes()),
            expires_at_unix: expiry,
            bearer: p::Secret::new(TOKEN.into()),
            redactions: vec![
                p::Secret::new(TOKEN.into()),
                p::Secret::new("synthetic-refresh-secret".into()),
            ],
        })
    }
}
struct Body {
    data: Vec<u8>,
    position: usize,
}
impl Body {
    fn new(data: Vec<u8>) -> Self {
        Self { data, position: 0 }
    }
}
impl HttpBody for Body {
    fn read(
        &mut self,
        maximum: usize,
    ) -> BoxFuture<'_, std::result::Result<Option<Vec<u8>>, p::Failure>> {
        Box::pin(async move {
            if self.position == self.data.len() {
                return Ok(None);
            }
            let end = (self.position + maximum.min(97)).min(self.data.len());
            let out = self.data[self.position..end].to_vec();
            self.position = end;
            Ok(Some(out))
        })
    }
}
#[derive(Clone, Copy)]
enum Reply {
    Complete,
    MissingEffort,
    WrongEffort,
    MissingUsage,
    Truncated,
    TrailingError,
    Denied,
    BadCarrier,
    WrongSpecimen,
    Secret,
    Never,
}
struct MockHttp {
    catalog: Mutex<Vec<u8>>,
    gets: AtomicUsize,
    posts: AtomicUsize,
    requests: Mutex<Vec<Value>>,
    reply: Mutex<Reply>,
    entered: tokio::sync::Notify,
}
impl MockHttp {
    fn new() -> Self {
        Self {
            catalog: Mutex::new(
                serde_json::to_vec(&json!({"models":[
                    {"slug":"hidden-model","display_name":"Hidden","visibility":"hide"},
                    {"slug":MODEL,"display_name":"Fixture display name","visibility":"list"},
                    {"slug":"second-model","display_name":"Second","visibility":"list"}
                ]}))
                .unwrap(),
            ),
            gets: AtomicUsize::new(0),
            posts: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            reply: Mutex::new(Reply::Complete),
            entered: tokio::sync::Notify::new(),
        }
    }
}
fn frame(v: &Value) -> String {
    format!("data: {}\n\n", serde_json::to_string(v).unwrap())
}
fn response(request: &Value, reply: Reply) -> Vec<u8> {
    let model = request["model"].as_str().unwrap();
    let specimen = request["input"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .split_once('\n')
        .unwrap()
        .1;
    let mut carrier = json!({"decision_json":specimen}).to_string();
    match reply {
        Reply::BadCarrier => carrier = "{broken".into(),
        Reply::WrongSpecimen => {
            carrier =
                json!({"decision_json":specimen.replace("siwc-probe-", "wrong-probe-")}).to_string()
        }
        Reply::Secret => {
            carrier = json!({"decision_json":json!({"secret":TOKEN}).to_string()}).to_string()
        }
        _ => (),
    }
    let mut created = json!({"type":"response.created","response":{"id":"resp_fixture","model":model,"status":"in_progress","output":[]}});
    let mut done = json!({"type":"response.completed","response":{"id":"resp_fixture","model":model,"status":"completed",
        "output":[{"id":"msg_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":carrier,"annotations":[]}]}],
        "usage":{"input_tokens":10,"output_tokens":20,"total_tokens":30}}});
    if !matches!(reply, Reply::MissingEffort) {
        let effort = if matches!(reply, Reply::WrongEffort) {
            "low"
        } else {
            request["reasoning"]["effort"].as_str().unwrap()
        };
        created["response"]["reasoning"] = json!({"effort":effort});
        done["response"]["reasoning"] = json!({"effort":effort});
    }
    if matches!(reply, Reply::MissingUsage) {
        done["response"]["usage"] = Value::Null;
    }
    let mut result = frame(&created);
    if !matches!(reply, Reply::Truncated) {
        result.push_str(&frame(&done));
    }
    if matches!(reply, Reply::TrailingError) {
        result.push_str(&frame(&json!({"type":"error"})));
    }
    result.into_bytes()
}
impl HttpTransport for MockHttp {
    fn post(&self, _: HttpRequest) -> BoxFuture<'_, std::result::Result<HttpResponse, p::Failure>> {
        panic!("unguarded transport must never be used")
    }
    fn post_with_intake_guard(
        &self,
        request: HttpRequest,
        guard: Arc<p::IntakeLaunchGuard>,
    ) -> BoxFuture<'_, std::result::Result<HttpResponse, p::Failure>> {
        Box::pin(async move {
            assert_eq!(request.mode, p::AccessMode::SiwcOAuth);
            assert!(request.account_header.is_none());
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["store"], false);
            assert_eq!(body["stream"], true);
            assert_eq!(body["text"]["format"]["strict"], true);
            assert_eq!(
                body["text"]["format"]["schema"],
                crate::runtime::provider_schema()
            );
            for key in [
                "max_output_tokens",
                "tools",
                "previous_response_id",
                "temperature",
                "conversation",
            ] {
                assert!(body.get(key).is_none());
            }
            guard.begin_launch()?;
            self.posts.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(body.clone());
            self.entered.notify_one();
            let reply = *self.reply.lock().unwrap();
            if matches!(reply, Reply::Never) {
                return std::future::pending().await;
            }
            Ok(HttpResponse {
                status: if matches!(reply, Reply::Denied) {
                    403
                } else {
                    200
                },
                is_unencoded_sse: true,
                body: Box::new(Body::new(response(&body, reply))),
            })
        })
    }
}
impl wire::Transport for MockHttp {
    fn catalog(
        &self,
        bearer: p::Secret,
        guard: Arc<p::IntakeLaunchGuard>,
    ) -> BoxFuture<'_, std::result::Result<wire::CatalogResponse, p::Failure>> {
        Box::pin(async move {
            assert!(!bearer.expose().is_empty());
            guard.begin_launch()?;
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(wire::CatalogResponse {
                status: 200,
                unencoded_json: true,
                body: Box::new(Body::new(self.catalog.lock().unwrap().clone())),
            })
        })
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    backend: Backend,
    clock: Arc<TestClock>,
    access: Arc<FakeAccess>,
    http: Arc<MockHttp>,
}
fn fixture() -> Fixture {
    // Honors TMPDIR. Use an isolated /var/tmp directory on hosts where /tmp has
    // repository markers. Never relax the production repository/path guard.
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut backend = Backend::open(&dir.path().join("account.json")).unwrap();
    let clock = Arc::new(TestClock(AtomicU64::new(1_000)));
    let access = Arc::new(FakeAccess::new());
    let http = Arc::new(MockHttp::new());
    backend.clock = clock.clone();
    backend.source = access.clone();
    backend.transport = Some(http.clone());
    Fixture {
        dir,
        backend,
        clock,
        access,
        http,
    }
}
fn consent() -> ProbeConsent {
    ProbeConsent {
        authorize_one_request: true,
        retain_private_evidence: true,
        acknowledge_uncapped_remote_usage: true,
    }
}
async fn prepare(f: &mut Fixture, id: &str) -> PlanView {
    let catalog = f.backend.catalogue(true, true).await.unwrap();
    f.backend
        .plan(
            PlanRequest {
                id: id.into(),
                catalogue_sha256: catalog.catalogue_sha256,
                model: MODEL.into(),
                effort: "high".into(),
                timeout_ms: 10_000,
                max_observed_total_tokens: 100,
                require_remote_token_hard_bound: false,
            },
            true,
        )
        .unwrap()
}
async fn run(f: &mut Fixture, plan: &PlanView) -> ProbeStatus {
    f.backend
        .execute(
            &plan.plan.request.id,
            &plan.plan_sha256,
            consent(),
            p::Cancellation::default(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn catalogue_filters_preserves_server_order_and_proves_nothing() {
    let mut f = fixture();
    let view = f.backend.catalogue(true, true).await.unwrap();
    assert_eq!(
        view.models
            .iter()
            .map(|m| m.slug.as_str())
            .collect::<Vec<_>>(),
        [MODEL, "second-model"]
    );
    assert_eq!(view.models[0].display_name, "Fixture display name");
    assert!(!view.strict_carrier_verified && !view.inference_performed);
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
    assert_eq!(
        view.catalogue_sha256,
        digest(f.backend.read().unwrap().catalogue.as_ref().unwrap()).unwrap()
    );
}
#[test]
fn catalogue_rejects_duplicate_keys_duplicate_slugs_oversize_and_unsafe_labels() {
    assert!(models(r#"{"models":[],"models":[]}"#).is_err());
    let model = json!({"slug":MODEL,"display_name":"Fixture","visibility":"list"});
    assert!(models(&json!({"models":[model.clone(),model.clone()]}).to_string()).is_err());
    assert!(models(&json!({"models":vec![model;MAX_MODELS+1]}).to_string()).is_err());
    assert!(models(&"x".repeat(wire::CATALOG_BYTES + 1)).is_err());
    assert!(models(
        &json!({"models":[{"slug":MODEL,"display_name":"bad\nlabel","visibility":"list"}]})
            .to_string()
    )
    .is_err());
}
#[tokio::test]
async fn denied_expired_and_fenced_access_never_gets_or_posts() {
    let mut f = fixture();
    assert!(matches!(
        f.backend.catalogue(false, true).await,
        Err(Code::Denied)
    ));
    for code in [
        Code::CredentialExpired,
        Code::CredentialFenced,
        Code::Denied,
        Code::CredentialUnavailable,
    ] {
        *f.access.failure.lock().unwrap() = Some(code);
        assert!(matches!(f.backend.catalogue(true, true).await, Err(actual) if actual == code));
    }
    assert_eq!(f.http.gets.load(Ordering::SeqCst), 0);
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
    assert!(f.backend.vault.read_onboarding().unwrap().is_none());
}
#[tokio::test]
async fn exact_completed_proof_generates_typed_native_settings_and_never_replays() {
    let mut f = fixture();
    let plan = prepare(&mut f, "complete").await;
    let output = run(&mut f, &plan).await;
    assert_eq!(output.phase, ProbePhase::CompletedVerified);
    let settings = f
        .backend
        .configuration("complete", &plan.plan_sha256)
        .unwrap();
    settings.validate().unwrap();
    let encoded = serde_json::to_value(&settings).unwrap();
    assert_eq!(encoded["credentials"]["kind"], "o_auth");
    assert_eq!(encoded["credentials"]["grant"], "siwc_token_sharing");
    assert_eq!(settings.model, MODEL);
    assert_eq!(settings.effort, "high");
    assert_eq!(
        settings.evidence.structured_output_evidence_sha256,
        output.proof_sha256
    );
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
    // Replay does not depend on current credentials and cannot redispatch.
    *f.access.failure.lock().unwrap() = Some(Code::CredentialExpired);
    let repeated = run(&mut f, &plan).await;
    assert_eq!(repeated.proof_sha256, output.proof_sha256);
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
    assert!(matches!(
        f.backend.configuration("complete", &plan.plan_sha256),
        Err(Code::CredentialExpired)
    ));
}
#[tokio::test]
async fn hidden_model_rebound_key_and_changed_plan_digest_are_rejected() {
    let mut f = fixture();
    let plan = prepare(&mut f, "binding").await;
    let mut changed = plan.plan.request.clone();
    changed.effort = "low".into();
    assert!(matches!(
        f.backend.plan(changed, true),
        Err(Code::KeyConflict)
    ));
    let mut hidden = plan.plan.request.clone();
    hidden.id = "hidden".into();
    hidden.model = "hidden-model".into();
    assert!(matches!(
        f.backend.plan(hidden, true),
        Err(Code::ModelNotSelectable)
    ));
    assert!(matches!(
        f.backend
            .execute(
                "binding",
                &"f".repeat(64),
                consent(),
                p::Cancellation::default()
            )
            .await,
        Err(Code::KeyConflict)
    ));
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn consent_is_specific_and_remote_token_cap_is_honestly_unsupported() {
    let mut f = fixture();
    let plan = prepare(&mut f, "consent").await;
    for c in [
        ProbeConsent::default(),
        ProbeConsent {
            authorize_one_request: true,
            retain_private_evidence: true,
            acknowledge_uncapped_remote_usage: false,
        },
    ] {
        assert!(matches!(
            f.backend
                .execute("consent", &plan.plan_sha256, c, p::Cancellation::default())
                .await,
            Err(Code::Denied)
        ));
    }
    let mut bounded = plan.plan.request.clone();
    bounded.id = "hard-cap".into();
    bounded.require_remote_token_hard_bound = true;
    assert!(matches!(
        f.backend.plan(bounded, true),
        Err(Code::RemoteTokenBoundUnsupported)
    ));
    assert_eq!(
        f.backend
            .probe_status("consent", &plan.plan_sha256)
            .unwrap()
            .phase,
        ProbePhase::Planned
    );
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn missing_or_conflicting_effort_usage_or_carrier_never_establishes_proof() {
    for reply in [
        Reply::MissingEffort,
        Reply::WrongEffort,
        Reply::MissingUsage,
        Reply::BadCarrier,
        Reply::WrongSpecimen,
    ] {
        let mut f = fixture();
        *f.http.reply.lock().unwrap() = reply;
        let plan = prepare(&mut f, "unverified").await;
        let output = run(&mut f, &plan).await;
        assert_ne!(output.phase, ProbePhase::CompletedVerified);
        assert!(output.proof_sha256.is_none());
        assert!(matches!(
            f.backend.configuration("unverified", &plan.plan_sha256),
            Err(Code::ProofUnestablished)
        ));
        run(&mut f, &plan).await;
        assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn truncated_trailing_error_and_http_denial_retain_uncertainty_without_replay() {
    for reply in [Reply::Truncated, Reply::TrailingError, Reply::Denied] {
        let mut f = fixture();
        *f.http.reply.lock().unwrap() = reply;
        let plan = prepare(&mut f, "failed").await;
        let output = run(&mut f, &plan).await;
        assert_eq!(output.phase, ProbePhase::FailedOrUncertain);
        assert!(output.remote_usage_may_remain && output.proof_sha256.is_none());
        if matches!(reply, Reply::Denied) {
            assert_eq!(output.code, Some(Code::Denied));
        }
        run(&mut f, &plan).await;
        assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn abandoned_dispatch_waits_for_actual_transport_entry_then_blocks_replay() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut f = fixture(); *f.http.reply.lock().unwrap() = Reply::Never;
        let plan = prepare(&mut f, "abandoned").await;
        let mut running = Box::pin(f.backend.execute("abandoned", &plan.plan_sha256, consent(), p::Cancellation::default()));
        tokio::select! {
            _ = f.http.entered.notified() => (),
            result = &mut running => panic!("probe returned before dispatch observation: {result:?}"),
        }
        drop(running);
        assert_eq!(f.backend.probe_status("abandoned", &plan.plan_sha256).unwrap().phase, ProbePhase::OutcomeUnrecorded);
        run(&mut f, &plan).await; assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
        let mut next = plan.plan.request.clone(); next.id = "new-key".into();
        let next = f.backend.plan(next, true).unwrap();
        assert!(matches!(f.backend.execute("new-key", &next.plan_sha256, consent(), p::Cancellation::default()).await, Err(Code::UnresolvedProbe)));
        assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
    }).await.expect("bounded overall fixture deadline");
}
#[tokio::test]
async fn cancelled_before_dispatch_is_consumed_but_never_sent() {
    let mut f = fixture();
    let plan = prepare(&mut f, "cancelled").await;
    let cancellation = p::Cancellation::default();
    cancellation.cancel();
    let status = f
        .backend
        .execute("cancelled", &plan.plan_sha256, consent(), cancellation)
        .await
        .unwrap();
    assert_eq!(status.phase, ProbePhase::NotDispatched);
    assert_eq!(status.code, Some(Code::Cancelled));
    run(&mut f, &plan).await;
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn expired_plan_or_rotated_generation_cannot_dispatch() {
    let mut f = fixture();
    let plan = prepare(&mut f, "expiry").await;
    f.clock
        .0
        .store(plan.plan.authorization_deadline_unix, Ordering::SeqCst);
    assert!(matches!(
        f.backend
            .execute(
                "expiry",
                &plan.plan_sha256,
                consent(),
                p::Cancellation::default()
            )
            .await,
        Err(Code::CredentialExpired)
    ));
    f.clock.0.store(1_000, Ordering::SeqCst);
    f.access.generation.store(2, Ordering::SeqCst);
    assert!(matches!(
        f.backend
            .execute(
                "expiry",
                &plan.plan_sha256,
                consent(),
                p::Cancellation::default()
            )
            .await,
        Err(Code::CredentialChanged)
    ));
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn retained_result_survives_reopen_and_concurrent_owner_is_excluded() {
    let mut f = fixture();
    let plan = prepare(&mut f, "retained").await;
    let first = run(&mut f, &plan).await;
    let path = f.dir.path().join("account.json");
    assert!(matches!(Backend::open(&path), Err(Code::Busy)));
    drop(f.backend);
    let reopened = Backend::open(&path).unwrap();
    let recovered = reopened
        .probe_status("retained", &plan.plan_sha256)
        .unwrap();
    assert_eq!(first.proof_sha256, recovered.proof_sha256);
    assert_eq!(recovered.phase, ProbePhase::CompletedVerified);
}
#[tokio::test]
async fn secret_echoes_never_reach_journal_or_machine_outputs() {
    let mut f = fixture();
    *f.http.reply.lock().unwrap() = Reply::Secret;
    let plan = prepare(&mut f, "secret").await;
    let output = run(&mut f, &plan).await;
    assert_eq!(output.code, Some(Code::BodyWithheld));
    assert!(output.proof_sha256.is_none());
    let journal = f.backend.vault.read_onboarding().unwrap().unwrap();
    assert!(!String::from_utf8(journal).unwrap().contains(TOKEN));
    assert!(!serde_json::to_string(&output).unwrap().contains(TOKEN));
    *f.http.catalog.lock().unwrap() =
        json!({"models":[{"slug":MODEL,"display_name":TOKEN,"visibility":"list"}]})
            .to_string()
            .into_bytes();
    assert!(matches!(
        f.backend.catalogue(true, true).await,
        Err(Code::BodyWithheld)
    ));
}
#[tokio::test]
async fn malformed_journal_and_symlink_are_rejected_without_network() {
    let f = fixture();
    f.backend
        .vault
        .write_onboarding(br#"{"version":1,"version":1}"#)
        .unwrap();
    assert!(matches!(f.backend.read(), Err(Code::InvalidState)));
    fs::remove_file(f.dir.path().join("account.json.onboarding")).unwrap();
    let target = f.dir.path().join("untouched");
    fs::write(&target, b"untouched").unwrap();
    std::os::unix::fs::symlink(&target, f.dir.path().join("account.json.onboarding")).unwrap();
    assert!(f.backend.read().is_err());
    assert_eq!(fs::read(&target).unwrap(), b"untouched");
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn renewal_requires_new_evidence_and_explicit_migration_without_mutating_saved_settings() {
    let mut f = fixture();
    let plan = prepare(&mut f, "renewal").await;
    run(&mut f, &plan).await;
    let settings = f
        .backend
        .configuration("renewal", &plan.plan_sha256)
        .unwrap();
    let before = serde_json::to_value(&settings).unwrap();
    f.access.generation.store(2, Ordering::SeqCst);
    f.access.expiry.store(4_500, Ordering::SeqCst);
    let renewal = f.backend.renewal_status(&settings).unwrap();
    assert!(
        renewal.account_matches
            && renewal.explicit_provider_migration_required
            && renewal.fresh_catalogue_and_probe_required
    );
    assert!(!renewal.runtime_modified && !renewal.inference_performed);
    assert!(matches!(
        f.backend.configuration("renewal", &plan.plan_sha256),
        Err(Code::CredentialChanged)
    ));
    *f.access.account.lock().unwrap() = hash(b"different-account");
    assert!(matches!(
        f.backend.catalogue(true, true).await,
        Err(Code::AccountChanged)
    ));
    assert!(matches!(
        f.backend.renewal_status(&settings),
        Err(Code::AccountChanged)
    ));
    assert_eq!(before, serde_json::to_value(&settings).unwrap());
    assert_eq!(f.http.posts.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn observed_token_threshold_is_not_misreported_as_a_remote_cap() {
    let mut f = fixture();
    let catalog = f.backend.catalogue(true, true).await.unwrap();
    let plan = f
        .backend
        .plan(
            PlanRequest {
                id: "small".into(),
                catalogue_sha256: catalog.catalogue_sha256,
                model: MODEL.into(),
                effort: "high".into(),
                timeout_ms: 10_000,
                max_observed_total_tokens: 1,
                require_remote_token_hard_bound: false,
            },
            true,
        )
        .unwrap();
    let result = run(&mut f, &plan).await;
    assert_eq!(result.phase, ProbePhase::CompletedUnverified);
    assert!(result.proof_sha256.is_none());
    assert_eq!(result.usage["total_tokens"], 30);
    assert!(result.remote_usage_may_remain);
    assert!(!plan.remote_token_hard_bound && !plan.dollar_hard_bound);
}
#[tokio::test]
async fn actual_selected_vault_source_and_generated_settings_preserve_native_contract() {
    use crate::auth::{CredentialRecord, OsEntropy, StoredCredential};
    use base64::Engine;
    use p::CredentialSource;
    let mut f = fixture();
    let now = crate::auth::SystemClock.now_unix();
    f.clock.0.store(now, Ordering::SeqCst);
    let host = f.backend.vault.host_id(&mut OsEntropy).unwrap();
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"alg":"RS256","kid":"fixture","typ":"JWT"}"#);
    // Trusted synthetic authorization fixture, not a verified real sign-in.
    let record = CredentialRecord::new(
        StoredCredential {
            version: 1,
            issuer: crate::auth::ISSUER.into(),
            subject: "synthetic-subject".into(),
            client_id: "oaiapp_fixture".into(),
            ext_agent_host_id: host,
            callback_registration: super::super::CALLBACK_REGISTRATION.into(),
            nonce: "A".repeat(43),
            access_token: TOKEN.into(),
            refresh_token: "synthetic-refresh-secret".into(),
            id_token: format!("{header}.e30.AA"),
            token_type: "Bearer".into(),
            scopes: vec!["resource.invoke".into(), "chatgpt.tokens.use.direct".into()],
            saved_at_unix: now,
            expires_at_unix: now + 3_600,
            earliest_refresh_at: None,
            refresh_uncertain: None,
        },
        true,
    );
    f.backend.vault.save(&record).unwrap();
    f.backend.source = Arc::new(SelectedVault);
    let vault_before = fs::read(f.dir.path().join("account.json")).unwrap();
    let plan = prepare(&mut f, "native").await;
    run(&mut f, &plan).await;
    let settings = f
        .backend
        .configuration("native", &plan.plan_sha256)
        .unwrap();
    assert_eq!(
        vault_before,
        fs::read(f.dir.path().join("account.json")).unwrap()
    );
    drop(f.backend);
    let source = crate::auth::native::VaultCredentialSource::open(
        &f.dir.path().join("account.json"),
        &settings,
    )
    .unwrap();
    let loaded = source
        .load(&settings.credentials, &settings.evidence.account_env)
        .unwrap();
    assert!(
        matches!(loaded.credential, p::Credential::OAuth { grant:p::OAuthGrant::SiwcTokenSharing, expires_at_unix, .. } if expires_at_unix == now+3_600)
    );
}
