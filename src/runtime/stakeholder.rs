//! Explicit trusted-local human decisions. No persona operation invokes this ingress.
use super::*;

/// A freshness-bound human decision, supplied only through the trusted local host.
/// JSON fields are not authentication: the caller must have actual authorization
/// to record the named human's statement. Model output is never human consent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanStakeholderDecisionRequest {
    pub binding: WorkAuthority,
    pub id: String,
    /// Exact release identity: 1..256 UTF-8 bytes, without whitespace or controls.
    pub release: String,
    /// SHA-256 of compact serde_json::Value serialization of the retained Release.
    pub release_sha256: String,
    pub delivery: String,
    pub delivery_receipt: w::ExactRef,
    pub accepts: bool,
    /// The actual explicit human statement, at most 4096 UTF-8 bytes.
    pub statement: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanStakeholderDecisionReceipt {
    pub id: String,
    pub work: String,
    pub stakeholder: String,
    pub release: String,
    pub accepts: bool,
    pub receipt: w::ExactRef,
    pub work_version: w::Version,
}

impl Runtime {
    /// Record explicit human acceptance or rejection of one exact delivered release.
    /// `principal` identifies the human represented by this trusted local call;
    /// this API records that assertion, not independent identity or consent proof.
    /// It never closes work, resumes a persona, or transforms a model judgment.
    pub fn record_stakeholder_decision(
        &mut self,
        principal: &str,
        request: HumanStakeholderDecisionRequest,
        now: i64,
    ) -> Result<HumanStakeholderDecisionReceipt> {
        identity(principal)?;
        identity(&request.id)?;
        p::check_id(&request.release, "release")?;
        identity(&request.delivery)?;
        text(&request.statement, 4096)?;
        require(
            serde_json::to_vec(&request)?.len() <= 16_384,
            "human decision request bound",
        )?;
        let (old, mut state) = self.load()?;
        check_work_authority(&state, &request.binding)?;
        let work_id = &request.binding.work;
        let wid = w::WorkId(work_id.clone());
        let release_id = w::RecordId(request.release.clone());
        let work = state.board.work(&wid)?;
        require(
            matches!(work.phase, w::Phase::Open | w::Phase::Paused { .. }),
            "work is stopped",
        )?;
        require(
            !state.personas.contains_key(principal)
                && state.config.local_recipients.contains(principal)
                && work.mandate().stakeholders.contains(principal),
            "required local human stakeholder absent",
        )?;
        require(
            state.board.release_current(&wid, &release_id),
            "release is not currently supported",
        )?;
        let release = work
            .releases
            .get(&release_id)
            .ok_or_else(|| held("release absent"))?;
        require(
            digest(&serde_json::to_value(release)?)? == request.release_sha256,
            "exact stakeholder release differs",
        )?;
        let delivery = work
            .deliveries
            .iter()
            .find(|d| d.id.0 == request.delivery)
            .ok_or_else(|| held("local delivery absent"))?;
        require(
            delivery.release == release_id
                && delivery.recipient == principal
                && delivery.receipt == request.delivery_receipt
                && delivery.by.authority == work.mandate().authority,
            "stakeholder delivery binding differs",
        )?;
        let local = state
            .local_deliveries
            .get(&request.delivery)
            .ok_or_else(|| held("local delivery body absent"))?;
        require(
            local["work"] == *work_id
                && local["release"] == request.release
                && local["recipient"] == principal
                && local["kind"] == "atomic_local_inbox_deposit"
                && local["external_export"] == false
                && w::ExactRef::new(request.delivery.clone(), 1, &serde_json::to_vec(local)?)
                    == delivery.receipt,
            "exact local delivery receipt differs",
        )?;
        let w::Origin::Persona(sender) = &delivery.by.principal else {
            return Err(held("local delivery origin unavailable"));
        };
        let sender = state
            .personas
            .get(&sender.0)
            .ok_or_else(|| held("delivery persona absent"))?;
        let admission = f::Admission {
            audience: BTreeSet::from([principal.to_owned()]),
            ..admission(sender, now)
        };
        let exposure = delivery_evidence::validate(&state, work_id, delivery, &admission)?;
        // Consent has its own provenance. Retaining the human's statement does
        // not adopt delivery-only context into the shared work core. Projection
        // and closeout recheck this exact event/observation lineage at use time.
        let receipt_id = composite_id("human-stakeholder", &[work_id, &request.id]);
        require(
            !state.operator_events.iter().any(|event| {
                event["kind"] == "human_stakeholder_decision"
                    && event["receipt"]["id"] == receipt_id
            }),
            "human decision receipt identity conflict",
        )?;
        let body = json!({"principal":principal,"request":request,"at_unix":now,
            "source":"explicit_local_human_statement","independent_identity_attestation":false,
            "semantic_truth_verified_by_runtime":false,"automatic_from_model_output":false});
        let receipt = w::ExactRef::new(receipt_id, 1, &serde_json::to_vec(&body)?);
        observe(
            &mut state,
            principal,
            work_id,
            w::RuntimeRight::StakeholderAcceptance,
            w::RuntimeOperation::Stakeholder {
                release: release_id,
                accepts: request.accepts,
                receipt: receipt.clone(),
            },
        )?;
        let current = state.board.work(&wid)?;
        let decision = current
            .stakeholder_decisions
            .last()
            .ok_or_else(|| held("human decision absent"))?
            .clone();
        let work_version = current.version;
        let recipients = current
            .participants
            .iter()
            .filter(|(_, p)| p.acceptance.is_some())
            .map(|(id, _)| id.0.clone())
            .collect::<Vec<_>>();
        for actor in recipients {
            let observation_id = composite_id("human-stakeholder", &[work_id, &request.id, &actor]);
            require(
                !state.observations.contains_key(&observation_id),
                "human decision observation conflict",
            )?;
            let focus = state
                .personas
                .get_mut(&actor)
                .and_then(|p| p.focus.get_mut(work_id))
                .ok_or_else(|| held("accepted stakeholder decision recipient focus absent"))?;
            focus.pending_observations.push(observation_id.clone());
            state.observations.insert(
                observation_id.clone(),
                Observation {
                    id: observation_id,
                    actor,
                    work: work_id.clone(),
                    invocation: None,
                    kind: "human_stakeholder_decision".into(),
                    at_unix: now,
                    payload: json!({"release":request.release,"record":decision,"receipt":receipt,
                    "metadata_only":true,"source":"explicit_local_human_decision"}),
                    sources: Vec::new(),
                    exposure: Some(exposure.clone()),
                    qualified_roots: Vec::new(),
                },
            );
        }
        state.operator_events.push(
            json!({"kind":"human_stakeholder_decision","receipt":receipt,
            "body":body,"exposure":exposure}),
        );
        self.save(&old, &state)?;
        Ok(HumanStakeholderDecisionReceipt {
            id: request.id,
            work: work_id.clone(),
            stakeholder: principal.into(),
            release: request.release,
            accepts: request.accepts,
            receipt,
            work_version,
        })
    }
}
