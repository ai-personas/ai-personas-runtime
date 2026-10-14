//! Explicit host-side SIWC access actions and fixed in-process native launch.
//!
//! Native-run retains the selected Store execution owner and vault lock through
//! the bounded run. Capability evidence digests are operator pins, not proof of
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
    ffi::{CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    future::Future,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    pin::Pin,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

type Result<T> = std::result::Result<T, AuthError>;
const MAX_BODY: usize = 262_144;
const MAX_CALLBACK: usize = 16_384;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const ISSUER: &str = "https://auth.openai.com";
const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/oauth/token";
const EXEC_UNAVAILABLE: &str = "exec is unavailable: native execution-owner-bound exact saved-configuration handoff is not implemented";
const MIGRATION_NOTICE: &str = "A separate explicit ai-personas migrate-provider is required when saved expiry, model, or account bindings differ. Refresh does not update saved runtime configuration.";

#[derive(Parser)]
#[command(
    name = "personas-auth",
    about = "Explicit host-side Personas SIWC sign-in, refresh, and status",
    after_help = "Use an explicit absolute external --vault path. No credential discovery occurs. Open the authorization URL in a browser on the same host as this helper. Authentication establishes a grant, not model capability; no inference is performed by sign-in, refresh, or status. Capability digests are structural checks, not proof of actual capability. Native-run requires --authorize, an explicit absolute --state-root, --operator, and exactly one --need or --work. It performs only the fixed in-process execution-owner-bound exact saved-configuration handoff. Exec is unavailable; --allow-handoff cannot enable it. After refresh, use a separate explicit ai-personas migrate-provider when saved expiry, model, or account bindings differ. Native-run adds no grant, refresh, probe, migration, fallback or general resume."
)]
pub(crate) struct Cli {
    /// Selected external credential file; absolute, protected, and without symlinks.
    #[arg(long, global = true)]
    vault: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read selected-vault account metadata only. No authentication or network.
    Status,
    /// Explicitly authorize one new browser sign-in; never opens a browser automatically.
    SignIn {
        #[arg(long, required = true)]
        authorize: bool,
        /// Bind only 127.0.0.1; zero selects an available port.
        #[arg(long, default_value_t = 0)]
        callback_port: u16,
    },
    /// Explicitly renew the selected grant; saved runtime configuration is unchanged.
    Refresh {
        #[arg(long, required = true)]
        authorize: bool,
    },
    /// Authorize one fixed in-process run using an already configured Store.
    NativeRun {
        #[arg(long, required = true)]
        authorize: bool,
        #[arg(long)]
        state_root: PathBuf,
        #[arg(long)]
        operator: String,
        #[arg(long, required_unless_present = "work", conflicts_with = "work")]
        need: Option<String>,
        #[arg(long, required_unless_present = "need")]
        work: Option<String>,
    },
    /// Unavailable: generic execution-owner-bound exact saved-configuration child handoff.
    Exec {
        /// Ignored compatibility flag; does not enable exec.
        #[arg(long)]
        allow_handoff: bool,
        /// Ignored compatibility path; no configuration is opened.
        #[arg(long)]
        provider_config: Option<PathBuf>,
        /// Ignored compatibility path; no executable is opened.
        #[arg(long)]
        runtime_bin: Option<PathBuf>,
        /// Ignored compatibility arguments; no child is started.
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
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
        AuthError::Vault => {
            "explicit absolute external vault with protected ownership and no symlinks is required"
        }
        AuthError::Busy => "selected vault is busy",
        AuthError::TooEarly => "credential renewal is too early",
        AuthError::CommitUncertain => "credential commit outcome is unknown; do not retry",
        AuthError::Entropy => "secure randomness is unavailable",
        AuthError::State => "authorization state validation failed",
        AuthError::Denied => "operation denied or canceled",
    }
}

