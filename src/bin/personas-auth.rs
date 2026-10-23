//! Explicit host-side SIWC access actions and fixed in-process native launch.
//!
//! Native-run retains the selected Store execution owner and vault lock through
//! the selected run. Capability evidence digests are operator pins, not proof of
//! actual model capability. Generic Exec remains disabled.
//! After renewal, use an explicit migrate-provider when saved expiry, model,
//! or account bindings differ. The runtime independently checks its saved pins.

use ai_personas::auth::{
    AuthError, AuthFlow, Clock, CredentialRecord, Entropy, IdTokenVerifier, LockedVault, OsEntropy,
    PendingAuthorization, PendingRegistration, SystemClock, TokenRequest, TokenResponse,
    TokenTransport, Vault,
};
use clap::{error::ErrorKind, Parser, Subcommand};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

type Result<T> = std::result::Result<T, AuthError>;
const ISSUER: &str = "https://auth.openai.com";
const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/oauth/token";
const MIGRATION_NOTICE: &str = "A separate explicit ai-personas migrate-provider is required when saved expiry, model, or account bindings differ. Refresh does not update saved runtime configuration.";

#[derive(Parser)]
#[command(
    name = "personas-auth",
    about = "Explicit host-side Personas SIWC sign-in, refresh, and status",
    after_help = "Select an explicit --vault path. Relative paths and existing aliases resolve to the same account location and lock. No credential discovery occurs. Open the authorization URL in a browser on the same host as this helper. Authentication establishes a grant, not model capability; no inference is performed by sign-in, refresh, or status. Capability digests are structural checks, not proof of actual capability. Native-run requires an explicit --state-root, --operator, and exactly one --need, --work, or --delegation. Optional --intake-delegation selects an exact grant only with --work and creates no opportunity. It performs only the fixed in-process execution-owner-bound exact saved-configuration handoff. After refresh, use a separate explicit ai-personas migrate-provider when saved expiry, model, or account bindings differ. Native-run adds no grant, refresh, probe, migration, fallback or general resume."
)]
pub(crate) struct Cli {
    /// Selected credential file; relative paths and existing aliases are accepted.
    #[arg(long, global = true)]
    vault: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read selected-vault account metadata only. No authentication or network.
    Status,
    /// Start a selected browser sign-in. No inference.
    SignIn {
        /// Historical optional metadata; the selected command starts the action.
        #[arg(long, hide = true)]
        authorize: bool,
        /// Bind only 127.0.0.1; zero selects an available port.
        #[arg(long, default_value_t = 0)]
        callback_port: u16,
    },
    /// Explicitly renew the selected grant; saved runtime configuration is unchanged.
    Refresh {
        /// Historical optional metadata; the selected command starts the action.
        #[arg(long, hide = true)]
        authorize: bool,
    },
    /// Start one in-process run using an already configured Store.
    NativeRun {
        /// Historical optional metadata; the selected command starts the action.
        #[arg(long, hide = true)]
        authorize: bool,
        #[arg(long)]
        state_root: PathBuf,
        #[arg(long)]
        operator: String,
        #[arg(long, required_unless_present_any = ["work", "delegation"], conflicts_with_all = ["work", "delegation"])]
        need: Option<String>,
        #[arg(long, required_unless_present_any = ["need", "delegation"], conflicts_with_all = ["need", "delegation"])]
        work: Option<String>,
        #[arg(long, required_unless_present_any = ["need", "work"], conflicts_with_all = ["need", "work"])]
        delegation: Option<String>,
        /// Select an exact admitted child when --delegation has several children.
        #[arg(long, requires = "delegation")]
        child_work: Option<String>,
        /// Exact grant reference for proposal authoring on --work.
        /// Retains the selected vault lock and direct in-process transport.
        #[arg(long, requires = "work", conflicts_with_all = ["need", "delegation"])]
        intake_delegation: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return;
            }
            // Clap diagnostics can echo supplied argument values. Keep failures static.
            eprintln!("{{\"status\":\"error\",\"error\":\"invalid arguments; see --help\",\"automatic_retry\":false}}");
            std::process::exit(2);
        }
    };
    match run(cli).await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!(
                "{}",
                json!({
                    "status": "error",
                    "error": error_message(&error),
                    "code": error_code(&error),
                    "automatic_retry": false
                })
            );
            // Also terminates any blocking lock acquisition abandoned on cancellation.
            std::process::exit(1);
        }
    }
}

