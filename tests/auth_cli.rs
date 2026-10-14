//! Offline CLI and boundary tests. Synthetic vaults, injected failures and
//! structural JWT fixtures exercise the production helpers without external HTTP
//! or live authentication. These tests do not establish live model capability.

#[path = "../src/bin/personas-auth.rs"]
#[allow(dead_code)]
mod helper;

use ai_personas::auth::{
    AuthError, Clock, Entropy, IdTokenVerifier, TokenRequest, TokenResponse, TokenTransport, Vault,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    future::Future,
    os::unix::fs::{symlink, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    pin::Pin,
    process::{Command, Output},
    sync::Mutex,
    time::Duration,
};

const CANARY: &str = "AUTH_CLI_SYNTHETIC_SECRET_MUST_NOT_APPEAR";

fn cli(vault: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_personas-auth"))
        .arg("--vault")
        .arg(vault)
        .args(arguments)
        .env("PERSONAS_SIWC_ACCESS_TOKEN", CANARY)
        .env("PERSONAS_SIWC_ACCOUNT_BINDING", CANARY)
        .env("OPENAI_API_KEY", CANARY)
        .output()
        .unwrap()
}

fn private_directory() -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("personas-auth-cli-")
        .tempdir_in(std::fs::canonicalize("/tmp").unwrap())
        .unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn redacted_failure(output: &Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(CANARY));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["automatic_retry"], false);
}

#[test]
fn help_needs_no_vault_or_authorization() {
    for arguments in [
        vec!["--help"],
        vec!["sign-in", "--help"],
        vec!["refresh", "--help"],
        vec!["exec", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_personas-auth"))
            .args(&arguments)
            .env("PERSONAS_SIWC_ACCESS_TOKEN", CANARY)
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(!help.contains(CANARY));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(CANARY));
        if arguments.first() == Some(&"exec") {
            assert!(help.contains("Unavailable"));
            assert!(help.contains("execution-owner-bound"));
            assert!(help.contains("saved-configuration"));
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_personas-auth"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("migrate-provider"));
    assert!(help.contains("not proof of actual capability"));
    assert!(help.contains("same host"));
    assert!(help.contains("unavailable"));
    assert!(help.contains("execution-owner-bound"));
    assert!(help.contains("saved-configuration"));
}

#[test]
fn authorization_and_argument_errors_are_redacted_before_any_io() {
    let directory = private_directory();
    let vault = directory.path().join("missing-parent").join(CANARY);
    for arguments in [
        vec!["sign-in"],
        vec!["refresh"],
        vec!["exec", "--unexpected", CANARY],
        vec!["sign-in", "--authorize", "--callback-port", CANARY],
        vec!["sign-in", "--authorize", "--unexpected", CANARY],
    ] {
        let output = cli(&vault, &arguments);
        assert_eq!(output.status.code(), Some(2));
        redacted_failure(&output);
    }
    assert!(!vault.parent().unwrap().exists());
    let output = Command::new(env!("CARGO_BIN_EXE_personas-auth"))
        .arg("status")
        .output()
        .unwrap();
    redacted_failure(&output);
}

#[test]
fn relative_repository_and_symlink_vaults_are_refused_before_authentication() {
    redacted_failure(&cli(Path::new(CANARY), &["sign-in", "--authorize"]));
    let repository_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(CANARY)
        .join("vault.json");
    redacted_failure(&cli(&repository_path, &["sign-in", "--authorize"]));
    assert!(!repository_path.parent().unwrap().exists());

    let directory = private_directory();
    let protected = directory.path().join("protected");
    fs::create_dir(&protected).unwrap();
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = directory.path().join("alias");
    symlink(&protected, &alias).unwrap();
    redacted_failure(&cli(&alias.join(CANARY), &["sign-in", "--authorize"]));
    assert!(!protected.join(CANARY).exists());
    let target = protected.join("target");
    fs::write(&target, CANARY).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let link = protected.join("vault");
    symlink(&target, &link).unwrap();
    redacted_failure(&cli(&link, &["sign-in", "--authorize"]));
    assert_eq!(fs::read_to_string(target).unwrap(), CANARY);
}

#[test]
fn vault_parent_creation_and_permissions_are_explicit() {
    let directory = private_directory();
    let parent = directory.path().join("new-private-parent");
    let vault = parent.join("credentials.json");
    assert!(helper::prepare_vault_path(&vault, false).is_err());
    assert!(!parent.exists());
    assert!(!helper::prepare_vault_path(&vault, true).unwrap());
    assert_eq!(
        fs::metadata(&parent).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    assert!(!vault.exists());
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(helper::prepare_vault_path(&vault, true).is_err());
    assert_eq!(
        fs::metadata(&parent).unwrap().permissions().mode() & 0o7777,
        0o755
    );
    assert!(helper::prepare_vault_path(
        &directory.path().join("new").join("nested").join("vault"),
        true
    )
    .is_err());
    assert!(!directory.path().join("new").exists());
    assert!(
        helper::prepare_vault_path(Path::new(&format!("/{}", "x".repeat(4096))), false).is_err()
    );
}

#[test]
fn external_git_worktree_paths_are_refused() {
    let directory = private_directory();
    fs::write(directory.path().join(".git"), "gitdir: synthetic").unwrap();
    assert!(helper::prepare_vault_path(&directory.path().join("vault.json"), true).is_err());
    assert!(!directory.path().join("vault.json").exists());
}

#[test]
fn status_does_not_discover_credentials_or_echo_invalid_selected_contents() {
    let directory = private_directory();
    let home = directory.path().join("synthetic-home");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(home.join(".codex/auth.json"), CANARY).unwrap();
    let vault = directory.path().join("selected.json");
    let output = Command::new(env!("CARGO_BIN_EXE_personas-auth"))
        .arg("--vault")
        .arg(&vault)
        .arg("status")
        .env("HOME", &home)
        .env("PERSONAS_SIWC_ACCESS_TOKEN", CANARY)
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["grant_connected"], false);
    assert_eq!(value["capability_verified"], false);
    assert_eq!(value["inference_performed"], false);
    assert!(!vault.exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(CANARY));
    fs::write(&vault, CANARY).unwrap();
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o600)).unwrap();
    redacted_failure(&cli(&vault, &["status"]));
    assert_eq!(fs::read_to_string(&vault).unwrap(), CANARY);
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(helper::prepare_vault_path(&vault, false).is_err());
}

