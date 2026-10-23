//! Direct Responses transport. Persona orchestration remains in Runtime.
//!
//! Credential sources and transport implementations are trusted host interfaces.
//! Neither is an interface for accepting model-authored transport receipts.
//! There is no credential discovery, refresh, subprocess, retry, or fallback.

#[cfg(unix)]
pub(crate) mod onboarding;
mod sse;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::watch;

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretReference {
    pub env: String,
}
impl SecretReference {
    fn valid(&self) -> bool {
        !self.env.is_empty()
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
    /// Explicit source identity for an ephemeral, auth-only handoff. The
    /// ordinary environment source rejects this variant; it never falls back.
    EphemeralAuthOnly {
        grant: OAuthGrant,
        /// Informational expiry snapshot only; the source returns the current token expiry.
        /// Runtime validity is determined from the actual handoff result and clock.
        expires_at_unix: u64,
    },
}

/// Account binding source, without inventing an environment name for ephemeral
/// credentials. The Environment variant serializes exactly as the historical
/// `{"env":"..."}` object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AccountBindingReference {
    Environment(SecretReference),
    EphemeralAuthOnly { ephemeral_auth_only: bool },
}
impl AccountBindingReference {
    pub(crate) fn environment(&self) -> Option<&SecretReference> {
        match self {
            Self::Environment(reference) => Some(reference),
            Self::EphemeralAuthOnly { .. } => None,
        }
    }

    fn validate(&self) -> Result<(), Failure> {
        match self {
            Self::Environment(reference) if reference.valid() => Ok(()),
            Self::EphemeralAuthOnly {
                ephemeral_auth_only: true,
            } => Ok(()),
            _ => Err(Failure::Configuration),
        }
    }
}