fn error_message(error: &AuthError) -> &'static str {
    match error {
        AuthError::Transport => "authorization transport failed",
        AuthError::Timeout => "operation deadline exceeded",
        AuthError::Protocol => "invalid or oversized authorization input",
        AuthError::Identity => "identity verification failed",
        AuthError::Scope => "required grant scope is unavailable",
        AuthError::Expired => "credential expired; explicit renewal is required",
        AuthError::Binding => "selected credential or configuration binding mismatch",
        AuthError::Vault => "selected vault or its transaction files are unavailable or invalid",
        AuthError::Busy => "selected vault is busy",
        AuthError::TooEarly => "credential renewal is too early",
        AuthError::CommitUncertain => "credential commit outcome is unknown; do not retry",
        AuthError::Entropy => "secure randomness is unavailable",
        AuthError::State => "authorization state validation failed",
        AuthError::Denied => "operation denied or canceled",
    }
}

fn error_code(error: &AuthError) -> &'static str {
    match error {
        AuthError::Vault => "native_vault_location_unsupported",
        AuthError::Busy => "vault_busy",
        AuthError::Expired => "credential_expired",
        AuthError::Binding => "credential_binding_mismatch",
        AuthError::Scope => "grant_scope_unavailable",
        AuthError::Denied => "authorization_denied",
        AuthError::CommitUncertain => "credential_commit_uncertain",
        AuthError::Transport => "authorization_transport_failed",
        AuthError::Timeout => "operation_timeout",
        AuthError::Protocol => "invalid_authorization_protocol",
        AuthError::Identity => "identity_verification_failed",
        AuthError::TooEarly => "credential_renewal_too_early",
        AuthError::Entropy => "secure_randomness_unavailable",
        AuthError::State => "authorization_state_invalid",
    }
}

pub(crate) async fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::NativeRun {
            authorize: _,
            state_root,
            operator,
            need,
            work,
            delegation,
            child_work,
            intake_delegation,
        } => {
            // Parse and validate before opening the selected vault or Runtime.
            let selection = intake_delegation
                .as_ref()
                .map(|file| {
                    let reference: ai_personas::protocol::need::GrantReference =
                        ai_personas::runtime::read_json_file(file, 4096)
                            .map_err(|_| AuthError::Protocol)?;
                    reference.validate().map_err(|_| AuthError::Protocol)?;
                    Ok::<_, AuthError>(reference)
                })
                .transpose()?;
            let input = match (
                need.as_deref(),
                work.as_deref(),
                delegation.as_deref(),
                selection.as_ref(),
            ) {
                (Some(need), None, None, None) => ai_personas::launch::NativeInput::Need(need),
                (None, Some(work), None, None) => ai_personas::launch::NativeInput::Work(work),
                (None, Some(work), None, Some(selection)) => {
                    ai_personas::launch::NativeInput::WorkWithIntake { work, selection }
                }
                (None, None, Some(grant), None) => {
                    match child_work.as_deref() {
                        Some(work) => ai_personas::launch::NativeInput::DelegatedWork { grant, work },
                        None => ai_personas::launch::NativeInput::Delegation(grant),
                    }
                }
                _ => return Err(AuthError::State),
            };
            let vault = cli.vault.ok_or(AuthError::Vault)?;
            match ai_personas::launch::native_run(&state_root, &operator, &vault, input).await {
                Ok(output) => emit(serde_json::to_value(output).map_err(|_| AuthError::Protocol)?)?,
                Err(_) => {
                    eprintln!(
                        "{}",
                        json!({
                            "status":"error",
                            "error":"native launch admission or execution failed",
                            "automatic_retry":false
                        })
                    );
                    return Ok(1);
                }
            }
        }
        Command::Status => {
            let vault = cli.vault.ok_or(AuthError::Vault)?;
            // A protected registration can exist without a credential file.
            // Read both completely under the selected vault lock.
            // Status neither creates a host identity nor constructs HTTP adapters.
            let guard = open_locked(&vault, false).await?;
            let record = guard.load()?;
            let registration_present = guard.pending_registration()?.is_some();
            match record {
                Some(record) => emit_record(&record)?,
                None => emit(json!({
                    "status": if registration_present {
                        "registration_pending"
                    } else {
                        "grant_unavailable"
                    },
                    "grant_connected": false,
                    "grant_record_present": false,
                    "registration_present": registration_present,
                    "account_verified": false,
                    "capability_verified": false,
                    "inference_performed": false
                }))?,
            }
        }
        Command::SignIn {
            authorize: _,
            callback_port,
        } => {
            let vault = cli.vault.ok_or(AuthError::Vault)?;
            cancellable(sign_in(&vault, callback_port)).await?;
        }
        Command::Refresh { authorize: _ } => {
            let vault = cli.vault.ok_or(AuthError::Vault)?;
            cancellable(refresh(&vault)).await?;
        }
    }
    Ok(0)
}