fn callback(target: &str, headers: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\n{headers}\r\n\r\n").into_bytes()
}

const FIXTURE_CLIENT: &str = "cli-issued-client";
const FIXTURE_CALLBACK: &str = "http://127.0.0.1:43210/auth/callback";

struct FixtureClock(u64);

impl Clock for FixtureClock {
    fn now_unix(&self) -> u64 {
        self.0
    }
}

struct FixtureEntropy(u8);

impl Entropy for FixtureEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), AuthError> {
        self.0 = self.0.checked_add(1).ok_or(AuthError::Entropy)?;
        out.fill(self.0);
        Ok(())
    }
}

fn fixture_verifier() -> IdTokenVerifier {
    // A synthetic public modulus is sufficient for these failed exchanges.
    // No signing key, real identity, or successful token response is involved.
    IdTokenVerifier::from_jwks(
        &serde_json::to_vec(&json!({
            "keys": [{
                "kty": "RSA",
                "kid": "cli-fixture",
                "n": URL_SAFE_NO_PAD.encode([0x81_u8; 256]),
                "e": "AQAB",
                "alg": "RS256",
                "use": "sig"
            }]
        }))
        .unwrap(),
    )
    .unwrap()
}

fn authorization_parameters(attempt: &helper::SignInAttempt) -> BTreeMap<String, String> {
    assert!(attempt
        .authorization_url()
        .starts_with("https://auth.openai.com/api/accounts/authorize?"));
    let url = url::Url::parse(attempt.authorization_url()).unwrap();
    let parameters: BTreeMap<String, String> = url.query_pairs().into_owned().collect();
    assert!(parameters.keys().all(|key| matches!(
        key.as_str(),
        "client_id"
            | "agent_name_hint"
            | "ext_agent_host_id"
            | "response_type"
            | "redirect_uri"
            | "scope"
            | "resource"
            | "state"
            | "nonce"
            | "code_challenge_method"
            | "code_challenge"
    )));
    assert!(!parameters.contains_key("id_token_hint"));
    parameters
}

