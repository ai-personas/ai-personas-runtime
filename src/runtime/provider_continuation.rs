//! Persona-selected provider continuation. State holds an exact terminal
//! reference; complete prior inputs and opaque outputs stay host-private.
use super::*;

pub(super) const INTERFACE: &str = "selected-provider-context-v1";
const RULE: &str = "Provider continuation is optional supporting context. To carry this invocation's complete provider input and completed output into the next decision, add provider_context:{kind:current,lifetime:work} to a next_assets replacement. Use lifetime:next_call for one following admission of this persona on this work; calls on other work do not consume it. This includes any earlier provider history actually supplied to this invocation, its old text and images, and opaque reasoning items when returned. It can be large; select it only when useful. To keep the existing provider turn while replacing files or images, use provider_context:{kind:preserve}. That preserves the old turn without appending this invocation or renewing its lifetime. Omitting provider_context from a replacement clears that selection; next_assets clear clears text, images and provider history, while next_assets preserve retains surviving selections without appending a turn. To select only provider context use kind:replace, references:[], provider_context:{kind:current}, and your reason. No full history or item bodies are placed in this metadata. supplied_history describes the actual extra provider input for this admission, with complete serialized bytes rather than an invented token estimate. unavailable means no earlier provider items were supplied; choose current to start a new continuation from this actual input, or clear it. Current requires the declared interface and a completed direct provider response; thin or unavailable output remains explicit without inventing history. The selected model and transport route must remain compatible with the retained turn. Current character, the latest CONTROL and schema, current request and observed effects govern the new response. Earlier assistant messages are historical proposals, including any fenced operations, not new actions, current authority or proof of successful effects. Character adoption can preserve or clear an existing selection but cannot add this turn. Retention or later item supply alone proves neither rendered reasoning reuse nor better work or character maturation.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selected {
    pub invocation: String,
    pub terminal: s::ObjectRef,
    pub persona: String,
    pub work: String,
    pub model: String,
    pub character_revision: Option<String>,
    pub lifetime: String,
    /// Inclusive deadline in this persona/work's admission sequence.
    pub last_admission: Option<u64>,
}

impl Selected {
    pub(super) fn active(&self, serial: u64) -> bool {
        self.last_admission.is_none_or(|last| serial <= last)
    }
}

pub(super) fn rule_bytes() -> usize { RULE.len() }

#[derive(Default)]
pub(crate) struct Material {
    selected: Option<Selected>,
    history: Option<provider::InputHistory>,
    exposure: Option<Exposure>,
    unavailable: Option<String>,
}

impl Material {
    pub(crate) fn pending(store: &mut s::Store, state: &State, actor: &str, work: &str) -> Self {
        let selected = state.personas.get(actor).and_then(|p| p.focus.get(work))
            .and_then(|focus| focus.provider_context.as_ref());
        selected.map_or_else(Self::default, |selected| Self::load(store, selected, None))
    }

    fn load(store: &mut s::Store, selected: &Selected, current: Option<&Prepared>) -> Self {
        let loaded = (|| -> Result<(provider::InputHistory, Exposure)> {
            let invocation = store.trusted_host().invocation(&selected.invocation)?;
            let prepared = decode_prepared(&invocation)?;
            let is_current = current.is_some_and(|value| value.request.id == selected.invocation
                && value.prompt_sha256 == prepared.prompt_sha256);
            require(invocation.state == s::InvocationState::Adopted
                || (is_current && invocation.state == s::InvocationState::Responded),
                "selected provider turn is not an accepted response")?;
            require(prepared.request.profile == Profile::Direct
                && prepared.request.persona == selected.persona
                && prepared.request.work == selected.work
                && prepared.requested_model == selected.model
                && prepared.character_revision == selected.character_revision,
                "selected provider turn provenance differs")?;
            let terminal = store.trusted_host().read_terminal_evidence(
                &s::EntityId::Invocation(selected.invocation.clone()), &selected.terminal.id,
            )?.ok_or_else(|| held("selected provider terminal is unavailable"))?;
            let receipt = &terminal.body["receipt"];
            let response = terminal.body["response_body"]["text"].as_str()
                .ok_or_else(|| held("selected provider response body is unavailable"))?;
            let response_hash = s::sha256_bytes(response.as_bytes());
            require(terminal.exact_ref()? == selected.terminal
                && terminal.kind == "runtime-terminal-transport"
                && terminal.body["prepare_hash"] == invocation.prepare_hash
                && terminal.body["prepared_epoch"] == prepared.epoch
                && terminal.body["invocation_id"] == selected.invocation
                && terminal.body["prompt_sha256"] == prepared.prompt_sha256
                && invocation.response_hash.as_deref() == Some(response_hash.as_str())
                && receipt["inner_sha256"].as_str().or_else(|| receipt["transport_sha256"].as_str())
                    == Some(response_hash.as_str())
                && receipt["kind"] == "provider_complete" && receipt["synthetic"] == false
                && receipt["completion_disposition"] == "ready"
                && receipt["attempt"]["response_completed"] == true
                && receipt["attempt"]["stream_eof"] == true,
                "selected provider terminal binding is unavailable")?;
            let audit = &receipt["audit"];
            // The canonical provider serializer owns supported wire profiles.
            // A second version list here can reject an otherwise exact current
            // request when the carrier changes.
            require(prepared.provider_preparation.as_ref() == Some(audit)
                && audit["invocation_id"] == selected.invocation
                && provider::response_only_request_matches(audit, &prepared.requested_model,
                    &prepared.requested_effort, &prepared.prompt, prepared.reusable_prefix_bytes,
                    &prepared.images, prepared.input_history.as_ref(), &prepared.output_schema),
                "selected provider input cannot be reconstructed exactly")?;
            let output = &terminal.body["provider_output_items"];
            require(provider::retained_output_metadata(output).as_ref()
                == Some(&receipt["attempt"]["output_items"]),
                "selected provider output snapshots are unavailable")?;
            let history = provider::InputHistory::from_completed(prepared.input_history.as_ref(),
                &selected.invocation, &selected.model, audit["transport_profile"].as_str().unwrap(), &prepared.prompt,
                prepared.reusable_prefix_bytes, &prepared.images, output)
                .map_err(|_| held("selected provider turn lacks a complete replayable output sequence"))?;
            Ok((history, prepared_exposure(&prepared)?.clone()))
        })();
        match loaded {
            Ok((history, exposure)) => Self { selected: Some(selected.clone()), history: Some(history),
                exposure: Some(exposure), unavailable: None },
            Err(_) => Self { selected: Some(selected.clone()),
                unavailable: Some("The exact complete selected provider turn is unavailable or incompatible; no prior items are supplied.".into()),
                ..Self::default() },
        }
    }

