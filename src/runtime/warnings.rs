//! Explicit primary-authored warnings, independent of content and work closeout.
//!
//! This module stages changes in the enclosing whole-response transaction. It
//! performs no I/O, inference, semantic scanning, dispatch, or resource renewal.
//! Exact receipt files are protected host wrappers around actual authored
//! operations; affected file bytes and every historical receipt stay immutable.

use super::*;

const ATTRIBUTION: &str = "Accepted primary-authored warning lifecycle judgment. Acceptance establishes recorded authorship and disposition, not content correctness, patch success, or correction-work completion. Historical receipts do not override the current accepted binding snapshot.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningAuthorship {
    pub persona: String,
    pub work: String,
    pub invocation: String,
    pub owner_epoch: u64,
    pub prepared_epoch: u64,
    pub character_revision: Option<String>,
    pub profile: Profile,
    pub authority: WorkAuthority,
    pub operation: p::Operation,
    pub exposure: Exposure,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningResolution {
    pub revision: String,
    pub prior_revision: String,
    pub authored: WarningAuthorship,
    pub marker: f::Reference,
    pub bindings: Vec<f::AcceptedBinding>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticWarning {
    pub id: String,
    /// Immutable adoption revision; current_revision follows the appended history.
    pub revision: String,
    pub authored: WarningAuthorship,
    pub marker: f::Reference,
    pub bindings: Vec<f::AcceptedBinding>,
    pub history: Vec<WarningResolution>,
}

impl SemanticWarning {
    pub fn current_revision(&self) -> &str {
        self.history
            .last()
            .map_or(self.revision.as_str(), |event| event.revision.as_str())
    }

    pub fn active(&self) -> bool {
        !self.history.last().is_some_and(|event| {
            matches!(
                event.authored.operation,
                p::Operation::WarningResolve {
                    disposition: p::WarningResolutionDisposition::Release {},
                    ..
                }
            )
        })
    }

    fn current_bindings(&self) -> &[f::AcceptedBinding] {
        self.history
            .last()
            .map_or(self.bindings.as_slice(), |event| event.bindings.as_slice())
    }

    fn targets(&self) -> Result<(p::Scope, &[p::ExactRef])> {
        match &self.authored.operation {
            p::Operation::WarningAdopt { scope, targets, .. } => Ok((*scope, targets)),
            _ => Err(held("warning adoption history has the wrong operation")),
        }
    }

    fn markers(&self) -> Vec<f::Reference> {
        std::iter::once(self.marker.clone())
            .chain(self.history.iter().map(|event| event.marker.clone()))
            .collect()
    }
}

fn same_revision(left: &f::Reference, right: &f::Reference) -> bool {
    left.source == right.source && left.revision == right.revision
}

fn warning_binding(state: &State, id: &str) -> bool {
    state
        .semantic_warnings
        .values()
        .any(|warning| warning.bindings.iter().any(|binding| binding.id == id))
}

fn warning_marker(state: &State, reference: &f::Reference) -> bool {
    state.semantic_warnings.values().any(|warning| {
        same_revision(&warning.marker, reference)
            || warning
                .history
                .iter()
                .any(|event| same_revision(&event.marker, reference))
    })
}

/// Retain exact source ancestry and policy identity independently of warning
/// applicability. Authored policy and warning judgments impose no access gate.
pub(super) fn confidentiality(state: &State, input: &Exposure) -> Result<Exposure> {
    let mut result = input.clone();
    let mut pending = input.sources.clone();
    let mut seen = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if !seen.insert(reference.clone()) {
            continue;
        }
        let stored = record(state, &reference)?;
        if let Some(inherited) = &stored.exposure {
            extend_exposure(&mut result, inherited);
            pending.extend(inherited.sources.iter().cloned());
        } else {
            require(
                revision_importer(&state.sources[&reference.source], stored).is_some(),
                "warning source has unknown exposure lineage",
            )?;
        }
        let restriction = scoped_policy(
            "file",
            &reference.source,
            &reference.revision,
            &stored.record.policy,
        );
        if !result.policies.contains(&restriction) {
            result.policies.push(restriction);
        }
        pending.extend(stored.lineage.iter().cloned());
        pending.extend(stored.record.qualifications.iter().cloned());
        for binding in state.accepted_bindings.iter().filter(|binding| {
            binding.source == reference.source && binding.file_revision == reference.revision
        }) {
            if warning_binding(state, &binding.id) {
                continue;
            }
            let restriction = scoped_policy(
                "accepted-binding",
                &binding.id,
                &binding.revision,
                &binding.policy,
            );
            if !result.policies.contains(&restriction) {
                result.policies.push(restriction);
            }
            if let f::BindingDisposition::Qualify { required } = &binding.disposition {
                pending.extend(required.iter().cloned());
            }
        }
    }
    result.sources = seen.into_iter().collect();
    Ok(result)
}

