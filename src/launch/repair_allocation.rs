//! Explicit finite repair suballowances within existing work attempts.
//! Allocation changes neither continuation nor any controlling resource balance.
use super::*;

const KIND: &str = "work-repair-allocation";
const MAX_DECLARED: u64 = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RepairAllocationRequest {
    pub id: String,
    pub operator: String,
    pub work_id: String,
    pub reason: String,
    pub authorization_reference: String,
    #[serde(default)]
    pub authorize_allocation: bool,
    pub expected: WorkResourceGrantFence,
    pub expected_declared_repair_attempts: u64,
    pub additional_repair_attempts: u64,
}

impl<'de> Deserialize<'de> for RepairAllocationRequest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(remote = "RepairAllocationRequest", deny_unknown_fields)]
        struct ObjectRequest {
            id: String,
            operator: String,
            work_id: String,
            reason: String,
            authorization_reference: String,
            #[serde(default)]
            authorize_allocation: bool,
            expected: WorkResourceGrantFence,
            expected_declared_repair_attempts: u64,
            additional_repair_attempts: u64,
        }
        let value = Value::deserialize(deserializer)?;
        for path in [
            "",
            "/expected",
            "/expected/runtime",
            "/expected/launch",
            "/expected/remaining",
            "/expected/remaining/navigation",
        ] {
            if !value.pointer(path).is_some_and(Value::is_object) {
                return Err(serde::de::Error::custom(
                    "repair allocation requires JSON objects",
                ));
            }
        }
        ObjectRequest::deserialize(value).map_err(serde::de::Error::custom)
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn receipt_id(id: &str) -> String {
    format!("{KIND}:{id}")
}