fn returned_callback(parameters: &BTreeMap<String, String>, code: &str) -> String {
    let mut callback = url::Url::parse(&parameters["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &parameters["state"])
        .append_pair("code", code)
        .append_pair("client_id", FIXTURE_CLIENT);
    callback.into()
}

fn fixture_sidecar(path: &Path, suffix: &str) -> PathBuf {
    path.with_file_name(format!(
        "{}.{suffix}",
        path.file_name().unwrap().to_str().unwrap()
    ))
}

fn registration_document(host: &str) -> Value {
    json!({
        "version": 1,
        "host_id": host,
        "callback_registration": "http://127.0.0.1/auth/callback",
        "client_id": FIXTURE_CLIENT
    })
}

fn write_private_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn assert_receipt(path: &Path, expected: &Value) {
    let path = fixture_sidecar(path, "registration");
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    assert!(metadata.len() <= 2048);
    let bytes = fs::read(path).unwrap();
    // Exact shape excludes codes, state, nonce, PKCE secrets and all tokens.
    assert_eq!(&serde_json::from_slice::<Value>(&bytes).unwrap(), expected);
}

struct FailingExchange {
    path: PathBuf,
    registration: Value,
    failure: AuthError,
    requests: Mutex<Vec<TokenRequest>>,
}

impl FailingExchange {
    fn new(path: &Path, parameters: &BTreeMap<String, String>, failure: AuthError) -> Self {
        Self {
            path: path.to_owned(),
            registration: registration_document(&parameters["ext_agent_host_id"]),
            failure,
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl TokenTransport for FailingExchange {
    fn send<'a>(
        &'a self,
        request: TokenRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TokenResponse, AuthError>> + Send + 'a>> {
        // Check before even constructing the transport future: the receipt is
        // already protected on disk and the selected account is still locked.
        assert_receipt(&self.path, &self.registration);
        assert!(matches!(
            Vault::open(&self.path).unwrap().lock(),
            Err(AuthError::Busy)
        ));
        self.requests.lock().unwrap().push(request);
        let failure = self.failure;
        Box::pin(async move { Err(failure) })
    }
}

fn assert_code_request(request: &TokenRequest, parameters: &BTreeMap<String, String>, code: &str) {
    let form: BTreeMap<String, String> = request.form.iter().cloned().collect();
    assert_eq!(request.endpoint, ai_personas::auth::TOKEN_ENDPOINT);
    assert_eq!(request.form.len(), 6);
    assert_eq!(form.len(), 6);
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["client_id"], FIXTURE_CLIENT);
    assert_eq!(form["code"], code);
    assert_eq!(form["redirect_uri"], parameters["redirect_uri"]);
    assert_eq!(form["resource"], ai_personas::auth::RESOURCE);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())),
        parameters["code_challenge"]
    );
}

#[tokio::test]
async fn failed_exchange_retains_client_for_explicit_sign_in_and_pending_status() {
    for failure in [
        AuthError::Transport,
        AuthError::Timeout,
        AuthError::Protocol,
    ] {
        let directory = private_directory();
        let path = directory.path().join("selected.json");
        let guard = Vault::open(&path).unwrap().lock().unwrap();
        let clock = FixtureClock(1_700_000_000);
        let mut entropy = FixtureEntropy(0);
        let verifier = fixture_verifier();
        let first =
            helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock)
                .unwrap();
        let first_parameters = authorization_parameters(&first);
        assert_eq!(first_parameters["client_id"], "dynamic_agent_client");
        assert_eq!(first_parameters["agent_name_hint"], "AI Personas");
        let transport = FailingExchange::new(&path, &first_parameters, failure);
        let callback = returned_callback(&first_parameters, CANARY);
        assert_eq!(
            first
                .complete(&guard, &callback, &transport, &verifier, &clock)
                .await
                .unwrap_err(),
            failure
        );
        {
            let requests = transport.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_code_request(&requests[0], &first_parameters, CANARY);
        }
        assert!(guard.load().unwrap().is_none());
        assert_eq!(
            guard.pending_registration().unwrap().unwrap().client_id(),
            FIXTURE_CLIENT
        );
        assert_receipt(&path, &transport.registration);
        let receipt = fs::read(fixture_sidecar(&path, "registration")).unwrap();
        assert!(!path.exists());
        drop(guard);

        // Exercise the real status command while the credential file is absent.
        let output = cli(&path, &["status"]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            json!({
                "status": "registration_pending",
                "grant_connected": false,
                "grant_record_present": false,
                "registration_present": true,
                "account_verified": false,
                "capability_verified": false,
                "inference_performed": false
            })
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains(CANARY));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);

        // A separately initiated attempt reloads the protected receipt and may
        // select a new loopback port, but must reuse the issued client and host.
        let guard = Vault::open(&path).unwrap().lock().unwrap();
        let next = helper::SignInAttempt::begin(
            &guard,
            "http://127.0.0.1:43211/auth/callback".to_owned(),
            &mut entropy,
            &clock,
        )
        .unwrap();
        let next_parameters = authorization_parameters(&next);
        assert_eq!(next_parameters["client_id"], FIXTURE_CLIENT);
        assert_eq!(
            next_parameters["ext_agent_host_id"],
            first_parameters["ext_agent_host_id"]
        );
        assert!(!next_parameters.contains_key("agent_name_hint"));
        for key in ["state", "nonce", "code_challenge"] {
            assert_ne!(next_parameters[key], first_parameters[key]);
        }
        let next_code = format!("{CANARY}_SECOND");
        let callback = returned_callback(&next_parameters, &next_code);
        assert_eq!(
            next.complete(&guard, &callback, &transport, &verifier, &clock)
                .await
                .unwrap_err(),
            failure
        );
        {
            let requests = transport.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_code_request(&requests[1], &next_parameters, &next_code);
        }
        assert_eq!(
            fs::read(fixture_sidecar(&path, "registration")).unwrap(),
            receipt
        );
        assert!(guard.load().unwrap().is_none());

        let rejected =
            helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock)
                .unwrap();
        let parameters = authorization_parameters(&rejected);
        let callback = returned_callback(&parameters, CANARY)
            .replace(FIXTURE_CLIENT, "different-issued-client");
        assert_eq!(
            rejected
                .complete(&guard, &callback, &transport, &verifier, &clock)
                .await
                .unwrap_err(),
            AuthError::Binding
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 2);
        assert_eq!(
            fs::read(fixture_sidecar(&path, "registration")).unwrap(),
            receipt
        );
        assert!(!path.exists());
    }
}

