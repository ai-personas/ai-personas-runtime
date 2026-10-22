//! Exact native write-conflict facts from the write plan. The rejected
//! response stays rejected; only a fresh, independently admitted decision acts.
use super::*;

#[derive(Debug)]
pub struct NativeWriteConflict {
    diagnostic: Diagnostic,
    additional_exposure: Exposure,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Diagnostic {
    ScopedPath {
        scope: f::Scope,
        path: String,
        targets: [Target; 2],
    },
    StaleBase {
        source: String,
        operation: String,
        supplied_revision: Option<String>,
        current_revision: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    source: String,
    operation: Option<String>,
    current_revision: Option<String>,
}

impl std::fmt::Display for NativeWriteConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match &self.diagnostic {
            Diagnostic::ScopedPath { .. } => "active scoped path conflict",
            Diagnostic::StaleBase { .. } => "stale file base revision",
        })
    }
}

pub(super) fn stale_base(
    original: &State,
    candidate: &State,
    prepared: &Prepared,
    operation: &p::Operation,
) -> RuntimeError {
    let failure = (|| {
        prepared.exposure.as_ref()?;
        let Some(p::FileSave {
            id,
            source,
            base_revision,
            ..
        }) = operation.file_save()
        else {
            return None;
        };
        let current = original.sources.get(source)?;
        let staged = candidate.sources.get(source)?;
        // A prior write in this same rejected response might have moved its head.
        // Such response-local conflicts do not acquire an accepted-base diagnostic.
        if current.head != staged.head
            || base_revision.as_ref() == Some(&current.head)
        {
            return None;
        }
        let roots = Exposure {
            sources: vec![f::Reference::full(
                source,
                &current.head,
                f::Representation::Utf8,
            )],
            policies: Vec::new(),
        };
        Some(NativeWriteConflict {
            diagnostic: Diagnostic::StaleBase {
                source: source.clone(),
                operation: id.clone(),
                supplied_revision: base_revision.clone(),
                current_revision: current.head.clone(),
            },
            additional_exposure: expanded_exposure(original, &roots).ok()?,
        })
    })();
    failure
        .map(|failure| RuntimeError::NativeWriteConflict(Box::new(failure)))
        .unwrap_or_else(|| held("stale file base revision"))
}

// Called only where apply_writes already rejects the final active path map,
// after allowed writes and retirements have been staged and checked. This
// preserves failure ordering, moves, swaps and retire-and-replace decisions.
pub(super) fn rejection(
    original: &State,
    candidate: &State,
    prepared: &Prepared,
    writes: &[&p::Operation],
    sources: [&str; 2],
) -> RuntimeError {
    diagnose(original, candidate, prepared, writes, sources)
        .map(|failure| RuntimeError::NativeWriteConflict(Box::new(failure)))
        .unwrap_or_else(|| held("active scoped path conflict"))
}

fn diagnose(
    original: &State,
    candidate: &State,
    prepared: &Prepared,
    writes: &[&p::Operation],
    sources: [&str; 2],
) -> Option<NativeWriteConflict> {
    prepared.exposure.as_ref()?;
    let mut roots = Exposure::default();
    let mut path = None;
    let mut targets = Vec::new();
    for id in sources {
        let source = candidate.sources.get(id)?;
        if source.retired {
            return None;
        }
        let head = source
            .revisions
            .iter()
            .find(|r| r.record.revision == source.head)?;
        let location = (head.record.scope, head.record.path.clone());
        if path.as_ref().is_some_and(|p| p != &location) {
            return None;
        }
        path = Some(location);
        let mut matching = writes.iter().filter_map(|op| {
            let p::FileSave { id:operation, source, scope:requested, path, .. } = op.file_save()?;
            (source == id).then_some((operation, requested, path))
        });
        let operation = matching.next();
        if matching.next().is_some() {
            return None;
        }
        if let Some((_, requested, path)) = operation {
            if scope(requested) != head.record.scope || *path != head.record.path {
                return None;
            }
        }
        let current_revision = if let Some(current) = original.sources.get(id) {
            // An unchanged occupant's path comes from this exact accepted head.
            // A changed occupant's proposed path comes from the rejected response;
            // its old head is still the source of the reported current revision.
            if operation.is_none() && current.head != source.head {
                return None;
            }
            roots.sources.push(f::Reference::full(
                id,
                &current.head,
                f::Representation::Utf8,
            ));
            Some(current.head.clone())
        } else {
            operation?;
            None
        };
        targets.push(Target {
            source: id.into(),
            operation: operation.map(|(id, _, _)| id.clone()),
            current_revision,
        });
    }
    // Do not reclassify an unrelated preexisting inventory conflict.
    if targets.iter().all(|target| target.operation.is_none()) {
        return None;
    }
    let (scope, path) = path?;
    Some(NativeWriteConflict {
        diagnostic: Diagnostic::ScopedPath {
            scope,
            path,
            targets: targets.try_into().ok()?,
        },
        // Expand against the original accepted state, never against unadopted
        // revisions or bindings. No file body enters the diagnostic.
        additional_exposure: expanded_exposure(original, &roots).ok()?,
    })
}

