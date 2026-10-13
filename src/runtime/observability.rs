//! Bounded actor-facing receipts and exact mutation/selection metadata.
//! These projections never select, refresh, read a new body, or grant authority.
use super::*;

const MAX_MUTATION_SOURCES: usize = 256;

pub(super) fn selection_state(
    state: &State,
    focus: &Focus,
    serial: u64,
    a: &f::Admission,
    visible: &mut BTreeSet<f::Reference>,
) -> Result<Value> {
    let mut entries = |selection: &Option<Vec<Selected>>| -> Result<Vec<Value>> {
        let mut output = Vec::new();
        require(
            selection.as_ref().is_none_or(|s| s.len() <= 256),
            "selection metadata bound",
        )?;
        for selected in selection.iter().flatten() {
            let reference = file_reference(&selected.reference)?;
            if !lineage_permitted(state, std::slice::from_ref(&reference), a) {
                continue;
            }
            visible.insert(reference);
            output.push(json!({
                "reference":selected.reference,
                "last_admission":selected.last_admission,
                "active_this_admission":selected.last_admission.is_none_or(|last| serial <= last),
                "preserved_for_next_admission":selected.last_admission.is_none_or(|last| serial < last),
                "metadata_only":true
            }));
        }
        Ok(output)
    };
    Ok(json!({
        "fragments_initialized":focus.fragments.is_some(),
        "assets_initialized":focus.assets.is_some(),
        "admission_serial":serial,
        "fragments":entries(&focus.fragments)?,
        "assets":entries(&focus.assets)?,
        "scope":"exact authored selections; inaccessible entries omitted; no lifetime renewal"
    }))
}

pub(super) fn observation_references(
    observation: &Observation,
    visible: &mut BTreeSet<f::Reference>,
) -> Result<()> {
    match observation.kind.as_str() {
        "qualified_read" | "selection_repair_read" | "local_dependency_event" => {
            visible.extend(observation.sources.iter().cloned())
        }
        "accepted_file_write" => {
            let reference: p::ExactRef =
                serde_json::from_value(observation.payload["source"].clone())?;
            visible.insert(file_reference(&reference)?);
        }
        "discovery" => {
            // Search provenance includes all consulted sources. Only actual hits
            // are observed candidates; never turn the consulted scope into inventory.
            // Legacy Search retains encoded JSON; explicit Discover retains an object.
            let page = match observation.payload.get("page") {
                Some(Value::String(page)) => serde_json::from_str::<Value>(page)?,
                Some(page @ Value::Object(_)) => page.clone(),
                _ => return Ok(()),
            };
            if let Some(hits) = page["hits"].as_array() {
                for hit in hits {
                    visible.insert(serde_json::from_value(hit["reference"].clone())?);
                }
            }
        }
        _ => (),
    }
    Ok(())
}

pub(super) fn mutation_metadata(
    state: &State,
    visible: &BTreeSet<f::Reference>,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<(Vec<Value>, bool)> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    let mut truncated = false;
    for reference in visible {
        if !lineage_permitted(state, std::slice::from_ref(reference), a)
            || !seen.insert(reference.source.clone())
        {
            continue;
        }
        if output.len() == MAX_MUTATION_SOURCES {
            truncated = true;
            break;
        }
        let source = &state.sources[&reference.source];
        let stored = record(state, reference)?;
        exposure.sources.push(reference.clone());
        let head = f::Reference::full(&reference.source, &source.head, f::Representation::Utf8);
        let current_head = if lineage_permitted(state, std::slice::from_ref(&head), a) {
            exposure.sources.push(head.clone());
            Some(wire_reference(&head))
        } else {
            None
        };
        output.push(json!({
            "source":reference.source,
            "source_owner":source.owner,
            "source_protected":source.protected,
            "scope":stored.record.scope,
            "current_head":current_head,
            "current_head_status":if current_head.is_some() { "permitted" } else { "unavailable" },
            "metadata_only":true
        }));
    }
    Ok((output, truncated))
}