async fn cancellable<T>(operation: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => Err(AuthError::Denied),
        result = operation => result,
    }
}

async fn open_locked(path: &Path, create_parent: bool) -> Result<LockedVault> {
    let path = path.to_owned();
    let task = tokio::task::spawn_blocking(move || {
        let vault = if create_parent {
            Vault::open_for_sign_in(&path)?
        } else {
            Vault::open(&path)?
        };
        vault.lock()
    });
    task.await.map_err(|_| AuthError::Vault)?
}

pub(crate) struct SignInAttempt {
    host_id: String,
    pending: PendingAuthorization,
}

impl SignInAttempt {
    pub(crate) fn begin(
        guard: &LockedVault,
        redirect_uri: String,
        entropy: &mut dyn Entropy,
        clock: &dyn Clock,
    ) -> Result<Self> {
        let previous = guard.load()?;
        let registration = guard.pending_registration()?;
        // Persist or reuse the shared protected host identity before displaying
        // a URL, including when only a credential-free registration survives.
        let host_id = guard.host_id(entropy)?;
        if let Some(record) = previous.as_ref() {
            check_host(record, &host_id)?;
        }
        let flow = AuthFlow::new(host_id.clone(), redirect_uri)?;
        let pending = match (previous.as_ref(), registration.as_ref()) {
            // A receipt must never replace an existing account's identity pin.
            (Some(record), _) => flow.begin(entropy, clock, Some(record))?,
            (None, Some(registration)) => flow.begin_registered(entropy, clock, registration)?,
            (None, None) => flow.begin(entropy, clock, None)?,
        };
        Ok(Self { host_id, pending })
    }

    pub(crate) fn authorization_url(&self) -> &str {
        self.pending.authorization_url()
    }

    pub(crate) async fn complete(
        self,
        guard: &LockedVault,
        callback: &str,
        transport: &dyn TokenTransport,
        verifier: &IdTokenVerifier,
        clock: &dyn Clock,
    ) -> Result<CredentialRecord> {
        self.complete_with_retention(
            guard,
            callback,
            transport,
            verifier,
            clock,
            |registration| guard.retain_pending_registration(registration),
        )
        .await
    }