impl NativeWriteConflict {
    pub(super) fn context(
        &self,
        terminal_sha256: &str,
        invocation: &s::InvocationRecord,
        prepared: &Prepared,
    ) -> Value {
        let mut exposure = PlanningExposure::new(prepared.exposure.as_ref());
        exposure.include(&self.additional_exposure);
        let mut value = exposure.context(terminal_sha256, invocation, prepared, None);
        if value["kind"] != "known_admitted_v1" {
            return value;
        }
        let object = value.as_object_mut().expect("typed failure context");
        object.remove("diagnostic");
        object.insert("kind".into(), json!("native_write_conflict_v1"));
        object.insert("conflict".into(), json!(self.diagnostic));
        value
    }
}

pub(super) fn observation(
    invocation: &s::InvocationRecord,
    evidence: &s::StoredObject,
    prepared: &Prepared,
    now: i64,
) -> Result<Option<Observation>> {
    let Some(RetainedFailureContext::NativeWriteConflict {
        terminal_sha256,
        conflict,
        ..
    }) = invocation
        .runtime_failure_context
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok())
    else {
        return Ok(None);
    };
    let terminal = evidence.exact_ref()?;
    let receipt = &evidence.body["receipt"];
    let completed = match prepared.request.profile {
        Profile::Synthetic => {
            receipt["kind"] == "synthetic_complete" && receipt["synthetic"] == true
        }
        Profile::Direct | Profile::NonconformantResearch => {
            receipt["kind"] == "provider_complete"
                && receipt["synthetic"] == false
                && receipt["completion_disposition"] == "ready"
                && receipt["attempt"]["response_completed"] == true
                && receipt["attempt"]["stream_eof"] == true
                && prepared.provider_preparation.as_ref() == Some(&receipt["audit"])
                && receipt["audit"]["invocation_id"] == prepared.request.id
                && receipt["audit"]["prompt_sha256"] == prepared.prompt_sha256
        }
        Profile::Strict => false,
    };
    let prefix = format!("terminal:{} ", terminal.sha256);
    let failed = invocation.history.iter().any(|record| {
        record.subject == s::EntityId::Invocation(invocation.id.clone())
            && record.status == s::ReceiptStatus::Failed
            && record
                .detail
                .as_ref()
                .is_some_and(|detail| detail.starts_with(&prefix))
    });
    let response_bound = evidence.body["response_body"]["text"]
        .as_str()
        .is_some_and(|body| {
            let hash = s::sha256_bytes(body.as_bytes());
            invocation.response_hash.as_deref() == Some(hash.as_str())
                && receipt["inner_sha256"]
                    .as_str()
                    .or_else(|| receipt["transport_sha256"].as_str())
                    == Some(hash.as_str())
        });
    if invocation.state != s::InvocationState::Failed
        || invocation.id != prepared.request.id
        || prepared.request.mode != Mode::Ordinary
        || evidence.kind != "runtime-terminal-transport"
        || terminal.id != terminal_id(&invocation.id)
        || terminal.revision != 1
        || terminal.sha256 != terminal_sha256
        || !completed
        || !failed
        || !response_bound
        || evidence.body["invocation_id"] != prepared.request.id
        || evidence.body["prepare_hash"] != invocation.prepare_hash
        || evidence.body["prepared_epoch"] != prepared.epoch
        || evidence.body["prompt_sha256"] != prepared.prompt_sha256
        || s::sha256_bytes(prepared.prompt.as_bytes()) != prepared.prompt_sha256
    {
        return Ok(None);
    }
    let Some(exposure) = retained_failure_exposure(invocation, &terminal.sha256, prepared) else {
        return Ok(None);
    };
    let (code, detail) = match &conflict {
        Diagnostic::ScopedPath { .. } => (
            "native_scoped_path_conflict_v1",
            "Two active native sources would have occupied the same path in this persona's scope. Source identity determines creation versus revision. To update an existing file, use that source and its exact current base revision after checking current state. To create a separate file, choose an unused scoped path.",
        ),
        Diagnostic::StaleBase { .. } => (
            "native_file_base_conflict_v1",
            "The file save targeted an existing source without its exact accepted head as base_revision. Null means a new source, not use the latest revision. Check the current source before revising it, and supply its exact accepted base revision. If you intend a separate file, choose a new source and an unused scoped path.",
        ),
    };
    Ok(Some(Observation {
        id: composite_id("native-write-correction", &[&invocation.id]),
        actor: prepared.request.persona.clone(),
        work: prepared.request.work.clone(),
        invocation: Some(invocation.id.clone()),
        kind: "native_write_correction".into(),
        at_unix: now,
        payload: json!({
            "code":code,"terminal":terminal,
            "at_rejection":conflict,
            "detail":detail,
            "scope":"The metadata describes the rejected plan and accepted revisions at that time; it does not reserve a path or establish current revisions. No operation in the rejected decision was adopted. If currently authorized, return a complete fresh Decision.",
            "authority":"This diagnostic grants no continuation, extra resources or replay authorization.",
            "response_body_supplied":false,"replay_authorized":false,
            "source_permissions_rechecked_on_supply":true
        }),
        sources: prepared.sources.clone(),
        exposure: Some(exposure),
        qualified_roots: Vec::new(),
        qualification_sources: None,
    }))
}
