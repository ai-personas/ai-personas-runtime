//! Explicit growth of local provider transport allowances. The account, model,
//! response bound and persona state are preserved; no request is dispatched.
use super::*;

const KIND: &str = "transport-capacity-extension";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportBounds {
    pub max_stream_bytes: usize,
    pub max_events: usize,
    pub timeout_ms: u64,
}
impl TransportBounds {
    fn from_provider(provider: &DirectProviderSettings) -> Self {
        Self {
            max_stream_bytes: provider.limits.max_stream_bytes,
            max_events: provider.limits.max_events,
            timeout_ms: provider.limits.timeout_ms,
        }
    }
    fn validate(&self) -> Result<()> {
        // Match the existing provider ceilings; validate the complete prospective
        // provider too before mutation. These are local bounds, not token funds.
        require(
            (1..=64 * 1024 * 1024).contains(&self.max_stream_bytes)
                && (1..=100_000).contains(&self.max_events)
                && (1..=86_400_000).contains(&self.timeout_ms),
            "transport capacity exceeds provider configuration bounds",
        )
    }
}

fn exact_reference<'de, D>(deserializer: D) -> std::result::Result<s::ObjectRef, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reference {
        id: String,
        revision: u64,
        sha256: String,
    }
    let reference = Reference::deserialize(deserializer)?;
    Ok(s::ObjectRef {
        id: reference.id,
        revision: reference.revision,
        sha256: reference.sha256,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportCapacityRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub authorization_reference: String,
    pub authorize_extension: bool,
    #[serde(deserialize_with = "exact_reference")]
    pub expected_runtime: s::ObjectRef,
    #[serde(deserialize_with = "exact_reference")]
    pub expected_launch: s::ObjectRef,
    pub expected_transport_output_reserve_bytes: u64,
    pub expected_limits: TransportBounds,
    pub new_limits: TransportBounds,
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl TransportCapacityRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        for id in [operator, &self.operator, &self.id] {
            identity(id)?;
        }
        require(
            self.operator == operator && self.id.len() <= 96 && self.authorize_extension,
            "explicit matching operator transport authorization required",
        )?;
        for value in [&self.reason, &self.authorization_reference] {
            text(value, 4096)?;
            require(
                !value.trim().is_empty(),
                "transport extension attribution required",
            )?;
        }
        for (reference, id) in [
            (&self.expected_runtime, STATE_ID),
            (&self.expected_launch, ID),
        ] {
            require(
                reference.id == id && reference.revision > 0 && valid_hash(&reference.sha256),
                "invalid transport extension exact reference",
            )?;
            increment(reference.revision)?;
        }
        self.expected_limits.validate()?;
        self.new_limits.validate()?;
        require(
            self.expected_transport_output_reserve_bytes
                >= self.expected_limits.max_stream_bytes as u64
                && self.new_limits.max_stream_bytes >= self.expected_limits.max_stream_bytes
                && self.new_limits.max_events >= self.expected_limits.max_events
                && self.new_limits.timeout_ms >= self.expected_limits.timeout_ms
                && self.new_limits != self.expected_limits,
            "transport extension must increase an allowance without reducing another",
        )?;
        require(
            serde_json::to_vec(self)?.len() <= 16_384,
            "transport request exceeds bound",
        )
    }
    fn new_reserve(&self) -> u64 {
        self.expected_transport_output_reserve_bytes
            .max(self.new_limits.max_stream_bytes as u64)
    }
}
fn authorize(state: &State, saved: &Saved, operator: &str) -> Result<()> {
    admission_extension::authorize(state, saved, operator)?;
    admission_extension::validate_saved(state, saved)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransportCapacityReceipt {
    version: u32,
    request: TransportCapacityRequest,
    request_sha256: String,
    created_at_unix: i64,
    target_runtime: s::ObjectRef,
    target_launch: s::ObjectRef,
    transport_output_reserve_bytes_after: u64,
}
impl TransportCapacityReceipt {
    fn validate(&self) -> Result<()> {
        self.request.validate(&self.request.operator)?;
        require(
            self.version == 1
                && self.created_at_unix >= 0
                && self.request_sha256 == digest(&self.request)?
                && self.target_runtime.id == STATE_ID
                && self.target_runtime.revision
                    == increment(self.request.expected_runtime.revision)?
                && valid_hash(&self.target_runtime.sha256)
                && self.target_launch.id == ID
                && self.target_launch.revision == increment(self.request.expected_launch.revision)?
                && valid_hash(&self.target_launch.sha256)
                && self.transport_output_reserve_bytes_after == self.request.new_reserve(),
            "invalid transport extension receipt",
        )
    }
}

pub fn snapshot_at(root: &Path, operator: &str) -> Result<Value> {
    identity(operator)?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    authorize(&state, &saved, operator)?;
    Ok(json!({"operator":operator,
        "expected_runtime":runtime.exact_ref()?, "expected_launch":launch.exact_ref()?,
        "expected_transport_output_reserve_bytes":state.config.transport_output_reserve_bytes,
        "expected_limits":TransportBounds::from_provider(&saved.execution_provider()?),
        "snapshot_atomic":false,"capacity_reserved":false,
        "scope":"Local transport allowances and their per-call output reservation. No response-size, provider identity, resource refill or continuation change."
    }))
}

pub fn extend_at(
    root: &Path,
    request: &TransportCapacityRequest,
    apply: bool,
    now: i64,
) -> Result<Value> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative transport extension time")?;
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
        "transport extension authority changed during inspection",
    )?;
    authorize(&state, &saved, &request.operator)?;
    let hash = digest(request)?;
    let id = format!("{KIND}:{}", request.id);
    if let Some(object) = app.store.read(&id)? {
        require(
            object.id == id && object.kind == KIND && object.revision == 1,
            "invalid transport extension receipt object",
        )?;
        let receipt: TransportCapacityReceipt = serde_json::from_value(object.body)?;
        receipt.validate()?;
        if receipt.request != *request || receipt.request_sha256 != hash {
            return Err(s::StoreError::Conflict(id).into());
        }
        return Ok(json!({"receipt":receipt,"replayed":true,"applied":true,
            "inference_performed":false,"resumed":false,"budget_reset":false}));
    }
    let mut provider = saved.execution_provider()?;
    require(
        runtime_head.exact_ref()? == request.expected_runtime
            && launch_head.exact_ref()? == request.expected_launch
            && state.config.transport_output_reserve_bytes
                == request.expected_transport_output_reserve_bytes
            && TransportBounds::from_provider(&provider) == request.expected_limits,
        "transport extension heads or allowances changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks transport extension",
    )?;
    app.store.require_quiescent_invocations()?;
    provider.limits.max_stream_bytes = request.new_limits.max_stream_bytes;
    provider.limits.max_events = request.new_limits.max_events;
    provider.limits.timeout_ms = request.new_limits.timeout_ms;
    provider.validate()?;
    let mut config = state.config.clone();
    config.transport_output_reserve_bytes = request.new_reserve();
    config.validate()?;
    // Patch only these exact fields, preserving omitted legacy defaults,
    // persona-authored history, account evidence and uncertain charges.
    let mut runtime = runtime_head.clone();
    runtime.revision = increment(runtime.revision)?;
    runtime.body["config"]["transport_output_reserve_bytes"] = json!(request.new_reserve());
    runtime.body["operator_events"].as_array_mut()
        .ok_or_else(|| held("invalid operator event history"))?
        .push(json!({"kind":KIND,"request":request,"request_sha256":hash,
            "authorization_basis":"saved operator attestation; reference is not independent authentication",
            "response_bound_changed":false,"provider_identity_changed":false,
            "resumed":false,"inference_performed":false,"budget_reset":false}));
    let mut launch = launch_head.clone();
    launch.revision = increment(launch.revision)?;
    let limits = &mut launch.body["setup"]["provider"]["limits"];
    limits["max_stream_bytes"] = json!(request.new_limits.max_stream_bytes);
    limits["max_events"] = json!(request.new_limits.max_events);
    limits["timeout_ms"] = json!(request.new_limits.timeout_ms);
    let receipt = TransportCapacityReceipt {
        version: 1,
        request: request.clone(),
        request_sha256: hash,
        created_at_unix: now,
        target_runtime: runtime.exact_ref()?,
        target_launch: launch.exact_ref()?,
        transport_output_reserve_bytes_after: request.new_reserve(),
    };
    receipt.validate()?;
    let changes = vec![
        s::Change {
            expected_revision: Some(runtime_head.revision),
            object: runtime,
            depends_on: vec![receipt.target_launch.clone()],
        },
        s::Change {
            expected_revision: Some(launch_head.revision),
            object: launch,
            depends_on: vec![receipt.target_runtime.clone()],
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
    Ok(json!({"receipt":receipt,"replayed":false,"applied":apply,
        "inference_performed":false,"resumed":false,"budget_reset":false}))
}