    // Production always enters through complete with the locked vault writer.
    // This shared ordering seam also permits offline storage-failure injection.
    pub(crate) async fn complete_with_retention(
        self,
        guard: &LockedVault,
        callback: &str,
        transport: &dyn TokenTransport,
        verifier: &IdTokenVerifier,
        clock: &dyn Clock,
        retain: impl FnOnce(&PendingRegistration) -> Result<()>,
    ) -> Result<CredentialRecord> {
        let registration = self
            .pending
            .validate_callback_registration(callback, clock)?;
        // This receipt contains no authorization secrets. Any retention error,
        // including uncertain durability, stops before constructing an exchange.
        retain(&registration)?;
        let record = self
            .pending
            .exchange(callback, transport, verifier, clock)
            .await?;
        check_host(&record, &self.host_id)?;
        if record.expires_at_unix() <= clock.now_unix() {
            return Err(AuthError::Expired);
        }
        // Give cancellation priority before the synchronous atomic commit.
        // The vault cleans the matching receipt only after durable credentials.
        tokio::task::yield_now().await;
        guard.save(&record)?;
        Ok(record)
    }
}

async fn sign_in(path: &Path, port: u16) -> Result<()> {
    let guard = open_locked(path, true).await?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| AuthError::Transport)?;
    let port = listener
        .local_addr()
        .map_err(|_| AuthError::Transport)?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/auth/callback");
    let mut entropy = OsEntropy;
    let clock = SystemClock;
    let attempt = SignInAttempt::begin(&guard, redirect_uri, &mut entropy, &clock)?;
    let transport = HttpsTransport::new()?;
    let verifier = transport.verifier().await?;
    // Only this library-generated authorization URL is displayed. Callback
    // queries, token requests, credentials, and transport errors are never logged.
    eprintln!("Open this new authorization URL in a browser on this same host:");
    eprintln!("{}", attempt.authorization_url());
    let callback = receive_callback(listener, port).await?;
    // receive_callback owns and closes the listener before this single exchange.
    // The selected vault lock remains held through receipt and credential commits.
    let record = attempt
        .complete(&guard, &callback, &transport, &verifier, &clock)
        .await?;
    emit_record(&record)
}

async fn refresh(path: &Path) -> Result<()> {
    let mut guard = open_locked(path, false).await?;
    let previous = guard.load()?.ok_or(AuthError::Vault)?;
    let host_id = guard.host_id(&mut OsEntropy)?;
    check_host(&previous, &host_id)?;
    let transport = HttpsTransport::new()?;
    let verifier = transport.verifier().await?;
    // The selected account lock remains held during validation, transport, and
    // the library's atomic credential replacement. No runtime state is opened.
    let record = guard.refresh(&transport, &verifier, &SystemClock).await?;
    emit_record(&record)
}

fn check_host(record: &CredentialRecord, host_id: &str) -> Result<()> {
    if record.metadata().ext_agent_host_id != host_id {
        return Err(AuthError::Binding);
    }
    Ok(())
}

fn account_digest(account: &str) -> String {
    hex::encode(Sha256::digest(account.as_bytes()))
}

fn emit_record(record: &CredentialRecord) -> Result<()> {
    emit(record_status(record, &SystemClock))
}

pub(crate) fn record_status(record: &CredentialRecord, clock: &dyn Clock) -> serde_json::Value {
    let metadata = record.metadata();
    // Handoff checks refresh fences and stale handles as well as expiry.
    let current = record.handoff(clock).is_ok();
    // Explicit allowlist: never serialize CredentialRecord or RuntimeHandoff.
    json!({
        "status": if current { "grant_connected" } else { "grant_unavailable" },
        "grant_connected": current,
        "grant_record_present": true,
        "capability_verified": false,
        "inference_performed": false,
        "expires_at_unix": record.expires_at_unix(),
        "account_sha256": account_digest(&record.account_binding()),
        "metadata": {
            "issuer": metadata.issuer,
            "subject": metadata.subject,
            "client_id": metadata.client_id,
            "ext_agent_host_id": metadata.ext_agent_host_id,
            "expires_at_unix": metadata.expires_at_unix,
            "scopes": metadata.scopes
        },
        "runtime_configuration_updated": false,
        "migration_notice": MIGRATION_NOTICE
    })
}

