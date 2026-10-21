//! Explicit retrieval of retained observations and immutable adopted authored
//! Decisions. Receipts persist references only; fresh prompts re-project bodies.
use super::*;

pub(crate) const INTERFACE: &str = "retained-record-history-v1";

#[derive(Default)]
pub(crate) struct Material {
    archives: BTreeMap<String, (s::DecisionRecord, s::StoredObject)>,
}
impl Material {
    fn insert(&mut self, entries: Vec<(s::DecisionRecord, s::StoredObject)>) -> bool {
        let mut next = self.archives.clone();
        for (proof, object) in entries {
            next.insert(proof.invocation_id.clone(), (proof, object));
        }
        if next.len() > MAX_CURSOR_MEMBERS || bounded(&next, INPUT_LIMIT).is_none() {
            return false;
        }
        self.archives = next;
        true
    }

    pub(crate) fn pending(
        store: &mut s::Store,
        state: &State,
        actor: &str,
        work: &str,
        now: i64,
    ) -> Self {
        let mut material = Self::default();
        let Some(persona) = state.personas.get(actor) else {
            return material;
        };
        let Some(focus) = persona.focus.get(work) else {
            return material;
        };
        if !focus.pending_observations.iter().any(|id| {
            state.observations.get(id).is_some_and(|o| {
                o.kind == "history_record_read"
                    && o.payload["reference"]["kind"] == "adopted_decision"
            })
        }) {
            return material;
        }
        let a = admission(persona, now);
        let (Ok(permissions), Ok(corpus)) = (Permissions::new(state, &a), corpus(state, &a)) else {
            return material;
        };
        let references: Vec<_> = focus
            .pending_observations
            .iter()
            .filter_map(|id| {
                let observation = state.observations.get(id)?;
                if observation.actor != actor
                    || observation.work != work
                    || observation.kind != "history_record_read"
                {
                    return None;
                }
                permissions.exposure(observation.exposure.as_ref()?)?;
                let reference: p::HistoryRecordReference =
                    serde_json::from_value(observation.payload["reference"].clone()).ok()?;
                p::check_history_reference(&reference, "history.pending").ok()?;
                matches!(reference, p::HistoryRecordReference::AdoptedDecision { .. })
                    .then_some(reference)
            })
            .take(MAX_CURSORS + 1)
            .collect();
        if references.is_empty() || references.len() > MAX_CURSORS {
            return material;
        }
        let ids = references
            .iter()
            .filter_map(|reference| match reference {
                p::HistoryRecordReference::AdoptedDecision { invocation_id, .. } => {
                    Some(invocation_id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if let Ok(Some(entries)) = store.history_archives(Some(&ids)) {
            let batch = Self {
                archives: entries
                    .into_iter()
                    .map(|(proof, object)| (proof.invocation_id.clone(), (proof, object)))
                    .collect(),
            };
            let permitted = references
                .iter()
                .filter_map(|reference| {
                    let candidate = record(
                        state,
                        actor,
                        reference,
                        &a,
                        &corpus,
                        &batch,
                        &permissions,
                        false,
                    )?;
                    match candidate.reference {
                        p::HistoryRecordReference::AdoptedDecision { invocation_id, .. } => {
                            batch.archives.get(&invocation_id).cloned()
                        }
                        _ => None,
                    }
                })
                .collect();
            material.insert(permitted);
        }
        material
    }
}

struct Record {
    reference: p::HistoryRecordReference,
    metadata: Value,
    body: Value,
    exposure: Exposure,
    roots: Vec<f::Reference>,
    root_groups: Vec<Vec<f::Reference>>,
    body_bytes: usize,
}
fn key(reference: &p::HistoryRecordReference) -> String {
    match reference {
        p::HistoryRecordReference::Observation { observation_id, .. } => {
            format!("observation:{observation_id}")
        }
        p::HistoryRecordReference::AdoptedDecision { invocation_id, .. } => {
            format!("decision:{invocation_id}")
        }
    }
}

fn observation_record(
    state: &State,
    actor: &str,
    id: &str,
    a: &f::Admission,
    corpus: &f::Corpus,
    permissions: &Permissions<'_>,
    full: bool,
) -> Option<Record> {
    let observation = state.observations.get(id)?;
    if observation.id != id
        || observation.actor != actor
        || p::check_history_observation_id(id, "history.observation_id").is_err()
    {
        return None;
    }
    // Value objects use serde_json's sorted maps. The domain and complete retained
    // record (including original restrictions) define the immutable digest.
    let canonical = serde_json::to_value(observation).ok()?;
    let bytes = bounded(&("history-observation-record-v1", canonical), INPUT_LIMIT)?;
    let reference = p::HistoryRecordReference::Observation {
        observation_id: id.into(),
        sha256: s::sha256_bytes(&bytes),
    };
    let mut original_exposure = observation.exposure.clone()?;
    original_exposure
        .sources
        .extend(observation.sources.iter().cloned());
    original_exposure
        .sources
        .extend(observation.qualified_roots.iter().cloned());
    let exposure = permissions.exposure(&original_exposure)?;
    let (body, exposure, roots) = if full {
        observation_projection::project(state, observation, a, corpus).ok()?
    } else {
        (Value::Null, exposure, observation.qualified_roots.clone())
    };
    let metadata = json!({"reference":reference,"actor":observation.actor,"work":observation.work,
        "producing_invocation":observation.invocation,"kind":observation.kind,"at_unix":observation.at_unix,
        "consuming_invocation":"not inferred from producing invocation"});
    Some(Record {
        reference,
        metadata,
        body,
        exposure,
        roots,
        root_groups: observation
            .qualified_roots
            .iter()
            .cloned()
            .map(|r| vec![r])
            .collect(),
        body_bytes: bytes.len(),
    })
}

#[allow(clippy::too_many_arguments)] // Explicit current projection and immutable material, never ambient state.
fn decision_record(
    state: &State,
    actor: &str,
    id: &str,
    a: &f::Admission,
    corpus: &f::Corpus,
    material: &Material,
    permissions: &Permissions<'_>,
    full: bool,
) -> Option<Record> {
    let (proof, object) = material.archives.get(id)?;
    let reference = object.exact_ref().ok()?;
    if proof.invocation_id != id
        || object.id != archive_id(id)
        || object.revision != 1
        || object.kind != "adopted-persona-response"
        || !proof.objects.contains(&reference)
    {
        return None;
    }
    let archive: AdoptedResponse = serde_json::from_value(object.body.clone()).ok()?;
    if archive.actor != actor
        || archive.response.invocation_id != id
        || archive.response.epoch != proof.previous_epoch
        || archive.report.invocation_id != id
        || archive.report.accepted_epoch != Some(proof.accepted_epoch)
        || archive.response_hash != proof.response_hash
        || !matches!(
            archive.report.status,
            AdoptionStatus::Adopted | AdoptionStatus::CharacterOnly
        )
        || !state.personas.contains_key(&archive.actor)
        || !state.work_meta.contains_key(&archive.work)
    {
        return None;
    }
    let mut exposure = permissions.exposure(archive.exposure.as_ref()?)?;
    let authored = archive.authored_json.as_ref()?;
    if s::sha256_bytes(authored.as_bytes()) != archive.response_hash {
        return None;
    }
    let parsed = p::parse_decision(authored.as_bytes(), INPUT_LIMIT).ok()?;
    if serde_json::to_value(parsed).ok()? != serde_json::to_value(&archive.response).ok()? {
        return None;
    }
    let typed = p::HistoryRecordReference::AdoptedDecision {
        invocation_id: id.into(),
        archive_id: reference.id,
        revision: reference.revision,
        sha256: reference.sha256,
        response_sha256: archive.response_hash,
    };
    let metadata = json!({"reference":typed,"actor":archive.actor,"work":archive.work,"invocation":id,
        "record_type":"adopted_authored_decision", "operation_dispositions":"unavailable_original_disclosure_provenance"});
    // Prepared exposure covers authored content, not arbitrary later host/provider
    // diagnostics. Do not promote authored intentions to executed operations.
    let mut roots = archive.exposure.as_ref()?.sources.clone();
    roots.sort();
    roots.dedup();
    let body = if full {
        let accompaniment = if roots.is_empty() {
            Value::Null
        } else {
            let bundle = f::assemble(
                corpus,
                &roots,
                &[],
                &[],
                a,
                state.config.context_bytes,
                0,
                0,
            )
            .ok()?;
            let references = bundle
                .entries
                .iter()
                .map(|e| e.reference.clone())
                .collect::<Vec<_>>();
            exposure.sources.extend(references.iter().cloned());
            let context: Value = serde_json::from_str(
                &attributed_context(state, &bundle.rendered_context, &references).ok()?,
            )
            .ok()?;
            context
        };
        json!({"actor":archive.actor,"work":archive.work,"invocation":id,
            "authored_json":authored,"current_accompaniment":accompaniment,
            "accompaniment_proves_original_supply_or_use":false,
            "operation_dispositions":"unavailable_original_disclosure_provenance",
            "authorship_establishes_execution":false,"provider_metadata_supplied":false})
    } else {
        Value::Null
    };
    Some(Record {
        reference: typed,
        metadata,
        body,
        exposure,
        root_groups: if roots.is_empty() {
            Vec::new()
        } else {
            vec![roots.clone()]
        },
        roots,
        body_bytes: bounded(object, INPUT_LIMIT)?.len(),
    })
}

#[allow(clippy::too_many_arguments)] // Both record namespaces share the same checked projection inputs.
fn record(
    state: &State,
    actor: &str,
    reference: &p::HistoryRecordReference,
    a: &f::Admission,
    corpus: &f::Corpus,
    material: &Material,
    permissions: &Permissions<'_>,
    full: bool,
) -> Option<Record> {
    let candidate = match reference {
        p::HistoryRecordReference::Observation { observation_id, .. } => {
            observation_record(state, actor, observation_id, a, corpus, permissions, full)
        }
        p::HistoryRecordReference::AdoptedDecision { invocation_id, .. } => decision_record(
            state,
            actor,
            invocation_id,
            a,
            corpus,
            material,
            permissions,
            full,
        ),
    }?;
    (candidate.reference == *reference).then_some(candidate)
}

/// Aggregate per-render projection envelope, separate from retained bodies and
/// shared across all history receipts. Exceeding it supplies unavailable status.
pub(crate) fn render_permissions<'a>(
    state: &'a State,
    admission: &'a f::Admission,
) -> Option<Permissions<'a>> {
    Permissions::new(state, admission).ok()
}

pub(crate) fn supply_budget() -> f::NavigationBudget {
    f::NavigationBudget::new(f::NavigationLimits {
        calls: 0,
        scanned_bytes: PROJECTION_BYTES,
        inspected_revisions: PROJECTION_REVISIONS * 2 + 1,
        matches: 0,
        result_bytes: 0,
    })
}

fn reserve_projection(
    state: &State,
    descriptor: &Record,
    navigation: &mut f::NavigationBudget,
) -> bool {
    let assemblies = match descriptor.reference {
        p::HistoryRecordReference::Observation { .. } => descriptor.roots.len(),
        p::HistoryRecordReference::AdoptedDecision { .. } => {
            usize::from(!descriptor.roots.is_empty())
        }
    };
    navigation.charge(f::NavigationUsage {
        scanned_bytes: state
            .config
            .corpus
            .max_closure_bytes
            .saturating_mul(assemblies)
            .saturating_add(descriptor.body_bytes.saturating_mul(2))
            .saturating_add(CONTROL_BYTES),
        inspected_revisions: state
            .config
            .corpus
            .max_closure_references
            .saturating_add(state.config.corpus.max_closure_edges)
            .saturating_mul(assemblies)
            .saturating_add(1),
        ..Default::default()
    })
}

fn status(status: &str) -> Output {
    let mut output = Output::status(status);
    output.payload["profile"] = json!(INTERFACE);
    if status == "stale_cursor" {
        output.payload["restart_required"] = json!(true);
    }
    if status == "cursor_retention_unavailable" {
        output.mark_retention_unavailable();
    }
    output.payload["coverage_scope"] = json!("recoverable currently permitted records only; absent, legacy, malformed or unavailable records are not established absent");
    output.payload["historical_only"] = json!(true);
    output.payload["operative_evidence_or_event"] = json!(false);
    output
}

// This is called on every rendering, including final response-fit validation and
// later preparation. Old record identities never enter supplied_observations.
type HistoricalProjection = (Value, Exposure, Vec<f::Reference>, Vec<Vec<f::Reference>>);

pub(crate) fn supply(
    state: &State,
    receipt: &Observation,
    a: &f::Admission,
    corpus: &f::Corpus,
    material: &Material,
    projection: &mut f::NavigationBudget,
    permissions: &Permissions<'_>,
) -> Result<HistoricalProjection> {
    // Preflight before descriptor decoding or repeated body reconstruction.
    require(
        projection.charge(f::NavigationUsage {
            scanned_bytes: INPUT_LIMIT,
            inspected_revisions: 1,
            ..Default::default()
        }),
        "historical projection envelope exhausted",
    )?;
    require(
        receipt.sources.is_empty() && receipt.qualified_roots.is_empty(),
        "historical wrapper has unexpected operative roots",
    )?;
    let mut exposure = permissions
        .exposure(
            receipt
                .exposure
                .as_ref()
                .ok_or_else(|| held("history receipt exposure missing"))?,
        )
        .ok_or_else(|| held("history receipt unavailable"))?;
    let mut wrapper = json!({"id":receipt.id,"actor":receipt.actor,"work":receipt.work,"invocation":receipt.invocation,
        "kind":receipt.kind,"at_unix":receipt.at_unix,"payload":receipt.payload,"current_required_qualifications":[]});
    let reference: p::HistoryRecordReference =
        serde_json::from_value(receipt.payload["reference"].clone())?;
    let candidate = record(
        state,
        &receipt.actor,
        &reference,
        a,
        corpus,
        material,
        permissions,
        false,
    )
    .filter(|descriptor| reserve_projection(state, descriptor, projection))
    .and_then(|_| {
        record(
            state,
            &receipt.actor,
            &reference,
            a,
            corpus,
            material,
            permissions,
            true,
        )
    });
    let Some(candidate) = candidate else {
        wrapper["payload"]["status"] = json!("unavailable");
        wrapper["payload"]["body_supplied"] = json!(false);
        wrapper["payload"]["record"] = Value::Null;
        return Ok((wrapper, exposure, Vec::new(), Vec::new()));
    };
    let maximum = receipt.payload["supply_byte_limit"].as_u64().unwrap_or(0) as usize;
    let body = json!({"reference":candidate.reference,"historical_record":candidate.body,
        "historical_only":true,"operative_evidence_or_event":false});
    if bounded(&body, maximum.min(state.config.response_bytes)).is_none() {
        wrapper["payload"]["status"] = json!("entry_exceeds_current_supply_bound");
        wrapper["payload"]["body_supplied"] = json!(false);
        wrapper["payload"]["record"] = Value::Null;
        return Ok((wrapper, exposure, Vec::new(), Vec::new()));
    }
    extend_exposure(&mut exposure, &candidate.exposure);
    wrapper["payload"]["record"] = body;
    wrapper["payload"]["status"] = json!("read");
    wrapper["payload"]["body_supplied"] = json!(true);
    // All historical qualification bundles stay inside historical_record; the
    // outer current_required_qualifications stays empty to avoid evidence reuse.
    Ok((wrapper, exposure, candidate.roots, candidate.root_groups))
}

#[allow(clippy::too_many_arguments)]
fn query(
    state: &State,
    prepared: &Prepared,
    id: &str,
    record_kind: p::HistoryRecordKind,
    mode: &f::SearchMode,
    limit: usize,
    token: Option<&str>,
    a: &f::Admission,
    permissions: &Permissions<'_>,
    corpus: &f::Corpus,
    material: &Material,
    navigation: &mut f::NavigationBudget,
    result_limit: usize,
    consulted: &mut Exposure,
) -> Result<Output> {
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let focus = &state.personas[actor].focus[work];
    let query_hash = digest(&(INTERFACE, record_kind, mode))?;
    let Ok(matcher) = f::compile_search_mode(mode, &state.config.corpus) else {
        return Ok(status("invalid_query"));
    };
    let parameters = json!({"kind":"history_records_query","record_kind":record_kind,"mode":mode});
    let Some(item_limit) = query_item_limit(&parameters, result_limit) else {
        return Ok(status("exhausted"));
    };
    let previous = match token {
        None => None,
        Some(token) => {
            let Some(cursor) = focus.history_cursors.get(token) else {
                return Ok(status("stale_cursor"));
            };
            if !fresh_cursor(state, prepared, permissions, token, cursor)
                || cursor.query != query_hash
                || cursor
                    .records
                    .as_ref()
                    .is_none_or(|records| cursor.next_index > records.len())
            {
                return Ok(status("stale_cursor"));
            }
            Some(cursor)
        }
    };
    let mut candidates = Vec::new();
    if let Some(previous) = previous {
        if let Some(exposure) = state
            .observations
            .get(&previous.observation)
            .and_then(|o| o.exposure.as_ref())
        {
            extend_exposure(consulted, exposure);
        }
        for reference in previous.records.as_ref().unwrap() {
            let Some(candidate) = record(
                state,
                actor,
                reference,
                a,
                corpus,
                material,
                permissions,
                false,
            ) else {
                return Ok(status("stale_cursor"));
            };
            extend_exposure(consulted, &candidate.exposure);
            candidates.push(candidate);
        }
    } else {
        match record_kind {
            p::HistoryRecordKind::Observation => {
                for id in state.observations.keys() {
                    if let Some(candidate) =
                        observation_record(state, actor, id, a, corpus, permissions, false)
                    {
                        extend_exposure(consulted, &candidate.exposure);
                        candidates.push(candidate);
                    }
                }
            }
            p::HistoryRecordKind::AdoptedDecision => {
                for id in material.archives.keys() {
                    if let Some(candidate) =
                        decision_record(state, actor, id, a, corpus, material, permissions, false)
                    {
                        extend_exposure(consulted, &candidate.exposure);
                        candidates.push(candidate);
                    }
                }
            }
        }
        candidates.sort_by_key(|entry| key(&entry.reference));
    }
    if candidates.len() > MAX_CURSOR_MEMBERS {
        return Ok(status("cursor_retention_unavailable"));
    }
    let members = candidates
        .iter()
        .map(|r| r.reference.clone())
        .collect::<Vec<_>>();
    if bounded(&members, MAX_CURSOR_BYTES).is_none() {
        return Ok(status("cursor_retention_unavailable"));
    }
    let mut view_exposure = Exposure::default();
    for candidate in &candidates {
        extend_exposure(consulted, &candidate.exposure);
        extend_exposure(&mut view_exposure, &candidate.exposure);
    }
    let Some(manifest) = permissions.manifest(&view_exposure) else {
        return Ok(status("unavailable"));
    };
    let Some(bytes) = bounded(
        &(
            INTERFACE,
            actor,
            work,
            &query_hash,
            &members,
            &state.personas[actor].seed.destination,
            &state.personas[actor].seed.audience,
            manifest,
        ),
        INPUT_LIMIT,
    ) else {
        return Ok(status("unavailable"));
    };
    let snapshot = s::sha256_bytes(&bytes);
    let (mut position, mut emitted, mut omitted) = match previous {
        Some(cursor) if cursor.view == snapshot => (
            cursor.next_index,
            cursor.emitted_before,
            cursor.omitted_before,
        ),
        Some(_) => return Ok(status("stale_cursor")),
        None => (0, false, false),
    };
    let mut output = status("incomplete");
    output.payload["query"] = parameters;
    output.consumed = token.map(str::to_owned);
    let mut hits = Vec::new();
    let mut retry = None;
    while position < candidates.len() && hits.len() < limit {
        let descriptor = &candidates[position];
        let full;
        let candidate = if matches!(mode, f::SearchMode::Listing) {
            descriptor
        } else {
            if !reserve_projection(state, descriptor, navigation) {
                break;
            }
            let Some(projected) = record(
                state,
                actor,
                &descriptor.reference,
                a,
                corpus,
                material,
                permissions,
                true,
            ) else {
                output.payload["omitted"] = json!({"reference":descriptor.reference,"reason":"current_projection_unavailable"});
                retry = Some((position, emitted, omitted));
                omitted = true;
                position += 1;
                break;
            };
            full = projected;
            &full
        };
        let remaining = navigation.remaining();
        if remaining.scanned_bytes == 0 || remaining.inspected_revisions == 0 {
            break;
        }
        let maximum = remaining.scanned_bytes.min(INPUT_LIMIT);
        let search = if matches!(mode, f::SearchMode::Listing) {
            &candidate.metadata
        } else {
            &candidate.body
        };
        let bytes = bounded(search, maximum);
        require(
            navigation.charge(f::NavigationUsage {
                scanned_bytes: bytes.as_ref().map_or(maximum, Vec::len),
                inspected_revisions: 1,
                ..Default::default()
            }),
            "history record scan accounting",
        )?;
        let Some(bytes) = bytes else {
            output.payload["omitted"] =
                json!({"reference":candidate.reference,"reason":"entry_exceeds_scan_bound"});
            retry = Some((position, emitted, omitted));
            omitted = true;
            position += 1;
            break;
        };
        if matcher
            .first_match(std::str::from_utf8(&bytes).expect("JSON UTF-8"))
            .is_none()
        {
            position += 1;
            continue;
        }
        if !navigation.charge(f::NavigationUsage {
            matches: 1,
            ..Default::default()
        }) {
            break;
        }
        hits.push(candidate.metadata.clone());
        if bounded(&hits, item_limit).is_none() {
            hits.pop();
            if hits.is_empty() {
                output.payload["omitted"] =
                    json!({"reference":candidate.reference,"reason":"entry_exceeds_result_bound"});
                retry = Some((position, emitted, omitted));
                omitted = true;
                position += 1;
            }
            break;
        }
        emitted = true;
        position += 1;
    }
    let end = position == candidates.len();
    output.payload["records"] = json!(hits);
    output.payload["end_reached"] = json!(end);
    output.payload["coverage_complete"] = json!(end && !omitted);
    output.payload["status"] = json!(if end && !omitted {
        if emitted {
            "complete"
        } else {
            "no_match"
        }
    } else {
        "incomplete"
    });
    let template = Cursor {
        observation: observation_id(&prepared.request.id, "history_records_query", id),
        actor: actor.clone(),
        work: work.clone(),
        query: query_hash,
        view: snapshot,
        members: None,
        records: Some(members),
        next_index: 0,
        emitted_before: false,
        omitted_before: false,
    };
    if !end {
        if let Some(handle) = queue_cursor(
            &mut output,
            &focus.history_cursors,
            &template,
            "next",
            position,
            emitted,
            omitted,
        )? {
            output.payload["next_cursor"] = json!(handle);
            output.payload["continuation_available"] = json!(true);
        } else {
            output.mark_retention_unavailable();
        }
    }
    if let Some((position, emitted, omitted)) = retry {
        if let Some(handle) = queue_cursor(
            &mut output,
            &focus.history_cursors,
            &template,
            "retry",
            position,
            emitted,
            omitted,
        )? {
            output.payload["retry_cursor"] = json!(handle);
        } else {
            output.mark_retention_unavailable();
        }
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply(
    state: &mut State,
    prepared: &Prepared,
    operation: &p::Operation,
    navigation: &mut f::NavigationBudget,
    context_base_bytes: usize,
    context_exposure: &Exposure,
    planning_exposure: &mut PlanningExposure,
    now: i64,
    store: &mut s::Store,
    material: &mut Material,
) -> Result<bool> {
    require(
        prepared.history_records_interface.as_deref() == Some(INTERFACE),
        "retained record retrieval was not offered by this admission",
    )?;
    let kind = match operation {
        p::Operation::HistoryRecordsQuery { .. } => "history_records_query",
        p::Operation::HistoryRecordRead { .. } => "history_record_read",
        _ => return Err(held("history record operation required")),
    };
    let id = operation.id();
    let result_limit = navigation
        .remaining()
        .result_bytes
        .min(state.config.response_bytes)
        .min(
            state
                .config
                .context_bytes
                .saturating_sub(context_base_bytes)
                .saturating_sub(navigation.used().result_bytes)
                .saturating_sub(PROMPT_MARGIN),
        )
        .min(storage_limits().max_body_bytes as usize);
    let mut output = status("exhausted");
    let mut consulted = context_exposure.clone();
    if bounded(
        &projected(prepared, id, kind, &output.payload, now),
        result_limit,
    )
    .is_none()
    {
        let _ = navigation.begin_call();
        return Ok(false);
    }
    if navigation.begin_call().is_ok()
        && result_limit > CONTROL_BYTES
        && state.config.corpus.max_revisions <= PROJECTION_REVISIONS
        && navigation.charge(f::NavigationUsage {
            scanned_bytes: PROJECTION_BYTES,
            inspected_revisions: PROJECTION_REVISIONS * 2 + 1,
            ..Default::default()
        })
        && bounded(state, INPUT_LIMIT).is_some()
    {
        let a = admission(&state.personas[&prepared.request.persona], now);
        if let (Ok(permissions), Ok(corpus)) = (Permissions::new(state, &a), corpus(state, &a)) {
            let cursor_authorized = match operation {
                p::Operation::HistoryRecordsQuery {
                    cursor: Some(handle),
                    record_kind,
                    mode,
                    ..
                } => {
                    let expected_query = digest(&(INTERFACE, record_kind, mode))?;
                    let cursors = &state.personas[&prepared.request.persona].focus
                        [&prepared.request.work]
                        .history_cursors;
                    retention_fits(cursors.iter())
                        && cursors.get(handle).is_some_and(|c| {
                            fresh_cursor(state, prepared, &permissions, handle, c)
                                && c.query == expected_query
                                && c.records.as_ref().is_some_and(|refs| {
                                    c.next_index <= refs.len() && refs.iter().all(|r| {
                                        matches!(
                                            (record_kind, r),
                                            (
                                                p::HistoryRecordKind::Observation,
                                                p::HistoryRecordReference::Observation { .. }
                                            ) | (
                                                p::HistoryRecordKind::AdoptedDecision,
                                                p::HistoryRecordReference::AdoptedDecision { .. }
                                            )
                                        )
                                    })
                                })
                        })
                }
                _ => true,
            };
            let archive_ids = match operation {
                p::Operation::HistoryRecordsQuery {
                    record_kind: p::HistoryRecordKind::AdoptedDecision,
                    cursor: Some(handle),
                    ..
                } => {
                    let ids = state.personas[&prepared.request.persona].focus
                        [&prepared.request.work]
                        .history_cursors
                        .get(handle)
                        .filter(|c| fresh_cursor(state, prepared, &permissions, handle, c))
                        .and_then(|c| c.records.as_ref())
                        .map(|refs| {
                            refs.iter()
                                .filter_map(|r| match r {
                                    p::HistoryRecordReference::AdoptedDecision {
                                        invocation_id,
                                        ..
                                    } => Some(invocation_id.clone()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                        });
                    // Snapshot continuation is bounded and revalidates the original
                    // immutable membership; the Store exact batch has its own cap.
                    Some(ids)
                }
                p::Operation::HistoryRecordsQuery {
                    record_kind: p::HistoryRecordKind::AdoptedDecision,
                    ..
                } => Some(None),
                p::Operation::HistoryRecordRead {
                    reference: p::HistoryRecordReference::AdoptedDecision { invocation_id, .. },
                    ..
                } => Some(Some(vec![invocation_id.clone()])),
                _ => None,
            };
            // Lookup material is local to this operation until a typed exact
            // read passes actor, permissions and digest validation. Denied or
            // mismatched guesses cannot occupy reusable material capacity.
            let mut archive_batch = None;
            let archive_ready = if !cursor_authorized {
                false
            } else if let Some(ids) = archive_ids {
                let ids = ids.filter(|ids| ids.len() <= MAX_CURSORS);
                match store.history_archives(ids.as_deref()) {
                    Ok(Some(entries)) => {
                        archive_batch = Some(Material {
                            archives: entries
                                .into_iter()
                                .map(|(proof, object)| {
                                    (proof.invocation_id.clone(), (proof, object))
                                })
                                .collect(),
                        });
                        true
                    }
                    _ => false,
                }
            } else {
                true
            };
            if archive_ready {
                output = match operation {
                    p::Operation::HistoryRecordsQuery {
                        record_kind,
                        mode,
                        limit,
                        cursor,
                        ..
                    } => query(
                        state,
                        prepared,
                        id,
                        *record_kind,
                        mode,
                        *limit,
                        cursor.as_deref(),
                        &a,
                        &permissions,
                        &corpus,
                        archive_batch.as_ref().unwrap_or(material),
                        navigation,
                        result_limit,
                        &mut consulted,
                    )?,
                    p::Operation::HistoryRecordRead { reference, .. } => {
                        if let Some(descriptor) = record(
                            state,
                            &prepared.request.persona,
                            reference,
                            &a,
                            &corpus,
                            archive_batch.as_ref().unwrap_or(material),
                            &permissions,
                            false,
                        ) {
                            extend_exposure(&mut consulted, &descriptor.exposure);
                            let cache_ready = match reference {
                                p::HistoryRecordReference::AdoptedDecision {
                                    invocation_id,
                                    ..
                                } => archive_batch
                                    .as_ref()
                                    .and_then(|batch| batch.archives.get(invocation_id))
                                    .is_some_and(|entry| material.insert(vec![entry.clone()])),
                                _ => true,
                            };
                            if !cache_ready {
                                status("unavailable")
                            } else if !reserve_projection(state, &descriptor, navigation) {
                                status("exhausted")
                            } else if let Some(candidate) = record(
                                state,
                                &prepared.request.persona,
                                reference,
                                &a,
                                &corpus,
                                material,
                                &permissions,
                                true,
                            ) {
                                extend_exposure(&mut consulted, &candidate.exposure);
                                let mut output = status("incomplete");
                                let body = json!({"reference":reference,"historical_record":candidate.body,"historical_only":true,"operative_evidence_or_event":false});
                                let maximum = result_limit.saturating_sub(CONTROL_BYTES);
                                if let Some(bytes) = bounded(&body, maximum) {
                                    if navigation.charge(f::NavigationUsage {
                                        scanned_bytes: bytes.len(),
                                        inspected_revisions: 1,
                                        matches: 1,
                                        ..Default::default()
                                    }) {
                                        output.payload["status"] =
                                            json!("read_pending_fresh_supply");
                                        output.payload["reference"] = json!(reference);
                                        output.payload["supply_byte_limit"] = json!(maximum);
                                    } else {
                                        output = status("exhausted");
                                    }
                                } else {
                                    output.payload["omitted"] = json!({"reference":reference,"reason":"entry_exceeds_result_bound"});
                                }
                                output
                            } else {
                                status("unavailable")
                            }
                        } else {
                            status("unavailable")
                        }
                    }
                    _ => unreachable!(),
                };
            } else {
                output = status(if cursor_authorized {
                    "unavailable"
                } else {
                    "stale_cursor"
                });
            }
        }
    }
    planning_exposure.include(&consulted);
    // Account the actual fresh projection, while retaining only the reference.
    let mut preview = projected(prepared, id, kind, &output.payload, now);
    if output.payload.get("reference").is_some() {
        let receipt = Observation {
            id: observation_id(&prepared.request.id, kind, id),
            actor: prepared.request.persona.clone(),
            work: prepared.request.work.clone(),
            invocation: Some(prepared.request.id.clone()),
            kind: kind.into(),
            at_unix: now,
            payload: output.payload.clone(),
            sources: Vec::new(),
            exposure: Some(consulted.clone()),
            qualified_roots: Vec::new(),
            qualification_sources: None,
        };
        let a = admission(&state.personas[&prepared.request.persona], now);
        if let Ok(corpus) = corpus(state, &a) {
            preview = supply(
                state,
                &receipt,
                &a,
                &corpus,
                material,
                &mut supply_budget(),
                &Permissions::new(state, &a)?,
            )?
            .0;
        }
    }
    require(
        navigation.charge_serialized_result(&preview, result_limit),
        "history record result exceeds bound",
    )?;
    add_observation(
        state,
        prepared,
        id,
        kind,
        output.payload,
        Vec::new(),
        Vec::new(),
        now,
    )?;
    let observation = observation_id(&prepared.request.id, kind, id);
    extend_exposure(
        state
            .observations
            .get_mut(&observation)
            .unwrap()
            .exposure
            .as_mut()
            .unwrap(),
        &consulted,
    );
    let cursors = &mut state
        .personas
        .get_mut(&prepared.request.persona)
        .unwrap()
        .focus
        .get_mut(&prepared.request.work)
        .unwrap()
        .history_cursors;
    let mut replacement = cursors.clone();
    if let Some(consumed) = output.consumed {
        require(
            replacement.remove(&consumed).is_some(),
            "history record cursor disappeared",
        )?;
    }
    for (handle, cursor) in output.cursors {
        require(
            replacement.insert(handle, cursor).is_none(),
            "history record cursor conflict",
        )?;
    }
    require(
        retention_fits(replacement.iter()),
        "aggregate history cursor bounds",
    )?;
    *cursors = replacement;
    Ok(true)
}