pub(crate) async fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Exec {
            allow_handoff: _allow_handoff,
            provider_config: _provider_config,
            runtime_bin: _runtime_bin,
            args: _args,
        } => {
            // Compatibility inputs are ignored. Reject before resolving any
            // vault path, acquiring a lock, reading files, or starting a child.
            eprintln!(
                "{}",
                json!({
                    "status": "error",
                    "error": EXEC_UNAVAILABLE,
                    "automatic_retry": false
                })
            );
            return Ok(1);
        }
        Command::NativeRun {
            authorize,
            state_root,
            operator,
            need,
            work,
        } => {
            if !authorize {
                return Err(AuthError::Denied);
            }
            if !absolute_path(&state_root) {
                return Err(AuthError::State);
            }
            let input = match (need.as_deref(), work.as_deref()) {
                (Some(need), None) => ai_personas::launch::NativeInput::Need(need),
                (None, Some(work)) => ai_personas::launch::NativeInput::Work(work),
                _ => return Err(AuthError::State),
            };
            let vault = cli.vault.ok_or(AuthError::Vault)?;
            prepare_vault_path(&vault, false)?;
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
            prepare_vault_path(&vault, false)?;
            // A protected registration can exist without a credential file.
            // Read both under the selected lock through the bounded vault APIs.
            // Status neither creates a host identity nor constructs HTTP adapters.
            let guard = open_locked(&vault).await?;
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
            authorize,
            callback_port,
        } => {
            if !authorize {
                return Err(AuthError::Denied);
            }
            let vault = cli.vault.ok_or(AuthError::Vault)?;
            cancellable(sign_in(&vault, callback_port)).await?;
        }
        Command::Refresh { authorize } => {
            if !authorize {
                return Err(AuthError::Denied);
            }
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

async fn open_locked(path: &Path) -> Result<LockedVault> {
    let path = path.to_owned();
    let task = tokio::task::spawn_blocking(move || {
        let vault = Vault::open(&path)?;
        vault.lock()
    });
    tokio::time::timeout(CONNECTION_TIMEOUT, task)
        .await
        .map_err(|_| AuthError::Timeout)?
        .map_err(|_| AuthError::Vault)?
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
    prepare_vault_path(path, true)?;
    let guard = open_locked(path).await?;
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
    prepare_vault_path(path, false)?;
    let mut guard = open_locked(path).await?;
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

fn absolute_path(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    path.is_absolute()
        && bytes.len() <= 4096
        && !bytes.contains(&0)
        && !bytes.ends_with(b"/")
        && !bytes
            .split(|b| *b == b'/')
            .any(|part| part == b"." || part == b"..")
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
}

fn trusted_directory(directory: &File) -> Result<()> {
    let metadata = directory.metadata().map_err(|_| AuthError::Vault)?;
    // SAFETY: geteuid has no arguments or memory preconditions.
    let uid = unsafe { libc::geteuid() };
    let writable = metadata.mode() & 0o022 != 0;
    let root_sticky = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
    if !metadata.is_dir()
        || (metadata.uid() != uid && metadata.uid() != 0)
        || (writable && !root_sticky)
    {
        return Err(AuthError::Vault);
    }
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: directory owns its live fd; the C string and output storage are valid.
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            c".git".as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound {
        return Err(AuthError::Vault);
    }
    Ok(())
}

fn open_directory_at(parent: &File, name: &OsStr, create: bool) -> Result<File> {
    let name = CString::new(name.as_bytes()).map_err(|_| AuthError::Vault)?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: parent owns its fd and name is a valid, single-component C string.
    let mut fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 && create && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        // Only the final protected parent may be created, under an already
        // checked directory descriptor. Existing directories are never chmodded.
        // SAFETY: the descriptor and C string remain valid.
        let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(AuthError::Vault);
        }
        // SAFETY: as for the preceding openat; O_NOFOLLOW rejects a racing symlink.
        fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    }
    if fd < 0 {
        return Err(AuthError::Vault);
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Validate without discovering any credentials. Only authorized sign-in may
/// create the final missing parent. Vault subsequently repeats its own protected
/// path and no-follow checks for all credential and shared host-identity I/O.
pub(crate) fn prepare_vault_path(path: &Path, create: bool) -> Result<bool> {
    if !absolute_path(path) || path.starts_with(env!("CARGO_MANIFEST_DIR")) {
        return Err(AuthError::Vault);
    }
    if let Ok(source) = fs::canonicalize(env!("CARGO_MANIFEST_DIR")) {
        if path.starts_with(source) {
            return Err(AuthError::Vault);
        }
    }
    let parent = path.parent().ok_or(AuthError::Vault)?;
    let components: Vec<_> = parent
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| AuthError::Vault)?;
    trusted_directory(&directory)?;
    for (index, name) in components.iter().enumerate() {
        directory = open_directory_at(&directory, name, create && index + 1 == components.len())?;
        trusted_directory(&directory)?;
    }
    let metadata = directory.metadata().map_err(|_| AuthError::Vault)?;
    // SAFETY: geteuid has no arguments or memory preconditions.
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7777 != 0o700 {
        return Err(AuthError::Vault);
    }
    let name = CString::new(path.file_name().ok_or(AuthError::Vault)?.as_bytes())
        .map_err(|_| AuthError::Vault)?;
    // SAFETY: directory owns its live fd and name is a valid C string.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(AuthError::Vault)
        };
    }
    // SAFETY: openat returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|_| AuthError::Vault)?;
    // SAFETY: geteuid has no arguments or memory preconditions.
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(AuthError::Vault);
    }
    Ok(true)
}