/// Only protected, exactly identified lifecycle receipts use this projection.
/// Recompute their source confidentiality on every access; flattening here does
/// not remove the stored source references or authorize their payload supply.
pub(super) fn inherited_exposure(
    state: &State,
    reference: &f::Reference,
    inherited: &Exposure,
) -> Result<Exposure> {
    if warning_marker(state, reference) {
        let mut checked = confidentiality(state, inherited)?;
        checked.sources.clear();
        Ok(checked)
    } else {
        Ok(inherited.clone())
    }
}

fn control_and_context(prepared: &Prepared) -> Result<(Value, Value)> {
    let decoded = request_context::decode(prepared)?;
    Ok((decoded.control, decoded.qualified))
}

fn collect_entries(value: &Value, result: &mut BTreeSet<f::Reference>) -> Result<()> {
    if let Some(entries) = value.get("entries").and_then(Value::as_array) {
        for entry in entries {
            result.insert(observability::context_reference(
                value,
                &entry["reference"],
            )?);
        }
    }
    Ok(())
}

fn supplied_bodies(control: &Value, context: &Value) -> Result<BTreeSet<f::Reference>> {
    let mut result = BTreeSet::new();
    collect_entries(context, &mut result)?;
    if let Some(observations) = control["actual_observations"].as_array() {
        for observation in observations {
            if matches!(
                observation["kind"].as_str(),
                Some("qualified_read" | "selection_repair_read" | "local_dependency_event")
            ) {
                collect_entries(&observation["payload"]["result"], &mut result)?;
            }
            if let Some(bundles) = observation["current_required_qualifications"].as_array() {
                for bundle in bundles {
                    collect_entries(bundle, &mut result)?;
                }
            }
        }
    }
    Ok(result)
}

/// Validate exact complete entry bytes and qualification bindings against the
/// retained host-rendered prompt, not a provenance-only reference inventory.
pub(super) fn supplied_bundle_proof(
    prepared: &Prepared,
    expected: &[f::ContextEntry],
) -> Result<Value> {
    fn append(value: &Value, entries: &mut Vec<Value>) -> Result<()> {
        if let Some(values) = value.get("entries").and_then(Value::as_array) {
            for entry in values {
                let mut normalized = entry.clone();
                normalized["reference"] = serde_json::to_value(observability::context_reference(
                    value,
                    &entry["reference"],
                )?)?;
                entries.push(normalized);
            }
        }
        Ok(())
    }
    fn without_roles(mut entry: Value) -> Value {
        // A subject may also be selected as another root's qualification. That
        // role label does not change its bytes or enforced binding restrictions.
        if let Some(object) = entry.as_object_mut() {
            object.remove("roles");
        }
        entry
    }
    let (control, context) = control_and_context(prepared)?;
    let mut actual = Vec::new();
    append(&context, &mut actual)?;
    if let Some(observations) = control["actual_observations"].as_array() {
        for observation in observations {
            if matches!(
                observation["kind"].as_str(),
                Some("qualified_read" | "selection_repair_read" | "local_dependency_event")
            ) {
                append(&observation["payload"]["result"], &mut actual)?;
            }
            if let Some(bundles) = observation["current_required_qualifications"].as_array() {
                for bundle in bundles {
                    append(bundle, &mut actual)?;
                }
            }
        }
    }
    let actual: Vec<_> = actual.into_iter().map(without_roles).collect();
    let mut proof = Vec::new();
    for entry in expected {
        let expected_entry = without_roles(serde_json::to_value(entry)?);
        require(actual.contains(&expected_entry),
            "model evidence requires the exact complete body and qualifications actually supplied to this invocation")?;
        proof.push(json!({"reference":entry.reference,"body_sha256":s::sha256_bytes(entry.text.as_bytes()),
            "accepted_bindings":entry.accepted_bindings,"binding_restrictions":entry.binding_restrictions,
            "policy":entry.policy}));
    }
    Ok(json!({"qualified_bundle_sha256":digest(expected)?,"entries":proof}))
}

