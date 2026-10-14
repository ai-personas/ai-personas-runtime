//! Direct Responses transport. Persona orchestration remains in Runtime.
//!
//! Credential sources and transport implementations are trusted host interfaces.
//! Neither is an interface for accepting model-authored transport receipts.
//! There is no credential discovery, refresh, subprocess, retry, or fallback.

mod sse;
#[cfg(test)]
mod tests;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::watch;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const INSTRUCTIONS: &str = "Return only the JSON value required by text.format.";
const USER_AGENT: &str = "ai-personas/direct-responses-v1";
const PUBLIC_HEADERS: &[(&str, &str)] = &[
    ("content-type", "application/json"),
    ("accept", "text/event-stream"),
    ("accept-encoding", "identity"),
    ("user-agent", USER_AGENT),
];
const CHATGPT_HEADERS: &[(&str, &str)] = &[
    ("content-type", "application/json"),
    ("accept", "text/event-stream"),
    ("accept-encoding", "identity"),
    ("user-agent", USER_AGENT),
    ("originator", "ai-personas"),
    ("openai-beta", "responses=experimental"),
];

/// Immutable wire contract. Change its versioned ID whenever any field changes.
/// Credential values are excluded; their header placement is explicit.
#[derive(Serialize)]
struct WireProfile {
    id: &'static str,
    method: &'static str,
    url: &'static str,
    headers: &'static [(&'static str, &'static str)],
    authorization_header: &'static str,
    authorization_scheme: &'static str,
    account_header: Option<&'static str>,
    input_role: &'static str,
    instructions: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    #[serde(rename = "chatgpt_oauth")]
    ChatgptOAuth,
    #[serde(rename = "siwc_oauth")]
    SiwcOAuth,
    PlatformApiKey,
}
impl AccessMode {
    pub fn endpoint(self) -> &'static str {
        self.wire_profile().url
    }
    pub fn profile(self) -> &'static str {
        self.wire_profile().id
    }
    fn wire_profile(self) -> WireProfile {
        let (id, url, headers, account_header) = match self {
            Self::ChatgptOAuth => (
                "traditional_chatgpt_responses_v2",
                "https://chatgpt.com/backend-api/codex/responses",
                CHATGPT_HEADERS,
                Some("chatgpt-account-id"),
            ),
            Self::SiwcOAuth => (
                "siwc_documented_public_responses_v2",
                "https://api.openai.com/v1/responses",
                PUBLIC_HEADERS,
                None,
            ),
            Self::PlatformApiKey => (
                "platform_responses_v2",
                "https://api.openai.com/v1/responses",
                PUBLIC_HEADERS,
                None,
            ),
        };
        WireProfile {
            id,
            method: "POST",
            url,
            headers,
            authorization_header: "authorization",
            authorization_scheme: "Bearer",
            account_header,
            input_role: "user",
            instructions: INSTRUCTIONS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthGrant {
    TraditionalChatgpt,
    SiwcTokenSharing,
}

/// A reference, never a secret value or an implicit credential search path.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretReference {
    pub env: String,
}
impl SecretReference {
    fn valid(&self) -> bool {
        !self.env.is_empty()
            && self.env.len() <= 128
            && self
                .env
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_uppercase() || (i > 0 && b.is_ascii_digit()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialReference {
    OAuth {
        grant: OAuthGrant,
        token_env: SecretReference,
        account_env: SecretReference,
        expires_at_unix: u64,
    },
    ApiKey {
        key_env: SecretReference,
        account_env: SecretReference,
    },
}

/// The operator must retain evidence for this exact route, account, model and
/// effort. Digests name that evidence; they do not authenticate it themselves.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEvidence {
    pub mode: AccessMode,
    pub model: String,
    pub efforts: BTreeSet<String>,
    pub account_env: SecretReference,
    /// Lowercase SHA-256 of the exact account bytes captured with this evidence.
    /// This configuration pin must never be refreshed from a mutable source.
    pub account_sha256: String,
    /// Exact immutable wire contract under which this capability was evidenced.
    pub transport_profile: String,
    pub catalog_evidence_sha256: String,
    pub structured_output_evidence_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub timeout_ms: u64,
    pub max_prompt_bytes: usize,
    pub max_schema_bytes: usize,
    pub max_schema_nodes: usize,
    pub max_schema_depth: usize,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_stream_bytes: usize,
    pub max_event_bytes: usize,
    pub max_events: usize,
}
impl Limits {
    fn validate(&self) -> Result<(), Failure> {
        let bytes = [
            self.max_prompt_bytes,
            self.max_schema_bytes,
            self.max_request_bytes,
            self.max_response_bytes,
            self.max_stream_bytes,
            self.max_event_bytes,
        ];
        if bytes.into_iter().any(|n| n == 0 || n > MAX_BYTES)
            || !(1..=86_400_000).contains(&self.timeout_ms)
            || !(1..=65_536).contains(&self.max_schema_nodes)
            || !(1..=64).contains(&self.max_schema_depth)
            || !(1..=100_000).contains(&self.max_events)
            || self.max_event_bytes > self.max_stream_bytes
            || self.max_response_bytes > self.max_event_bytes
        {
            return Err(Failure::Configuration);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectSettings {
    pub mode: AccessMode,
    pub model: String,
    pub effort: String,
    pub credentials: CredentialReference,
    pub evidence: CapabilityEvidence,
    pub limits: Limits,
}
impl<'de> Deserialize<'de> for DirectSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Document {
            mode: AccessMode,
            model: String,
            effort: String,
            credentials: CredentialReference,
            evidence: CapabilityEvidence,
            limits: Limits,
        }
        let value = Value::deserialize(deserializer)?;
        let document: Document = serde_json::from_value(value).map_err(|_| {
            serde::de::Error::custom(
                "direct provider settings required: replace legacy CLI executable, pin, temporary-directory and feature settings with explicit mode, credential references, account/model/schema evidence and direct limits; migrate saved launch metadata explicitly without resetting Store state",
            )
        })?;
        Ok(Self {
            mode: document.mode,
            model: document.model,
            effort: document.effort,
            credentials: document.credentials,
            evidence: document.evidence,
            limits: document.limits,
        })
    }
}
impl DirectSettings {
    pub fn validate(&self) -> Result<(), ProviderError> {
        self.validate_inner().map_err(ProviderError::Rejected)
    }
    fn validate_inner(&self) -> Result<(), Failure> {
        self.limits.validate()?;
        if !identifier(&self.model) || !identifier(&self.effort) {
            return Err(Failure::Configuration);
        }
        if self.evidence.mode != self.mode
            || self.evidence.model != self.model
            || self.evidence.transport_profile != self.mode.profile()
        {
            return Err(Failure::ModelUnavailable);
        }
        if !self.evidence.efforts.contains(&self.effort) {
            return Err(Failure::EffortUnsupported);
        }
        if self.evidence.efforts.len() > 64
            || self.evidence.efforts.iter().any(|s| !identifier(s))
            || !digest_string(&self.evidence.catalog_evidence_sha256)
            || !digest_string(&self.evidence.account_sha256)
            || !self.evidence.account_env.valid()
        {
            return Err(Failure::Configuration);
        }
        if !self
            .evidence
            .structured_output_evidence_sha256
            .as_deref()
            .is_some_and(digest_string)
        {
            return Err(Failure::SchemaCapabilityMissing);
        }
        let (secret, account, grant) = match &self.credentials {
            CredentialReference::OAuth {
                grant,
                token_env,
                account_env,
                ..
            } => (token_env, account_env, Some(*grant)),
            CredentialReference::ApiKey {
                key_env,
                account_env,
            } => (key_env, account_env, None),
        };
        if !secret.valid() || !account.valid() {
            return Err(Failure::Configuration);
        }
        if !grant_matches(self.mode, grant) {
            return Err(Failure::GrantMismatch);
        }
        Ok(())
    }
    /// Builds a client without reading credentials or making a request.
    pub fn connect(&self) -> Result<DirectProvider, ProviderError> {
        self.validate()?;
        DirectProvider::new(
            self.clone(),
            Arc::new(EnvironmentCredentialSource),
            Arc::new(ReqwestTransport::new()?),
            Arc::new(SystemClock),
        )
    }
}

/// Deliberately not Serialize. Debug never displays the value.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);
impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}
#[derive(Clone, Debug)]
pub enum Credential {
    OAuth {
        grant: OAuthGrant,
        bearer: Secret,
        account: Secret,
        expires_at_unix: u64,
    },
    ApiKey {
        key: Secret,
        account: Secret,
    },
}
#[derive(Clone, Debug)]
pub struct CredentialBundle {
    pub credential: Credential,
    pub evidence_account: Secret,
}

/// Trusted host source. Grant provenance and account binding must originate in
/// the application's access action, never a response, CLI file or guessed JWT.
pub trait CredentialSource: Send + Sync {
    fn load(
        &self,
        reference: &CredentialReference,
        evidence_account: &SecretReference,
    ) -> Result<CredentialBundle, Failure>;
}
pub struct EnvironmentCredentialSource;
impl CredentialSource for EnvironmentCredentialSource {
    fn load(
        &self,
        reference: &CredentialReference,
        evidence_account: &SecretReference,
    ) -> Result<CredentialBundle, Failure> {
        fn read(reference: &SecretReference) -> Result<Secret, Failure> {
            if !reference.valid() {
                return Err(Failure::Configuration);
            }
            std::env::var(&reference.env)
                .map(Secret::new)
                .map_err(|_| Failure::CredentialMissing)
        }
        let credential = match reference {
            CredentialReference::OAuth {
                grant,
                token_env,
                account_env,
                expires_at_unix,
            } => Credential::OAuth {
                grant: *grant,
                bearer: read(token_env)?,
                account: read(account_env)?,
                expires_at_unix: *expires_at_unix,
            },
            CredentialReference::ApiKey {
                key_env,
                account_env,
            } => Credential::ApiKey {
                key: read(key_env)?,
                account: read(account_env)?,
            },
        };
        Ok(CredentialBundle {
            credential,
            evidence_account: read(evidence_account)?,
        })
    }
}
pub trait Clock: Send + Sync {
    fn unix_seconds(&self) -> Result<u64, Failure>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn unix_seconds(&self) -> Result<u64, Failure> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|_| Failure::Configuration)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    #[error("invalid direct provider configuration")]
    Configuration,
    #[error("exact route/account/model eligibility evidence is absent")]
    ModelUnavailable,
    #[error("exact requested effort is not evidenced")]
    EffortUnsupported,
    #[error("strict structured-output capability is not established")]
    SchemaCapabilityMissing,
    #[error("explicit credential source is unavailable")]
    CredentialMissing,
    #[error("credential is invalid")]
    InvalidCredential,
    #[error("OAuth credential expired; a separate access renewal is required")]
    CredentialExpired,
    #[error("credential grant does not match the selected mode")]
    GrantMismatch,
    #[error("credential account does not match the capability evidence")]
    AccountMismatch,
    #[error("local cancellation; remote stop is not confirmed")]
    Cancelled,
    #[error("local transport deadline exceeded")]
    Timeout,
    #[error("HTTPS transport failed")]
    Network,
    #[error("HTTP response was not successful")]
    HttpStatus,
    #[error("HTTP response was not an unencoded SSE stream")]
    ContentType,
    #[error("local resource limit exceeded")]
    Limit,
    #[error("Responses stream contract was not satisfied")]
    Protocol,
    #[error("provider reported failure, refusal or unsupported output")]
    RemoteFailure,
    #[error("reported model differs from the exact requested model")]
    ReportedModelMismatch,
    #[error("reported reasoning effort conflicts with the request or an earlier snapshot")]
    ReportedEffortMismatch,
    #[error("prepared request/result binding mismatch")]
    Binding,
}
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("direct provider rejected before dispatch: {0}")]
    Rejected(Failure),
    #[error("direct provider attempt failed: {failure}")]
    Attempt {
        failure: Failure,
        receipt: Box<AttemptReceipt>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub actual_cost_microusd: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RequestAudit {
    pub invocation_id: String,
    pub prompt_sha256: String,
    pub prompt_bytes: u64,
    pub schema_sha256: String,
    pub schema_bytes: u64,
    pub instructions_sha256: String,
    pub request_sha256: String,
    pub request_bytes: u64,
    pub configuration_sha256: String,
    pub transport_profile: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct AttemptReceipt {
    pub audit: RequestAudit,
    pub dispatch_may_have_begun: bool,
    pub http_status: Option<u16>,
    pub stream_bytes_observed: u64,
    pub stream_eof: bool,
    pub event_count: u64,
    pub response_completed: bool,
    /// Last explicit provider report, not independently verified served identity.
    /// An omitted/null snapshot supplies no new report.
    pub reported_effort: Option<String>,
    /// The prior report if two snapshots contradict. Both values are bounded.
    pub previous_reported_effort: Option<String>,
    pub usage: Usage,
    pub elapsed_ms: u64,
    pub remote_stop_confirmed: bool,
    pub remote_effects_and_charges_may_remain: bool,
}
impl AttemptReceipt {
    fn new(audit: RequestAudit) -> Self {
        Self {
            audit,
            dispatch_may_have_begun: false,
            http_status: None,
            stream_bytes_observed: 0,
            stream_eof: false,
            event_count: 0,
            response_completed: false,
            reported_effort: None,
            previous_reported_effort: None,
            usage: Usage::default(),
            elapsed_ms: 0,
            remote_stop_confirmed: false,
            remote_effects_and_charges_may_remain: false,
        }
    }
}
/// Local disposition of a validated complete response after stream EOF.
/// Cancelled still carries terminal evidence: persist it before rejecting local
/// adoption. Neither variant confirms remote stop or measured charges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionDisposition {
    Ready,
    Cancelled,
}
pub struct ProviderOutput {
    pub disposition: CompletionDisposition,
    pub response: String,
    pub usage: Usage,
    pub requested_model: String,
    pub reported_model: Option<String>,
    pub requested_effort: String,
    pub audit: RequestAudit,
    pub attempt: AttemptReceipt,
}
impl fmt::Debug for ProviderOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderOutput")
            .field("response_bytes", &self.response.len())
            .field("usage", &self.usage)
            .finish_non_exhaustive()
    }
}
impl ProviderOutput {
    pub fn bound_to(
        self,
        audit: &RequestAudit,
        intent: &ProviderIntent,
    ) -> Result<Self, ProviderError> {
        if self.audit != *audit
            || self.attempt.audit != *audit
            || self.requested_model != intent.model
            || self.reported_model.as_deref() != Some(intent.model.as_str())
            || self.requested_effort != intent.effort
            || self
                .attempt
                .reported_effort
                .as_deref()
                .is_some_and(|effort| !identifier(effort) || effort != intent.effort.as_str())
            || self.attempt.previous_reported_effort.is_some()
            || self.response.len() > intent.max_response_bytes
            || !self.attempt.response_completed
            || !self.attempt.stream_eof
            || self.usage != self.attempt.usage
        {
            // Retain bounded observations even when the host result fails its
            // binding check. The failure belongs to the expected admission.
            let mut receipt = self.attempt;
            receipt.audit = audit.clone();
            receipt.reported_effort = receipt.reported_effort.filter(|value| identifier(value));
            receipt.previous_reported_effort = receipt
                .previous_reported_effort
                .filter(|value| identifier(value));
            receipt.dispatch_may_have_begun = true;
            receipt.remote_effects_and_charges_may_remain = true;
            return Err(attempt_error(Failure::Binding, receipt));
        }
        Ok(self)
    }
}

