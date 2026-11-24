//! Explicit connection changes for an existing workspace, without inference.
use super::*;
use ai_personas::{launch, provider::DirectSettings};

const RECORD: &str = "provider-change.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    request: launch::ProviderMigrationRequest,
    selected: Option<ApiProvider>,
    codex: Option<CodexPaths>,
    completed: bool,
}

impl Controller {
    fn saved_provider_change(&self) -> Result<Option<Change>> {
        self.j.read(RECORD)?.map(|bytes| serde_json::from_slice(&bytes)
            .map_err(|_| "The saved provider change is invalid.".into())).transpose()
    }

    pub(super) fn provider_change_pending(&self) -> Result<bool> {
        Ok(self.saved_provider_change()?.is_some_and(|change| !change.completed))
    }

    pub(super) fn change_provider(&mut self, selected: Option<ApiProvider>, selection: Selection, preserve_models: bool, effort_override: Option<String>) -> Result<()> {
        journal::require(self.run.is_none() && self.login.is_none(), "Pause work and finish sign-in before applying a connection.")?;
        journal::require(!self.provider_change_pending()?, "Finish the saved provider change first.")?;
        // A prior build may have left a completed response's local lease when
        // Pause won the adoption race. Settle only proven stopped rejections
        // before taking exact migration heads; no inference or resumption.
        self.h.call("ai-personas", &self.session_args(vec!["retire-stopped-invocations".into()])?)?;
        let snapshot: launch::MigrationSnapshot = serde_json::from_value(self.h.call("ai-personas",
            &self.session_args(vec!["provider-migration-snapshot".into()])?)?)
            .map_err(|_| "The current provider bindings are unavailable.".to_owned())?;
        let codex = if selected.is_none() { Some(self.view.codex.clone().ok_or("Locate Codex in Account settings first.")?) } else { None };
        let settings: DirectSettings = if let Some(provider) = selected {
            self.credentials.settings(provider, &selection)?
        } else {
            let binding = snapshot.persona_bindings.values().next().ok_or("The workspace has no persona model bindings.")?;
            let mut args = vec!["configure-codex".into()];
            args.extend(helpers::codex_args(codex.as_ref().unwrap()));
            args.extend(["--model".into(), binding.requested_model.clone(), "--effort".into(), effort_override.as_ref().unwrap_or(&binding.requested_effort).clone()]);
            serde_json::from_value(self.h.call("personas-provider", &args)?)
                .map_err(|_| "Codex connection settings are unavailable.".to_owned())?
        };
        let change = Change {
            request: launch::ProviderMigrationRequest {
                id: journal::id("provider-change")?, operator: snapshot.operator,
                reason: "Connection selected explicitly in desktop Settings.".into(),
                expected_runtime: snapshot.runtime, expected_launch: snapshot.launch,
                expected_persona_bindings: snapshot.persona_bindings, provider: settings,
                preserve_persona_models: preserve_models || selected.is_none(),
                persona_effort_override: effort_override,
                repair_episode_bindings_from: None, allow_direct_provider: true, model_confirmed: true,
                authorized_destination: snapshot.destination,
            }, selected, codex, completed: false,
        };
        self.j.write("provider-change-request.json", &change.request)?;
        self.h.call("ai-personas", &self.session_args(vec!["migrate-provider".into(),
            self.j.root.join("provider-change-request.json").to_string_lossy().into_owned(), "--preview".into()])?)?;
        // Retain the exact request before applying it. A retry completes this
        // operation's receipt; it cannot silently rebind a newer workspace.
        self.j.write(RECORD, &change)?;
        self.finish_provider_change()
    }

    pub(super) fn finish_provider_change(&mut self) -> Result<()> {
        journal::require(self.run.is_none() && self.login.is_none(), "Pause work and finish sign-in before applying a connection.")?;
        let mut change = self.saved_provider_change()?.filter(|change| !change.completed)
            .ok_or("There is no unfinished provider change.")?;
        self.j.write("provider-change-request.json", &change.request)?;
        let result: launch::ProviderMigrationOutcome = serde_json::from_value(self.h.call("ai-personas",
            &self.session_args(vec!["migrate-provider".into(), self.j.root.join("provider-change-request.json").to_string_lossy().into_owned()])?)?)
            .map_err(|_| "The provider change receipt is unavailable; retry this saved change.".to_owned())?;
        journal::require(result.receipt.id == change.request.id
            && result.receipt.target_mode == change.request.provider.mode,
            "The provider change receipt does not match the saved request.")?;
        self.j.keep("settings", &serde_json::to_value(&change.request.provider).map_err(|_| "Invalid provider settings.")?)?;
        self.j.data.codex_auth = change.codex.clone();
        self.j.data.vault = None;
        self.j.save()?;
        self.provider_config.selected = change.selected;
        self.provider_config.save(&self.j)?;
        self.view.native = false;
        if let Some(codex) = change.codex.clone() { self.view.codex = Some(codex); }
        self.view.account = Value::Null;
        self.view.models = Value::Null;
        change.completed = true;
        self.j.write(RECORD, &change)?;
        self.refresh()?;
        self.events.send(Event::Notice("Connection applied. Existing work and persona history are retained; no inference was started.".into()));
        self.metadata()
    }
}
