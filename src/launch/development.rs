//! Permission-filtered inspection of accepted characteristic revisions.
//! Reading this view makes no model call and selects no future persona context.
use super::*;
#[path = "resource_use.rs"]
mod resource_use;

pub fn development(
    root: &Path,
    operator: &str,
    persona_id: &str,
    source_id: Option<&str>,
    limit: usize,
) -> Result<Value> {
    development_at(root, operator, persona_id, source_id, limit, None)
}

/// Inspect one explicitly named accepted invocation without scanning history or
/// mistaking discovery metadata for a fragment body sent to the provider.
pub fn development_at(
    root: &Path,
    operator: &str,
    persona_id: &str,
    source_id: Option<&str>,
    limit: usize,
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
    require((1..=16).contains(&limit), "revision limit must be 1..=16")?;
    // Do not recover or interrupt a live invocation merely to inspect development.
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (_, saved) = load(&mut app)?;
    let state = app.snapshot()?;
    require(
        saved.operator == operator && state.config.local_recipients.contains(operator),
        "development inspection requires the saved requester",
    )?;
    let persona = state
        .personas
        .get(persona_id)
        .ok_or_else(|| held("persona unavailable"))?;
    require(
        persona.created_by == operator
            && persona.seed.audience.contains(operator)
            && persona.seed.grants.communicate_with.contains(operator),
        "persona development is unavailable to this requester",
    )?;
    let mut audience = admission(persona, unix_now()?);
    audience.audience = BTreeSet::from([operator.into()]);
    let character = persona.character.as_ref().filter(|character| {
        character_exposure(character)
            .is_ok_and(|exposure| exposure_permitted(&state, &exposure, &audience))
    });
    let corpus = corpus(&state, &audience)?;
    let mut navigation = f::NavigationBudget::new(state.config.navigation_per_call);
    let mut revisions = Vec::new();
    let mut truncated = false;
    let mut encoded_bytes = 0;
    'sources: for (id, source) in &state.sources {
        if source.owner != persona_id || source.retired || source_id.is_some_and(|s| s != id) {
            continue;
        }
        // Newest first within each source, retaining the exact historical text.
        for revision in source.revisions.iter().rev() {
            if revision.record.scope != f::Scope::Fragments {
                continue;
            }
            let reference = revision.record.reference(f::Representation::Utf8);
            if !lineage_permitted(&state, std::slice::from_ref(&reference), &audience) {
                continue;
            }
            if revisions.len() == limit {
                truncated = true;
                break 'sources;
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
                "origin":revision.origin,
                "context":serde_json::from_str::<Value>(&context)?
            });
            encoded_bytes += serde_json::to_vec(&item)?.len();
            if encoded_bytes > 262_144 {
                truncated = true;
                break 'sources;
            }
            revisions.push(item);
        }
    }
    let mut output = json!({
        "persona":persona_id, "label":persona.seed.label,
        "lifecycle":lifecycle::projection(persona),
        "character":character, "characteristic_revisions":revisions,
        "truncated":truncated, "inference_performed":false,
        "scope":"currently permitted accepted characteristic revisions; supporting assets are separate",
        "behavioral_improvement_established":false
    });
    if let Some(origin) = &persona.birth {
        output["creation_origin"] = if exposure_permitted(&state, &origin.exposure, &audience) {
            json!({"available":true,"kind":"persona_authored_proposal",
                "parent":origin.parent,"invocation":origin.invocation,"work":origin.work,
                "proposal":origin.proposal,"creation_receipt":origin.receipt,
                "initialization_limit":origin.initialization_limit,
                "initialization_attempts_spent":origin.initialization_attempts,
                "character_accepted":persona.character.is_some(),
                "experience_or_competence_established":false})
        } else { json!({"available":false}) };
    }
    if let Some(invocation) = invocation_id {
        output["invocation_evidence"] = invocation_evidence(
            &mut app, &state, &audience, persona_id, invocation, &revisions,
        )?;
    }
    Ok(output)
}

