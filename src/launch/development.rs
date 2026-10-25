//! Attributed inspection of accepted characteristic revisions.
//! Reading this view makes no model call and selects no future persona context.
use super::*;
#[path = "resource_use.rs"]
mod resource_use;
#[path = "supplied_context.rs"]
mod supplied_context;

pub fn development(
    root: &Path,
    operator: &str,
    persona_id: &str,
    source_id: Option<&str>,
) -> Result<Value> {
    development_at(root, operator, persona_id, source_id, None)
}

/// Inspect one explicitly named accepted invocation without scanning history or
/// mistaking discovery metadata for a fragment body sent to the provider.
pub fn development_at(
    root: &Path,
    operator: &str,
    persona_id: &str,
    source_id: Option<&str>,
    invocation_id: Option<&str>,
) -> Result<Value> {
    identity(operator)?;
    identity(persona_id)?;
    if let Some(source) = source_id {
        identity(source)?;
    }
    if let Some(invocation) = invocation_id {
        identity(invocation)?;
    }
    // Do not recover or interrupt a live invocation merely to inspect development.
    inspection::project(root, |snapshot, records| {
        project(snapshot, records, operator, persona_id, source_id, invocation_id)
    })
}

fn project(
    snapshot: &inspection::Snapshot,
    records: &s::RuntimeInspection<'_, '_>,
    operator: &str,
    persona_id: &str,
    source_id: Option<&str>,
    invocation_id: Option<&str>,
) -> Result<Value> {
    let saved = &snapshot.saved;
    let state = &snapshot.state;
    require(
        saved.operator == operator && state.config.local_recipients.contains(operator),
        "development inspection requires the saved requester",
    )?;
    let persona = state
        .personas
        .get(persona_id)
        .ok_or_else(|| held("persona unavailable"))?;
    let mut audience = admission(persona, unix_now()?);
    audience.audience = BTreeSet::from([operator.into()]);
    let character = persona.character.as_ref().filter(|character| {
        character_exposure(character)
            .is_ok_and(|exposure| exposure_permitted(&state, &exposure, &audience))
    });
    let corpus = corpus(&state, &audience)?;
    let affect = affect::project(state, persona_id, &audience, &corpus)?;
    let mut navigation = f::NavigationBudget::new(state.config.navigation_per_call);
    let mut revisions = Vec::new();
    let mut truncated = false;
    'sources: for (id, source) in &state.sources {
        if source_id.is_some_and(|s| s != id) {
            continue;
        }
        // Newest first within each source, retaining the exact historical text.
        for revision in source.revisions.iter().rev() {
            if revision.record.scope != f::Scope::Fragments
                || (source_id.is_none() && source.owner != persona_id && revision.record.author != persona_id) {
                continue;
            }
            let reference = revision.record.reference(f::Representation::Utf8);
            if !lineage_permitted(&state, std::slice::from_ref(&reference), &audience) {
                continue;
            }
            let bundle = match corpus.read(
                &reference,
                &audience,
                &mut navigation,
                state.config.navigation_per_call.result_bytes,
            ) {
                Ok(bundle) => bundle,
                Err(f::FilesError::NavigationExhausted | f::FilesError::ReadOverflow { .. }) => {
                    truncated = true;
                    break 'sources;
                }
                Err(_) => continue,
            };
            let references = bundle
                .entries
                .iter()
                .map(|entry| entry.reference.clone())
                .collect::<Vec<_>>();
            let context = attributed_context(&state, &bundle.rendered_context, &references)?;
            let item = json!({
                "source":id, "revision":revision.record.revision,
                "current":source.head == revision.record.revision,
                "source_owner":source.owner,"source_protected":source.protected,
                "source_retired":source.retired,"recorded_author":revision.record.author,
                "origin":revision.origin,
                "context":serde_json::from_str::<Value>(&context)?
            });
            revisions.push(item);
        }
    }
    let mut output = json!({
        "persona":persona_id, "label":persona.seed.label,
        "lifecycle":lifecycle::projection(persona),
        "character":character, "characteristic_revisions":revisions,
        "transient_affect":{"context":affect.control,"observation":affect.observation},
        "truncated":truncated, "inference_performed":false,
        "scope":"Accepted characteristic revisions owned or authored by this persona, or the explicitly selected source. Retired revisions remain labeled; supporting assets are separate.",
        "behavioral_improvement_established":false
    });
    if let Some(origin) = &persona.birth {
        output["creation_origin"] = if exposure_permitted(&state, &origin.exposure, &audience) {
            json!({"available":true,"kind":"persona_authored_proposal",
                "parent":origin.parent,"proposal_author_invocation":origin.invocation,
                "causal_work":origin.work,
                "proposal":origin.proposal,"creation_receipt":origin.receipt,
                "meaning":"Proposal authorship and creation are separate events. The proposal_author_invocation may belong to different work; causal_work identifies the creation context. The exact receipt records the creation request and any creating persona operation.",
                "initialization_limit":null,"historical_initialization_limit":origin.initialization_limit,
                "initialization_attempts_spent":origin.initialization_attempts,
                "character_accepted":persona.character.is_some(),
                "experience_or_competence_established":false})
        } else { json!({"available":false}) };
    }
    if let Some(invocation) = invocation_id {
        output["invocation_evidence"] = invocation_evidence(
            records, &state, &audience, persona_id, invocation, &revisions,
        )?;
    }
    Ok(output)
}

