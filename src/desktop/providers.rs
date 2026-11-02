//! Workspace-scoped provider configuration. Secrets are never projected into View.
use super::journal::{self, Journal, Result};
use ai_personas::provider::{AccessMode, DirectSettings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProvider { OpenAi, OpenRouter, Anthropic, OpenCode }
impl ApiProvider {
    pub const ALL: [Self; 4] = [Self::OpenAi, Self::OpenRouter, Self::Anthropic, Self::OpenCode];
    pub fn label(self) -> &'static str { match self {
        Self::OpenAi => "OpenAI", Self::OpenRouter => "OpenRouter", Self::Anthropic => "Anthropic / Claude", Self::OpenCode => "OpenCode Console (Zen)",
    }}
    pub fn mode(self) -> AccessMode { match self {
        Self::OpenAi => AccessMode::PlatformApiKey, Self::OpenRouter => AccessMode::OpenRouterApiKey,
        Self::Anthropic => AccessMode::AnthropicApiKey, Self::OpenCode => AccessMode::OpenCodeApiKey,
    }}
    fn models_url(self) -> &'static str { match self {
        Self::OpenAi => "https://api.openai.com/v1/models",
        Self::OpenRouter => "https://openrouter.ai/api/v1/models",
        Self::Anthropic => "https://api.anthropic.com/v1/models",
        Self::OpenCode => "https://opencode.ai/inference/v1/models",
    }}
    fn prefix(self) -> String { format!("PERSONAS_{}", self.mode().destination().to_ascii_uppercase()) }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub model: String,
    pub effort: String,
    pub max_output_tokens: Option<u64>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub selected: Option<ApiProvider>,
    pub defaults: BTreeMap<ApiProvider, Selection>,
}
// Intentionally no Debug implementation and no place in the public UI state.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials { keys: BTreeMap<ApiProvider, String> }
impl Credentials {
    pub fn load(j: &Journal) -> Result<Self> {
        j.read("provider-credentials.json")?.map(|raw| serde_json::from_slice(&raw)
            .map_err(|_| "The private provider credential file is invalid.".into())).unwrap_or_else(|| Ok(Self::default()))
    }
    pub fn configured(&self) -> BTreeSet<ApiProvider> { self.keys.keys().copied().collect() }
    pub fn key(&self, provider: ApiProvider) -> Result<String> {
        self.keys.get(&provider).cloned().ok_or_else(|| format!("Add the {} API key in Settings → Providers.", provider.label()))
    }
    pub fn save_key(&mut self, j: &Journal, provider: ApiProvider, key: String) -> Result<()> {
        journal::require(!key.is_empty() && key.bytes().all(|c| c.is_ascii_graphic()), "Enter a nonempty API key without spaces.")?;
        let mut next = self.keys.clone(); next.insert(provider, key);
        j.write("provider-credentials.json", &Self { keys:next.clone() })?;
        self.keys = next;
        Ok(())
    }
    pub fn remove(&mut self, j: &Journal, provider: ApiProvider) -> Result<()> {
        let mut next = self.keys.clone(); next.remove(&provider);
        j.write("provider-credentials.json", &Self { keys:next.clone() })?;
        self.keys = next;
        Ok(())
    }
    fn binding(provider: ApiProvider, key: &str) -> String {
        // This is a local credential fingerprint, never a claimed provider account ID.
        format!("desktop-key:{}:{}", provider.mode().destination(), hex::encode(Sha256::digest(key.as_bytes())))
    }
    pub fn settings(&self, provider: ApiProvider, selection: &Selection) -> Result<DirectSettings> {
        let binding = Self::binding(provider, &self.key(provider)?);
        let prefix = provider.prefix();
        let settings: DirectSettings = serde_json::from_value(json!({
            "mode":provider.mode(), "model":selection.model, "effort":selection.effort,
            "max_output_tokens":selection.max_output_tokens,
            "credentials":{"kind":"api_key","key_env":{"env":format!("{prefix}_API_KEY")},"account_env":{"env":format!("{prefix}_BINDING")}},
            "evidence":{"mode":provider.mode(),"model":selection.model,"efforts":[],
                "account_env":{"env":format!("{prefix}_BINDING")},"account_sha256":hex::encode(Sha256::digest(binding.as_bytes())),
                "transport_profile":provider.mode().profile()}
        })).map_err(|_| "Provider settings could not be constructed.".to_owned())?;
        settings.validate().map_err(|e| e.to_string())?;
        Ok(settings)
    }
    pub fn environment(&self, required: &[DirectSettings]) -> Result<BTreeMap<String, String>> {
        let mut env = BTreeMap::new();
        for settings in required {
            let provider = ApiProvider::ALL.into_iter().find(|p| p.mode() == settings.mode).ok_or("The workspace uses an unsupported credential route.")?;
            let key = self.key(provider)?;
            let binding = Self::binding(provider, &key);
            journal::require(settings.evidence.account_sha256 == hex::encode(Sha256::digest(binding.as_bytes())),
                "The saved API key differs from this workspace's current binding. Apply the provider connection in Settings, or restore the bound key.")?;
            let expected = self.settings(provider, &Selection { model:settings.model.clone(), effort:settings.effort.clone(), max_output_tokens:settings.max_output_tokens })?;
            journal::require(serde_json::to_value(&expected.credentials).ok() == serde_json::to_value(&settings.credentials).ok(), "The workspace uses different credential references.")?;
            env.insert(format!("{}_API_KEY", provider.prefix()), key);
            env.insert(format!("{}_BINDING", provider.prefix()), binding);
        }
        Ok(env)
    }
}
impl Configuration {
    pub fn load(j: &Journal) -> Result<Self> {
        j.read("provider-settings.json")?.map(|raw| serde_json::from_slice(&raw)
            .map_err(|_| "The provider settings file is invalid.".into())).unwrap_or_else(|| Ok(Self::default()))
    }
    pub fn save(&self, j: &Journal) -> Result<()> { j.write("provider-settings.json", self) }
}

