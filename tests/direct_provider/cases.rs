use super::*;
use ai_personas::provider as direct;
use futures_util::future::BoxFuture;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type TransportResult<T> = std::result::Result<T, direct::Failure>;

struct Credentials;
impl direct::CredentialSource for Credentials {
    fn load(
        &self,
        reference: &direct::CredentialReference,
        evidence_account: &direct::SecretReference,
    ) -> TransportResult<direct::CredentialBundle> {
        let direct::CredentialReference::ApiKey {
            key_env,
            account_env,
        } = reference
        else {
            panic!("fixture requires an explicit API-key reference");
        };
        assert_eq!(key_env.env, "UNREAD_FIXTURE_KEY");
        assert_eq!(account_env.env, "UNREAD_FIXTURE_ACCOUNT");
        assert_eq!(evidence_account.env, "UNREAD_FIXTURE_EVIDENCE_ACCOUNT");
        Ok(direct::CredentialBundle {
            credential: direct::Credential::ApiKey {
                key: direct::Secret::new("synthetic-key".into()),
                account: direct::Secret::new("synthetic-account".into()),
            },
            evidence_account: direct::Secret::new("synthetic-account".into()),
        })
    }
}

struct Clock;
impl direct::Clock for Clock {
    fn unix_seconds(&self) -> TransportResult<u64> {
        Ok(NOW as u64)
    }
}

