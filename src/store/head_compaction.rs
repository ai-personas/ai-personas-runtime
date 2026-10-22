//! Reclaim unused ordinary object-head allocation without changing stored data.
//! The runtime state head and all protected/control cells keep their prepayment.
use super::*;

const KIND: &str = "store-head-compaction-v1";
const RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadCompactionRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub authorization_reference: String,
    #[serde(default)]
    pub authorize_compaction: bool,
    pub expected_epoch: u64,
    pub expected_runtime: ObjectRef,
    pub expected_launch: ObjectRef,
    pub expected_limits: Limits,
}

fn receipt_id(id: &str) -> String {
    format!("store-head-compaction:{}", sha256_bytes(id.as_bytes()))
}

impl Store {
    pub(crate) fn validate_head_compaction_request(request: &HeadCompactionRequest) -> Result<()> {
        let valid_id = |value: &str| !value.is_empty()
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
        if !valid_id(&request.id) || !valid_id(&request.operator)
            || request.reason.trim().is_empty()
            || request.authorization_reference.trim().is_empty()
            || request.expected_runtime.id != RUNTIME_CONTROL_STATE_ID
            || request.expected_launch.id != "runtime-launch-v1"
        {
            return Err(StoreError::Invalid("invalid head compaction request".into()));
        }
        encode(request, RECEIPT_BYTES)?;
        hash_valid(&request.expected_runtime.sha256)?;
        hash_valid(&request.expected_launch.sha256)?;
        Ok(())
    }