fn emit(value: serde_json::Value) -> Result<()> {
    use std::io::Write;
    let encoded = serde_json::to_vec(&value).map_err(|_| AuthError::Protocol)?;
    let mut output = std::io::stdout().lock();
    output
        .write_all(&encoded)
        .map_err(|_| AuthError::Protocol)?;
    output.write_all(b"\n").map_err(|_| AuthError::Protocol)?;
    output.flush().map_err(|_| AuthError::Protocol)
}

pub(crate) struct HttpsTransport {
    client: reqwest::Client,
}

impl HttpsTransport {
    pub(crate) fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connection_verbose(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .build()
            .map_err(|_| AuthError::Transport)?;
        Ok(Self { client })
    }

    pub(crate) fn build_token_request(&self, request: TokenRequest) -> Result<reqwest::Request> {
        if request.endpoint != TOKEN_ENDPOINT {
            return Err(AuthError::Protocol);
        }
        self.client
            .post(TOKEN_ENDPOINT)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .form(&request.form)
            .build()
            .map_err(|_| AuthError::Transport)
    }

    async fn fetch(&self, request: reqwest::Request) -> Result<TokenResponse> {
        let response = self.client.execute(request).await.map_err(http_error)?;
        {
            let values = response
                .headers()
                .get_all(reqwest::header::CONTENT_ENCODING);
            let mut encodings = values.iter();
            if encodings
                .next()
                .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
                || encodings.next().is_some()
            {
                return Err(AuthError::Protocol);
            }
        }
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        loop {
            tokio::task::yield_now().await;
            match stream.next().await {
                Some(chunk) => {
                    let chunk = chunk.map_err(http_error)?;
                    body.extend_from_slice(&chunk);
                }
                None => return Ok(TokenResponse { status, body }),
            }
        }
    }

    async fn get_document(&self, url: Url) -> Result<Vec<u8>> {
        let request = self
            .client
            .get(url)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .build()
            .map_err(|_| AuthError::Transport)?;
        let response = self.fetch(request).await?;
        if response.status != 200 {
            return Err(AuthError::Protocol);
        }
        Ok(response.body)
    }

    async fn verifier(&self) -> Result<IdTokenVerifier> {
        let discovery = Url::parse(DISCOVERY).map_err(|_| AuthError::Protocol)?;
        let configuration = self.get_document(discovery).await?;
        let jwks = self.get_document(jwks_url(&configuration)?).await?;
        IdTokenVerifier::from_jwks(&jwks)
    }
}

impl TokenTransport for HttpsTransport {
    fn send<'a>(
        &'a self,
        request: TokenRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TokenResponse>> + Send + 'a>> {
        Box::pin(async move {
            // Endpoint rejection precedes request execution or DNS resolution.
            let request = self.build_token_request(request)?;
            self.fetch(request).await
        })
    }
}

fn http_error(error: reqwest::Error) -> AuthError {
    if error.is_timeout() {
        AuthError::Timeout
    } else {
        AuthError::Transport
    }
}

pub(crate) fn jwks_url(configuration: &[u8]) -> Result<Url> {
    #[derive(Deserialize)]
    struct Configuration {
        issuer: String,
        jwks_uri: String,
    }
    let value = ai_personas::files::parse_json_unique(configuration)
        .map_err(|_| AuthError::Protocol)?;
    let document: Configuration = serde_json::from_value(value).map_err(|_| AuthError::Protocol)?;
    if document.issuer != ISSUER {
        return Err(AuthError::Identity);
    }
    let authority = document
        .jwks_uri
        .strip_prefix("https://")
        .ok_or(AuthError::Protocol)?
        .split(['/', '?', '#'])
        .next()
        .ok_or(AuthError::Protocol)?;
    if authority.contains('@') || document.jwks_uri.contains('\\') {
        return Err(AuthError::Protocol);
    }
    let url = Url::parse(&document.jwks_uri).map_err(|_| AuthError::Protocol)?;
    if url.scheme() != "https"
        || url.host_str() != Some("auth.openai.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthError::Protocol);
    }
    Ok(url)
}

