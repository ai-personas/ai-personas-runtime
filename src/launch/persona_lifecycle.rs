//! Explicit, snapshot-bound identity controls for the saved local operator.
//! These commands neither run inference nor resume any work. Protected pause
//! and quarantine use the separate prepaid CloseoutRuntime interface.
use super::*;

const KIND: &str = "persona-lifecycle-change-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Provision,
    Reactivate,
    ReleaseQuarantine,
    Retire,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub operator: String,
    pub persona: String,
    pub action: Action,
    pub reason: String,
    pub authorization_reference: String,
    pub authorize_change: bool,
    pub expected_runtime: s::ObjectRef,
    pub expected_launch: s::ObjectRef,
    pub expected_lifecycle_revision: Option<u64>,
}

fn valid_reference(reference: &s::ObjectRef, id: &str) -> bool {
    reference.id == id
        && reference.revision > 0
        && reference.sha256.len() == 64
        && reference
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Request {
    pub fn validate(&self, operator: &str) -> Result<()> {
        for id in [&self.id, &self.operator, &self.persona] {
            identity(id)?;
        }
        require(
            self.operator == operator && self.id.len() <= 96 && self.authorize_change,
            "explicit matching operator lifecycle authorization required",
        )?;
        for value in [&self.reason, &self.authorization_reference] {
            text(value, 4096)?;
            require(!value.trim().is_empty(), "lifecycle attribution required")?;
        }
        require(
            valid_reference(&self.expected_runtime, STATE_ID)
                && valid_reference(&self.expected_launch, ID),
            "invalid lifecycle snapshot references",
        )?;
        increment(self.expected_runtime.revision)?;
        require(
            serde_json::to_vec(self)?.len() <= 16_384,
            "lifecycle request exceeds bound",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    request: Request,
    request_sha256: String,
    created_at_unix: i64,
    target_runtime: s::ObjectRef,
    target_launch: s::ObjectRef,
    lifecycle: lifecycle::Record,
    revoked_computation_grants: Vec<String>,
    revoked_http_fetch_grants: Vec<String>,
}

impl Receipt {
    fn validate(&self) -> Result<()> {
        self.request.validate(&self.request.operator)?;
        require(
            self.version == 1
                && self.created_at_unix >= 0
                && self.request_sha256 == digest(&self.request)?
                && valid_reference(&self.target_runtime, STATE_ID)
                && self.target_runtime.revision
                    == increment(self.request.expected_runtime.revision)?
                && self.target_launch == self.request.expected_launch
                && self.lifecycle.revision
                    == self
                        .request
                        .expected_lifecycle_revision
                        .map_or(Ok(1), increment)?
                && self.lifecycle.last.runtime_revision == self.target_runtime.revision,
            "invalid lifecycle receipt",
        )
    }
}

fn authorize<'a>(
    state: &'a State,
    saved: &Saved,
    operator: &str,
    actor: &str,
) -> Result<&'a Persona> {
    admission_extension::authorize(state, saved, operator)?;
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("persona unavailable"))?;
    require(
        persona.created_by == operator,
        "lifecycle control requires the creating operator",
    )?;
    Ok(persona)
}

