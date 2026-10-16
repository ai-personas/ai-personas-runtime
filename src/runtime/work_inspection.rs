//! Primary-requested bounded inspection of immutable accepted work records.
//!
//! Cursors bind ordered immutable record identities and the effective permission
//! projection, not the board's continuation revision, admission serial or clock.
//! No new receipts, artifact bodies, selections, events or authority are created.
use super::*;
use std::io::{self, Write};

#[derive(Serialize)]
#[serde(untagged)]
enum Record<'a> {
    Commitment {
        persona: &'a w::PersonaId,
        acceptance_id: &'a w::RecordId,
        acceptance: &'a w::Acceptance,
    },
    Assessment(&'a w::Assessment),
    Judgment(&'a w::IndividualJudgment),
    Finding(&'a w::Finding),
    Blocker(&'a w::BlockerDisposition),
    Shared(&'a w::SharedDecision),
    Release(&'a w::Release),
    Delivery(&'a w::Delivery),
}
struct Entry<'a> {
    id: &'a str,
    record: Record<'a>,
    artifacts: BTreeSet<&'a w::ArtifactRef>,
    evidence: BTreeSet<&'a w::RecordId>,
}
impl<'a> Entry<'a> {
    fn evidence(&mut self, ids: &'a BTreeSet<w::RecordId>) {
        self.evidence.extend(ids);
    }
}
fn entries(work: &w::Work, collection: p::WorkCollection) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    let make = |id, record, artifacts| Entry {
        id,
        record,
        artifacts,
        evidence: BTreeSet::new(),
    };
    match collection {
        p::WorkCollection::CurrentCommitments => {
            for persona in work.participants.keys() {
                if let Some((id, acceptance)) = work.acceptance(persona) {
                    entries.push(make(
                        &id.0,
                        Record::Commitment {
                            persona,
                            acceptance_id: id,
                            acceptance,
                        },
                        BTreeSet::new(),
                    ));
                }
            }
        }
        p::WorkCollection::Assessments => {
            for record in &work.assessments {
                let mut entry = make(
                    &record.input.id.0,
                    Record::Assessment(record),
                    BTreeSet::from([&record.input.artifact]),
                );
                for evidence in &record.snapshot {
                    entry.artifacts.extend(evidence.artifacts.iter());
                }
                entries.push(entry);
            }
        }
        p::WorkCollection::Judgments => {
            for record in &work.judgments {
                let mut entry = make(
                    &record.input.id.0,
                    Record::Judgment(record),
                    record.input.candidate.artifacts.values().collect(),
                );
                for claim in record.input.claims.values() {
                    entry.evidence(&claim.evidence);
                }
                entries.push(entry);
            }
        }
        p::WorkCollection::Findings => {
            for (id, record) in &work.findings {
                let mut entry = make(
                    &id.0,
                    Record::Finding(record),
                    record.artifact.iter().collect(),
                );
                entry.evidence(&record.evidence);
                entries.push(entry);
            }
        }
        p::WorkCollection::BlockerDispositions => {
            for record in &work.blocker_dispositions {
                let mut entry = make(&record.finding.0, Record::Blocker(record), BTreeSet::new());
                if let Some(finding) = work.findings.get(&record.finding) {
                    entry.artifacts.extend(finding.artifact.iter());
                    entry.evidence(&finding.evidence);
                }
                entries.push(entry);
            }
        }
        p::WorkCollection::SharedDecisions => {
            for (id, record) in &work.shared {
                entries.push(make(
                    &id.0,
                    Record::Shared(record),
                    record.candidate.artifacts.values().collect(),
                ));
            }
        }
        p::WorkCollection::Releases => {
            for (id, record) in &work.releases {
                entries.push(make(
                    &id.0,
                    Record::Release(record),
                    record.candidate.artifacts.values().collect(),
                ));
            }
        }
        p::WorkCollection::Deliveries => {
            for record in &work.deliveries {
                let Some(release) = work.releases.get(&record.release) else {
                    continue;
                };
                let artifacts = release.candidate.artifacts.values().collect();
                entries.push(make(&record.id.0, Record::Delivery(record), artifacts));
            }
        }
    }
    entries
}

/// A bounded writer also avoids allocating oversized serialized record copies.
struct BoundedBytes {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("inspection serialization bound"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn bounded<T: Serialize>(value: &T, maximum: usize) -> Option<Vec<u8>> {
    let mut writer = BoundedBytes {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut writer, value).ok()?;
    Some(writer.bytes)
}
fn cursor(snapshot: &str, offset: usize) -> String {
    format!("inspect-work-v1:{snapshot}:{offset}")
}
fn offset(token: Option<&str>, snapshot: &str, count: usize) -> Option<usize> {
    let Some(token) = token else { return Some(0) };
    let prefix = format!("inspect-work-v1:{snapshot}:");
    let number = token.strip_prefix(&prefix)?;
    let offset = number.parse::<usize>().ok()?;
    (offset <= count && number == offset.to_string()).then_some(offset)
}
fn page(collection: p::WorkCollection, status: &str) -> Value {
    json!({
        "collection":collection, "status":status, "records":[],
        "coverage_complete":false, "permitted_view_only":true,
        "coverage_scope":"remaining permitted view from start_position",
        "start_position":0, "snapshot":null, "page_records_complete":false, "end_reached":false,
        "next_cursor":null, "restart_required":false,
        "artifact_bodies_supplied":false, "semantic_truth_verified":false,
        "recipient_attention_established":false, "stakeholder_acceptance_established":false,
        "informal_interpretations":"deferred_unknown_exposure_lineage"
    })
}
// A permission-independent reservation covers the complete retained metadata
// projection: eight Store envelopes and two modes per admitted source revision.
// It is 32 MiB / 8193 revisions under defaults (64 MiB / 16384 available).
pub(super) const PROJECTION_BYTES: usize = INPUT_LIMIT * 8;
pub(super) const PROJECTION_REVISIONS: usize = 4096;
#[derive(Default)]
struct PermissionNode {
    edges: Vec<usize>,
    denied: bool,
    fingerprint: String,
    references: usize,
    bytes: usize,
}
pub(super) struct Permissions<'a> {
    state: &'a State,
    admission: &'a f::Admission,
    index: BTreeMap<(&'a str, &'a str), usize>,
    nodes: Vec<PermissionNode>,
    closures: Vec<Vec<u64>>,
    bounds: Vec<(usize, usize)>,
    artifact_cache: std::cell::RefCell<BTreeMap<String, Option<Exposure>>>,
    evidence_cache: std::cell::RefCell<BTreeMap<String, Option<Exposure>>>,
}
impl<'a> Permissions<'a> {
    pub(super) fn new(state: &'a State, admission: &'a f::Admission) -> Result<Self> {
        require(
            state.config.corpus.max_revisions <= PROJECTION_REVISIONS,
            "work inspection projection configuration unsupported",
        )?;
        let mut index = BTreeMap::new();
        let mut revisions = Vec::new();
        for (name, source) in &state.sources {
            for revision in &source.revisions {
                index.insert(
                    (name.as_str(), revision.record.revision.as_str()),
                    revisions.len() * 2,
                );
                revisions.push((name.as_str(), source, revision));
            }
        }
        require(
            revisions.len() <= state.config.corpus.max_revisions,
            "work inspection source envelope exceeds admitted revision bound",
        )?;
        let mut nodes: Vec<PermissionNode> = (0..revisions.len() * 2)
            .map(|_| PermissionNode::default())
            .collect();
        let bindings = state.accepted_bindings.iter().fold(
            BTreeMap::<(&str, &str), Vec<&f::AcceptedBinding>>::new(),
            |mut map, binding| {
                map.entry((&binding.source, &binding.file_revision))
                    .or_default()
                    .push(binding);
                map
            },
        );
        let mut warning_bindings = BTreeSet::new();
        let mut markers = BTreeSet::new();
        for warning in state.semantic_warnings.values() {
            warning_bindings.extend(warning.bindings.iter().map(|binding| binding.id.as_str()));
            markers.insert((
                warning.marker.source.as_str(),
                warning.marker.revision.as_str(),
            ));
            markers.extend(
                warning
                    .history
                    .iter()
                    .map(|event| (event.marker.source.as_str(), event.marker.revision.as_str())),
            );
        }
        let policies: BTreeMap<_, _> = state
            .config
            .policies
            .active_policies
            .iter()
            .map(|policy| (policy.id.as_str(), policy))
            .collect();
        let permitted = |policy: &f::Policy| {
            policies
                .get(policy.id.as_str())
                .is_some_and(|current| *current == policy)
                && !state
                    .config
                    .policies
                    .revoked_policies
                    .contains(&policy.identity())
                && policy.readers.contains(&admission.principal)
                && policy
                    .processing_destinations
                    .contains(&admission.destination)
                && admission.audience.is_subset(&policy.audience)
                && policy.derivative_use == f::DerivativeUse::AllowDerivatives
                && policy
                    .expires_at_unix
                    .is_none_or(|t| admission.now_unix < t)
                && policy
                    .revoked_at_unix
                    .is_none_or(|t| admission.now_unix < t)
        };
        for (position, (name, source, stored)) in revisions.iter().enumerate() {
            let revision = stored.record.revision.as_str();
            let attached = bindings
                .get(&(*name, revision))
                .cloned()
                .unwrap_or_default();
            for mode in 0..2 {
                let node = &mut nodes[position * 2 + mode];
                node.denied = source.retired
                    || !permitted(&stored.record.policy)
                    || (stored.exposure.is_none() && revision_importer(source, stored).is_none());
                let mut restrictions =
                    vec![scoped_policy("file", name, revision, &stored.record.policy)];
                let mut links: Vec<(&f::Reference, usize)> = Vec::new();
                if let Some(inherited) = &stored.exposure {
                    restrictions.extend(inherited.policies.iter().cloned());
                    node.denied |= inherited.policies.iter().any(|p| !permitted(&p.policy));
                    let inherited_mode =
                        usize::from(mode == 1 || markers.contains(&(*name, revision)));
                    links.extend(inherited.sources.iter().map(|r| (r, inherited_mode)));
                }
                links.extend(
                    stored
                        .lineage
                        .iter()
                        .chain(&stored.record.qualifications)
                        .map(|r| (r, mode)),
                );
                let mut applied = Vec::new();
                for binding in &attached {
                    if mode == 1 && warning_bindings.contains(binding.id.as_str()) {
                        continue;
                    }
                    applied.push(*binding);
                    restrictions.push(scoped_policy(
                        "accepted-binding",
                        &binding.id,
                        &binding.revision,
                        &binding.policy,
                    ));
                    node.denied |= !permitted(&binding.policy);
                    match &binding.disposition {
                        f::BindingDisposition::Qualify { required } => {
                            links.extend(required.iter().map(|r| (r, mode)))
                        }
                        f::BindingDisposition::Retired { .. }
                        | f::BindingDisposition::Ineligible { .. } => node.denied |= mode == 0,
                        #[allow(unreachable_patterns)]
                        _ => (),
                    }
                }
                node.fingerprint = digest(&(
                    name,
                    revision,
                    mode,
                    &stored.origin,
                    &stored.exposure,
                    &stored.lineage,
                    &stored.record.policy,
                    &stored.record.qualifications,
                    applied,
                    markers.contains(&(*name, revision)),
                ))?;
                node.references = links.len();
                node.bytes = serde_json::to_vec(&restrictions)?.len();
                for (reference, mode) in links {
                    node.bytes = node
                        .bytes
                        .saturating_add(serde_json::to_vec(reference)?.len() + 1);
                    if let Some(target) =
                        index.get(&(reference.source.as_str(), reference.revision.as_str()))
                    {
                        node.edges.push(target + mode);
                    } else {
                        node.denied = true;
                    }
                }
                node.edges.sort_unstable();
                node.edges.dedup();
            }
        }
        let mut reverse = vec![Vec::new(); nodes.len()];
        for (source, node) in nodes.iter().enumerate() {
            for target in &node.edges {
                reverse[*target].push(source);
            }
        }
        let mut denied: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.denied.then_some(i))
            .collect();
        while let Some(target) = denied.pop() {
            for source in &reverse[target] {
                if !nodes[*source].denied {
                    nodes[*source].denied = true;
                    denied.push(*source);
                }
            }
        }
        // Iterative Kosaraju condenses cycles. Graph construction, denial and
        // SCC walks each visit every edge at most a constant number of times.
        // Reachability then performs a bounded bitset union per component edge.
        let mut seen = vec![false; nodes.len()];
        let mut order = Vec::new();
        for root in 0..nodes.len() {
            if seen[root] {
                continue;
            }
            seen[root] = true;
            let mut stack = vec![(root, 0)];
            while let Some((node, next)) = stack.last_mut() {
                if *next == nodes[*node].edges.len() {
                    order.push(*node);
                    stack.pop();
                    continue;
                }
                let child = nodes[*node].edges[*next];
                *next += 1;
                if !seen[child] {
                    seen[child] = true;
                    stack.push((child, 0));
                }
            }
        }
        let mut component = vec![usize::MAX; nodes.len()];
        let mut members = Vec::<Vec<usize>>::new();
        for root in order.into_iter().rev() {
            if component[root] != usize::MAX {
                continue;
            }
            let id = members.len();
            let mut group = Vec::new();
            let mut pending = vec![root];
            component[root] = id;
            while let Some(node) = pending.pop() {
                group.push(node);
                for parent in &reverse[node] {
                    if component[*parent] == usize::MAX {
                        component[*parent] = id;
                        pending.push(*parent);
                    }
                }
            }
            members.push(group);
        }
        let words = nodes.len().div_ceil(64);
        let mut closures = vec![vec![0_u64; words]; members.len()];
        for id in (0..members.len()).rev() {
            for node in &members[id] {
                closures[id][node / 64] |= 1 << (node % 64);
                for child in &nodes[*node].edges {
                    let other = component[*child];
                    if other == id {
                        continue;
                    }
                    debug_assert!(other > id);
                    let (earlier, later) = closures.split_at_mut(other);
                    for (to, from) in earlier[id].iter_mut().zip(&later[0]) {
                        *to |= from;
                    }
                }
            }
        }
        // Compute conservative closure bounds once per SCC, even for roots
        // subsequently withheld. No hidden root triggers a later variable debit.
        let bounds = closures
            .iter()
            .map(|closure| {
                nodes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| closure[i / 64] & (1 << (i % 64)) != 0)
                    .fold((0_usize, 0_usize), |(refs, bytes), (_, node)| {
                        (
                            refs.saturating_add(node.references),
                            bytes.saturating_add(node.bytes),
                        )
                    })
            })
            .collect();
        // One shared closure per SCC, never one expanded exposure per record.
        for (node, component) in nodes.iter_mut().zip(component) {
            node.edges = vec![component];
        }
        Ok(Self {
            state,
            admission,
            index,
            nodes,
            closures,
            bounds,
            artifact_cache: Default::default(),
            evidence_cache: Default::default(),
        })
    }
    fn reachable(&self, input: &Exposure) -> Option<Vec<u64>> {
        if input
            .policies
            .iter()
            .any(|p| !policy_permitted(self.state, &p.policy, self.admission))
        {
            return None;
        }
        let maximum = self
            .state
            .config
            .corpus
            .max_revisions
            .saturating_add(self.state.config.corpus.max_closure_references);
        let mut references = input.sources.len();
        let mut bytes = serde_json::to_vec(input).ok()?.len();
        let mut reachable = vec![0_u64; self.nodes.len().div_ceil(64)];
        for reference in &input.sources {
            let index = *self
                .index
                .get(&(reference.source.as_str(), reference.revision.as_str()))?;
            if self.nodes[index].denied {
                return None;
            }
            let component = self.nodes[index].edges[0];
            references = references.saturating_add(self.bounds[component].0);
            bytes = bytes.saturating_add(self.bounds[component].1);
            if references > maximum || bytes > INPUT_LIMIT {
                return None;
            }
            for (to, from) in reachable.iter_mut().zip(&self.closures[component]) {
                *to |= from;
            }
        }
        // Counting duplicate edges and both modes conservatively bounds exact
        // references, pending edges and each expanded manifest. Unsupported
        // closure size withholds only entries depending on that closure.
        (references <= maximum && bytes <= INPUT_LIMIT).then_some(reachable)
    }
    pub(super) fn exposure(&self, input: &Exposure) -> Option<Exposure> {
        self.reachable(input).map(|_| input.clone())
    }
    pub(super) fn manifest(&self, input: &Exposure) -> Option<Vec<&str>> {
        let reachable = self.reachable(input)?;
        Some(
            self.nodes
                .iter()
                .enumerate()
                .filter_map(|(i, node)| {
                    (reachable[i / 64] & (1 << (i % 64)) != 0).then_some(node.fingerprint.as_str())
                })
                .collect(),
        )
    }
    fn artifact(&self, work: &str, artifact: &w::ArtifactRef) -> Result<Option<Exposure>> {
        let key = digest(&ArtifactKey::from_native(artifact))?;
        if let Some(exposure) = self.artifact_cache.borrow().get(&key) {
            return Ok(exposure.clone());
        }
        let subject = self.state.work_meta[work].artifact_sources.get(&key);
        let exposure = match subject {
            Some(subject) => self.exposure(&Exposure {
                sources: vec![file_reference(subject)?],
                policies: vec![],
            }),
            None => None,
        };
        self.artifact_cache
            .borrow_mut()
            .insert(key, exposure.clone());
        Ok(exposure)
    }
    fn evidence(&self, work: &str, id: &w::RecordId) -> Result<Option<Exposure>> {
        if let Some(exposure) = self.evidence_cache.borrow().get(&id.0) {
            return Ok(exposure.clone());
        }
        let record = self
            .state
            .board
            .work(&w::WorkId(work.into()))?
            .evidence
            .get(id);
        let mut exposure = record.map(|_| Exposure::default());
        if let Some(record) = record {
            for artifact in &record.artifacts {
                let Some(artifact) = self.artifact(work, artifact)? else {
                    exposure = None;
                    break;
                };
                extend_exposure(exposure.as_mut().unwrap(), &artifact);
            }
        }
        if let Some(input) = &exposure {
            exposure = self.exposure(input);
        }
        self.evidence_cache
            .borrow_mut()
            .insert(id.0.clone(), exposure.clone());
        Ok(exposure)
    }
    fn entry(&self, work: &str, entry: &Entry<'_>) -> Result<Option<Exposure>> {
        let mut exposure = Exposure::default();
        // Origin is checked before traversing its release artifacts. Unknown
        // legacy receipt provenance never gets inferred from a public release.
        if let Record::Delivery(delivery) = &entry.record {
            let Some(origin) = delivery_exposure(self.state, work, delivery) else {
                return Ok(None);
            };
            let Some(origin) = self.exposure(origin) else {
                return Ok(None);
            };
            extend_exposure(&mut exposure, &origin);
        }
        // Arrangement-bound history must retain its own original authority
        // lineage. Current aggregate work restrictions cannot reconstruct it.
        let acceptance_id = match &entry.record {
            Record::Commitment { acceptance_id, .. } => Some(*acceptance_id),
            Record::Assessment(record) => Some(&record.accepted_role),
            Record::Judgment(record) => Some(&record.acceptance),
            _ => None,
        };
        if let Some(acceptance_id) = acceptance_id {
            match arrangements::retained_acceptance_origin(
                self.state,
                work,
                acceptance_id,
                self.admission,
            ) {
                Ok(Some(origin)) => extend_exposure(&mut exposure, &origin),
                Ok(None) => (),
                Err(_) => return Ok(None),
            }
        }
        let arrangement = match &entry.record {
            Record::Judgment(record) => record.arrangement.as_ref(),
            Record::Shared(record) => record.arrangement.as_ref(),
            Record::Release(record) => record.arrangement.as_ref(),
            Record::Delivery(record) => self
                .state
                .board
                .work(&w::WorkId(work.into()))?
                .releases
                .get(&record.release)
                .and_then(|release| release.arrangement.as_ref()),
            _ => None,
        };
        if let Some(exact) = arrangement {
            let Ok(origin) =
                arrangements::retained_arrangement_origin(self.state, work, exact, self.admission)
            else {
                return Ok(None);
            };
            extend_exposure(&mut exposure, &origin);
        }
        for artifact in &entry.artifacts {
            let Some(artifact) = self.artifact(work, artifact)? else {
                return Ok(None);
            };
            extend_exposure(&mut exposure, &artifact);
        }
        for id in &entry.evidence {
            let Some(evidence) = self.evidence(work, id)? else {
                return Ok(None);
            };
            extend_exposure(&mut exposure, &evidence);
        }
        Ok(self.exposure(&exposure))
    }
}

/// Only an exact retained accepted receipt can supply a delivery's originating
/// restrictions. Core/release permissions cannot reconstruct an omitted origin.
pub(super) fn delivery_exposure<'a>(
    state: &'a State,
    work: &str,
    delivery: &w::Delivery,
) -> Option<&'a Exposure> {
    let local = state.local_deliveries.get(&delivery.id.0)?;
    let invocation = local.get("invocation")?.as_str()?;
    let w::Origin::Persona(actor) = &delivery.by.principal else {
        return None;
    };
    if local.get("actor")?.as_str()? != actor.0 || local.get("work")?.as_str()? != work {
        return None;
    }
    let current = observation_id(invocation, "accepted_local_delivery", &delivery.id.0);
    // Existing accepted delivery receipts retain their exact stored identity.
    // A new bounded ID must not make a legacy long receipt lose its lineage.
    let legacy = composite_id(
        "observation",
        &[invocation, "accepted_local_delivery", &delivery.id.0],
    );
    let (id, observation) = state
        .observations
        .get_key_value(&current)
        .or_else(|| state.observations.get_key_value(&legacy))?;
    if observation.id != *id
        || observation.kind != "accepted_local_delivery"
        || observation.actor != actor.0
        || observation.work != work
        || observation.invocation.as_deref() != Some(invocation)
        || observation.payload.get("record")? != &serde_json::to_value(delivery).ok()?
    {
        return None;
    }
    observation.exposure.as_ref()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    state: &mut State,
    prepared: &Prepared,
    id: &str,
    collection: p::WorkCollection,
    limit: usize,
    token: Option<&str>,
    navigation: &mut f::NavigationBudget,
    context_base_bytes: usize,
    context_exposure: &Exposure,
    planning_exposure: &mut PlanningExposure,
    now: i64,
) -> Result<()> {
    // Reuse the already performed post-consumption, pre-navigation render.
    // No new render, collection walk, source traversal or content serialization
    // precedes the call and fixed state-projection reservation below.
    // The caller's final transactional render remains the authoritative exact
    // context fit check; this conservative local cap never expands its allowance.
    let result_limit = navigation
        .remaining()
        .result_bytes
        .min(
            state
                .config
                .context_bytes
                .saturating_sub(context_base_bytes)
                .saturating_sub(navigation.used().result_bytes)
                .saturating_sub(PROMPT_MARGIN),
        )
        .min(storage_limits().max_body_bytes as usize);
    // Pagination and budget statuses depend on the available context after
    // ordinary selection changes. Keep its permission lineage without claiming
    // that those selected bodies were supplied by this inspection.
    let mut inspected_exposure = Exposure::default();
    planning_exposure.include(context_exposure);
    let mut payload = page(collection, "budget_exhausted");
    let mut sources = BTreeSet::new();
    if navigation.charge(f::NavigationUsage {
        calls: 1,
        ..Default::default()
    }) && state.config.corpus.max_revisions <= PROJECTION_REVISIONS
        && navigation.charge(f::NavigationUsage {
            scanned_bytes: PROJECTION_BYTES,
            inspected_revisions: PROJECTION_REVISIONS * 2 + 1,
            ..Default::default()
        })
    {
        'projection: {
            let a = admission(&state.personas[&prepared.request.persona], now);
            let meta = &state.work_meta[&prepared.request.work];
            require(
                policy_permitted(state, current_policy(state, &meta.original.policy_id)?, &a),
                "work inspection core policy unavailable",
            )?;
            let permissions = Permissions::new(state, &a).inspect_err(|_| {
                planning_exposure.unknown();
            })?;
            let core = permissions
                .exposure(&work_exposure(state, &prepared.request.work)?)
                .ok_or_else(|| held("work inspection core lineage unavailable"))?;
            extend_exposure(&mut inspected_exposure, &core);
            planning_exposure.include(&core);
            let work = state
                .board
                .work(&w::WorkId(prepared.request.work.clone()))?;
            let mut records = Vec::new();
            for entry in entries(work, collection) {
                if let Some(exposure) = permissions
                    .entry(&prepared.request.work, &entry)
                    .inspect_err(|_| {
                        planning_exposure.unknown();
                    })?
                {
                    // Identity, pagination and accounting depend on the entire
                    // consulted permitted view, including off-page origins. Keep
                    // these restrictions even if this metadata exhausts the scan.
                    extend_exposure(&mut inspected_exposure, &exposure);
                    planning_exposure.include(&exposure);
                    let remaining_scan = navigation.remaining().scanned_bytes;
                    let Some(encoded) = bounded(&(entry.id, &exposure), remaining_scan) else {
                        require(
                            navigation.charge(f::NavigationUsage {
                                scanned_bytes: remaining_scan,
                                ..Default::default()
                            }),
                            "work inspection metadata exhaustion accounting mismatch",
                        )?;
                        break 'projection;
                    };
                    require(
                        navigation.charge(f::NavigationUsage {
                            scanned_bytes: encoded.len(),
                            ..Default::default()
                        }),
                        "work inspection metadata accounting mismatch",
                    )?;
                    records.push((entry, exposure));
                }
            }
            // Only the permitted view participates in identity, pagination and
            // completion. Hidden insertion/removal cannot reveal an ordinal, alter
            // the cursor digest, or create an otherwise empty continuation page.
            let identities: Vec<_> = records.iter().map(|(entry, _)| entry.id).collect();
            let visible_permissions: Vec<_> =
                records.iter().map(|(_, exposure)| exposure).collect();
            let visible_lineage = &inspected_exposure;
            let permission_manifest = permissions.manifest(visible_lineage).ok_or_else(|| {
                held("work inspection visible lineage exceeds bounded projection")
            })?;
            let snapshot_input = (
                "inspect-work-v1",
                &prepared.request.work,
                collection,
                &a.principal,
                &a.destination,
                &a.audience,
                &core,
                &identities,
                &visible_permissions,
                &permission_manifest,
            );
            let scan_limit = navigation.remaining().scanned_bytes;
            if let Some(bytes) = bounded(&snapshot_input, scan_limit) {
                require(
                    navigation.charge(f::NavigationUsage {
                        scanned_bytes: bytes.len(),
                        ..Default::default()
                    }),
                    "inspection metadata accounting mismatch",
                )?;
                let snapshot = s::sha256_bytes(&bytes);
                if let Some(mut position) = offset(token, &snapshot, records.len()) {
                    payload = page(collection, "page");
                    payload["start_position"] = json!(position);
                    payload["snapshot"] = json!(snapshot);
                    // Withheld entries have no emitted identities or contents.
                    // A complete page describes only the currently permitted view.
                    let mut complete_records = true;
                    let mut results = Vec::new();
                    while position < records.len() && results.len() < limit {
                        if navigation.remaining().matches == 0
                            || navigation.remaining().scanned_bytes == 0
                        {
                            break;
                        }
                        let (entry, exposure) = &records[position];
                        let scan_max = navigation
                            .remaining()
                            .scanned_bytes
                            .min(storage_limits().max_body_bytes as usize);
                        let serialized = bounded(&entry.record, scan_max);
                        let scanned = serialized.as_ref().map_or(scan_max, Vec::len);
                        require(
                            navigation.charge(f::NavigationUsage {
                                scanned_bytes: scanned,
                                matches: 1,
                                ..Default::default()
                            }),
                            "inspection record accounting mismatch",
                        )?;
                        let mut item = json!({
                            "record_id":entry.id, "position":position, "status":"record",
                            "record":null
                        });
                        if let Some(bytes) = serialized {
                            item["record"] = serde_json::from_slice(&bytes)?;
                        } else {
                            item["status"] = json!("record_exceeds_scan_bound");
                            item["retry_cursor"] = json!(cursor(&snapshot, position));
                            complete_records = false;
                        }
                        let mut trial = payload.clone();
                        let mut trial_results = results.clone();
                        trial_results.push(item.clone());
                        trial["records"] = json!(trial_results);
                        trial["next_cursor"] = json!(cursor(&snapshot, position + 1));
                        if bounded(&trial, result_limit).is_none() {
                            item = json!({
                                "record_id":entry.id, "position":position,
                                "status":"record_exceeds_result_bound",
                                "retry_cursor":cursor(&snapshot, position),
                            });
                            trial_results.pop();
                            trial_results.push(item.clone());
                            trial["records"] = json!(trial_results);
                            if bounded(&trial, result_limit).is_none() {
                                // Current page is full. Leave this record for an explicit
                                // continuation; no identity/content was silently truncated.
                                break;
                            }
                            complete_records = false;
                        }
                        sources.extend(exposure.sources.iter().cloned());
                        results.push(item);
                        position += 1;
                    }
                    payload["records"] = json!(results);
                    payload["coverage_complete"] =
                        json!(position == records.len() && complete_records);
                    payload["page_records_complete"] = json!(complete_records);
                    payload["end_reached"] = json!(position == records.len());
                    payload["next_cursor"] = if position < records.len() {
                        json!(cursor(&snapshot, position))
                    } else {
                        Value::Null
                    };
                } else {
                    payload = page(collection, "stale_cursor");
                    payload["restart_required"] = json!(true);
                }
            } else {
                // Failed bounded serialization consumes its authorized scan allowance.
                require(
                    navigation.charge(f::NavigationUsage {
                        scanned_bytes: scan_limit,
                        ..Default::default()
                    }),
                    "inspection metadata accounting mismatch",
                )?;
            }
        }
    }
    require(
        navigation.charge_serialized_result(&payload, result_limit),
        "work inspection result/status exceeds remaining navigation or context bound",
    )?;
    add_observation(
        state,
        prepared,
        id,
        "work_inspection",
        payload,
        sources.into_iter().collect(),
        Vec::new(),
        now,
    )?;
    // The core can have acquired new restrictions through this same response's
    // accepted commands; retain them even when the page is empty or unavailable.
    let observation_id = observation_id(&prepared.request.id, "work_inspection", id);
    let observation = state.observations.get_mut(&observation_id).unwrap();
    extend_exposure(observation.exposure.as_mut().unwrap(), context_exposure);
    extend_exposure(observation.exposure.as_mut().unwrap(), &inspected_exposure);
    Ok(())
}
