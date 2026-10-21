//! Bounded, prepaid closeout for one trusted foreground launcher session.
//! The claim commits before execution. Filling its terminal is observation-only;
//! it grants no dispatch, recovery, replay, state change, or resource refund.
use super::*;

pub(super) const NAMESPACE: &str = "launch_session_terminal";
const CLAIM_KIND: &str = "launch-session-claim-v1";
const END_KIND: &str = "launch-session-end-v1";
const CLAIM_BYTES: u64 = 64 * 1024;
const END_BYTES: u64 = 8 * 1024;
const SLOT_BYTES: u64 = 16 * 1024;
const LAUNCH_ID: &str = "runtime-launch-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionTerminalSlot {
    version: u32,
    claim: ObjectRef,
    max_launch_bytes: u64,
    object: Option<StoredObject>,
}

impl Store {
    /// Create the immutable claim and its one terminal allocation atomically,
    /// from existing Ordinary capacity. Prepay one final launch metadata read
    /// and the bounded receipt response. Failure admits no session or driver.
    pub(crate) fn claim_launch_session(&mut self, claim: Change, terminal_id: &str) -> Result<()> {
        self.transaction(|work| {
            work.admission()?;
            work.require_terminal_invocations()?;
            work.id(terminal_id)?;
            if claim.expected_revision.is_some()
                || claim.object.kind != CLAIM_KIND
                || claim.object.body["version"] != 2
                || terminal_id == claim.object.id
            {
                return Err(StoreError::Invalid("launch session claim".into()));
            }
            encode(&claim.object, CLAIM_BYTES)?;
            let candidates = work.validate_changes(std::slice::from_ref(&claim))?;
            work.dependencies(&claim.depends_on, &candidates)?;
            if work.cell("object", terminal_id)?.is_some()
                || work
                    .cell("revision", &revision_key(terminal_id, 1))?
                    .is_some()
                || work.cell("terminal_evidence", terminal_id)?.is_some()
                || work.cell(NAMESPACE, terminal_id)?.is_some()
            {
                return Err(StoreError::Conflict(terminal_id.into()));
            }
            let slot = SessionTerminalSlot {
                version: 1,
                claim: claim.object.exact_ref()?,
                max_launch_bytes: work.limits.max_record_bytes,
                object: None,
            };
            // Account conservatively for the final launch head, claim and slot
            // reads, metadata, and the full bounded session response. No later
            // ordinary read or allocation is needed to persist/return closeout.
            let navigation = slot
                .max_launch_bytes
                .checked_add(2 * CLAIM_BYTES + 2 * SLOT_BYTES + 2 * META_CAPACITY)
                .ok_or(StoreError::Overflow("session closeout navigation"))?;
            work.meta.ordinary.consumed.navigation_bytes = work
                .meta
                .ordinary
                .consumed
                .navigation_bytes
                .checked_add(navigation)
                .ok_or(StoreError::Overflow("session closeout navigation"))?;
            work.meta
                .check_dimension(Resource::NavigationBytes, BudgetClass::Ordinary)?;
            work.bump_epoch()?;
            // These heads are immutable. Their actual bounded sizes suffice;
            // a small claim need not reserve a whole runtime snapshot cell.
            work.insert_exact(
                "revision",
                &revision_key(&claim.object.id, 1),
                &claim.object,
                BudgetClass::Ordinary,
            )?;
            work.insert_exact(
                "object",
                &claim.object.id,
                &claim.object,
                BudgetClass::Ordinary,
            )?;
            work.insert(
                NAMESPACE,
                terminal_id,
                &slot,
                Some(SLOT_BYTES),
                BudgetClass::Ordinary,
                1,
            )
        })
    }