struct Body {
    bytes: Vec<u8>,
    position: usize,
    failure_at_end: Option<direct::Failure>,
    cancel_at_eof: Option<direct::Cancellation>,
}
impl direct::HttpBody for Body {
    fn read(&mut self, maximum: usize) -> BoxFuture<'_, TransportResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            if maximum == 0 {
                return Err(direct::Failure::Limit);
            }
            if self.position == self.bytes.len() {
                if let Some(cancellation) = &self.cancel_at_eof {
                    cancellation.cancel();
                }
                return self.failure_at_end.map_or(Ok(None), Err);
            }
            let end = (self.position + maximum.min(7)).min(self.bytes.len());
            let bytes = self.bytes[self.position..end].to_vec();
            self.position = end;
            Ok(Some(bytes))
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Behavior {
    Valid,
    WithEffects,
    InvalidDecision,
    Stale,
    Forbidden(&'static str),
    FailAfterComplete,
    BoundedStream,
}

pub(super) struct Transport {
    root: PathBuf,
    carrier: Option<String>,
    behavior: Behavior,
    release: Option<PathBuf>,
    pub(super) calls: AtomicUsize,
    inner: Mutex<Option<String>>,
    cancel_at_eof: Mutex<Option<direct::Cancellation>>,
    effort_reports: Mutex<Option<(Value, Value)>>,
}

fn frame(event: &Value) -> String {
    format!("data: {}\n\n", serde_json::to_string(event).unwrap())
}

impl direct::HttpTransport for Transport {
    fn post(
        &self,
        request: direct::HttpRequest,
    ) -> BoxFuture<'_, TransportResult<direct::HttpResponse>> {
        Box::pin(async move {
            assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
            assert_eq!(request.mode, direct::AccessMode::PlatformApiKey);
            assert_eq!(request.bearer.expose(), "synthetic-key");
            assert!(request.account_header.is_none());
            let wire: Value = serde_json::from_slice(&request.body).unwrap();
            let prompt = wire["input"][0]["content"][0]["text"].as_str().unwrap();
            let control: Value = serde_json::from_str(
                prompt
                    .split_once("\nCONTROL\n")
                    .unwrap()
                    .1
                    .split_once("\nQUALIFIED_CONTEXT\n")
                    .unwrap()
                    .0,
            )
            .unwrap();
            let id = control["invocation_id"].as_str().unwrap();
            let mut host =
                s::Store::open_for_inspection(&self.root.join("state.sqlite3"), storage_limits())
                    .unwrap();
            let invocation = host.invocation(id).unwrap();
            assert_eq!(invocation.state, s::InvocationState::Dispatched);
            assert!(invocation.response_hash.is_none());
            assert!(matches!(
                host.acquire_execution_owner(),
                Err(s::StoreError::InvalidState(reason))
                    if reason == "an execution owner is already active"
            ));
            let admitted: Prepared = serde_json::from_value(invocation.request.unwrap()).unwrap();
            assert_eq!(admitted.request.profile, Profile::Direct);
            assert_eq!(admitted.prompt.as_bytes(), prompt.as_bytes());
            assert_eq!(admitted.output_schema, provider_schema());
            assert!(prompt.contains(&serde_json::to_string(&p::decision_schema()).unwrap()));
            assert_eq!(
                wire,
                json!({
                    "model": admitted.requested_model,
                    "store": false,
                    "stream": true,
                    "instructions": "Return only the JSON value required by text.format.",
                    "input": [{"role":"user","content":[{"type":"input_text","text":prompt}]}],
                    "reasoning": {"effort":admitted.requested_effort},
                    "text": {"format":{
                        "type":"json_schema","name":"persona_decision","strict":true,
                        "schema":admitted.output_schema
                    }}
                })
            );
            let audit = admitted.provider_preparation.as_ref().unwrap();
            assert_eq!(audit["invocation_id"], id);
            assert_eq!(audit["prompt_sha256"], s::sha256_bytes(prompt.as_bytes()));
            assert_eq!(audit["prompt_bytes"], prompt.len());
            assert_eq!(
                audit["schema_sha256"],
                s::sha256_bytes(&serde_json::to_vec(&admitted.output_schema).unwrap())
            );
            assert_eq!(audit["request_sha256"], s::sha256_bytes(&request.body));
            assert_eq!(audit["request_bytes"], request.body.len());
            assert_eq!(
                audit["transport_profile"],
                direct::AccessMode::PlatformApiKey.profile()
            );
            assert!(host
                .trusted_host()
                .read_terminal_evidence(
                    &s::EntityId::Invocation(id.into()),
                    &format!("terminal:{}", s::sha256_bytes(id.as_bytes())),
                )
                .unwrap()
                .is_none());

            let carrier = self.carrier.clone().unwrap_or_else(|| {
                let mut response = decision(&admitted, true);
                if matches!(self.behavior, Behavior::WithEffects) {
                    response.operations.push(p::Operation::Communicate {
                        id: "fixture-message".into(),
                        recipient: "human".into(),
                        subject: "Synthetic completed response".into(),
                        text: "This local deposit requires complete Decision adoption.".into(),
                    });
                }
                let mut raw = serde_json::to_value(response).unwrap();
                if matches!(self.behavior, Behavior::InvalidDecision) {
                    raw.as_object_mut().unwrap().remove("next_assets");
                }
                let inner = format!(" \n{}\n ", serde_json::to_string(&raw).unwrap());
                serde_json::to_string(&json!({"decision_json":inner})).unwrap()
            });
            *self.inner.lock().unwrap() = serde_json::from_str::<Value>(&carrier)
                .ok()
                .and_then(|value| value["decision_json"].as_str().map(str::to_owned));

            if matches!(self.behavior, Behavior::Stale) {
                // This is a validated authority mutation through a separate Store,
                // not a fabricated provider completion or an edited epoch counter.
                let epoch = host.epoch().unwrap();
                let pause = control_request(
                    &host,
                    &self.root,
                    "direct-concurrent-pause",
                    "work",
                    s::RuntimeControlAction::Pause,
                );
                host.trusted_host().commit_runtime_control(pause).unwrap();
                assert_eq!(host.epoch().unwrap(), epoch + 1);
                assert_eq!(
                    host.invocation(id).unwrap().state,
                    s::InvocationState::Dispatched
                );
                assert!(
                    closeout_head(&self.root).body["board"]["works"]["work"]["phase"]
                        .get("Paused")
                        .is_some()
                );
            }
            drop(host);

            // These paths belong exclusively to the fresh Fixture tempdir.
            let directory = self.root.parent().unwrap();
            std::fs::write(directory.join("captured-prompt"), prompt.as_bytes()).unwrap();
            std::fs::write(
                directory.join("captured-schema"),
                serde_json::to_vec(&wire["text"]["format"]["schema"]).unwrap(),
            )
            .unwrap();
            std::fs::write(directory.join("fixture-entered"), b"entered").unwrap();
            let mut launches = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("fixture-launches"))
                .unwrap();
            launches.write_all(b"launch\n").unwrap();
            drop(launches);
            if let Some(release) = &self.release {
                // DirectProvider's finite deadline bounds this wait; dropping the
                // transport future stops it, including when the real owner dies.
                while !release.exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }

            let effort_reports = self.effort_reports.lock().unwrap().clone();
            let mut created = json!({"type":"response.created","response":{
                "id":"r-fixture","status":"in_progress",
                "model":admitted.requested_model,"output":[]
            }});
            if let Some((created_report, _)) = &effort_reports {
                created["response"]["reasoning"] = created_report.clone();
            }
            let mut bytes = frame(&created).into_bytes();
            if matches!(self.behavior, Behavior::BoundedStream) {
                bytes.extend_from_slice(
                    frame(&json!({
                        "type":"response.output_text.delta","response_id":"r-fixture",
                        "item_id":"m-fixture","output_index":0,"content_index":0,"delta":carrier
                    }))
                    .as_bytes(),
                );
                let comment = format!(": {}\n\n", "x".repeat(1024));
                while bytes.len() <= settings().limits.max_stream_bytes {
                    bytes.extend_from_slice(comment.as_bytes());
                }
            } else {
                let output = match self.behavior {
                    Behavior::Forbidden(kind) => json!([{
                        "id":"forbidden-fixture","type":kind,"status":"completed",
                        "call_id":"call-fixture","name":"fixture_action",
                        "arguments":"PRIVATE DISCARDED FIXTURE PAYLOAD",
                        "output":"PRIVATE DISCARDED FIXTURE PAYLOAD",
                        "action":{"type":"fixture_action","text":"PRIVATE DISCARDED FIXTURE PAYLOAD"}
                    }]),
                    _ => json!([
                        {"id":"reasoning-fixture","type":"reasoning","status":"completed",
                         "summary":[{"type":"summary_text","text":"PRIVATE DISCARDED FIXTURE PAYLOAD"}]},
                        {"id":"m-fixture","type":"message","role":"assistant","status":"completed",
                         "content":[{"type":"output_text","text":carrier}]}
                    ]),
                };
                let mut completed = json!({
                    "type":"response.completed","response":{
                        "id":"r-fixture","status":"completed",
                        "model":admitted.requested_model,"output":output
                    }
                });
                if let Some((_, completed_report)) = &effort_reports {
                    completed["response"]["reasoning"] = completed_report.clone();
                }
                bytes.extend_from_slice(frame(&completed).as_bytes());
            }
            Ok(direct::HttpResponse {
                status: 200,
                is_unencoded_sse: true,
                body: Box::new(Body {
                    bytes,
                    position: 0,
                    failure_at_end: matches!(self.behavior, Behavior::FailAfterComplete)
                        .then_some(direct::Failure::Network),
                    cancel_at_eof: self.cancel_at_eof.lock().unwrap().clone(),
                }),
            })
        })
    }
}

