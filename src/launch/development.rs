//! Permission-filtered inspection of accepted characteristic revisions.
//! Reading this view makes no model call and selects no future persona context.
use super::*;

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
    let mut navigation = f::NavigationBudget::new(state.config.navigation_per_call.clone());
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
        "character":character, "characteristic_revisions":revisions,
        "truncated":truncated, "inference_performed":false,
        "scope":"currently permitted accepted characteristic revisions; supporting assets are separate",
        "behavioral_improvement_established":false
    });
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
    let (_, qualified) = prepared
        .prompt
        .rsplit_once("\nQUALIFIED_CONTEXT\n")
        .ok_or_else(|| held("qualified input evidence unavailable"))?;
    let qualified: Value = serde_json::from_str(qualified)?;
    let entries = qualified["entries"]
        .as_array()
        .ok_or_else(|| held("qualified input evidence unavailable"))?;
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
        if entries.iter().any(|entry| {
            entry["scope"] == "fragments"
                && entry["reference"]["source"] == source
                && entry["reference"]["revision"] == digest
                && entry["reference"]["representation"] == "utf8"
                && entry["reference"]["start"].is_null()
                && entry["reference"]["end"].is_null()
                && entry["text"] == text
        }) {
            supplied.push(json!({
                "reference":wire_reference(&reference),
                "origin":stored.origin,
                "complete_body_in_prepared_input":true,
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
        "scope":"Exact full bodies among the characteristic revisions returned by this bounded view. Other revisions and invocations are not inspected. Current permissions still apply.",
        "behavioral_improvement_established":false,
        "limits":"Input supply and an accepted response do not prove attention, causation, improved behavior, or successful execution of every proposed operation. Inspect the work result separately."
    }))
}