pub fn snapshot_at(root: &Path, operator: &str, actor: &str, now: i64) -> Result<Value> {
    identity(operator)?;
    identity(actor)?;
    require(now >= 0, "negative lifecycle snapshot time")?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let (runtime, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    let persona = authorize(&state, &saved, operator, actor)?;
    let blockers = lifecycle::retirement_blockers(&state, actor)?;
    let mut audience = admission(persona, now);
    audience.audience = BTreeSet::from([operator.into()]);
    let dispositions: Vec<_> = blockers.iter().map(|(work, reason)| {
        if persona.seed.audience.contains(operator)
            && persona.seed.grants.communicate_with.contains(operator)
            && work_exposure(&state, work).is_ok_and(|e| exposure_permitted(&state, &e, &audience))
        { json!({"work":work,"reason":reason}) }
        else { json!({"work":null,"reason":"obligation disposition unavailable under current permissions"}) }
    }).collect();
    Ok(json!({"operator":operator,"persona":actor,
        "expected_runtime":runtime.exact_ref()?,"expected_launch":launch.exact_ref()?,
        "expected_lifecycle_revision":persona.lifecycle.as_ref().map(|r|r.revision),
        "lifecycle":lifecycle::projection(persona),"latest_control":persona.lifecycle,
        "retirement_blockers":dispositions,
        "active_invocation":persona.active_invocation,
        "failed_attempt_held":persona.failed_attempt_hold.is_some(),
        "root_has_active_invocations":!active_invocations(&state).is_empty(),
        "snapshot_atomic":false,"capacity_reserved":false,"inference_performed":false,
        "scope":"Current identity controls and retained obligations. Provisioning and widening require an exact authorized request and existing capacity; this snapshot grants no execution or retirement authority."}))
}

fn revoke_grants(runtime: &mut s::StoredObject, field: &str, actor: &str) -> Vec<String> {
    let mut revoked = Vec::new();
    // Mutable indexing would insert null for an omitted legacy empty map.
    // Preserve absence: the typed State expects a map whenever the key exists.
    if let Some(grants) = runtime.body.get_mut(field).and_then(Value::as_object_mut) {
        for (id, grant) in grants {
            if grant["request"]["actor"] == actor && grant["revoked"] == false {
                grant["revoked"] = json!(true);
                revoked.push(id.clone());
            }
        }
    }
    revoked
}

pub fn change_at(root: &Path, request: &Request, apply: bool, now: i64) -> Result<Value> {
    request.validate(&request.operator)?;
    require(now >= 0, "negative lifecycle change time")?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let _owner = app
        .store
        .try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (head, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    let (current, _) = app.load()?;
    require(
        current.exact_ref()? == head.exact_ref()?,
        "lifecycle authority changed during inspection",
    )?;
    let persona = authorize(&state, &saved, &request.operator, &request.persona)?;
    let hash = digest(request)?;
    let id = format!("{KIND}:{}", request.id);
    if let Some(object) = app.store.read(&id)? {
        require(
            object.kind == KIND && object.id == id && object.revision == 1,
            "invalid lifecycle receipt object",
        )?;
        let receipt: Receipt = serde_json::from_value(object.body)?;
        receipt.validate()?;
        if receipt.request != *request || receipt.request_sha256 != hash {
            return Err(s::StoreError::Conflict(id).into());
        }
        return Ok(json!({"receipt":receipt,"replayed":true,"applied":true,
            "inference_performed":false,"resumed":false,"budget_reset":false,
            "current_eligibility":"inspect again; a historical receipt grants no renewed authority"}));
    }
    require(
        head.exact_ref()? == request.expected_runtime
            && launch.exact_ref()? == request.expected_launch
            && persona.lifecycle.as_ref().map(|r| r.revision)
                == request.expected_lifecycle_revision,
        "lifecycle snapshot changed",
    )?;
    require(
        active_invocations(&state).is_empty(),
        "active invocation requires completion or reconciliation",
    )?;
    app.store.require_terminal_invocations()?;
    let revision = request
        .expected_lifecycle_revision
        .map_or(Ok(1), increment)?;
    let runtime_revision = increment(head.revision)?;
    let action = match request.action {
        Action::Provision => "provision",
        Action::Reactivate => "reactivate",
        Action::ReleaseQuarantine => "release_quarantine",
        Action::Retire => "retire",
    };
    let notice = lifecycle::notice(
        action,
        &request.operator,
        &request.reason,
        runtime_revision,
        revision,
    );
    let mut record = match request.action {
        Action::Provision => {
            require(
                persona.lifecycle.is_none(),
                "persona lifecycle controls are already provisioned",
            )?;
            lifecycle::Record {
                revision,
                status: lifecycle::Status::Active,
                pause: None,
                quarantine: None,
                retirement: None,
                last: notice.clone(),
            }
        }
        _ => persona
            .lifecycle
            .clone()
            .ok_or_else(|| held("provision persona lifecycle controls first"))?,
    };
    match request.action {
        Action::Provision => {}
        Action::Reactivate => {
            require(
                record.status != lifecycle::Status::Active,
                "persona identity is already active",
            )?;
            record.status = lifecycle::Status::Active;
            // Quarantine, revoked grants, character and history are independent.
        }
        Action::ReleaseQuarantine => {
            require(record.quarantine.is_some(), "persona is not quarantined")?;
            record.quarantine = None;
        }
        Action::Retire => {
            require(
                record.status != lifecycle::Status::Retired,
                "persona is already retired",
            )?;
            require(
                persona.failed_attempt_hold.is_none(),
                "failed-attempt disposition blocks retirement",
            )?;
            require(lifecycle::retirement_blockers(&state, &request.persona)?.is_empty(),
                "retirement requires disposition of accepted work, consequential feedback, maintenance and execution; inspect persona-lifecycle-snapshot")?;
            record.status = lifecycle::Status::Retired;
            record.retirement = Some(notice.clone());
        }
    }
    record.revision = revision;
    record.last = notice;
    let mut runtime = head.clone();
    runtime.revision = runtime_revision;
    let target = &mut runtime.body["personas"][&request.persona];
    target["lifecycle"] = serde_json::to_value(&record)?;
    if request.action != Action::Provision {
        target["owner_epoch"] = json!(increment(persona.owner_epoch)?);
        lifecycle::clear_routes_object(target)?;
    }
    let (computation, http) = if request.action == Action::Retire {
        (
            revoke_grants(&mut runtime, "computation_grants", &request.persona),
            revoke_grants(&mut runtime, "http_fetch_grants", &request.persona),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    runtime.body["operator_events"].as_array_mut().ok_or_else(|| held("operator history unavailable"))?
        .push(json!({"kind":KIND,"request":request,"request_sha256":hash,
            "authorization_basis":"saved creating operator attestation; reference is not independent authentication",
            "obligations_resolved":false,"inference_performed":false,"resumed":false,"budget_reset":false,
            "revoked_computation_grants":computation,"revoked_http_fetch_grants":http}));
    let receipt = Receipt {
        version: 1,
        request: request.clone(),
        request_sha256: hash,
        created_at_unix: now,
        target_runtime: runtime.exact_ref()?,
        target_launch: launch.exact_ref()?,
        lifecycle: record,
        revoked_computation_grants: computation,
        revoked_http_fetch_grants: http,
    };
    receipt.validate()?;
    let changes = vec![
        s::Change {
            expected_revision: Some(head.revision),
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
