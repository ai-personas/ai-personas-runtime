//! Persona-selected provider continuation. State holds an exact terminal
//! reference; selected prior inputs and complete opaque outputs stay host-private.
use super::*;

pub(super) const INTERFACE: &str = "selected-provider-context-v2";
const RULE: &str = "Provider continuation is optional supporting context. Choose only a scope in available_scopes. In a next_assets replacement, provider_context:{kind:current_output,lifetime:work} retains every completed output item from this invocation, including returned opaque reasoning and your Decision, without its original prompt, input images or separate earlier provider history. Select needed facts, observations and resources separately in native context. Use this when the next input supplies what is needed and repeating the old input is unnecessary. current_turn keeps this invocation's own complete input, images and output, excluding earlier separate provider history. current includes this turn and all earlier provider history actually supplied to it. All choices keep complete output items unchanged. Current qualifications on omitted inputs accompany output-only history in native context and can still be large. Select again to move the chosen scope forward. lifetime:next_call lasts for one following admission of this persona on this work; work is the default. provider_context:{kind:preserve} retains the existing selection without appending a turn or renewing its lifetime. Omitting provider_context from a replacement clears it; next_assets clear clears text, images and provider history; next_assets preserve retains surviving selections. To choose provider context alone, use kind:replace, references:[], provider_context and your reason. supplied_history reports extra input bytes, item counts and omitted input turns, never token estimates or opaque bodies. unavailable means no prior provider items were supplied; select an available scope again, preserve the unavailable reference or clear it. Missing output and incompatible model or transport remain unavailable. Current character, CONTROL, schema, human request and observed effects govern each decision. Earlier Decisions are historical proposals, not new operations or proof of effects. Character adoption can preserve or clear, but cannot add this turn. Retention or supply proves neither useful reasoning reuse, progress nor maturation.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selected {
    pub invocation: String,
    pub terminal: s::ObjectRef,
    pub persona: String,
    pub work: String,
    pub model: String,
    pub character_revision: Option<String>,
    /// An older exact selection includes the history supplied to its turn.
    /// New choices can explicitly keep one turn, or just its complete output.
    #[serde(default)]
    pub scope: HistoryScope,
    pub lifetime: String,
    /// Inclusive deadline in this persona/work's admission sequence.
    pub last_admission: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HistoryScope {
    #[default]
    SuppliedHistory,
    CurrentTurn,
    CurrentOutput,
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
    qualification_sources: Vec<f::Reference>,
    unavailable: Option<String>,
}

impl Material {
    pub(crate) fn pending(store: &mut s::Store, state: &State, actor: &str, work: &str) -> Self {
        let Some(persona) = state.personas.get(actor) else { return Self::default() };
        let Some(focus) = persona.focus.get(work) else { return Self::default() };
        let Some(selected) = focus.provider_context.as_ref() else { return Self::default() };
        // Reject known-inapplicable metadata before reading a retained invocation,
        // terminal, images or opaque output. Projection still owns the diagnostic
        // and rechecks the exact admission; the authored selection stays intact.
        // This serial is a conservative lower bound during response planning:
        // do not guess the next serial and drop a currently valid next_call turn.
        if !selected.active(focus.work_admission_serial)
            || selected.persona != actor || selected.work != work
            || selected.model != persona.seed.requested_model
            || focus.provider_context_recoveries.iter().any(|pending|
                pending.rejected_provider_context.as_ref() == Some(selected))
        {
            return Self { selected: Some(selected.clone()), ..Self::default() };
        }
        Self::load(store, state, selected, None)
    }

