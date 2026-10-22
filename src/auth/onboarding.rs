//! Operator-side SIWC configuration and optional capability observations.
//!
//! This is isolated host conformance work, not persona judgment. All production
//! credentials come from the explicitly selected locked Personas vault. No login,
//! refresh, account switch, runtime migration, inference retry, or grants are
//! implicit. Same-UID/root tampering is outside the vault's integrity boundary.
use super::{AuthError, Clock as AuthClock, LockedVault, Vault};
use crate::provider::{self as p, onboarding as wire};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    sync::Arc,
};

pub type Result<T> = std::result::Result<T, Code>;
pub const COST_NOTICE: &str = "A selected SIWC probe may consume ChatGPT plan usage. Application duration, byte and observed-token quotas are disabled. Actual provider usage is retained when reported; unknown outcomes remain unknown and an existing request identity is never replayed.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    #[error("invalid_request")]
    InvalidRequest,
    #[error("consent_denied")]
    Denied,
    #[error("selected_vault_busy")]
    Busy,
    #[error("protected_vault_unavailable")]
    VaultUnavailable,
    #[error("credential_unavailable")]
    CredentialUnavailable,
    #[error("credential_expired")]
    CredentialExpired,
    #[error("credential_refresh_uncertain_or_fenced")]
    CredentialFenced,
    #[error("account_changed_evidence_invalid")]
    AccountChanged,
    #[error("credential_changed")]
    CredentialChanged,
    #[error("catalogue_unavailable")]
    CatalogueUnavailable,
    #[error("catalogue_stale_or_changed")]
    CatalogueStale,
    #[error("model_not_selectable")]
    ModelNotSelectable,
    #[error("effort_label_unsupported_by_local_protocol")]
    EffortLabelUnsupported,
    #[error("remote_token_hard_bound_unsupported")]
    RemoteTokenBoundUnsupported,
    #[error("local_bound_exceeded")]
    Limit,
    #[error("key_bound_to_different_request")]
    KeyConflict,
    #[error("plan_unavailable")]
    Unavailable,
    #[error("prior_probe_unresolved_no_fresh_dispatch")]
    UnresolvedProbe,
    #[error("retained_state_invalid")]
    InvalidState,
    #[error("storage_unavailable")]
    Storage,
    #[error("persistence_uncertain_inspect_no_replay")]
    PersistenceUncertain,
    #[error("network_or_protocol_unavailable")]
    Network,
    #[error("deadline_exceeded")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
    #[error("proof_unestablished")]
    ProofUnestablished,
    #[error("completed_body_withheld_malformed_or_secret")]
    BodyWithheld,
    #[error("response_protocol_rejected")]
    ResponseRejected,
    #[error("saved_provider_binding_unsupported")]
    SavedProviderUnsupported,
}
fn auth_code(e: AuthError) -> Code {
    match e {
        AuthError::Expired => Code::CredentialExpired,
        AuthError::Busy => Code::Busy,
        AuthError::State => Code::CredentialFenced,
        AuthError::Binding => Code::AccountChanged,
        AuthError::Denied | AuthError::Scope => Code::Denied,
        AuthError::CommitUncertain => Code::PersistenceUncertain,
        _ => Code::VaultUnavailable,
    }
}
fn check(value: bool, code: Code) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(code)
    }
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identifier(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}
fn valid_key(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Code::InvalidState)
}
fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    Ok(hash(&encode(value)?))
}
fn unique(bytes: &[u8], _historical_maximum: usize) -> Result<Value> {
    crate::files::parse_json_unique(
        bytes,
        &crate::files::JsonLimits {
            max_source_bytes: 0,
            max_depth: 32,
            max_nodes: 65_536,
            max_decoded_bytes: 0,
        },
    )
    .map_err(|_| Code::InvalidState)
}