#[tokio::test]
async fn callback_tampering_never_calls_retention_or_exchange() {
    for tamper in [
        "state",
        "duplicate-state",
        "empty-code",
        "missing-client",
        "dynamic-client",
        "wrong-host",
        "wrong-port",
        "wrong-path",
        "fragment",
        "denied",
        "mixed-error",
        "unknown-field",
        "expired",
    ] {
        let directory = private_directory();
        let path = directory.path().join("selected.json");
        let guard = Vault::open(&path).unwrap().lock().unwrap();
        let clock = FixtureClock(1_700_000_000);
        let mut entropy = FixtureEntropy(0);
        let verifier = fixture_verifier();
        let attempt =
            helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock)
                .unwrap();
        let parameters = authorization_parameters(&attempt);
        let transport = FailingExchange::new(&path, &parameters, AuthError::Transport);
        let valid = returned_callback(&parameters, CANARY);
        let callback = match tamper {
            "state" => valid.replace(parameters["state"].as_str(), "wrong-state"),
            "duplicate-state" => format!("{valid}&%73tate=duplicate"),
            "empty-code" => valid.replace(&format!("code={CANARY}"), "code="),
            "missing-client" => valid.replace(&format!("&client_id={FIXTURE_CLIENT}"), ""),
            "dynamic-client" => valid.replace(FIXTURE_CLIENT, "dynamic_agent_client"),
            "wrong-host" => valid.replace("127.0.0.1", "localhost"),
            "wrong-port" => valid.replace(":43210/", ":43211/"),
            "wrong-path" => valid.replace("/auth/callback", "/wrong"),
            "fragment" => format!("{valid}#fragment"),
            "denied" => format!(
                "{}?state={}&error=access_denied",
                parameters["redirect_uri"], parameters["state"]
            ),
            "mixed-error" => format!("{valid}&error=access_denied"),
            "unknown-field" => format!("{valid}&id_token_hint={CANARY}"),
            "expired" => valid,
            _ => unreachable!(),
        };
        let callback_clock = FixtureClock(clock.0 + if tamper == "expired" { 600 } else { 0 });
        let retention_calls = Cell::new(0);
        let result = attempt
            .complete_with_retention(
                &guard,
                &callback,
                &transport,
                &verifier,
                &callback_clock,
                |registration| {
                    retention_calls.set(retention_calls.get() + 1);
                    guard.retain_pending_registration(registration)
                },
            )
            .await;
        assert!(result.is_err(), "accepted tampering: {tamper}");
        assert_eq!(retention_calls.get(), 0);
        assert!(transport.requests.lock().unwrap().is_empty());
        assert!(guard.pending_registration().unwrap().is_none());
        assert!(guard.load().unwrap().is_none());
        assert!(!fixture_sidecar(&path, "registration").exists());
        assert!(!path.exists());
    }
}

