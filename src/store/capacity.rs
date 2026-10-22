//! Explicit updates to historical storage settings and allocation metadata.
//! Immutable receipts bind reopening; counters and history survive.
use super::*;

const KIND: &str = "runtime-store-capacity-extension-v1";
const RECEIPT_BYTES: u64 = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityExtensionRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub authorization_reference: String,
    #[serde(default)]
    pub authorize_extension: bool,
    pub expected_epoch: u64,
    pub expected_runtime: ObjectRef,
    pub expected_launch: ObjectRef,
    pub expected_limits: Limits,
    pub max_body_bytes: u64,
    pub protected_closeout_retained_bytes: u64,
    /// Explicit storage-read funding only. Omission preserves the existing total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_navigation_bytes: Option<u64>,
    /// Explicit retained-storage funding. Omission preserves the existing total.
    /// This never refunds the charged history or allocates physical disk space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_retained_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Extension {
    version: u32,
    request: CapacityExtensionRequest,
    previous: Option<ObjectRef>,
    limits: Limits,
    control_plan: String,
    control_capacity_added_bytes: u64,
    #[serde(default)]
    ordinary_capacity_added_bytes: u64,
    #[serde(default)]
    navigation_allowance_added_bytes: u64,
    #[serde(default)]
    retained_allowance_added_bytes: u64,
    epoch: u64,
}

fn receipt_id(id: &str) -> String {
    format!("store-capacity:{}", sha256_bytes(id.as_bytes()))
}

fn proposed_limits(request: &CapacityExtensionRequest) -> Result<Limits> {
    let old = &request.expected_limits;
    old.validate()?;
    if request.id.is_empty()
        || request.id.chars().any(char::is_whitespace)
        || request.id.chars().any(char::is_control)
        || request.operator.is_empty()
        || request.operator.chars().any(char::is_whitespace)
        || request.operator.chars().any(char::is_control)
        || request.reason.trim().is_empty()
        || request.authorization_reference.trim().is_empty()
        || request.expected_runtime.id != RUNTIME_CONTROL_STATE_ID
        || request.expected_launch.id != "runtime-launch-v1"
        || request.max_body_bytes < old.max_body_bytes
        || (request.max_body_bytes == old.max_body_bytes
            && request.protected_closeout_retained_bytes != old.protected_closeout.retained_bytes)
        || request.protected_closeout_retained_bytes < old.protected_closeout.retained_bytes
        || request
            .total_navigation_bytes
            .is_some_and(|n| n < old.total.navigation_bytes)
        || request
            .total_retained_bytes
            .is_some_and(|n| n < old.total.retained_bytes)
    {
        return Err(StoreError::Invalid("unsupported capacity extension".into()));
    }
    let mut next = old.clone();
    next.max_body_bytes = request.max_body_bytes;
    next.max_record_bytes = request.max_body_bytes.checked_add(CONTROL_HEADROOM)
        .ok_or(StoreError::Overflow("capacity record allocation"))?;
    next.max_read_bytes = next
        .max_read_bytes
        .max(next.max_record_bytes.checked_next_power_of_two()
            .unwrap_or(next.max_record_bytes));
    next.max_page_bytes = next.max_page_bytes.max(next.max_read_bytes);
    next.protected_closeout.retained_bytes = request.protected_closeout_retained_bytes;
    if let Some(bytes) = request.total_navigation_bytes {
        next.total.navigation_bytes = bytes;
    }
    if let Some(bytes) = request.total_retained_bytes {
        next.total.retained_bytes = bytes;
    }
    // Only explicitly requested storage funding can change a total. More
    // protected retained space reduces Ordinary's ceiling. Usage is not refunded.
    next.validate()?;
    Ok(next)
}

