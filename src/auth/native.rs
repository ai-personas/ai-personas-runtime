//! Selected-vault credentials for the fixed native launch entry only.
use super::{AuthError, Clock, LockedVault, RuntimeHandoff, SystemClock, Vault};
use crate::provider as p;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) struct VaultCredentialSource {
    guard: LockedVault,
    expires_at_unix: u64,
    account_sha256: String,
}

fn expiry(reference: &p::CredentialReference) -> Result<u64, p::Failure> {
    match reference {
        p::CredentialReference::OAuth {
            grant: p::OAuthGrant::SiwcTokenSharing,
            token_env,
            account_env,
            expires_at_unix,
        } if token_env.env == super::ACCESS_TOKEN_ENV
            && account_env.env == super::ACCOUNT_BINDING_ENV =>
        {
            Ok(*expires_at_unix)
        }
        _ => Err(p::Failure::GrantMismatch),
    }
}

impl VaultCredentialSource {
    /// The caller must already own execution of the selected Store.
    pub(crate) fn open(path: &Path, settings: &p::DirectSettings) -> Result<Self, p::Failure> {
        settings.validate().map_err(|_| p::Failure::Configuration)?;
        if settings.mode != p::AccessMode::SiwcOAuth
            || settings
                .evidence
                .account_env
                .environment()
                .is_none_or(|account| account.env != super::ACCOUNT_BINDING_ENV)
        {
            return Err(p::Failure::GrantMismatch);
        }
        let expires_at_unix = expiry(&settings.credentials)?;
        if SystemClock.now_unix() >= expires_at_unix {
            return Err(p::Failure::CredentialExpired);
        }
        if !path.is_absolute() {
            return Err(p::Failure::Configuration);
        }
        let guard = Vault::open(path)
            .and_then(|vault| vault.lock())
            .map_err(|_| p::Failure::CredentialMissing)?;
        let source = Self {
            guard,
            expires_at_unix,
            account_sha256: settings.evidence.account_sha256.clone(),
        };
        source.check()?;
        Ok(source)
    }

    fn handoff(&self) -> Result<RuntimeHandoff, p::Failure> {
        // Every read uses the retained lock and the record's current-source fence.
        let record = self
            .guard
            .load()
            .map_err(|_| p::Failure::InvalidCredential)?
            .ok_or(p::Failure::CredentialMissing)?;
        if record.expires_at_unix() != self.expires_at_unix {
            return Err(p::Failure::InvalidCredential);
        }
        if hex::encode(Sha256::digest(record.account_binding().as_bytes())) != self.account_sha256 {
            return Err(p::Failure::AccountMismatch);
        }
        record.handoff(&SystemClock).map_err(|error| match error {
            AuthError::Expired => p::Failure::CredentialExpired,
            _ => p::Failure::InvalidCredential,
        })
    }

    pub(crate) fn check(&self) -> Result<(), p::Failure> {
        self.handoff().map(|_| ())
    }
}

impl p::CredentialSource for VaultCredentialSource {
    fn load(
        &self,
        reference: &p::CredentialReference,
        evidence_account: &p::SecretReference,
    ) -> Result<p::CredentialBundle, p::Failure> {
        if expiry(reference)? != self.expires_at_unix
            || evidence_account.env != super::ACCOUNT_BINDING_ENV
        {
            return Err(p::Failure::Configuration);
        }
        // Parent-private handoff fields become provider secrets only after all
        // record, refresh-fence, current-source, account and expiry checks.
        let handoff = self.handoff()?;
        let account = p::Secret::new(handoff.account_binding);
        Ok(p::CredentialBundle {
            credential: p::Credential::OAuth {
                grant: p::OAuthGrant::SiwcTokenSharing,
                bearer: p::Secret::new(handoff.access_token),
                account: account.clone(),
                expires_at_unix: self.expires_at_unix,
            },
            evidence_account: account,
        })
    }
}
