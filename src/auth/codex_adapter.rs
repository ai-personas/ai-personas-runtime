//! Adapter from the selected Codex auth-status source to native ChatGPT OAuth.
//! Each load acquires credentials once and binds account and expiry to the same
//! returned bearer. Parsing imposes no application token or account byte quota.
//! Decoded claims provide consistency checks; trust comes from the selected source.
use crate::provider as p;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodexAuthMethod { Chatgpt, Unsupported }

/// Typed subset of getAuthStatus. Only exact upstream `chatgpt` maps to Chatgpt.
pub(crate) struct AuthStatusSnapshot {
    pub(crate) auth_method: Option<CodexAuthMethod>,
    pub(crate) auth_token: Option<String>,
}

/// One acquisition from the explicitly selected credential source. Implementations
/// retain process ownership until the actual result and cleanup are observed.
pub(crate) trait AuthorizedAuthStatusSource: Send + Sync {
    fn acquire(&self) -> Result<AuthStatusSnapshot, p::Failure>;
}

pub(super) fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, p::Failure> {
    // Validate complete JSON and duplicate keys without parsing quotas.
    crate::files::parse_json_unique(bytes).map_err(|_| p::Failure::InvalidCredential)
}

pub(crate) struct CodexCredentialSource<S> {
    source: S,
    expected_account_sha256: String,
}

impl<S: AuthorizedAuthStatusSource> CodexCredentialSource<S> {
    pub(crate) fn new(source: S, expected_account_sha256: String) -> Result<Self, p::Failure> {
        if expected_account_sha256.len() != 64
            || !expected_account_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(p::Failure::Configuration);
        }
        Ok(Self { source, expected_account_sha256 })
    }
}

#[derive(Deserialize)]
struct AccessClaims {
    exp: Option<i64>,
    #[serde(rename = "https://api.openai.com/auth")]
    auth: Option<AccountClaims>,
}
#[derive(Deserialize)]
struct AccountClaims { chatgpt_account_id: Option<String> }

fn token_identity_and_expiry(token: &str) -> Result<(String, u64), p::Failure> {
    if token.is_empty()
        || !token.bytes().all(|b| b.is_ascii_graphic())
    { return Err(p::Failure::InvalidCredential); }
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else { return Err(p::Failure::InvalidCredential); };
    if header.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(p::Failure::InvalidCredential);
    }
    let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| p::Failure::InvalidCredential)?;
    let claims: AccessClaims = serde_json::from_value(parse_json(&payload)?).map_err(|_| p::Failure::InvalidCredential)?;
    let expiry = claims.exp.and_then(|v| u64::try_from(v).ok()).filter(|v| *v > 0)
        .ok_or(p::Failure::InvalidCredential)?;
    let account = claims.auth.and_then(|a| a.chatgpt_account_id).ok_or(p::Failure::AccountMismatch)?;
    if account.is_empty()
        || !account.bytes().all(|b| b.is_ascii_graphic())
    { return Err(p::Failure::AccountMismatch); }
    Ok((account, expiry))
}

fn account_digest(account: &str) -> String { hex::encode(Sha256::digest(account.as_bytes())) }

/// Public picker metadata, with no bearer tokens or model calls.
#[derive(serde::Serialize)]
pub struct CodexModelCatalogue {
    pub models: Vec<super::codex_jsonrpc::CodexModelChoice>,
    pub inference_performed: bool,
    pub capability_verified: bool,
}

/// Graphical account status/sign-in. All output is a deliberate metadata
/// projection; bearer tokens never cross this helper boundary.
pub fn codex_account(
    executable: &std::path::Path, host_home: &std::path::Path, codex_home: &std::path::Path,
    login: Option<bool>, cancellation: p::Cancellation, event: impl FnMut(serde_json::Value),
) -> Result<serde_json::Value, super::onboarding::Code> {
    use super::onboarding::Code;
    let executable = std::fs::canonicalize(executable).map_err(|_| Code::CredentialUnavailable)?;
    let host_home = std::fs::canonicalize(host_home).map_err(|_| Code::CredentialUnavailable)?;
    // A first Codex sign-in may create its selected home; status never does.
    if login.is_some() {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(codex_home)
            .map_err(|_| Code::CredentialUnavailable)?;
    }
    let codex_home = std::fs::canonicalize(codex_home).map_err(|_| Code::CredentialUnavailable)?;
    if !executable.is_file() || !host_home.is_dir() || !codex_home.is_dir() {
        return Err(Code::CredentialUnavailable);
    }
    let factory = super::codex_jsonrpc::ProcessJsonRpcPeerFactory::new(
        executable, vec!["app-server".into()], host_home, codex_home,
    ).with_cancellation(cancellation);
    let rpc = super::codex_jsonrpc::AuthOnlyJsonRpc::new(factory);
    match login {
        Some(device) => rpc.account_login(device, event),
        None => rpc.account_read(),
    }.map_err(|_| Code::CredentialUnavailable)
}