fn decode_extension(object: &StoredObject, reference: &ObjectRef) -> Result<Extension> {
    let value: Extension = serde_json::from_value(object.body.clone())?;
    if object.exact_ref()? != *reference
        || object.kind != KIND
        || object.revision != 1
        || object.id != receipt_id(&value.request.id)
        || value.version != 1
        || proposed_limits(&value.request)? != value.limits
        || (value.limits == value.request.expected_limits
            && value.ordinary_capacity_added_bytes == 0)
        || value.navigation_allowance_added_bytes
            != value.limits.total.navigation_bytes
                - value.request.expected_limits.total.navigation_bytes
        || value.retained_allowance_added_bytes
            != value.limits.total.retained_bytes
                - value.request.expected_limits.total.retained_bytes
        || value.control_plan != format!("{}:plan", object.id)
        || value.epoch
            != value
                .request
                .expected_epoch
                .checked_add(1)
                .ok_or(StoreError::Overflow("capacity epoch"))?
    {
        return Err(StoreError::Corrupt("capacity extension receipt".into()));
    }
    Ok(value)
}

/// This host bootstrap read validates the complete exact receipt chain.
/// It supplies no application body or persona context. Normal Store reads remain
/// metered after opening, and a concurrent migration fails the strict open check.
fn approved_limits(path: &Path, base: &Limits) -> Result<Limits> {
    let path = secure_database_path(path)?;
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_handler(Some(wait_for_database))?;
    let meta = load_meta(&db)?;
    if !supported_schema(meta.schema_version) {
        return Err(StoreError::Unsupported("database schema version"));
    }
    meta.limits.validate()?;
    let mut current = meta.limits.clone();
    let mut next = meta.capacity_extension.clone();
    let mut first = true;
    let mut seen = BTreeSet::new();
    while let Some(reference) = next {
        if !seen.insert(reference.id.clone()) {
            return Err(StoreError::Corrupt("capacity extension cycle".into()));
        }
        let bytes: Vec<u8> = db.query_row(
            "SELECT data FROM store_cells WHERE ns='capacity_extension' AND k=?1",
            params![reference.id], |row| row.get(0),
        )?;
        let object: StoredObject = decode_json(&bytes)?;
        let extension = decode_extension(&object, &reference)?;
        if extension.limits != current
            || (first && meta.runtime_control_plan.as_deref() != Some(&extension.control_plan))
        {
            return Err(StoreError::LimitsMismatch);
        }
        current = extension.request.expected_limits;
        next = extension.previous;
        first = false;
    }
    if current != *base || (first && meta.runtime_control_plan.is_some()) {
        return Err(StoreError::LimitsMismatch);
    }
    Ok(meta.limits)
}

impl Store {
    pub(crate) fn open_runtime(path: &Path, base: Limits, recover: bool) -> Result<Self> {
        let limits = approved_limits(path, &base)?;
        let mut store = Self::open_for_inspection(path, limits)?;
        if recover {
            store.recover_if_unowned()?;
        }
        Ok(store)
    }

    /// Narrow host-control inspection, including when Ordinary reads are spent.
    /// One metered transaction supplies coherent authority heads to the trusted
    /// launcher; it does not supply persona context or allow general state access.
    pub(crate) fn capacity_authority(&mut self) -> Result<(StoredObject, StoredObject)> {
        self.transaction(|work| {
            let runtime: StoredObject = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let launch: StoredObject = work.required("object", "runtime-launch-v1")?;
            let heads = (runtime, launch);
            work.returned(&heads, work.limits.max_read_bytes, BudgetClass::Closeout)?;
            Ok(heads)
        })
    }

    /// Retained operator receipt only, with finite Closeout read accounting.
    pub(crate) fn capacity_extension(&mut self, id: &str) -> Result<Option<StoredObject>> {
        self.transaction(|work| {
            let object: Option<StoredObject> = work.get("capacity_extension", &receipt_id(id))?;
            if let Some(object) = &object {
                decode_extension(object, &object.exact_ref()?)?;
                work.returned(object, work.limits.max_read_bytes, BudgetClass::Closeout)?;
            }
            Ok(object)
        })
    }