fn invocation_evidence(
    records: &s::RuntimeInspection<'_, '_>,
    state: &State,
    audience: &f::Admission,
    persona_id: &str,
    invocation_id: &str,
    revisions: &[Value],
) -> Result<Value> {
    let invocation = records.invocation(invocation_id)?
        .ok_or_else(|| s::StoreError::Missing(invocation_id.into()))?;
    let prepared: Prepared = serde_json::from_value(
        invocation
            .request
            .ok_or_else(|| held("invocation evidence unavailable"))?,
    )?;
    require(
        invocation.state == s::InvocationState::Adopted
            && prepared.request.id == invocation_id
            && prepared.request.persona == persona_id
            && invocation.prepared_epoch == prepared.epoch
            && s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
            && exposure_permitted(state, prepared_exposure(&prepared)?, audience)
            && exposure_permitted(
                state,
                &work_exposure(state, &prepared.request.work)?,
                audience,
            ),
        "invocation evidence unavailable",
    )?;
    let proof = records.decision(invocation_id)?;
    let reservation = records.reservation(&invocation.reservation)?;
    let object = records.object(&archive_id(invocation_id))?
        .ok_or_else(|| held("invocation evidence unavailable"))?;
    let exact = object.exact_ref()?;
    let archive: AdoptedResponse = serde_json::from_value(object.body.clone())?;
    let authored = archive
        .authored_json
        .as_ref()
        .ok_or_else(|| held("invocation evidence unavailable"))?;
    require(
        object.id == archive_id(invocation_id)
            && object.kind == "adopted-persona-response"
            && object.revision == 1
            && proof.invocation_id == invocation_id
            && proof.objects.contains(&exact)
            && proof.response_hash == archive.response_hash
            && reservation.navigation_charge.as_ref().is_none_or(|charge|
                charge.response_sha256 == archive.response_hash)
            && archive.actor == persona_id
            && archive.work == prepared.request.work
            && archive.response.invocation_id == invocation_id
            && archive.response.epoch == prepared.epoch
            && proof.prepared_epoch.unwrap_or(proof.previous_epoch) == prepared.epoch
            && prepared.epoch <= proof.previous_epoch
            && archive.report.invocation_id == invocation_id
            && archive.report.accepted_epoch == Some(proof.accepted_epoch)
            && matches!(
                archive.report.status,
                AdoptionStatus::Adopted | AdoptionStatus::CharacterOnly
            )
            && s::sha256_bytes(authored.as_bytes()) == archive.response_hash
            && archive
                .exposure
                .as_ref()
                .is_some_and(|e| exposure_permitted(state, e, audience)),
        "invocation evidence unavailable",
    )?;
    let decoded = request_context::decode(&prepared)?;
    let qualified_bytes = decoded.qualified_bytes;
    let qualified = decoded.qualified;
    require(qualified["entries"].is_array(), "qualified input evidence unavailable")?;
    let instructions = decoded.instructions;
    let control_bytes = decoded.control_bytes;
    let control = decoded.control;
    let mut supplied: BTreeMap<f::Reference, Value> = BTreeMap::new();
    for candidate in supplied_context::entries(&qualified, &control, &prepared, "fragments") {
        let reference = supplied_context::reference(&candidate)?;
        if !revisions.iter().any(|revision| {
            revision["source"] == reference.source && revision["revision"] == reference.revision
        }) {
            continue;
        }
        let stored = record(state, &reference)?;
        let text = candidate.value["text"].as_str()
            .ok_or_else(|| held("supplied characteristic text unavailable"))?;
        require(
            stored.record.scope == f::Scope::Fragments
                && lineage_permitted(state, std::slice::from_ref(&reference), audience)
                && f::project_text(&stored.record, &reference, &state.config.corpus.json)
                    .is_ok_and(|retained| retained == text),
            "supplied characteristic evidence does not match its exact retained projection",
        )?;
        let value = supplied.entry(reference.clone()).or_insert_with(|| json!({
            "reference":wire_reference(&reference),
            "origin":stored.origin,
            "exact_projected_body_matched":true,
            "complete_body_in_prepared_input":supplied_context::full_utf8(&reference),
            "supplied_utf8_bytes":text.len(),
            "supplied_text_sha256":s::sha256_bytes(text.as_bytes()),
            "supply_routes":[],
            "different_from_authoring_invocation":matches!(&stored.origin,
                Some(RevisionOrigin::PersonaInvocation { invocation_id: authoring, .. }
                    | RevisionOrigin::NativeFileEdit { invocation_id: authoring, .. }) if authoring != invocation_id)
        }));
        let mut route = if candidate.route["kind"] == "qualified_context" {
            json!({"kind":"selected_or_mandatory_context"})
        } else {
            candidate.route
        };
        route["roles"] = candidate.value["roles"].clone();
        let routes = value["supply_routes"].as_array_mut().unwrap();
        if !routes.contains(&route) {
            routes.push(route);
        }
    }
    let supplied = supplied.into_values().collect::<Vec<_>>();
    let receipt = archive
        .provider_observation
        .as_ref()
        .and_then(|v| v.get("receipt"));
    let provider_completed = prepared.request.profile == Profile::Direct
        && receipt.is_some_and(|receipt| {
            receipt["kind"] == "provider_complete"
                && receipt["synthetic"] == false
                && receipt["audit"]["invocation_id"] == invocation_id
                && receipt["audit"]["prompt_sha256"] == prepared.prompt_sha256
                && crate::provider::response_only_request_matches(
                    &receipt["audit"], &prepared.requested_model, &prepared.requested_effort,
                    &prepared.prompt, prepared.reusable_prefix_bytes,
                    &prepared.images, &prepared.output_schema,
                )
                && receipt["attempt"]["response_completed"] == true
        });
    Ok(json!({
        "invocation":invocation_id, "work":prepared.request.work,
        "prompt_sha256":prepared.prompt_sha256, "profile":prepared.request.profile,
        "adopted_response":exact, "response_sha256":archive.response_hash,
        "provider_completed_with_matching_input":provider_completed,
        "supplied_characteristic_revisions":supplied,
        "supplied_transient_affect":control["transient_affect"],
        "supplied_image_inputs":control["image_inputs"],
        "supplied_affect_observations":control["actual_observations"].as_array()
            .map(|observations| observations.iter().filter(|observation|
                observation["kind"] == "affect_update").cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
        "resource_use":resource_use::inspect(state, audience, &prepared, &control, &qualified, &proof)?,
        "computation_execution":tools::inspect(records, state, audience, &proof)?,
        "http_fetch_execution":http_tools::inspect(records, state, audience, &proof)?,
        "host_command_execution":host_tools::inspect(records, state, audience, &proof)?,
        "accepted_decision":accepted_decision_view(state, audience, &prepared, &archive)?,
        "input_accounting":{
            "prompt_utf8_bytes":prepared.prompt.len(),
            "image_content_parts":prepared.images.len(),
            "instructions_and_application_schema_utf8_bytes":instructions.len(),
            "control_utf8_bytes":control_bytes,
            "qualified_context_utf8_bytes":qualified_bytes,
            "exact_text_sharing":decoded.accounting,
            "section_separator_utf8_bytes":"\nCONTROL\n\nQUALIFIED_CONTEXT\n".len(),
            "assembled_bytes_with_reserve":prepared.assembled_bytes_with_reserve,
            "provider_reported_usage":if provider_completed { receipt.and_then(|r| r.get("usage")) } else { None },
            "provider_request_bytes":if provider_completed { receipt.and_then(|r| r.get("audit")).and_then(|a| a.get("request_bytes")) } else { None },
            "store_usage_is_unknown":archive.report.usage_is_unknown,
            "store_reservation":{
                "state":reservation.state,
                "original_allowance":reservation.amount,
                "known_native_navigation":reservation.navigation_charge,
                "complete_measurement":reservation.measured
            },
            "scope":"This retained call only. Prompt bytes, reserved assembly bytes, provider transport bytes, reported tokens and Store accounting are distinct; no whole-episode cost or savings claim."
        },
        "scope":"Exact full bodies in selected/mandatory context or qualified Read observations, among the characteristic revisions returned by this bounded view. Other revisions and invocations are not inspected. Current permissions still apply.",
        "behavioral_improvement_established":false,
        "limits":"Input supply and an accepted response do not prove attention, causation, improved behavior, or successful execution of every proposed operation. Inspect the work result separately."
    }))
}

/// Keep authored judgments distinct from locally applied operations. The
/// invocation/archive permission checks above cover the decision; newly written
/// file metadata additionally needs its own current source permission check.
fn accepted_decision_view(
    state: &State,
    audience: &f::Admission,
    prepared: &Prepared,
    archive: &AdoptedResponse,
) -> Result<Value> {
    let unavailable = || {
        json!({
            "available":false,
            "reason":"The complete decision view is unavailable under current source permissions or retention; no partial judgment is presented."
        })
    };
    // A next selection may name material that was not in this call's input.
    // Its identities must not inherit disclosure from the input's exposure.
    let mut resolved_input_selections = BTreeMap::new();
    for (scope, selection) in [
        ("fragments", &archive.response.next_fragments),
        ("assets", &archive.response.next_assets),
    ] {
        let references = input_files::replacement_references(
            prepared, &archive.response, selection,
        )?.unwrap_or_default();
        match selection {
            p::SelectionIntent::ReplaceFromInput { handles, .. }
            | p::SelectionIntent::ReplaceFromSavedFiles { input_handles: handles, .. }
                if !handles.is_empty() => {
                let references = input_files::resolve(prepared, handles)?;
                resolved_input_selections.insert(scope, handles.iter().zip(&references)
                    .map(|(handle, reference)| json!({"handle":handle,"reference":reference,"metadata_only":true}))
                    .collect::<Vec<_>>());
            }
            _ => (),
        }
        for reference in &references {
            if reference.revision.starts_with(p::PROPOSED_REVISION_PREFIX)
                || reference.revision.starts_with(p::PROPOSED_OPERATION_PREFIX)
            {
                // Same-response save policies and retained origins are
                // checked below before any of this projection is returned.
                continue;
            }
            if !file_reference(reference)
                .is_ok_and(|reference| lineage_permitted(state, &[reference], audience))
            {
                return Ok(unavailable());
            }
        }
    }
    let mut operations = Vec::new();
    for operation in &archive.response.operations {
        let value = serde_json::to_value(operation)?;
        let applied = archive.report.applied.iter().any(|id| id == operation.id());
        let fenced = archive.report.fenced.iter().any(|id| id == operation.id());
        let mut item = json!({
            "id":operation.id(), "kind":value["kind"],
            "disposition":if applied { "applied_locally" } else if fenced { "fenced" } else { "unestablished" }
        });
        if let p::Operation::WorkCommand { inputs, .. } = operation {
            item["command"] = inputs["kind"].clone();
        }
        if let p::Operation::Effect {
            capability_id,
            inputs,
            ..
        } = operation
        {
            item["capability_id"] = json!(capability_id);
            if capability_id == "work.command.v1" {
                item["command"] = inputs["kind"].clone();
                item["execution_scope"] = json!("internal_work_operation");
            }
        }
        if let Some(p::FileSave { source, policy_id, .. }) = operation.file_save()
        {
            if !current_policy(state, policy_id)
                .is_ok_and(|policy| policy_permitted(state, policy, audience))
            {
                return Ok(unavailable());
            }
            // Never infer successful storage from an authored save alone.
            // A currently retained exact origin corroborates the adoption report.
            let retained = if applied {
                state.sources.get(source).and_then(|source| {
                    source.revisions.iter().find(|stored| {
                        matches!(&stored.origin,
                            Some(RevisionOrigin::PersonaInvocation { persona, invocation_id, operation_id }
                                | RevisionOrigin::NativeFileEdit { persona, invocation_id, operation_id, .. }
                                | RevisionOrigin::HostFileImport { persona, invocation_id, operation_id, .. })
                            if persona == &archive.actor
                                && invocation_id == &archive.response.invocation_id
                                && operation_id == operation.id())
                            && lineage_permitted(
                                state,
                                &[stored.record.reference(f::Representation::Utf8)],
                                audience,
                            )
                    })
                })
            } else {
                None
            };
            if applied && retained.is_none() {
                // The maintenance explanation or proposed selection may name
                // this new material too. Withhold the whole decision view.
                return Ok(unavailable());
            }
            item["retained_write"] = match retained {
                Some(stored) => json!({
                    "status":"retained_exact_revision",
                    "reference":wire_reference(&stored.record.reference(f::Representation::Utf8)),
                    "scope":stored.record.scope,
                    "origin":stored.origin,
                    "source_retired":state.sources[source].retired,
                    "metadata_only":true,
                    "current_selection_not_inferred":true
                }),
                None => json!({"status":"unavailable_or_not_applied"}),
            };
        }
        operations.push(item);
    }
    let decision = &archive.response;
    let mut view = json!({
        "available":true,
        "adoption_status":archive.report.status,
        "adoption_reason":archive.report.reason,
        "work":decision.work,
        "fragment_maintenance":decision.fragment_maintenance,
        "asset_maintenance":decision.asset_maintenance,
        "next_fragments":decision.next_fragments,
        "next_assets":decision.next_assets,
        "continuation":decision.continuation,
        "operations":operations,
        "scope":"Authored judgments and next-context intentions from this exact accepted response. Applied means adopted locally, not independently verified quality, external execution or delivery. Fenced proposals did not take effect. Retained-write metadata does not supply a file body, prove current selection or establish later benefit."
    });
    if !resolved_input_selections.is_empty() {
        view["resolved_input_selections"] = serde_json::to_value(resolved_input_selections)?;
    }
    Ok(view)
}
