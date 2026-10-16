//! Explicit operator additions to one existing standalone work's logical
//! allowance. No new work, repair allowance, continuation, resume or inference.
//! Store consumption, reservations, uncertainty and protected funds are retained.
//! Snapshot/preview use metered reads; normal open/owner recovery can occur.
use super::*;

const RECEIPT_KIND: &str = "work-resource-grant";
const MAX_REQUEST_BYTES: usize = 16_384;
const AUTH_BASIS: &str =
    "saved operator attestation; authorization reference is not independent authentication";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResourceGrantFence {
    pub runtime: intake_delegation::IntakeReference,
    pub launch: intake_delegation::IntakeReference,
    pub work_revision: u64,
    pub work_epoch: u64,
    pub remaining: WorkAllocation,
    pub steps: u64,
    pub max_total_steps: u64,
    pub max_steps: usize,
}

/// An addition, never a replacement balance. Repair capacity is deliberately
/// absent from this contract. Each field remains subject to configured bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResourceIncrease {
    pub attempts: u64,
    pub navigation: f::NavigationLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResourceGrantRequest {
    pub id: String,
    pub operator: String,
    pub work_id: String,
    pub reason: String,
    pub authorization_reference: String,
    pub authorize_grant: bool,
    pub expected: WorkResourceGrantFence,
    pub increase: WorkResourceIncrease,
    /// The minimal nondecreasing cap sufficient for the resulting work balance.
    /// This cap is launch-global, not a reservation exclusive to this work.
    pub new_max_total_steps: u64,
    /// Operator snapshot only; the supervisor must enforce cumulative usage and
    /// all pending/unknown reserves before every subsequent provider admission.
    pub token_attestation: intake_delegation::IntakeTokenAttestation,
}