#[tokio::test]
async fn registration_write_failure_or_uncertain_durability_prevents_exchange() {
    for failure in [AuthError::Vault, AuthError::CommitUncertain] {
        let directory = private_directory();
        let path = directory.path().join("selected.json");
        let guard = Vault::open(&path).unwrap().lock().unwrap();
        let clock = FixtureClock(1_700_000_000);
        let mut entropy = FixtureEntropy(0);
        let verifier = fixture_verifier();
        let attempt =
            helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock)
                .unwrap();
        let parameters = authorization_parameters(&attempt);
        let callback = returned_callback(&parameters, CANARY);
        let transport = FailingExchange::new(&path, &parameters, AuthError::Transport);
        let retention_calls = Cell::new(0);
        let result = attempt
            .complete_with_retention(
                &guard,
                &callback,
                &transport,
                &verifier,
                &clock,
                |registration| {
                    retention_calls.set(retention_calls.get() + 1);
                    // Inject failure before publication, or an uncertain result
                    // after publication. Neither permits a token request.
                    if failure == AuthError::CommitUncertain {
                        guard.retain_pending_registration(registration)?;
                    }
                    Err(failure)
                },
            )
            .await;
        assert_eq!(result.unwrap_err(), failure);
        assert_eq!(retention_calls.get(), 1);
        assert!(transport.requests.lock().unwrap().is_empty());
        assert!(guard.load().unwrap().is_none());
        assert!(!path.exists());
        assert_eq!(
            guard.pending_registration().unwrap().is_some(),
            failure == AuthError::CommitUncertain
        );
        if failure == AuthError::CommitUncertain {
            assert_receipt(&path, &transport.registration);
            let next = helper::SignInAttempt::begin(
                &guard,
                FIXTURE_CALLBACK.to_owned(),
                &mut entropy,
                &clock,
            )
            .unwrap();
            let parameters = authorization_parameters(&next);
            assert_eq!(parameters["client_id"], FIXTURE_CLIENT);
            assert!(!parameters.contains_key("agent_name_hint"));
        }
    }
}

#[tokio::test]
async fn blocked_registration_destination_prevents_exchange_through_real_writer() {
    let directory = private_directory();
    let path = directory.path().join("selected.json");
    let guard = Vault::open(&path).unwrap().lock().unwrap();
    let clock = FixtureClock(1_700_000_000);
    let mut entropy = FixtureEntropy(0);
    let verifier = fixture_verifier();
    let attempt =
        helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock)
            .unwrap();
    let parameters = authorization_parameters(&attempt);
    let callback = returned_callback(&parameters, CANARY);
    let transport = FailingExchange::new(&path, &parameters, AuthError::Transport);
    let destination = fixture_sidecar(&path, "registration");
    // Unlike a chmod-only failure, this obstruction also fails when tests run
    // as root. Install it after begin to exercise the production retention step.
    fs::create_dir(&destination).unwrap();
    assert_eq!(
        attempt
            .complete(&guard, &callback, &transport, &verifier, &clock)
            .await
            .unwrap_err(),
        AuthError::Vault
    );
    assert!(transport.requests.lock().unwrap().is_empty());
    assert!(destination.is_dir());
    assert!(guard.load().unwrap().is_none());
    assert!(!path.exists());
}

fn synthetic_credential(host: &str, now: u64) -> Value {
    // Protected-load fixture only: this JWT has a valid structural header but
    // no valid signature. It is never supplied as a successful token response.
    let header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "alg": "RS256", "kid": "cli-fixture", "typ": "JWT"
        }))
        .unwrap(),
    );
    json!({
        "version": 1,
        "issuer": "https://auth.openai.com",
        "subject": "fixture-subject",
        "client_id": FIXTURE_CLIENT,
        "ext_agent_host_id": host,
        "callback_registration": "http://127.0.0.1/auth/callback",
        "nonce": URL_SAFE_NO_PAD.encode([9_u8; 32]),
        "access_token": format!("{CANARY}_ACCESS"),
        "refresh_token": format!("{CANARY}_REFRESH"),
        "id_token": format!("{header}.e30.AQ"),
        "token_type": "Bearer",
        "scopes": ["openid", "resource.invoke", "chatgpt.tokens.use.direct"],
        "saved_at_unix": now,
        "expires_at_unix": now + 3600,
        "earliest_refresh_at": now
    })
}