/// Discover models and reasoning options through the selected Codex CLI.
/// This lists client metadata; it neither requests credentials nor starts inference.
pub fn catalogue_codex(
    executable: &std::path::Path,
    host_home: &std::path::Path,
    codex_home: &std::path::Path,
) -> Result<CodexModelCatalogue, super::onboarding::Code> {
    catalogue_codex_cancellable(executable, host_home, codex_home, p::Cancellation::default())
}

pub fn catalogue_codex_cancellable(
    executable: &std::path::Path,
    host_home: &std::path::Path,
    codex_home: &std::path::Path,
    cancellation: p::Cancellation,
) -> Result<CodexModelCatalogue, super::onboarding::Code> {
    use super::onboarding::Code;
    let executable = std::fs::canonicalize(executable).map_err(|_| Code::CatalogueUnavailable)?;
    let host_home = std::fs::canonicalize(host_home).map_err(|_| Code::CatalogueUnavailable)?;
    let codex_home = std::fs::canonicalize(codex_home).map_err(|_| Code::CatalogueUnavailable)?;
    if !executable.is_file() || !host_home.is_dir() || !codex_home.is_dir() {
        return Err(Code::CatalogueUnavailable);
    }
    let factory = super::codex_jsonrpc::ProcessJsonRpcPeerFactory::new(
        executable, vec!["app-server".into()], host_home, codex_home,
    ).with_cancellation(cancellation);
    let models = super::codex_jsonrpc::AuthOnlyJsonRpc::new(factory)
        .request_models().map_err(|_| Code::CatalogueUnavailable)?;
    Ok(CodexModelCatalogue { models, inference_performed: false, capability_verified: false })
}

/// Bind fresh settings to the selected Codex login without exporting credentials.
/// This performs authentication only; model capability remains unverified.
#[cfg(unix)]
pub fn configure_codex(
    executable: &std::path::Path,
    host_home: &std::path::Path,
    codex_home: &std::path::Path,
    model: &str,
    effort: &str,
) -> Result<p::DirectSettings, p::ProviderError> {
    use p::Clock;
    let configure = || -> Result<p::DirectSettings, p::Failure> {
        if [model, effort].iter().any(|value| value.is_empty()
            || value.chars().any(|c| c.is_whitespace() || c.is_control())) {
            return Err(p::Failure::Configuration);
        }
        let executable = std::fs::canonicalize(executable).map_err(|_| p::Failure::CredentialMissing)?;
        let host_home = std::fs::canonicalize(host_home).map_err(|_| p::Failure::CredentialMissing)?;
        let codex_home = std::fs::canonicalize(codex_home).map_err(|_| p::Failure::CredentialMissing)?;
        if !executable.is_file() || !host_home.is_dir() || !codex_home.is_dir() {
            return Err(p::Failure::CredentialMissing);
        }
        let factory = super::codex_jsonrpc::ProcessJsonRpcPeerFactory::new(
            executable, vec!["app-server".into()], host_home, codex_home,
        );
        let status = super::codex_jsonrpc::AuthOnlyJsonRpc::new(factory).acquire()?;
        if status.auth_method != Some(CodexAuthMethod::Chatgpt) {
            return Err(p::Failure::GrantMismatch);
        }
        let token = status.auth_token.ok_or(p::Failure::CredentialMissing)?;
        let (account, expiry) = token_identity_and_expiry(&token)?;
        if p::SystemClock.unix_seconds()? >= expiry {
            return Err(p::Failure::CredentialExpired);
        }
        Ok(p::DirectSettings {
            mode: p::AccessMode::ChatgptOAuth,
            model: model.into(),
            effort: effort.into(),
            credentials: p::CredentialReference::EphemeralAuthOnly {
                grant: p::OAuthGrant::TraditionalChatgpt,
                expires_at_unix: expiry,
            },
            evidence: p::CapabilityEvidence {
                mode: p::AccessMode::ChatgptOAuth,
                model: model.into(),
                efforts: Default::default(),
                account_env: p::AccountBindingReference::EphemeralAuthOnly { ephemeral_auth_only: true },
                account_sha256: account_digest(&account),
                transport_profile: p::AccessMode::ChatgptOAuth.profile().into(),
                catalog_evidence_sha256: None,
                structured_output_evidence_sha256: None,
            },
        })
    };
    let settings = configure().map_err(p::ProviderError::Rejected)?;
    settings.validate()?;
    Ok(settings)
}