pub struct ProviderRequest {
    pub prompt: String,
    pub output_schema: Value,
    pub invocation_id: String,
}
#[derive(Clone, Debug)]
pub struct ProviderIntent {
    pub model: String,
    pub effort: String,
    pub max_prompt_bytes: usize,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_stream_bytes: usize,
}
/// Implementations are trusted host components, not public result-ingress APIs.
pub trait ModelProvider: Send + Sync {
    fn intent(&self) -> ProviderIntent;
    fn prepare(&self, request: ProviderRequest) -> Result<Box<dyn PreparedCall>, ProviderError>;
}
pub trait PreparedCall: Send {
    fn audit(&self) -> &RequestAudit;
    fn cancel(self: Box<Self>) -> Result<(), ProviderError>;
    fn dispatch(
        self: Box<Self>,
        cancellation: Cancellation,
    ) -> BoxFuture<'static, Result<ProviderOutput, ProviderError>>;
}

#[derive(Clone, Debug)]
pub struct Cancellation {
    sender: watch::Sender<bool>,
}
impl Default for Cancellation {
    fn default() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }
}
impl Cancellation {
    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.sender.borrow()
    }
    async fn cancelled(&self) {
        let mut receiver = self.sender.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

/// No URL field: routing is fixed by the explicit mode.
pub struct HttpRequest {
    pub mode: AccessMode,
    pub body: Vec<u8>,
    pub bearer: Secret,
    pub account_header: Option<Secret>,
}
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("mode", &self.mode)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}
pub struct HttpResponse {
    pub status: u16,
    pub is_unencoded_sse: bool,
    pub body: Box<dyn HttpBody>,
}
/// The returned slice must be at most maximum bytes. Empty chunks are permitted;
/// None alone establishes EOF. Implementations must stop local work when dropped.
pub trait HttpBody: Send {
    fn read(&mut self, maximum: usize) -> BoxFuture<'_, Result<Option<Vec<u8>>, Failure>>;
}
pub trait HttpTransport: Send + Sync {
    fn post(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Failure>>;
}

pub struct ReqwestTransport {
    client: reqwest::Client,
}
impl ReqwestTransport {
    pub fn new() -> Result<Self, ProviderError> {
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
            .build()
            .map_err(|_| ProviderError::Rejected(Failure::Configuration))?;
        Ok(Self { client })
    }
    /// Builds the actual wire request without sending or resolving credentials.
    pub fn build_request(&self, request: HttpRequest) -> Result<reqwest::Request, Failure> {
        use reqwest::header::HeaderValue;
        let profile = request.mode.wire_profile();
        if !secret_valid(&request.bearer, 16_384) {
            return Err(Failure::InvalidCredential);
        }
        let mut authorization = HeaderValue::from_str(&format!(
            "{} {}",
            profile.authorization_scheme,
            request.bearer.expose(),
        ))
        .map_err(|_| Failure::InvalidCredential)?;
        authorization.set_sensitive(true);
        let method = reqwest::Method::from_bytes(profile.method.as_bytes())
            .map_err(|_| Failure::Configuration)?;
        let mut builder = self
            .client
            .request(method, profile.url)
            .header(profile.authorization_header, authorization);
        for &(name, value) in profile.headers {
            builder = builder.header(name, value);
        }
        match (profile.account_header, request.account_header.as_ref()) {
            (Some(name), Some(account)) => {
                if !secret_valid(account, 512) {
                    return Err(Failure::InvalidCredential);
                }
                let mut value = HeaderValue::from_str(account.expose())
                    .map_err(|_| Failure::InvalidCredential)?;
                value.set_sensitive(true);
                builder = builder.header(name, value);
            }
            (Some(_), None) => return Err(Failure::InvalidCredential),
            (None, Some(_)) => return Err(Failure::GrantMismatch),
            (None, None) => (),
        }
        builder
            .body(request.body)
            .build()
            .map_err(|_| Failure::Configuration)
    }
}
impl HttpTransport for ReqwestTransport {
    fn post(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
        Box::pin(async move {
            let request = self.build_request(request)?;
            let response = self
                .client
                .execute(request)
                .await
                .map_err(|_| Failure::Network)?;
            let status = response.status().as_u16();
            Ok(HttpResponse {
                status,
                is_unencoded_sse: unencoded_sse(response.headers()),
                body: Box::new(ReqwestBody {
                    response,
                    pending: bytes::Bytes::new(),
                }),
            })
        })
    }
}
fn unencoded_sse(headers: &reqwest::header::HeaderMap) -> bool {
    use reqwest::header::{CONTENT_ENCODING, CONTENT_TYPE};
    let content_types = headers.get_all(CONTENT_TYPE);
    let mut types = content_types.iter();
    let sse = types.next().and_then(|v| v.to_str().ok()).is_some_and(|v| {
        v.split(';')
            .next()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
    }) && types.next().is_none();
    let content_encodings = headers.get_all(CONTENT_ENCODING);
    let mut encodings = content_encodings.iter();
    let identity = encodings
        .next()
        .is_none_or(|v| v.as_bytes().eq_ignore_ascii_case(b"identity"))
        && encodings.next().is_none();
    sse && identity
}
struct ReqwestBody {
    response: reqwest::Response,
    pending: bytes::Bytes,
}
impl HttpBody for ReqwestBody {
    fn read(&mut self, maximum: usize) -> BoxFuture<'_, Result<Option<Vec<u8>>, Failure>> {
        Box::pin(async move {
            if maximum == 0 {
                return Err(Failure::Limit);
            }
            // Return empty chunks to the caller so its cancellation/deadline can run.
            if self.pending.is_empty() {
                match self.response.chunk().await.map_err(|_| Failure::Network)? {
                    Some(bytes) => self.pending = bytes,
                    None => return Ok(None),
                }
            }
            let count = maximum.min(self.pending.len());
            Ok(Some(self.pending.split_to(count).to_vec()))
        })
    }
}