#[test]
fn returning_sign_in_keeps_selected_client_and_rejects_receipt_mismatches() {
    let directory = private_directory();
    let path = directory.path().join("selected.json");
    let guard = Vault::open(&path).unwrap().lock().unwrap();
    let clock = FixtureClock(1_700_000_000);
    let mut entropy = FixtureEntropy(0);
    let host = guard.host_id(&mut entropy).unwrap();
    write_private_json(&path, &synthetic_credential(&host, clock.0));
    let receipt = fixture_sidecar(&path, "registration");
    write_private_json(&receipt, &registration_document(&host));
    let original = fs::read(&path).unwrap();
    let attempt =
        helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock)
            .unwrap();
    let parameters = authorization_parameters(&attempt);
    assert_eq!(parameters["client_id"], FIXTURE_CLIENT);
    assert_eq!(parameters["ext_agent_host_id"], host);
    assert!(!parameters.contains_key("agent_name_hint"));
    for field in ["client_id", "host_id"] {
        let mut conflicting = registration_document(&host);
        conflicting[field] = json!("different-issued-value");
        write_private_json(&receipt, &conflicting);
        assert!(matches!(
            helper::SignInAttempt::begin(&guard, FIXTURE_CALLBACK.to_owned(), &mut entropy, &clock,),
            Err(AuthError::Binding)
        ));
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}

#[test]
fn record_status_rejects_fenced_and_stale_unexpired_credentials() {
    let directory = private_directory();
    let path = directory.path().join("selected.json");
    let guard = Vault::open(&path).unwrap().lock().unwrap();
    let clock = FixtureClock(1_700_000_000);
    let host = guard.host_id(&mut FixtureEntropy(0)).unwrap();
    let mut document = synthetic_credential(&host, clock.0);
    write_private_json(&path, &document);
    let stale = guard.load().unwrap().unwrap();
    assert_eq!(
        helper::record_status(&stale, &clock)["grant_connected"],
        true
    );

    document["access_token"] = json!(format!("{CANARY}_REPLACEMENT"));
    write_private_json(&path, &document);
    assert_eq!(
        helper::record_status(&stale, &clock)["grant_connected"],
        false
    );
    let current = guard.load().unwrap().unwrap();
    assert_eq!(
        helper::record_status(&current, &clock)["grant_connected"],
        true
    );

    let mut recovery = document.clone();
    recovery["refresh_uncertain"] = json!(vec![7_u8; 32]);
    write_private_json(&fixture_sidecar(&path, "refresh"), &recovery);
    let fenced = guard.load().unwrap().unwrap();
    for record in [&stale, &current, &fenced] {
        assert!(record.expires_at_unix() > clock.now_unix());
        let status = helper::record_status(record, &clock);
        assert_eq!(status["status"], "grant_unavailable");
        assert_eq!(status["grant_connected"], false);
        assert_eq!(status["grant_record_present"], true);
        assert_eq!(status["capability_verified"], false);
        assert_eq!(status["inference_performed"], false);
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains(CANARY));
        for field in ["access_token", "refresh_token", "id_token", "nonce"] {
            assert!(!encoded.contains(document[field].as_str().unwrap()));
        }
    }
}

#[test]
fn pending_status_rejects_unprotected_or_oversized_receipts() {
    let directory = private_directory();
    let path = directory.path().join("selected.json");
    let guard = Vault::open(&path).unwrap().lock().unwrap();
    let host = guard.host_id(&mut FixtureEntropy(0)).unwrap();
    drop(guard);
    let receipt = fixture_sidecar(&path, "registration");
    write_private_json(&receipt, &registration_document(&host));
    fs::set_permissions(&receipt, fs::Permissions::from_mode(0o644)).unwrap();
    redacted_failure(&cli(&path, &["status"]));
    fs::write(&receipt, vec![b'x'; 2049]).unwrap();
    fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600)).unwrap();
    redacted_failure(&cli(&path, &["status"]));
    assert!(!path.exists());
}

#[test]
fn callback_parser_preserves_the_exact_url_and_checks_the_actual_port() {
    let request = callback(
        "/auth/callback?code=synthetic%2Bcode&state=synthetic",
        "Host: 127.0.0.1:43210\r\nAccept: text/html",
    );
    assert_eq!(
        helper::parse_callback_request(&request, 43210).unwrap(),
        "http://127.0.0.1:43210/auth/callback?code=synthetic%2Bcode&state=synthetic"
    );
    assert!(helper::parse_callback_request(&request, 43211).is_err());
    assert!(helper::parse_callback_request(&request, 0).is_err());
}

