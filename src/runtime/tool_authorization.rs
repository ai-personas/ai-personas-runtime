//! Explicit, fenced updates to existing tool allocations. Original requests,
//! spent attempts and execution history remain intact. No update dispatches,
//! selects context, resumes a persona or adds work/Store funding.
use super::*;

const MAX_UPDATES: usize = 64;
const MAX_REQUEST_BYTES: usize = 16_384;
const MAX_RECEIPT_BYTES: usize = 32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGrantKind {
    Computation,
    HttpFetch,
}

impl ToolGrantKind {
    fn implementation_revision(self) -> String {
        match self {
            Self::Computation => crate::computation::engine_revision(),
            Self::HttpFetch => crate::http_fetch::client_revision(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolGrantUpdateFence {
    pub runtime: s::ObjectRef,
    pub grant_sha256: String,
    /// The implementation explicitly approved by this update, not the old grant.
    pub implementation_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolGrantUpdateTerms {
    Computation {
        guide: p::ExactRef,
        additional_attempts: u32,
    },
    HttpFetch {
        guide: p::ExactRef,
        additional_attempts: u32,
        origins: BTreeSet<String>,
        expires_at_unix: i64,
    },
}

impl ToolGrantUpdateTerms {
    fn kind(&self) -> ToolGrantKind {
        match self {
            Self::Computation { .. } => ToolGrantKind::Computation,
            Self::HttpFetch { .. } => ToolGrantKind::HttpFetch,
        }
    }

    fn guide(&self) -> &p::ExactRef {
        match self {
            Self::Computation { guide, .. } | Self::HttpFetch { guide, .. } => guide,
        }
    }

    fn additional_attempts(&self) -> u32 {
        match self {
            Self::Computation {
                additional_attempts,
                ..
            }
            | Self::HttpFetch {
                additional_attempts,
                ..
            } => *additional_attempts,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolGrantUpdateRequest {
    pub id: String,
    pub operator: String,
    pub grant: String,
    pub reason: String,
    /// Saved-operator attestation, not an independently authenticated document.
    pub authorization_reference: String,
    pub authorize_update: bool,
    pub expected: ToolGrantUpdateFence,
    pub terms: ToolGrantUpdateTerms,
}

impl<'de> Deserialize<'de> for ToolGrantUpdateRequest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(remote = "ToolGrantUpdateRequest", deny_unknown_fields)]
        struct ObjectRequest {
            id: String,
            operator: String,
            grant: String,
            reason: String,
            authorization_reference: String,
            authorize_update: bool,
            expected: ToolGrantUpdateFence,
            terms: ToolGrantUpdateTerms,
        }
        let value = Value::deserialize(deserializer)?;
        for path in [
            "",
            "/expected",
            "/expected/runtime",
            "/terms",
            "/terms/guide",
        ] {
            if !value.pointer(path).is_some_and(Value::is_object) {
                return Err(serde::de::Error::custom(format!(
                    "tool update requires a JSON object at {path:?}"
                )));
            }
        }
        if value["expected"]["runtime"]
            .as_object()
            .unwrap()
            .keys()
            .any(|key| !matches!(key.as_str(), "id" | "revision" | "sha256"))
        {
            return Err(serde::de::Error::custom(
                "unknown tool update runtime reference field",
            ));
        }
        ObjectRequest::deserialize(value).map_err(serde::de::Error::custom)
    }
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ToolGrantUpdateRequest {
    pub fn validate(&self, operator: &str) -> Result<()> {
        for value in [&self.id, &self.operator, &self.grant] {
            identity(value)?;
        }
        text(&self.reason, 4096)?;
        text(&self.authorization_reference, 4096)?;
        require(
            self.operator == operator
                && self.id.len() <= 96
                && self.authorize_update
                && !self.reason.trim().is_empty()
                && !self.authorization_reference.trim().is_empty()
                && self.expected.runtime.id == STATE_ID
                && self.expected.runtime.revision > 0
                && sha256(&self.expected.runtime.sha256)
                && sha256(&self.expected.grant_sha256)
                && sha256(&self.expected.implementation_revision)
                && self.terms.additional_attempts() <= 32
                && serde_json::to_vec(self)?.len() <= MAX_REQUEST_BYTES,
            "explicit bounded tool update and exact expected bindings required",
        )?;
        file_reference(self.terms.guide())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolGrantUpdateReceipt {
    pub version: u32,
    pub request: ToolGrantUpdateRequest,
    pub request_sha256: String,
    pub at_unix: i64,
    pub previous_interface: Value,
    pub resulting_interface: Value,
    pub used_preserved: u32,
    pub work_attempts_preserved: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantUpdate<T> {
    pub effective_request: T,
    pub implementation_revision: String,
    pub receipt: ToolGrantUpdateReceipt,
}

struct Current {
    actor: String,
    work: String,
    operator: String,
    used: u32,
    limit: u32,
    revoked: bool,
    updates: usize,
    grant_sha256: String,
    interface: Value,
}

fn current(state: &State, kind: ToolGrantKind, id: &str) -> Result<Current> {
    match kind {
        ToolGrantKind::Computation => {
            let grant = state
                .computation_grants
                .get(id)
                .ok_or_else(|| held("computation grant absent"))?;
            Ok(Current {
                actor: grant.request.actor.clone(),
                work: grant.request.work.clone(),
                operator: grant.operator.clone(),
                used: grant.used,
                limit: grant.current_request().attempts,
                revoked: grant.revoked,
                updates: grant.updates.len(),
                grant_sha256: digest(grant)?,
                interface: serde_json::to_value(tools::interface(grant)?)?,
            })
        }
        ToolGrantKind::HttpFetch => {
            let grant = state
                .http_fetch_grants
                .get(id)
                .ok_or_else(|| held("http_fetch grant absent"))?;
            Ok(Current {
                actor: grant.request.actor.clone(),
                work: grant.request.work.clone(),
                operator: grant.operator.clone(),
                used: grant.used,
                limit: grant.current_request().attempts,
                revoked: grant.revoked,
                updates: grant.updates.len(),
                grant_sha256: digest(grant)?,
                interface: serde_json::to_value(http_tools::interface(grant)?)?,
            })
        }
    }
}

fn recorded<'a>(state: &'a State, id: &str) -> Option<&'a ToolGrantUpdateReceipt> {
    state
        .computation_grants
        .values()
        .flat_map(|grant| grant.updates.iter().map(|update| &update.receipt))
        .chain(
            state
                .http_fetch_grants
                .values()
                .flat_map(|grant| grant.updates.iter().map(|update| &update.receipt)),
        )
        .find(|receipt| receipt.request.id == id)
}

fn outcome(receipt: &ToolGrantUpdateReceipt, status: &str) -> Value {
    json!({
        "status":status,"receipt":receipt,"replayed":status == "replayed",
        "recorded":status != "preview","metadata_only":true,
        "current_authority_asserted":false,"work_or_store_funding_added":false,
        "spent_attempts_reset":false,"context_selected":false,
        "persona_resumed":false,"inference_performed":false,"execution_dispatched":false
    })
}

fn require_settled(state: &State, actor: &str, work: &str) -> Result<()> {
    require(
        state
            .computation_jobs
            .values()
            .chain(state.http_fetch_jobs.values())
            .filter(|job| job.actor == actor && job.work == work)
            .all(|job| {
                job.observation.is_some()
                    && matches!(
                        job.state,
                        s::IntentState::Succeeded
                            | s::IntentState::Failed
                            | s::IntentState::Canceled
                    )
            }),
        "tool update requires settled executions; recover without replay first",
    )
}

impl Runtime {
    /// Trusted operator metadata. This neither supplies a guide to a persona nor
    /// establishes current operation-specific execution authority.
    pub fn tool_grant_update_snapshot(
        &mut self,
        operator: &str,
        kind: ToolGrantKind,
        id: &str,
        now: i64,
    ) -> Result<Value> {
        identity(operator)?;
        identity(id)?;
        let (old, state) = self.load()?;
        let current = current(&state, kind, id)?;
        require(
            current.operator == operator,
            "tool grant belongs to another operator",
        )?;
        let expected = ToolGrantUpdateFence {
            runtime: old.exact_ref()?,
            grant_sha256: current.grant_sha256,
            implementation_revision: kind.implementation_revision(),
        };
        Ok(json!({
            "operator":operator,"kind":kind,"grant":id,
            "actor":current.actor,"work":current.work,"expected":expected,
            "current_interface":current.interface,"used":current.used,
            "attempt_limit":current.limit,"revoked":current.revoked,
            "updates":current.updates,"maximum_updates":MAX_UPDATES,
            "ordinary_work_and_episode_attempts_left":
                need_proposals::ordinary_attempts_left(&state, &current.work, now),
            "maximum_additional_attempts":32,"maximum_remaining_after_increase":32,
            "metadata_only":true,"capacity_reserved":false,"inference_performed":false
        }))
    }

    /// Append an explicit authorization revision while retaining the original
    /// grant and all prior update receipts. Exact replay returns history only.
    pub fn update_tool_grant(
        &mut self,
        operator: &str,
        request: ToolGrantUpdateRequest,
        apply: bool,
        now: i64,
    ) -> Result<Value> {
        request.validate(operator)?;
        require(now >= 0, "invalid tool update time")?;
        let _owner = self.store.acquire_execution_owner()?;
        let (old, mut state) = self.load()?;
        if let Some(receipt) = recorded(&state, &request.id) {
            require(
                receipt.version == 1
                    && receipt.request == request
                    && receipt.request_sha256 == digest(&request)?,
                "tool update identity conflict",
            )?;
            return Ok(outcome(receipt, "replayed"));
        }
        let kind = request.terms.kind();
        let current = current(&state, kind, &request.grant)?;
        require(
            old.exact_ref()? == request.expected.runtime
                && current.grant_sha256 == request.expected.grant_sha256
                && kind.implementation_revision() == request.expected.implementation_revision,
            "tool update snapshot or implementation changed; take a fresh snapshot",
        )?;
        require(
            current.operator == operator
                && !current.revoked
                && current.updates < MAX_UPDATES
                && current.used <= current.limit,
            "tool grant revoked, foreign, inconsistent or update history full",
        )?;
        lifecycle::require_active(&state, &current.actor)?;
        let persona = state
            .personas
            .get(&current.actor)
            .ok_or_else(|| held("tool actor absent"))?;
        require(
            persona.active_invocation.is_none()
                && persona.focus.contains_key(&current.work)
                && state.board.work(&w::WorkId(current.work.clone()))?.phase == w::Phase::Open,
            "tool update requires idle actor and existing Open work focus",
        )?;
        require_settled(&state, &current.actor, &current.work)?;
        optional_activity::require_current(&state, &current.work, &current.actor, now)?;
        need_proposals::check_work(self, &state, &current.work, now)?;
        let a = admission(persona, now);
        let extra = request.terms.additional_attempts();
        let limit = current
            .limit
            .checked_add(extra)
            .ok_or_else(|| invalid("tool attempt limit overflow"))?;
        if extra > 0 {
            let remaining = limit - current.used;
            require(
                remaining <= 32
                    && u64::from(remaining)
                        <= need_proposals::ordinary_attempts_left(&state, &current.work, now),
                "tool cap increase exceeds finite remaining work/episode funding",
            )?;
        }
        let implementation_revision = &request.expected.implementation_revision;
        let resulting_interface = match &request.terms {
            ToolGrantUpdateTerms::Computation { guide, .. } => {
                tools::full_asset(&state, guide, &a)?;
                let mut terms = state.computation_grants[&request.grant]
                    .current_request()
                    .clone();
                terms.guide = guide.clone();
                terms.attempts = limit;
                serde_json::to_value(tools::interface_for(&terms, implementation_revision)?)?
            }
            ToolGrantUpdateTerms::HttpFetch {
                guide,
                origins,
                expires_at_unix,
                ..
            } => {
                require(
                    !origins.is_empty()
                        && origins.len() <= 8
                        && origins
                            .iter()
                            .all(|origin| crate::http_fetch::valid_origin(origin))
                        && *expires_at_unix > now
                        && *expires_at_unix <= now.saturating_add(http_tools::MAX_GRANT_SECONDS),
                    "HTTPS update requires explicit canonical origins and expiry within seven days",
                )?;
                http_tools::full_asset(&state, guide, &a)?;
                for origin in origins {
                    let mut destination = a.clone();
                    destination.destination = origin.clone();
                    http_tools::full_asset(&state, guide, &destination)?;
                }
                let mut terms = state.http_fetch_grants[&request.grant]
                    .current_request()
                    .clone();
                terms.guide = guide.clone();
                terms.attempts = limit;
                terms.origins = origins.clone();
                terms.expires_at_unix = *expires_at_unix;
                serde_json::to_value(http_tools::interface_for(&terms, implementation_revision)?)?
            }
        };
        require(
            resulting_interface != current.interface,
            "tool update makes no authorization change",
        )?;
        let receipt = ToolGrantUpdateReceipt {
            version: 1,
            request: request.clone(),
            request_sha256: digest(&request)?,
            at_unix: now,
            previous_interface: current.interface,
            resulting_interface,
            used_preserved: current.used,
            work_attempts_preserved: state.work_meta[&current.work].attempts_left,
        };
        require(
            serde_json::to_vec(&receipt)?.len() <= MAX_RECEIPT_BYTES,
            "tool update receipt exceeds retention bound",
        )?;
        if !apply {
            return Ok(outcome(&receipt, "preview"));
        }
        match &request.terms {
            ToolGrantUpdateTerms::Computation { guide, .. } => {
                let grant = state.computation_grants.get_mut(&request.grant).unwrap();
                let mut terms = grant.current_request().clone();
                terms.guide = guide.clone();
                terms.attempts = limit;
                grant.updates.push(GrantUpdate {
                    effective_request: terms,
                    implementation_revision: implementation_revision.clone(),
                    receipt: receipt.clone(),
                });
            }
            ToolGrantUpdateTerms::HttpFetch {
                guide,
                origins,
                expires_at_unix,
                ..
            } => {
                let grant = state.http_fetch_grants.get_mut(&request.grant).unwrap();
                let mut terms = grant.current_request().clone();
                terms.guide = guide.clone();
                terms.attempts = limit;
                terms.origins = origins.clone();
                terms.expires_at_unix = *expires_at_unix;
                grant.updates.push(GrantUpdate {
                    effective_request: terms,
                    implementation_revision: implementation_revision.clone(),
                    receipt: receipt.clone(),
                });
            }
        }
        state.operator_events.push(json!({
            "kind":"tool_grant_update","operator":operator,"update":request.id,
            "grant":request.grant,"tool_kind":kind,"request_sha256":receipt.request_sha256,
            "binding":receipt.resulting_interface["binding_revision"],
            "used_preserved":current.used,"additional_tool_attempts":extra,
            "work_or_store_funding_added":false
        }));
        self.save(&old, &state)?;
        Ok(outcome(&receipt, "applied"))
    }
}