    pub(super) fn project(
        &self,
        state: &State,
        focus: &Focus,
        actor: &str,
        work: &str,
        model: &str,
        serial: u64,
        audience: &f::Admission,
        exposure: &mut Exposure,
    ) -> Result<(Value, Option<provider::InputHistory>)> {
        let selected = focus.provider_context.as_ref();
        let active = selected.filter(|selected| selected.active(serial));
        let mut unavailable = None;
        let mut history = None;
        let rejection = focus.provider_context_recoveries.iter().rev().find(|pending|
            active.is_some() && pending.rejected_provider_context.as_ref() == active);
        if let Some(selected) = active {
            if rejection.is_some() {
                unavailable = Some("An actual provider rejection made this selected history unavailable for further supply. The authored selection is unchanged. Choose current to rebase on this fresh input, preserve the unavailable reference, or clear it.");
            } else if selected.persona != actor || selected.work != work || selected.model != model {
                unavailable = Some("Selected provider context belongs to a different persona, work or configured model.");
            } else if self.selected.as_ref() != Some(selected) {
                unavailable = Some("Selected provider context material was not loaded for this exact reference.");
            } else if let (Some(input), Some(inherited)) = (&self.history, &self.exposure) {
                if exposure_permitted(state, inherited, audience) && input.validate(model).is_ok() {
                    extend_exposure(exposure, inherited);
                    history = Some(input.clone());
                } else {
                    unavailable = Some("Selected provider context is unavailable under the current input binding.");
                }
            } else {
                unavailable = self.unavailable.as_deref();
            }
        }
        let metadata = history.as_ref().map(provider::InputHistory::metadata).transpose()
            .map_err(|_| held("provider history accounting unavailable"))?;
        Ok((json!({"interface":INTERFACE,"rule":RULE,"selected":selected,
            "active_this_admission":active.is_some(),
            "preserved_for_next_admission":selected.is_some_and(|selected| selected.last_admission.is_none_or(|last| serial < last)),
            "supplied_history":metadata,"unavailable":unavailable,
            "rejected_input":rejection.map(|pending| json!({"invocation":pending.invocation,"terminal":pending.terminal}))}), history))
    }
}

pub(super) fn select(
    store: &mut s::Store,
    prepared: &Prepared,
    intent: &p::SelectionIntent,
    old: Option<&Selected>,
    reduction_only: bool,
    material: &mut Material,
) -> Result<Option<Selected>> {
    let next_serial = increment(prepared.work_admission_serial)?;
    let surviving = old.filter(|selected| selected.active(next_serial)).cloned();
    if matches!(intent, p::SelectionIntent::Preserve { .. })
        || matches!(intent.provider_context(), Some(p::ProviderContextSelection::Preserve)) {
        return Ok(surviving);
    }
    let Some(p::ProviderContextSelection::Current { lifetime }) = intent.provider_context() else {
        return Ok(None);
    };
    require(!reduction_only, "character adoption cannot add a provider continuation turn")?;
    let control = request_context::decode(prepared)?.control;
    require(prepared.request.profile == Profile::Direct && prepared.request.mode == Mode::Ordinary
        && control["provider_continuation"]["interface"] == INTERFACE,
        "provider continuation selection was not offered in this admission")?;
    let terminal = store.trusted_host().read_terminal_evidence(
        &s::EntityId::Invocation(prepared.request.id.clone()), &terminal_id(&prepared.request.id),
    )?.ok_or_else(|| held("completed provider terminal is unavailable"))?;
    let lifetime = lifetime.as_deref().unwrap_or("work");
    require(matches!(lifetime, "work" | "next_call"), "invalid provider context lifetime")?;
    let selected = Selected { invocation: prepared.request.id.clone(), terminal: terminal.exact_ref()?,
        persona: prepared.request.persona.clone(), work: prepared.request.work.clone(),
        model: prepared.requested_model.clone(), character_revision: prepared.character_revision.clone(),
        lifetime: lifetime.into(), last_admission: (lifetime == "next_call").then_some(next_serial) };
    *material = Material::load(store, &selected, Some(prepared));
    Ok(Some(selected))
}
