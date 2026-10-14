//! Fixed native SIWC execution over an already configured Store.
use super::*;
use crate::auth::native::VaultCredentialSource;
use std::sync::Arc;

pub enum NativeInput<'a> {
    Need(&'a str),
    Work(&'a str),
}

fn rejected(failure: provider::Failure) -> RuntimeError {
    provider::ProviderError::Rejected(failure).into()
}

/// Calling this entry explicitly authorizes one existing bounded launch run.
/// There is no caller-selected provider, transport, executable or endpoint.
pub async fn native_run(
    root: &Path,
    operator: &str,
    vault: &Path,
    input: NativeInput<'_>,
) -> Result<Output> {
    require(
        root.is_absolute() && vault.is_absolute(),
        "explicit absolute paths required",
    )?;
    identity(operator)?;
    match &input {
        NativeInput::Need(request) => text(request, 32_768)?,
        NativeInput::Work(work) => identity(work)?,
    }
    // Ordinary open may recover orphan dispatches. Do not load launch settings
    // with launch::open before acquiring the owner below.
    let mut app = Runtime::open(root)?;
    run_opened(&mut app, operator, input, |settings| {
        let source = Arc::new(VaultCredentialSource::open(vault, settings).map_err(rejected)?);
        let provider = provider::DirectProvider::new(
            settings.clone(),
            source.clone(),
            Arc::new(provider::ReqwestTransport::new()?),
            Arc::new(provider::SystemClock),
        )?;
        Ok((source, provider))
    })
    .await
}

fn binding(saved: &Saved) -> Result<serde_json::Value> {
    let mut binding = saved.clone();
    // These two fields advance through the ordinary launch loop.
    binding.works.clear();
    binding.steps = 0;
    Ok(serde_json::to_value(binding)?)
}

fn validate_saved(
    app: &mut Runtime,
    saved: &Saved,
    operator: &str,
) -> Result<(DirectProviderSettings, serde_json::Value)> {
    require(saved.operator == operator, "saved operator mismatch")?;
    let settings = saved.execution_provider()?;
    let setup = &saved.setup;
    require(
        setup.share_cohort
            && (2..=8).contains(&setup.cohort)
            && (1..=setup.cohort).contains(&setup.owner)
            && (1..=128).contains(&setup.max_steps)
            && (1..=128).contains(&setup.max_considerations)
            && (1..=128).contains(&setup.max_work_attempts)
            && setup.max_total_steps > 0
            && setup.max_total_steps
                <= storage_limits().total.attempts - storage_limits().protected_closeout.attempts
            && (1..=128).contains(&setup.max_works)
            && saved.steps <= setup.max_total_steps
            && saved.works.len() as u64 <= setup.max_works,
        "invalid saved foreground bounds",
    )?;
    let (head, state) = app.load()?;
    let config = &state.config;
    config.validate()?;
    require(
        config.allow_direct_provider
            && !config.require_physical_hard_bound
            && !config.require_token_hard_bound
            && !config.require_dollar_hard_bound
            && config.max_work_attempts == setup.max_work_attempts
            && settings.limits.max_prompt_bytes <= config.context_bytes
            && settings.limits.max_response_bytes <= config.response_bytes
            && settings.limits.max_stream_bytes as u64 <= config.transport_output_reserve_bytes,
        "saved runtime does not admit the fixed native provider",
    )?;
    for id in people(setup.cohort) {
        require(id != operator, "requester collides with a cohort identity")?;
        let persona = state
            .personas
            .get(&id)
            .ok_or_else(|| invalid("saved cohort is incomplete"))?;
        require(
            serde_json::to_value(&persona.seed)?
                == serde_json::to_value(seed_for(saved, &settings, &id))?,
            "saved cohort binding changed",
        )?;
    }
    let working = fs::canonicalize(&saved.working_directory)?;
    require(
        working == saved.working_directory
            && working.is_dir()
            && !working.starts_with(app.state_root())
            && !app.state_root().starts_with(&working),
        "saved working directory binding changed",
    )?;
    Ok((settings, head.body["config"].clone()))
}

// Private construction seam: production supplies only the fixed closure above;
// offline tests supply a synthetic transport after the same owner admission.
async fn run_opened(
    app: &mut Runtime,
    operator: &str,
    input: NativeInput<'_>,
    connect: impl FnOnce(
        &DirectProviderSettings,
    ) -> Result<(Arc<VaultCredentialSource>, provider::DirectProvider)>,
) -> Result<Output> {
    let owner = app.store.acquire_execution_owner()?;
    let (_, saved) = load(app)?;
    let (settings, config) = validate_saved(app, &saved, operator)?;
    if let NativeInput::Work(work) = &input {
        require(saved.works.contains_key(*work), "unknown launch work")?;
    }
    let launch = binding(&saved)?;
    let (source, provider) = connect(&settings)?;
    let mut driver = NativeDriver {
        owner: &owner,
        source,
        provider,
        launch,
        config,
    };
    driver.before_step(app)?;
    let work = match input {
        NativeInput::Need(request) => start(app, request)?,
        NativeInput::Work(work) => work.to_owned(),
    };
    let result = run_with(app, &work, &mut driver).await;
    drop(driver);
    drop(owner);
    result
}

struct NativeDriver<'a> {
    owner: &'a s::ExecutionOwner,
    source: Arc<VaultCredentialSource>,
    provider: provider::DirectProvider,
    launch: serde_json::Value,
    config: serde_json::Value,
}

