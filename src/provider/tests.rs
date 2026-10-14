use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

struct TestClock(AtomicU64);
impl Clock for TestClock {
    fn unix_seconds(&self) -> Result<u64, Failure> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
struct Source(Mutex<Result<CredentialBundle, Failure>>);
impl CredentialSource for Source {
    fn load(
        &self,
        _: &CredentialReference,
        _: &SecretReference,
    ) -> Result<CredentialBundle, Failure> {
        self.0.lock().unwrap().clone()
    }
}
struct Body {
    bytes: Vec<u8>,
    position: usize,
    chunk: usize,
    pending: bool,
}
impl HttpBody for Body {
    fn read(&mut self, maximum: usize) -> BoxFuture<'_, Result<Option<Vec<u8>>, Failure>> {
        Box::pin(async move {
            if self.pending {
                return futures_util::future::pending().await;
            }
            if self.position == self.bytes.len() {
                return Ok(None);
            }
            let end = (self.position + maximum.min(self.chunk)).min(self.bytes.len());
            let bytes = self.bytes[self.position..end].to_vec();
            self.position = end;
            Ok(Some(bytes))
        })
    }
}
struct Transport {
    requests: Mutex<Vec<HttpRequest>>,
    stream: Vec<u8>,
    status: u16,
    pending: bool,
    fail: bool,
    entered: tokio::sync::Notify,
    calls: AtomicUsize,
}
impl HttpTransport for Transport {
    fn post(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(request);
            self.entered.notify_one();
            if self.fail {
                return Err(Failure::Network);
            }
            Ok(HttpResponse {
                status: self.status,
                is_unencoded_sse: true,
                body: Box::new(Body {
                    bytes: self.stream.clone(),
                    position: 0,
                    chunk: 1,
                    pending: self.pending,
                }),
            })
        })
    }
}
fn reference(name: &str) -> SecretReference {
    SecretReference { env: name.into() }
}
fn settings(mode: AccessMode) -> DirectSettings {
    let credentials = match mode {
        AccessMode::ChatgptOAuth | AccessMode::SiwcOAuth => CredentialReference::OAuth {
            grant: if mode == AccessMode::ChatgptOAuth {
                OAuthGrant::TraditionalChatgpt
            } else {
                OAuthGrant::SiwcTokenSharing
            },
            token_env: reference("FIXTURE_TOKEN"),
            account_env: reference("FIXTURE_ACCOUNT"),
            expires_at_unix: 200,
        },
        AccessMode::PlatformApiKey => CredentialReference::ApiKey {
            key_env: reference("FIXTURE_KEY"),
            account_env: reference("FIXTURE_ACCOUNT"),
        },
    };
    DirectSettings {
        mode,
        model: "model/exact:revision".into(),
        effort: "medium".into(),
        credentials,
        evidence: CapabilityEvidence {
            mode,
            model: "model/exact:revision".into(),
            efforts: BTreeSet::from(["medium".into()]),
            account_env: reference("FIXTURE_EVIDENCE_ACCOUNT"),
            account_sha256: digest(b"PRIVATE-ACCOUNT-SENTINEL"),
            transport_profile: mode.profile().into(),
            catalog_evidence_sha256: "1".repeat(64),
            structured_output_evidence_sha256: Some("2".repeat(64)),
        },
        limits: Limits {
            timeout_ms: 1000,
            max_prompt_bytes: 4096,
            max_schema_bytes: 4096,
            max_schema_nodes: 256,
            max_schema_depth: 16,
            max_request_bytes: 16_384,
            max_response_bytes: 4096,
            max_stream_bytes: 65_536,
            max_event_bytes: 16_384,
            max_events: 128,
        },
    }
}
fn bundle(mode: AccessMode) -> CredentialBundle {
    let secret = || Secret::new("PRIVATE-TOKEN-SENTINEL".into());
    let account = || Secret::new("PRIVATE-ACCOUNT-SENTINEL".into());
    CredentialBundle {
        credential: match mode {
            AccessMode::ChatgptOAuth => Credential::OAuth {
                grant: OAuthGrant::TraditionalChatgpt,
                bearer: secret(),
                account: account(),
                expires_at_unix: 200,
            },
            AccessMode::SiwcOAuth => Credential::OAuth {
                grant: OAuthGrant::SiwcTokenSharing,
                bearer: secret(),
                account: account(),
                expires_at_unix: 200,
            },
            AccessMode::PlatformApiKey => Credential::ApiKey {
                key: secret(),
                account: account(),
            },
        },
        evidence_account: account(),
    }
}
fn request() -> ProviderRequest {
    ProviderRequest {
        prompt: "  exact α\n\\literal\"\n".into(),
        output_schema: crate::runtime::provider_schema(),
        invocation_id: "request-1".into(),
    }
}
fn records(text: &str) -> Vec<Value> {
    vec![
        json!({"type":"response.created","response":{"id":"r1","status":"in_progress","model":"model/exact:revision","output":[]}}),
        json!({"type":"response.output_text.delta","item_id":"m1","output_index":0,"content_index":0,"delta":text}),
        json!({"type":"response.completed","response":{"id":"r1","status":"completed","model":"model/exact:revision","output":[
            {"id":"m1","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text}]}],
            "usage":{"input_tokens":11,"input_tokens_details":{"cached_tokens":3},"output_tokens":5,"total_tokens":16}}}),
    ]
}
fn stream(records: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        // A comment, CRLF, an explicit event name, and multi-line JSON data.
        let text = serde_json::to_string(record).unwrap();
        bytes.extend_from_slice(
            format!(
                ": fixture\r\nevent: {}\r\ndata: {{\r\ndata: {}\r\n\r\n",
                record["type"].as_str().unwrap(),
                &text[1..]
            )
            .as_bytes(),
        );
    }
    bytes
}
fn transport(bytes: Vec<u8>) -> Arc<Transport> {
    Arc::new(Transport {
        requests: Mutex::new(Vec::new()),
        stream: bytes,
        status: 200,
        pending: false,
        fail: false,
        entered: tokio::sync::Notify::new(),
        calls: AtomicUsize::new(0),
    })
}
fn make(
    settings: DirectSettings,
    source: Arc<Source>,
    http: Arc<Transport>,
    clock: Arc<TestClock>,
) -> DirectProvider {
    DirectProvider::new(settings, source, http, clock).unwrap()
}
fn collect(records: &[Value], chunk: usize) -> Result<String, Failure> {
    let settings = settings(AccessMode::PlatformApiKey);
    let mut collector = sse::Collector::new(&settings.model, &settings.limits);
    for part in stream(records).chunks(chunk) {
        collector.push(part)?;
    }
    collector.finish()
}

#[tokio::test]
async fn exact_prompt_schema_routing_and_usage_for_each_explicit_mode() {
    let wire_transport = ReqwestTransport::new().unwrap();
    for mode in [
        AccessMode::ChatgptOAuth,
        AccessMode::SiwcOAuth,
        AccessMode::PlatformApiKey,
    ] {
        let source = Arc::new(Source(Mutex::new(Ok(bundle(mode)))));
        let http = transport(stream(&records("{\"decision_json\":\"α\"}")));
        let provider = make(
            settings(mode),
            source,
            http.clone(),
            Arc::new(TestClock(AtomicU64::new(100))),
        );
        let original = request();
        let prompt = original.prompt.clone();
        let schema = serde_json::to_vec(&original.output_schema).unwrap();
        let prepared = provider.prepare(original).unwrap();
        let audit = prepared.audit().clone();
        let output = prepared.dispatch(Cancellation::default()).await.unwrap();
        assert_eq!(output.usage.input_tokens, Some(11));
        assert_eq!(output.usage.cached_input_tokens, Some(3));
        assert_eq!(output.usage.total_tokens, Some(16));
        assert_eq!(output.usage.actual_cost_microusd, None);
        assert!(output.attempt.stream_eof);
        assert!(!output.attempt.remote_stop_confirmed);
        assert_eq!(http.calls.load(Ordering::SeqCst), 1);
        let requests = http.requests.lock().unwrap();
        let request = &requests[0];
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(request.mode, mode);
        assert_eq!(
            body["input"][0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .as_bytes(),
            prompt.as_bytes()
        );
        assert_eq!(
            serde_json::to_vec(&body["text"]["format"]["schema"]).unwrap(),
            schema
        );
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(body["text"]["format"]["name"], "persona_decision");
        assert_eq!(body["model"], "model/exact:revision");
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body.as_object().unwrap().len(), 7);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(audit.request_sha256, digest(&request.body));
        assert_eq!(audit.schema_sha256, digest(&schema));
        assert_eq!(audit.prompt_sha256, digest(prompt.as_bytes()));
        assert_eq!(
            request.account_header.is_some(),
            mode == AccessMode::ChatgptOAuth
        );
        assert_eq!(
            mode.endpoint(),
            if mode == AccessMode::ChatgptOAuth {
                "https://chatgpt.com/backend-api/codex/responses"
            } else {
                "https://api.openai.com/v1/responses"
            }
        );
        let expected_profile = match mode {
            AccessMode::ChatgptOAuth => "traditional_chatgpt_responses_v2",
            AccessMode::SiwcOAuth => "siwc_documented_public_responses_v2",
            AccessMode::PlatformApiKey => "platform_responses_v2",
        };
        assert_eq!(mode.profile(), expected_profile);
        assert_eq!(audit.transport_profile, expected_profile);
        assert_eq!(
            body,
            json!({
                "model": "model/exact:revision",
                "store": false,
                "stream": true,
                "instructions": "Return only the JSON value required by text.format.",
                "input": [{"role":"user","content":[{"type":"input_text","text":prompt}]}],
                "reasoning": {"effort":"medium"},
                "text": {"format":{"type":"json_schema","name":"persona_decision",
                    "strict":true,"schema":serde_json::from_slice::<Value>(&schema).unwrap()}},
            })
        );
        assert_eq!(
            audit.instructions_sha256,
            digest(b"Return only the JSON value required by text.format.")
        );
        let configuration = settings(mode);
        assert_eq!(
            audit.configuration_sha256,
            digest(
                &encoded(
                    &json!({
                        "settings": &configuration,
                        "transport_profile": mode.wire_profile(),
                    }),
                    32_768
                )
                .unwrap()
            )
        );
        let wire = wire_transport
            .build_request(HttpRequest {
                mode: request.mode,
                body: request.body.clone(),
                bearer: request.bearer.clone(),
                account_header: request.account_header.clone(),
            })
            .unwrap();
        assert_eq!(wire.method(), reqwest::Method::POST);
        assert_eq!(
            wire.url().as_str(),
            if mode == AccessMode::ChatgptOAuth {
                "https://chatgpt.com/backend-api/codex/responses"
            } else {
                "https://api.openai.com/v1/responses"
            }
        );
        assert_eq!(
            wire.body().and_then(|body| body.as_bytes()),
            Some(request.body.as_slice())
        );
        let mut expected_headers = vec![
            ("authorization", "Bearer PRIVATE-TOKEN-SENTINEL"),
            ("content-type", "application/json"),
            ("accept", "text/event-stream"),
            ("accept-encoding", "identity"),
            ("user-agent", "ai-personas/direct-responses-v1"),
        ];
        if mode == AccessMode::ChatgptOAuth {
            expected_headers.extend([
                ("chatgpt-account-id", "PRIVATE-ACCOUNT-SENTINEL"),
                ("originator", "ai-personas"),
                ("openai-beta", "responses=experimental"),
            ]);
            assert!(wire
                .headers()
                .get("chatgpt-account-id")
                .unwrap()
                .is_sensitive());
        } else {
            for header in ["chatgpt-account-id", "originator", "openai-beta"] {
                assert!(!wire.headers().contains_key(header));
            }
        }
        assert_eq!(wire.headers().len(), expected_headers.len());
        for (name, value) in expected_headers {
            assert_eq!(wire.headers().get(name).unwrap().to_str().unwrap(), value);
        }
        assert!(wire.headers().get("authorization").unwrap().is_sensitive());
        let public = format!(
            "{:?}{}",
            request,
            serde_json::to_string(&output.attempt).unwrap()
        );
        assert!(!public.contains("PRIVATE-TOKEN"));
        assert!(!public.contains("PRIVATE-ACCOUNT"));
    }
}

#[tokio::test]
async fn missing_expired_wrong_grant_and_account_mismatch_never_post() {
    for case in 0..5 {
        let mode = AccessMode::SiwcOAuth;
        let mut credentials = bundle(mode);
        match case {
            1 => credentials.credential = bundle(AccessMode::ChatgptOAuth).credential,
            2 => credentials.credential = bundle(AccessMode::PlatformApiKey).credential,
            3 => credentials.evidence_account = Secret::new("another-account".into()),
            _ => (),
        }
        let source = Arc::new(Source(Mutex::new(if case == 0 {
            Err(Failure::CredentialMissing)
        } else {
            Ok(credentials)
        })));
        let http = transport(stream(&records("{}")));
        let provider = make(
            settings(mode),
            source,
            http.clone(),
            Arc::new(TestClock(AtomicU64::new(if case == 4 { 200 } else { 100 }))),
        );
        assert!(provider.prepare(request()).is_err());
        assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    }
    let mode = AccessMode::SiwcOAuth;
    let clock = Arc::new(TestClock(AtomicU64::new(100)));
    let source = Arc::new(Source(Mutex::new(Ok(bundle(mode)))));
    let http = transport(stream(&records("{}")));
    let provider = make(settings(mode), source, http.clone(), clock.clone());
    let prepared = provider.prepare(request()).unwrap();
    clock.0.store(200, Ordering::SeqCst);
    let error = prepared
        .dispatch(Cancellation::default())
        .await
        .unwrap_err();
    match error {
        ProviderError::Attempt { failure, receipt } => {
            assert_eq!(failure, Failure::CredentialExpired);
            assert!(!receipt.dispatch_may_have_begun);
            assert!(!receipt.remote_effects_and_charges_may_remain);
        }
        _ => panic!("missing attempt receipt"),
    }
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn capability_and_legacy_configuration_fail_closed() {
    let original = settings(AccessMode::SiwcOAuth);
    for case in 0..4 {
        let mut settings = original.clone();
        match case {
            0 => settings.evidence.structured_output_evidence_sha256 = None,
            1 => settings.evidence.model = "other".into(),
            2 => settings.evidence.mode = AccessMode::PlatformApiKey,
            _ => settings.effort = "unevidenced".into(),
        }
        assert!(settings.validate().is_err());
    }
    let error = serde_json::from_value::<DirectSettings>(
        json!({"executable":"private-cli-path","cli_version":"0.160.0"}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("replace legacy CLI"));
    assert!(!error.to_string().contains("private-cli-path"));
    let mut unknown = serde_json::to_value(original).unwrap();
    unknown["base_url"] = json!("https://example.invalid");
    assert!(serde_json::from_value::<DirectSettings>(unknown).is_err());
}

#[test]
fn sse_arbitrary_chunking_and_unknown_usage() {
    let text = " {\"decision_json\":\"α β\\n\"} ";
    for chunk in 1..=97 {
        assert_eq!(collect(&records(text), chunk).unwrap(), text);
    }
    let mut events = records(text);
    events[2]["response"]
        .as_object_mut()
        .unwrap()
        .remove("usage");
    let configuration = settings(AccessMode::PlatformApiKey);
    let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
    collector.push(&stream(&events)).unwrap();
    collector.finish().unwrap();
    assert_eq!(collector.usage, Usage::default());
    events[2]["response"]["usage"] = json!({"input_tokens":0,"output_tokens":0,"total_tokens":0});
    let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
    collector.push(&stream(&events)).unwrap();
    collector.finish().unwrap();
    assert_eq!(collector.usage.input_tokens, Some(0));
    assert_eq!(collector.usage.cached_input_tokens, None);
}

#[test]
fn no_partial_success_for_bad_streams_or_trailing_events() {
    for case in 0..16 {
        let mut events = records("{}");
        match case {
            0 => {
                events.pop();
            }
            1 => events[2]["type"] = json!("response.failed"),
            2 => events[2]["type"] = json!("response.incomplete"),
            3 => events[2]["response"]["status"] = json!("in_progress"),
            4 => events[2]["response"]["id"] = json!("another-response"),
            5 => events[2]["response"]["model"] = json!("different-model"),
            6 => events[2]["response"]["output"][0]["content"][0]["type"] = json!("refusal"),
            7 => events[2]["response"]["output"][0]["type"] = json!("function_call"),
            8 => events[1]["item_id"] = json!("conflicting-item"),
            9 => events[1]["delta"] = json!("different text"),
            10 => events.push(events[2].clone()),
            11 => events.push(json!({"type":"error","message":"PRIVATE-DIAGNOSTIC"})),
            12 => events[2]["response"]["usage"]["input_tokens"] = json!(-1),
            13 => events[2]["response"]["usage"]["input_tokens"] = json!(1.5),
            14 => {
                events[2]["response"]["usage"]["input_tokens_details"]["cached_tokens"] = json!(12)
            }
            _ => events[2]["response"]["usage"]["total_tokens"] = json!(99),
        }
        assert!(collect(&events, 1).is_err(), "case {case}");
    }
    let configuration = settings(AccessMode::PlatformApiKey);
    for bad in [
        b"data: not-json\n\n".as_slice(),
        b"data: {\"type\":\"error\",\"type\":\"response.completed\"}\n\n",
    ] {
        let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
        assert!(collector.push(bad).is_err());
        assert!(collector.finish().is_err());
    }
    let mut truncated = stream(&records("{}"));
    truncated.pop();
    let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
    collector.push(&truncated).unwrap();
    assert!(collector.finish().is_err());
}

#[test]
fn reasoning_is_separate_and_bounds_fail_closed() {
    let mut events = records("{}");
    events[1]["output_index"] = json!(1);
    let message = events[2]["response"]["output"][0].clone();
    events[2]["response"]["output"] = json!([
        {"type":"reasoning","id":"reason-1","summary":[{"type":"summary_text","text":"PRIVATE REASONING"}]}, message
    ]);
    assert_eq!(collect(&events, 1).unwrap(), "{}");
    for case in 0..4 {
        let mut configuration = settings(AccessMode::PlatformApiKey);
        match case {
            0 => configuration.limits.max_response_bytes = 1,
            1 => configuration.limits.max_event_bytes = 16,
            2 => configuration.limits.max_events = 1,
            _ => configuration.limits.max_stream_bytes = 32,
        }
        let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
        assert_eq!(collector.push(&stream(&records("{}"))), Err(Failure::Limit));
        assert!(collector.finish().is_err());
    }
}

#[tokio::test]
async fn cancellation_before_and_after_post_and_network_failure_are_not_retried() {
    for before in [true, false] {
        let mode = AccessMode::PlatformApiKey;
        let source = Arc::new(Source(Mutex::new(Ok(bundle(mode)))));
        let http = Arc::new(Transport {
            pending: true,
            ..Transport {
                requests: Mutex::new(Vec::new()),
                stream: vec![],
                status: 200,
                pending: false,
                fail: false,
                entered: tokio::sync::Notify::new(),
                calls: AtomicUsize::new(0),
            }
        });
        let provider = make(
            settings(mode),
            source,
            http.clone(),
            Arc::new(TestClock(AtomicU64::new(100))),
        );
        let prepared = provider.prepare(request()).unwrap();
        let cancellation = Cancellation::default();
        if before {
            cancellation.cancel();
        }
        let mut running = prepared.dispatch(cancellation.clone());
        if !before {
            tokio::select! {
                _ = http.entered.notified() => (),
                result = &mut running => panic!("ended before cancellation: {result:?}"),
            }
            cancellation.cancel();
        }
        let error = running.await.unwrap_err();
        if let ProviderError::Attempt { failure, receipt } = error {
            assert_eq!(failure, Failure::Cancelled);
            assert_eq!(receipt.dispatch_may_have_begun, !before);
            assert_eq!(receipt.remote_effects_and_charges_may_remain, !before);
            assert!(!receipt.remote_stop_confirmed);
        } else {
            panic!("missing receipt");
        }
        assert_eq!(http.calls.load(Ordering::SeqCst), usize::from(!before));
    }
    for status in [0, 401, 429, 503] {
        let mode = AccessMode::PlatformApiKey;
        let source = Arc::new(Source(Mutex::new(Ok(bundle(mode)))));
        let http = Arc::new(Transport {
            requests: Mutex::new(Vec::new()),
            stream: b"PRIVATE ERROR BODY".to_vec(),
            status,
            pending: false,
            fail: status == 0,
            entered: tokio::sync::Notify::new(),
            calls: AtomicUsize::new(0),
        });
        let provider = make(
            settings(mode),
            source,
            http.clone(),
            Arc::new(TestClock(AtomicU64::new(100))),
        );
        let error = provider
            .prepare(request())
            .unwrap()
            .dispatch(Cancellation::default())
            .await
            .unwrap_err();
        assert_eq!(http.calls.load(Ordering::SeqCst), 1);
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
        if let ProviderError::Attempt { receipt, .. } = error {
            assert!(receipt.dispatch_may_have_begun);
            assert_eq!(receipt.stream_bytes_observed, 0);
        } else {
            panic!("missing receipt");
        }
    }
}

#[test]
fn account_and_wire_profile_pins_are_required_and_validated() {
    let original = settings(AccessMode::SiwcOAuth);
    for field in ["account_sha256", "transport_profile"] {
        let mut document = serde_json::to_value(&original).unwrap();
        document["evidence"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<DirectSettings>(document).is_err());
    }
    for invalid in [
        String::new(),
        "0".repeat(63),
        "0".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let mut configuration = original.clone();
        configuration.evidence.account_sha256 = invalid;
        assert!(matches!(
            configuration.validate(),
            Err(ProviderError::Rejected(Failure::Configuration))
        ));
    }
    for profile in [
        "",
        AccessMode::PlatformApiKey.profile(),
        "siwc_documented_public_responses_v1",
    ] {
        let mut configuration = original.clone();
        configuration.evidence.transport_profile = profile.into();
        assert!(matches!(
            configuration.validate(),
            Err(ProviderError::Rejected(Failure::ModelUnavailable))
        ));
    }
    for (mode, name) in [
        (AccessMode::ChatgptOAuth, "chatgpt_oauth"),
        (AccessMode::SiwcOAuth, "siwc_oauth"),
        (AccessMode::PlatformApiKey, "platform_api_key"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), json!(name));
        assert_eq!(
            serde_json::from_value::<AccessMode>(json!(name)).unwrap(),
            mode
        );
    }
}

#[tokio::test]
async fn changing_both_account_sources_cannot_reuse_old_evidence() {
    for mode in [
        AccessMode::ChatgptOAuth,
        AccessMode::SiwcOAuth,
        AccessMode::PlatformApiKey,
    ] {
        let source = Arc::new(Source(Mutex::new(Ok(bundle(mode)))));
        let http = transport(stream(&records("{}")));
        let clock = Arc::new(TestClock(AtomicU64::new(100)));
        let provider = make(settings(mode), source.clone(), http.clone(), clock.clone());
        let prepared = provider.prepare(request()).unwrap();
        let mut changed = bundle(mode);
        let new_account = Secret::new("NEW-ACCOUNT-SENTINEL".into());
        match &mut changed.credential {
            Credential::OAuth { account, .. } | Credential::ApiKey { account, .. } => {
                *account = new_account.clone();
            }
        }
        changed.evidence_account = new_account;
        *source.0.lock().unwrap() = Ok(changed);
        match prepared
            .dispatch(Cancellation::default())
            .await
            .unwrap_err()
        {
            ProviderError::Attempt { failure, receipt } => {
                assert_eq!(failure, Failure::AccountMismatch);
                assert!(!receipt.dispatch_may_have_begun);
                assert!(!receipt.remote_effects_and_charges_may_remain);
            }
            _ => panic!("missing attempt receipt"),
        }
        // Repreparation has no previous in-memory account fence to rely on.
        assert!(matches!(
            provider.prepare(request()),
            Err(ProviderError::Rejected(Failure::AccountMismatch))
        ));
        let mut renamed = settings(mode);
        match &mut renamed.credentials {
            CredentialReference::OAuth { account_env, .. }
            | CredentialReference::ApiKey { account_env, .. } => {
                *account_env = reference("FIXTURE_NEW_ACCOUNT");
            }
        }
        renamed.evidence.account_env = reference("FIXTURE_NEW_EVIDENCE_ACCOUNT");
        let retargeted = make(renamed, source, http.clone(), clock);
        assert!(matches!(
            retargeted.prepare(request()),
            Err(ProviderError::Rejected(Failure::AccountMismatch))
        ));
        assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn exact_xhigh_effort_survives_preparation_dispatch_and_result_binding() {
    let mode = AccessMode::SiwcOAuth;
    let mut configuration = settings(mode);
    configuration.effort = "xhigh".into();
    configuration.evidence.efforts = BTreeSet::from(["xhigh".into()]);
    let source = Arc::new(Source(Mutex::new(Ok(bundle(mode)))));
    let http = transport(stream(&records("{}")));
    let provider = make(
        configuration,
        source,
        http.clone(),
        Arc::new(TestClock(AtomicU64::new(100))),
    );
    let prepared = provider.prepare(request()).unwrap();
    let audit = prepared.audit().clone();
    let output = prepared
        .dispatch(Cancellation::default())
        .await
        .unwrap()
        .bound_to(&audit, &provider.intent())
        .unwrap();
    assert_eq!(output.requested_effort, "xhigh");
    let requests = http.requests.lock().unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["reasoning"], json!({"effort":"xhigh"}));
}

#[tokio::test]
async fn offline_request_builder_rejects_misrouted_and_invalid_credentials() {
    let transport = ReqwestTransport::new().unwrap();
    let make_request = |mode: AccessMode, bearer: &str, account: Option<&str>| HttpRequest {
        mode,
        body: b"{}".to_vec(),
        bearer: Secret::new(bearer.into()),
        account_header: account.map(|value| Secret::new(value.into())),
    };
    assert!(matches!(
        transport.build_request(make_request(AccessMode::ChatgptOAuth, "FAKE-TOKEN", None,)),
        Err(Failure::InvalidCredential)
    ));
    for mode in [AccessMode::SiwcOAuth, AccessMode::PlatformApiKey] {
        assert!(matches!(
            transport.build_request(make_request(mode, "FAKE-TOKEN", Some("FAKE-ACCOUNT"),)),
            Err(Failure::GrantMismatch)
        ));
    }
    for bearer in ["", "bad token", "bad\r\nheader"] {
        assert!(matches!(
            transport.build_request(make_request(AccessMode::PlatformApiKey, bearer, None,)),
            Err(Failure::InvalidCredential)
        ));
    }
    for account in ["", "bad account", "bad\r\nheader"] {
        assert!(matches!(
            transport.build_request(make_request(
                AccessMode::ChatgptOAuth,
                "FAKE-TOKEN",
                Some(account),
            )),
            Err(Failure::InvalidCredential)
        ));
    }
}

#[test]
fn lifecycle_snapshots_cannot_smuggle_output_into_a_clean_completion() {
    let snapshots = [
        Some(json!([{"id":"tool-1","type":"function_call","name":"run","arguments":"{}"}])),
        Some(json!([{"id":"tool-1","type":"web_search_call","status":"completed"}])),
        Some(json!([{"id":"tool-1","type":"computer_call","status":"completed"}])),
        Some(
            json!([{"id":"m1","type":"message","status":"completed","role":"assistant",
            "content":[{"type":"refusal","refusal":"denied"}]}]),
        ),
        Some(
            json!([{"id":"m1","type":"message","status":"completed","role":"assistant",
            "content":[{"type":"output_text","text":"{}"}]}]),
        ),
        Some(json!([{"id":"reason-1","type":"reasoning","summary":[]} ])),
        Some(Value::Null),
        Some(json!({})),
        None,
    ];
    let configuration = settings(AccessMode::PlatformApiKey);
    for kind in ["response.created", "response.in_progress"] {
        for snapshot in &snapshots {
            let mut events = records("{}");
            let mut event = events[0].clone();
            event["type"] = json!(kind);
            match snapshot {
                Some(output) => event["response"]["output"] = output.clone(),
                None => {
                    event["response"].as_object_mut().unwrap().remove("output");
                }
            }
            if kind == "response.created" {
                events[0] = event;
            } else {
                events.insert(1, event);
            }
            assert_eq!(collect(&events, 1), Err(Failure::Protocol));
            let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
            assert_eq!(collector.push(&stream(&events)), Err(Failure::Protocol));
            assert!(!collector.completed);
            assert_eq!(collector.finish(), Err(Failure::Protocol));
        }
    }
    let mut events = records("{}");
    let mut progress = events[0].clone();
    progress["type"] = json!("response.in_progress");
    events.insert(1, progress);
    assert_eq!(collect(&events, 1).unwrap(), "{}");
}

#[test]
fn sse_response_headers_must_be_unambiguous_and_unencoded() {
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_ENCODING, CONTENT_TYPE};
    let mut headers = HeaderMap::new();
    assert!(!unencoded_sse(&headers));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    assert!(unencoded_sse(&headers));
    for encoding in ["identity", "IDENTITY"] {
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static(encoding));
        assert!(unencoded_sse(&headers));
    }
    for encoding in ["gzip", "br", "deflate", "zstd", "identity, gzip", ""] {
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static(encoding));
        assert!(!unencoded_sse(&headers));
    }
    headers.insert(CONTENT_ENCODING, HeaderValue::from_static("identity"));
    headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    assert!(!unencoded_sse(&headers));
    headers.remove(CONTENT_ENCODING);
    headers.append(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    assert!(!unencoded_sse(&headers));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    assert!(!unencoded_sse(&headers));
}

#[tokio::test]
async fn empty_chunks_allow_cancellation_before_another_ready_read() {
    struct EmptyBody {
        cancellation: Cancellation,
        reads: Arc<AtomicUsize>,
    }
    impl HttpBody for EmptyBody {
        fn read(&mut self, _: usize) -> BoxFuture<'_, Result<Option<Vec<u8>>, Failure>> {
            Box::pin(async move {
                let read = self.reads.fetch_add(1, Ordering::SeqCst);
                if read == 0 {
                    self.cancellation.cancel();
                }
                // A broken reader loop still terminates this fixture finitely.
                if read >= 8 {
                    return Err(Failure::Network);
                }
                Ok(Some(Vec::new()))
            })
        }
    }
    struct EmptyTransport {
        cancellation: Cancellation,
        reads: Arc<AtomicUsize>,
    }
    impl HttpTransport for EmptyTransport {
        fn post(&self, _: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
            Box::pin(async move {
                Ok(HttpResponse {
                    status: 200,
                    is_unencoded_sse: true,
                    body: Box::new(EmptyBody {
                        cancellation: self.cancellation.clone(),
                        reads: self.reads.clone(),
                    }),
                })
            })
        }
    }
    let mode = AccessMode::PlatformApiKey;
    let cancellation = Cancellation::default();
    let reads = Arc::new(AtomicUsize::new(0));
    let provider = DirectProvider::new(
        settings(mode),
        Arc::new(Source(Mutex::new(Ok(bundle(mode))))),
        Arc::new(EmptyTransport {
            cancellation: cancellation.clone(),
            reads: reads.clone(),
        }),
        Arc::new(TestClock(AtomicU64::new(100))),
    )
    .unwrap();
    match provider
        .prepare(request())
        .unwrap()
        .dispatch(cancellation)
        .await
        .unwrap_err()
    {
        ProviderError::Attempt { failure, receipt } => {
            assert_eq!(failure, Failure::Cancelled);
            assert!(receipt.dispatch_may_have_begun);
            assert!(receipt.remote_effects_and_charges_may_remain);
            assert!(!receipt.response_completed);
            assert!(!receipt.stream_eof);
            assert_eq!(receipt.stream_bytes_observed, 0);
        }
        _ => panic!("missing attempt receipt"),
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_at_eof_preserves_only_valid_complete_transport() {
    #[derive(Clone, Copy, Debug)]
    enum Tail {
        Eof,
        Empty,
        Network,
        Pending,
    }
    struct TailBody {
        body: Body,
        cancellation: Cancellation,
        tail: Tail,
    }
    impl HttpBody for TailBody {
        fn read(&mut self, maximum: usize) -> BoxFuture<'_, Result<Option<Vec<u8>>, Failure>> {
            Box::pin(async move {
                if self.body.position != self.body.bytes.len() {
                    return self.body.read(maximum).await;
                }
                self.cancellation.cancel();
                match self.tail {
                    Tail::Eof => Ok(None),
                    Tail::Empty => Ok(Some(Vec::new())),
                    Tail::Network => Err(Failure::Network),
                    Tail::Pending => futures_util::future::pending().await,
                }
            })
        }
    }
    struct TailTransport {
        bytes: Vec<u8>,
        cancellation: Cancellation,
        tail: Tail,
        calls: AtomicUsize,
    }
    impl HttpTransport for TailTransport {
        fn post(&self, _: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
            Box::pin(async move {
                assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
                Ok(HttpResponse {
                    status: 200,
                    is_unencoded_sse: true,
                    body: Box::new(TailBody {
                        body: Body {
                            bytes: self.bytes.clone(),
                            position: 0,
                            chunk: 8192,
                            pending: false,
                        },
                        cancellation: self.cancellation.clone(),
                        tail: self.tail,
                    }),
                })
            })
        }
    }
    let text = " \n{\"decision_json\":\"α\"}\n ";
    for complete in [true, false] {
        for tail in [Tail::Eof, Tail::Empty, Tail::Network, Tail::Pending] {
            let mut events = records(text);
            if !complete {
                events.pop();
            }
            let cancellation = Cancellation::default();
            let http = Arc::new(TailTransport {
                bytes: stream(&events),
                cancellation: cancellation.clone(),
                tail,
                calls: AtomicUsize::new(0),
            });
            let mode = AccessMode::PlatformApiKey;
            let provider = DirectProvider::new(
                settings(mode),
                Arc::new(Source(Mutex::new(Ok(bundle(mode))))),
                http.clone(),
                Arc::new(TestClock(AtomicU64::new(100))),
            )
            .unwrap();
            let prepared = provider.prepare(request()).unwrap();
            let audit = prepared.audit().clone();
            let result = prepared.dispatch(cancellation.clone()).await;
            assert!(cancellation.is_cancelled());
            assert_eq!(http.calls.load(Ordering::SeqCst), 1);
            if complete && matches!(tail, Tail::Eof) {
                let output = result
                    .unwrap()
                    .bound_to(&audit, &provider.intent())
                    .unwrap();
                assert_eq!(output.disposition, CompletionDisposition::Cancelled);
                assert_eq!(output.response.as_bytes(), text.as_bytes());
                assert_eq!(digest(output.response.as_bytes()), digest(text.as_bytes()));
                assert!(output.attempt.response_completed);
                assert!(output.attempt.stream_eof);
                assert_eq!(output.attempt.audit, audit);
                assert_eq!(output.attempt.usage.input_tokens, Some(11));
                assert!(!output.attempt.remote_stop_confirmed);
                assert!(output.attempt.remote_effects_and_charges_may_remain);
            } else {
                match result.unwrap_err() {
                    ProviderError::Attempt { failure, receipt } => {
                        assert_eq!(
                            failure,
                            match tail {
                                Tail::Eof => Failure::Protocol,
                                Tail::Network => Failure::Network,
                                Tail::Empty | Tail::Pending => Failure::Cancelled,
                            }
                        );
                        assert_eq!(receipt.response_completed, complete);
                        assert_eq!(receipt.stream_eof, matches!(tail, Tail::Eof));
                        assert_eq!(receipt.audit, audit);
                        assert!(receipt.dispatch_may_have_begun);
                        assert!(receipt.remote_effects_and_charges_may_remain);
                        assert!(!receipt.remote_stop_confirmed);
                    }
                    other => panic!("missing attempt receipt: {other:?}"),
                }
            }
        }
    }
}

fn effort_provider(events: &[Value]) -> (DirectProvider, Arc<Transport>) {
    let mode = AccessMode::PlatformApiKey;
    let mut configuration = settings(mode);
    configuration.effort = "xhigh".into();
    configuration.evidence.efforts = BTreeSet::from(["xhigh".into()]);
    let http = transport(stream(events));
    let provider = make(
        configuration,
        Arc::new(Source(Mutex::new(Ok(bundle(mode))))),
        http.clone(),
        Arc::new(TestClock(AtomicU64::new(100))),
    );
    (provider, http)
}

#[tokio::test]
async fn optional_effort_reports_remain_distinct_from_requested_effort() {
    for case in 0..8 {
        let mut events = records("{}");
        match case {
            0 => (),
            1 => {
                events[0]["response"]["reasoning"] = Value::Null;
                events[2]["response"]["reasoning"] = Value::Null;
            }
            2 => {
                events[0]["response"]["reasoning"] = json!({"effort":null});
                events[2]["response"]["reasoning"] = json!({"effort":null});
            }
            3 => events[2]["response"]["reasoning"] = json!({}),
            4 => {
                events[0]["response"]["reasoning"] = json!({"effort":"xhigh"});
                events[2]["response"]["reasoning"] = json!({"effort":"xhigh"});
            }
            5 => events[2]["response"]["reasoning"] = json!({"effort":"xhigh"}),
            6 => events[0]["response"]["reasoning"] = json!({"effort":"xhigh"}),
            _ => {
                events[0]["response"]["reasoning"] = json!({"effort":"xhigh"});
                events[2]["response"]["reasoning"] = json!({"effort":null});
                let mut progress = events[0].clone();
                progress["type"] = json!("response.in_progress");
                events.insert(1, progress);
            }
        }
        let (provider, http) = effort_provider(&events);
        let prepared = provider.prepare(request()).unwrap();
        let audit = prepared.audit().clone();
        let output = prepared
            .dispatch(Cancellation::default())
            .await
            .unwrap()
            .bound_to(&audit, &provider.intent())
            .unwrap();
        assert_eq!(output.requested_effort, "xhigh");
        assert_eq!(output.disposition, CompletionDisposition::Ready);
        assert_eq!(
            output.attempt.reported_effort.as_deref(),
            (case >= 4).then_some("xhigh")
        );
        assert_eq!(output.attempt.previous_reported_effort, None);
        assert_eq!(output.attempt.audit, audit);
        assert_eq!(http.calls.load(Ordering::SeqCst), 1);
        let requests = http.requests.lock().unwrap();
        let wire: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(wire["reasoning"]["effort"], "xhigh");
    }
}

#[test]
fn effort_report_shapes_and_metadata_bounds_are_checked_on_every_snapshot() {
    for reasoning in [
        json!(false),
        json!(17),
        json!("xhigh"),
        json!([]),
        json!({"effort":false}),
        json!({"effort":17}),
        json!({"effort":[]}),
        json!({"effort":{}}),
        json!({"effort":""}),
        json!({"effort":"bad effort"}),
        json!({"effort":"bad\neffort"}),
        json!({"effort":"x".repeat(129)}),
        json!({"effort":"λ".repeat(65)}),
    ] {
        for kind in [
            "response.created",
            "response.in_progress",
            "response.completed",
        ] {
            let mut events = records("{}");
            let mut snapshot = events[if kind == "response.completed" { 2 } else { 0 }].clone();
            snapshot["type"] = json!(kind);
            snapshot["response"]["reasoning"] = reasoning.clone();
            match kind {
                "response.created" => events[0] = snapshot,
                "response.completed" => events[2] = snapshot,
                _ => events.insert(1, snapshot),
            }
            let configuration = settings(AccessMode::PlatformApiKey);
            let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
            assert_eq!(
                collector.push(&stream(&events)),
                Err(Failure::Protocol),
                "{kind}: {reasoning}"
            );
            assert_eq!(collector.finish(), Err(Failure::Protocol));
            assert!(!collector.completed);
            assert_eq!(collector.reported_effort, None);
        }
    }
    let mut events = records("{}");
    events[2]["response"]["reasoning"] = json!({"effort":"λ".repeat(64)});
    let configuration = settings(AccessMode::PlatformApiKey);
    let mut collector = sse::Collector::new(&configuration.model, &configuration.limits);
    collector.push(&stream(&events)).unwrap();
    assert_eq!(collector.finish().unwrap(), "{}");
    assert_eq!(collector.reported_effort.unwrap().len(), 128);
}

#[tokio::test]
async fn effort_mismatches_and_snapshot_conflicts_preserve_bounded_attempt_reports() {
    for case in 0..4 {
        let mut events = records("{}");
        let (reported, previous, eof) = match case {
            0 => {
                events[2]["response"]["reasoning"] = json!({"effort":"low"});
                ("low", None, true)
            }
            1 => {
                events[0]["response"]["reasoning"] = json!({"effort":"xhigh"});
                events[2]["response"]["reasoning"] = json!({"effort":"low"});
                ("low", Some("xhigh"), false)
            }
            2 => {
                events[0]["response"]["reasoning"] = json!({"effort":"low"});
                events[2]["response"]["reasoning"] = json!({"effort":"xhigh"});
                ("xhigh", Some("low"), false)
            }
            _ => {
                events[0]["response"]["reasoning"] = json!({"effort":"xhigh"});
                events[2]["response"]["reasoning"] = json!({"effort":"xhigh"});
                let mut progress = events[0].clone();
                progress["type"] = json!("response.in_progress");
                progress["response"]["reasoning"] = json!({"effort":"low"});
                events.insert(1, progress);
                ("low", Some("xhigh"), false)
            }
        };
        let (provider, http) = effort_provider(&events);
        let prepared = provider.prepare(request()).unwrap();
        let audit = prepared.audit().clone();
        match prepared
            .dispatch(Cancellation::default())
            .await
            .unwrap_err()
        {
            ProviderError::Attempt { failure, receipt } => {
                assert_eq!(failure, Failure::ReportedEffortMismatch);
                assert_eq!(receipt.reported_effort.as_deref(), Some(reported));
                assert_eq!(receipt.previous_reported_effort.as_deref(), previous);
                assert_eq!(receipt.stream_eof, eof);
                assert_eq!(receipt.response_completed, eof);
                assert_eq!(receipt.audit, audit);
                assert!(receipt.dispatch_may_have_begun);
                assert!(receipt.remote_effects_and_charges_may_remain);
                assert!(!receipt.remote_stop_confirmed);
            }
            other => panic!("missing attempt receipt: {other:?}"),
        }
        assert_eq!(http.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn result_binding_checks_effort_reports_and_audits_rejected_attempts() {
    for case in 0..4 {
        let mut events = records("{}");
        events[2]["response"]["reasoning"] = json!({"effort":"xhigh"});
        let (provider, http) = effort_provider(&events);
        let prepared = provider.prepare(request()).unwrap();
        let audit = prepared.audit().clone();
        let mut output = prepared.dispatch(Cancellation::default()).await.unwrap();
        let observed = output.attempt.stream_bytes_observed;
        match case {
            0 => output.attempt.reported_effort = Some("low".into()),
            1 => output.attempt.reported_effort = Some("x".repeat(129)),
            2 => output.attempt.previous_reported_effort = Some("low".into()),
            _ => {
                output.audit.request_sha256 = "0".repeat(64);
                output.attempt.audit = output.audit.clone();
            }
        }
        match output.bound_to(&audit, &provider.intent()).unwrap_err() {
            ProviderError::Attempt { failure, receipt } => {
                assert_eq!(failure, Failure::Binding);
                assert_eq!(receipt.audit, audit);
                assert_eq!(receipt.stream_bytes_observed, observed);
                assert!(receipt.response_completed && receipt.stream_eof);
                assert_eq!(receipt.usage.input_tokens, Some(11));
                assert_eq!(
                    receipt.reported_effort.as_deref(),
                    match case {
                        0 => Some("low"),
                        1 => None,
                        _ => Some("xhigh"),
                    }
                );
                assert_eq!(
                    receipt.previous_reported_effort.as_deref(),
                    (case == 2).then_some("low")
                );
                assert!(receipt.dispatch_may_have_begun);
                assert!(receipt.remote_effects_and_charges_may_remain);
            }
            other => panic!("missing binding receipt: {other:?}"),
        }
        assert_eq!(http.calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn product_provider_and_native_step_have_no_coding_harness_adapter() {
    for source in [
        include_str!("../provider.rs"),
        include_str!("../runtime.rs"),
        include_str!("../launch.rs"),
    ] {
        for forbidden in [
            "tokio::process",
            "std::process::Command",
            "CodexProvider",
            "dispatch_research",
            "cli_environment",
            "app-server",
        ] {
            assert!(
                !source.contains(forbidden),
                "obsolete adapter token: {forbidden}"
            );
        }
    }
}