pub struct DirectProvider {
    inner: Arc<Inner>,
}
struct Inner {
    settings: DirectSettings,
    source: Arc<dyn CredentialSource>,
    transport: Arc<dyn HttpTransport>,
    clock: Arc<dyn Clock>,
    configuration_sha256: String,
}
impl DirectProvider {
    pub fn new(
        settings: DirectSettings,
        source: Arc<dyn CredentialSource>,
        transport: Arc<dyn HttpTransport>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ProviderError> {
        settings.validate()?;
        let configuration_sha256 = digest(
            &encoded(
                &json!({
                    "settings": &settings,
                    "transport_profile": settings.mode.wire_profile(),
                }),
                32_768,
            )
            .map_err(ProviderError::Rejected)?,
        );
        Ok(Self {
            inner: Arc::new(Inner {
                settings,
                source,
                transport,
                clock,
                configuration_sha256,
            }),
        })
    }
}
impl Inner {
    fn credentials(&self, prepared_account: Option<&Secret>) -> Result<(Secret, Secret), Failure> {
        let settings = &self.settings;
        let bundle = self
            .source
            .load(&settings.credentials, &settings.evidence.account_env)?;
        let (bearer, account, grant) = match bundle.credential {
            Credential::OAuth {
                grant,
                bearer,
                account,
                expires_at_unix,
            } => {
                if self.clock.unix_seconds()? >= expires_at_unix {
                    return Err(Failure::CredentialExpired);
                }
                (bearer, account, Some(grant))
            }
            Credential::ApiKey { key, account } => (key, account, None),
        };
        if !grant_matches(settings.mode, grant) {
            return Err(Failure::GrantMismatch);
        }
        if !secret_valid(&bearer, 16_384)
            || !secret_valid(&account, 512)
            || !secret_valid(&bundle.evidence_account, 512)
        {
            return Err(Failure::InvalidCredential);
        }
        if account != bundle.evidence_account
            || digest(account.expose().as_bytes()) != settings.evidence.account_sha256
            || prepared_account.is_some_and(|old| old != &account)
        {
            return Err(Failure::AccountMismatch);
        }
        Ok((bearer, account))
    }
}
impl ModelProvider for DirectProvider {
    fn intent(&self) -> ProviderIntent {
        let s = &self.inner.settings;
        ProviderIntent {
            model: s.model.clone(),
            effort: s.effort.clone(),
            max_prompt_bytes: s.limits.max_prompt_bytes,
            max_request_bytes: s.limits.max_request_bytes,
            max_response_bytes: s.limits.max_response_bytes,
            max_stream_bytes: s.limits.max_stream_bytes,
        }
    }
    fn prepare(&self, request: ProviderRequest) -> Result<Box<dyn PreparedCall>, ProviderError> {
        let prepare = || -> Result<DirectCall, Failure> {
            let settings = &self.inner.settings;
            let limits = &settings.limits;
            if request.prompt.trim().is_empty() || !identifier(&request.invocation_id) {
                return Err(Failure::Configuration);
            }
            if request.prompt.len() > limits.max_prompt_bytes {
                return Err(Failure::Limit);
            }
            let schema = schema_bytes(&request.output_schema, limits)?;
            let (_, account) = self.inner.credentials(None)?;
            let profile = settings.mode.wire_profile();
            let body = encoded(
                &json!({
                    "model": settings.model,
                    "store": false,
                    "stream": true,
                    "instructions": profile.instructions,
                    "input": [{"role":profile.input_role,"content":[{"type":"input_text","text":request.prompt}]}],
                    "reasoning": {"effort":settings.effort},
                    "text": {"format":{"type":"json_schema","name":"persona_decision",
                        "strict":true,"schema":request.output_schema}}
                }),
                limits.max_request_bytes,
            )?;
            let audit = RequestAudit {
                invocation_id: request.invocation_id.clone(),
                prompt_sha256: digest(request.prompt.as_bytes()),
                prompt_bytes: request.prompt.len() as u64,
                schema_sha256: digest(&schema),
                schema_bytes: schema.len() as u64,
                instructions_sha256: digest(profile.instructions.as_bytes()),
                request_sha256: digest(&body),
                request_bytes: body.len() as u64,
                configuration_sha256: self.inner.configuration_sha256.clone(),
                transport_profile: profile.id.into(),
            };
            Ok(DirectCall {
                inner: self.inner.clone(),
                body,
                audit,
                account,
            })
        };
        prepare()
            .map(|call| Box::new(call) as Box<dyn PreparedCall>)
            .map_err(ProviderError::Rejected)
    }
}
struct DirectCall {
    inner: Arc<Inner>,
    body: Vec<u8>,
    audit: RequestAudit,
    account: Secret,
}
impl PreparedCall for DirectCall {
    fn audit(&self) -> &RequestAudit {
        &self.audit
    }
    fn cancel(self: Box<Self>) -> Result<(), ProviderError> {
        Ok(())
    }
    fn dispatch(
        self: Box<Self>,
        cancellation: Cancellation,
    ) -> BoxFuture<'static, Result<ProviderOutput, ProviderError>> {
        Box::pin(async move {
            let DirectCall {
                inner,
                body,
                audit,
                account,
            } = *self;
            let mut receipt = AttemptReceipt::new(audit.clone());
            if cancellation.is_cancelled() {
                return Err(attempt_error(Failure::Cancelled, receipt));
            }
            // Recheck expiry, grant, the immutable evidence digest and the
            // preparation's account fence immediately before the HTTP operation.
            let (bearer, account) = inner
                .credentials(Some(&account))
                .map_err(|failure| attempt_error(failure, receipt.clone()))?;
            let settings = &inner.settings;
            let mut collector = sse::Collector::new(&settings.model, &settings.limits);
            let started = tokio::time::Instant::now();
            let result = {
                let running = async {
                    if cancellation.is_cancelled() {
                        return Err(Failure::Cancelled);
                    }
                    receipt.dispatch_may_have_begun = true;
                    let response = inner
                        .transport
                        .post(HttpRequest {
                            mode: settings.mode,
                            body,
                            bearer,
                            account_header: (settings.mode == AccessMode::ChatgptOAuth)
                                .then_some(account),
                        })
                        .await?;
                    receipt.http_status = Some(response.status);
                    if response.status != 200 {
                        return Err(Failure::HttpStatus);
                    }
                    if !response.is_unencoded_sse {
                        return Err(Failure::ContentType);
                    }
                    let mut body = response.body;
                    loop {
                        // Even an always-ready body must allow cancellation and
                        // the deadline to run, including on empty chunks.
                        tokio::task::yield_now().await;
                        let remaining = settings
                            .limits
                            .max_stream_bytes
                            .saturating_sub(collector.observed);
                        // Exact exhaustion also rejects: EOF has not been observed.
                        if remaining == 0 {
                            return Err(Failure::Limit);
                        }
                        let maximum = remaining.min(8192);
                        match body.read(maximum).await? {
                            Some(bytes) => {
                                if bytes.len() > maximum {
                                    return Err(Failure::Limit);
                                }
                                collector.push(&bytes)?;
                            }
                            None => {
                                receipt.stream_eof = true;
                                return collector.finish();
                            }
                        }
                    }
                };
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(Failure::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(settings.limits.timeout_ms)) => Err(Failure::Timeout),
                    result = running => result,
                }
            };
            receipt.stream_bytes_observed = collector.observed as u64;
            receipt.event_count = collector.events as u64;
            receipt.response_completed = collector.completed;
            receipt.reported_effort = collector.reported_effort.clone();
            receipt.previous_reported_effort = collector.previous_reported_effort.clone();
            receipt.usage = collector.usage.clone();
            receipt.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            receipt.remote_effects_and_charges_may_remain = receipt.dispatch_may_have_begun;
            let response = result.map_err(|failure| attempt_error(failure, receipt.clone()))?;
            if collector
                .reported_effort
                .as_deref()
                .is_some_and(|effort| effort != settings.effort.as_str())
            {
                return Err(attempt_error(Failure::ReportedEffortMismatch, receipt));
            }
            // Only successful finish at EOF reaches this handoff. Cancellation
            // must prevent adoption without discarding the complete evidence.
            let disposition = if cancellation.is_cancelled() {
                CompletionDisposition::Cancelled
            } else {
                CompletionDisposition::Ready
            };
            Ok(ProviderOutput {
                disposition,
                response,
                usage: collector.usage,
                requested_model: settings.model.clone(),
                reported_model: Some(settings.model.clone()),
                requested_effort: settings.effort.clone(),
                audit,
                attempt: receipt,
            })
        })
    }
}
fn attempt_error(failure: Failure, receipt: AttemptReceipt) -> ProviderError {
    ProviderError::Attempt {
        failure,
        receipt: Box::new(receipt),
    }
}
fn grant_matches(mode: AccessMode, grant: Option<OAuthGrant>) -> bool {
    matches!(
        (mode, grant),
        (
            AccessMode::ChatgptOAuth,
            Some(OAuthGrant::TraditionalChatgpt)
        ) | (AccessMode::SiwcOAuth, Some(OAuthGrant::SiwcTokenSharing))
            | (AccessMode::PlatformApiKey, None)
    )
}
fn secret_valid(secret: &Secret, maximum: usize) -> bool {
    !secret.0.is_empty()
        && secret.0.len() <= maximum
        && secret.0.bytes().all(|b| b.is_ascii_graphic())
}
fn identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.chars().any(|c| c.is_control() || c.is_whitespace())
}
fn digest_string(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn encoded<T: Serialize + ?Sized>(value: &T, maximum: usize) -> Result<Vec<u8>, Failure> {
    struct Buffer {
        bytes: Vec<u8>,
        maximum: usize,
    }
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("serialization bound"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut buffer, value).map_err(|_| Failure::Limit)?;
    Ok(buffer.bytes)
}
fn schema_bytes(value: &Value, limits: &Limits) -> Result<Vec<u8>, Failure> {
    if !value.is_object() {
        return Err(Failure::Configuration);
    }
    let mut pending = vec![(value, 0usize)];
    let mut count = 0usize;
    while let Some((value, depth)) = pending.pop() {
        count += 1;
        if count > limits.max_schema_nodes || depth > limits.max_schema_depth {
            return Err(Failure::Limit);
        }
        let children: Box<dyn Iterator<Item = &Value>> = match value {
            Value::Object(v) => Box::new(v.values()),
            Value::Array(v) => Box::new(v.iter()),
            _ => Box::new(std::iter::empty()),
        };
        for child in children {
            if pending.len() >= limits.max_schema_nodes.saturating_sub(count) {
                return Err(Failure::Limit);
            }
            pending.push((child, depth + 1));
        }
    }
    encoded(value, limits.max_schema_bytes)
}