/// Process-backed acquisition for an explicitly selected helper executable.
#[cfg(unix)]
pub(crate) struct ProcessAuthStatusSource {
    executable: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
}

#[cfg(unix)]
impl ProcessAuthStatusSource {
    pub(crate) fn new(executable: std::path::PathBuf, args: Vec<std::ffi::OsString>) -> Self {
        Self { executable, args }
    }
}

#[cfg(unix)]
impl AuthorizedAuthStatusSource for ProcessAuthStatusSource {
    fn acquire(&self) -> Result<AuthStatusSnapshot, p::Failure> {
        use std::process::Command;
        let mut command = Command::new(&self.executable);
        command.args(&self.args);
        let output = crate::auth::codex_process::run(&mut command)
            .map_err(|_| p::Failure::CredentialMissing)?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireStatus {
            auth_method: Option<String>,
            auth_token: Option<String>,
        }
        // No child output or parser details enter error messages.
        let wire: WireStatus = serde_json::from_value(parse_json(&output.stdout)?)
            .map_err(|_| p::Failure::InvalidCredential)?;
        let auth_method = match wire.auth_method.as_deref() {
            Some("chatgpt") => Some(CodexAuthMethod::Chatgpt),
            Some(_) => Some(CodexAuthMethod::Unsupported),
            None => None,
        };
        Ok(AuthStatusSnapshot { auth_method, auth_token: wire.auth_token })
    }
}

impl<S: AuthorizedAuthStatusSource> CodexCredentialSource<S> {
    fn acquire_bundle(&self) -> Result<p::CredentialBundle, p::Failure> {
        let status = self.source.acquire()?;
        if status.auth_method != Some(CodexAuthMethod::Chatgpt) {
            return Err(p::Failure::GrantMismatch);
        }
        let token = status.auth_token.ok_or(p::Failure::CredentialMissing)?;
        let (account_id, expires_at_unix) = token_identity_and_expiry(&token)?;
        if account_digest(&account_id) != self.expected_account_sha256 {
            return Err(p::Failure::AccountMismatch);
        }
        let account = p::Secret::new(account_id);
        Ok(p::CredentialBundle {
            credential: p::Credential::OAuth {
                grant: p::OAuthGrant::TraditionalChatgpt,
                bearer: p::Secret::new(token),
                account: account.clone(),
                expires_at_unix,
            },
            evidence_account: account,
        })
    }
}

impl<S: AuthorizedAuthStatusSource> p::CredentialSource for CodexCredentialSource<S> {
    fn load(
        &self,
        reference: &p::CredentialReference,
        evidence_account: &p::SecretReference,
    ) -> Result<p::CredentialBundle, p::Failure> {
        let (token_env, account_env) = match reference {
            p::CredentialReference::OAuth {
                grant: p::OAuthGrant::TraditionalChatgpt,
                token_env,
                account_env,
                ..
            } => (token_env, account_env),
            _ => return Err(p::Failure::GrantMismatch),
        };

        // Preserve legacy behavior: the configured credential account reference
        // and evidence reference may use different names. Provider validation
        // compares actual account values and the saved account fingerprint.
        if token_env.env.is_empty()
            || account_env.env.is_empty()
            || evidence_account.env.is_empty()
        {
            return Err(p::Failure::Configuration);
        }
        self.acquire_bundle()
    }

    fn load_ephemeral(
        &self,
        reference: &p::CredentialReference,
        expected_account_sha256: &str,
    ) -> Result<p::CredentialBundle, p::Failure> {
        match reference {
            p::CredentialReference::EphemeralAuthOnly {
                grant: p::OAuthGrant::TraditionalChatgpt,
                expires_at_unix,
            } if *expires_at_unix > 0 => {}
            _ => return Err(p::Failure::GrantMismatch),
        }

        // This must be the fingerprint from saved CapabilityEvidence. Never
        // derive or update it from the newly acquired token.
        if expected_account_sha256 != self.expected_account_sha256 {
            return Err(p::Failure::AccountMismatch);
        }

        // Saved expiry is a snapshot. Return the fresh token's actual expiry;
        // DirectProvider enforces it against the injected clock before dispatch.
        self.acquire_bundle()
    }
}
