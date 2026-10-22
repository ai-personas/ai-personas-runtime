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
    // JsonLimits fields are historical metadata; the shared parser validates
    // complete JSON and duplicate keys without application parsing quotas.
    crate::files::parse_json_unique(bytes, &crate::files::JsonLimits {
        max_source_bytes: 0, max_depth: 0, max_nodes: 0, max_decoded_bytes: 0,
    }).map_err(|_| p::Failure::InvalidCredential)
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
