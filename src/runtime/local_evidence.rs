//! Trusted local mock evidence forwarding. No execution capability or selector.
//!
//! A basis records a bounded host attestation about the complete mock input and
//! its source restrictions. It does not independently authenticate that semantic
//! relationship. Host authorization remains necessary. Dispatch hosts must check
//! current work and sources and use the returned exact epoch, not sample a newer
//! epoch after an intervening change. Native persona Effect remains unsupported.
use super::*;
use crate::mock_effects as m;

/// Compact serde_json serialization of the COMPLETE StoredObject. Decoding it
/// recovers the exact object, including the exact result string. This is lossless
/// object serialization, not a claim to preserve SQLite's JSON lexical layout.
pub const LOCAL_EVIDENCE_SERIALIZATION: &str = "stored-object-json-lossless-v1";
const BRIDGE_INPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEvidenceBridge {
    pub bases: BTreeMap<String, LocalEvidenceBasis>,
    pub ingestions: BTreeMap<String, LocalEvidenceIngestion>,
    pub pending: BTreeMap<String, PendingMockDependency>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockEvidenceBasisRequest {
    pub binding: WorkAuthority,
    pub mock_request: m::MockRequest,
    pub policy: f::Policy,
    pub lineage: Vec<f::Reference>,
    pub qualifications: Vec<f::Reference>,
    /// Explicit host assertion about input/result lineage and policy. Not an
    /// authentication result, persona authorship, or a grant from serialized data.
    pub attestation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEvidenceBasis {
    pub reference: s::ObjectRef,
    pub specification: MockEvidenceBasisRequest,
    pub operator: String,
    pub mandate_sha256: String,
    pub exposure: Exposure,
    pub at_unix: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestMockEvidenceRequest {
    pub binding: WorkAuthority,
    pub id: String,
    pub actor: String,
    pub intent_id: String,
    pub basis: s::ObjectRef,
    pub admitted_request: s::ObjectRef,
    pub evidence: s::ObjectRef,
    pub source: String,
    pub path: String,
    pub max_evidence_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEvidenceReceipt {
    pub id: String,
    pub request_hash: String,
    pub file: p::ExactRef,
    pub bytes_sha256: String,
    pub evidence: s::ObjectRef,
    pub terminal_receipt: s::Receipt,
    pub terminal_sha256: String,
    pub operator: String,
    pub at_unix: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEvidenceIngestion {
    pub request: IngestMockEvidenceRequest,
    pub receipt: LocalEvidenceReceipt,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterPendingMockRequest {
    pub binding: WorkAuthority,
    pub id: String,
    pub basis: s::ObjectRef,
    pub admitted_request: s::ObjectRef,
    pub participants: BTreeSet<String>,
    pub accountability: w::Accountability,
}
/// Immutable registration receipt. It has a Store request identity and no file
/// result identity. Result arrival never rewrites this registration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingMockRegistration {
    pub id: String,
    pub revision: String,
    pub key: w::EventKey,
    pub request: RegisterPendingMockRequest,
    pub registered_by: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingMockDependency {
    pub registration: PendingMockRegistration,
    pub event: Option<LocalEventRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliverPendingMockEventRequest {
    pub binding: WorkAuthority,
    pub id: String,
    pub dependency_id: String,
    pub binding_revision: String,
    pub ingestion_id: String,
    pub max_delivery_bytes: usize,
}

fn input_bound(value: &impl Serialize) -> Result<()> {
    require(
        serde_json::to_vec(value)?.len() <= BRIDGE_INPUT_BYTES,
        "local evidence input bound",
    )
}
fn mock_error(error: m::MockError) -> RuntimeError {
    match error {
        m::MockError::Store(error) => error.into(),
        m::MockError::Protocol(error) => error.into(),
        m::MockError::Json(error) => error.into(),
        other => held(other.to_string()),
    }
}
fn pending_key(work: &str, id: &str) -> String {
    composite_id("pending-mock", &[work, id])
}
fn basis<'a>(state: &'a State, reference: &s::ObjectRef) -> Result<&'a LocalEvidenceBasis> {
    state
        .local_evidence
        .bases
        .get(&reference.id)
        .filter(|basis| &basis.reference == reference)
        .ok_or_else(|| held("exact Runtime mock evidence basis absent"))
}
fn basis_exposure(state: &State, basis: &LocalEvidenceBasis) -> Result<Exposure> {
    let mut exposure = work_exposure(state, &basis.specification.binding.work)?;
    extend_exposure(&mut exposure, &basis.exposure);
    expanded_exposure(state, &exposure)
}
fn validate_basis(
    state: &State,
    basis: &LocalEvidenceBasis,
    now: i64,
    terminal: bool,
) -> Result<Exposure> {
    let spec = &basis.specification;
    let current = state.board.work(&w::WorkId(spec.binding.work.clone()))?;
    require(
        current.mandate().authority == spec.binding.authority
            && digest(current.mandate())? == basis.mandate_sha256
            && current.version.epoch >= spec.binding.version.epoch,
        "mock basis authority or mandate changed",
    )?;
    if !terminal {
        require(
            matches!(current.phase, w::Phase::Open)
                && current.version.epoch == spec.binding.version.epoch,
            "mock registration or dispatch requires the original open work epoch",
        )?;
    }
    let actor = &spec.mock_request.actor;
    require(
        spec.mock_request.work == spec.binding.work
            && current
                .participants
                .contains_key(&w::PersonaId(actor.clone())),
        "mock actor is not a current participant of the bound work",
    )?;
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("mock actor absent"))?;
    let a = admission(persona, now);
    let exposure = basis_exposure(state, basis)?;
    check_output_policy(state, &spec.policy, &exposure, &a)?;
    Ok(exposure)
}
fn match_request(basis: &LocalEvidenceBasis, proof: &m::BridgeRequestProof) -> Result<()> {
    require(
        proof.request == basis.specification.mock_request
            && proof.prerequisites == vec![basis.reference.clone()],
        "mock request must bind the exact stable Runtime basis as its sole prerequisite",
    )
}
fn pending_exposure(state: &State, dependency: &PendingMockDependency) -> Result<Exposure> {
    let registration = &dependency.registration;
    let mut exposure = basis_exposure(state, basis(state, &registration.request.basis)?)?;
    if let Some(event) = &dependency.event {
        exposure.sources.push(file_reference(&event.observation)?);
    }
    expanded_exposure(state, &exposure)
}
fn pending_visible(
    state: &State,
    dependency: &PendingMockDependency,
    actor: &str,
    now: i64,
) -> bool {
    dependency.registration.request.participants.contains(actor)
        && state.personas.get(actor).is_some_and(|persona| {
            pending_exposure(state, dependency).is_ok_and(|exposure| {
                exposure_permitted(state, &exposure, &admission(persona, now))
            })
        })
}

impl Runtime {
    /// Allocate from the existing work navigation account and root Store account
    /// together. The conservative projection includes bounded proof records and
    /// corpus validation. It is never refunded or reused after a failed attempt.
    fn reserve_bridge_navigation(
        &mut self,
        old: &s::StoredObject,
        state: &mut State,
        work: &str,
        operation_hash: &str,
    ) -> Result<(s::StoredObject, f::NavigationLimits, u64)> {
        let requested = state.config.navigation_per_call;
        let limits = allocate_navigation(
            &mut state
                .work_meta
                .get_mut(work)
                .ok_or_else(|| held("bridge work account absent"))?
                .navigation_left,
            requested,
        );
        let scan = (storage_limits().max_record_bytes as usize)
            .checked_mul(8)
            .and_then(|n| n.checked_add(state.config.corpus.max_total_record_bytes))
            .and_then(|n| n.checked_add(INPUT_LIMIT))
            .ok_or_else(|| invalid("bridge projection overflow"))?;
        let inspections = state
            .config
            .corpus
            .max_revisions
            .checked_add(state.config.corpus.max_closure_references)
            .ok_or_else(|| invalid("bridge inspection overflow"))?;
        require(
            limits.calls > 0
                && limits.matches > 0
                && limits.result_bytes > 0
                && limits.scanned_bytes >= scan
                && limits.inspected_revisions >= inspections,
            "local evidence navigation allocation exhausted",
        )?;
        let navigation_bytes = u64::try_from(limits.scanned_bytes)
            .ok()
            .and_then(|n| {
                u64::try_from(limits.result_bytes)
                    .ok()
                    .and_then(|r| n.checked_add(r))
            })
            .ok_or_else(|| invalid("bridge navigation overflow"))?;
        let charged = state_object(old, state)?;
        let receipt = self.store.trusted_host().reserve_runtime_navigation(
            &old.exact_ref()?,
            charged.clone(),
            operation_hash,
            navigation_bytes,
        )?;
        Ok((charged, limits, receipt.epoch))
    }
    fn current_basis_object(&mut self, basis: &LocalEvidenceBasis) -> Result<()> {
        let current = self
            .store
            .read(&basis.reference.id)?
            .ok_or_else(|| held("stable mock basis object absent"))?;
        require(
            current.exact_ref()? == basis.reference,
            "stable mock basis was replaced",
        )
    }
    fn save_bridge(
        &mut self,
        old: &s::StoredObject,
        state: &State,
        basis: &s::ObjectRef,
    ) -> Result<()> {
        self.store.compare_and_swap_batch(vec![s::Change {
            expected_revision: Some(old.revision),
            object: state_object(old, state)?,
            depends_on: vec![basis.clone()],
        }])?;
        Ok(())
    }
    /// Host-only, explicit lineage/policy attestation for one complete mock
    /// request. This stable Store prerequisite does not bind the changing Runtime
    /// head. Its creation admits no intent and performs no execution.
    pub fn bind_mock_evidence(
        &mut self,
        operator: &str,
        request: MockEvidenceBasisRequest,
        now: i64,
    ) -> Result<s::ObjectRef> {
        identity(operator)?;
        input_bound(&request)?;
        require(now >= 0, "negative evidence time")?;
        text(&request.attestation, 4096)?;
        for id in [
            &request.mock_request.id,
            &request.mock_request.invocation_id,
            &request.mock_request.actor,
            &request.mock_request.work,
            &request.mock_request.connection,
        ] {
            identity(id)?;
        }
        require(
            request.mock_request.work == request.binding.work
                && request.mock_request.interface_version == m::INTERFACE_VERSION
                && request
                    .mock_request
                    .request_text
                    .len()
                    .checked_add(request.mock_request.inputs.len())
                    .is_some_and(|n| n <= 16_384)
                && request.lineage.len() <= 64
                && request.qualifications.len() <= 64,
            "mock basis input or lineage bound",
        )?;
        text(&request.mock_request.request_text, 16_384)?;
        let hash = digest(&("mock-evidence-basis-v1", operator, &request))?;
        let id = format!("mock-basis:{hash}");
        let (old, mut state) = self.load()?;
        if let Some(existing) = state.local_evidence.bases.get(&id) {
            return Ok(existing.reference.clone());
        }
        require(
            !state
                .local_evidence
                .bases
                .values()
                .any(|old| old.specification.mock_request.id == request.mock_request.id),
            "mock intent already has a different Runtime basis",
        )?;
        check_work_authority(&state, &request.binding)?;
        let work = request.binding.work.clone();
        let (charged, limits, _) =
            self.reserve_bridge_navigation(&old, &mut state, &work, &hash)?;
        let mut exposure = work_exposure(&state, &work)?;
        exposure.sources.extend(request.lineage.iter().cloned());
        exposure
            .sources
            .extend(request.qualifications.iter().cloned());
        exposure.policies.push(scoped_policy(
            "mock-input-attestation",
            &request.mock_request.id,
            &hash,
            &request.policy,
        ));
        let exposure = expanded_exposure(&state, &exposure)?;
        let current = state.board.work(&w::WorkId(work))?;
        let mut retained = LocalEvidenceBasis {
            reference: s::ObjectRef {
                id: id.clone(),
                revision: 1,
                sha256: String::new(),
            },
            specification: request,
            operator: operator.into(),
            mandate_sha256: digest(current.mandate())?,
            exposure,
            at_unix: now,
        };
        validate_basis(&state, &retained, now, false)?;
        let object = s::StoredObject {
            id: id.clone(),
            revision: 1,
            kind: "runtime-local-evidence-basis".into(),
            body: json!({"specification":retained.specification,"operator":operator,
                "mandate_sha256":retained.mandate_sha256,"exposure":retained.exposure,
                "at_unix":now,"independent_authentication":false}),
        };
        require(
            serde_json::to_vec(&object)?.len() <= limits.result_bytes,
            "mock basis result bound",
        )?;
        retained.reference = object.exact_ref()?;
        let reference = retained.reference.clone();
        state.local_evidence.bases.insert(id, retained);
        self.store.compare_and_swap_batch(vec![
            s::Change {
                expected_revision: Some(charged.revision),
                object: state_object(&charged, &state)?,
                depends_on: Vec::new(),
            },
            s::Change {
                expected_revision: None,
                object,
                depends_on: Vec::new(),
            },
        ])?;
        Ok(reference)
    }
    /// Read/check only, apart from the durable finite navigation debit. The host
    /// must use THIS returned epoch in HostAdmission for subsequent mock dispatch.
    /// Resampling a later epoch would discard the current-work fence. This API
    /// supplies no HostAdmission, dispatch, retry, or native Effect capability.
    pub fn check_mock_evidence_dispatch(
        &mut self,
        operator: &str,
        binding: WorkAuthority,
        reference: s::ObjectRef,
        now: i64,
    ) -> Result<u64> {
        identity(operator)?;
        require(now >= 0, "negative evidence time")?;
        input_bound(&(&binding, &reference))?;
        let (old, mut state) = self.load()?;
        check_work_authority(&state, &binding)?;
        let basis = basis(&state, &reference)?.clone();
        require(
            basis.specification.binding.work == binding.work,
            "mock basis work mismatch",
        )?;
        let hash = digest(&(
            "mock-dispatch-current-check-v1",
            operator,
            &binding,
            &reference,
            now,
        ))?;
        let (_, _, epoch) =
            self.reserve_bridge_navigation(&old, &mut state, &binding.work, &hash)?;
        validate_basis(&state, &basis, now, false)?;
        self.current_basis_object(&basis)?;
        require(
            self.store.epoch()? == epoch,
            "concurrent change during mock dispatch check",
        )?;
        Ok(epoch)
    }
    /// Prefer the sealed actual terminal handle. It is reverified against this
    /// Runtime's Store; serialization of the handle is not accepted as authority.
    pub fn ingest_mock_terminal(
        &mut self,
        operator: &str,
        request: IngestMockEvidenceRequest,
        terminal: m::MockTerminal,
        now: i64,
    ) -> Result<LocalEvidenceReceipt> {
        self.ingest_mock_evidence(operator, request, Some(terminal), now)
    }
    /// Crash/reopen path: the reference is only a lookup constraint. Actual
    /// subject-bound evidence, complete admission and APPLIED terminal receipt
    /// must all be found in this Store. An unfilled/unapplied slot cannot pass.
    pub fn ingest_retained_mock_terminal(
        &mut self,
        operator: &str,
        request: IngestMockEvidenceRequest,
        now: i64,
    ) -> Result<LocalEvidenceReceipt> {
        self.ingest_mock_evidence(operator, request, None, now)
    }
    fn ingest_mock_evidence(
        &mut self,
        operator: &str,
        request: IngestMockEvidenceRequest,
        supplied: Option<m::MockTerminal>,
        now: i64,
    ) -> Result<LocalEvidenceReceipt> {
        identity(operator)?;
        input_bound(&request)?;
        require(now >= 0, "negative evidence time")?;
        for id in [
            &request.id,
            &request.actor,
            &request.intent_id,
            &request.source,
        ] {
            identity(id)?;
        }
        text(&request.path, 4096)?;
        require(
            (1..=BRIDGE_INPUT_BYTES).contains(&request.max_evidence_bytes),
            "evidence byte bound",
        )?;
        if let Some(terminal) = &supplied {
            require(
                terminal.evidence() == &request.evidence
                    && terminal.receipt().subject == s::EntityId::Intent(request.intent_id.clone()),
                "sealed terminal subject or digest mismatch",
            )?;
        }
        let hash = digest(&("local-mock-ingestion-v1", operator, &request))?;
        let (old, mut state) = self.load()?;
        if let Some(existing) = state.local_evidence.ingestions.get(&request.id) {
            require(
                existing.receipt.request_hash == hash,
                "ingestion identity/content conflict",
            )?;
            if let Some(terminal) = &supplied {
                require(
                    digest(terminal)? == existing.receipt.terminal_sha256,
                    "ingestion terminal conflict",
                )?;
            }
            // Exact historical receipt only, including after withdrawal or ending.
            return Ok(existing.receipt.clone());
        }
        require(
            !state
                .local_evidence
                .ingestions
                .values()
                .any(|old| old.request.intent_id == request.intent_id),
            "mock terminal already ingested under a different identity",
        )?;
        check_work_authority(&state, &request.binding)?;
        let basis = basis(&state, &request.basis)?.clone();
        require(
            basis.specification.binding.work == request.binding.work
                && basis.specification.mock_request.actor == request.actor
                && basis.specification.mock_request.id == request.intent_id,
            "ingestion actor, work or intent mismatch",
        )?;
        let (charged, limits, _) =
            self.reserve_bridge_navigation(&old, &mut state, &request.binding.work, &hash)?;
        let exposure = validate_basis(&state, &basis, now, true)?;
        self.current_basis_object(&basis)?;
        let proof = m::bridge_terminal(
            &mut self.store,
            &request.intent_id,
            &request.admitted_request,
            &request.evidence,
        )
        .map_err(mock_error)?;
        match_request(&basis, &proof.request)?;
        if let Some(terminal) = supplied {
            require(
                digest(&terminal)? == digest(&proof.terminal)?,
                "sealed terminal differs from this Store",
            )?;
        }
        let bytes = serde_json::to_vec(&proof.object)?;
        require(
            bytes.len() <= request.max_evidence_bytes
                && bytes.len() <= state.config.corpus.max_file_bytes
                && serde_json::from_slice::<s::StoredObject>(&bytes)? == proof.object,
            "complete lossless terminal evidence exceeds its bound",
        )?;
        require(
            !state.sources.contains_key(&request.source),
            "evidence source identity conflict",
        )?;
        require(
            !state.sources.values().any(|source| {
                !source.retired
                    && source.owner == request.actor
                    && source.revisions.iter().any(|revision| {
                        revision.record.revision == source.head
                            && revision.record.scope == f::Scope::Assets
                            && revision.record.path == request.path
                    })
            }),
            "evidence active scoped path conflict",
        )?;
        let bytes_sha256 = s::sha256_bytes(&bytes);
        let file = f::FileRevision {
            source: request.source.clone(),
            revision: String::new(),
            path: request.path.clone(),
            scope: f::Scope::Assets,
            bytes,
            author: format!("local-mock:{operator}"),
            policy: basis.specification.policy.clone(),
            qualifications: basis.specification.qualifications.clone(),
            retired: false,
        }
        .seal()?;
        let receipt = LocalEvidenceReceipt {
            id: request.id.clone(),
            request_hash: hash,
            file: wire_reference(&file.reference(f::Representation::Utf8)),
            bytes_sha256: bytes_sha256.clone(),
            evidence: request.evidence.clone(),
            terminal_receipt: proof.terminal.receipt().clone(),
            terminal_sha256: digest(&proof.terminal)?,
            operator: operator.into(),
            at_unix: now,
        };
        let origin = RevisionOrigin::LocalEvidence {
            operator: operator.into(),
            actor: request.actor.clone(),
            work: request.binding.work.clone(),
            intent_id: request.intent_id.clone(),
            basis: request.basis.clone(),
            request: Box::new(request.admitted_request.clone()),
            evidence: Box::new(request.evidence.clone()),
            terminal_receipt: Box::new(receipt.terminal_receipt.clone()),
            serialization: LOCAL_EVIDENCE_SERIALIZATION.into(),
            bytes_sha256,
            file_revision: file.revision.clone(),
        };
        require(
            serde_json::to_vec(&(&file, &origin, &receipt))?.len() <= limits.result_bytes,
            "copied evidence and attribution exceed the navigation result bound",
        )?;
        state.sources.insert(
            file.source.clone(),
            Source {
                owner: request.actor.clone(),
                protected: true,
                retired: false,
                head: file.revision.clone(),
                imported_by: None,
                revisions: vec![ManagedRevision {
                    record: file,
                    lineage: basis.specification.lineage.clone(),
                    exposure: Some(exposure),
                    origin: Some(origin),
                }],
            },
        );
        validate_inventory(&state)?;
        let a = admission(&state.personas[&request.actor], now);
        f::assemble(
            &corpus(&state, &a)?,
            &[file_reference(&receipt.file)?],
            &[],
            &[],
            &a,
            limits.result_bytes,
            0,
            0,
        )?;
        state.local_evidence.ingestions.insert(
            request.id.clone(),
            LocalEvidenceIngestion {
                request,
                receipt: receipt.clone(),
            },
        );
        // File, exact origin and idempotency mapping commit in one ordinary CAS.
        // Store accounts all copied retention and peak rewrites. No wake occurs.
        self.save_bridge(&charged, &state, &basis.reference)?;
        Ok(receipt)
    }
    pub fn register_pending_mock_dependency(
        &mut self,
        operator: &str,
        request: RegisterPendingMockRequest,
        now: i64,
    ) -> Result<PendingMockRegistration> {
        identity(operator)?;
        identity(&request.id)?;
        input_bound(&request)?;
        require(now >= 0, "negative evidence time")?;
        require(
            (1..=128).contains(&request.participants.len()),
            "pending mock participant bound",
        )?;
        let revision = digest(&("pending-mock-dependency-v1", operator, &request))?;
        let key = pending_key(&request.binding.work, &request.id);
        let (old, mut state) = self.load()?;
        if let Some(existing) = state.local_evidence.pending.get(&key) {
            require(
                existing.registration.revision == revision,
                "pending mock registration conflict",
            )?;
            return Ok(existing.registration.clone());
        }
        check_work_authority(&state, &request.binding)?;
        let meta = state
            .work_meta
            .get(&request.binding.work)
            .ok_or_else(|| held("work metadata absent"))?;
        require(
            !meta.local_dependencies.contains_key(&request.id),
            "dependency identity already belongs to an exact file",
        )?;
        check_local_dependency_capacity(&state, &request.binding.work)?;
        let basis = basis(&state, &request.basis)?.clone();
        require(
            basis.specification.binding.work == request.binding.work,
            "pending mock work mismatch",
        )?;
        require(
            !state.local_evidence.pending.values().any(|entry| {
                entry.registration.request.admitted_request == request.admitted_request
            }),
            "mock request already registered",
        )?;
        let (charged, _, _) =
            self.reserve_bridge_navigation(&old, &mut state, &request.binding.work, &revision)?;
        validate_basis(&state, &basis, now, false)?;
        self.current_basis_object(&basis)?;
        let proof = m::bridge_request(
            &mut self.store,
            &basis.specification.mock_request.id,
            &request.admitted_request,
        )
        .map_err(mock_error)?;
        match_request(&basis, &proof)?;
        let bytes = serde_json::to_vec(&("pending-mock-dependency-v1", operator, &request))?;
        let registration = PendingMockRegistration {
            id: request.id.clone(),
            revision,
            key: w::EventKey {
                dependency: w::ExactRef::new(&request.id, 1, &bytes),
                epoch: request.binding.version.epoch,
            },
            request,
            registered_by: operator.into(),
        };
        let dependency = PendingMockDependency {
            registration: registration.clone(),
            event: None,
        };
        let work = &registration.request.binding.work;
        let current = state.board.work(&w::WorkId(work.clone()))?;
        for actor in &registration.request.participants {
            identity(actor)?;
            require(
                current
                    .participants
                    .contains_key(&w::PersonaId(actor.clone()))
                    && pending_visible(&state, &dependency, actor, now),
                "pending mock participant or source policy unavailable",
            )?;
        }
        observe(
            &mut state,
            operator,
            work,
            w::RuntimeRight::Observation,
            w::RuntimeOperation::RegisterDependency(w::Dependency {
                key: registration.key.clone(),
                accountability: registration.request.accountability.clone(),
            }),
        )?;
        state.local_evidence.pending.insert(key, dependency);
        self.save_bridge(&charged, &state, &basis.reference)?;
        Ok(registration)
    }
    /// Separate explicit event authorization. Resolve only an already ingested
    /// actual result; no result placeholder or mutation of the request identity.
    pub fn deliver_pending_mock_event(
        &mut self,
        operator: &str,
        request: DeliverPendingMockEventRequest,
        now: i64,
    ) -> Result<LocalEventReceipt> {
        identity(operator)?;
        identity(&request.ingestion_id)?;
        input_bound(&request)?;
        let (_, state) = self.load()?;
        let ingested = state
            .local_evidence
            .ingestions
            .get(&request.ingestion_id)
            .ok_or_else(|| held("actual ingested mock result absent"))?;
        let normalized = DeliverLocalEventRequest {
            binding: request.binding,
            id: request.id,
            dependency_id: request.dependency_id,
            binding_revision: request.binding_revision,
            observation: ingested.receipt.file.clone(),
            max_delivery_bytes: request.max_delivery_bytes,
        };
        self.deliver_event(operator, normalized, now, Some(request.ingestion_id))
    }
}

pub(super) fn has_pending_dependency(state: &State, work: &str, id: &str) -> bool {
    state
        .local_evidence
        .pending
        .contains_key(&pending_key(work, id))
}
/// Temporary adapter for the common delivery mechanism, constructed only AFTER
/// a verified result exists. It is never persisted as the pending registration.
fn result_dependency(pending: &PendingMockDependency, source: p::ExactRef) -> LocalDependency {
    let registration = &pending.registration;
    LocalDependency {
        id: registration.id.clone(),
        revision: registration.revision.clone(),
        key: registration.key.clone(),
        source,
        participants: registration.request.participants.clone(),
        accountability: registration.request.accountability.clone(),
        origin: LocalEventOrigin::LocalMock,
        registered_by: registration.registered_by.clone(),
        event: pending.event.clone(),
    }
}
pub(super) fn delivery_dependency(
    state: &State,
    request: &DeliverLocalEventRequest,
    ingestion: Option<&str>,
) -> Result<LocalDependency> {
    let work = &request.binding.work;
    let Some(ingestion) = ingestion else {
        require(
            !has_pending_dependency(state, work, &request.dependency_id),
            "pending mock requires its typed delivery API",
        )?;
        return state
            .work_meta
            .get(work)
            .and_then(|meta| meta.local_dependencies.get(&request.dependency_id))
            .cloned()
            .ok_or_else(|| held("local dependency binding absent"));
    };
    let pending = state
        .local_evidence
        .pending
        .get(&pending_key(work, &request.dependency_id))
        .ok_or_else(|| held("pending mock dependency absent"))?;
    let registered = &pending.registration.request;
    let ingested = state
        .local_evidence
        .ingestions
        .get(ingestion)
        .ok_or_else(|| held("ingested mock result absent"))?;
    let basis = basis(state, &registered.basis)?;
    require(
        registered.binding.work == *work
            && ingested.request.binding.work == *work
            && ingested.request.binding.authority == registered.binding.authority
            && ingested.request.basis == registered.basis
            && ingested.request.admitted_request == registered.admitted_request
            && ingested.request.actor == basis.specification.mock_request.actor
            && ingested.request.intent_id == basis.specification.mock_request.id
            && ingested.receipt.file == request.observation
            && ingested.receipt.evidence == ingested.request.evidence
            && ingested.receipt.terminal_receipt.subject
                == s::EntityId::Intent(ingested.request.intent_id.clone())
            && ingested.receipt.terminal_receipt.evidence.as_ref()
                == Some(&ingested.receipt.evidence),
        "pending result does not match the registered request, subject or policy basis",
    )?;
    Ok(result_dependency(pending, ingested.receipt.file.clone()))
}
pub(super) fn find_event_dependency(
    state: &State,
    work: &str,
    id: &str,
) -> Option<LocalDependency> {
    if let Some(dependency) =
        state
            .work_meta
            .get(work)?
            .local_dependencies
            .values()
            .find(|dependency| {
                dependency
                    .event
                    .as_ref()
                    .is_some_and(|event| event.id == id)
            })
    {
        return Some(dependency.clone());
    }
    state.local_evidence.pending.values().find_map(|pending| {
        let event = pending.event.as_ref()?;
        (pending.registration.request.binding.work == work && event.id == id)
            .then(|| result_dependency(pending, event.observation.clone()))
    })
}
pub(super) fn persist_delivery(
    state: &mut State,
    work: &str,
    id: String,
    dependency: LocalDependency,
    ingestion: Option<&str>,
) -> Result<()> {
    if ingestion.is_some() {
        let pending = state
            .local_evidence
            .pending
            .get_mut(&pending_key(work, &id))
            .ok_or_else(|| held("pending mock registration disappeared"))?;
        require(
            pending.event.is_none(),
            "pending mock result already delivered",
        )?;
        pending.event = dependency.event;
    } else {
        state
            .work_meta
            .get_mut(work)
            .ok_or_else(|| held("work metadata absent"))?
            .local_dependencies
            .insert(id, dependency);
    }
    Ok(())
}
pub(super) fn pending_event_observation_id<'a>(
    state: &'a State,
    work: &str,
    actor: &str,
    key: &w::EventKey,
) -> Option<&'a String> {
    state
        .local_evidence
        .pending
        .values()
        .find(|entry| {
            entry.registration.request.binding.work == work && &entry.registration.key == key
        })?
        .event
        .as_ref()?
        .observations
        .get(actor)
}
pub(super) fn supplied_pending_events(state: &State, prepared: &Prepared) -> BTreeSet<w::EventKey> {
    state
        .local_evidence
        .pending
        .values()
        .filter(|entry| {
            entry.registration.request.binding.work == prepared.request.work
                && entry
                    .event
                    .as_ref()
                    .and_then(|event| event.observations.get(&prepared.request.persona))
                    .is_some_and(|id| prepared.supplied_observations.contains(id))
        })
        .map(|entry| entry.registration.key.clone())
        .collect()
}
pub(super) fn pending_wait_visible(
    state: &State,
    work: &str,
    id: &str,
    actor: &str,
    now: i64,
) -> bool {
    state
        .local_evidence
        .pending
        .get(&pending_key(work, id))
        .is_some_and(|entry| {
            state
                .board
                .work(&w::WorkId(work.into()))
                .is_ok_and(|current| {
                    entry.registration.key.epoch == current.version.epoch
                        && pending_visible(state, entry, actor, now)
                })
        })
}
pub(super) fn render_pending_dependencies(
    state: &State,
    work: &str,
    actor: &str,
    now: i64,
    exposure: &mut Exposure,
    dependencies: &mut Vec<Value>,
    events: &mut Vec<w::Event>,
) -> Result<()> {
    let current = state.board.work(&w::WorkId(work.into()))?;
    for pending in state.local_evidence.pending.values().filter(|entry| {
        entry.registration.request.binding.work == work
            && entry.registration.key.epoch == current.version.epoch
            && pending_visible(state, entry, actor, now)
    }) {
        let registration = &pending.registration;
        let basis = basis(state, &registration.request.basis)?;
        extend_exposure(exposure, &pending_exposure(state, pending)?);
        dependencies.push(json!({
            "kind":"pending_mock_request","key":registration.key,
            "accountability":registration.request.accountability,
            "origin":"local_mock","binding_revision":registration.revision,
            "admitted_request":registration.request.admitted_request,
            "basis":registration.request.basis,"host_attestation":basis.specification.attestation,
            "payload_supplied_by_registration":false,"external_subscription":false,
            "independent_authentication":false,"future_result_authorized":false
        }));
        if pending
            .event
            .as_ref()
            .is_some_and(|event| event.observations.contains_key(actor))
        {
            events.extend(
                current
                    .events
                    .iter()
                    .filter(|event| event.key == registration.key)
                    .cloned(),
            );
        }
    }
    Ok(())
}