pub(super) fn accepted_writes(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    allowed: &BTreeSet<String>,
    proposals: &BTreeMap<String, String>,
    now: i64,
) -> Result<()> {
    for operation in &decision.operations {
        let p::Operation::FileWrite { id, source, .. } = operation else {
            continue;
        };
        if !allowed.contains(id) {
            continue;
        }
        let revision = &proposals[&format!("{}{}", p::PROPOSED_OPERATION_PREFIX, id)];
        let entry = &state.sources[source];
        let reference = f::Reference::full(source, revision, f::Representation::Utf8);
        let stored = record(state, &reference)?;
        let payload = json!({
            "operation_id":id,
            "source":wire_reference(&reference),
            "scope":stored.record.scope,
            "source_owner":entry.owner,
            "source_protected":entry.protected,
            "accepted_head":revision,
            "metadata_only":true,
            "selection_changed_by_receipt":false
        });
        // State and receipt are adopted atomically. A later failed operation
        // commits neither. Exact file restrictions follow the receipt.
        let mut receipt_exposure = prepared_exposure(prepared)?.clone();
        receipt_exposure.sources.push(reference);
        let receipt_exposure = warnings::confidentiality(state, &receipt_exposure)?;
        add_observation(
            state,
            prepared,
            id,
            "accepted_file_write",
            payload,
            Vec::new(),
            Vec::new(),
            now,
        )?;
        let observation_id = composite_id(
            "observation",
            &[&prepared.request.id, "accepted_file_write", id],
        );
        state
            .observations
            .get_mut(&observation_id)
            .expect("inserted file receipt")
            .exposure = Some(receipt_exposure);
    }
    Ok(())
}

pub(super) fn adoption_error(
    inv: &s::InvocationRecord,
    evidence: &s::StoredObject,
) -> Result<Value> {
    let prefix = format!("terminal:{} ", evidence.exact_ref()?.sha256);
    let detail = inv
        .history
        .iter()
        .rev()
        .filter(|receipt| {
            receipt.subject == s::EntityId::Invocation(inv.id.clone())
                && receipt.status == s::ReceiptStatus::Failed
        })
        .find_map(|receipt| receipt.detail.as_deref()?.strip_prefix(&prefix));
    Ok(match detail {
        Some(detail) => json!({
            "status":"recorded",
            "detail":safe_error_detail(detail),
            "source":"terminal-bound failed invocation receipt",
            "response_body_supplied":false,
            "replay_authorized":false
        }),
        None => {
            json!({"status":"unavailable","response_body_supplied":false,"replay_authorized":false})
        }
    })
}

// Parser diagnostics can echo rejected field names/values, including embedded
// quote delimiters. Preserve the actual category before the first quoted token;
// never try to recover untrusted quote boundaries from an error string.
fn safe_error_detail(detail: &str) -> String {
    let detail = match detail.find(['`', '"', '\'']) {
        Some(start) => format!(
            "{}[quoted diagnostic details withheld; consult supplied schema]",
            &detail[..start]
        ),
        None => detail.to_owned(),
    };
    bounded_detail(
        &detail
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>(),
        180,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_diagnostic_preserves_static_cause_and_withholds_quoted_tokens() {
        let cause = "held without substantive adoption: stale file base revision";
        assert_eq!(safe_error_detail(cause), cause);
        for cause in [
            "unknown field `PRIVATE`INJECTED`, expected a valid field",
            "invalid value: string \"PRIVATE\\\"INJECTED\", expected a number",
            "unknown variant 'PRIVATE'INJECTED', expected a valid variant",
        ] {
            let detail = safe_error_detail(cause);
            assert!(!detail.contains("PRIVATE"));
            assert!(!detail.contains("INJECTED"));
            assert!(detail.contains("consult supplied schema"));
        }
        let detail = safe_error_detail(&"λ\n".repeat(200));
        assert!(detail.len() <= 180);
        assert!(!detail.contains('\n'));
    }
}