impl Driver for NativeDriver<'_> {
    fn before_step(&mut self, app: &mut Runtime) -> Result<()> {
        let (_, saved) = load(app)?;
        require(
            binding(&saved)? == self.launch,
            "saved launch binding changed",
        )?;
        let (head, _) = app.load()?;
        require(
            head.body["config"] == self.config,
            "saved runtime configuration changed",
        )?;
        self.source.check().map_err(rejected)
    }

    async fn step(
        &mut self,
        app: &mut Runtime,
        request: PrepareRequest,
        saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        require(
            binding(saved)? == self.launch,
            "reserved launch binding changed",
        )?;
        self.before_step(app)?;
        app.step_with_owned_provider(
            request,
            &self.provider,
            provider::Cancellation::default(),
            self.owner,
            now,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{self, Clock as _};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use futures_util::future::BoxFuture;
    use std::{
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };

    const SECRET: &str = "NATIVE_SYNTHETIC_SECRET_MUST_NOT_APPEAR";

    fn write_private(path: &Path, value: &serde_json::Value) {
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, Runtime, PathBuf) {
        let directory = tempfile::Builder::new()
            .prefix("personas-native-")
            .tempdir_in(fs::canonicalize("/tmp").unwrap())
            .unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("selected.json");
        let guard = auth::Vault::open(&path).unwrap().lock().unwrap();
        let host = guard.host_id(&mut auth::OsEntropy).unwrap();
        let now = auth::SystemClock.now_unix();
        let header =
            URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"native-fixture","typ":"JWT"}"#);
        // Protected-load fixture only, with no signed identity or live evidence.
        write_private(
            &path,
            &json!({
                "version":1,"issuer":auth::ISSUER,"subject":"native-fixture-subject",
                "client_id":"native-fixture-client","ext_agent_host_id":host,
                "callback_registration":"http://127.0.0.1/auth/callback",
                "nonce":URL_SAFE_NO_PAD.encode([9_u8;32]),"access_token":SECRET,
                "refresh_token":format!("{SECRET}_REFRESH"),"id_token":format!("{header}.e30.AQ"),
                "token_type":"Bearer","scopes":["openid","resource.invoke","chatgpt.tokens.use.direct"],
                "saved_at_unix":now,"expires_at_unix":now+3600,"earliest_refresh_at":now
            }),
        );
        let account = guard.load().unwrap().unwrap().account_binding();
        drop(guard);
        let provider: DirectProviderSettings = serde_json::from_value(json!({
            "mode":"siwc_oauth","model":"native-fixture-model","effort":"high",
            "credentials":{"kind":"o_auth","grant":"siwc_token_sharing",
                "token_env":{"env":auth::ACCESS_TOKEN_ENV},
                "account_env":{"env":auth::ACCOUNT_BINDING_ENV},"expires_at_unix":now+3600},
            "evidence":{"mode":"siwc_oauth","model":"native-fixture-model","efforts":["high"],
                "account_env":{"env":auth::ACCOUNT_BINDING_ENV},"account_sha256":s::sha256_bytes(account.as_bytes()),
                "transport_profile":provider::AccessMode::SiwcOAuth.profile(),
                "catalog_evidence_sha256":"1".repeat(64),"structured_output_evidence_sha256":"2".repeat(64)},
            "limits":{"timeout_ms":1000,"max_prompt_bytes":196608,"max_schema_bytes":262144,
                "max_schema_nodes":65536,"max_schema_depth":64,"max_request_bytes":2097152,
                "max_response_bytes":32768,"max_stream_bytes":131072,"max_event_bytes":98304,"max_events":1024}
        })).unwrap();
        fs::create_dir(directory.path().join("working")).unwrap();
        let app = setup(
            &directory.path().join("state"),
            "requester",
            Setup {
                cohort: 2,
                owner: 1,
                destination: "fixture".into(),
                provider,
                working_directory: directory.path().join("working"),
                share_cohort: true,
                max_steps: 1,
                max_considerations: 128,
                max_work_attempts: 32,
                max_total_steps: 32,
                max_works: 4,
            },
            true,
        )
        .unwrap();
        (directory, app, path)
    }

    fn assert_locked(root: &Path, vault: &Path) {
        assert!(matches!(
            auth::Vault::open(vault).unwrap().lock(),
            Err(auth::AuthError::Busy)
        ));
        let mut peer = Runtime::open(root).unwrap();
        assert!(peer.store.acquire_execution_owner().is_err());
    }

    struct FixtureHttp {
        root: PathBuf,
        vault: PathBuf,
        calls: AtomicUsize,
    }
    struct Body {
        root: PathBuf,
        vault: PathBuf,
        bytes: Vec<u8>,
        offset: usize,
    }
    impl provider::HttpBody for Body {
        fn read(
            &mut self,
            maximum: usize,
        ) -> BoxFuture<'_, std::result::Result<Option<Vec<u8>>, provider::Failure>> {
            Box::pin(async move {
                assert_locked(&self.root, &self.vault); // Includes the EOF read.
                if self.offset == self.bytes.len() {
                    return Ok(None);
                }
                let end = (self.offset + maximum).min(self.bytes.len());
                let bytes = self.bytes[self.offset..end].to_vec();
                self.offset = end;
                Ok(Some(bytes))
            })
        }
    }
    impl provider::HttpTransport for FixtureHttp {
        fn post(
            &self,
            request: provider::HttpRequest,
        ) -> BoxFuture<'_, std::result::Result<provider::HttpResponse, provider::Failure>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert_locked(&self.root, &self.vault);
                assert_eq!(request.mode, provider::AccessMode::SiwcOAuth);
                assert!(request.account_header.is_none());
                assert!(request.bearer.expose() == SECRET);
                assert!(!format!("{request:?}").contains(SECRET));
                let mut peer = Runtime::open(&self.root).unwrap();
                let prepared = decode_prepared(&peer.invocation("call-00000001").unwrap()).unwrap();
                let decision = json!({
                    "version":1,"invocation_id":prepared.request.id,"epoch":prepared.epoch,
                    "work":{"kind":"ongoing","reason":"Synthetic unfinished work.","evidence":[]},
                    "fragment_maintenance":{"kind":"change","reason":"Initial character.","operation_ids":["voice","character"]},
                    "asset_maintenance":{"kind":"no_change","reason":"Retain."},
                    "next_fragments":{"kind":"clear","reason":"No optional fragments."},
                    "next_assets":{"kind":"clear","reason":"No optional assets."},
                    "continuation":{"kind":"stop","reason":"Explicit stop."},
                    "reply":{"kind":"authored","audience":["requester"],"text":"Synthetic retained message."},
                    "operations":[
                        {"kind":"file_write","id":"voice","scope":{"kind":"fragments"},
                         "source":"native-voice","path":"voice.txt","base_revision":null,
                         "text":"My accepted synthetic own voice.","qualifications":[],"policy_id":POLICY},
                        {"kind":"character_adopt","id":"character","base_revision":null,
                         "ocean":{"openness":"Modeled","conscientiousness":"Modeled","extraversion":"Modeled","agreeableness":"Modeled","neuroticism":"Modeled"},
                         "vad":{"valence":"Modeled","activation":"Modeled","control":"Modeled"},
                         "own_voice":[{"source":"native-voice","revision":"proposal-op:voice","representation":"utf8"}],
                         "reason":"Explicit initial account.","evidence":[]}
                    ]
                });
                let carrier = json!({"decision_json":decision.to_string()}).to_string();
                let part = json!({"type":"output_text","text":carrier,"annotations":[]});
                let message = json!({"id":"msg_native","type":"message","role":"assistant","status":"completed","content":[part]});
                let events = vec![
                    json!({"type":"response.created","response":{"id":"resp_native","object":"response","status":"in_progress","model":prepared.requested_model,"output":[]}}),
                    json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_native","type":"message","role":"assistant","status":"in_progress","content":[]}}),
                    json!({"type":"response.content_part.added","item_id":"msg_native","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
                    json!({"type":"response.output_text.delta","item_id":"msg_native","output_index":0,"content_index":0,"delta":carrier}),
                    json!({"type":"response.output_text.done","item_id":"msg_native","output_index":0,"content_index":0,"text":carrier}),
                    json!({"type":"response.content_part.done","item_id":"msg_native","output_index":0,"content_index":0,"part":part}),
                    json!({"type":"response.output_item.done","output_index":0,"item":message}),
                    json!({"type":"response.completed","response":{"id":"resp_native","object":"response","status":"completed","model":prepared.requested_model,"reasoning":{"effort":prepared.requested_effort},"output":[message],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
                ];
                let mut bytes = Vec::new();
                for (sequence, mut event) in events.into_iter().enumerate() {
                    event["sequence_number"] = json!(sequence);
                    bytes.extend_from_slice(
                        format!(
                            "event: {}\ndata: {}\n\n",
                            event["type"].as_str().unwrap(),
                            event
                        )
                        .as_bytes(),
                    );
                }
                Ok(provider::HttpResponse {
                    status: 200,
                    is_unencoded_sse: true,
                    body: Box::new(Body {
                        root: self.root.clone(),
                        vault: self.vault.clone(),
                        bytes,
                        offset: 0,
                    }),
                })
            })
        }
    }

    fn transport(app: &Runtime, vault: &Path) -> Arc<FixtureHttp> {
        Arc::new(FixtureHttp {
            root: app.state_root().to_owned(),
            vault: vault.to_owned(),
            calls: AtomicUsize::new(0),
        })
    }
    fn connect_fixture(
        settings: &DirectProviderSettings,
        path: &Path,
        http: Arc<FixtureHttp>,
    ) -> Result<(Arc<VaultCredentialSource>, provider::DirectProvider)> {
        use provider::CredentialSource as _;
        let source = Arc::new(VaultCredentialSource::open(path, settings).map_err(rejected)?);
        let bundle = source
            .load(&settings.credentials, &settings.evidence.account_env)
            .map_err(rejected)?;
        assert!(!format!("{bundle:?}").contains(SECRET));
        let provider = provider::DirectProvider::new(
            settings.clone(),
            source.clone(),
            http,
            Arc::new(provider::SystemClock),
        )?;
        Ok((source, provider))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bad_admission_never_reserves_or_calls_http() {
        for case in 0..9 {
            let (_directory, mut app, path) = fixture();
            let (old, mut saved) = load(&mut app).unwrap();
            let p = &mut saved.setup.provider;
            match case {
                0 => {
                    p["mode"] = json!("chatgpt_oauth");
                    p["evidence"]["mode"] = json!("chatgpt_oauth");
                    p["evidence"]["transport_profile"] =
                        json!(provider::AccessMode::ChatgptOAuth.profile());
                    p["credentials"]["grant"] = json!("traditional_chatgpt");
                }
                1 => p["credentials"]["token_env"]["env"] = json!("OTHER_TOKEN"),
                2 => p["credentials"]["account_env"]["env"] = json!("OTHER_ACCOUNT"),
                3 => p["evidence"]["account_env"]["env"] = json!("OTHER_ACCOUNT"),
                4 => p["evidence"]["account_sha256"] = json!("0".repeat(64)),
                5 => {
                    p["credentials"]["expires_at_unix"] = json!(auth::SystemClock.now_unix() + 7200)
                }
                6 => p["credentials"]["expires_at_unix"] = json!(1),
                7 => {
                    let mut record: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    record["refresh_uncertain"] = json!(vec![7_u8; 32]);
                    write_private(&path.with_file_name("selected.json.refresh"), &record);
                }
                8 => p["limits"]["max_prompt_bytes"] = json!(196609),
                _ => unreachable!(),
            }
            save(&mut app, Some(&old), &saved).unwrap();
            let before = serde_json::to_value(&saved).unwrap();
            let attempts = app.ledger().unwrap().ordinary.consumed.attempts;
            let http = transport(&app, &path);
            let result = run_opened(
                &mut app,
                "requester",
                NativeInput::Need("Synthetic need."),
                |settings| connect_fixture(settings, &path, http.clone()),
            )
            .await;
            assert!(result.is_err(), "case {case}");
            assert!(!result.err().unwrap().to_string().contains(SECRET));
            assert_eq!(http.calls.load(Ordering::SeqCst), 0);
            assert_eq!(
                serde_json::to_value(load(&mut app).unwrap().1).unwrap(),
                before
            );
            assert_eq!(app.ledger().unwrap().ordinary.consumed.attempts, attempts);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn owner_contention_precedes_settings_and_credentials() {
        let (_directory, mut app, path) = fixture();
        let (old, mut saved) = load(&mut app).unwrap();
        saved.setup.provider = json!({"invalid":"must not be read before ownership"});
        save(&mut app, Some(&old), &saved).unwrap();
        let owner = app.store.acquire_execution_owner().unwrap();
        let before = app.ledger().unwrap();
        let http = transport(&app, &path);
        let mut source_opens = 0;
        let result = run_opened(
            &mut app,
            "requester",
            NativeInput::Need("Synthetic need."),
            |settings| {
                source_opens += 1;
                connect_fixture(settings, &path, http.clone())
            },
        )
        .await;
        assert!(matches!(result, Err(RuntimeError::Store(_))));
        assert_eq!(source_opens, 0);
        assert_eq!(http.calls.load(Ordering::SeqCst), 0);
        assert_eq!(app.ledger().unwrap(), before);
        assert_eq!(
            serde_json::to_value(load(&mut app).unwrap().1).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
        drop(owner);
    }

    #[test]
    fn held_source_rechecks_new_refresh_fences() {
        let (_directory, mut app, path) = fixture();
        let settings = load(&mut app).unwrap().1.execution_provider().unwrap();
        let source = VaultCredentialSource::open(&path, &settings).unwrap();
        source.check().unwrap();
        assert!(matches!(
            auth::Vault::open(&path).unwrap().lock(),
            Err(auth::AuthError::Busy)
        ));
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["refresh_uncertain"] = json!(vec![7_u8; 32]);
        write_private(&path.with_file_name("selected.json.refresh"), &record);
        assert!(source.check().is_err());
        drop(source);
        assert!(auth::Vault::open(&path).unwrap().lock().is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_owned_transport_uses_ordinary_adoption_and_initial_handover() {
        let (_directory, mut app, path) = fixture();
        let http = transport(&app, &path);
        let output = run_opened(
            &mut app,
            "requester",
            NativeInput::Need("Synthetic native need."),
            |settings| connect_fixture(settings, &path, http.clone()),
        )
        .await
        .unwrap();
        assert_eq!(http.calls.load(Ordering::SeqCst), 1);
        let (_, saved) = load(&mut app).unwrap();
        assert_eq!(saved.steps, 1);
        let id = saved.works["work-00000001"].as_deref().unwrap();
        let invocation = app.invocation(id).unwrap();
        assert_eq!(invocation.state, s::InvocationState::Adopted);
        assert_eq!(
            decode_prepared(&invocation).unwrap().request.profile,
            Profile::Direct
        );
        let adopted = app.adopted(id).unwrap();
        assert_eq!(adopted.report.status, AdoptionStatus::CharacterOnly);
        assert!(handed_over(&app.snapshot().unwrap(), &adopted.actor));
        let encoded = serde_json::to_string(
            &json!({"output":output,"terminal":app.terminal_evidence(id).unwrap()}),
        )
        .unwrap();
        assert!(!encoded.contains(SECRET));
        assert!(auth::Vault::open(&path).unwrap().lock().is_ok());
        assert!(app.store.acquire_execution_owner().is_ok());
    }
}
