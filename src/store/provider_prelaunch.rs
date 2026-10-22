//! Close a runtime authorization failure using its prepaid lifecycle records.
use super::*;

impl TrustedHost<'_> {
    /// Retain the actual no-launch observation, reject its Prepared invocation,
    /// and hold unknown usage in one transaction. The private runtime caller
    /// authenticates its exact request and local observation. No full invocation
    /// is returned through a metered read, and no admission or refund occurs.
    pub(crate) fn reject_runtime_prelaunch<F>(
        &mut self,
        id: &str,
        object: StoredObject,
        authenticate: F,
    ) -> Result<Receipt>
    where
        F: FnOnce(&InvocationRecord, &StoredObject) -> Result<()>,
    {
        self.store.transaction(|work| {
            work.id(id)?;
            let record: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&record)?;
            if record.state != InvocationState::Prepared {
                return Err(StoreError::InvalidState(id.into()));
            }
            let subject = EntityId::Invocation(id.into());
            work.fill_terminal_evidence(object, |owner, value| {
                if owner != &subject {
                    return Err(StoreError::Conflict(id.into()));
                }
                authenticate(&record, value)
            })?;
            work.finish_recorded_runtime_prelaunch(&record)?
                .ok_or_else(|| StoreError::Invalid("runtime no-launch binding mismatch".into()))
        })
    }
}

impl Work<'_> {
    /// Also used by normal owner recovery for older writes that filled the slot
    /// before rejecting the invocation. Authentication comes from the immutable
    /// trusted-host slot, not a label supplied by a model or an ordinary object.
    /// Missing or other evidence leaves Prepared unchanged; it proves no outcome.
    pub(super) fn finish_recorded_runtime_prelaunch(
        &mut self,
        record: &InvocationRecord,
    ) -> Result<Option<Receipt>> {
        if record.state != InvocationState::Prepared
            || record.expired_at_epoch.is_some()
            || record.response_hash.is_some()
            || record.reconciliation.is_some()
            || record.reservation.scope != ReservationScope::Invocation
            || record.reservation.id != record.id
        {
            return Ok(None);
        }
        let Some(request) = record.request.as_ref() else {
            return Ok(None);
        };
        let Some(prompt) = request["prompt"].as_str() else {
            return Ok(None);
        };
        let subject = EntityId::Invocation(record.id.clone());
        if record.history.len() != 1
            || record.history[0].subject != subject
            || record.history[0].status != ReceiptStatus::Prepared
            || record.history[0].epoch != record.prepared_epoch
            || request["request"]["id"] != record.id
            || request["epoch"].as_u64() != Some(record.prepared_epoch)
            || !matches!(
                request["request"]["profile"].as_str(),
                Some("direct" | "nonconformant_research")
            )
            || request["prompt_sha256"] != sha256_bytes(prompt.as_bytes())
            || request["provider_preparation"]["invocation_id"] != record.id
            || request["provider_preparation"]["prompt_sha256"] != request["prompt_sha256"]
            || self.cell("decision", &record.id)?.is_some()
        {
            return Ok(None);
        }
        let id = format!("terminal:{}", sha256_bytes(record.id.as_bytes()));
        let Some(slot): Option<TerminalEvidenceSlot> = self.get("terminal_evidence", &id)? else {
            return Ok(None);
        };
        let Some(object) = slot.object else {
            return Ok(None);
        };
        let receipt = &object.body["receipt"];
        if slot.subject != subject
            || object.id != id
            || object.revision != 1
            || object.kind != "runtime-terminal-transport"
            || object.body["invocation_id"] != record.id
            || object.body["prepared_epoch"].as_u64() != Some(record.prepared_epoch)
            || object.body["prompt_sha256"] != request["prompt_sha256"]
            || object.body.get("response_body").is_some()
            || receipt["kind"] != "provider_prelaunch_rejection"
            || receipt["stage"] != "runtime_authorization"
            || receipt["synthetic"].as_bool() != Some(false)
            || receipt["launched"].as_bool() != Some(false)
            || receipt["dispatch_may_have_begun"].as_bool() == Some(true)
            || receipt["attempt_receipt"]["dispatch_may_have_begun"].as_bool() == Some(true)
            || receipt["replay_authorized"].as_bool() != Some(false)
        {
            return Ok(None);
        }
        let mut record = record.clone();
        record.state = InvocationState::Failed;
        let receipt = self.event(
            subject,
            ReceiptStatus::Failed,
            Some("Runtime authorization rejected before provider launch".into()),
            Some(object.exact_ref()?),
        )?;
        record.history.push(receipt.clone());
        self.update("invocation", &record.id, &record)?;
        // Preserve later measurements or uncertainty. An open reservation moves
        // to Unknown with the same aggregate hold and admitted attempt charge.
        self.mark_usage_unknown_if_open(&record.reservation)?;
        Ok(Some(receipt))
    }
}