// Serde's ordinary struct decoder also accepts positional sequences. Enforce
// object-only JSON at every nested request boundary before the CLI opens state,
// without changing the shared types' contracts for unrelated APIs.
impl<'de> Deserialize<'de> for WorkResourceGrantRequest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(remote = "WorkResourceGrantRequest", deny_unknown_fields)]
        struct ObjectRequest {
            id: String,
            operator: String,
            work_id: String,
            reason: String,
            authorization_reference: String,
            authorize_grant: bool,
            expected: WorkResourceGrantFence,
            increase: WorkResourceIncrease,
            new_max_total_steps: u64,
            token_attestation: intake_delegation::IntakeTokenAttestation,
        }
        let value = Value::deserialize(deserializer)?;
        for path in [
            "",
            "/expected",
            "/expected/runtime",
            "/expected/launch",
            "/expected/remaining",
            "/expected/remaining/navigation",
            "/increase",
            "/increase/navigation",
            "/token_attestation",
        ] {
            if !value.pointer(path).is_some_and(Value::is_object) {
                return Err(serde::de::Error::custom(format!(
                    "work resource grant requires a JSON object at {path:?}"
                )));
            }
        }
        ObjectRequest::deserialize(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResourceGrantReceipt {
    pub version: u32,
    pub request_sha256: String,
    pub request: WorkResourceGrantRequest,
    pub created_at_unix: i64,
    pub target_runtime: s::ObjectRef,
    pub target_launch: s::ObjectRef,
    pub remaining_after: WorkAllocation,
    pub carried_episode_bindings: Vec<String>,
    pub authorization_basis: String,
    pub capacity_reserved: bool,
    pub inference_performed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkResourceGrantOutcome {
    pub receipt: WorkResourceGrantReceipt,
    pub replayed: bool,
}

fn receipt_id(id: &str) -> String {
    format!("work-resource-grant:{id}")
}

fn nonblank(value: &str, maximum: usize) -> Result<()> {
    text(value, maximum)?;
    require(
        !value.trim().is_empty(),
        "nonblank work grant text required",
    )
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_ref(reference: &intake_delegation::IntakeReference, id: &str) -> Result<()> {
    require(
        reference.id == id && reference.revision > 0 && valid_sha256(&reference.sha256),
        "invalid work grant exact reference",
    )?;
    increment(reference.revision)?;
    Ok(())
}

fn add_remaining(
    remaining: WorkAllocation,
    increase: WorkResourceIncrease,
) -> Result<WorkAllocation> {
    fn add(a: usize, b: usize) -> Result<usize> {
        a.checked_add(b)
            .ok_or_else(|| invalid("work navigation addition overflow"))
    }
    Ok(WorkAllocation {
        attempts: remaining
            .attempts
            .checked_add(increase.attempts)
            .ok_or_else(|| invalid("work attempt addition overflow"))?,
        repair_attempts: remaining.repair_attempts,
        navigation: f::NavigationLimits {
            calls: add(remaining.navigation.calls, increase.navigation.calls)?,
            scanned_bytes: add(
                remaining.navigation.scanned_bytes,
                increase.navigation.scanned_bytes,
            )?,
            inspected_revisions: add(
                remaining.navigation.inspected_revisions,
                increase.navigation.inspected_revisions,
            )?,
            matches: add(remaining.navigation.matches, increase.navigation.matches)?,
            result_bytes: add(
                remaining.navigation.result_bytes,
                increase.navigation.result_bytes,
            )?,
        },
    })
}

impl WorkResourceGrantRequest {
    /// Structural/attribution checks run before opening state. Current saved
    /// operator authorization and exact heads are checked again during planning.
    pub fn validate(&self, operator: &str) -> Result<()> {
        for id in [operator, &self.operator, &self.id, &self.work_id] {
            identity(id)?;
        }
        require(
            self.operator == operator && self.id.len() <= 96 && receipt_id(&self.id).len() <= 256,
            "work grant operator or receipt identity mismatch",
        )?;
        nonblank(&self.reason, 4096)?;
        nonblank(&self.authorization_reference, 4096)?;
        require(
            self.authorize_grant,
            "explicit authorize_grant true required",
        )?;
        check_ref(&self.expected.runtime, STATE_ID)?;
        check_ref(&self.expected.launch, ID)?;
        let expected = &self.expected;
        require(
            expected.work_revision > 0
                && expected.work_epoch > 0
                && expected.remaining.repair_attempts == 0
                && expected.max_total_steps > 0
                && expected.steps <= expected.max_total_steps
                && (1..=128).contains(&expected.max_steps)
                && (1..=128).contains(&self.increase.attempts),
            "invalid work grant counters, nonzero repair allowance or zero attempt addition",
        )?;
        let after = add_remaining(expected.remaining, self.increase)?;
        let needed_steps = expected
            .steps
            .checked_add(after.attempts)
            .ok_or_else(|| invalid("work grant lifetime bound overflow"))?;
        let limits = storage_limits();
        let ordinary_ceiling = limits
            .total
            .attempts
            .checked_sub(limits.protected_closeout.attempts)
            .ok_or_else(|| invalid("invalid ordinary attempt ceiling"))?;
        require(
            self.new_max_total_steps == expected.max_total_steps.max(needed_steps)
                && self.new_max_total_steps <= ordinary_ceiling,
            "work grant requires the minimal nondecreasing lifetime bound",
        )?;
        admission_extension::validate_token_attestation(
            self.token_attestation.conservative_tokens_used,
            self.token_attestation.conservative_token_ceiling,
            self.token_attestation.per_call_reserve_tokens,
        )?;
        require(
            serde_json::to_vec(self)?.len() <= MAX_REQUEST_BYTES,
            "work grant request exceeds bound",
        )
    }
}

fn target<'a>(state: &'a State, saved: &Saved, work: &str) -> Result<&'a WorkMeta> {
    identity(work)?;
    require(
        saved.works.contains_key(work),
        "work absent from saved launch",
    )?;
    require(
        !need_proposals::bound(state, work),
        "episode-bound work requires separate explicit episode authority",
    )?;
    require(
        matches!(
            state.board.work(&w::WorkId(work.into()))?.phase,
            w::Phase::Open
        ),
        "work resource grant requires an existing Open work",
    )?;
    let meta = state
        .work_meta
        .get(work)
        .ok_or_else(|| held("work resource account absent"))?;
    require(
        meta.original.id == work,
        "work original need identity mismatch",
    )?;
    require(
        meta.repair_attempts_left == 0,
        "ordinary-only resource grant does not support retained repair allowance",
    )?;
    Ok(meta)
}

fn fence(
    runtime: &s::StoredObject,
    state: &State,
    launch: &s::StoredObject,
    saved: &Saved,
    work: &str,
) -> Result<WorkResourceGrantFence> {
    let meta = target(state, saved, work)?;
    let version = state.board.work(&w::WorkId(work.into()))?.version;
    Ok(WorkResourceGrantFence {
        runtime: intake_delegation::IntakeReference::of(runtime)?,
        launch: intake_delegation::IntakeReference::of(launch)?,
        work_revision: version.revision,
        work_epoch: version.epoch,
        remaining: WorkAllocation {
            attempts: meta.attempts_left,
            navigation: meta.navigation_left,
            repair_attempts: meta.repair_attempts_left,
        },
        steps: saved.steps,
        max_total_steps: saved.setup.max_total_steps,
        max_steps: saved.setup.max_steps,
    })
}

/// Metered, non-atomic inspection only. No eligibility, reservation or grant is
/// created. Normal Runtime opening may recover orphaned dispatches.
pub fn work_resource_grant_snapshot(
    app: &mut Runtime,
    operator: &str,
    work: &str,
) -> Result<Value> {
    identity(operator)?;
    identity(work)?;
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    Ok(json!({
        "operator": operator,
        "work_id": work,
        "expected": fence(&runtime, &state, &launch, &saved, work)?,
        "maximum_remaining": WorkAllocation::configured(&state.config),
        "navigation_per_call": state.config.navigation_per_call,
        "repair_grants_supported": false,
        "capacity_reserved": false,
        "snapshot_atomic": false
    }))
}

fn validate_receipt(receipt: &WorkResourceGrantReceipt) -> Result<()> {
    let request = &receipt.request;
    request.validate(&request.operator)?;
    let launch_changed = request.new_max_total_steps != request.expected.max_total_steps;
    let launch_revision = if launch_changed {
        increment(request.expected.launch.revision)?
    } else {
        request.expected.launch.revision
    };
    require(
        receipt.version == 1
            && receipt.created_at_unix >= 0
            && receipt.request_sha256 == digest(request)?
            && receipt.authorization_basis == AUTH_BASIS
            && !receipt.capacity_reserved
            && !receipt.inference_performed
            && receipt.target_runtime.id == STATE_ID
            && receipt.target_runtime.revision == increment(request.expected.runtime.revision)?
            && valid_sha256(&receipt.target_runtime.sha256)
            && receipt.target_launch.id == ID
            && receipt.target_launch.revision == launch_revision
            && valid_sha256(&receipt.target_launch.sha256)
            && (launch_changed || receipt.target_launch == request.expected.launch.store_ref())
            && receipt.remaining_after
                == add_remaining(request.expected.remaining, request.increase)?
            && (launch_changed || receipt.carried_episode_bindings.is_empty())
            && receipt
                .carried_episode_bindings
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && serde_json::to_vec(receipt)?.len() <= 65_536,
        "invalid work resource grant receipt",
    )?;
    for id in &receipt.carried_episode_bindings {
        identity(id)?;
    }
    Ok(())
}

fn replay(
    app: &mut Runtime,
    request: &WorkResourceGrantRequest,
    hash: &str,
) -> Result<Option<WorkResourceGrantReceipt>> {
    replay_with_read_hook(app, request, hash, |_| Ok(()))
}

// The private hook lets synthetic tests force a real atomic Store change at
// this read boundary; the production path always supplies the empty hook.
fn replay_with_read_hook(
    app: &mut Runtime,
    request: &WorkResourceGrantRequest,
    hash: &str,
    after_runtime_read: impl FnOnce(&mut Runtime) -> Result<()>,
) -> Result<Option<WorkResourceGrantReceipt>> {
    let id = receipt_id(&request.id);
    let Some(object) = app.store.read(&id)? else {
        return Ok(None);
    };
    let (runtime_head, state) = app.load()?;
    after_runtime_read(app)?;
    let (_, saved) = load(app)?;
    // Immutable, monotonic revisions establish that this runtime and launch
    // coexisted at the launch read. Never authorize a mixed pair of heads.
    let (current_runtime, _) = app.load()?;
    require(
        current_runtime.exact_ref()? == runtime_head.exact_ref()?,
        "work grant replay authority changed during inspection",
    )?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    require(
        object.id == id && object.kind == RECEIPT_KIND && object.revision == 1,
        "invalid work resource grant receipt object",
    )?;
    let receipt: WorkResourceGrantReceipt = serde_json::from_value(object.body)?;
    validate_receipt(&receipt)?;
    if receipt.request_sha256 != hash || receipt.request != *request {
        return Err(s::StoreError::Conflict(id).into());
    }
    Ok(Some(receipt))
}

struct Planned {
    receipt: WorkResourceGrantReceipt,
    changes: Vec<s::Change>,
    minimum: s::Resources,
}

fn plan(
    app: &mut Runtime,
    request: &WorkResourceGrantRequest,
    hash: String,
    now: i64,
) -> Result<Planned> {
    let (runtime_head, state) = app.load()?;
    let (launch_head, mut saved) = load(app)?;
    admission_extension::authorize(&state, &saved, &request.operator)?;
    admission_extension::validate_saved(&state, &saved)?;
    require(
        fence(
            &runtime_head,
            &state,
            &launch_head,
            &saved,
            &request.work_id,
        )? == request.expected,
        "work grant runtime, launch, work version or balance changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks work resource grant",
    )?;
    app.store.require_terminal_invocations()?;
    let after = add_remaining(request.expected.remaining, request.increase)?;
    after.require_within_config(&state.config)?;

    let previous_binding = intake_delegation::episode_launch_binding(&saved)?;
    saved.setup.max_total_steps = request.new_max_total_steps;
    let next_binding = intake_delegation::episode_launch_binding(&saved)?;
    let launch_changed = request.new_max_total_steps != request.expected.max_total_steps;
    let mut launch = launch_head.clone();
    if launch_changed {
        launch.revision = increment(launch.revision)?;
        launch.body["setup"]["max_total_steps"] = json!(request.new_max_total_steps);
    }

    // Patch only the declared balance and append-only evidence. Preserve all
    // other retained JSON, including omitted historical defaults and holds.
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    runtime.body["work_meta"][&request.work_id]["attempts_left"] = json!(after.attempts);
    runtime.body["work_meta"][&request.work_id]["navigation_left"] = json!(after.navigation);
    let mut carried = Vec::new();
    if previous_binding != next_binding {
        for (id, episode) in &state.intake_episodes {
            if episode.launch_binding_sha256 == previous_binding {
                runtime.body["intake_episodes"][id]["launch_binding_sha256"] = json!(next_binding);
                carried.push(id.clone());
            }
        }
    }
    runtime.body["operator_events"]
        .as_array_mut()
        .ok_or_else(|| held("invalid operator event history"))?
        .push(json!({
            "kind": RECEIPT_KIND, "id": request.id, "operator": request.operator,
            "work": request.work_id, "request_sha256": hash,
            "authorization_basis": AUTH_BASIS, "request": request,
            "remaining_after": after,
            "lifetime_cap_scope": "launch_global_not_reserved_for_target_work",
            "episode_binding_transition": {
                "from": previous_binding, "to": next_binding, "carried_episodes": carried
            },
            "holds_changed": false, "resumed": false, "inference_performed": false
        }));
    let receipt = WorkResourceGrantReceipt {
        version: 1,
        request_sha256: hash,
        request: request.clone(),
        created_at_unix: now,
        target_runtime: runtime.exact_ref()?,
        target_launch: launch.exact_ref()?,
        remaining_after: after,
        carried_episode_bindings: carried,
        authorization_basis: AUTH_BASIS.into(),
        capacity_reserved: false,
        inference_performed: false,
    };
    validate_receipt(&receipt)?;
    let mut changes = vec![s::Change {
        expected_revision: Some(runtime_head.revision),
        object: runtime,
        depends_on: vec![receipt.target_launch.clone()],
    }];
    if launch_changed {
        changes.push(s::Change {
            expected_revision: Some(launch_head.revision),
            object: launch,
            depends_on: Vec::new(),
        });
    }
    changes.push(s::Change {
        expected_revision: None,
        object: s::StoredObject {
            id: receipt_id(&request.id),
            revision: 1,
            kind: RECEIPT_KIND.into(),
            body: serde_json::to_value(&receipt)?,
        },
        depends_on: vec![
            receipt.target_runtime.clone(),
            receipt.target_launch.clone(),
        ],
    });
    let navigation_bytes = u64::try_from(request.increase.navigation.scanned_bytes)
        .ok()
        .and_then(|scanned| {
            u64::try_from(request.increase.navigation.result_bytes)
                .ok()
                .and_then(|result| scanned.checked_add(result))
        })
        .ok_or_else(|| invalid("work grant navigation byte bound overflow"))?;
    // Runtime preparation reserves twice the serialized request body, and Store
    // independently bounds that body. This conservative inspection quote needs
    // no provider/credential resolution and does not promise later admission.
    let input_bytes = storage_limits()
        .max_body_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_mul(request.increase.attempts))
        .ok_or_else(|| invalid("work grant input bound overflow"))?;
    let output_bytes = state
        .config
        .transport_output_reserve_bytes
        .checked_mul(request.increase.attempts)
        .ok_or_else(|| invalid("work grant output bound overflow"))?;
    Ok(Planned {
        receipt,
        changes,
        // A feasibility floor after this batch's real metadata charges, never a
        // reservation or complete quote for future rendered/provider requests.
        minimum: s::Resources {
            attempts: request.increase.attempts,
            input_bytes,
            output_bytes,
            navigation_bytes,
            retained_bytes: 1,
        },
    })
}

