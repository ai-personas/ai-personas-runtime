//! An operator-selected private OpenAI key file. Secrets never enter the SQLite
//! store, the workspace environment or the persona's request.
use super::{failure, Result};
use crate::provider::{
    self, AccessMode, CredentialReference, CredentialSource, Failure, Secret, SecretReference,
};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
const KEY: &str = "PERSONAS_OPENAI_API_KEY";
const BINDING: &str = "PERSONAS_OPENAI_BINDING";

pub(super) fn canonical(path: &Path, working_directory: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    if path.starts_with(working_directory.canonicalize()?) {
        return Err(failure("Keep the private key file outside the shared working directory"));
    }
    Ok(path)
}

pub(super) struct FileSource(pub PathBuf);
impl FileSource {
    fn key(&self) -> Result<String> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.0)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return Err(failure(
                "Choose a private key file owned by your account (permissions 0600)",
            ));
        }
        let mut raw = String::new();
        file.read_to_string(&mut raw)?;
        let value = raw.trim();
        let key = if value.starts_with('{') {
            let data: serde_json::Value =
                serde_json::from_str(value).map_err(|_| failure("Invalid private key file"))?;
            data["keys"]["open_ai"]
                .as_str()
                .ok_or_else(|| failure("No OpenAI key in this credential file"))?
                .to_owned()
        } else {
            value.to_owned()
        };
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(failure("Invalid OpenAI key file"));
        }
        Ok(key)
    }
}
fn binding(key: &str) -> String {
    format!(
        "studio-openai-key:{}",
        crate::store::sha256_bytes(key.as_bytes())
    )
}
impl CredentialSource for FileSource {
    fn load(
        &self,
        reference: &CredentialReference,
        evidence_account: &SecretReference,
    ) -> std::result::Result<provider::CredentialBundle, Failure> {
        match reference {
            CredentialReference::ApiKey {
                key_env,
                account_env,
            } if key_env.env == KEY
                && account_env.env == BINDING
                && evidence_account.env == BINDING =>
            {
                ()
            }
            _ => return Err(Failure::Configuration),
        }
        let key = self.key().map_err(|_| Failure::CredentialMissing)?;
        let account = binding(&key);
        Ok(provider::CredentialBundle {
            credential: provider::Credential::ApiKey {
                key: Secret::new(key),
                account: Secret::new(account.clone()),
            },
            evidence_account: Secret::new(account),
        })
    }
}
pub(super) fn settings(path: &Path, model: &str, effort: &str) -> Result<provider::DirectSettings> {
    let key = FileSource(path.into()).key()?;
    let mode = AccessMode::PlatformApiKey;
    let account = SecretReference {
        env: BINDING.into(),
    };
    let settings = provider::DirectSettings {
        mode,
        model: model.into(),
        effort: effort.into(),
        credentials: CredentialReference::ApiKey {
            key_env: SecretReference { env: KEY.into() },
            account_env: account.clone(),
        },
        evidence: provider::CapabilityEvidence {
            mode,
            model: model.into(),
            efforts: Default::default(),
            account_env: provider::AccountBindingReference::Environment(account),
            account_sha256: crate::store::sha256_bytes(binding(&key).as_bytes()),
            transport_profile: mode.profile().into(),
            catalog_evidence_sha256: None,
            structured_output_evidence_sha256: None,
        },
    };
    settings.validate()?;
    Ok(settings)
}