fn valid_percent_encoding(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

pub(crate) fn parse_callback_request(bytes: &[u8], port: u16) -> Result<String> {
    if port == 0 || !bytes.is_ascii() || !bytes.ends_with(b"\r\n\r\n") {
        return Err(AuthError::Protocol);
    }
    let end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(AuthError::Protocol)?;
    if end + 4 != bytes.len() {
        return Err(AuthError::Protocol);
    }
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| AuthError::Protocol)?;
    let mut lines = text.split("\r\n");
    let mut request = lines.next().ok_or(AuthError::Protocol)?.split(' ');
    if request.next() != Some("GET") {
        return Err(AuthError::Protocol);
    }
    let target = request.next().ok_or(AuthError::Protocol)?;
    if request.next() != Some("HTTP/1.1")
        || request.next().is_some()
        || target.bytes().any(|byte| !byte.is_ascii_graphic())
        || target.contains('#')
    {
        return Err(AuthError::Protocol);
    }
    let (path, query) = target.split_once('?').ok_or(AuthError::Protocol)?;
    if path != "/auth/callback" || query.is_empty() || !valid_percent_encoding(query.as_bytes()) {
        return Err(AuthError::Protocol);
    }
    let mut names = BTreeSet::new();
    let mut host = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(AuthError::Protocol)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || value
                .bytes()
                .any(|byte| !(byte == b'\t' || (b' '..=b'~').contains(&byte)))
        {
            return Err(AuthError::Protocol);
        }
        let name = name.to_ascii_lowercase();
        if !names.insert(name.clone())
            || matches!(
                name.as_str(),
                "content-length" | "transfer-encoding" | "expect"
            )
        {
            return Err(AuthError::Protocol);
        }
        if name == "host" {
            host = Some(value.trim_matches([' ', '\t']));
        }
    }
    let expected_host = format!("127.0.0.1:{port}");
    if host != Some(expected_host.as_str()) {
        return Err(AuthError::Protocol);
    }
    let exact_url = format!("http://{expected_host}{target}");
    let parsed = Url::parse(&exact_url).map_err(|_| AuthError::Protocol)?;
    let mut query_names = BTreeSet::new();
    for (key, value) in parsed.query_pairs() {
        if key.is_empty()
            || key.chars().chain(value.chars()).any(char::is_control)
            || !query_names.insert(key.into_owned())
        {
            return Err(AuthError::Protocol);
        }
    }
    // Keep the exact supplied query. The library verifies state, code, client,
    // and all remaining callback semantics before any token transport call.
    Ok(exact_url)
}

async fn read_callback(stream: &mut TcpStream, port: u16) -> Result<String> {
    let mut bytes = Vec::with_capacity(1024);
    let mut buffer = [0u8; 1024];
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|_| AuthError::Protocol)?;
        if count == 0 {
            return Err(AuthError::Protocol);
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return parse_callback_request(&bytes, port);
        }
    }
}

async fn callback_response(stream: &mut TcpStream, accepted: bool) {
    let (status, body) = if accepted {
        (
            "200 OK",
            "Callback received. Return to the Personas helper.\n",
        )
    } else {
        (
            "400 Bad Request",
            "Callback rejected. Return to the Personas helper.\n",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if stream.write_all(response.as_bytes()).await.is_ok() {
        let _ = stream.shutdown().await;
    }
}

async fn receive_callback(listener: TcpListener, port: u16) -> Result<String> {
    // Independent connections remain serviceable while an earlier browser
    // connection is idle. Dropping the set cancels those reads on completion
    // or explicit cancellation, without imposing a duration or attempt quota.
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            incoming = listener.accept() => {
                let (mut stream, peer) = incoming.map_err(|_| AuthError::Transport)?;
                if peer.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
                    continue;
                }
                connections.spawn(async move {
                    let parsed = read_callback(&mut stream, port).await;
                    callback_response(&mut stream, parsed.is_ok()).await;
                    parsed
                });
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Ok(Ok(callback))) = completed {
                    return Ok(callback);
                }
            }
        }
    }
}
