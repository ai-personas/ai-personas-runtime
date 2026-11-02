//! Project an operator view without holding the runtime's writer lock.
use super::*;

pub(crate) struct RuntimeInspection<'a, 'db> {
    pub(super) work: &'a Work<'db>,
}

impl RuntimeInspection<'_, '_> {
    pub(crate) fn epoch(&self) -> u64 {
        self.work.meta.epoch
    }
    pub(crate) fn frozen(&self) -> bool {
        false
    }

    pub(crate) fn invocation(&self, id: &str) -> Result<Option<InvocationRecord>> {
        self.work.id(id)?;
        let record: Option<InvocationRecord> = self.work.get("invocation", id)?;
        if let Some(record) = &record {
            live_invocation(record)?;
        }
        Ok(record)
    }

    pub(crate) fn reservation(&self, id: &ReservationId) -> Result<ReservationRecord> {
        self.work.id(&id.id)?;
        self.work.required("reservation", &id.key())
    }

    pub(crate) fn object(&self, id: &str) -> Result<Option<StoredObject>> {
        self.work.id(id)?;
        self.work.get("object", id)
    }

    pub(crate) fn decision(&self, id: &str) -> Result<DecisionRecord> {
        self.work.id(id)?;
        self.work.required("decision", id)
    }

    pub(crate) fn intent(&self, id: &str) -> Result<IntentRecord> {
        self.work.id(id)?;
        self.work.required("intent", id)
    }

    pub(crate) fn terminal_evidence(
        &self,
        subject: &EntityId,
        id: &str,
    ) -> Result<Option<StoredObject>> {
        self.work.terminal_evidence(subject, id)
    }

    pub(crate) fn session_terminal(
        &self,
        claim: &ObjectRef,
        terminal_id: &str,
    ) -> Result<Option<StoredObject>> {
        self.work.launch_session_terminal(claim, terminal_id)
    }
}

impl TrustedHost<'_> {
    /// The trusted projection validates operator authority and disclosure before
    /// returning its output. Internal heads and supporting records never
    /// cross this view boundary. Only the returned projection spends Ordinary
    /// navigation. The callback has no Store mutation or execution capability.
    pub(crate) fn runtime_projection<T: Serialize, E, F>(
        &mut self,
        _maximum: u64,
        project: F,
    ) -> Result<std::result::Result<T, E>>
    where
        F: FnOnce(
            StoredObject,
            StoredObject,
            &RuntimeInspection<'_, '_>,
        ) -> std::result::Result<T, E>,
    {
        // Inspection can decode substantial retained history. BEGIN IMMEDIATE
        // here used to block model admission, adoption and tool settlement for
        // that entire read, repeatedly when a desktop was open. WAL readers can
        // instead retain one coherent snapshot while the foreground progresses.
        let (view, bytes) = {
            let tx = self.store.connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
            let work = Work::load(&tx, &self.store.limits)?;
            let runtime = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let launch = work.required("object", "runtime-launch-v1")?;
            let view = project(runtime, launch, &RuntimeInspection { work: &work });
            let bytes = view.as_ref().ok().map(serialized_size).transpose()?;
            drop(work);
            tx.commit()?;
            (view, bytes)
        };
        // Charge the exact returned bytes against the latest ledger in a short
        // write transaction; never flush metadata from the older read snapshot.
        // No output is returned if accounting fails. This read grants no action:
        // mutation callers still validate their own current exact references.
        if let Some(bytes) = bytes {
            self.store.transaction(|work| work.charge_returned_bytes(bytes, BudgetClass::Ordinary))?;
        }
        Ok(view)
    }
}
