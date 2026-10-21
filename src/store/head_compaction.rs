//! Reclaim unused ordinary object-head allocation without changing stored data.
//! The runtime state head and all protected/control cells keep their prepayment.
use super::*;

const KIND: &str = "store-head-compaction-v1";
const MAX_HEADS: usize = 4096;
const HEAD_BYTES: u64 = 64 * 1024 * 1024;
const RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadCompactionRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub authorization_reference: String,
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
        request.expected_limits.validate()?;
        let valid_id = |value: &str| !value.is_empty() && value.len() <= 256
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
        if !request.authorize_compaction || !valid_id(&request.id) || !valid_id(&request.operator)
            || request.reason.trim().is_empty() || request.reason.len() > 4096
            || request.authorization_reference.trim().is_empty()
            || request.authorization_reference.len() > 4096
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
        work.require_terminal_invocations()?;

        // Admit one finite scan envelope before consulting the head inventory.
        // Lengths are checked before fetching bodies; no bulk historical scan.
        let navigation = HEAD_BYTES.checked_add(
            (MAX_HEADS as u64 + 1).checked_mul(u64::from(work.limits.max_id_bytes) + 64)
                .ok_or(StoreError::Overflow("head scan metadata"))?
        ).and_then(|n| n.checked_add(2 * META_CAPACITY))
            .ok_or(StoreError::Overflow("head scan envelope"))?;
        work.meta.ordinary.consumed.navigation_bytes = work.meta.ordinary.consumed
            .navigation_bytes.checked_add(navigation)
            .ok_or(StoreError::Overflow("head compaction navigation"))?;
        work.meta.check_dimension(Resource::NavigationBytes, BudgetClass::Ordinary)?;
        let keys = {
            let mut statement = tx.prepare(
                "SELECT CASE WHEN length(CAST(k AS BLOB))<=?1 THEN k ELSE NULL END,length(data)
                 FROM store_cells WHERE ns='object' ORDER BY k LIMIT ?2"
            )?;
            let rows = statement.query_map(params![work.limits.max_id_bytes, MAX_HEADS as i64 + 1],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        if keys.len() > MAX_HEADS {
            return Err(StoreError::Invalid("head compaction count bound".into()));
        }
        let bytes = keys.iter().try_fold(0u64, |sum, (key, bytes)| {
            if key.is_none() || *bytes < 0 || *bytes as u64 > work.limits.max_record_bytes {
                return Err(StoreError::TooLarge);
            }
            sum.checked_add(*bytes as u64).ok_or(StoreError::Overflow("head scan bytes"))
        })?;
        if bytes > HEAD_BYTES { return Err(StoreError::TooLarge); }

        let prior_schema = work.meta.schema_version;
        let prior_retained = work.meta.ordinary.consumed.retained_bytes;
        let mut reclaimed = 0u64;
        let mut compacted = 0usize;
        let mut preserved = Vec::new();
        for (key, length) in &keys {
            let key = key.as_ref().ok_or(StoreError::TooLarge)?;
            let length = u64::try_from(*length).map_err(|_| StoreError::TooLarge)?;
            work.id(key)?;
            let mut head = work.cell("object", key)?.ok_or_else(|| StoreError::Missing(key.clone()))?;
            let object: StoredObject = serde_json::from_slice(&head.data)?;
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
        let retained_after = work.meta.ordinary.consumed.retained_bytes.checked_add(receipt_allocation)
            .ok_or(StoreError::Overflow("compaction receipt allocation"))?;
        let receipt = StoredObject {
            id: id.clone(), revision: 1, kind: KIND.into(),
            body: serde_json::json!({
                "version":1,"request":request,"epoch":work.meta.epoch,
                "previous_schema_version":prior_schema,"schema_version":SCHEMA_VERSION,
                "heads_scanned":keys.len(),"head_data_bytes":bytes,"heads_compacted":compacted,
                "head_data_and_references_sha256":fingerprint(&preserved, 4 * 1024 * 1024)?,
                "gross_logical_allocation_reclaimed":reclaimed,
                "receipt_logical_allocation":receipt_allocation,
                "ordinary_retained_before":prior_retained,"ordinary_retained_after":retained_after,
                "head_scan_navigation_charge":navigation,"head_scan_count_limit":MAX_HEADS,
                "head_scan_byte_limit":HEAD_BYTES,
                "limits_unchanged":true,"existing_data_unchanged":true,"history_deleted":false,
                "provider_usage_settled":false,"allowances_reset":false,
                "physical_reclamation_guaranteed":false,
                "scope":"Unused Ordinary object-head allocation only. Runtime state, protected cells and existing data bytes are preserved. Schema 2 future ordinary heads pay for actual size and subsequent growth. The immutable receipt consumes its stated new allocation. No work, identity, history slot, provider attempt, uncertainty or permission is reset."
            }),
        };
        encode(&receipt, RECEIPT_BYTES)?;
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
