//! Explicit current-source selection, independent of retained delegation ancestry.
//! Selection makes no model call and changes no participation or continuation.
use super::*;
use intake_delegation::IntakeReference;

const KIND: &str = "launch-intake-source-selection-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub operator: String,
    pub work: String,
    pub source: p::need::GrantReference,
    pub expected_runtime: IntakeReference,
    pub expected_launch: IntakeReference,
}

impl Request {
    fn validate(&self, operator: &str) -> Result<()> {
        for id in [&self.id, &self.operator, &self.work] { identity(id)?; }
        require(self.operator == operator, "intake source selection operator mismatch")?;
        self.source.validate()?;
        self.expected_runtime.validate(STATE_ID)?;
        self.expected_launch.validate(ID)
    }
}

pub fn snapshot_at(root: &Path, operator: &str, work: &str) -> Result<Value> {
    identity(operator)?;
    identity(work)?;
    let (mut app, snapshot) = inspection::Snapshot::open(root)?;
    admission_extension::authorize(&snapshot.state, &snapshot.saved, operator)?;
    require(snapshot.saved.works.contains_key(work), "unknown launch work")?;
    let (_, registry) = intake_delegation::registry(&mut app)?;
    let now = unix_now()?;
    let sources = registry.grants.values().map(|grant| Ok(json!({
        "source":{"id":grant.request.id,"sha256":grant.source_sha256()?},
        "purpose_at_issue":grant.request.purpose,
        "historical_work_ids":grant.request.proposal_work_ids,
        "automatically_issued":grant.automatically_issued,
        "active":grant.active_at(now),
        "retained_entry_works":snapshot.state.intake_episodes.get(&grant.request.id)
            .map(|episode| episode.roots().collect::<Vec<_>>()).unwrap_or_default()
    }))).collect::<Result<Vec<_>>>()?;
    snapshot.unchanged(&app)?;
    Ok(json!({"operator":operator,"work":work,
        "expected_runtime":IntakeReference::of(&snapshot.runtime)?,
        "expected_launch":IntakeReference::of(&snapshot.launch)?,
        "selected":need_proposals::reference(&snapshot.state, work),
        "retained_sources":snapshot.state.intake_episodes.iter()
            .filter(|(_, episode)| episode.work_binding(work).is_some())
            .map(|(id, _)| id).collect::<Vec<_>>(),
        "sources":sources,"inference_performed":false,
        "scope":"Choose any exact active source for this work. Historical work lists are provenance, not allowlists. Selection preserves earlier proposals, children, charges, task scope and every persona's Stop/Pause or participation choice."}))
}

pub fn select_at(root: &Path, operator: &str, request: &Request, now: i64) -> Result<Value> {
    request.validate(operator)?;
    require(now >= 0, "negative source selection time")?;
    let mut app = Runtime::open_for_launch_inspection(root)?;
    let _owner = app.store.try_execution_owner_without_recovery()?
        .ok_or_else(|| held("execution owner busy"))?;
    let (head, state) = app.load()?;
    let (launch, saved) = load(&mut app)?;
    admission_extension::authorize(&state, &saved, operator)?;
    let hash = digest(request)?;
    let id = format!("{KIND}:{}", s::sha256_bytes(request.id.as_bytes()));
    if let Some(record) = app.store.read(&id)? {
        let retained: Request = serde_json::from_value(record.body["request"].clone())?;
        require(record.kind == KIND && record.revision == 1
            && record.body["request_sha256"] == hash && digest(&retained)? == hash,
            "source selection identity conflicts with its retained request")?;
        return Ok(json!({"receipt":record.exact_ref()?,"replayed":true,
            "work":request.work,"selected_by_this_request":request.source,
            "current_selection":need_proposals::reference(&state, &request.work),
            "selection_applied":false,"continuations_changed":false,"inference_performed":false}));
    }
    require(IntakeReference::of(&head)? == request.expected_runtime
        && IntakeReference::of(&launch)? == request.expected_launch,
        "source selection snapshot changed")?;
    require(saved.works.contains_key(&request.work), "unknown launch work")?;
    let previous = need_proposals::reference(&state, &request.work);
    let mut next = state;
    need_proposals::select_source(&mut app, &mut next, &request.work, &request.source, now)?;
    next.operator_events.push(json!({"kind":"intake_source_selection","id":request.id,
        "operator":operator,"work":request.work,"previous":previous,"selected":request.source,
        "at_unix":now,"continuations_changed":false,"budget_reset":false}));
    let registry = next.intake_episodes[&request.source.id].last_preparation_registry.store_ref();
    let runtime = state_object(&head, &next)?;
    let record = s::StoredObject { id, revision: 1, kind: KIND.into(), body: json!({
        "request":request,"request_sha256":hash,"previous":previous,
        "runtime_after":runtime.exact_ref()?,"registry":registry,"at_unix":now
    }) };
    let receipt = record.exact_ref()?;
    app.store.compare_and_swap_batch(vec![
        s::Change { expected_revision: Some(head.revision), object: runtime,
            depends_on: vec![launch.exact_ref()?, registry] },
        s::Change { expected_revision: None, object: record, depends_on: Vec::new() },
    ])?;
    Ok(json!({"receipt":receipt,"replayed":false,"work":request.work,
        "selected_by_this_request":request.source,"current_selection":request.source,
        "selection_applied":true,"continuations_changed":false,"inference_performed":false}))
}