fn validate_basis(
    state: &State,
    a: &f::Admission,
    basis: &[p::ExactRef],
    supplied: &BTreeSet<f::Reference>,
) -> Result<Vec<f::Reference>> {
    let corpus = corpus(state, a)?;
    let mut result = Vec::new();
    for reference in basis {
        let reference = file_reference(reference)?;
        require(
            supplied.iter().any(|body| covers(body, &reference)),
            "warning basis was not an exact body supplied to this invocation",
        )?;
        f::assemble(
            &corpus,
            std::slice::from_ref(&reference),
            &[],
            &[],
            a,
            state.config.context_bytes,
            0,
            0,
        )?;
        result.push(reference);
    }
    Ok(result)
}

fn gate(
    state: &State,
    prepared: &Prepared,
    requested_scope: p::Scope,
    targets: &[p::ExactRef],
    now: i64,
) -> Result<()> {
    require(
        prepared.request.mode == Mode::Ordinary,
        "warning changes require ordinary mode",
    )?;
    let actor = &prepared.request.persona;

    let a = admission(&state.personas[actor], now);
    for target in targets {
        let reference = file_reference(target)?;
        let stored = record(state, &reference)?;
        require(
            stored.record.scope == scope(requested_scope),
            "warning target scope mismatch",
        )?;
        require(
            policy_permitted(state, &stored.record.policy, &a),
            "warning target policy unavailable",
        )?;
    }
    Ok(())
}

fn author(
    state: &State,
    prepared: &Prepared,
    operation: &p::Operation,
    extra: &[f::Reference],
    inherited: Option<&Exposure>,
    policy: &f::Policy,
    now: i64,
) -> Result<WarningAuthorship> {
    let persona = &state.personas[&prepared.request.persona];

    let mut exposure = prepared_exposure(prepared)?.clone();
    exposure.sources.extend(extra.iter().cloned());
    if let Some(inherited) = inherited {
        extend_exposure(&mut exposure, inherited);
    }
    let exposure = confidentiality(state, &exposure)?;
    // Preserve exact policy provenance alongside the authored judgment.
    check_output_policy(
        state,
        policy,
        &Exposure {
            sources: Vec::new(),
            policies: exposure.policies.clone(),
        },
        &admission(persona, now),
    )?;
    let work = state
        .board
        .work(&w::WorkId(prepared.request.work.clone()))?;
    Ok(WarningAuthorship {
        persona: prepared.request.persona.clone(),
        work: prepared.request.work.clone(),
        invocation: prepared.request.id.clone(),
        owner_epoch: prepared.owner_epoch,
        prepared_epoch: prepared.epoch,
        character_revision: prepared.character_revision.clone(),
        profile: prepared.request.profile,
        authority: WorkAuthority {
            work: prepared.request.work.clone(),
            version: work.version,
            authority: work.mandate().authority.clone(),
        },
        operation: operation.clone(),
        exposure,
    })
}

fn create_marker(
    state: &mut State,
    warning_id: &str,
    revision: &str,
    prior: Option<&str>,
    authored: &WarningAuthorship,
    requested_scope: p::Scope,
    policy: &f::Policy,
) -> Result<f::Reference> {
    let key = digest(&("semantic-warning-receipt-v1", warning_id, revision))?;
    let source = format!("semantic-warning:{key}");
    require(
        !state.sources.contains_key(&source),
        "warning receipt identity conflict",
    )?;
    let record = f::FileRevision {
        source: source.clone(),
        revision: String::new(),
        path: format!("warnings/{key}.json"),
        scope: scope(requested_scope),
        bytes: serde_json::to_vec(&json!({
            "warning_id":warning_id,"warning_revision":revision,"prior_warning_revision":prior,
            "actor":authored.persona,"origin_work":authored.work,"origin_invocation":authored.invocation,
            "authority":authored.authority,"operation":authored.operation,
            "attribution":ATTRIBUTION,"wrapper":"host rendering of exact accepted authored fields"
        }))?,
        author: authored.persona.clone(),
        policy: policy.clone(),
        qualifications: Vec::new(),
        retired: false,
    }.seal()?;
    let reference = record.reference(f::Representation::Utf8);
    state.sources.insert(
        source,
        Source {
            owner: authored.persona.clone(),
            protected: true,
            retired: false,
            head: record.revision.clone(),
            imported_by: None,
            revisions: vec![ManagedRevision {
                record,
                lineage: Vec::new(),
                qualification_sources: None,
                exposure: Some(authored.exposure.clone()),
                origin: Some(RevisionOrigin::PersonaInvocation {
                    persona: authored.persona.clone(),
                    invocation_id: authored.invocation.clone(),
                    operation_id: authored.operation.id().into(),
                }),
            }],
        },
    );
    Ok(reference)
}