fn settings() -> direct::DirectSettings {
    let reference = |name: &str| direct::SecretReference { env: name.into() };
    direct::DirectSettings {
        mode: direct::AccessMode::PlatformApiKey,
        model: "fixture-unavailable-model".into(),
        effort: "medium".into(),
        credentials: direct::CredentialReference::ApiKey {
            key_env: reference("UNREAD_FIXTURE_KEY"),
            account_env: reference("UNREAD_FIXTURE_ACCOUNT"),
        },
        evidence: direct::CapabilityEvidence {
            mode: direct::AccessMode::PlatformApiKey,
            model: "fixture-unavailable-model".into(),
            efforts: set(&["medium"]),
            account_env: reference("UNREAD_FIXTURE_EVIDENCE_ACCOUNT"),
            account_sha256: s::sha256_bytes(b"synthetic-account"),
            transport_profile: direct::AccessMode::PlatformApiKey.profile().into(),
            catalog_evidence_sha256: "1".repeat(64),
            structured_output_evidence_sha256: Some("2".repeat(64)),
        },
        limits: direct::Limits {
            timeout_ms: 5000,
            max_prompt_bytes: config().context_bytes,
            max_schema_bytes: 262_144,
            max_schema_nodes: 65_536,
            max_schema_depth: 64,
            max_request_bytes: 2 * 1024 * 1024,
            max_response_bytes: 32_768,
            max_stream_bytes: 128 * 1024,
            max_event_bytes: 96 * 1024,
            max_events: 128,
        },
    }
}

