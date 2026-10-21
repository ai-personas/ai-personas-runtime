//! Return terminal evidence without exporting unrelated invocation payloads.
use super::*;

impl Work<'_> {
    pub(super) fn terminal_evidence(
        &self,
        subject: &EntityId,
        id: &str,
    ) -> Result<Option<StoredObject>> {
        self.id(id)?;
        let slot: TerminalEvidenceSlot = self.required("terminal_evidence", id)?;
        if slot.subject != *subject {
            return Err(StoreError::Conflict(id.into()));
        }
        Ok(slot.object)
    }
}

impl TrustedHost<'_> {
    /// Resolve the existing history/archive reference or fixed subject-bound
    /// slot inside one transaction. Only the returned evidence spends Closeout
    /// navigation; internal lookup does not expose the request or archive.
    /// This read changes no lifecycle, semantic epoch, or execution ownership.
    pub(crate) fn invocation_terminal_evidence<F>(
        &mut self,
        id: &str,
        terminal_id: &str,
        archive_id: &str,
        archived_reference: F,
    ) -> Result<Option<StoredObject>>
    where
        F: FnOnce(StoredObject) -> Result<Option<ObjectRef>>,
    {
        self.store.transaction(|work| {
            work.id(id)?;
            work.id(terminal_id)?;
            work.id(archive_id)?;
            let invocation: InvocationRecord = work.required("invocation", id)?;
            live_invocation(&invocation)?;
            let mut reference = invocation
                .history
                .iter()
                .rev()
                .find_map(|receipt| receipt.evidence.clone());
            if reference.is_none() {
                reference = invocation
                    .history
                    .iter()
                    .rev()
                    .filter_map(|receipt| receipt.detail.as_deref())
                    .find_map(|detail| {
                        let hash = detail
                            .strip_prefix("terminal:")?
                            .split_whitespace()
                            .next()?;
                        Some(ObjectRef {
                            id: terminal_id.into(),
                            revision: 1,
                            sha256: hash.into(),
                        })
                    });
            }
            if reference.is_none() && invocation.state == InvocationState::Adopted {
                reference = archived_reference(work.required("object", archive_id)?)?;
            }
            let object = match reference {
                Some(reference) => {
                    if reference.id != terminal_id || reference.revision != 1 {
                        return Err(StoreError::Conflict(terminal_id.into()));
                    }
                    Some(work.exact_evidence(&reference)?)
                }
                None => {
                    // A crash may leave an authenticated filled slot before its
                    // invocation history or adoption archive points to it.
                    let slot: TerminalEvidenceSlot =
                        work.required("terminal_evidence", terminal_id)?;
                    if slot.subject != EntityId::Invocation(id.into()) {
                        return Err(StoreError::Conflict(terminal_id.into()));
                    }
                    slot.object
                }
            };
            work.returned(&object, work.limits.max_read_bytes, BudgetClass::Closeout)?;
            Ok(object)
        })
    }
}
