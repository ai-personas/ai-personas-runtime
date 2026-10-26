//! Explicit operator updates to historical assembled-context metadata.
//! No provider settings, selections, continuations or consumption are changed.
use super::*;

const KIND: &str = "context-capacity-extension";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCapacityRequest {
    pub id: String,
    pub operator: String,
    pub reason: String,
    pub authorization_reference: String,
    #[serde(default)]
    pub authorize_extension: bool,
    pub expected_runtime: s::ObjectRef,
    pub expected_launch: s::ObjectRef,
    pub expected_context_bytes: usize,
    pub new_context_bytes: usize,
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ContextCapacityRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        for id in [operator, &self.operator, &self.id] {
            identity(id)?;
        }
        require(
            self.operator == operator,
            "context metadata operator mismatch",
        )?;
        for value in [&self.reason, &self.authorization_reference] {
            text(value, 4096)?;
            require(
                !value.trim().is_empty(),
                "context extension attribution required",
            )?;
        }
        for (reference, id) in [
            (&self.expected_runtime, STATE_ID),
            (&self.expected_launch, ID),
        ] {
            require(
                reference.id == id && reference.revision > 0 && valid_hash(&reference.sha256),
                "invalid context extension exact reference",
            )?;
        }
        increment(self.expected_runtime.revision)?;
        Ok(())
    }
}

fn authorize(state: &State, saved: &Saved, operator: &str) -> Result<()> {
    admission_extension::authorize(state, saved, operator)?;
    admission_extension::validate_saved(state, saved)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextCapacityReceipt {
    version: u32,
    request: ContextCapacityRequest,
    request_sha256: String,
    created_at_unix: i64,
    target_runtime: s::ObjectRef,
    target_launch: s::ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximum_context_bytes_at_extension: Option<usize>,
}

impl ContextCapacityReceipt {
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
                && self.target_launch == self.request.expected_launch,
            "invalid context extension receipt",
        )
    }
}

pub fn snapshot_at(root: &Path, operator: &str) -> Result<Value> {
    identity(operator)?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    authorize(&state, &saved, operator)?;
    Ok(json!({
        "operator":operator,
        "expected_runtime":runtime.exact_ref()?,
        "expected_launch":launch.exact_ref()?,
        "expected_context_bytes":state.config.context_bytes,
        "maximum_context_bytes":null,"application_quotas_enforced":false,
        "snapshot_atomic":false,"capacity_reserved":false,
        "scope":"Historical launch-global context metadata. Context assembly has no application byte quota; metadata updates create no provider capacity, selection change or resume."
    }))
}

pub fn extend_at(
    root: &Path,
    request: &ContextCapacityRequest,
    apply: bool,
    now: i64,
) -> Result<Value> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative context extension time")?;
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
        "context extension authority changed during inspection",
    )?;
    authorize(&state, &saved, &request.operator)?;
    let hash = digest(request)?;
    let id = format!("{KIND}:{}", request.id);
    if let Some(object) = app.store.read(&id)? {
        require(
            object.id == id && object.kind == KIND && object.revision == 1,
            "invalid context extension receipt object",
        )?;
        let receipt: ContextCapacityReceipt = serde_json::from_value(object.body)?;
        receipt.validate()?;
        if receipt.request != *request || receipt.request_sha256 != hash {
            return Err(s::StoreError::Conflict(id).into());
        }
        return Ok(json!({"receipt":receipt,"replayed":true,"applied":true,
            "inference_performed":false,"resumed":false,"budget_reset":false}));
    }
    require(
        runtime_head.exact_ref()? == request.expected_runtime
            && launch_head.exact_ref()? == request.expected_launch
            && state.config.context_bytes == request.expected_context_bytes,
        "context extension heads or allocation changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation blocks context extension",
    )?;
    app.store.require_quiescent_invocations()?;
    let mut config = state.config.clone();
    config.context_bytes = request.new_context_bytes;
    config.validate()?;
    // Patch only the authorized limit and its operator event. Preserve omitted
    // legacy fields, persona-authored history, selections and every hold.
    let mut runtime = runtime_head.as_ref().clone();
    runtime.revision = increment(runtime.revision)?;
    runtime.body["config"]["context_bytes"] = json!(request.new_context_bytes);
    runtime.body["operator_events"].as_array_mut()
        .ok_or_else(|| held("invalid operator event history"))?
        .push(json!({"kind":KIND,"request":request,"request_sha256":hash,
            "authorization_basis":"saved operator attestation; reference is not independent authentication",
            "provider_limits_changed":false,"holds_changed":false,"selections_changed":false,
            "resumed":false,"inference_performed":false,"budget_reset":false}));
    let receipt = ContextCapacityReceipt {
        version: 1,
        request: request.clone(),
        request_sha256: hash,
        created_at_unix: now,
        target_runtime: runtime.exact_ref()?,
        target_launch: launch_head.exact_ref()?,
        maximum_context_bytes_at_extension: None,
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
    Ok(json!({"receipt":receipt,"replayed":false,"applied":apply,
        "inference_performed":false,"resumed":false,"budget_reset":false}))
}
