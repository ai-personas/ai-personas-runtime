//! Bounded awareness of the actor's retained, optionally usable fragment heads.
//! Metadata is not body supply, character adoption, or evidence of improvement.
use super::*;
use std::ops::Bound::{Excluded, Unbounded};

const MAX_WINDOW_HEADS: usize = 64;
const MAX_ENTRIES: usize = 16;
const MAX_METADATA_BYTES: usize = 8192;

fn projection(entries: &[Value], truncated: bool, index_bound: usize) -> Value {
    json!({
        "entries":entries,
        "metadata_only":true,
        "body_supplied":false,
        "automatic_selection":false,
        "automatic_character_adoption":false,
        "truncated":truncated,
        "coverage":"One bounded window over currently permitted own fragment heads. A private scan position advances only with an already-authorized ordinary admission and restarts after the end. No extra call is scheduled or funded; discovery remains available for explicit bounded lookup. Incomplete windows and oversized omissions do not prove absence.",
        "scope":"Current permitted heads of active, unprotected, nonhidden fragments owned and authored by this persona through an accepted persona invocation, excluding accepted own-voice source IDs. Authored scope is not semantic verification or evidence of learning. Omitted entries establish neither absence nor permission.",
        "bounds":{"window_heads":MAX_WINDOW_HEADS,"index_sources":index_bound,"entries":MAX_ENTRIES,
            "serialized_metadata_bytes":MAX_METADATA_BYTES},
        "accounting":"Host metadata index from the already loaded state, bounded by the existing corpus revision limit; each output window is separately bounded. Full-state Store reads and serialized bytes are charged by normal preparation; serialized bytes and exposure remain within ordinary context, preparation and Store bounds. No file body or navigation operation is supplied or performed."
    })
}

pub(super) fn context(
    state: &State,
    actor: &str,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<(Value, Option<String>)> {
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("persona absent"))?;
    let mandatory = character_refs(persona)?
        .into_iter()
        .map(|reference| reference.source)
        .collect::<BTreeSet<_>>();
    // Every accepted source consumes at least one retained revision. The existing
    // corpus limit therefore bounds this entire index construction, not only the
    // emitted page. Never silently stop a global scan at a private source count.
    let index_bound = state.config.corpus.max_revisions;
    require(
        state.sources.len() <= index_bound,
        "fragment index exceeds corpus bound",
    )?;
    let mut visible = BTreeMap::new();
    for (id, source) in state.sources.iter().take(index_bound) {
        if source.owner != actor || source.retired || source.protected || mandatory.contains(id) {
            continue;
        }
        let reference = f::Reference::full(id, &source.head, f::Representation::Utf8);
        let stored = record(state, &reference)?;
        if stored.record.scope != f::Scope::Fragments
            || stored.record.retired
            || stored.record.author != actor
            || stored
                .record
                .path
                .split('/')
                .any(|part| part.starts_with('.'))
            || !matches!(&stored.origin, Some(RevisionOrigin::PersonaInvocation { persona, .. }) if persona == actor)
            || !lineage_permitted(state, std::slice::from_ref(&reference), a)
        {
            continue;
        }
        // The permitted view determines page contents, absence and truncation,
        // even when a candidate is omitted. Retain its complete restrictions.
        exposure.sources.push(reference.clone());
        visible.insert(id.clone(), (source, stored, reference));
    }
    let mut entries = Vec::new();
    let start = persona
        .fragment_inventory_after
        .as_ref()
        .filter(|id| visible.contains_key(*id));
    let mut inspected = visible
        .range::<String, _>((start.map_or(Unbounded, Excluded), Unbounded))
        .peekable();
    let mut truncated = start.is_some();
    if inspected.peek().is_none() {
        inspected = visible.range::<String, _>(..).peekable();
    }
    let mut next_after = None;
    let mut retry_omission = false;
    for _ in 0..MAX_WINDOW_HEADS {
        if entries.len() == MAX_ENTRIES {
            break;
        }
        let Some((id, (source, stored, reference))) = inspected.next() else {
            break;
        };
        let previous_after = next_after.replace(id.clone());
        let Ok(exact) = checked_wire_reference(reference, "retained_optional_fragments.reference")
        else {
            truncated = true;
            continue;
        };
        let entry = json!({"reference":exact,"path":stored.record.path,
            "scope":"fragments","source_owner":source.owner,
            "recorded_author":stored.record.author,"origin":stored.origin,
            "optional":true,"metadata_only":true,"body_supplied":false,
            "character_member":false,"endorsement_or_success":false});
        entries.push(entry);
        // Reserve the longer spelling of the final truncation flag as well.
        if serde_json::to_vec(&projection(&entries, false, index_bound))?.len() > MAX_METADATA_BYTES
        {
            let omitted = entries.pop().unwrap();
            truncated = true;
            if serde_json::to_vec(&projection(&[omitted], false, index_bound))?.len()
                <= MAX_METADATA_BYTES
            {
                // Retry a fitting entry in a fresh window, instead of repeatedly
                // omitting it behind the same earlier output entries.
                next_after = previous_after;
                retry_omission = true;
                break;
            }
            // Individually oversized metadata is omitted explicitly; advance so
            // it cannot prevent later entries from being considered.
            continue;
        }
    }
    if inspected.peek().is_some() {
        truncated = true;
    } else if !retry_omission {
        next_after = None;
    }
    let value = projection(&entries, truncated, index_bound);
    require(
        serde_json::to_vec(&value)?.len() <= MAX_METADATA_BYTES,
        "retained fragment metadata exceeds bound",
    )?;
    Ok((value, next_after))
}