/// Metadata only; does not generate tokens or treat model listing as inference verification.
pub fn catalogue(provider: ApiProvider, key: String, cancel: super::helpers::Cancel) -> Result<Value> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(journal::io)?;
    runtime.block_on(async move {
      tokio::select! {
        biased;
        _ = async { while !cancel.load(std::sync::atomic::Ordering::Acquire) { tokio::time::sleep(std::time::Duration::from_millis(50)).await; } } => Err("Model loading cancelled.".into()),
        result = async {
        let client = reqwest::Client::builder().https_only(true).no_proxy()
            .redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never())
            .build().map_err(|_| "Could not initialize provider connection.")?;
        let mut models = Vec::new();
        if provider == ApiProvider::OpenRouter {
            // The model catalogue is public; verify this key separately.
            let response = client.get("https://openrouter.ai/api/v1/key").bearer_auth(&key)
                .send().await.map_err(|_| "Could not check the OpenRouter key.")?;
            journal::require(response.status().is_success(), &format!("OpenRouter key verification returned HTTP {}.", response.status().as_u16()))?;
        }
        let mut after = None;
        let mut seen = BTreeSet::new();
        loop {
            let mut request = client.get(provider.models_url());
            if provider == ApiProvider::Anthropic {
                request = request.header("x-api-key", &key).header("anthropic-version", "2026-10-21");
                if let Some(cursor) = &after { request = request.query(&[("after_id", cursor)]); }
            } else { request = request.bearer_auth(&key); }
            let response = request.send().await.map_err(|_| format!("Could not reach {}. Check your connection or use manual model entry.", provider.label()))?;
            journal::require(response.status().is_success(), &format!("{} model discovery returned HTTP {}. Check the API key and account access.", provider.label(), response.status().as_u16()))?;
            let bytes = response.bytes().await.map_err(|_| "Could not read the model catalogue.")?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| "The provider returned an invalid model catalogue.")?;
            let data = value["data"].as_array().ok_or("The provider catalogue is missing its model list.")?;
            for m in data {
                let id = m["id"].as_str().filter(|s| !s.is_empty() && !s.chars().any(char::is_whitespace)).ok_or("The provider returned an invalid model ID.")?;
                models.push(json!({"slug":id,"display_name":m["display_name"].as_str().or(m["name"].as_str()).unwrap_or(id),
                    "description":m["description"].as_str().unwrap_or("Availability is listed by the provider; inference compatibility is checked when used."),
                    "default_effort":"default","efforts":[],"max_output_tokens":m["max_tokens"]}));
            }
            if provider != ApiProvider::Anthropic || value["has_more"] != true { break; }
            let cursor = value["last_id"].as_str().ok_or("The provider catalogue omitted its next page cursor.")?.to_owned();
            journal::require(seen.insert(cursor.clone()), "The provider catalogue repeated a page.")?;
            after = Some(cursor);
        }
        models.sort_by(|a,b| a["slug"].as_str().cmp(&b["slug"].as_str()));
        Ok(json!({"models":models,"provider":provider,"inference_verified":false}))
        } => result,
      }
    })
}