    pub(crate) fn capacity_request_matches(
        object: &StoredObject,
        request: &CapacityExtensionRequest,
    ) -> Result<bool> {
        Ok(decode_extension(object, &object.exact_ref()?)?.request == *request)
    }

    pub(crate) fn validate_capacity_request(request: &CapacityExtensionRequest) -> Result<Limits> {
        proposed_limits(request)
    }

    /// The launcher holds execution ownership and authenticates its saved local
    /// operator through the callback on the exact transaction-bound heads. One
    /// transaction grows the state-head and pending
    /// control allocations, changes structural limits and appends the immutable
    /// authorization receipt.
    /// Preview rolls back all candidate changes, including the authority-read
    /// charge. Apply charges those reads against the explicitly requested limits;
    /// it need not spend exhausted old funding before admitting the extension.
    pub(crate) fn extend_runtime_capacity<F>(
        &mut self,
        request: &CapacityExtensionRequest,
        apply: bool,
        authorize: F,
    ) -> Result<StoredObject>
    where
        F: FnOnce(&StoredObject, &StoredObject) -> Result<()>,
    {
        let limits = proposed_limits(request)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut work = Work::load(&tx, &self.limits)?;
        work.admission()?;
        work.epoch(request.expected_epoch)?;
        if work.meta.limits != request.expected_limits {
            return Err(StoreError::LimitsMismatch);
        }
        let state: StoredObject = work.required("object", &request.expected_runtime.id)?;
        let launch: StoredObject = work.required("object", &request.expected_launch.id)?;
        if state.exact_ref()? != request.expected_runtime
            || launch.exact_ref()? != request.expected_launch
        {
            return Err(StoreError::RevisionConflict(
                "capacity extension fence".into(),
            ));
        }
        authorize(&state, &launch)?;
        let old_plan = work
            .runtime_control_plan()?
            .ok_or(StoreError::Unsupported("runtime control plan absent"))?;
        let previous = work.meta.capacity_extension.clone();
        let object_id = receipt_id(&request.id);
        if work.cell("capacity_extension", &object_id)?.is_some() {
            return Err(StoreError::Conflict(object_id));
        }
        let plan_id = format!("{object_id}:plan");
        let plan = RuntimeControlPlan {
            provisioned_at: state.exact_ref()?,
            operator: request.operator.clone(),
            max_object_bytes: limits
                .max_record_bytes
                .checked_sub(RUNTIME_CONTROL_HEADROOM)
                .ok_or(StoreError::TooLarge)?,
        };
        let requirements = crate::runtime::runtime_control_requirements(&state)?;
        // Load against the old allocation before switching limits. Committed
        // snapshots remain immutable; every current unused slot is enlarged,
        // including slots for a completed work that might later be resumed.
        let mut slots = Vec::new();
        for requirement in &requirements {
            for ending in [false, true] {
                let key = work.current_runtime_control_key(&requirement.work, ending)?;
                let cell = work
                    .cell("runtime_control", &key)?
                    .ok_or_else(|| StoreError::Missing(key.clone()))?;
                let slot: RuntimeControlSlot = decode_json(&cell.data)?;
                if slot.commit.is_none() {
                    work.runtime_control_slot(&key, &requirement.work, ending, &old_plan)?;
                    slots.push((key, cell, slot));
                }
            }
        }
        work.limits = &limits;
        work.meta.limits = limits.clone();
        work.returned(&(&state, &launch), limits.max_read_bytes, BudgetClass::Closeout)?;
        // This terminal-history scan is paid from the explicitly proposed
        // allowance. A rejected or preview transaction grants no new capacity.
        // Current operator authority and its read charge are in this transaction.
        work.require_quiescent_invocations()?;
        for resource in RESOURCES {
            work.meta.check_dimension(resource, BudgetClass::Ordinary)?;
            work.meta.check_dimension(resource, BudgetClass::Closeout)?;
        }
        // The mutable Ordinary head has a prepaid allocation too. Raising only
        // the global bound and Closeout slots leaves its next update capped at
        // the original allocation. Charge its growth while preserving its data,
        // class, revision and history. Equal-limit requests can repair an older
        // extension that did not enlarge this head, but cannot become no-op grants.
        let mut head = work
            .cell("object", RUNTIME_CONTROL_STATE_ID)?
            .ok_or_else(|| StoreError::Missing(RUNTIME_CONTROL_STATE_ID.into()))?;
        if head.class != BudgetClass::Ordinary {
            return Err(StoreError::Corrupt("runtime head allocation".into()));
        }
        let head_before = head.charge("object", RUNTIME_CONTROL_STATE_ID)?;
        head.capacity = head.capacity.max(limits.max_record_bytes);
        let ordinary_added = head
            .charge("object", RUNTIME_CONTROL_STATE_ID)?
            .checked_sub(head_before)
            .ok_or(StoreError::Overflow("runtime head growth"))?;
        if limits == request.expected_limits && ordinary_added == 0 {
            return Err(StoreError::Invalid(
                "runtime head is already aligned with its capacity".into(),
            ));
        }
        work.meta.ordinary.consumed.retained_bytes = work
            .meta
            .ordinary
            .consumed
            .retained_bytes
            .checked_add(ordinary_added)
            .ok_or(StoreError::Overflow("runtime head charge"))?;
        work.meta
            .check_dimension(Resource::RetainedBytes, BudgetClass::Ordinary)?;
        if ordinary_added > 0 {
            work.pending
                .insert(("object".into(), RUNTIME_CONTROL_STATE_ID.into()), head);
        }
        let mut added = 0u64;
        for (key, mut cell, mut slot) in slots {
            let before = cell.charge("runtime_control", &key)?;
            slot.max_object_bytes = plan.max_object_bytes;
            cell.capacity = cell.capacity.max(limits.max_record_bytes);
            cell.data = encode(&slot, cell.capacity)?;
            cell.capacity = cell.capacity.max(cell.data.len() as u64);
            let increase = cell
                .charge("runtime_control", &key)?
                .checked_sub(before)
                .ok_or(StoreError::Overflow("capacity must grow"))?;
            added = added
                .checked_add(increase)
                .ok_or(StoreError::Overflow("capacity allocation"))?;
            work.pending.insert(("runtime_control".into(), key), cell);
        }
        work.meta.closeout.consumed.retained_bytes = work
            .meta
            .closeout
            .consumed
            .retained_bytes
            .checked_add(added)
            .ok_or(StoreError::Overflow("capacity charge"))?;
        work.meta
            .check_dimension(Resource::RetainedBytes, BudgetClass::Closeout)?;
        work.insert_exact(
            "runtime_control_plan",
            &plan_id,
            &plan,
            BudgetClass::Closeout,
        )?;
        work.meta.runtime_control_plan = Some(plan_id.clone());
        work.check_runtime_control_capacity(&state, &plan)?;
        work.bump_epoch()?;
        let extension = Extension {
            version: 1,
            request: request.clone(),
            previous,
            limits: limits.clone(),
            control_plan: plan_id,
            control_capacity_added_bytes: added,
            ordinary_capacity_added_bytes: ordinary_added,
            navigation_allowance_added_bytes: limits.total.navigation_bytes
                - request.expected_limits.total.navigation_bytes,
            retained_allowance_added_bytes: limits.total.retained_bytes
                - request.expected_limits.total.retained_bytes,
            epoch: work.meta.epoch,
        };
        let object = StoredObject {
            id: object_id,
            revision: 1,
            kind: KIND.into(),
            body: serde_json::to_value(extension)?,
        };
        encode(&object, RECEIPT_BYTES)?;
        work.insert_exact(
            "capacity_extension",
            &object.id,
            &object,
            BudgetClass::Ordinary,
        )?;
        work.meta.capacity_extension = Some(object.exact_ref()?);
        if apply {
            work.flush()?;
        }
        drop(work);
        if apply {
            tx.commit()?;
            self.limits = limits;
        }
        Ok(object)
    }
}