#[test]
fn callback_parser_rejects_hosts_paths_duplicates_fragments_and_bodies() {
    let target = "/auth/callback?code=synthetic&state=synthetic";
    for headers in [
        "Host: localhost:43210",
        "Host: 0.0.0.0:43210",
        "Host: 127.0.0.1",
        "Host: 127.0.0.1:43211",
        "Accept: text/html",
        "Host: 127.0.0.1:43210\r\nhOsT: 127.0.0.1:43210",
        "Host: 127.0.0.1:43210\r\nX-Test: one\r\nx-test: two",
        "Host: 127.0.0.1:43210\r\nContent-Length: 0",
        "Host: 127.0.0.1:43210\r\nContent-Length: 4",
        "Host: 127.0.0.1:43210\r\nTransfer-Encoding: chunked",
        "Host: 127.0.0.1:43210\r\nExpect: 100-continue",
        " Host: 127.0.0.1:43210",
        "Host: 127.0.0.1:43210\r\n folded-header",
        "Host: 127.0.0.1:43210\nX-Test: bare-lf",
    ] {
        assert!(helper::parse_callback_request(&callback(target, headers), 43210).is_err());
    }
    for target in [
        "/wrong?code=synthetic&state=synthetic",
        "/auth/callback/extra?code=synthetic&state=synthetic",
        "/auth/%63allback?code=synthetic&state=synthetic",
        "/auth/callback?code=synthetic&state=synthetic#fragment",
        "/auth/callback?code=synthetic&state=one&state=two",
        "/auth/callback?code=one&%63ode=two&state=synthetic",
        "/auth/callback?code=%ZZ&state=synthetic",
        "/auth/callback?code=%0a&state=synthetic",
        "/auth/callback?",
        "http://127.0.0.1:43210/auth/callback?code=synthetic",
    ] {
        assert!(
            helper::parse_callback_request(&callback(target, "Host: 127.0.0.1:43210"), 43210)
                .is_err()
        );
    }
    let valid = callback(target, "Host: 127.0.0.1:43210");
    let mut body = valid.clone();
    body.extend_from_slice(b"body");
    assert!(helper::parse_callback_request(&body, 43210).is_err());
    let mut pipelined = valid.clone();
    pipelined.extend_from_slice(&valid);
    assert!(helper::parse_callback_request(&pipelined, 43210).is_err());
    for prefix in ["POST", "HEAD"] {
        let request = String::from_utf8(valid.clone())
            .unwrap()
            .replacen("GET", prefix, 1);
        assert!(helper::parse_callback_request(request.as_bytes(), 43210).is_err());
    }
    let old_http = String::from_utf8(valid)
        .unwrap()
        .replace("HTTP/1.1", "HTTP/1.0");
    assert!(helper::parse_callback_request(old_http.as_bytes(), 43210).is_err());
    let oversized = callback(
        target,
        &format!("Host: 127.0.0.1:43210\r\nX-Large: {}", "x".repeat(16_384)),
    );
    assert!(helper::parse_callback_request(&oversized, 43210).is_err());
}

#[test]
fn oidc_configuration_accepts_only_the_exact_issuer_and_trusted_jwks_route() {
    let document = |issuer: &str, uri: &str| {
        serde_json::to_vec(&json!({"issuer":issuer,"jwks_uri":uri})).unwrap()
    };
    assert!(helper::jwks_url(&document(
        "https://auth.openai.com",
        "https://auth.openai.com/.well-known/jwks.json"
    ))
    .is_ok());
    assert!(helper::jwks_url(&document(
        "https://auth.openai.com/",
        "https://auth.openai.com/jwks"
    ))
    .is_err());
    for uri in [
        "http://auth.openai.com/jwks",
        "https://untrusted.invalid/jwks",
        "https://auth.openai.com.untrusted.invalid/jwks",
        "https://auth.openai.com:444/jwks",
        "https://user:password@auth.openai.com/jwks",
        "https://@auth.openai.com/jwks",
        "https://auth.openai.com/jwks?token=synthetic",
        "https://auth.openai.com/jwks#fragment",
        "https://auth.openai.com/jwks?",
    ] {
        assert!(helper::jwks_url(&document("https://auth.openai.com", uri)).is_err());
    }
    assert!(helper::jwks_url(&vec![b'x'; 262_145]).is_err());
}

fn token_request(endpoint: &'static str) -> TokenRequest {
    TokenRequest {
        endpoint,
        form: vec![
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), "synthetic".into()),
        ],
        timeout: Duration::from_secs(30),
        max_response_bytes: 262_144,
    }
}

