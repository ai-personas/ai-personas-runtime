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
    identity(operator)?;
    identity(persona_id)?;
    if let Some(source) = source_id {
        identity(source)?;
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
    Ok(json!({
        "persona":persona_id, "label":persona.seed.label,
        "character":character, "characteristic_revisions":revisions,
        "truncated":truncated, "inference_performed":false,
        "scope":"currently permitted accepted characteristic revisions; supporting assets are separate",
        "behavioral_improvement_established":false
    }))
}
