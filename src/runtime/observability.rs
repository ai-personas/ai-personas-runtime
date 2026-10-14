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
        let observation_id = observation_id(&prepared.request.id, "accepted_file_write", id);
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

/// Render only known host-owned reference slots as actionable Decision ExactRef
/// objects. Never rewrite retained observations, authored text, policies or bindings.
pub(super) fn actionable_reference(value: &mut Value) -> Result<()> {
    let reference: f::Reference = serde_json::from_value(value.clone())?;
    let wire = checked_wire_reference(&reference, "actionable.reference")
        .map_err(|_| held("unsupported Decision reference in qualified context"))?;
    let mut projected = serde_json::to_value(wire)?;
    // Absent lifetime is the existing work default. Avoid adding redundant null
    // metadata to every reference in an already size-admitted complete body.
    projected.as_object_mut().unwrap().remove("lifetime");
    *value = projected;
    Ok(())
}

/// Decode a reference only from an already retained, host-rendered body bundle.
/// The explicit format marker distinguishes new actor projections from historical
/// prepared prompts and local event receipts. This is not a response parser.
pub(super) fn context_reference(bundle: &Value, reference: &Value) -> Result<f::Reference> {
    match bundle.get("reference_format") {
        None => Ok(serde_json::from_value(reference.clone())?),
        Some(Value::String(format)) if format == "decision_exact_ref" => {
            let reference: p::ExactRef = serde_json::from_value(reference.clone())?;
            require(
                reference.lifetime.is_none(),
                "body reference has a selection lifetime",
            )?;
            file_reference(&reference)
        }
        _ => Err(held("unknown retained context reference format")),
    }
}

pub(super) fn actionable_context(value: &mut Value) -> Result<()> {
    let entries = value["entries"]
        .as_array_mut()
        .ok_or_else(|| held("qualified context entries unavailable"))?;
    for entry in entries {
        actionable_reference(&mut entry["reference"])?;
        for role in entry["roles"]
            .as_array_mut()
            .ok_or_else(|| held("qualified context roles unavailable"))?
        {
            if role["role"] == "qualification" {
                actionable_reference(&mut role["required_by"])?;
            }
        }
    }
    value["reference_format"] = json!("decision_exact_ref");
    Ok(())
}