fn injected(
    root: &Path,
    carrier: Option<String>,
    behavior: Behavior,
    release: Option<PathBuf>,
) -> (direct::DirectProvider, Arc<Transport>) {
    injected_with_settings(root, carrier, behavior, release, settings())
}

fn injected_with_settings(
    root: &Path,
    carrier: Option<String>,
    behavior: Behavior,
    release: Option<PathBuf>,
    mut settings: direct::DirectSettings,
) -> (direct::DirectProvider, Arc<Transport>) {
    if release.is_some() {
        settings.limits.timeout_ms = 60_000;
    }
    let transport = Arc::new(Transport {
        root: root.into(),
        carrier,
        behavior,
        release,
        calls: AtomicUsize::new(0),
        inner: Mutex::new(None),
        cancel_at_eof: Mutex::new(None),
        effort_reports: Mutex::new(None),
    });
    let provider = direct::DirectProvider::new(
        settings,
        Arc::new(Credentials),
        transport.clone(),
        Arc::new(Clock),
    )
    .unwrap();
    (provider, transport)
}

pub(super) fn transport_fixture(
    fixture: &Fixture,
    response: &str,
    behavior: Behavior,
) -> (direct::DirectProvider, Arc<Transport>) {
    injected(
        fixture.app.state_root(),
        Some(response.into()),
        behavior,
        None,
    )
}

