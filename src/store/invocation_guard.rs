//! Reuse decoded invocation metadata only after reading and fingerprinting its
//! complete current bytes. Ownership and accounting are checked on every pass.
use super::*;

#[derive(Default)]
pub(super) struct Cache {
    records: BTreeMap<String, Checked>,
}

#[derive(Clone, Copy)]
struct Checked {
    raw_sha256: [u8; 32],
    state: InvocationState,
    returned_bytes: u64,
}

impl Work<'_> {
    pub(super) fn require_owned_invocations(&mut self, owned: &BTreeSet<String>) -> Result<()> {
        phase_timing::measure("store.check_invocation_ownership", || self.check_owned_invocations(owned))
    }

    fn check_owned_invocations(&mut self, owned: &BTreeSet<String>) -> Result<()> {
        // Stream the same durable keys in the same order as the uncached guard.
        // Pending replacements take precedence, including within a rolled-back
        // preview. A cached decode never substitutes for these fresh cell reads.
        let db = self.db;
        let mut statement = db.prepare(
            "SELECT k,data,capacity,lane,slots FROM store_cells WHERE ns='invocation' ORDER BY k",
        )?;
        let mut rows = statement.query([])?;
        let mut seen = BTreeSet::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let cell = match self.pending.get(&("invocation".into(), id.clone())) {
                Some(cell) => cell.clone(),
                None => Cell::from_storage(
                    db, "invocation", row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                )?,
            };
            let raw_sha256: [u8; 32] = Sha256::digest(&cell.data).into();
            let cached = self.invocation_checks.borrow().records.get(&id)
                .filter(|known| known.raw_sha256 == raw_sha256).copied();
            let checked = if let Some(known) = cached {
                self.charge_returned_bytes(known.returned_bytes, BudgetClass::Ordinary)?;
                known
            } else {
                let record: InvocationRecord = decode_json(&cell.data)?;
                let returned_bytes = serialized_size(&record)?;
                self.charge_returned_bytes(returned_bytes, BudgetClass::Ordinary)?;
                if record.id != id {
                    return Err(StoreError::Corrupt("invocation identity mismatch".into()));
                }
                let checked = Checked { raw_sha256, state: record.state, returned_bytes };
                self.invocation_checks.borrow_mut().records.insert(id.clone(), checked);
                checked
            };
            // Cache only decoding, never the permission supplied by `owned`.
            // A failed transaction may leave verified metadata in memory, but
            // the next pass must match that transaction's complete current bytes.
            let pending = owned.contains(&id)
                && matches!(checked.state, InvocationState::Dispatched | InvocationState::Responded);
            if !pending && !matches!(
                checked.state,
                InvocationState::Adopted | InvocationState::Failed | InvocationState::Uncertain
            ) {
                return Err(StoreError::InvalidState(
                    "active transport or unfinished adoption requires its owner".into(),
                ));
            }
            seen.insert(id);
        }
        self.invocation_checks.borrow_mut().records.retain(|id, _| seen.contains(id));
        Ok(())
    }
}