#[tokio::test]
async fn configured_transport_rejects_untrusted_endpoints_before_http() {
    let transport = helper::HttpsTransport::new().unwrap();
    // Only build requests for external-looking URLs; no external request is sent.
    for endpoint in [
        "https://untrusted.invalid/oauth/token",
        "https://auth.openai.com/api/accounts/oauth/token?extra=synthetic",
        "https://user@auth.openai.com/api/accounts/oauth/token",
        "https://auth.openai.com/api/accounts/oauth/token#fragment",
    ] {
        assert!(matches!(
            transport.build_token_request(token_request(endpoint)),
            Err(AuthError::Protocol)
        ));
    }
    // This exercised send path must reject before HTTP. Even a regression has
    // only a synthetic loopback destination, never an external auth endpoint.
    assert!(matches!(
        transport
            .send(token_request("http://127.0.0.1:9/token"))
            .await,
        Err(AuthError::Protocol)
    ));
    let request = transport
        .build_token_request(token_request(
            "https://auth.openai.com/api/accounts/oauth/token",
        ))
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(
        request.url().as_str(),
        "https://auth.openai.com/api/accounts/oauth/token"
    );
    assert_eq!(request.headers()["accept-encoding"], "identity");
    assert_eq!(
        request.headers()["content-type"],
        "application/x-www-form-urlencoded"
    );
    let mut oversized = token_request("https://auth.openai.com/api/accounts/oauth/token");
    oversized.max_response_bytes += 1;
    assert!(matches!(
        transport.build_token_request(oversized),
        Err(AuthError::Protocol)
    ));
    let mut slow = token_request("https://auth.openai.com/api/accounts/oauth/token");
    slow.timeout = Duration::from_secs(31);
    assert!(matches!(
        transport.build_token_request(slow),
        Err(AuthError::Protocol)
    ));
}

#[test]
fn exec_is_unavailable_before_vault_config_or_child_access() {
    let directory = private_directory();
    let vault_parent = directory.path().join("missing-vault-parent");
    let vault = vault_parent.join(CANARY);
    let config_parent = directory.path().join("missing-config-parent");
    let config = config_parent.join(CANARY);
    let home = directory.path().join("unused-home");
    let executable = directory.path().join("marker-child");
    let marker = directory.path().join("child-ran");
    // This executable would create the marker if the helper launched it.
    fs::write(&executable, b"#!/bin/sh\n: > \"$1\"\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_personas-auth"));
        command
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .env("PERSONAS_SIWC_ACCESS_TOKEN", CANARY)
            .env("PERSONAS_SIWC_ACCOUNT_BINDING", CANARY)
            .env("OPENAI_API_KEY", CANARY);
        command
    };
    let assert_unavailable = |output: Output| {
        redacted_failure(&output);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap(),
            json!({
                "status": "error",
                "error": "exec is unavailable: native execution-owner-bound exact saved-configuration handoff is not implemented",
                "automatic_retry": false
            })
        );
        assert!(!marker.exists());
        assert!(!vault_parent.exists());
        assert!(!config_parent.exists());
        assert!(!home.exists());
    };

    assert_unavailable(command().arg("exec").output().unwrap());
    // No real vault or valid grant is supplied. All cases must stop before
    // credential access, authentication, network activity, or child execution.
    for selected_vault in [None, Some(vault.as_path()), Some(Path::new(CANARY))] {
        for allow_handoff in [false, true] {
            let mut process = command();
            if let Some(path) = selected_vault {
                process.arg("--vault").arg(path);
            }
            process.arg("exec");
            if allow_handoff {
                process.arg("--allow-handoff");
            }
            let output = process
                .arg("--provider-config")
                .arg(&config)
                .arg("--runtime-bin")
                .arg(&executable)
                .arg("--")
                .arg(&marker)
                .arg(CANARY)
                .output()
                .unwrap();
            assert_unavailable(output);
        }
    }
}

#[test]
fn migration_commands_have_offline_help_and_require_explicit_state_and_operator() {
    let directory = private_directory();
    let state = directory.path().join("absent-state");
    for name in ["provider-migration-snapshot", "migrate-provider"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ai-personas"))
            .args([name, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("orphan recovery")
                || String::from_utf8_lossy(&output.stdout).contains("recover ordinary orphans")
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ai-personas"))
        .arg("provider-migration-snapshot")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--state-root"));
    let output = Command::new(env!("CARGO_BIN_EXE_ai-personas"))
        .arg("--state-root")
        .arg(&state)
        .arg("provider-migration-snapshot")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--operator"));
    let request = directory.path().join("invalid-request.json");
    fs::write(&request, b"not JSON").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai-personas"))
        .arg("--state-root")
        .arg(&state)
        .args(["--operator", "fixture-operator", "migrate-provider"])
        .arg(&request)
        .arg("--preview")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!state.exists());
}
