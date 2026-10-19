//! Opt-in adapter from an authorized Codex auth-status source to Personas' legacy
//! ChatGPT OAuth credential contract. No app-server calls, environment reads,
//! JWT signature verification, refresh, or networking occur in this module.
//!
//! The trusted host must construct the source only after explicit user consent.
//! Each load performs one auth-only acquisition; Codex may still proactively
//! refresh internally even when refreshToken=false. External support for the
//! deprecated token-output API remains unestablished, so no live source is wired.
//!
//! Account ID and expiry are parsed from the same returned access token. Parsing
//! is bounded and consistency-only; trust in the bearer comes from the authorized
//! source, not from the decoded JWT claims.
use crate::provider as p;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_TOKEN_BYTES: usize = 16_384;
const MAX_ACCOUNT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodexAuthMethod { Chatgpt, Unsupported }

/// Typed subset of getAuthStatus. Only exact upstream `chatgpt` maps to Chatgpt.
pub(crate) struct AuthStatusSnapshot {
    pub(crate) auth_method: Option<CodexAuthMethod>,
    pub(crate) auth_token: Option<String>,
}

/// Maximum time allowed for one auth-only acquisition.
const AUTH_STATUS_ACQUISITION_BUDGET: std::time::Duration =
    std::time::Duration::from_secs(5);

/// Trusted source, constructed only after explicit user authorization. A real
/// implementation must request includeToken=true, refreshToken=false once per
/// load and enforce `timeout` internally. On timeout it must cancel the request
/// and synchronously clean up before returning `Failure::Timeout`.
///
/// For a process-backed source, terminate and reap the child before returning;
/// never detach a worker or abandon a child. The synchronous CredentialSource
/// API cannot preempt a nonconforming blocking implementation. This source-side
/// timeout/cleanup guarantee is mandatory. The adapter also detects a late return
/// defensively, but that post-return check is not itself a hard timeout. Codex may
/// proactively refresh internally even when refreshToken=false.
pub(crate) trait AuthorizedAuthStatusSource: Send + Sync {
    fn acquire_bounded(
        &self,
        timeout: std::time::Duration,
    ) -> Result<AuthStatusSnapshot, p::Failure>;
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
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES
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
    if payload.len() > 16_384 { return Err(p::Failure::InvalidCredential); }
    let claims: AccessClaims = serde_json::from_slice(&payload).map_err(|_| p::Failure::InvalidCredential)?;
    let expiry = claims.exp.and_then(|v| u64::try_from(v).ok()).filter(|v| *v > 0)
        .ok_or(p::Failure::InvalidCredential)?;
    let account = claims.auth.and_then(|a| a.chatgpt_account_id).ok_or(p::Failure::AccountMismatch)?;
    if account.is_empty() || account.len() > MAX_ACCOUNT_BYTES
        || !account.bytes().all(|b| b.is_ascii_graphic())
    { return Err(p::Failure::AccountMismatch); }
    Ok((account, expiry))
}

fn account_digest(account: &str) -> String { hex::encode(Sha256::digest(account.as_bytes())) }


/// Process-backed implementation for a fixed, explicitly authorized helper.
/// This type is not selected by production code and is exercised only with
/// synthetic `/bin/sh` fixtures. Never accept its executable/argv from model data.
#[cfg(unix)]
pub(crate) struct ProcessAuthStatusSource {
    executable: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
    timeout_cap: std::time::Duration,
}

#[cfg(unix)]
impl ProcessAuthStatusSource {
    pub(crate) fn new(executable: std::path::PathBuf, args: Vec<std::ffi::OsString>) -> Self {
        Self {
            executable,
            args,
            timeout_cap: AUTH_STATUS_ACQUISITION_BUDGET,
        }
    }

    /// Lower per-source deadline for tighter trusted host bounds.
    /// Production acquisition remains unwired; zero is rejected by the supervisor.
    pub(crate) fn with_timeout_cap(
        executable: std::path::PathBuf,
        args: Vec<std::ffi::OsString>,
        timeout_cap: std::time::Duration,
    ) -> Self {
        Self { executable, args, timeout_cap }
    }
}

#[cfg(unix)]
impl AuthorizedAuthStatusSource for ProcessAuthStatusSource {
    fn acquire_bounded(
        &self,
        timeout: std::time::Duration,
    ) -> Result<AuthStatusSnapshot, p::Failure> {
        use std::process::Command;
        let timeout = timeout.min(AUTH_STATUS_ACQUISITION_BUDGET).min(self.timeout_cap);
        let mut command = Command::new(&self.executable);
        command.args(&self.args);
        let output = crate::auth::codex_process::run_bounded(
            &mut command,
            crate::auth::codex_process::Limits {
                timeout,
                stdout_bytes: 32 * 1024,
                stderr_bytes: 8 * 1024,
            },
        )
        .map_err(|error| match error {
            crate::auth::codex_process::Error::Timeout
            | crate::auth::codex_process::Error::CleanupTimeout => p::Failure::Timeout,
            crate::auth::codex_process::Error::StdoutLimit
            | crate::auth::codex_process::Error::StderrLimit
            | crate::auth::codex_process::Error::InvalidLimits => p::Failure::Limit,
            crate::auth::codex_process::Error::Spawn
            | crate::auth::codex_process::Error::Io
            | crate::auth::codex_process::Error::UnsuccessfulExit => {
                p::Failure::CredentialMissing
            }
        })?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireStatus {
            auth_method: Option<String>,
            auth_token: Option<String>,
        }
        // No child output or parser details enter error messages.
        let wire: WireStatus = serde_json::from_slice(&output.stdout)
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
        let started = std::time::Instant::now();
        let status = self.source.acquire_bounded(AUTH_STATUS_ACQUISITION_BUDGET)?;
        if started.elapsed() > AUTH_STATUS_ACQUISITION_BUDGET {
            return Err(p::Failure::Timeout);
        }
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