struct Now(u64);
impl AuthClock for Now {
    fn now_unix(&self) -> u64 {
        self.0
    }
}
struct Access {
    account_sha256: String,
    credential_sha256: String,
    expires_at_unix: u64,
    bearer: p::Secret,
    redactions: Vec<p::Secret>,
}
opaque_debug!(Access);
trait AccessSource: Send + Sync {
    fn load(&self, vault: &LockedVault, now: u64) -> Result<Access>;
}
struct SelectedVault;
impl AccessSource for SelectedVault {
    fn load(&self, vault: &LockedVault, now: u64) -> Result<Access> {
        let record = vault
            .load()
            .map_err(auth_code)?
            .ok_or(Code::CredentialUnavailable)?;
        let handoff = record.handoff(&Now(now)).map_err(auth_code)?;
        // A private one-way generation commitment, never a token value. It fences
        // even replacements with identical account and expiry metadata.
        let credential_sha256 = digest(&record.data)?;
        Ok(Access {
            account_sha256: hash(handoff.account_binding.as_bytes()),
            credential_sha256,
            expires_at_unix: record.expires_at_unix(),
            bearer: p::Secret::new(handoff.access_token),
            redactions: vec![
                p::Secret::new(record.data.access_token.clone()),
                p::Secret::new(record.data.refresh_token.clone()),
                p::Secret::new(record.data.id_token.clone()),
            ],
        })
    }
}
fn contains_secret(value: &Value, access: &Access, depth: usize) -> bool {
    match value {
        Value::String(s) => {
            if access.redactions.iter().any(|v| s.contains(v.expose())) {
                return true;
            }
            // Also inspect JSON strings embedded in the carrier, after decoding
            // escapes. Non-JSON text is never interpreted as executable material.
            if s.starts_with('{') || s.starts_with('[') {
                if let Ok(v) = unique(s.as_bytes(), wire::MAX_CARRIER) {
                    return contains_secret(&v, access, depth + 1);
                }
            }
            false
        }
        Value::Array(a) => a.iter().any(|v| contains_secret(v, access, depth + 1)),
        Value::Object(o) => o.iter().any(|(k, v)| {
            access
                .redactions
                .iter()
                .any(|secret| k.contains(secret.expose()))
                || contains_secret(v, access, depth + 1)
        }),
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelChoice {
    pub slug: String,
    pub display_name: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    account_sha256: String,
    credential_sha256: String,
    credential_expires_at_unix: u64,
    observed_at_unix: u64,
    body: String,
}
fn models(body: &str) -> Result<Vec<ModelChoice>> {
    let value =
        unique(body.as_bytes(), wire::CATALOG_BYTES).map_err(|_| Code::CatalogueUnavailable)?;
    let entries = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or(Code::CatalogueUnavailable)?;
    let mut selected = Vec::new();
    let mut slugs = BTreeSet::new();
    for entry in entries {
        check(entry.is_object(), Code::CatalogueUnavailable)?;
        let slug = entry
            .get("slug")
            .and_then(Value::as_str)
            .ok_or(Code::CatalogueUnavailable)?;
        let display_name = entry
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(slug);
        check(
            identifier(slug)
                && !display_name.trim().is_empty()
                && !display_name.chars().any(char::is_control)
                && slugs.insert(slug),
            Code::CatalogueUnavailable,
        )?;
        selected.push(ModelChoice {
            slug: slug.into(),
            display_name: display_name.into(),
        });
    }
    Ok(selected)
}
impl Catalogue {
    fn validate(&self) -> Result<()> {
        check(
            is_hash(&self.account_sha256)
                && is_hash(&self.credential_sha256)
                && self.observed_at_unix > 0
                && self.observed_at_unix < self.credential_expires_at_unix,
            Code::InvalidState,
        )?;
        models(&self.body)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct CatalogueView {
    pub catalogue_sha256: String,
    pub account_sha256: String,
    pub credential_expires_at_unix: u64,
    pub observed_at_unix: u64,
    pub models: Vec<ModelChoice>,
    /// Illustrative labels only, NOT a required or supported-effort list.
    pub effort_candidates_unverified: Vec<&'static str>,
    pub strict_carrier_verified: bool,
    pub inference_performed: bool,
}
fn catalogue_view(c: &Catalogue) -> Result<CatalogueView> {
    Ok(CatalogueView {
        catalogue_sha256: digest(c)?,
        account_sha256: c.account_sha256.clone(),
        credential_expires_at_unix: c.credential_expires_at_unix,
        observed_at_unix: c.observed_at_unix,
        models: models(&c.body)?,
        effort_candidates_unverified: vec![
            "none", "minimal", "low", "medium", "high", "xhigh", "max",
        ],
        strict_carrier_verified: false,
        inference_performed: false,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub id: String,
    #[serde(default)]
    pub catalogue_sha256: Option<String>,
    pub model: String,
    pub effort: String,
    /// Historical request metadata; no local timeout or token quota is enforced.
    #[serde(default)]
    pub timeout_ms: u64,
    #[serde(default)]
    pub max_observed_total_tokens: u64,
    /// Historical metadata. The view reports the actual lack of remote hard bounds.
    #[serde(default)]
    pub require_remote_token_hard_bound: bool,
}
impl PlanRequest {
    fn validate(&self) -> Result<()> {
        check(
            valid_key(&self.id)
                && self.catalogue_sha256.as_deref().is_none_or(is_hash)
                && identifier(&self.model),
            Code::InvalidRequest,
        )?;
        check(identifier(&self.effort), Code::InvalidRequest)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbePlan {
    pub profile: String,
    pub request: PlanRequest,
    pub account_sha256: String,
    pub credential_sha256: String,
    pub credential_expires_at_unix: u64,
    pub created_at_unix: u64,
    pub authorization_deadline_unix: u64,
    pub request_sha256: String,
    pub schema_sha256: String,
}
impl ProbePlan {
    pub fn sha256(&self) -> Result<String> {
        digest(self)
    }
}
#[derive(Serialize)]
pub struct PlanView {
    pub plan: ProbePlan,
    pub plan_sha256: String,
    pub cost_notice: &'static str,
    pub request_identity_single_use: bool,
    pub resource_quotas_enforced: bool,
    pub maximum_request_bytes: Option<usize>,
    pub maximum_response_bytes: Option<usize>,
    pub maximum_stream_bytes: Option<usize>,
    pub remote_token_hard_bound: bool,
    pub dollar_hard_bound: bool,
}
#[derive(Clone, Copy, Default)]
pub struct ProbeConsent {
    pub authorize_one_request: bool,
    pub retain_private_evidence: bool,
    pub acknowledge_uncapped_remote_usage: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Terminal {
    finished_at_unix: u64,
    code: Option<Code>,
    observation: Option<wire::Observation>,
    completed_carrier_sha256: Option<String>,
    completed_carrier_bytes: Option<usize>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    plan: ProbePlan,
    catalogue: Option<Catalogue>,
    /// None is a plan only. Some means consumed, even without a terminal record.
    claimed_at_unix: Option<u64>,
    terminal: Option<Terminal>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    account_sha256: Option<String>,
    catalogue: Option<Catalogue>,
    attempts: BTreeMap<String, Attempt>,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbePhase {
    Planned,
    OutcomeUnrecorded,
    CompletedVerified,
    CompletedUnverified,
    NotDispatched,
    FailedOrUncertain,
}
#[derive(Debug, Serialize)]
pub struct ProbeStatus {
    pub id: String,
    pub plan_sha256: String,
    pub account_sha256: String,
    pub model: String,
    pub effort: String,
    pub phase: ProbePhase,
    pub code: Option<Code>,
    pub dispatch_may_have_begun: bool,
    pub remote_usage_may_remain: bool,
    pub http_status: Option<u16>,
    pub usage: Value,
    pub proof_sha256: Option<String>,
    pub replay_authorized: bool,
    pub current_credential_checked: bool,
}
fn verified(a: &Attempt) -> Result<bool> {
    let Some(t) = &a.terminal else {
        return Ok(false);
    };
    let Some(o) = &t.observation else {
        return Ok(false);
    };
    if t.code.is_some()
        || o.failure.is_some()
        || !o.dispatch_may_have_begun
        || !o.completed
        || !o.eof
        || o.http_status != Some(200)
        || o.reported_effort.as_deref() != Some(a.plan.request.effort.as_str())
        || o.previous_reported_effort.is_some()
    {
        return Ok(false);
    }
    let (Some(input), Some(output), Some(total)) = (
        o.usage.input_tokens,
        o.usage.output_tokens,
        o.usage.total_tokens,
    ) else {
        return Ok(false);
    };
    if input.checked_add(output) != Some(total)
        || total == 0
        || o.usage.cached_input_tokens.is_some_and(|v| v > input)
    {
        return Ok(false);
    }
    let Some(carrier) = o.carrier.as_ref() else {
        return Ok(false);
    };
    let Ok(inner) = crate::runtime::decode_provider_carrier(carrier.as_bytes(), wire::MAX_CARRIER)
    else {
        return Ok(false);
    };
    if crate::protocol::parse_decision(inner.as_bytes(), wire::MAX_CARRIER).is_err() {
        return Ok(false);
    }
    Ok(inner == wire::specimen(&a.plan.request.id).map_err(|_| Code::InvalidState)?)
}
fn unresolved(a: &Attempt) -> bool {
    if a.claimed_at_unix.is_none() {
        return false;
    }
    match &a.terminal {
        None => true,
        Some(t) => t.observation.as_ref().is_some_and(|o| {
            o.dispatch_may_have_begun
                && (o.failure.is_some()
                    || !o.eof
                    || !o.completed
                    || o.usage.input_tokens.is_none()
                    || o.usage.output_tokens.is_none()
                    || o.usage.total_tokens.is_none())
        }),
    }
}
fn status(a: &Attempt) -> Result<ProbeStatus> {
    let proof = verified(a)?;
    let observation = a.terminal.as_ref().and_then(|t| t.observation.as_ref());
    let phase = if a.claimed_at_unix.is_none() {
        ProbePhase::Planned
    } else if a.terminal.is_none() {
        ProbePhase::OutcomeUnrecorded
    } else if proof {
        ProbePhase::CompletedVerified
    } else if observation.is_some_and(|o| o.completed && o.eof) {
        ProbePhase::CompletedUnverified
    } else if observation.is_none_or(|o| !o.dispatch_may_have_begun) {
        ProbePhase::NotDispatched
    } else {
        ProbePhase::FailedOrUncertain
    };
    let dispatched = match (&a.claimed_at_unix, &a.terminal) {
        (Some(_), None) => true,
        (_, Some(t)) => t
            .observation
            .as_ref()
            .is_some_and(|o| o.dispatch_may_have_begun),
        _ => false,
    };
    Ok(ProbeStatus {
        id: a.plan.request.id.clone(),
        plan_sha256: a.plan.sha256()?,
        account_sha256: a.plan.account_sha256.clone(),
        model: a.plan.request.model.clone(),
        effort: a.plan.request.effort.clone(),
        phase,
        code: a.terminal.as_ref().and_then(|t| t.code),
        dispatch_may_have_begun: dispatched,
        remote_usage_may_remain: dispatched,
        http_status: observation.and_then(|o| o.http_status),
        usage: observation
            .map(|o| serde_json::to_value(&o.usage))
            .transpose()
            .map_err(|_| Code::InvalidState)?
            .unwrap_or(Value::Null),
        proof_sha256: if proof { Some(digest(a)?) } else { None },
        replay_authorized: false,
        current_credential_checked: false,
    })
}

/// Holds the selected account lock for the entire operation. Open never signs in,
/// probes, refreshes, imports credentials, or touches a Runtime/Store.
pub struct Backend {
    vault: LockedVault,
    clock: Arc<dyn p::Clock>,
    source: Arc<dyn AccessSource>,
    transport: Option<Arc<dyn wire::Transport>>,
}
opaque_debug!(Backend);
impl Backend {
    pub fn open(path: &Path) -> Result<Self> {
        let vault = Vault::open_for_onboarding(path)
            .and_then(|v| v.lock())
            .map_err(auth_code)?;
        let backend = Self {
            vault,
            clock: Arc::new(p::SystemClock),
            source: Arc::new(SelectedVault),
            transport: None,
        };
        Ok(backend)
    }
    fn now(&self) -> Result<u64> {
        self.clock.unix_seconds().map_err(|_| Code::InvalidState)
    }
    fn access(&self) -> Result<Access> {
        self.source.load(&self.vault, self.now()?)
    }
    fn transport(&self) -> Result<Arc<dyn wire::Transport>> {
        match &self.transport {
            Some(t) => Ok(t.clone()),
            None => Ok(Arc::new(
                p::ReqwestTransport::new().map_err(|_| Code::Network)?,
            )),
        }
    }
    fn read(&self) -> Result<Journal> {
        let Some(bytes) = self.vault.read_onboarding().map_err(auth_code)? else {
            return Ok(Journal {
                version: 1,
                ..Journal::default()
            });
        };
        let j: Journal =
            serde_json::from_value(unique(&bytes, 0)?).map_err(|_| Code::InvalidState)?;
        validate_journal(&j)?;
        Ok(j)
    }
    fn save(&self, j: &Journal) -> Result<()> {
        validate_journal(j)?;
        self.vault
            .write_onboarding(&encode(j)?)
            .map_err(|e| match e {
                AuthError::CommitUncertain => Code::PersistenceUncertain,
                _ => Code::Storage,
            })
    }
    /// Explicit catalogue read; exact private evidence is retained by default.
    pub async fn catalogue(
        &mut self,
        _authorize_read: bool,
        _retain_private_evidence: bool,
    ) -> Result<CatalogueView> {
        let mut j = self.read()?;
        let access = self.access()?;
        bind_account(&j, &access)?;
        let transport = self.transport()?;
        let guard = wire::expiry_guard(access.expires_at_unix, self.clock.clone())
            .map_err(|_| Code::CredentialExpired)?;
        let operation = async {
            let response = transport
                .catalog(access.bearer.clone(), guard)
                .await
                .map_err(|e| match e {
                    p::Failure::IntakeExpired | p::Failure::CredentialExpired => {
                        Code::CredentialExpired
                    }
                    _ => Code::Network,
                })?;
            check(!matches!(response.status, 401 | 403), Code::Denied)?;
            check(
                response.status == 200 && response.unencoded_json,
                Code::CatalogueUnavailable,
            )?;
            let mut stream = response.body;
            let mut bytes = Vec::new();
            loop {
                tokio::task::yield_now().await;
                let maximum = 8_192;
                match stream.read(maximum).await.map_err(|_| Code::Network)? {
                    Some(part) => {
                        check(part.len() <= maximum, Code::Limit)?;
                        bytes.extend_from_slice(&part);
                    }
                    None => break,
                }
            }
            Ok::<_, Code>(bytes)
        };
        let bytes = operation.await?;
        let value = unique(&bytes, wire::CATALOG_BYTES).map_err(|_| Code::CatalogueUnavailable)?;
        check(!contains_secret(&value, &access, 0), Code::BodyWithheld)?;
        let fresh = self.access()?;
        check(
            fresh.credential_sha256 == access.credential_sha256,
            Code::CredentialChanged,
        )?;
        let catalog = Catalogue {
            account_sha256: access.account_sha256.clone(),
            credential_sha256: access.credential_sha256,
            credential_expires_at_unix: access.expires_at_unix,
            observed_at_unix: self.now()?,
            body: String::from_utf8(bytes).map_err(|_| Code::CatalogueUnavailable)?,
        };
        let view = catalogue_view(&catalog)?;
        j.account_sha256 = Some(access.account_sha256);
        j.catalogue = Some(catalog);
        self.save(&j)?;
        Ok(view)
    }
    /// Persist an exact plan, not inference consent. Changed contents under a key
    /// conflict even after credential rotation or after its result is retained.
    pub fn plan(
        &mut self,
        request: PlanRequest,
        _retain_private_evidence: bool,
    ) -> Result<PlanView> {
        request.validate()?;
        let mut j = self.read()?;
        if let Some(old) = j.attempts.get(&request.id) {
            check(old.plan.request == request, Code::KeyConflict)?;
            return plan_view(&old.plan);
        }
        let access = self.access()?;
        bind_account(&j, &access)?;
        let catalogue = match &request.catalogue_sha256 {
            Some(expected) => {
                let catalogue = j.catalogue.as_ref().ok_or(Code::CatalogueUnavailable)?;
                check(digest(catalogue)? == *expected, Code::CatalogueStale)?;
                check_catalogue(catalogue, &access, self.now()?)?;
                Some(catalogue.clone())
            }
            None => None,
        };
        let body = wire::request(
            &request.id,
            &request.model,
            &request.effort,
            request.timeout_ms,
        )
        .map_err(|_| Code::InvalidRequest)?;
        let now = self.now()?;
        let plan = ProbePlan {
            profile: wire::PROBE_PROFILE.into(),
            request,
            account_sha256: access.account_sha256,
            credential_sha256: access.credential_sha256,
            credential_expires_at_unix: access.expires_at_unix,
            created_at_unix: now,
            authorization_deadline_unix: access.expires_at_unix,
            request_sha256: hash(&body),
            schema_sha256: digest(&crate::runtime::provider_schema())?,
        };
        let view = plan_view(&plan)?;
        j.account_sha256 = Some(plan.account_sha256.clone());
        j.attempts.insert(
            plan.request.id.clone(),
            Attempt {
                plan,
                catalogue,
                claimed_at_unix: None,
                terminal: None,
            },
        );
        self.save(&j)?;
        Ok(view)
    }
    pub fn probe_status(&self, id: &str, plan_sha256: &str) -> Result<ProbeStatus> {
        let j = self.read()?;
        status(find(&j, id, plan_sha256)?)
    }
    /// At most one possible dispatch per immutable ID. A claimed ID is never
    /// replayed, including after a cancelled future or uncertain terminal save.
    pub async fn execute(
        &mut self,
        id: &str,
        plan_sha256: &str,
        _consent: ProbeConsent,
        cancellation: p::Cancellation,
    ) -> Result<ProbeStatus> {
        let mut j = self.read()?;
        let old = find(&j, id, plan_sha256)?;
        if old.claimed_at_unix.is_some() {
            return status(old);
        }
        let access = self.access()?;
        bind_account(&j, &access)?;
        check_plan_access(&old.plan, &access, self.now()?)?;
        let plan = old.plan.clone();
        let transport = self.transport()?;
        let body = wire::request(
            id,
            &plan.request.model,
            &plan.request.effort,
            plan.request.timeout_ms,
        )
        .map_err(|_| Code::InvalidState)?;
        check(hash(&body) == plan.request_sha256, Code::InvalidState)?;
        j.attempts
            .get_mut(id)
            .ok_or(Code::Unavailable)?
            .claimed_at_unix = Some(self.now()?);
        self.save(&j)?; // Durable consumed identity BEFORE any possible HTTP dispatch.
        let operation = async {
            let current = self.access()?;
            check_plan_access(&plan, &current, self.now()?)?;
            let guard = wire::expiry_guard(current.expires_at_unix, self.clock.clone())
                .map_err(|_| Code::CredentialExpired)?;
            Ok::<_, Code>(
                wire::probe(
                    transport.as_ref(),
                    body,
                    current.bearer.clone(),
                    &plan.request.model,
                    plan.request.timeout_ms,
                    plan.request.max_observed_total_tokens,
                    guard,
                    cancellation,
                )
                .await,
            )
        }
        .await;
        let mut terminal = Terminal {
            finished_at_unix: self.now()?,
            code: None,
            observation: None,
            completed_carrier_sha256: None,
            completed_carrier_bytes: None,
        };
        match operation {
            Err(code) => terminal.code = Some(code),
            Ok(mut o) => {
                terminal.code = o.failure.map(|f| match f {
                    wire::WireFailure::Cancelled => Code::Cancelled,
                    wire::WireFailure::Timeout => Code::Timeout,
                    wire::WireFailure::Denied => Code::Denied,
                    wire::WireFailure::Expired => Code::CredentialExpired,
                    wire::WireFailure::Limit => Code::Limit,
                    wire::WireFailure::Http | wire::WireFailure::Transport => Code::Network,
                    wire::WireFailure::Protocol | wire::WireFailure::Binding => {
                        Code::ResponseRejected
                    }
                });
                if let Some(body) = &o.carrier {
                    terminal.completed_carrier_sha256 = Some(hash(body.as_bytes()));
                    terminal.completed_carrier_bytes = Some(body.len());
                    let safe =
                        crate::runtime::decode_provider_carrier(body.as_bytes(), wire::MAX_CARRIER)
                            .ok()
                            .and_then(|s| unique(s.as_bytes(), wire::MAX_CARRIER).ok())
                            .is_some_and(|v| !contains_secret(&v, &access, 0));
                    if !safe
                        || contains_secret(
                            &serde_json::to_value(&o).map_err(|_| Code::InvalidState)?,
                            &access,
                            0,
                        )
                    {
                        o.carrier = None;
                        terminal.code = Some(Code::BodyWithheld);
                    }
                }
                terminal.observation = Some(o);
            }
        }
        let attempt = j.attempts.get_mut(id).ok_or(Code::Unavailable)?;
        attempt.terminal = Some(terminal);
        if !verified(attempt)? {
            if let Some(terminal) = attempt.terminal.as_mut() {
                terminal.code.get_or_insert(Code::ProofUnestablished);
            }
        }
        let output = status(attempt)?;
        self.save(&j).map_err(|_| Code::PersistenceUncertain)?;
        Ok(output)
    }
    /// Select exact model/effort settings using the current vault identity. This
    /// performs no catalogue read or probe and claims no capability observations.
    pub fn configure(&self, model: &str, effort: &str) -> Result<p::DirectSettings> {
        check(
            identifier(model) && identifier(effort),
            Code::InvalidRequest,
        )?;
        native_settings(model, effort, self.access()?)
    }
    /// Optionally attach actual retained successful proof for the exact current
    /// credential generation. The ordinary configure path needs no such proof.
    pub fn configuration(&self, id: &str, plan_sha256: &str) -> Result<p::DirectSettings> {
        let j = self.read()?;
        let a = find(&j, id, plan_sha256)?;
        check(verified(a)?, Code::ProofUnestablished)?;
        let access = self.access()?;
        bind_account(&j, &access)?;
        check(
            a.plan.account_sha256 == access.account_sha256,
            Code::AccountChanged,
        )?;
        check(
            a.plan.credential_sha256 == access.credential_sha256
                && a.plan.credential_expires_at_unix == access.expires_at_unix,
            Code::CredentialChanged,
        )?;
        let mut settings = native_settings(&a.plan.request.model, &a.plan.request.effort, access)?;
        settings
            .evidence
            .efforts
            .insert(a.plan.request.effort.clone());
        settings.evidence.catalog_evidence_sha256 = a.catalogue.as_ref().map(digest).transpose()?;
        settings.evidence.structured_output_evidence_sha256 = Some(digest(a)?);
        settings.validate().map_err(|_| Code::InvalidState)?;
        Ok(settings)
    }
    /// Read-only same-account renewal diagnosis. No Runtime is opened and no
    /// existing expiry/evidence is silently rebound after authentication renewal.
    pub fn renewal_status(&self, saved: &p::DirectSettings) -> Result<RenewalStatus> {
        saved
            .validate()
            .map_err(|_| Code::SavedProviderUnsupported)?;
        let expiry = match &saved.credentials {
            p::CredentialReference::OAuth {
                grant: p::OAuthGrant::SiwcTokenSharing,
                token_env,
                account_env,
                expires_at_unix,
            } if saved.mode == p::AccessMode::SiwcOAuth
                && token_env.env == super::ACCESS_TOKEN_ENV
                && account_env.env == super::ACCOUNT_BINDING_ENV
                && saved
                    .evidence
                    .account_env
                    .environment()
                    .is_some_and(|account| account.env == super::ACCOUNT_BINDING_ENV) =>
            {
                *expires_at_unix
            }
            _ => return Err(Code::SavedProviderUnsupported),
        };
        let access = self.access()?;
        check(
            saved.evidence.account_sha256 == access.account_sha256,
            Code::AccountChanged,
        )?;
        // Optional history cannot gate an otherwise valid selected credential.
        // A missing, old or unreadable observation is never promoted to proof.
        let (current_proof, proof_check_code) =
            if saved.evidence.structured_output_evidence_sha256.is_some() {
                match self.read() {
                    Ok(journal) => (
                        journal.attempts.values().any(|a| {
                            a.plan.account_sha256 == access.account_sha256
                                && a.plan.credential_sha256 == access.credential_sha256
                                && a.plan.credential_expires_at_unix == access.expires_at_unix
                                && a.plan.request.model == saved.model
                                && a.plan.request.effort == saved.effort
                                && verified(a).unwrap_or(false)
                                && digest(a).ok().as_deref()
                                    == saved.evidence.structured_output_evidence_sha256.as_deref()
                        }),
                        None,
                    ),
                    Err(code) => (false, Some(code)),
                }
            } else {
                (false, None)
            };
        Ok(RenewalStatus {
            account_matches: true,
            saved_expiry: expiry,
            selected_expiry: access.expires_at_unix,
            explicit_provider_migration_required: expiry != access.expires_at_unix,
            fresh_catalogue_and_probe_required: false,
            capability_evidence_required: false,
            retained_probe_matches_current_credential: current_proof,
            retained_probe_check_code: proof_check_code,
            runtime_modified: false,
            inference_performed: false,
        })
    }
}
#[derive(Serialize)]
pub struct RenewalStatus {
    pub account_matches: bool,
    pub saved_expiry: u64,
    pub selected_expiry: u64,
    pub explicit_provider_migration_required: bool,
    pub fresh_catalogue_and_probe_required: bool,
    pub capability_evidence_required: bool,
    pub retained_probe_matches_current_credential: bool,
    pub retained_probe_check_code: Option<Code>,
    pub runtime_modified: bool,
    pub inference_performed: bool,
}
fn native_settings(model: &str, effort: &str, access: Access) -> Result<p::DirectSettings> {
    let account = p::SecretReference {
        env: super::ACCOUNT_BINDING_ENV.into(),
    };
    let settings = p::DirectSettings {
        mode: p::AccessMode::SiwcOAuth,
        model: model.into(),
        effort: effort.into(),
        credentials: p::CredentialReference::OAuth {
            grant: p::OAuthGrant::SiwcTokenSharing,
            token_env: p::SecretReference {
                env: super::ACCESS_TOKEN_ENV.into(),
            },
            account_env: account.clone(),
            expires_at_unix: access.expires_at_unix,
        },
        evidence: p::CapabilityEvidence {
            mode: p::AccessMode::SiwcOAuth,
            model: model.into(),
            efforts: BTreeSet::new(),
            account_env: p::AccountBindingReference::Environment(account),
            account_sha256: access.account_sha256,
            transport_profile: p::AccessMode::SiwcOAuth.profile().into(),
            catalog_evidence_sha256: None,
            structured_output_evidence_sha256: None,
        },
        limits: native_limits(),
    };
    settings.validate().map_err(|_| Code::InvalidState)?;
    Ok(settings)
}
fn native_limits() -> p::Limits {
    // Historical settings only; Direct transport does not enforce these quotas.
    p::Limits {
        timeout_ms: 300_000,
        max_prompt_bytes: 196_608,
        max_schema_bytes: 65_536,
        max_schema_nodes: 10_000,
        max_schema_depth: 64,
        max_request_bytes: 524_288,
        max_response_bytes: 32_768,
        max_stream_bytes: 1_048_576,
        // A completed event includes the full provider output envelope.
        max_event_bytes: 524_288,
        max_events: 10_000,
    }
}
fn bind_account(j: &Journal, a: &Access) -> Result<()> {
    check(
        j.account_sha256
            .as_ref()
            .is_none_or(|v| v == &a.account_sha256),
        Code::AccountChanged,
    )
}
fn check_catalogue(c: &Catalogue, a: &Access, now: u64) -> Result<()> {
    check(c.account_sha256 == a.account_sha256, Code::AccountChanged)?;
    check(
        c.credential_sha256 == a.credential_sha256
            && c.credential_expires_at_unix == a.expires_at_unix,
        Code::CredentialChanged,
    )?;
    check(now >= c.observed_at_unix, Code::CatalogueStale)
}
fn check_plan_access(p: &ProbePlan, a: &Access, now: u64) -> Result<()> {
    check(p.account_sha256 == a.account_sha256, Code::AccountChanged)?;
    check(
        p.credential_sha256 == a.credential_sha256
            && p.credential_expires_at_unix == a.expires_at_unix,
        Code::CredentialChanged,
    )?;
    check(
        now >= p.created_at_unix && now < a.expires_at_unix,
        Code::CredentialExpired,
    )
}
fn find<'a>(j: &'a Journal, id: &str, plan: &str) -> Result<&'a Attempt> {
    check(valid_key(id) && is_hash(plan), Code::InvalidRequest)?;
    let a = j.attempts.get(id).ok_or(Code::Unavailable)?;
    check(a.plan.sha256()? == plan, Code::KeyConflict)?;
    Ok(a)
}
fn plan_view(p: &ProbePlan) -> Result<PlanView> {
    Ok(PlanView {
        plan: p.clone(),
        plan_sha256: p.sha256()?,
        cost_notice: COST_NOTICE,
        request_identity_single_use: true,
        resource_quotas_enforced: false,
        maximum_request_bytes: None,
        maximum_response_bytes: None,
        maximum_stream_bytes: None,
        remote_token_hard_bound: false,
        dollar_hard_bound: false,
    })
}
fn validate_journal(j: &Journal) -> Result<()> {
    check(
        j.version == 1 && j.account_sha256.as_ref().is_none_or(|s| is_hash(s)),
        Code::InvalidState,
    )?;
    if let Some(c) = &j.catalogue {
        c.validate()?;
        check(
            j.account_sha256.as_ref() == Some(&c.account_sha256),
            Code::InvalidState,
        )?;
    }
    for (key, a) in &j.attempts {
        a.plan.request.validate().map_err(|_| Code::InvalidState)?;
        let p = &a.plan;
        if let Some(catalogue) = &a.catalogue {
            catalogue.validate()?;
            check(
                p.account_sha256 == catalogue.account_sha256
                    && p.credential_sha256 == catalogue.credential_sha256
                    && p.credential_expires_at_unix == catalogue.credential_expires_at_unix
                    && p.request.catalogue_sha256.as_deref() == Some(digest(catalogue)?.as_str())
                    && p.created_at_unix >= catalogue.observed_at_unix,
                Code::InvalidState,
            )?;
        } else {
            check(p.request.catalogue_sha256.is_none(), Code::InvalidState)?;
        }
        check(
            key == &p.request.id
                && p.profile == wire::PROBE_PROFILE
                && j.account_sha256.as_ref() == Some(&p.account_sha256)
                && is_hash(&p.account_sha256)
                && is_hash(&p.credential_sha256)
                && p.created_at_unix > 0
                && p.authorization_deadline_unix > p.created_at_unix
                && p.authorization_deadline_unix <= p.credential_expires_at_unix
                && p.schema_sha256 == digest(&crate::runtime::provider_schema())?
                && p.request_sha256
                    == hash(
                        &wire::request(
                            key,
                            &p.request.model,
                            &p.request.effort,
                            p.request.timeout_ms,
                        )
                        .map_err(|_| Code::InvalidState)?,
                    ),
            Code::InvalidState,
        )?;
        if let Some(at) = a.claimed_at_unix {
            check(
                at >= p.created_at_unix && at < p.credential_expires_at_unix,
                Code::InvalidState,
            )?;
        }
        if let Some(t) = &a.terminal {
            check(
                a.claimed_at_unix.is_some_and(|v| t.finished_at_unix >= v),
                Code::InvalidState,
            )?;
            if let Some(o) = &t.observation {
                check(
                    is_hash(&o.stream_sha256)
                        && o.reported_effort.as_deref().is_none_or(identifier)
                        && o.previous_reported_effort.as_deref().is_none_or(identifier)
                        && o.response_id_sha256.as_deref().is_none_or(is_hash),
                    Code::InvalidState,
                )?;
                if let Some(carrier) = &o.carrier {
                    check(
                        o.eof
                            && o.completed
                            && o.failure.is_none()
                            && t.completed_carrier_sha256.as_deref()
                                == Some(hash(carrier.as_bytes()).as_str())
                            && t.completed_carrier_bytes == Some(carrier.len()),
                        Code::InvalidState,
                    )?;
                }
            } else {
                check(t.code.is_some(), Code::InvalidState)?;
            }
            check(
                t.completed_carrier_sha256.as_deref().is_none_or(is_hash)
                    && t.completed_carrier_sha256.is_some() == t.completed_carrier_bytes.is_some(),
                Code::InvalidState,
            )?;
        }
    }
    Ok(())
}
