//! One explicit inventory for both immutable decoding and mutable packing.
// Method parameters change borrowing only; the paths and validation are shared.
use super::{invalid_storage, POOL_VERSION};
use serde_json::Value;

macro_rules! inventory {
    ($module:ident, [$($mutability:tt)*], $get:ident, $as_object:ident,
     $values:ident, $as_array:ident, $iter:ident) => {
        mod $module {
            use super::*;
fn object_records<'a>(
    parent: &'a $($mutability)* Value,
    field: &str,
) -> serde_json::Result<impl Iterator<Item = &'a $($mutability)* Value>> {
    let records = parent
        .$get(field)
        .map(|value| {
            value
                .$as_object()
                .ok_or_else(|| invalid_storage("retained object collection unavailable"))
        })
        .transpose()?;
    Ok(records.into_iter().flat_map(|records| records.$values()))
}

fn array_records<'a>(
    parent: &'a $($mutability)* Value,
    field: &str,
) -> serde_json::Result<impl Iterator<Item = &'a $($mutability)* Value>> {
    let records = parent
        .$get(field)
        .map(|value| {
            value
                .$as_array()
                .ok_or_else(|| invalid_storage("retained array collection unavailable"))
        })
        .transpose()?;
    Ok(records.into_iter().flat_map(|records| records.$iter()))
}

fn exposure_field(
    record: & $($mutability)* Value,
    field: &str,
    visit: &mut impl FnMut(& $($mutability)* Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    if let Some(exposure) = record.$get(field).filter(|value| !value.is_null()) {
        visit(exposure)?;
    }
    Ok(())
}

fn nested_exposure(
    record: & $($mutability)* Value,
    field: &str,
    visit: &mut impl FnMut(& $($mutability)* Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    if let Some(nested) = record.$get(field).filter(|value| !value.is_null()) {
        exposure_field(nested, "exposure", visit)?;
    }
    Ok(())
}

// Visit only typed host fields. Authored text, arbitrary observation payloads,
// delivery bundles, archives and nested historical records are never rewritten.
// Version 1 permits references only in observation/message exposures. Version 2
// adds this explicit inventory of Exposure-typed State fields, never a recursive
// search for similarly named or shaped values in authored content.
pub(super) fn manifests(
    state: & $($mutability)* Value,
    version: u32,
    mut visit: impl FnMut(& $($mutability)* Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    for name in ["observations", "messages"] {
        let records = state
            .$get(name)
            .and_then(Value::$as_object)
            .ok_or_else(|| invalid_storage("retained record collection unavailable"))?;
        for record in records.$values() {
            exposure_field(record, "exposure", &mut visit)?;
        }
    }
    if version == 1 {
        return Ok(());
    }
    if version != POOL_VERSION {
        return Err(invalid_storage("unsupported retained exposure pool"));
    }
    for persona in object_records(state, "personas")? {
        nested_exposure(persona, "character", &mut visit)?;
        for focus in object_records(persona, "focus")? {
            nested_exposure(focus, "authored_continuation", &mut visit)?;
            exposure_field(focus, "judgment_exposure", &mut visit)?;
            for exposure in array_records(focus, "maintenance_exposures")? {
                visit(exposure)?;
            }
            for field in [
                "feedback_dispositions",
                "peer_feedback",
                "maintenance_dispositions",
            ] {
                for receipt in array_records(focus, field)? {
                    exposure_field(receipt, "exposure", &mut visit)?;
                }
            }
        }
    }
    for source in object_records(state, "sources")? {
        for revision in array_records(source, "revisions")? {
            exposure_field(revision, "exposure", &mut visit)?;
        }
    }
    for meta in object_records(state, "work_meta")? {
        exposure_field(meta, "exposure", &mut visit)?;
        for exposure in object_records(meta, "completion_exposures")? {
            visit(exposure)?;
        }
        if let Some(origins) = meta.$get("arrangement_origins") {
            for field in [
                "delegations",
                "proposals",
                "responses",
                "activations",
                "acceptances",
            ] {
                for exposure in object_records(origins, field)? {
                    visit(exposure)?;
                }
            }
        }
        for wait in object_records(meta, "message_waits")? {
            exposure_field(wait, "exposure", &mut visit)?;
        }
        for question in object_records(meta, "questions")? {
            nested_exposure(question, "origin", &mut visit)?;
            for field in ["replies", "conclusions"] {
                for record in array_records(question, field)? {
                    nested_exposure(record, "origin", &mut visit)?;
                }
            }
        }
    }
    for warning in object_records(state, "semantic_warnings")? {
        nested_exposure(warning, "authored", &mut visit)?;
        for resolution in array_records(warning, "history")? {
            nested_exposure(resolution, "authored", &mut visit)?;
        }
    }
    if let Some(bridge) = state.$get("local_evidence") {
        for basis in object_records(bridge, "bases")? {
            exposure_field(basis, "exposure", &mut visit)?;
        }
    }
    for field in ["computation_jobs", "http_fetch_jobs", "host_command_jobs"] {
        for job in object_records(state, field)? {
            exposure_field(job, "exposure", &mut visit)?;
        }
    }
    for opportunity in object_records(state, "optional_activities")? {
        exposure_field(opportunity, "exposure", &mut visit)?;
        for episode in object_records(opportunity, "episodes")? {
            for admission in array_records(episode, "admissions")? {
                exposure_field(admission, "exposure", &mut visit)?;
            }
            nested_exposure(episode, "ended", &mut visit)?;
        }
    }
    for creation in object_records(state, "persona_creations")? {
        exposure_field(creation, "exposure", &mut visit)?;
    }
    Ok(())
}

        }
    };
}

inventory!(read, [], get, as_object, values, as_array, iter);
inventory!(write, [mut], get_mut, as_object_mut, values_mut, as_array_mut, iter_mut);

pub(super) fn visit(
    state: &Value,
    version: u32,
    visit: impl FnMut(&Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    read::manifests(state, version, visit)
}

pub(super) fn edit(
    state: &mut Value,
    version: u32,
    visit: impl FnMut(&mut Value) -> serde_json::Result<()>,
) -> serde_json::Result<()> {
    write::manifests(state, version, visit)
}