pub(super) fn actionable_discovery(payload: &mut Value) -> Result<()> {
    // Legacy Search retained a JSON-encoded page; Discover retains an object.
    // Preserve that envelope and all cursor, coverage and query metadata.
    let encoded = payload["page"].as_str().is_some();
    let mut page = if let Some(text) = payload["page"].as_str() {
        serde_json::from_str(text)?
    } else {
        payload["page"].clone()
    };
    if let Some(hits) = page.get_mut("hits").and_then(Value::as_array_mut) {
        // Old retained pages can predate admission's representability check.
        // Preserve the stored receipt, but never advertise an uncopyable hit or
        // complete coverage. A fresh supported request is required; continuation
        // of this unsupported representation is not actionable either.
        let unsupported = hits
            .iter()
            .try_fold(false, |unsupported, hit| -> Result<bool> {
                let reference: f::Reference = serde_json::from_value(hit["reference"].clone())?;
                Ok(unsupported
                    || checked_wire_reference(&reference, "discovery.reference").is_err())
            })?;
        if unsupported {
            hits.clear();
            page["status"] = json!("unavailable");
            page["coverage_complete"] = json!(false);
            page["has_more"] = json!(false);
            page["error"] = json!("unsupported_decision_reference");
            payload["status"] = json!("unavailable");
            payload["coverage_complete"] = json!(false);
            payload["continuation_available"] = json!(false);
            payload["next_cursor"] = Value::Null;
            payload["error"] = json!("unsupported_decision_reference");
        } else {
            for hit in hits {
                actionable_reference(&mut hit["reference"])?;
            }
        }
        page["reference_format"] = json!("decision_exact_ref");
        payload["page"] = if encoded {
            Value::String(serde_json::to_string(&page)?)
        } else {
            page
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_context_reference_formats_are_explicit_and_preserve_exact_identity() {
        let mut reference = f::Reference::full(
            "source",
            "revision",
            f::Representation::JsonString {
                pointer: "/body".into(),
            },
        );
        reference.start = Some(1);
        reference.end = Some(3);
        let stored = serde_json::to_value(&reference).unwrap();
        let mut projected = stored.clone();
        actionable_reference(&mut projected).unwrap();
        let legacy = json!({});
        let current = json!({"reference_format":"decision_exact_ref"});
        assert_eq!(context_reference(&legacy, &stored).unwrap(), reference);
        assert_eq!(context_reference(&current, &projected).unwrap(), reference);
        assert!(context_reference(&legacy, &projected).is_err());
        assert!(context_reference(&current, &stored).is_err());
        assert!(context_reference(&json!({"reference_format":"unknown"}), &stored).is_err());
        projected["lifetime"] = json!("next_call");
        assert!(context_reference(&current, &projected).is_err());
    }

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

    #[test]
    fn observation_identity_is_bounded_and_preserves_legacy_short_ids() {
        let invocation = "i".repeat(128);
        let operation = "w".repeat(128);
        let bounded = observation_id(&invocation, "accepted_file_write", &operation);
        p::check_id(&bounded, "id").unwrap();
        assert_eq!(
            bounded,
            observation_id(&invocation, "accepted_file_write", &operation)
        );
        assert_ne!(
            bounded,
            observation_id(&invocation, "accepted_file_write", &"x".repeat(128))
        );
        assert_ne!(
            bounded,
            observation_id(&operation, "accepted_file_write", &invocation)
        );
        assert_ne!(
            bounded,
            observation_id(&invocation, "accepted_artifact_binding", &operation)
        );
        assert_eq!(
            observation_id("short", "discovery", "find"),
            composite_id("observation", &["short", "discovery", "find"])
        );
        let boundary = composite_id(
            "observation",
            &[&invocation, "accepted_file_write", &"x".repeat(82)],
        );
        assert_eq!(boundary.len(), p::MAX_ID_BYTES);
        assert_eq!(
            observation_id(&invocation, "accepted_file_write", &"x".repeat(82)),
            boundary
        );
        assert!(
            observation_id(&invocation, "accepted_file_write", &"x".repeat(83))
                .starts_with("observation-sha256:")
        );
    }

    #[test]
    fn retained_unsupported_discovery_reports_unavailable_without_mutating_receipt() {
        for encoded in [false, true] {
            for pointer in ["/a b".to_owned(), format!("/{}", "a".repeat(244))] {
                let reference = f::Reference::full(
                    "source",
                    "revision",
                    f::Representation::JsonString { pointer },
                );
                let page = json!({"hits":[{"reference":reference}], "coverage_complete":true, "has_more":true, "status":"complete"});
                let retained = json!({"page":if encoded { json!(serde_json::to_string(&page).unwrap()) } else {page},
                    "next_cursor":"discovery:existing", "continuation_available":true, "coverage_complete":true});
                let mut projected = retained.clone();
                actionable_discovery(&mut projected).unwrap();
                assert_eq!(projected["error"], "unsupported_decision_reference");
                assert_eq!(projected["coverage_complete"], false);
                assert_eq!(projected["continuation_available"], false);
                assert!(projected["next_cursor"].is_null());
                let page: Value = if encoded {
                    serde_json::from_str(projected["page"].as_str().unwrap()).unwrap()
                } else {
                    projected["page"].clone()
                };
                assert_eq!(page["hits"], json!([]));
                assert_eq!(page["status"], "unavailable");
                assert_eq!(page["coverage_complete"], false);
                assert_ne!(projected, retained);
            }
        }
    }

    #[test]
    fn message_identity_is_bounded_domain_separated_and_preserves_short_history() {
        let invocation = "i".repeat(128);
        for bytes in [128, 256] {
            let suffix = composite_id("operation", &[&"m".repeat(bytes)]);
            let parts = [invocation.as_str(), suffix.as_str(), "b"];
            let id = bounded_composite_id("message", &parts);
            p::check_id(&id, "message observation").unwrap();
            assert_eq!(id, bounded_composite_id("message", &parts));
            assert_ne!(id, bounded_composite_id("observation", &parts));
            assert_ne!(
                id,
                bounded_composite_id("message", &[&invocation, &suffix, "a"])
            );
        }
        let suffix = composite_id("operation", &["short"]);
        assert_eq!(
            bounded_composite_id("message", &["inv", &suffix, "b"]),
            composite_id("message", &["inv", &suffix, "b"])
        );
    }

    #[test]
    fn unsupported_reference_never_receives_an_actionable_context_marker() {
        for representation in [
            f::Representation::JsonString {
                pointer: "/a b".into(),
            },
            f::Representation::Unsupported {
                name: "utf8".into(),
            },
        ] {
            let reference = f::Reference::full("source", "revision", representation);
            let mut value =
                json!({"entries":[{"reference":reference,"roles":[],"text":"Authored bytes."}]});
            let before = value.clone();
            let error = actionable_context(&mut value).unwrap_err();
            assert!(error.to_string().contains("unsupported Decision reference"));
            assert!(value.get("reference_format").is_none());
            assert_eq!(value, before);
        }
    }
}
