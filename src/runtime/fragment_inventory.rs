//! Complete awareness of retained, optionally usable fragment heads.
//! Metadata is not body supply, character adoption, or evidence of improvement.
use super::*;
fn projection(entries: &[Value]) -> Value {
    json!({
        "entries":entries,"metadata_only":true,"body_supplied":false,
        "automatic_selection":false,"automatic_character_adoption":false,
        "truncated":false,"limits_enforced":false,
        "coverage":"All retained fragment heads with exact available provenance in this snapshot; no size, count or ownership quota. No extra model call is scheduled.",
        "scope":"Current heads of retained fragments, including peer, imported, protected, hidden and retired sources, excluding this persona's already mandatory own-voice source IDs. Metadata is not a body or evidence of learning.",
        "accounting":"Metadata from the loaded state; normal Store reads and complete serialized input are recorded. No file body or navigation operation is supplied."
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
    let mut visible = BTreeMap::new();
    for (id, source) in &state.sources {
        if mandatory.contains(id) {
            continue;
        }
        let reference = f::Reference::full(id, &source.head, f::Representation::Utf8);
        let stored = record(state, &reference)?;
        if stored.record.scope != f::Scope::Fragments
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
    for (_, (source, stored, reference)) in visible {
        let exact = checked_wire_reference(&reference, "retained_optional_fragments.reference")?;
        entries.push(json!({"reference":exact,"path":stored.record.path,
            "scope":"fragments","source_owner":source.owner,
            "source_protected":source.protected,"source_retired":source.retired,
            "recorded_author":stored.record.author,"origin":stored.origin,
            "optional":true,"metadata_only":true,"body_supplied":false,
            "character_member":false,"endorsement_or_success":false}));
    }
    Ok((projection(&entries), None))
}