/// Check the same guarded candidate batch as apply, then roll it back. Metered
/// reads and normal execution-owner recovery remain real; no grant is applied.
pub fn preview_work_resource_grant(
    app: &mut Runtime,
    request: &WorkResourceGrantRequest,
    now: i64,
) -> Result<WorkResourceGrantReceipt> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative work grant time")?;
    let hash = digest(request)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(receipt);
    }
    let _owner = app.store.acquire_execution_owner()?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(receipt);
    }
    let planned = plan(app, request, hash, now)?;
    app.store
        .preview_intake_delegation_batch(planned.changes, planned.minimum)?;
    Ok(planned.receipt)
}

/// Apply one explicit addition with an immutable idempotency receipt. Existing
/// work identity, holds, continuation and every previous resource charge remain.
pub fn grant_work_resources(
    app: &mut Runtime,
    request: &WorkResourceGrantRequest,
    now: i64,
) -> Result<WorkResourceGrantOutcome> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative work grant time")?;
    let hash = digest(request)?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(WorkResourceGrantOutcome {
            receipt,
            replayed: true,
        });
    }
    let _owner = app.store.acquire_execution_owner()?;
    if let Some(receipt) = replay(app, request, &hash)? {
        return Ok(WorkResourceGrantOutcome {
            receipt,
            replayed: true,
        });
    }
    let planned = plan(app, request, hash, now)?;
    app.store
        .commit_intake_delegation_batch(planned.changes, planned.minimum)?;
    Ok(WorkResourceGrantOutcome {
        receipt: planned.receipt,
        replayed: false,
    })
}

#[cfg(test)]
#[path = "work_resource_grant_tests.rs"]
mod tests;