    /// The original session owner makes one final observation attempt. The
    /// callback derives metadata from the actual current launch, never from a
    /// reconstructed run. No ordinary admission, charge, or epoch bump occurs.
    /// An error leaves the terminal unknown and never authorizes another run.
    pub(crate) fn finish_launch_session<F>(
        &mut self,
        claim: &ObjectRef,
        terminal_id: &str,
        build: F,
    ) -> Result<StoredObject>
    where
        F: FnOnce(&StoredObject) -> Result<StoredObject>,
    {
        self.transaction(|work| {
            let mut slot = work.launch_session_slot(terminal_id)?;
            if slot.claim != *claim || slot.object.is_some() {
                return Err(StoreError::Conflict(terminal_id.into()));
            }
            let retained: StoredObject = work.required("object", &claim.id)?;
            encode(&retained, CLAIM_BYTES)?;
            if retained.exact_ref()? != *claim
                || retained.kind != CLAIM_KIND
                || retained.revision != 1
                || retained.body["version"] != 2
            {
                return Err(StoreError::Conflict(claim.id.clone()));
            }
            let launch: StoredObject = work.required("object", LAUNCH_ID)?;
            encode(&launch, slot.max_launch_bytes)?;
            if launch.id != LAUNCH_ID || launch.kind != "foreground-launch" {
                return Err(StoreError::Corrupt("session closeout launch".into()));
            }
            let terminal = build(&launch)?;
            validate_terminal(&terminal, terminal_id, claim)?;
            slot.object = Some(terminal.clone());
            // This cell already paid for its identity, one history slot, full
            // possible payload and peak rewrite copy. Freeze can still fill it.
            work.update(NAMESPACE, terminal_id, &slot)?;
            Ok(terminal)
        })
    }

    /// Status/replay reads remain separately metered. An absent prepaid slot is
    /// corruption for a version-2 claim, not a new admission or a legacy fallback.
    pub(crate) fn read_launch_session_terminal(
        &mut self,
        claim: &ObjectRef,
        terminal_id: &str,
    ) -> Result<Option<StoredObject>> {
        self.transaction(|work| {
            let object = work.launch_session_terminal(claim, terminal_id)?;
            work.returned(&object, END_BYTES, BudgetClass::Ordinary)?;
            Ok(object)
        })
    }
}

fn validate_terminal(object: &StoredObject, id: &str, claim: &ObjectRef) -> Result<()> {
    encode(object, END_BYTES)?;
    if object.id != id
        || object.revision != 1
        || object.kind != END_KIND
        || object.body["version"] != 1
        || object.body["claim"] != serde_json::to_value(claim)?
    {
        return Err(StoreError::Corrupt("launch session terminal".into()));
    }
    Ok(())
}

impl Work<'_> {
    pub(super) fn launch_session_terminal(
        &self,
        claim: &ObjectRef,
        terminal_id: &str,
    ) -> Result<Option<StoredObject>> {
        let slot = self.launch_session_slot(terminal_id)?;
        if slot.claim != *claim {
            return Err(StoreError::Conflict(terminal_id.into()));
        }
        Ok(slot.object)
    }

    fn launch_session_slot(&self, id: &str) -> Result<SessionTerminalSlot> {
        self.id(id)?;
        let cell = self
            .cell(NAMESPACE, id)?
            .ok_or_else(|| StoreError::Missing(id.into()))?;
        let slot: SessionTerminalSlot = serde_json::from_slice(&cell.data)?;
        self.id(&slot.claim.id)?;
        hash_valid(&slot.claim.sha256)?;
        if cell.capacity != SLOT_BYTES
            || cell.class != BudgetClass::Ordinary
            || cell.slots != 1
            || slot.version != 1
            || slot.claim.revision != 1
            || slot.max_launch_bytes == 0
            || slot.max_launch_bytes > self.limits.max_record_bytes
        {
            return Err(StoreError::Corrupt("launch session allocation".into()));
        }
        if let Some(object) = &slot.object {
            validate_terminal(object, id, &slot.claim)?;
        }
        Ok(slot)
    }

    pub(super) fn launch_session_terminal_exact(
        &self,
        reference: &ObjectRef,
    ) -> Result<Option<StoredObject>> {
        if self.cell(NAMESPACE, &reference.id)?.is_none() {
            return Ok(None);
        }
        let slot = self.launch_session_slot(&reference.id)?;
        let object = slot
            .object
            .ok_or_else(|| StoreError::Missing(reference.id.clone()))?;
        if object.exact_ref()? != *reference {
            return Err(StoreError::Conflict(reference.id.clone()));
        }
        Ok(Some(object))
    }
}