fn combined_exposure(warning: &SemanticWarning) -> Exposure {
    let mut result = warning.authored.exposure.clone();
    for event in &warning.history {
        extend_exposure(&mut result, &event.authored.exposure);
    }
    result.sources.extend(warning.markers());
    result
}

fn visible(state: &State, warning: &SemanticWarning, a: &f::Admission) -> bool {
    confidentiality(state, &combined_exposure(warning)).is_ok_and(|exposure| {
        exposure
            .policies
            .iter()
            .all(|restriction| policy_permitted(state, &restriction.policy, a))
    })
}

// Borrow authored fields and stream history so capped serialization can reject
// a diagnostic before allocating its expanded JSON views or history copies.
#[derive(Serialize)]
struct WarningView<'a> {
    id: &'a str,
    revision: &'a str,
    adoption_revision: &'a str,
    active: bool,
    actor: &'a str,
    origin_work: &'a str,
    origin_invocation: &'a str,
    authority: &'a WorkAuthority,
    operation: &'a p::Operation,
    receipt: &'a f::Reference,
    #[serde(serialize_with = "serialize_resolution_views")]
    resolutions: &'a [WarningResolution],
    attribution: &'static str,
    release_rule: &'static str,
}

fn serialize_resolution_views<S>(
    history: &[WarningResolution],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;

    #[derive(Serialize)]
    struct ResolutionView<'a> {
        revision: &'a str,
        prior_revision: &'a str,
        actor: &'a str,
        work: &'a str,
        invocation: &'a str,
        authority: &'a WorkAuthority,
        operation: &'a p::Operation,
        receipt: &'a f::Reference,
    }

    let mut sequence = serializer.serialize_seq(Some(history.len()))?;
    for event in history {
        sequence.serialize_element(&ResolutionView {
            revision: &event.revision,
            prior_revision: &event.prior_revision,
            actor: &event.authored.persona,
            work: &event.authored.work,
            invocation: &event.authored.invocation,
            authority: &event.authored.authority,
            operation: &event.authored.operation,
            receipt: &event.marker,
        })?;
    }
    sequence.end()
}

fn view(warning: &SemanticWarning) -> WarningView<'_> {
    WarningView {
        id: &warning.id,
        revision: warning.current_revision(),
        adoption_revision: &warning.revision,
        active: warning.active(),
        actor: &warning.authored.persona,
        origin_work: &warning.authored.work,
        origin_invocation: &warning.authored.invocation,
        authority: &warning.authored.authority,
        operation: &warning.authored.operation,
        receipt: &warning.marker,
        resolutions: &warning.history,
        attribution: ATTRIBUTION,
        release_rule: "release removes only this warning disposition; the exact accepted resolution receipt remains required",
    }
}

pub(super) fn context(
    state: &State,
    a: &f::Admission,
    work: &str,
    serial: u64,
    observed: &BTreeSet<f::Reference>,
) -> Result<(Vec<Value>, Vec<f::Reference>)> {
    let persona = &state.personas[&a.principal];
    let focus = &persona.focus[work];
    let mut relevant = observed.clone();
    relevant.extend(character_refs(persona)?);
    for selected in active_selection(&focus.fragments, serial)
        .into_iter()
        .chain(active_selection(&focus.assets, serial))
    {
        relevant.insert(file_reference(&selected.reference)?);
    }
    let mut result = Vec::new();
    let mut sources = BTreeSet::new();
    for warning in state.semantic_warnings.values() {
        let (_, targets) = warning.targets()?;
        let concerned = warning.authored.persona == a.principal && warning.authored.work == work
            || targets.iter().any(|target| {
                relevant
                    .iter()
                    .any(|r| r.source == target.source && r.revision == target.revision)
            })
            || warning
                .markers()
                .iter()
                .any(|marker| relevant.iter().any(|r| same_revision(marker, r)));
        if concerned && visible(state, warning, a) {
            result.push(serde_json::to_value(view(warning))?);
            sources.extend(warning.markers());
        }
    }
    Ok((result, sources.into_iter().collect()))
}

