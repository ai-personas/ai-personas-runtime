//! Lossless sharing of repeated qualification-reference metadata. Only the
//! host-rendered roles[].required_by slots participate; authored JSON, source
//! bodies, entry identities, policies and qualification edges remain intact.
use super::*;

const FIELD: &str = "shared_references";
const PROFILE: &str = "shared-qualified-references-v1";
const RULE: &str = "In a qualified entry's roles, required_by:{\"shared_reference\":\"qN\"} names the complete exact source reference in QUALIFIED_CONTEXT.shared_references.values[qN]. That reference metadata is supplied in this same input, not a retrieval request. Each role, bundle, qualification edge and source body remains separate and retains its original meaning. Entry references and input-file handles remain unchanged. Expand the metadata when an operation needs the full reference; the alias is not a native file reference.";

fn key(value: &Value) -> Result<String> { Ok(serde_json::to_string(value)?) }

fn collect(bundle: &Value, groups: &mut BTreeMap<String, (Value, usize)>) -> Result<()> {
    // Historical bundles can retain another reference wire format. Preserve
    // those literal records; this encoding only shares actor-facing ExactRefs.
    if bundle["reference_format"] != "decision_exact_ref" { return Ok(()); }
    for entry in entries(bundle)? {
        for role in entry["roles"].as_array().ok_or_else(|| held("qualified roles unavailable"))? {
            if role["role"] != "qualification" { continue; }
            let reference = role.get("required_by").ok_or_else(|| held("qualification source missing"))?;
            require(reference.get("shared_reference").is_none(), "qualification reference is already encoded")?;
            let group = groups.entry(key(reference)?).or_insert_with(|| (reference.clone(), 0));
            group.1 += 1;
        }
    }
    Ok(())
}

fn replace(bundle: &mut Value, mapping: &BTreeMap<String, String>) -> Result<()> {
    if bundle["reference_format"] != "decision_exact_ref" { return Ok(()); }
    for entry in entries_mut(bundle)? {
        for role in entry["roles"].as_array_mut().ok_or_else(|| held("qualified roles unavailable"))? {
            if role["role"] == "qualification" {
                if let Some(alias) = mapping.get(&key(&role["required_by"])?) {
                    role["required_by"] = json!({"shared_reference":alias});
                }
            }
        }
    }
    Ok(())
}

pub(super) fn encode(control: &mut Value, qualified: &mut Value) -> Result<bool> {
    require(qualified.get(FIELD).is_none(), "qualification references are already encoded")?;
    let paths = encoding_bundle_paths(control);
    let original_size = sections_size(control, qualified)?;
    let mut groups = BTreeMap::new();
    collect(qualified, &mut groups)?;
    for path in &paths { collect(control.pointer(&pointer(path)).ok_or_else(|| held("qualified bundle unavailable"))?, &mut groups)?; }
    let mut mapping = BTreeMap::new();
    let mut values = serde_json::Map::new();
    for (canonical, (reference, count)) in groups {
        if count <= 1 { continue; }
        let alias = format!("q{}", values.len());
        let replacement_size = size(&json!({"shared_reference":alias}))?;
        if size(&reference)? * count <= size(&reference)? + alias.len() + 4 + replacement_size * count { continue; }
        mapping.insert(canonical, alias.clone());
        values.insert(alias, reference);
    }
    if values.is_empty() { return Ok(false); }
    let mut next_control = control.clone();
    let mut next_qualified = qualified.clone();
    replace(&mut next_qualified, &mapping)?;
    for path in &paths { replace(next_control.pointer_mut(&pointer(path)).ok_or_else(|| held("qualified bundle unavailable"))?, &mapping)?; }
    next_qualified[FIELD] = json!({"profile":PROFILE,"rule":RULE,"values":values});
    if sections_size(&next_control, &next_qualified)? >= original_size { return Ok(false); }
    *control = next_control;
    *qualified = next_qualified;
    Ok(true)
}

pub(super) struct Accounting { pub shared: usize, pub occurrences: usize }

fn restore(bundle: &mut Value, values: Option<&serde_json::Map<String, Value>>, used: &mut BTreeMap<String, usize>) -> Result<()> {
    if bundle["reference_format"] != "decision_exact_ref" { return Ok(()); }
    for entry in entries_mut(bundle)? {
        for role in entry["roles"].as_array_mut().ok_or_else(|| held("qualified roles unavailable"))? {
            if role["role"] != "qualification" { continue; }
            let reference = &mut role["required_by"];
            let Some(alias) = reference.get("shared_reference") else { continue; };
            require(reference.as_object().is_some_and(|object| object.len() == 1), "conflicting qualification reference encodings")?;
            let alias = alias.as_str().ok_or_else(|| held("invalid shared qualification reference"))?;
            let value = values.and_then(|values| values.get(alias)).ok_or_else(|| held("shared qualification reference unavailable"))?;
            *used.entry(alias.into()).or_default() += 1;
            *reference = value.clone();
        }
    }
    Ok(())
}

pub(super) fn decode(control: &mut Value, qualified: &mut Value) -> Result<Option<Accounting>> {
    let paths = encoding_bundle_paths(control);
    let encoded_size = sections_size(control, qualified)?;
    let pool = qualified.as_object_mut().ok_or_else(|| held("qualified context unavailable"))?.remove(FIELD);
    let values = if let Some(pool) = &pool {
        require(pool["profile"] == PROFILE && pool["rule"] == RULE
            && pool.as_object().is_some_and(|object| object.len() == 3), "unknown qualification reference encoding")?;
        let values = pool["values"].as_object().ok_or_else(|| held("shared qualification references unavailable"))?;
        require(!values.is_empty(), "empty qualification reference pool")?;
        let mut unique = BTreeSet::new();
        for (alias, value) in values {
            let suffix = alias.strip_prefix('q').ok_or_else(|| held("invalid qualification reference alias"))?;
            let ordinal: usize = suffix.parse().map_err(|_| held("invalid qualification reference alias"))?;
            require(alias == &format!("q{ordinal}") && ordinal < values.len(), "invalid qualification reference alias")?;
            let reference: p::ExactRef = serde_json::from_value(value.clone())?;
            file_reference(&reference)?;
            require(unique.insert(key(value)?), "duplicate shared qualification reference")?;
        }
        Some(values)
    } else { None };
    let mut used = BTreeMap::new();
    restore(qualified, values, &mut used)?;
    for path in &paths { restore(control.pointer_mut(&pointer(path)).ok_or_else(|| held("qualified bundle unavailable"))?, values, &mut used)?; }
    let Some(values) = values else { return Ok(None); };
    require(used.len() == values.len() && used.values().all(|count| *count > 1), "unused or unshared qualification reference")?;
    require(sections_size(control, qualified)? > encoded_size, "qualification reference sharing accounting mismatch")?;
    Ok(Some(Accounting { shared: values.len(), occurrences: used.values().sum() }))
}