    /// The launcher owns execution and authenticates its operator against the
    /// exact current heads inside this transaction. Preview rolls everything
    /// back. Apply preserves every existing data cell and immutable history;
    /// only unused head capacity, its allocation charge and schema policy change.
    pub(crate) fn compact_object_heads<F>(
        &mut self,
        request: &HeadCompactionRequest,
        apply: bool,
        authorize: F,
    ) -> Result<(StoredObject, bool)>
    where F: FnOnce(&StoredObject, &StoredObject) -> Result<()>,
    {
        Self::validate_head_compaction_request(request)?;
        let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut work = Work::load(&tx, &self.limits)?;
        let runtime: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
        let launch: StoredObject = work.required("object", "runtime-launch-v1")?;
        authorize(&runtime, &launch)?;
        work.returned(&(&runtime, &launch), work.limits.max_read_bytes, BudgetClass::Closeout)?;
        let id = receipt_id(&request.id);
        if let Some(receipt) = work.get::<StoredObject>("head_compaction", &id)? {
            if receipt.id != id || receipt.kind != KIND || receipt.revision != 1
                || receipt.body["request"] != serde_json::to_value(request)?
            {
                return Err(StoreError::Conflict(id));
            }
            work.returned(&receipt, RECEIPT_BYTES, BudgetClass::Closeout)?;
            if apply { work.flush()?; }
            drop(work);
            if apply { tx.commit()?; }
            return Ok((receipt, true));
        }
        work.admission()?;
        work.epoch(request.expected_epoch)?;
        if work.meta.limits != request.expected_limits {
            return Err(StoreError::LimitsMismatch);
        }
        if runtime.exact_ref()? != request.expected_runtime
            || launch.exact_ref()? != request.expected_launch
        {
            return Err(StoreError::RevisionConflict("head compaction fence".into()));
        }
        work.require_quiescent_invocations()?;

        // Inspect every current head. Charge its actual key and data lengths
        // plus the existing metadata envelope, without a count or byte quota.
        let keys = {
            let mut statement = tx.prepare(
                "SELECT k,length(data) FROM store_cells WHERE ns='object' ORDER BY k"
            )?;
            let rows = statement.query_map([],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let bytes = keys.iter().try_fold(0u64, |sum, (_, bytes)| {
            let bytes = u64::try_from(*bytes)
                .map_err(|_| StoreError::Corrupt("negative head length".into()))?;
            sum.checked_add(bytes).ok_or(StoreError::Overflow("head scan bytes"))
        })?;
        let key_bytes = keys.iter().try_fold(0u64, |sum, (key, _)| {
            sum.checked_add(key.len() as u64).and_then(|n| n.checked_add(8))
                .ok_or(StoreError::Overflow("head scan metadata"))
        })?;
        let navigation = bytes.checked_add(key_bytes)
            .and_then(|n| n.checked_add(2 * META_CAPACITY))
            .ok_or(StoreError::Overflow("head scan accounting"))?;
        work.meta.ordinary.consumed.navigation_bytes = work.meta.ordinary.consumed
            .navigation_bytes.checked_add(navigation)
            .ok_or(StoreError::Overflow("head compaction navigation"))?;

        let prior_schema = work.meta.schema_version;
        let prior_retained = work.meta.ordinary.consumed.retained_bytes;
        let mut reclaimed = 0u64;
        let mut compacted = 0usize;
        let mut preserved = Vec::new();
        for (key, length) in &keys {
            let length = u64::try_from(*length).map_err(|_| StoreError::TooLarge)?;
            work.id(key)?;
            let mut head = work.cell("object", key)?.ok_or_else(|| StoreError::Missing(key.clone()))?;
            let object: StoredObject = decode_json(&head.data)?;
            if object.id != *key || head.data.len() as u64 != length {
                return Err(StoreError::Corrupt("head identity or byte length".into()));
            }
            preserved.push((key.clone(), sha256_bytes(&head.data), object.exact_ref()?));
            if key == RUNTIME_CONTROL_STATE_ID || head.class != BudgetClass::Ordinary
                || head.slots != 0 || head.capacity == length
            { continue; }
            let before = head.charge("object", key)?;
            head.capacity = length;
            let released = before.checked_sub(head.charge("object", key)?)
                .ok_or_else(|| StoreError::Corrupt("negative head reclamation".into()))?;
            reclaimed = reclaimed.checked_add(released)
                .ok_or(StoreError::Overflow("head reclamation"))?;
            compacted += 1;
            // Keep the exact original data, class and event-slot count.
            work.pending.insert(("object".into(), key.clone()), head);
        }
        if prior_schema == SCHEMA_VERSION && reclaimed == 0 {
            return Err(StoreError::Invalid("no unused ordinary head capacity".into()));
        }
        work.meta.ordinary.consumed.retained_bytes = prior_retained.checked_sub(reclaimed)
            .ok_or_else(|| StoreError::Corrupt("head reclamation accounting".into()))?;
        work.meta.schema_version = SCHEMA_VERSION;
        let receipt_allocation = Cell { data: Vec::new(), capacity: RECEIPT_BYTES,
            class: BudgetClass::Ordinary, slots: 0 }.charge("head_compaction", &id)?;
        let mut retained_after = work.meta.ordinary.consumed.retained_bytes.checked_add(receipt_allocation)
            .ok_or(StoreError::Overflow("compaction receipt allocation"))?;
        let mut receipt = StoredObject {
            id: id.clone(), revision: 1, kind: KIND.into(),
            body: serde_json::json!({
                "version":1,"request":request,"epoch":work.meta.epoch,
                "previous_schema_version":prior_schema,"schema_version":SCHEMA_VERSION,
                "heads_scanned":keys.len(),"head_data_bytes":bytes,"heads_compacted":compacted,
                "head_data_and_references_sha256":fingerprint(&preserved, 4 * 1024 * 1024)?,
                "gross_logical_allocation_reclaimed":reclaimed,
                "receipt_logical_allocation":receipt_allocation,
                "ordinary_retained_before":prior_retained,"ordinary_retained_after":retained_after,
                "head_scan_navigation_charge":navigation,"head_scan_count_limit":null,
                "head_scan_byte_limit":null,"resource_quotas_enforced":false,
                "limits_unchanged":true,"existing_data_unchanged":true,"history_deleted":false,
                "provider_usage_settled":false,"allowances_reset":false,
                "physical_reclamation_guaranteed":false,
                "scope":"Unused Ordinary object-head allocation only. Runtime state, protected cells and existing data bytes are preserved. Schema 2 future ordinary heads pay for actual size and subsequent growth. The immutable receipt consumes its stated new allocation. No work, identity, history slot, provider attempt, uncertainty or permission is reset."
            }),
        };
        // Long requests can grow the receipt beyond its initial allocation.
        // Include that actual growth in both the receipt and the Store charge.
        loop {
            let data = encode(&receipt, RECEIPT_BYTES)?;
            let allocation = Cell { capacity: RECEIPT_BYTES.max(data.len() as u64),
                data, class: BudgetClass::Ordinary, slots: 0 }
                .charge("head_compaction", &id)?;
            retained_after = work.meta.ordinary.consumed.retained_bytes.checked_add(allocation)
                .ok_or(StoreError::Overflow("compaction receipt allocation"))?;
            if receipt.body["receipt_logical_allocation"] == allocation
                && receipt.body["ordinary_retained_after"] == retained_after
            { break; }
            receipt.body["receipt_logical_allocation"] = serde_json::json!(allocation);
            receipt.body["ordinary_retained_after"] = serde_json::json!(retained_after);
        }
        work.insert("head_compaction", &id, &receipt, Some(RECEIPT_BYTES), BudgetClass::Ordinary, 0)?;
        if work.meta.ordinary.consumed.retained_bytes != retained_after {
            return Err(StoreError::Corrupt("compaction receipt accounting".into()));
        }
        if apply { work.flush()?; }
        drop(work);
        if apply { tx.commit()?; }
        Ok((receipt, false))
    }
}