/// The selected route, account and model, with optional retained capability
/// observations. Missing observations do not gate inference. Digests name
/// historical evidence; they do not authenticate it or promise future support.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEvidence {
    pub mode: AccessMode,
    pub model: String,
    #[serde(default)]
    pub efforts: BTreeSet<String>,
    pub account_env: AccountBindingReference,
    /// Lowercase SHA-256 of the exact account bytes captured with this evidence.
    /// This configuration pin must never be refreshed from a mutable source.
    pub account_sha256: String,
    /// Exact immutable wire contract selected for this configuration.
    pub transport_profile: String,
    #[serde(default)]
    pub catalog_evidence_sha256: Option<String>,
    #[serde(default)]
    pub structured_output_evidence_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectSettings {
    pub mode: AccessMode,
    pub model: String,
    pub effort: String,
    pub credentials: CredentialReference,
    pub evidence: CapabilityEvidence,
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
        }
        let value = Value::deserialize(deserializer)?;
        let document: Document = serde_json::from_value(value).map_err(|_| {
            serde::de::Error::custom(
                "direct provider settings require an explicit mode, model, effort, credential references and account/route binding; catalogue and capability observations are optional",
            )
        })?;
        Ok(Self {
            mode: document.mode,
            model: document.model,
            effort: document.effort,
            credentials: document.credentials,
            evidence: document.evidence,
        })
    }
}
impl DirectSettings {
    pub fn validate(&self) -> Result<(), ProviderError> {
        self.validate_inner().map_err(ProviderError::Rejected)
    }
    fn validate_inner(&self) -> Result<(), Failure> {
        if !identifier(&self.model) || !identifier(&self.effort) {
            return Err(Failure::Configuration);
        }
        if self.evidence.mode != self.mode
            || self.evidence.model != self.model
            || self.evidence.transport_profile != self.mode.profile()
        {
            return Err(Failure::ModelUnavailable);
        }
        if self.evidence.efforts.iter().any(|s| !identifier(s))
            || self
                .evidence
                .catalog_evidence_sha256
                .as_deref()
                .is_some_and(|s| !digest_string(s))
            || self
                .evidence
                .structured_output_evidence_sha256
                .as_deref()
                .is_some_and(|s| !digest_string(s))
            || !digest_string(&self.evidence.account_sha256)
        {
            return Err(Failure::Configuration);
        }
        self.evidence.account_env.validate()?;
        let grant = match &self.credentials {
            CredentialReference::OAuth {
                grant,
                token_env,
                account_env,
                ..
            } => {
                if !token_env.valid()
                    || !account_env.valid()
                    || self.evidence.account_env.environment().is_none()
                {
                    return Err(Failure::Configuration);
                }
                Some(*grant)
            }
            CredentialReference::ApiKey {
                key_env,
                account_env,
            } => {
                if !key_env.valid()
                    || !account_env.valid()
                    || self.evidence.account_env.environment().is_none()
                {
                    return Err(Failure::Configuration);
                }
                None
            }
            CredentialReference::EphemeralAuthOnly {
                grant,
                expires_at_unix,
            } => {
                if self.mode != AccessMode::ChatgptOAuth || *grant != OAuthGrant::TraditionalChatgpt
                {
                    return Err(Failure::GrantMismatch);
                }
                if *expires_at_unix == 0
                    || self.evidence.account_env
                        != (AccountBindingReference::EphemeralAuthOnly {
                            ephemeral_auth_only: true,
                        })
                {
                    return Err(Failure::Configuration);
                }
                Some(*grant)
            }
        };
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

    /// Ephemeral auth-only sources may override this. The expected account
    /// fingerprint comes directly from the same saved CapabilityEvidence.
    /// The default deliberately refuses; it never falls back to environment lookup.
    fn load_ephemeral(
        &self,
        _reference: &CredentialReference,
        _expected_account_sha256: &str,
    ) -> Result<CredentialBundle, Failure> {
        Err(Failure::Configuration)
    }
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
            CredentialReference::EphemeralAuthOnly { .. } => {
                // Ephemeral auth is opt-in through a dedicated host source. This
                // source never interprets it as environment-backed credentials.
                return Err(Failure::Configuration);
            }
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

/// Opt-in guard for one finite episode grant, not general exposure-policy support.
/// The retained name covers intake delegation and optional-activity deadlines.
/// A trusted transport must call begin_launch at its final actual launch boundary.
/// The shared observation survives cancellation or dropping the transport future.
pub struct IntakeLaunchGuard {
    expires_at_unix: i64,
    clock: Arc<dyn Clock>,
    checked_at_unix: std::sync::atomic::AtomicU64,
}
impl IntakeLaunchGuard {
    fn new(expires_at_unix: i64, clock: Arc<dyn Clock>) -> Self {
        Self {
            expires_at_unix,
            clock,
            checked_at_unix: std::sync::atomic::AtomicU64::new(u64::MAX),
        }
    }

    /// No await or other preparatory work may separate this check from launch.
    /// Success is permission for this single launch, never a reusable grant.
    pub fn begin_launch(&self) -> Result<(), Failure> {
        let deadline = u64::try_from(self.expires_at_unix).map_err(|_| Failure::Configuration)?;
        if deadline == 0 {
            return Err(Failure::Configuration);
        }
        let now = self.clock.unix_seconds()?;
        if now >= deadline {
            return Err(Failure::IntakeExpired);
        }
        self.checked_at_unix
            .compare_exchange(
                u64::MAX,
                now,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map_err(|_| Failure::Binding)?;
        Ok(())
    }

    fn checked_at_unix(&self) -> Option<u64> {
        let value = self
            .checked_at_unix
            .load(std::sync::atomic::Ordering::Acquire);
        (value != u64::MAX).then_some(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    #[error("invalid direct provider configuration")]
    Configuration,
    #[error("selected route/model binding is inconsistent")]
    ModelUnavailable,
    #[error("explicit credential source is unavailable")]
    CredentialMissing,
    #[error("credential is invalid")]
    InvalidCredential,
    #[error("OAuth credential expired; a separate access renewal is required")]
    CredentialExpired,
    #[error("episode authorization expired before provider launch")]
    IntakeExpired,
    #[error("provider or transport cannot enforce the final episode launch guard")]
    IntakeGuardUnsupported,
    #[error("credential grant does not match the selected mode")]
    GrantMismatch,
    #[error("credential account does not match the saved account binding")]
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
/// Redacted fixed-shape stream failure metadata. Values are static labels,
/// numeric positions or bounded digests, never free-form provider text.
#[derive(Clone, Debug, Serialize)]
pub struct StreamDiagnostic {
    pub stage: &'static str,
    pub event_index: Option<u64>,
    pub event_type: Option<&'static str>,
    pub event_type_sha256: Option<String>,
    /// Sanitized nested error type; distinct from the SSE event's type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_error_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_error_code: Option<&'static str>,
    /// Unknown string labels are fingerprinted, never copied into diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_error_type_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_error_code_sha256: Option<String>,
    pub rule: &'static str,
    pub field: Option<&'static str>,
}
/// An observed provider response identifier, retained only in the protected host
/// receipt. Observation does not establish protocol binding, completion or retrieval authority.
/// Requests still use store:false; no remote retrieval or automatic retry is added.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct RetainedResponseId(String);
impl RetainedResponseId {
    fn from_wire(value: &str) -> Option<Self> {
        (value.starts_with("resp_")
            && value.len() > 5
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
        .then(|| Self(value.into()))
    }
}
impl fmt::Debug for RetainedResponseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RetainedResponseId([REDACTED])")
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct AttemptReceipt {
    pub audit: RequestAudit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intake_expires_at_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intake_prelaunch_checked_at_unix: Option<u64>,
    pub dispatch_may_have_begun: bool,
    pub http_status: Option<u16>,
    pub stream_bytes_observed: u64,
    pub stream_eof: bool,
    pub event_count: u64,
    pub response_completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_diagnostic: Option<StreamDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<RetainedResponseId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id_sha256: Option<String>,
    /// Last explicit provider report, not independently verified served identity.
    /// An omitted/null snapshot supplies no new report.
    pub reported_effort: Option<String>,
    /// The prior report if two snapshots contradict. Both values are bounded.
    pub previous_reported_effort: Option<String>,
    /// Safe reported model label retained even when host binding rejects output.
    /// This is provider-reported metadata, not proof of served-model identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_model: Option<String>,
    pub usage: Usage,
    pub elapsed_ms: u64,
    pub remote_stop_confirmed: bool,
    pub remote_effects_and_charges_may_remain: bool,
}
impl AttemptReceipt {
    fn new(audit: RequestAudit) -> Self {
        Self {
            audit,
            intake_expires_at_unix: None,
            intake_prelaunch_checked_at_unix: None,
            dispatch_may_have_begun: false,
            http_status: None,
            stream_bytes_observed: 0,
            stream_eof: false,
            event_count: 0,
            response_completed: false,
            stream_diagnostic: None,
            response_id: None,
            response_id_sha256: None,
            reported_effort: None,
            previous_reported_effort: None,
            reported_model: None,
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
                .is_some_and(|effort| {
                    reported_effort_label(effort).is_none() || effort != intent.effort.as_str()
                })
            || self.attempt.previous_reported_effort.is_some()
            || !self.attempt.response_completed
            || !self.attempt.stream_eof
            || self.usage != self.attempt.usage
        {
            // Retain bounded observations even when the host result fails its
            // binding check. The failure belongs to the expected admission.
            let mut receipt = self.attempt;
            receipt.audit = audit.clone();
            // Preserve only the same bounded safe identifier accepted for model
            // configuration. Do not weaken the exact equality check above.
            receipt.reported_model = self.reported_model.filter(|value| identifier(value));
            receipt.reported_effort = receipt
                .reported_effort
                .filter(|value| reported_effort_label(value).is_some());
            receipt.previous_reported_effort = receipt
                .previous_reported_effort
                .filter(|value| reported_effort_label(value).is_some());
            receipt.dispatch_may_have_begun = true;
            receipt.remote_effects_and_charges_may_remain = true;
            return Err(attempt_error(Failure::Binding, receipt));
        }
        Ok(self)
    }
}

pub struct ProviderRequest {
    pub prompt: String,
    /// The unconstrained object schema selects JSON-object transport. Other
    /// schemas use strict structured output. The application validates its
    /// complete response in either case.
    pub output_schema: Value,
    pub invocation_id: String,
}

/// A complete application JSON object, without an encoded JSON-string carrier.
/// Open-ended operation inputs remain governed by the application's schema and
/// parser; they cannot be expressed as closed objects in strict output schemas.
pub fn json_object_schema() -> Value {
    json!({"type":"object"})
}
#[derive(Clone, Debug)]
pub struct ProviderIntent {
    pub model: String,
    pub effort: String,
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

    /// Unsupported providers fail before dispatch. Implementations opting in
    /// must enforce this exact deadline at their final actual launch boundary.
    fn dispatch_with_intake_deadline(
        self: Box<Self>,
        _cancellation: Cancellation,
        _expires_at_unix: i64,
    ) -> BoxFuture<'static, Result<ProviderOutput, ProviderError>> {
        let cancelled = self.cancel();
        Box::pin(async move {
            cancelled?;
            Err(ProviderError::Rejected(Failure::IntakeGuardUnsupported))
        })
    }
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
    /// Header-level eligibility for unencoded SSE in the request's explicit mode.
    /// SIWC may omit Content-Type; this is provisional until the bounded
    /// collector validates completion and the body reaches clean EOF.
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

    /// Opt in only when begin_launch can run immediately before actual launch.
    /// Checking before calling an unguarded, potentially delayed post is unsafe.
    fn post_with_intake_guard(
        &self,
        _request: HttpRequest,
        _guard: Arc<IntakeLaunchGuard>,
    ) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
        Box::pin(async { Err(Failure::IntakeGuardUnsupported) })
    }
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
        self.post_guarded(request, None)
    }

    fn post_with_intake_guard(
        &self,
        request: HttpRequest,
        guard: Arc<IntakeLaunchGuard>,
    ) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
        self.post_guarded(request, Some(guard))
    }
}
impl ReqwestTransport {
    fn post_guarded(
        &self,
        request: HttpRequest,
        guard: Option<Arc<IntakeLaunchGuard>>,
    ) -> BoxFuture<'_, Result<HttpResponse, Failure>> {
        Box::pin(async move {
            let mode = request.mode;
            let request = self.build_request(request)?;
            // All request preparation is complete. Sample the current clock in
            // this polled future immediately before handing the request to HTTP.
            if let Some(guard) = &guard {
                guard.begin_launch()?;
            }
            let response = self
                .client
                .execute(request)
                .await
                .map_err(|_| Failure::Network)?;
            let status = response.status().as_u16();
            Ok(HttpResponse {
                status,
                is_unencoded_sse: permits_unencoded_sse(response.headers(), mode),
                body: Box::new(ReqwestBody {
                    response,
                    pending: bytes::Bytes::new(),
                }),
            })
        })
    }
}
/// Admits headers for SSE parsing; it does not establish body validity or EOF.
fn permits_unencoded_sse(headers: &reqwest::header::HeaderMap, mode: AccessMode) -> bool {
    use reqwest::header::{CONTENT_ENCODING, CONTENT_TYPE};
    let content_types = headers.get_all(CONTENT_TYPE);
    let mut types = content_types.iter();
    let sse = match types.next() {
        // Both OAuth routes can omit this header. The bounded SSE collector
        // still requires a complete, internally consistent Responses stream;
        // absence of a media type is not evidence of a valid response body.
        None => matches!(mode, AccessMode::SiwcOAuth | AccessMode::ChatgptOAuth),
        Some(value) => value.to_str().ok().is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        }),
    } && types.next().is_none();
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
            &encoded(&json!({
                "settings": &settings,
                "transport_profile": settings.mode.wire_profile(),
            }))
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
        let bundle = match &settings.credentials {
            CredentialReference::EphemeralAuthOnly { .. } => self
                .source
                .load_ephemeral(&settings.credentials, &settings.evidence.account_sha256)?,
            _ => {
                let evidence_account = settings
                    .evidence
                    .account_env
                    .environment()
                    .ok_or(Failure::Configuration)?;
                self.source.load(&settings.credentials, evidence_account)?
            }
        };
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
        // Preserve the original CredentialSource contract: current OAuth expiry is
        // checked against the clock above, but a custom source may reacquire a token
        // with a different future expiry than the saved environment-reference snapshot.
        // SIWC's selected-vault source independently fences its saved record expiry.
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

/// One serializer shared by dispatch and exact response-only recovery checks.
fn response_only_body(
    profile: &WireProfile,
    model: &str,
    effort: &str,
    prompt: &str,
    schema: &Value,
) -> Value {
    let format = if schema == &json_object_schema() {
        json!({"type":"json_object"})
    } else {
        json!({"type":"json_schema","name":"persona_decision","strict":true,"schema":schema})
    };
    json!({"model":model,"store":false,"stream":true,
        "instructions":profile.instructions,
        "input":[{"role":profile.input_role,"content":[{"type":"input_text","text":prompt}]}],
        "reasoning":{"effort":effort},
        "text":{"format":format}})
}

/// Verify the exact immutable response-only wire contract without credentials or I/O.
/// The dispatch serializer proves that no tools or external action plan were sent.
pub(crate) fn response_only_request_matches(
    audit: &Value,
    model: &str,
    effort: &str,
    prompt: &str,
    schema: &Value,
) -> bool {
    [
        AccessMode::ChatgptOAuth,
        AccessMode::SiwcOAuth,
        AccessMode::PlatformApiKey,
    ]
    .into_iter()
    .any(|mode| {
        let profile = mode.wire_profile();
        if audit["transport_profile"] != profile.id {
            return false;
        }
        let body = response_only_body(&profile, model, effort, prompt, schema);
        let (Ok(body), Ok(schema)) = (serde_json::to_vec(&body), serde_json::to_vec(schema)) else {
            return false;
        };
        audit["prompt_sha256"] == digest(prompt.as_bytes())
            && audit["prompt_bytes"] == prompt.len() as u64
            && audit["schema_sha256"] == digest(&schema)
            && audit["schema_bytes"] == schema.len() as u64
            && audit["instructions_sha256"] == digest(profile.instructions.as_bytes())
            && audit["request_sha256"] == digest(&body)
            && audit["request_bytes"] == body.len() as u64
    })
}

impl ModelProvider for DirectProvider {
    fn intent(&self) -> ProviderIntent {
        let s = &self.inner.settings;
        ProviderIntent {
            model: s.model.clone(),
            effort: s.effort.clone(),
        }
    }
    fn prepare(&self, request: ProviderRequest) -> Result<Box<dyn PreparedCall>, ProviderError> {
        let prepare = || -> Result<DirectCall, Failure> {
            let settings = &self.inner.settings;
            if request.prompt.trim().is_empty() || !identifier(&request.invocation_id) {
                return Err(Failure::Configuration);
            }
            let schema = schema_bytes(&request.output_schema)?;
            let (_, account) = self.inner.credentials(None)?;
            let profile = settings.mode.wire_profile();
            let body = encoded(&response_only_body(
                &profile,
                &settings.model,
                &settings.effort,
                &request.prompt,
                &request.output_schema,
            ))?;
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
        self.dispatch_guarded(cancellation, None)
    }

    fn dispatch_with_intake_deadline(
        self: Box<Self>,
        cancellation: Cancellation,
        expires_at_unix: i64,
    ) -> BoxFuture<'static, Result<ProviderOutput, ProviderError>> {
        self.dispatch_guarded(cancellation, Some(expires_at_unix))
    }
}
impl DirectCall {
    fn dispatch_guarded(
        self: Box<Self>,
        cancellation: Cancellation,
        intake_expires_at_unix: Option<i64>,
    ) -> BoxFuture<'static, Result<ProviderOutput, ProviderError>> {
        Box::pin(async move {
            let DirectCall {
                inner,
                body,
                audit,
                account,
            } = *self;
            let mut receipt = AttemptReceipt::new(audit.clone());
            receipt.intake_expires_at_unix = intake_expires_at_unix;
            let intake_guard = intake_expires_at_unix
                .map(|deadline| Arc::new(IntakeLaunchGuard::new(deadline, inner.clock.clone())));
            if cancellation.is_cancelled() {
                return Err(attempt_error(Failure::Cancelled, receipt));
            }
            // Recheck expiry, grant, the immutable evidence digest and the
            // preparation's account fence immediately before the HTTP operation.
            let (bearer, account) = inner
                .credentials(Some(&account))
                .map_err(|failure| attempt_error(failure, receipt.clone()))?;
            let settings = &inner.settings;
            let mut collector = sse::Collector::new(&settings.model).with_mode(settings.mode);
            let started = tokio::time::Instant::now();
            let result = {
                let running = async {
                    if cancellation.is_cancelled() {
                        return Err(Failure::Cancelled);
                    }
                    let request = HttpRequest {
                        mode: settings.mode,
                        body,
                        bearer,
                        account_header: (settings.mode == AccessMode::ChatgptOAuth)
                            .then_some(account),
                    };
                    let response = if let Some(guard) = &intake_guard {
                        inner
                            .transport
                            .post_with_intake_guard(request, guard.clone())
                            .await?
                    } else {
                        // Preserve the existing unguarded transport contract.
                        receipt.dispatch_may_have_begun = true;
                        inner.transport.post(request).await?
                    };
                    receipt.http_status = Some(response.status);
                    if response.status != 200 {
                        return Err(Failure::HttpStatus);
                    }
                    if !response.is_unencoded_sse {
                        return Err(Failure::ContentType);
                    }
                    let mut body = response.body;
                    loop {
                        // Even an always-ready body must allow explicit
                        // cancellation, including on empty chunks.
                        tokio::task::yield_now().await;
                        // A read chunk size is transport batching, not a stream quota.
                        let maximum = 8192;
                        match body.read(maximum).await? {
                            Some(bytes) => {
                                if bytes.len() > maximum {
                                    return Err(collector.transport_limit(true));
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
                    result = running => result,
                }
            };
            if let Some(guard) = &intake_guard {
                receipt.intake_prelaunch_checked_at_unix = guard.checked_at_unix();
                receipt.dispatch_may_have_begun =
                    receipt.intake_prelaunch_checked_at_unix.is_some();
            }
            receipt.stream_bytes_observed = collector.observed as u64;
            receipt.event_count = collector.events as u64;
            receipt.response_completed = collector.completed;
            receipt.stream_diagnostic = collector.diagnostic.clone();
            receipt.response_id = collector.retained_response_id();
            receipt.response_id_sha256 = collector.response_id_sha256();
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
fn secret_valid(secret: &Secret, _historical_maximum: usize) -> bool {
    !secret.0.is_empty() && secret.0.bytes().all(|b| b.is_ascii_graphic())
}
fn identifier(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_control() || c.is_whitespace())
}
// Preserve the exact reported label without a host-maintained vocabulary gate.
// The request/response binding still checks it against the selected effort.
fn reported_effort_label(value: &str) -> Option<&str> {
    identifier(value).then_some(value)
}
fn digest_string(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn encoded<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Failure> {
    serde_json::to_vec(value).map_err(|_| Failure::Configuration)
}
fn schema_bytes(value: &Value) -> Result<Vec<u8>, Failure> {
    if !value.is_object() {
        return Err(Failure::Configuration);
    }
    encoded(value)
}