pub(crate) struct HttpsTransport {
    client: reqwest::Client,
}

impl HttpsTransport {
    pub(crate) fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .pool_max_idle_per_host(0)
            .connection_verbose(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(HTTP_TIMEOUT)
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|_| AuthError::Transport)?;
        Ok(Self { client })
    }

    pub(crate) fn build_token_request(&self, request: TokenRequest) -> Result<reqwest::Request> {
        if request.endpoint != TOKEN_ENDPOINT
            || request.timeout.is_zero()
            || request.timeout > HTTP_TIMEOUT
            || request.max_response_bytes == 0
            || request.max_response_bytes > MAX_BODY
            || request.form.len() > 32
            || request
                .form
                .iter()
                .any(|(key, value)| key.len() > 128 || value.len() > 32_768)
            || request
                .form
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>()
                > 65_536
        {
            return Err(AuthError::Protocol);
        }
        self.client
            .post(TOKEN_ENDPOINT)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .timeout(request.timeout)
            .form(&request.form)
            .build()
            .map_err(|_| AuthError::Transport)
    }

    async fn fetch(
        &self,
        request: reqwest::Request,
        timeout: Duration,
        maximum: usize,
    ) -> Result<TokenResponse> {
        tokio::time::timeout(timeout, async {
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
            if response
                .content_length()
                .is_some_and(|length| length > maximum as u64)
            {
                return Err(AuthError::Protocol);
            }
            let status = response.status().as_u16();
            let mut stream = response.bytes_stream();
            let mut body = Vec::new();
            loop {
                tokio::task::yield_now().await;
                match stream.next().await {
                    Some(chunk) => {
                        let chunk = chunk.map_err(http_error)?;
                        if chunk.len() > maximum.saturating_sub(body.len()) {
                            return Err(AuthError::Protocol);
                        }
                        body.extend_from_slice(&chunk);
                    }
                    None => return Ok(TokenResponse { status, body }),
                }
            }
        })
        .await
        .map_err(|_| AuthError::Timeout)?
    }

    async fn get_document(&self, url: Url) -> Result<Vec<u8>> {
        let request = self
            .client
            .get(url)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|_| AuthError::Transport)?;
        let response = self.fetch(request, HTTP_TIMEOUT, MAX_BODY).await?;
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
            let timeout = request.timeout;
            let maximum = request.max_response_bytes;
            // Endpoint rejection precedes request execution or DNS resolution.
            let request = self.build_token_request(request)?;
            self.fetch(request, timeout, maximum).await
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
    if configuration.len() > MAX_BODY {
        return Err(AuthError::Protocol);
    }
    let document: Configuration =
        serde_json::from_slice(configuration).map_err(|_| AuthError::Protocol)?;
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
    if port == 0 || bytes.len() > MAX_CALLBACK || !bytes.is_ascii() || !bytes.ends_with(b"\r\n\r\n")
    {
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
        let remaining = MAX_CALLBACK.saturating_sub(bytes.len());
        if remaining == 0 {
            return Err(AuthError::Protocol);
        }
        let limit = remaining.min(buffer.len());
        let count = stream
            .read(&mut buffer[..limit])
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
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    })
    .await;
}

async fn receive_callback(listener: TcpListener, port: u16) -> Result<String> {
    tokio::time::timeout(CALLBACK_TIMEOUT, async {
        for _ in 0..32 {
            let (mut stream, peer) = listener.accept().await.map_err(|_| AuthError::Transport)?;
            if peer.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
                continue;
            }
            let parsed = tokio::time::timeout(CONNECTION_TIMEOUT, read_callback(&mut stream, port))
                .await
                .unwrap_or(Err(AuthError::Timeout));
            callback_response(&mut stream, parsed.is_ok()).await;
            if let Ok(callback) = parsed {
                return Ok(callback);
            }
        }
        Err(AuthError::Protocol)
    })
    .await
    .map_err(|_| AuthError::Timeout)?
}
