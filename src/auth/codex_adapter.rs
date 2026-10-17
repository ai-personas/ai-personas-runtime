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

    /// Lower per-source deadline for deterministic tests or tighter trusted hosts.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct FixtureSource {
        status: Mutex<Option<Result<AuthStatusSnapshot, p::Failure>>>,
        calls: AtomicUsize,
        budget: Mutex<Option<std::time::Duration>>,
        cleanup_complete: std::sync::atomic::AtomicBool,
    }
    impl FixtureSource {
        fn new(method: Option<CodexAuthMethod>, token: Option<String>) -> Self {
            Self {
                status: Mutex::new(Some(Ok(AuthStatusSnapshot { auth_method: method, auth_token: token }))),
                calls: AtomicUsize::new(0),
                budget: Mutex::new(None),
                cleanup_complete: std::sync::atomic::AtomicBool::new(false),
            }
        }
        fn timeout() -> Self {
            Self {
                status: Mutex::new(Some(Err(p::Failure::Timeout))),
                calls: AtomicUsize::new(0),
                budget: Mutex::new(None),
                cleanup_complete: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }
    impl AuthorizedAuthStatusSource for FixtureSource {
        fn acquire_bounded(&self, timeout: std::time::Duration) -> Result<AuthStatusSnapshot, p::Failure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.budget.lock().unwrap() = Some(timeout);
            let result = self.status.lock().unwrap().take().ok_or(p::Failure::CredentialMissing)?;
            if matches!(&result, Err(p::Failure::Timeout)) {
                // Synthetic source contract: cancellation/resource cleanup has
                // completed before the timeout result is returned.
                self.cleanup_complete.store(true, Ordering::SeqCst);
            }
            result
        }
    }
    pub(super) fn token(account: &str, exp: i64) -> String {
        let h = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&serde_json::json!({
            "exp": exp, "https://api.openai.com/auth": {"chatgpt_account_id": account}
        })).unwrap());
        format!("{h}.{payload}.synthetic-signature")
    }
    pub(super) fn reference() -> p::CredentialReference {
        p::CredentialReference::OAuth { grant: p::OAuthGrant::TraditionalChatgpt,
            token_env: p::SecretReference { env: "SYNTHETIC_CODEX_TOKEN".into() },
            account_env: p::SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() },
            expires_at_unix: 500 }
    }
    fn source(method: Option<CodexAuthMethod>, value: Option<String>, expected: &str) -> CodexCredentialSource<FixtureSource> {
        CodexCredentialSource::new(FixtureSource::new(method, value), account_digest(expected)).unwrap()
    }
    fn load(s: &impl p::CredentialSource) -> Result<p::CredentialBundle, p::Failure> {
        s.load(&reference(), &p::SecretReference { env: "SYNTHETIC_CODEX_ACCOUNT".into() })
    }
    #[test]
    fn valid_synthetic_snapshot_maps_to_legacy_provider_bundle() {
        let bearer = token("synthetic-account", 500);
        let s = source(Some(CodexAuthMethod::Chatgpt), Some(bearer.clone()), "synthetic-account");
        let bundle = load(&s).unwrap();
        match bundle.credential {
            p::Credential::OAuth { grant, bearer: got, account, expires_at_unix } => {
                assert_eq!(grant, p::OAuthGrant::TraditionalChatgpt);
                assert_eq!(got.expose(), bearer);
                assert_eq!(account.expose(), "synthetic-account");
                assert_eq!(expires_at_unix, 500);
                assert_eq!(bundle.evidence_account.expose(), "synthetic-account");
            },
            _ => panic!("expected OAuth bundle"),
        }
    }
    #[test]
    fn missing_or_unsupported_method_and_missing_token_fail_closed() {
        assert_eq!(load(&source(None, Some(token("synthetic-account", 500)), "synthetic-account")).unwrap_err(), p::Failure::GrantMismatch);
        assert_eq!(load(&source(Some(CodexAuthMethod::Unsupported), Some(token("synthetic-account", 500)), "synthetic-account")).unwrap_err(), p::Failure::GrantMismatch);
        assert_eq!(load(&source(Some(CodexAuthMethod::Chatgpt), None, "synthetic-account")).unwrap_err(), p::Failure::CredentialMissing);
    }
    #[test]
    fn malformed_missing_expiry_and_expired_token_fail_closed() {
        assert_eq!(load(&source(Some(CodexAuthMethod::Chatgpt), Some("not-a-jwt".into()), "synthetic-account")).unwrap_err(), p::Failure::InvalidCredential);
        let h = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"synthetic-account"}}"#);
        assert_eq!(load(&source(Some(CodexAuthMethod::Chatgpt), Some(format!("{h}.{payload}.sig")), "synthetic-account")).unwrap_err(), p::Failure::InvalidCredential);
        // Parsing carries the token's exp claim; only DirectProvider's injected
        // clock can determine expiry. The wired provider tests below cover it.
        let bundle = load(&source(Some(CodexAuthMethod::Chatgpt), Some(token("synthetic-account", 500)), "synthetic-account")).unwrap();
        assert!(matches!(bundle.credential, p::Credential::OAuth { expires_at_unix: 500, .. }));
    }
    #[test]
    fn account_switch_fails_against_saved_capability_evidence() {
        assert_eq!(load(&source(Some(CodexAuthMethod::Chatgpt), Some(token("other-account", 500)), "synthetic-account")).unwrap_err(), p::Failure::AccountMismatch);
    }
    #[test]
    fn constructor_requires_lowercase_digest_like_direct_settings() {
        assert!(CodexCredentialSource::new(
            FixtureSource::new(Some(CodexAuthMethod::Chatgpt), Some(token("synthetic-account", 500))),
            "A".repeat(64),
        ).is_err());
        assert!(CodexCredentialSource::new(
            FixtureSource::new(Some(CodexAuthMethod::Chatgpt), Some(token("synthetic-account", 500))),
            account_digest("synthetic-account"),
        ).is_ok());
    }

    #[test]
    fn bounded_source_timeout_is_propagated_with_five_second_budget() {
        let fixture = FixtureSource::timeout();
        let s = CodexCredentialSource::new(fixture, account_digest("synthetic-account")).unwrap();
        assert_eq!(load(&s).unwrap_err(), p::Failure::Timeout);
        assert_eq!(s.source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(*s.source.budget.lock().unwrap(), Some(std::time::Duration::from_secs(5)));
        assert!(s.source.cleanup_complete.load(Ordering::SeqCst));
    }

    fn ephemeral_reference(expiry: u64) -> p::CredentialReference {
        p::CredentialReference::EphemeralAuthOnly {
            grant: p::OAuthGrant::TraditionalChatgpt,
            expires_at_unix: expiry,
        }
    }

    #[test]
    fn ephemeral_load_uses_saved_fingerprint_without_environment_names() {
        let bearer = token("synthetic-account", 900);
        let s = source(Some(CodexAuthMethod::Chatgpt), Some(bearer.clone()), "synthetic-account");
        let bundle = p::CredentialSource::load_ephemeral(
            &s,
            &ephemeral_reference(500),
            &account_digest("synthetic-account"),
        ).unwrap();
        match bundle.credential {
            p::Credential::OAuth { grant, bearer: got, account, expires_at_unix } => {
                assert_eq!(grant, p::OAuthGrant::TraditionalChatgpt);
                assert_eq!(got.expose(), bearer);
                assert_eq!(account.expose(), "synthetic-account");
                // The fresh token's expiry is used; the saved snapshot need not match.
                assert_eq!(expires_at_unix, 900);
            }
            _ => panic!("expected OAuth bundle"),
        }
        assert_eq!(s.source.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ephemeral_load_rejects_fingerprint_mismatch_before_acquisition() {
        let s = source(
            Some(CodexAuthMethod::Chatgpt),
            Some(token("synthetic-account", 900)),
            "synthetic-account",
        );
        let result = p::CredentialSource::load_ephemeral(
            &s,
            &ephemeral_reference(500),
            &account_digest("different-account"),
        );
        assert!(matches!(result, Err(p::Failure::AccountMismatch)));
        assert_eq!(s.source.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn legacy_oauth_allows_distinct_nonempty_reference_names() {
        use p::CredentialSource as _;
        let s = source(
            Some(CodexAuthMethod::Chatgpt),
            Some(token("synthetic-account", 900)),
            "synthetic-account",
        );
        let reference = p::CredentialReference::OAuth {
            grant: p::OAuthGrant::TraditionalChatgpt,
            token_env: p::SecretReference { env: "LEGACY_TOKEN_NAME".into() },
            account_env: p::SecretReference { env: "LEGACY_CREDENTIAL_ACCOUNT".into() },
            expires_at_unix: 900,
        };
        let result = s.load(
            &reference,
            &p::SecretReference { env: "LEGACY_EVIDENCE_ACCOUNT".into() },
        );
        assert!(result.is_ok());
        assert_eq!(s.source.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn one_load_makes_at_most_one_acquisition_and_does_not_retry() {
        let fixture = FixtureSource::new(Some(CodexAuthMethod::Chatgpt), Some(token("synthetic-account", 500)));
        let s = CodexCredentialSource::new(fixture, account_digest("synthetic-account")).unwrap();
        assert!(load(&s).is_ok());
        assert_eq!(s.source.calls.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
#[path = "codex_adapter_tests.rs"]
mod direct_provider_tests;
