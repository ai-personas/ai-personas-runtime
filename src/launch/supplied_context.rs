//! Exact body entries in the current host-rendered parts of a retained input.
//! Metadata, authored JSON and nested history are not body-supply evidence.
use super::*;

pub(super) struct Entry<'a> {
    pub value: &'a Value,
    pub bundle: &'a Value,
    pub route: Value,
}

fn append<'a>(entries: &mut Vec<Entry<'a>>, bundle: &'a Value, scope: &str, route: Value) {
    if let Some(values) = bundle["entries"].as_array() {
        for value in values.iter().filter(|entry| entry["scope"] == scope) {
            entries.push(Entry {
                value,
                bundle,
                route: route.clone(),
            });
        }
    }
}

pub(super) fn entries<'a>(
    qualified: &'a Value,
    control: &'a Value,
    prepared: &Prepared,
    scope: &str,
) -> Vec<Entry<'a>> {
    let mut entries = Vec::new();
    append(
        &mut entries,
        qualified,
        scope,
        json!({"kind":"qualified_context"}),
    );
    for observation in control["actual_observations"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if observation["actor"] != prepared.request.persona
            || (observation["work"] != prepared.request.work
                && observation["kind"] != "affect_update")
            || !observation["id"].as_str().is_some_and(|id| {
                prepared
                    .supplied_observations
                    .iter()
                    .any(|supplied| supplied == id)
            })
        {
            continue;
        }
        let Some(kind) = observation["kind"].as_str() else {
            continue;
        };
        match kind {
            "qualified_read" | "selection_repair_read" | "local_dependency_event" => {
                append(
                    &mut entries,
                    &observation["payload"]["result"],
                    scope,
                    json!({"kind":kind,"observation":observation["id"],
                        "producing_invocation":observation["invocation"]}),
                );
            }
            "computation_result"
            | "http_fetch_result"
            | "host_command_result"
            | "affect_update" => {
                for bundle in observation["current_required_qualifications"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    append(
                        &mut entries,
                        bundle,
                        scope,
                        json!({"kind":format!("{kind}_qualification"),"observation":observation["id"],
                            "producing_invocation":observation["invocation"]}),
                    );
                }
            }
            _ => (),
        }
    }
    entries
}

pub(super) fn reference(entry: &Entry<'_>) -> Result<f::Reference> {
    observability::context_reference(entry.bundle, &entry.value["reference"])
}

pub(super) fn full_utf8(reference: &f::Reference) -> bool {
    reference.representation == f::Representation::Utf8
        && reference.start.is_none()
        && reference.end.is_none()
}
