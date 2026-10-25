//! Complete actor-facing receipts and exact mutation/selection metadata.
//! These projections never select, refresh, read a new body, or grant authority.
use super::*;

pub(super) const NAVIGATION_ACCOUNTING_SCOPE: &str =
    "Native navigation only. This record does not report provider usage or whether a model call completed.";

/// A host-authenticated completed transport becomes ordinary pending evidence
/// only if its candidate state is adopted. No authored claim or later effect
/// supplies these fields; the original terminal record remains authoritative.
pub(super) fn model_call_receipt(
    state: &mut State,
    prepared: &Prepared,
    terminal: &s::ObjectRef,
    receipt: &Value,
    now: i64,
) -> Result<()> {
    let mut payload = json!({
        "execution_profile":prepared.request.profile,
        "completion_kind":receipt["kind"],
        "synthetic":receipt["synthetic"],
        "terminal_evidence":terminal,
        "prompt_sha256":prepared.prompt_sha256,
        "response_sha256":receipt["inner_sha256"].as_str()
            .or_else(|| receipt["transport_sha256"].as_str()),
        "requested_model":prepared.requested_model,
        "requested_effort":prepared.requested_effort,
        "reported_model":receipt["reported_model"],
        "reported_effort":receipt["reported_effort"],
        "transport_profile":receipt["audit"]["transport_profile"],
        "provider_request_sha256":receipt["audit"]["request_sha256"],
        "provider_image_inputs":receipt["audit"]["image_inputs"],
        "transport_response_bytes":receipt["transport_response_bytes"],
        "completion_disposition":receipt["completion_disposition"],
        "provider_reported_usage":receipt["usage"],
        "provider_attempt":{
            "dispatch_may_have_begun":receipt["attempt"]["dispatch_may_have_begun"],
            "response_completed":receipt["attempt"]["response_completed"],
            "stream_eof":receipt["attempt"]["stream_eof"],
            "http_status":receipt["attempt"]["http_status"],
            "elapsed_ms":receipt["attempt"]["elapsed_ms"],
            "stream_bytes_observed":receipt["attempt"]["stream_bytes_observed"]
        },
        "scope":"Completion of this named response is separate from any tool effect, inspected feature or later call. Provider identity and usage remain attributed reports; unknown charges remain unknown."
    });
    if let Some(history) = receipt["audit"].get("input_history") {
        payload["provider_input_history"] = history.clone();
    }
    add_observation(state, prepared, "host-model-call", "model_call_receipt",
        payload, Vec::new(), Vec::new(), now)
}

pub(super) fn selection_state(
    state: &State,
    focus: &Focus,
    serial: u64,
    a: &f::Admission,
    visible: &mut BTreeSet<f::Reference>,
) -> Result<Value> {
    let mut entries = |selection: &Option<Vec<Selected>>| -> Result<Vec<Value>> {
        let mut output = Vec::new();
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
        "received_local_operator_feedback" | "received_local_persona_message"
        | "received_child_work_message" => {
            // A citation already identifies this source. Offer its current
            // mutation metadata without replacing the cited immutable revision,
            // reading either body, or treating the rest of its lineage as inventory.
            if let Some(references) = observation.payload["file_references"].as_array() {
                for value in references {
                    let reference: p::ExactRef = serde_json::from_value(value.clone())?;
                    visible.insert(file_reference(&reference)?);
                }
            }
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

/// Resource identities already encountered in the current input. A field or
/// range names the same immutable document for provenance metadata only.
pub(super) struct ResourceContext<'a> {
    revisions: BTreeSet<(&'a str, &'a str)>,
    observations: BTreeSet<&'a str>,
}

impl<'a> ResourceContext<'a> {
    pub(super) fn new(visible: &'a BTreeSet<f::Reference>, observations: &'a [String]) -> Self {
        Self {
            revisions: visible.iter().map(|r| (r.source.as_str(), r.revision.as_str())).collect(),
            observations: observations.iter().map(String::as_str).collect(),
        }
    }

    pub(super) fn references(&self, reference: &p::ExactRef) -> bool {
        self.revisions.contains(&(reference.source.as_str(), reference.revision.as_str()))
    }

    pub(super) fn includes_result(&self, reference: &p::ExactRef, observation: Option<&str>) -> bool {
        self.references(reference) || observation.is_some_and(|id| self.observations.contains(id))
    }
}

pub(super) fn mutation_metadata(
    state: &State,
    visible: &BTreeSet<f::Reference>,
    a: &f::Admission,
    exposure: &mut Exposure,
) -> Result<(Vec<Value>, bool)> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for reference in visible {
        if !lineage_permitted(state, std::slice::from_ref(reference), a)
            || !seen.insert(reference.source.clone())
        {
            continue;
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
            "source_retired":source.retired,
            "scope":stored.record.scope,
            "current_head":current_head,
            "current_head_status":if current_head.is_some() { "permitted" } else { "unavailable" },
            "metadata_only":true
        }));
    }
    Ok((output, false))
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
        let Some(p::FileSave { id, source, .. }) = operation.file_save() else {
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
            "origin":stored.origin,
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
            "detail":detail,
            "source":"terminal-bound failed invocation receipt",
            "response_body_supplied":false,
            "replay_authorized":false
        }),
        None => {
            json!({"status":"unavailable","response_body_supplied":false,"replay_authorized":false})
        }
    })
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