    fn load(store: &mut s::Store, state: &State, selected: &Selected, current: Option<&Prepared>) -> Self {
        let loaded = (|| -> Result<(provider::InputHistory, Exposure, Vec<f::Reference>)> {
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
            let previous = match selected.scope {
                HistoryScope::SuppliedHistory => prepared.input_history.as_ref(),
                HistoryScope::CurrentTurn | HistoryScope::CurrentOutput => None,
            };
            let history = provider::InputHistory::from_completed(previous,
                &selected.invocation, &selected.model, audit["transport_profile"].as_str().unwrap(), &prepared.prompt,
                prepared.reusable_prefix_bytes, &prepared.images, output,
                selected.scope != HistoryScope::CurrentOutput)
                .map_err(|_| held("selected provider turn lacks a complete replayable output sequence"))?;
            let mut qualification_sources = BTreeSet::new();
            for turn in history.input_omitted_turns() {
                if turn == &prepared.request.id {
                    qualification_sources.extend(history::own_decision_input_sources(
                        state, &prepared, &response_hash,
                        |id| Ok(store.trusted_host().invocation(id)?))?);
                } else {
                    let original = store.trusted_host().invocation(turn)?;
                    let input = decode_prepared(&original)?;
                    require(original.state == s::InvocationState::Adopted
                        && input.request.id == *turn && input.epoch < prepared.epoch
                        && input.request.persona == selected.persona
                        && input.request.work == selected.work && input.requested_model == selected.model,
                        "output-only provider input provenance differs")?;
                    qualification_sources.extend(history::own_decision_input_sources(
                        state, &input, original.response_hash.as_deref()
                            .ok_or_else(|| held("output-only provider response identity unavailable"))?,
                        |id| Ok(store.trusted_host().invocation(id)?))?);
                }
            }
            Ok((history, prepared_exposure(&prepared)?.clone(), qualification_sources.into_iter().collect()))
        })();
        match loaded {
            Ok((history, exposure, qualification_sources)) => Self { selected: Some(selected.clone()), history: Some(history),
                exposure: Some(exposure), qualification_sources, unavailable: None },
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
        transport_profile: Option<&str>,
        corpus: &f::Corpus,
    ) -> Result<Projection> {
        let selected = focus.provider_context.as_ref();
        let active = selected.filter(|selected| selected.active(serial));
        let mut unavailable = None;
        let mut history = None;
        let mut roots = Vec::new();
        let mut supplied = Vec::new();
        let mut qualifications = Value::Null;
        let rejection = focus.provider_context_recoveries.iter().rev().find(|pending|
            active.is_some() && pending.rejected_provider_context.as_ref() == active);
        if let Some(selected) = active {
            if rejection.is_some() {
                unavailable = Some("An actual provider rejection made this selected history unavailable for further supply. The authored selection is unchanged. Choose an available scope to rebase on this fresh input, preserve the unavailable reference, or clear it.");
            } else if selected.persona != actor || selected.work != work || selected.model != model {
                unavailable = Some("Selected provider context belongs to a different persona, work or configured model.");
            } else if self.selected.as_ref() != Some(selected) {
                unavailable = Some("Selected provider context material was not loaded for this exact reference.");
            } else if let (Some(input), Some(inherited)) = (&self.history, &self.exposure) {
                if exposure_permitted(state, inherited, audience) && input.validate(model).is_ok()
                    && transport_profile.is_none_or(|profile| profile == input.transport_profile()) {
                    let accompaniment = (|| -> Result<_> {
                        let roots = corpus.required_qualifications(&self.qualification_sources)
                            .ok_or_else(|| held("output-only provider qualifications unavailable"))?;
                        let bundle = f::assemble(corpus, &roots, &[], &[], audience, usize::MAX, 0, 0)?;
                        let references = bundle.entries.iter().map(|entry| entry.reference.clone()).collect::<Vec<_>>();
                        let context = if roots.is_empty() { Value::Null } else {
                            serde_json::from_str(&attributed_context(state, &bundle.rendered_context, &references)?)?
                        };
                        Ok((roots, references, context))
                    })();
                    match accompaniment {
                        Ok((required, references, context)) => {
                            roots = required; supplied = references; qualifications = context;
                            extend_exposure(exposure, inherited);
                            exposure.sources.extend(supplied.iter().cloned());
                            history = Some(input.clone());
                        }
                        Err(_) => unavailable = Some("Selected output-only provider history is unavailable because its current required qualifications could not be supplied."),
                    }
                } else {
                    unavailable = Some("Selected provider context is unavailable under the current model, transport or input binding.");
                }
            } else {
                unavailable = self.unavailable.as_deref();
            }
        }
        let metadata = history.as_ref().map(provider::InputHistory::metadata).transpose()
            .map_err(|_| held("provider history accounting unavailable"))?;
        let mut available_scopes = vec!["current_turn", "current"];
        if transport_profile.is_some_and(provider::supports_output_continuation) {
            available_scopes.insert(0, "current_output");
        }
        Ok(Projection { control: json!({"interface":INTERFACE,"rule":RULE,"selected":selected,
            "available_scopes":available_scopes,"transport_profile":transport_profile,
            "active_this_admission":active.is_some(),
            "preserved_for_next_admission":selected.is_some_and(|selected| selected.last_admission.is_none_or(|last| serial < last)),
            "supplied_history":metadata,"unavailable":unavailable,
            "current_required_qualifications":qualifications,
            "rejected_input":rejection.map(|pending| json!({"invocation":pending.invocation,"terminal":pending.terminal}))}), history, roots, supplied })
    }
}

pub(super) struct Projection {
    pub control: Value,
    pub history: Option<provider::InputHistory>,
    pub roots: Vec<f::Reference>,
    pub supplied: Vec<f::Reference>,
}

pub(super) fn select(
    store: &mut s::Store,
    state: &State,
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
    let (lifetime, scope) = match intent.provider_context() {
        Some(p::ProviderContextSelection::CurrentOutput { lifetime }) => (lifetime, HistoryScope::CurrentOutput),
        Some(p::ProviderContextSelection::Current { lifetime }) => (lifetime, HistoryScope::SuppliedHistory),
        Some(p::ProviderContextSelection::CurrentTurn { lifetime }) => (lifetime, HistoryScope::CurrentTurn),
        _ => return Ok(None),
    };
    require(!reduction_only, "character adoption cannot add a provider continuation turn")?;
    let control = request_context::decode(prepared)?.control;
    require(prepared.request.profile == Profile::Direct && prepared.request.mode == Mode::Ordinary
        && control["provider_continuation"]["interface"] == INTERFACE,
        "provider continuation selection was not offered in this admission")?;
    if scope == HistoryScope::CurrentOutput {
        require(control["provider_continuation"]["available_scopes"].as_array()
            .is_some_and(|scopes| scopes.iter().any(|scope| scope == "current_output")),
            "output-only provider continuation was not offered for this transport")?;
    }
    let terminal = store.trusted_host().read_terminal_evidence(
        &s::EntityId::Invocation(prepared.request.id.clone()), &terminal_id(&prepared.request.id),
    )?.ok_or_else(|| held("completed provider terminal is unavailable"))?;
    let lifetime = lifetime.as_deref().unwrap_or("work");
    require(matches!(lifetime, "work" | "next_call"), "invalid provider context lifetime")?;
    let selected = Selected { invocation: prepared.request.id.clone(), terminal: terminal.exact_ref()?,
        persona: prepared.request.persona.clone(), work: prepared.request.work.clone(),
        model: prepared.requested_model.clone(), character_revision: prepared.character_revision.clone(),
        scope,
        lifetime: lifetime.into(), last_admission: (lifetime == "next_call").then_some(next_serial) };
    *material = Material::load(store, state, &selected, Some(prepared));
    Ok(Some(selected))
}