#[tokio::test(flavor = "current_thread")]
async fn native_direct_step_preserves_admission_terminal_order_whole_decision_and_fences() {
    for behavior in [Behavior::Valid, Behavior::InvalidDecision, Behavior::Stale] {
        let mut configuration = config();
        configuration.allow_direct_provider = true;
        let mut primary = seed("a");
        primary.model_confirmed = true;
        let mut fixture = Fixture::configured(true, configuration, primary, need());
        fixture.accept();
        let (provider, transport) = injected(fixture.app.state_root(), None, behavior, None);
        let request = PrepareRequest {
            id: "direct-native".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Direct,
            mode: Mode::Ordinary,
        };
        let before = fixture.app.snapshot().unwrap().work_meta["work"].attempts_left;
        let report = fixture
            .app
            .step_with_provider(
                request.clone(),
                &provider,
                direct::Cancellation::default(),
                NOW,
            )
            .await
            .unwrap();
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert!(!report.synthetic);
        assert!(report.usage_is_unknown);
        let inner = transport.inner.lock().unwrap().clone().unwrap();
        let terminal = fixture.app.terminal_evidence("direct-native").unwrap();
        assert_eq!(terminal.body["receipt"]["kind"], "provider_complete");
        assert_eq!(
            terminal.body["response_body"]["text"].as_str(),
            Some(inner.as_str())
        );
        assert_eq!(
            terminal.body["response_body"]["sha256"],
            s::sha256_bytes(inner.as_bytes())
        );
        assert_eq!(
            terminal.body["receipt"]["usage"]["input_tokens"],
            Value::Null
        );
        assert_eq!(terminal.body["receipt"]["attempt"]["stream_eof"], true);
        assert_eq!(
            terminal.body["receipt"]["attempt"]["response_completed"],
            true
        );
        assert!(terminal.body["receipt"]["audit"].get("account").is_none());
        let serialized = serde_json::to_string(&terminal).unwrap();
        for discarded in [
            "synthetic-key",
            "synthetic-account",
            "PRIVATE DISCARDED FIXTURE PAYLOAD",
        ] {
            assert!(!serialized.contains(discarded));
        }
        match behavior {
            Behavior::Valid => {
                assert_eq!(report.status, AdoptionStatus::Adopted);
                assert_eq!(
                    fixture.app.adopted("direct-native").unwrap().authored_json,
                    Some(inner)
                );
            }
            Behavior::InvalidDecision | Behavior::Stale => {
                assert_eq!(report.status, AdoptionStatus::Held);
                assert!(report.applied.is_empty());
                assert!(fixture.app.adopted("direct-native").is_err());
                assert_eq!(
                    fixture.app.invocation("direct-native").unwrap().state,
                    s::InvocationState::Failed
                );
                if matches!(behavior, Behavior::Stale) {
                    assert!(report.reason.as_deref().unwrap().contains("epoch"));
                }
            }
            _ => unreachable!(),
        }
        assert_eq!(
            fixture.app.snapshot().unwrap().work_meta["work"].attempts_left,
            before - 1
        );
        let usage: s::ReservationRecord =
            adoption_accounting_record(fixture.app.state_root(), "reservation", "v:direct-native");
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert!(usage.measured.is_none());
        let ledger = fixture.app.ledger().unwrap();
        assert!(fixture
            .app
            .step_with_provider(request, &provider, direct::Cancellation::default(), NOW)
            .await
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_only_navigation_charged(ledger, fixture.app.ledger().unwrap());
        let invocation = fixture.app.invocation("direct-native").unwrap();
        let admitted: Prepared = serde_json::from_value(invocation.request.unwrap()).unwrap();
        let raw = serde_json::to_vec(&decision(&admitted, true)).unwrap();
        assert!(fixture
            .app
            .adopt_response("direct-native", &raw, NOW)
            .is_err());
        assert_eq!(
            fixture.app.terminal_evidence("direct-native").unwrap(),
            terminal
        );
    }
}

// Exercises cancellation after the direct provider has returned a Ready
// completion, independently of the body-at-EOF cancellation fixture.
struct CancelOnReturn(direct::DirectProvider);
struct CancelOnReturnCall(Box<dyn direct::PreparedCall>);
impl direct::ModelProvider for CancelOnReturn {
    fn intent(&self) -> direct::ProviderIntent {
        direct::ModelProvider::intent(&self.0)
    }
    fn prepare(
        &self,
        request: direct::ProviderRequest,
    ) -> std::result::Result<Box<dyn direct::PreparedCall>, direct::ProviderError> {
        Ok(Box::new(CancelOnReturnCall(
            direct::ModelProvider::prepare(&self.0, request)?,
        )))
    }
}
impl direct::PreparedCall for CancelOnReturnCall {
    fn audit(&self) -> &direct::RequestAudit {
        self.0.audit()
    }
    fn cancel(self: Box<Self>) -> std::result::Result<(), direct::ProviderError> {
        self.0.cancel()
    }
    fn dispatch(
        self: Box<Self>,
        cancellation: direct::Cancellation,
    ) -> BoxFuture<'static, std::result::Result<direct::ProviderOutput, direct::ProviderError>>
    {
        Box::pin(async move {
            let output = self.0.dispatch(cancellation.clone()).await?;
            assert_eq!(output.disposition, direct::CompletionDisposition::Ready);
            cancellation.cancel();
            Ok(output)
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_complete_transport_is_held_retained_and_recoverable_without_retransmission() {
    for at_eof in [true, false] {
        let mut configuration = config();
        configuration.allow_direct_provider = true;
        let mut primary = seed("a");
        primary.model_confirmed = true;
        let mut fixture = Fixture::configured(true, configuration, primary, need());
        fixture.accept();
        let before = persisted_state(&fixture.app);
        let root = fixture.app.state_root().to_owned();
        let (provider, transport) = injected(&root, None, Behavior::WithEffects, None);
        let cancellation = direct::Cancellation::default();
        let provider: Box<dyn direct::ModelProvider> = if at_eof {
            *transport.cancel_at_eof.lock().unwrap() = Some(cancellation.clone());
            Box::new(provider)
        } else {
            Box::new(CancelOnReturn(provider))
        };
        let request = PrepareRequest {
            id: "cancelled-complete".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Direct,
            mode: Mode::Ordinary,
        };
        let report = fixture
            .app
            .step_with_provider(
                request.clone(),
                provider.as_ref(),
                cancellation.clone(),
                NOW,
            )
            .await
            .unwrap();
        assert!(cancellation.is_cancelled());
        assert_eq!(report.status, AdoptionStatus::Held);
        assert!(report
            .reason
            .as_deref()
            .unwrap()
            .contains("local cancellation"));
        assert!(report.applied.is_empty());
        assert!(report.accepted_epoch.is_none());
        assert!(!report.synthetic);
        assert!(report.usage_is_unknown);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert!(fixture.app.adopted(&request.id).is_err());
        let after = persisted_state(&fixture.app);
        for field in [
            "messages",
            "sources",
            "board",
            "local_deliveries",
            "interpretations",
        ] {
            assert_eq!(after[field], before[field], "unexpected {field} adoption");
        }
        assert_eq!(
            after["personas"]["a"]["character"],
            before["personas"]["a"]["character"]
        );
        assert_eq!(after["personas"]["a"]["active_invocation"], request.id);
        assert_eq!(after["personas"]["a"]["focus"]["work"]["permit"], false);
        assert_eq!(
            after["work_meta"]["work"]["attempts_left"]
                .as_u64()
                .unwrap(),
            before["work_meta"]["work"]["attempts_left"]
                .as_u64()
                .unwrap()
                - 1,
        );
        let inner = transport.inner.lock().unwrap().clone().unwrap();
        let inner_hash = s::sha256_bytes(inner.as_bytes());
        let carrier = serde_json::to_string(&json!({"decision_json":inner})).unwrap();
        let terminal = fixture.app.terminal_evidence(&request.id).unwrap();
        assert_eq!(terminal.body["receipt"]["kind"], "provider_complete");
        assert_eq!(
            terminal.body["receipt"]["completion_disposition"],
            "cancelled"
        );
        assert_eq!(
            terminal.body["receipt"]["attempt"]["response_completed"],
            true
        );
        assert_eq!(terminal.body["receipt"]["attempt"]["stream_eof"], true);
        assert_eq!(
            terminal.body["receipt"]["attempt"]["remote_stop_confirmed"],
            false
        );
        assert_eq!(
            terminal.body["receipt"]["attempt"]["remote_effects_and_charges_may_remain"],
            true
        );
        assert_eq!(
            terminal.body["receipt"]["usage"]["actual_cost_microusd"],
            Value::Null
        );
        assert_eq!(
            terminal.body["receipt"]["transport_sha256"],
            s::sha256_bytes(carrier.as_bytes())
        );
        assert_eq!(terminal.body["receipt"]["inner_sha256"], inner_hash);
        assert_eq!(
            terminal.body["response_body"]["text"]
                .as_str()
                .unwrap()
                .as_bytes(),
            inner.as_bytes()
        );
        assert_eq!(terminal.body["response_body"]["sha256"], inner_hash);
        assert_eq!(terminal.body["response_body"]["bytes"], inner.len());
        let invocation = fixture.app.invocation(&request.id).unwrap();
        assert_eq!(invocation.state, s::InvocationState::Failed);
        assert_eq!(
            invocation.response_hash.as_deref(),
            Some(inner_hash.as_str())
        );
        assert_eq!(terminal.body["prepare_hash"], invocation.prepare_hash);
        let reference = terminal.exact_ref().unwrap();
        assert!(invocation
            .history
            .iter()
            .any(|receipt| receipt.evidence.as_ref() == Some(&reference)));
        let usage: s::ReservationRecord =
            adoption_accounting_record(&root, "reservation", "v:cancelled-complete");
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert!(usage.measured.is_none());

        drop(fixture.app);
        let mut reopened = Runtime::open(&root).unwrap();
        assert_eq!(reopened.terminal_evidence(&request.id).unwrap(), terminal);
        assert_eq!(
            reopened
                .invocation(&request.id)
                .unwrap()
                .response_hash
                .as_deref(),
            Some(inner_hash.as_str())
        );
        reopened
            .acknowledge_complete_rejection("operator", &request.id, reference)
            .unwrap();
        reopened.resume("operator", "a", "work").unwrap();
        assert_eq!(reopened.terminal_evidence(&request.id).unwrap(), terminal);
        assert!(reopened.adopted(&request.id).is_err());
        let resumed = reopened.snapshot().unwrap();
        assert!(resumed.personas["a"].active_invocation.is_none());
        assert!(resumed.personas["a"].focus["work"].permit);
        assert_eq!(
            serde_json::to_value(&resumed.messages).unwrap(),
            before["messages"]
        );
        let recovery = resumed
            .observations
            .values()
            .find(|observation| {
                observation.kind == "terminal_recovery"
                    && observation.invocation.as_deref() == Some(request.id.as_str())
            })
            .unwrap();
        assert_eq!(recovery.payload["replay_authorized"], false);
        assert_eq!(
            recovery.payload["terminal"]["response_body"]["sha256"],
            inner_hash
        );
        assert!(recovery.payload["terminal"]["response_body"]
            .get("text")
            .is_none());
        assert!(reopened
            .step_with_provider(
                request,
                provider.as_ref(),
                direct::Cancellation::default(),
                NOW,
            )
            .await
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        let usage: s::ReservationRecord =
            adoption_accounting_record(&root, "reservation", "v:cancelled-complete");
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert!(usage.measured.is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_effort_reports_are_retained_and_conflicts_never_adopt() {
    for case in 0..7 {
        let mut configuration = config();
        configuration.allow_direct_provider = true;
        let mut primary = seed("a");
        primary.model_confirmed = true;
        primary.requested_effort = "xhigh".into();
        let mut fixture = Fixture::configured(true, configuration, primary, need());
        fixture.accept();
        let before = persisted_state(&fixture.app);
        let root = fixture.app.state_root().to_owned();
        let mut configuration = settings();
        configuration.effort = "xhigh".into();
        configuration.evidence.efforts = set(&["xhigh"]);
        let (provider, transport) =
            injected_with_settings(&root, None, Behavior::WithEffects, None, configuration);
        let (reports, reported, previous) = match case {
            0 => (None, None, None),
            1 => (Some((Value::Null, json!({"effort":null}))), None, None),
            2 => (
                Some((json!({"effort":"xhigh"}), json!({"effort":"xhigh"}))),
                Some("xhigh"),
                None,
            ),
            3 => (
                Some((Value::Null, json!({"effort":"low"}))),
                Some("low"),
                None,
            ),
            4 => (
                Some((json!({"effort":"xhigh"}), json!({"effort":"low"}))),
                Some("low"),
                Some("xhigh"),
            ),
            5 => (
                Some((json!({"effort":"low"}), json!({"effort":"xhigh"}))),
                Some("xhigh"),
                Some("low"),
            ),
            _ => (Some((Value::Null, json!({"effort":false}))), None, None),
        };
        *transport.effort_reports.lock().unwrap() = reports;
        let request = PrepareRequest {
            id: "native-effort".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Direct,
            mode: Mode::Ordinary,
        };
        let result = fixture
            .app
            .step_with_provider(
                request.clone(),
                &provider,
                direct::Cancellation::default(),
                NOW,
            )
            .await;
        if case < 3 {
            let report = result.unwrap();
            assert_eq!(report.status, AdoptionStatus::Adopted);
            assert!(report.applied.iter().any(|id| id == "fixture-message"));
            let after = persisted_state(&fixture.app);
            assert_eq!(
                after["messages"].as_object().unwrap().len(),
                before["messages"].as_object().unwrap().len() + 1
            );
            let adopted = fixture.app.adopted(&request.id).unwrap();
            assert_eq!(
                adopted.provider_observation.unwrap()["receipt"]["reported_effort"],
                json!(reported)
            );
        } else {
            match result.unwrap_err() {
                RuntimeError::Provider(direct::ProviderError::Attempt { failure, receipt }) => {
                    assert_eq!(
                        failure,
                        if case == 6 {
                            direct::Failure::Protocol
                        } else {
                            direct::Failure::ReportedEffortMismatch
                        }
                    );
                    assert_eq!(receipt.reported_effort.as_deref(), reported);
                    assert_eq!(receipt.previous_reported_effort.as_deref(), previous);
                    assert_eq!(receipt.audit.invocation_id, request.id);
                    assert_eq!(receipt.stream_eof, case == 3);
                    assert_eq!(receipt.response_completed, case == 3);
                }
                other => panic!("unexpected native effort failure: {other:?}"),
            }
            assert!(fixture.app.adopted(&request.id).is_err());
            let after = persisted_state(&fixture.app);
            for field in [
                "messages",
                "sources",
                "board",
                "local_deliveries",
                "interpretations",
            ] {
                assert_eq!(after[field], before[field], "unexpected {field} adoption");
            }
            assert_eq!(after["personas"]["a"]["active_invocation"], request.id);
            assert_eq!(after["personas"]["a"]["focus"]["work"]["permit"], false);
        }
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        let terminal = fixture.app.terminal_evidence(&request.id).unwrap();
        assert_eq!(terminal.body["receipt"]["requested_effort"], "xhigh");
        assert_eq!(terminal.body["receipt"]["reported_effort"], json!(reported));
        let attempt = if case < 3 {
            "attempt"
        } else {
            "attempt_receipt"
        };
        assert_eq!(
            terminal.body["receipt"][attempt]["reported_effort"],
            json!(reported)
        );
        assert_eq!(
            terminal.body["receipt"][attempt]["previous_reported_effort"],
            json!(previous)
        );
        let invocation = fixture.app.invocation(&request.id).unwrap();
        let admitted: Prepared = serde_json::from_value(invocation.request.unwrap()).unwrap();
        assert_eq!(
            terminal.body["receipt"][attempt]["audit"],
            admitted.provider_preparation.unwrap()
        );
        if case >= 3 {
            assert_eq!(terminal.body["receipt"]["kind"], "provider_attempt_failed");
            assert!(terminal.body.get("response_body").is_none());
            assert!(invocation.response_hash.is_none());
        }
        drop(fixture.app);
        let mut reopened = Runtime::open(&root).unwrap();
        assert_eq!(reopened.terminal_evidence(&request.id).unwrap(), terminal);
        if case >= 3 {
            assert!(reopened
                .acknowledge_complete_rejection(
                    "operator",
                    &request.id,
                    terminal.exact_ref().unwrap(),
                )
                .is_err());
            assert!(reopened.resume("operator", "a", "work").is_err());
            assert!(reopened.adopted(&request.id).is_err());
            assert_eq!(persisted_state(&reopened)["messages"], before["messages"]);
        }
        assert!(reopened
            .step_with_provider(request, &provider, direct::Cancellation::default(), NOW,)
            .await
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct OwnerCall {
    pub(super) root: PathBuf,
    pub(super) id: String,
    pub(super) response: String,
    pub(super) result: PathBuf,
    pub(super) release: PathBuf,
}

#[cfg(target_os = "linux")]
pub(super) async fn spawn_owner(call: &OwnerCall) -> tokio::process::Child {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "direct_provider_cases::execution_owner_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(&serde_json::to_vec(call).unwrap())
        .await
        .unwrap();
    drop(input);
    child
}

#[cfg(target_os = "linux")]
pub(super) async fn owner_output(call: &OwnerCall) -> std::process::Output {
    let child = spawn_owner(call).await;
    tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("injected execution owner must finish without waiting for another owner")
        .unwrap()
}

#[cfg(target_os = "linux")]
pub(super) fn owner_result(call: &OwnerCall, output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&std::fs::read(&call.result).unwrap()).unwrap()
}

// Only this subprocess entry point is ignored by the ordinary harness. The
// parent test invokes it explicitly and checks adoption, contention and death.
// All launch data travels over stdin; no credential or fixture env is consulted.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
#[ignore = "subprocess entry point exercised by injected_owner_step_allows_cli_status_fences_mutation_and_recovers_owner_death"]
async fn execution_owner_child() {
    let call: OwnerCall = serde_json::from_reader(std::io::stdin().lock()).unwrap();
    let mut app = Runtime::open(&call.root).unwrap();
    let (provider, _) = injected(
        &call.root,
        Some(call.response),
        Behavior::Valid,
        Some(call.release),
    );
    let result = app
        .step_with_provider(
            PrepareRequest {
                id: call.id,
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Direct,
                mode: Mode::Ordinary,
            },
            &provider,
            direct::Cancellation::default(),
            NOW,
        )
        .await;
    match result {
        Ok(report) => std::fs::write(call.result, serde_json::to_vec(&report).unwrap()).unwrap(),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