impl RepairAllocationRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        for id in [operator, &self.operator, &self.id, &self.work_id] {
            identity(id)?;
        }
        require(
            self.operator == operator,
            "repair allocation operator or identity mismatch",
        )?;
        for value in [&self.reason, &self.authorization_reference] {
            text(value, 4096)?;
            require(
                !value.trim().is_empty(),
                "repair allocation attribution required",
            )?;
        }
        for (reference, id) in [
            (&self.expected.runtime, STATE_ID),
            (&self.expected.launch, ID),
        ] {
            require(
                reference.id == id && reference.revision > 0 && valid_hash(&reference.sha256),
                "invalid repair allocation exact reference",
            )?;
        }
        increment(self.expected.runtime.revision)?;
        require(
            self.expected.work_revision > 0
                && self.expected.work_epoch > 0
                && self.expected.max_total_steps > 0
                && self.expected.steps <= self.expected.max_total_steps
                && (1..=128).contains(&self.expected.max_steps)
                && self.expected.remaining.repair_attempts
                    <= self.expected_declared_repair_attempts
                && (1..=MAX_DECLARED).contains(&self.additional_repair_attempts),
            "invalid repair allocation counters",
        )?;
        let declared = self.declared_after()?;
        let remaining = self.remaining_after()?;
        require(
            declared <= MAX_DECLARED && remaining.repair_attempts <= declared,
            "cumulative repair allowance exceeds supported bound",
        )?;
        remaining.validate()?;
        Ok(())
    }

    fn declared_after(&self) -> Result<u64> {
        self.expected_declared_repair_attempts
            .checked_add(self.additional_repair_attempts)
            .ok_or_else(|| invalid("repair declaration overflow"))
    }

    fn remaining_after(&self) -> Result<WorkAllocation> {
        let mut remaining = self.expected.remaining;
        remaining.repair_attempts = remaining
            .repair_attempts
            .checked_add(self.additional_repair_attempts)
            .ok_or_else(|| invalid("repair suballowance overflow"))?;
        Ok(remaining)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairAllocationReceipt {
    pub version: u32,
    pub request: RepairAllocationRequest,
    pub request_sha256: String,
    pub created_at_unix: i64,
    pub target_runtime: s::ObjectRef,
    pub target_launch: s::ObjectRef,
    pub declared_repair_attempts_after: u64,
    pub remaining_after: WorkAllocation,
    pub attempts_added: u64,
    pub resumed: bool,
    pub inference_performed: bool,
    pub capacity_reserved: bool,
}

impl RepairAllocationReceipt {
    fn validate(&self) -> Result<()> {
        let request = &self.request;
        request.validate(&request.operator)?;
        require(
            self.version == 1
                && self.created_at_unix >= 0
                && self.request_sha256 == digest(request)?
                && self.target_runtime.id == STATE_ID
                && self.target_runtime.revision == increment(request.expected.runtime.revision)?
                && valid_hash(&self.target_runtime.sha256)
                && self.target_launch == request.expected.launch.store_ref()
                && self.declared_repair_attempts_after == request.declared_after()?
                && self.remaining_after == request.remaining_after()?
                && self.attempts_added == 0
                && !self.resumed
                && !self.inference_performed
                && !self.capacity_reserved,
            "invalid repair allocation receipt",
        )
    }
}

/// Metered inspection without orphan recovery. This snapshot reserves nothing.
pub fn work_repair_snapshot_at(root: &Path, operator: &str, work: &str) -> Result<Value> {
    identity(operator)?;
    identity(work)?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    let expected = work_resource_grant::fence(&runtime, &state, &launch, &saved, work)?;
    Ok(json!({
        "operator":operator, "work_id":work, "expected":expected,
        "expected_declared_repair_attempts":state.work_meta[work].declared_repair_attempts(&state.config),
        "maximum_declared_repair_attempts":MAX_DECLARED,
        "snapshot_atomic":false, "capacity_reserved":false,
        "scope":"Restricted primary selection repair within existing attempts, navigation and Store funds. Explicit continuation permission and fresh admission remain required."
    }))
}

/// One guarded atomic allocation and immutable receipt, or a historical replay.
/// An allocation opts this work into one pre-dispatch repair preparation when
/// ordinary preparation records eligible overflow. It never retries a dispatch.
pub fn allocate_work_repair_at(
    root: &Path,
    request: &RepairAllocationRequest,
    apply: bool,
    now: i64,
) -> Result<Value> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative repair allocation time")?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (runtime_head, state) = app.load()?;
    let (launch_head, saved) = load(&mut app)?;
    let (current_runtime, _) = app.load()?;
    require(
        current_runtime.exact_ref()? == runtime_head.exact_ref()?,
        "repair allocation authority changed during inspection",
    )?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    let hash = digest(request)?;
    let id = receipt_id(&request.id);
    if let Some(object) = app.store.read(&id)? {
        require(
            object.id == id && object.kind == KIND && object.revision == 1,
            "invalid repair allocation receipt object",
        )?;
        let receipt: RepairAllocationReceipt = serde_json::from_value(object.body)?;
        receipt.validate()?;
        if receipt.request != *request || receipt.request_sha256 != hash {
            return Err(s::StoreError::Conflict(id).into());
        }
        return Ok(json!({"receipt":receipt,"replayed":true,"applied":true}));
    }
    require(
        work_resource_grant::fence(
            &runtime_head,
            &state,
            &launch_head,
            &saved,
            &request.work_id,
        )? == request.expected
            && state.work_meta[&request.work_id].declared_repair_attempts(&state.config)
                == request.expected_declared_repair_attempts,
        "repair allocation state, work or balance changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks repair allocation",
    )?;
    app.store.require_quiescent_invocations()?;
    let declared = request.declared_after()?;
    let remaining = request.remaining_after()?;
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    runtime.body["work_meta"][&request.work_id]["repair_attempts_declared"] = json!(declared);
    runtime.body["work_meta"][&request.work_id]["repair_attempts_left"] =
        json!(remaining.repair_attempts);
    runtime.body["operator_events"].as_array_mut()
        .ok_or_else(|| held("invalid operator event history"))?
        .push(json!({"kind":KIND,"request":request,"request_sha256":hash,
            "authorization_basis":"saved operator attestation; reference is not independent authentication",
            "declared_repair_attempts_after":declared,"remaining_after":remaining,
            "attempts_added":0,"holds_changed":false,"resumed":false,"inference_performed":false}));
    let receipt = RepairAllocationReceipt {
        version: 1,
        request: request.clone(),
        request_sha256: hash,
        created_at_unix: now,
        target_runtime: runtime.exact_ref()?,
        target_launch: launch_head.exact_ref()?,
        declared_repair_attempts_after: declared,
        remaining_after: remaining,
        attempts_added: 0,
        resumed: false,
        inference_performed: false,
        capacity_reserved: false,
    };
    receipt.validate()?;
    let changes = vec![
        s::Change {
            expected_revision: Some(runtime_head.revision),
            object: runtime,
            depends_on: vec![receipt.target_launch.clone()],
        },
        s::Change {
            expected_revision: None,
            object: s::StoredObject {
                id,
                revision: 1,
                kind: KIND.into(),
                body: serde_json::to_value(&receipt)?,
            },
            depends_on: vec![
                receipt.target_runtime.clone(),
                receipt.target_launch.clone(),
            ],
        },
    ];
    let minimum = s::Resources {
        retained_bytes: 1,
        ..s::Resources::default()
    };
    if apply {
        app.store.commit_intake_delegation_batch(changes, minimum)?;
    } else {
        app.store
            .preview_intake_delegation_batch(changes, minimum)?;
    }
    Ok(json!({"receipt":receipt,"replayed":false,"applied":apply}))
}