/// Return the complete retained warning account for an unavailable target.
pub(super) fn unavailable_read(
    state: &State,
    reference: &f::Reference,
    a: &f::Admission,
    navigation: &mut f::NavigationBudget,
    byte_limit: usize,
) -> Result<(Value, Vec<f::Reference>)> {
    let unavailable = || {
        (
            json!({"status":"unavailable","text_supplied":false,"semantic_warnings":[]}),
            Vec::new(),
        )
    };
    let mut selected = Vec::new();
    for warning in state.semantic_warnings.values() {
        if warning.targets()?.1.iter().any(|target| {
            target.source == reference.source && target.revision == reference.revision
        }) && visible(state, warning, a)
        {
            selected.push(warning);
        }
    }
    if selected.is_empty() {
        return Ok(unavailable());
    }
    #[derive(Serialize)]
    struct Diagnostic<'a> {
        status: &'static str,
        text_supplied: bool,
        semantic_warnings: Vec<WarningView<'a>>,
    }
    let diagnostic = Diagnostic {
        status: "unavailable",
        text_supplied: false,
        semantic_warnings: selected.iter().copied().map(view).collect(),
    };
    // Count the complete payload, including every history entry and receipt
    // reference. No partial warning set or uncharged expanded view is staged.
    if !navigation.charge_serialized_result(&diagnostic, byte_limit) {
        return Ok(unavailable());
    }
    let payload = serde_json::to_value(&diagnostic)?;
    let sources = selected
        .into_iter()
        .flat_map(SemanticWarning::markers)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((payload, sources))
}

pub(super) fn adoption_reason(decision: &p::Decision) -> Option<String> {
    decision
        .operations
        .iter()
        .any(|operation| {
            matches!(
                operation,
                p::Operation::WarningAdopt { .. } | p::Operation::WarningResolve { .. }
            )
        })
        .then(|| ATTRIBUTION.into())
}