fn invocation_evidence(
    app: &mut Runtime,
    state: &State,
    audience: &f::Admission,
    persona_id: &str,
    invocation_id: &str,
    revisions: &[Value],
) -> Result<Value> {
    let invocation = app.invocation(invocation_id)?;
    let prepared: Prepared = serde_json::from_value(
        invocation
            .request
            .ok_or_else(|| held("invocation evidence unavailable"))?,
    )?;
    require(
        invocation.state == s::InvocationState::Adopted
            && prepared.request.id == invocation_id
            && prepared.request.persona == persona_id
            && s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256
            && exposure_permitted(state, prepared_exposure(&prepared)?, audience)
            && exposure_permitted(
                state,
                &work_exposure(state, &prepared.request.work)?,
                audience,
            ),
        "invocation evidence unavailable",
    )?;
    let proof = app.store.retained_decision(invocation_id)?;
    let object = app
        .store
        .read(&archive_id(invocation_id))?
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
            && archive.actor == persona_id
            && archive.work == prepared.request.work
            && archive.response.invocation_id == invocation_id
            && archive.response.epoch == proof.previous_epoch
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
    let entries = qualified["entries"]
        .as_array()
        .ok_or_else(|| held("qualified input evidence unavailable"))?;
    let instructions = decoded.instructions;
    let control_bytes = decoded.control_bytes;
    let control = decoded.control;
    let mut supplied = Vec::new();
    for revision in revisions {
        let source = revision["source"]
            .as_str()
            .ok_or_else(|| invalid("fragment source"))?;
        let digest = revision["revision"]
            .as_str()
            .ok_or_else(|| invalid("fragment revision"))?;
        let reference = f::Reference::full(source, digest, f::Representation::Utf8);
        let stored = record(state, &reference)?;
        let text = std::str::from_utf8(&stored.record.bytes)
            .map_err(|_| held("fragment text unavailable"))?;
        // A source manifest, inventory, selection, or digest alone cannot satisfy
        // this check: the exact complete body must be in the retained prompt.
        let matches_body = |entry: &Value| {
            entry["scope"] == "fragments"
                && entry["reference"]["source"] == source
                && entry["reference"]["revision"] == digest
                && entry["reference"]["representation"] == "utf8"
                && entry["reference"]["start"].is_null()
                && entry["reference"]["end"].is_null()
                && entry["text"] == text
        };
        let mut routes = Vec::new();
        if entries.iter().any(matches_body) {
            routes.push(json!({"kind":"selected_or_mandatory_context"}));
        }
        // A completed Read reaches a later prompt through a host-rendered
        // observation even when the persona has not selected that source. Match
        // only its qualified file entries, never text embedded in old Decisions,
        // messages, inventory metadata, or a proposed operation.
        if let Some(observations) = control["actual_observations"].as_array() {
            for observation in observations {
                if observation["kind"] == "qualified_read"
                    && observation["actor"] == persona_id
                    && observation["work"] == prepared.request.work
                    && observation["payload"]["result"]["entries"]
                        .as_array()
                        .is_some_and(|entries| entries.iter().any(matches_body))
                {
                    routes.push(json!({
                        "kind":"qualified_read",
                        "observation":observation["id"],
                        "producing_invocation":observation["invocation"]
                    }));
                }
            }
        }
        if !routes.is_empty() {
            supplied.push(json!({
                "reference":wire_reference(&reference),
                "origin":stored.origin,
                "complete_body_in_prepared_input":true,
                "supply_routes":routes,
                "different_from_authoring_invocation":matches!(&stored.origin,
                    Some(RevisionOrigin::PersonaInvocation { invocation_id: authoring, .. }) if authoring != invocation_id)
            }));
        }
    }
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
                && receipt["attempt"]["response_completed"] == true
        });
    Ok(json!({
        "invocation":invocation_id, "work":prepared.request.work,
        "prompt_sha256":prepared.prompt_sha256, "profile":prepared.request.profile,
        "adopted_response":exact, "response_sha256":archive.response_hash,
        "provider_completed_with_matching_input":provider_completed,
        "supplied_characteristic_revisions":supplied,
        "resource_use":resource_use::inspect(state, audience, &prepared, &control, &qualified, &proof)?,
        "computation_execution":tools::inspect(app, state, audience, &proof)?,
        "http_fetch_execution":http_tools::inspect(app, state, audience, &proof)?,
        "accepted_decision":accepted_decision_view(state, audience, &archive)?,
        "input_accounting":{
            "prompt_utf8_bytes":prepared.prompt.len(),
            "instructions_and_application_schema_utf8_bytes":instructions.len(),
            "control_utf8_bytes":control_bytes,
            "qualified_context_utf8_bytes":qualified_bytes,
            "qualified_text_sharing":decoded.accounting,
            "section_separator_utf8_bytes":"\nCONTROL\n\nQUALIFIED_CONTEXT\n".len(),
            "assembled_bytes_with_reserve":prepared.assembled_bytes_with_reserve,
            "provider_reported_usage":if provider_completed { receipt.and_then(|r| r.get("usage")) } else { None },
            "provider_request_bytes":if provider_completed { receipt.and_then(|r| r.get("audit")).and_then(|a| a.get("request_bytes")) } else { None },
            "store_usage_is_unknown":archive.report.usage_is_unknown,
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
    for selection in [
        &archive.response.next_fragments,
        &archive.response.next_assets,
    ] {
        if let p::SelectionIntent::Replace { references, .. } = selection {
            for reference in references {
                if reference.revision.starts_with(p::PROPOSED_REVISION_PREFIX)
                    || reference.revision.starts_with(p::PROPOSED_OPERATION_PREFIX)
                {
                    // Same-response FileWrite policies and retained origins are
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
        if let p::Operation::FileWrite {
            source, policy_id, ..
        } = operation
        {
            if !current_policy(state, policy_id)
                .is_ok_and(|policy| policy_permitted(state, policy, audience))
            {
                return Ok(unavailable());
            }
            // Never infer successful storage from an authored FileWrite alone.
            // A currently retained exact origin corroborates the adoption report.
            let retained = if applied {
                state.sources.get(source).and_then(|source| {
                    source.revisions.iter().find(|stored| {
                        matches!(&stored.origin,
                            Some(RevisionOrigin::PersonaInvocation { persona, invocation_id, operation_id })
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
    let view = json!({
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
    if serde_json::to_vec(&view)?.len() > 65_536 {
        return Ok(json!({
            "available":false,
            "reason":"The complete decision projection exceeds this view's 65536-byte bound; no partial judgment is presented."
        }));
    }
    Ok(view)
}