pub(super) fn apply(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
) -> Result<()> {
    if adoption_reason(decision).is_none() {
        return Ok(());
    }
    require(
        prepared.request.mode == Mode::Ordinary,
        "repair cannot author or resolve warnings",
    )?;
    let (control, context) = control_and_context(prepared)?;
    let supplied = supplied_bodies(&control, &context)?;
    let a = admission(&state.personas[&prepared.request.persona], now);
    for operation in &decision.operations {
        match operation {
            p::Operation::WarningAdopt {
                warning_id,
                scope: requested_scope,
                targets,
                basis,
                disposition,
                policy_id,
                ..
            } => {
                require(
                    !state.semantic_warnings.contains_key(warning_id),
                    "warning identity already has immutable history",
                )?;
                gate(state, prepared, *requested_scope, targets, now)?;
                let mut extra = validate_basis(state, &a, basis, &supplied)?;
                extra.extend(
                    targets
                        .iter()
                        .map(file_reference)
                        .collect::<Result<Vec<_>>>()?,
                );
                let mut required = Vec::new();
                if let p::WarningDisposition::Qualify {
                    required: references,
                } = disposition
                {
                    let corpus = corpus(state, &a)?;
                    for reference in references {
                        let reference = file_reference(reference)?;
                        f::assemble(
                            &corpus,
                            std::slice::from_ref(&reference),
                            &[],
                            &[],
                            &a,
                            state.config.context_bytes,
                            0,
                            0,
                        )?;
                        required.push(reference);
                    }
                }
                extra.extend(required.iter().cloned());
                let policy = current_policy(state, policy_id)?.clone();
                let authored = author(state, prepared, operation, &extra, None, &policy, now)?;
                let revision = digest(&("semantic-warning-v1", warning_id, &authored))?;
                let marker = create_marker(
                    state,
                    warning_id,
                    &revision,
                    None,
                    &authored,
                    *requested_scope,
                    &policy,
                )?;
                let mut bindings = Vec::new();
                for (index, target) in targets.iter().enumerate() {
                    let disposition = match disposition {
                        p::WarningDisposition::Qualify { .. } => {
                            let mut accompaniment = vec![marker.clone()];
                            accompaniment.extend(required.iter().cloned());
                            f::BindingDisposition::Qualify {
                                required: accompaniment,
                            }
                        }
                        p::WarningDisposition::Ineligible { reason } => {
                            f::BindingDisposition::Ineligible {
                                reason: reason.clone(),
                            }
                        }
                    };
                    let binding = f::AcceptedBinding {
                        id: format!("semantic-warning-binding:{}", digest(&(warning_id, index))?),
                        revision: String::new(),
                        source: target.source.clone(),
                        file_revision: target.revision.clone(),
                        policy: policy.clone(),
                        disposition,
                    }
                    .seal()?;
                    require(
                        !state
                            .accepted_bindings
                            .iter()
                            .any(|old| old.id == binding.id),
                        "warning binding identity conflict",
                    )?;
                    bindings.push(binding);
                }
                state.accepted_bindings.extend(bindings.iter().cloned());
                state.semantic_warnings.insert(
                    warning_id.clone(),
                    SemanticWarning {
                        id: warning_id.clone(),
                        revision,
                        authored,
                        marker,
                        bindings,
                        history: Vec::new(),
                    },
                );
            }
            p::Operation::WarningResolve {
                warning_id,
                warning_revision,
                scope: requested_scope,
                basis,
                disposition,
                ..
            } => {
                let warning = state
                    .semantic_warnings
                    .get(warning_id)
                    .cloned()
                    .ok_or_else(|| held("warning history unavailable"))?;
                require(
                    warning.current_revision() == warning_revision,
                    "stale warning resolution revision",
                )?;
                require(
                    warning.active(),
                    "warning already released; a new judgment requires a new identity",
                )?;
                require(
                    control["semantic_warnings"]
                        .as_array()
                        .is_some_and(|entries| {
                            entries.iter().any(|entry| {
                                entry["id"].as_str() == Some(warning_id.as_str())
                                    && entry["revision"].as_str() == Some(warning_revision.as_str())
                            })
                        }),
                    "exact current warning revision was not supplied to this invocation",
                )?;
                let (original_scope, targets) = warning.targets()?;
                require(
                    original_scope == *requested_scope,
                    "warning resolution scope conflict",
                )?;
                gate(state, prepared, *requested_scope, targets, now)?;
                let extra = validate_basis(state, &a, basis, &supplied)?;
                let previous = warning.current_bindings();
                for binding in previous {
                    require(
                        state
                            .accepted_bindings
                            .iter()
                            .filter(|current| current.id == binding.id)
                            .count()
                            == 1
                            && state.accepted_bindings.contains(binding),
                        "warning binding snapshot conflicts with immutable history",
                    )?;
                }
                let policy = previous
                    .first()
                    .ok_or_else(|| held("warning has no target bindings"))?
                    .policy
                    .clone();
                let inherited = combined_exposure(&warning);
                let authored = author(
                    state,
                    prepared,
                    operation,
                    &extra,
                    Some(&inherited),
                    &policy,
                    now,
                )?;
                let revision = digest(&(
                    "semantic-warning-resolution-v1",
                    warning_id,
                    warning_revision,
                    &authored,
                ))?;
                let marker = create_marker(
                    state,
                    warning_id,
                    &revision,
                    Some(warning_revision),
                    &authored,
                    *requested_scope,
                    &policy,
                )?;
                let mut bindings = Vec::new();
                for previous in previous {
                    let mut binding = previous.clone();
                    binding.disposition = match disposition {
                        p::WarningResolutionDisposition::Release {} => {
                            f::BindingDisposition::Qualify {
                                required: vec![marker.clone()],
                            }
                        }
                        p::WarningResolutionDisposition::Retain {} => match &previous.disposition {
                            f::BindingDisposition::Qualify { required } => {
                                let mut required = required.clone();
                                required.push(marker.clone());
                                f::BindingDisposition::Qualify { required }
                            }
                            f::BindingDisposition::Ineligible { reason } => {
                                f::BindingDisposition::Ineligible {
                                    reason: reason.clone(),
                                }
                            }
                            _ => return Err(held("unsupported warning binding disposition")),
                        },
                    };
                    bindings.push(binding.seal()?);
                }
                state
                    .accepted_bindings
                    .retain(|binding| !previous.iter().any(|old| old.id == binding.id));
                state.accepted_bindings.extend(bindings.iter().cloned());
                state
                    .semantic_warnings
                    .get_mut(warning_id)
                    .unwrap()
                    .history
                    .push(WarningResolution {
                        revision,
                        prior_revision: warning_revision.clone(),
                        authored,
                        marker,
                        bindings,
                    });
            }
            _ => (),
        }
    }
    validate_inventory(state)
}
