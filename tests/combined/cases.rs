//! Deterministic, synthetic, local mechanism traces. No scenario inference.
//! Run the ignored matrix test explicitly with a private manifest and output directory.
use super::*;
use ai_personas::mock_effects as m;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

const D_STATE_ID: &str = "runtime-state-v1";
const D_REPORT_BYTES: usize = 16 * 1024 * 1024;
const D_LOG_BYTES: usize = 12 * 1024 * 1024;
const D_CELL_BYTES: usize = 4 * 1024 * 1024 + 16 * 1024;
const D_ROW_BYTES: usize = 8 * 1024 * 1024;
const D_SCHEDULES: [&str; 6] = [
    "normal_completion",
    "event_before_wait",
    "event_after_wait",
    "interrupted_dispatch_late_reconciliation",
    "freeze_pause_resume",
    "warning_correction_reuse",
];
const D_TARGET: &str = "Synthetic historical token alpha; exact retained subject.";
const D_BASIS: &str = "Synthetic supplied token beta; fixture observation only.";
const D_QUALIFICATION: &str = "Synthetic token gamma; accompany the exact historical subject.";
const D_RESULT: &str = "Synthetic local observation: token delta.\nExact whitespace:  λ\\n";
const D_ARTIFACT: &str =
    "Synthetic authored token epsilon; no conceptual solution or quality claim.";
type DResult<T> = std::result::Result<T, String>;

fn d_error(error: impl Display) -> String {
    let text = error.to_string();
    let mut end = text.len().min(2048);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
fn d_require(condition: bool, message: &str) -> DResult<()> {
    if condition {
        Ok(())
    } else {
        Err(format!("contract mismatch: {message}"))
    }
}
fn d_json(value: impl Serialize) -> DResult<Value> {
    serde_json::to_value(value).map_err(d_error)
}
fn d_identity(value: &str) -> DResult<()> {
    d_require(
        !value.is_empty()
            && value.len() <= 96
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'),
        "fixture identity must be bounded lowercase ASCII without path separators",
    )
}
fn d_text(value: &str, maximum: usize) -> DResult<()> {
    d_require(
        !value.trim().is_empty() && value.len() <= maximum,
        "fixture text bound",
    )
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DTask {
    id: String,
    private_description: String,
    request: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DCase {
    id: String,
    task_id: String,
    roster_size: usize,
    schedule: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DCaps {
    mock_effects: usize,
    synthetic_persona_decisions: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DManifest {
    acceptance: Vec<String>,
    caps: DCaps,
    claims_excluded: Vec<String>,
    domain_files_publication: bool,
    evidence_type: String,
    external_effects: bool,
    provider_calls: u64,
    status: String,
    task_configurations: Vec<DTask>,
    traces: Vec<DCase>,
    version: u32,
}
impl DManifest {
    fn validate(&self) -> DResult<()> {
        d_require(self.version == 1, "manifest version")?;
        d_require(
            self.caps.mock_effects == 2 && self.caps.synthetic_persona_decisions == 32,
            "manifest caps",
        )?;
        d_require(
            !self.domain_files_publication && !self.external_effects && self.provider_calls == 0,
            "local-only manifest",
        )?;
        d_require(
            self.status == "configured_not_executed",
            "manifest execution status",
        )?;
        d_require(
            self.evidence_type == "deterministic synthetic mechanism fixtures",
            "manifest evidence type",
        )?;
        for values in [&self.acceptance, &self.claims_excluded] {
            d_require(
                !values.is_empty() && values.len() <= 16,
                "manifest statement count",
            )?;
            for value in values {
                d_text(value, 2048)?;
            }
        }
        d_require(
            self.task_configurations.len() == 2 && self.traces.len() == 24,
            "complete private matrix size",
        )?;
        let mut tasks = BTreeSet::new();
        let mut requests = BTreeSet::new();
        for task in &self.task_configurations {
            d_identity(&task.id)?;
            d_text(&task.request, 32_768)?;
            d_text(&task.private_description, 32_768)?;
            d_require(tasks.insert(task.id.clone()), "duplicate task identity")?;
            d_require(
                requests.insert(task.request.clone()),
                "task requests must actually differ",
            )?;
        }
        let mut actual = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for case in &self.traces {
            d_identity(&case.id)?;
            d_identity(&case.task_id)?;
            d_require(tasks.contains(&case.task_id), "unknown trace task")?;
            d_require([3, 5].contains(&case.roster_size), "trace roster")?;
            d_require(
                D_SCHEDULES.contains(&case.schedule.as_str()),
                "trace schedule",
            )?;
            d_require(
                case.id == format!("{}-{}-{}", case.task_id, case.roster_size, case.schedule),
                "trace identity binding",
            )?;
            d_require(ids.insert(case.id.clone()), "duplicate trace identity")?;
            d_require(
                actual.insert((
                    case.task_id.clone(),
                    case.roster_size,
                    case.schedule.clone(),
                )),
                "duplicate combination",
            )?;
        }
        let mut expected = BTreeSet::new();
        for task in tasks {
            for roster in [3, 5] {
                for schedule in D_SCHEDULES {
                    expected.insert((task.clone(), roster, schedule.to_owned()));
                }
            }
        }
        d_require(
            actual == expected,
            "matrix has every requested combination exactly once",
        )
    }
}

// Private paths must already have existing parents. A missing output leaf may
// be created, but traversal, symlinks, shared permissions and repository paths
// are refused. Reports never overwrite an existing file.
fn d_private_path(path: &Path, directory: bool, create_leaf: bool) -> DResult<PathBuf> {
    d_require(path.is_absolute(), "private paths must be absolute")?;
    let mut checked = PathBuf::new();
    let components: Vec<_> = path.components().collect();
    for (index, part) in components.iter().enumerate() {
        match part {
            Component::RootDir | Component::Normal(_) => checked.push(part.as_os_str()),
            _ => return Err("private path contains unsupported traversal".into()),
        }
        match fs::symlink_metadata(&checked) {
            Ok(metadata) => d_require(!metadata.file_type().is_symlink(), "private path symlink")?,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && directory
                    && create_leaf
                    && index + 1 == components.len() =>
            {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700).create(&checked).map_err(d_error)?;
            }
            Err(error) => return Err(d_error(error)),
        }
    }
    let cwd = fs::canonicalize(std::env::current_dir().map_err(d_error)?).map_err(d_error)?;
    let ancestors_from = if directory {
        checked.as_path()
    } else {
        checked.parent().ok_or("private file parent absent")?
    };
    for ancestor in ancestors_from.ancestors().chain(cwd.ancestors()) {
        for marker in [".git", ".hg", ".svn"] {
            match fs::symlink_metadata(ancestor.join(marker)) {
                Ok(_) => d_require(
                    !checked.starts_with(ancestor),
                    "private data must be outside the checkout",
                )?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(d_error(error)),
            }
        }
    }
    d_require(
        !checked.starts_with(&cwd),
        "private data must be outside the working directory",
    )?;
    let metadata = fs::symlink_metadata(&checked).map_err(d_error)?;
    d_require(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "private path type",
    )?;
    d_require(
        metadata.permissions().mode() & 0o077 == 0,
        "private path permissions",
    )?;
    d_require(
        metadata.uid() == unsafe { libc::geteuid() },
        "private path owner",
    )?;
    Ok(checked)
}
fn d_output() -> DResult<Option<PathBuf>> {
    std::env::var_os("PERSONAS_FIXTURE_OUTPUT_DIR")
        .map(|path| d_private_path(Path::new(&path), true, true))
        .transpose()
}
fn d_keep() -> DResult<bool> {
    match std::env::var("PERSONAS_FIXTURE_KEEP_STATES") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value == "0" => Ok(false),
        Ok(value) if value == "1" => Ok(true),
        _ => Err("PERSONAS_FIXTURE_KEEP_STATES must be absent, 0 or 1".into()),
    }
}
fn d_write_report(directory: &Path, case: &DCase, report: &Value) -> DResult<()> {
    let bytes = serde_json::to_vec(report).map_err(d_error)?;
    d_require(bytes.len() <= D_REPORT_BYTES, "16 MiB report ceiling")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join(format!("{}.json", case.id)))
        .map_err(d_error)?;
    file.write_all(&bytes).map_err(d_error)?;
    file.sync_all().map_err(d_error)
}

// Bounded, read-only diagnostic access. It never opens a Store owner, spends a
// runtime allowance, or changes accepted state. SQL returns no oversized body.
fn d_connection(root: &Path) -> DResult<Connection> {
    Connection::open_with_flags(root.join("state.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(d_error)
}
fn d_rows(root: &Path, namespace: &str, maximum: usize) -> DResult<Vec<(String, Value)>> {
    let connection = d_connection(root)?;
    let mut statement = connection.prepare(
        "SELECT k,length(data),CASE WHEN length(data)<=?1 THEN data ELSE NULL END FROM store_cells WHERE ns=?2 ORDER BY k LIMIT ?3"
    ).map_err(d_error)?;
    let mut rows = statement
        .query(rusqlite::params![
            D_CELL_BYTES as i64,
            namespace,
            (maximum + 1) as i64
        ])
        .map_err(d_error)?;
    let mut result = Vec::new();
    let mut total = 0usize;
    while let Some(row) = rows.next().map_err(d_error)? {
        d_require(result.len() < maximum, "diagnostic row count")?;
        let length: i64 = row.get(1).map_err(d_error)?;
        d_require(
            length >= 0 && length as usize <= D_CELL_BYTES,
            "diagnostic cell bound",
        )?;
        total = total
            .checked_add(length as usize)
            .ok_or("diagnostic size overflow")?;
        d_require(total <= D_ROW_BYTES, "diagnostic aggregate read bound")?;
        let bytes: Option<Vec<u8>> = row.get(2).map_err(d_error)?;
        let value =
            serde_json::from_slice(&bytes.ok_or("oversized diagnostic cell")?).map_err(d_error)?;
        result.push((row.get(0).map_err(d_error)?, value));
    }
    Ok(result)
}
fn d_cell(root: &Path, namespace: &str, key: &str) -> DResult<Value> {
    let connection = d_connection(root)?;
    let (length, bytes): (i64, Option<Vec<u8>>) = connection.query_row(
        "SELECT length(data),CASE WHEN length(data)<=?1 THEN data ELSE NULL END FROM store_cells WHERE ns=?2 AND k=?3",
        rusqlite::params![D_CELL_BYTES as i64, namespace, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(d_error)?;
    d_require(
        length >= 0 && length as usize <= D_CELL_BYTES,
        "exact diagnostic cell bound",
    )?;
    serde_json::from_slice(&bytes.ok_or("oversized exact diagnostic cell")?).map_err(d_error)
}
fn d_state(root: &Path) -> DResult<State> {
    serde_json::from_value(d_cell(root, "object", D_STATE_ID)?["body"].clone()).map_err(d_error)
}
fn d_meta(root: &Path) -> DResult<Value> {
    let connection = d_connection(root)?;
    let bytes: Option<Vec<u8>> = connection.query_row(
        "SELECT CASE WHEN length(data)<=65536 THEN data ELSE NULL END FROM store_meta WHERE singleton=1",
        [], |row| row.get(0),
    ).map_err(d_error)?;
    serde_json::from_slice(&bytes.ok_or("diagnostic accounting bound")?).map_err(d_error)
}
fn d_projection(root: &Path) -> DResult<Value> {
    if !root.join("state.sqlite3").exists() {
        return Ok(json!({"initialized":false,"synthetic_persona_admissions":0}));
    }
    let head: s::StoredObject =
        serde_json::from_value(d_cell(root, "object", D_STATE_ID)?).map_err(d_error)?;
    let state: State = serde_json::from_value(head.body.clone()).map_err(d_error)?;
    let mut personas = BTreeMap::new();
    let mut admissions = 0u64;
    for (id, persona) in &state.personas {
        admissions = admissions
            .checked_add(persona.admission_serial)
            .ok_or("admission counter overflow")?;
        let mut focuses = BTreeMap::new();
        for (work, focus) in &persona.focus {
            focuses.insert(work, json!({"permit":focus.permit,"ordinary_continuation":focus.ordinary_continuation,
                "continuation":focus.continuation,"pending_observations":focus.pending_observations,
                "fragments":focus.fragments,"assets":focus.assets,"last_judgment":focus.last_judgment,
                "protected_maintenance":focus.protected_maintenance}));
        }
        personas.insert(
            id,
            json!({"character_revision":persona.character.as_ref().map(|c| &c.revision),
            "owner_epoch":persona.owner_epoch,"admission_serial":persona.admission_serial,
            "active_invocation":persona.active_invocation,"focus":focuses}),
        );
    }
    let works: BTreeMap<_, _> = state.board.works().iter().map(|(id, work)| {
        let meta = &state.work_meta[&id.0];
        (id.0.clone(), json!({"board":work,"completion_supported":state.board.completion_supported(id),
            "attempts_left":meta.attempts_left,"navigation_left":meta.navigation_left,
            "repair_attempts_left":meta.repair_attempts_left,"artifact_sources":meta.artifact_sources,
            "request_sha256":s::sha256_bytes(meta.original.request.as_bytes())}))
    }).collect();
    let sources: BTreeMap<_, _> = state.sources.iter().map(|(id, source)| (id.clone(), json!({
        "owner":source.owner,"protected":source.protected,"retired":source.retired,"head":source.head,
        "revisions":source.revisions.iter().map(|revision| json!({"reference":revision.record.reference(f::Representation::Utf8),
            "bytes":revision.record.bytes.len(),"bytes_sha256":s::sha256_bytes(&revision.record.bytes),
            "origin":revision.origin,"qualifications":revision.record.qualifications})).collect::<Vec<_>>()
    }))).collect();
    let warnings: BTreeMap<_, _> = state.semantic_warnings.iter().map(|(id, warning)| (id.clone(), json!({
        "active":warning.active(),"revision":warning.current_revision(),"adoption_revision":warning.revision,
        "actor":warning.authored.persona,"work":warning.authored.work,"invocation":warning.authored.invocation,
        "operation":warning.authored.operation,"marker":warning.marker,
        "history":warning.history.iter().map(|event| json!({"revision":event.revision,"prior_revision":event.prior_revision,
            "actor":event.authored.persona,"work":event.authored.work,"invocation":event.authored.invocation,
            "operation":event.authored.operation,"marker":event.marker,"bindings":event.bindings})).collect::<Vec<_>>()
    }))).collect();
    let observations = state.observations.values().map(|observation| Ok(json!({
        "id":observation.id,"actor":observation.actor,"work":observation.work,"kind":observation.kind,
        "invocation":observation.invocation,"sources":observation.sources,"qualified_roots":observation.qualified_roots,
        "payload_sha256":s::sha256_bytes(&serde_json::to_vec(&observation.payload).map_err(d_error)?)
    }))).collect::<DResult<Vec<_>>>()?;
    let mut invocations = Vec::new();
    for (_, mut invocation) in d_rows(root, "invocation", 34)? {
        if let Some(request) = invocation
            .as_object_mut()
            .and_then(|value| value.remove("request"))
        {
            invocation["request_sha256"] = json!(s::sha256_bytes(
                &serde_json::to_vec(&request).map_err(d_error)?
            ));
            if request["request"]["profile"] == "synthetic" {
                invocation["prepared_context_checkpoint"] = request["request"]["id"].clone();
                invocation["prompt_sha256"] = request["prompt_sha256"].clone();
            } else {
                invocation["request"] = request;
            }
        }
        invocations.push(invocation);
    }
    let mut controls = Vec::new();
    for (key, value) in d_rows(root, "runtime_control", 16)? {
        let commit = &value["commit"];
        let object = if commit.is_null() {
            None
        } else {
            let object: s::StoredObject =
                serde_json::from_value(commit["object"].clone()).map_err(d_error)?;
            Some(object.exact_ref().map_err(d_error)?)
        };
        controls.push(
            json!({"key":key,"work":value["work"],"ending":value["ending"],
            "request":commit["request"],"receipt":commit["receipt"],"committed_object":object}),
        );
    }
    let result = json!({"initialized":true,"head":head.exact_ref().map_err(d_error)?,
        "ledger":d_meta(root)?,"synthetic_persona_admissions":admissions,"personas":personas,"works":works,
        "sources":sources,"warnings":warnings,"accepted_bindings":state.accepted_bindings,
        "observations":observations,"local_deliveries":state.local_deliveries,
        "bridge":{"basis_references":state.local_evidence.bases.values().map(|b| &b.reference).collect::<Vec<_>>(),
            "ingestions":state.local_evidence.ingestions,"pending":state.local_evidence.pending},
        "invocations":invocations,"intents":d_rows(root,"intent",2)?,
        "holds":d_rows(root,"reservation",128)?,"control_slots":controls});
    d_require(
        serde_json::to_vec(&result).map_err(d_error)?.len() <= 256 * 1024,
        "checkpoint projection ceiling",
    )?;
    Ok(result)
}
fn d_control(prepared: &Prepared) -> DResult<Value> {
    let tail = prepared
        .prompt
        .split_once("\nCONTROL\n")
        .ok_or("CONTROL missing")?
        .1;
    serde_json::from_str(
        tail.split_once("\nQUALIFIED_CONTEXT\n")
            .ok_or("context missing")?
            .0,
    )
    .map_err(d_error)
}
fn d_context(prepared: &Prepared) -> DResult<Value> {
    serde_json::from_str(
        prepared
            .prompt
            .split_once("\nQUALIFIED_CONTEXT\n")
            .ok_or("context missing")?
            .1,
    )
    .map_err(d_error)
}
fn d_entries(prepared: &Prepared) -> DResult<Vec<Value>> {
    let mut result = Vec::new();
    let context = d_context(prepared)?;
    if let Some(entries) = context["entries"].as_array() {
        result.extend(entries.iter().cloned());
    }
    let control = d_control(prepared)?;
    for observation in control["actual_observations"]
        .as_array()
        .ok_or("observations missing")?
    {
        if let Some(entries) = observation["payload"]["result"]["entries"].as_array() {
            result.extend(entries.iter().cloned());
        }
        if let Some(bundles) = observation["current_required_qualifications"].as_array() {
            for bundle in bundles {
                if let Some(entries) = bundle["entries"].as_array() {
                    result.extend(entries.iter().cloned());
                }
            }
        }
    }
    Ok(result)
}
fn d_has_body(prepared: &Prepared, reference: &p::ExactRef, text: &str) -> DResult<bool> {
    Ok(d_entries(prepared)?.iter().any(|entry| {
        entry["reference"]["source"] == reference.source
            && entry["reference"]["revision"] == reference.revision
            && entry["text"] == text
    }))
}
fn d_contains_id(value: &Value, id: &str) -> bool {
    match value {
        Value::String(value) => value == id,
        Value::Array(values) => values.iter().any(|value| d_contains_id(value, id)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key == id || d_contains_id(value, id)),
        _ => false,
    }
}
fn d_response(
    prepared: &Prepared,
    operations: Vec<p::Operation>,
    continuation: p::Continuation,
) -> DResult<p::Decision> {
    let core = d_control(prepared)?;
    let preserve = core["selection_state"]["fragments_initialized"] == true
        && core["selection_state"]["assets_initialized"] == true;
    let mut response = decision(prepared, preserve);
    response.work = p::WorkDisposition::Ongoing {
        reason: "Synthetic fixture-authored judgment; no quality conclusion.".into(),
        evidence: Vec::new(),
    };
    response.operations = operations;
    response.continuation = continuation;
    for (scope, maintenance) in [
        (p::Scope::Fragments {}, &mut response.fragment_maintenance),
        (p::Scope::Assets {}, &mut response.asset_maintenance),
    ] {
        let ids: Vec<_> = response
            .operations
            .iter()
            .filter(|operation| operation.maintenance_scope() == Some(scope))
            .map(|operation| operation.id().to_owned())
            .collect();
        if !ids.is_empty() {
            *maintenance = p::Maintenance::Change {
                reason: "Explicit synthetic fixture-authored maintenance.".into(),
                operation_ids: ids,
            };
        }
    }
    Ok(response)
}
fn d_immediate() -> p::Continuation {
    p::Continuation::Immediate {
        purpose: "Synthetic next step within the existing finite allowance.".into(),
    }
}
fn d_stop() -> p::Continuation {
    p::Continuation::Stop {
        reason: "Synthetic local boundary; no completion inference.".into(),
    }
}
fn d_wait(id: &str) -> p::Continuation {
    p::Continuation::Wait {
        dependency_ids: vec![id.into()],
        deadline_unix: (NOW + 100) as u64,
        reason: "Synthetic wait for this exact registered local dependency.".into(),
    }
}
fn d_need(task: &DTask, roster: usize, id: &str) -> NeedInput {
    let mut input = need();
    input.id = id.into();
    input.request = task.request.clone();
    input.boundaries = "Synthetic local mechanism evaluation only; no conceptual quality assessment or external effect.".into();
    input.authorization = format!(
        "Explicit synthetic fixture authority for {id}; exact local bookkeeping mandate only."
    );
    input.assumptions = "Fixture-authored responses and testimony; no model call, elapsed-time maturation or benefit claim.".into();
    input.terms = "Retain a synthetic exact artifact, attributed judgment, shared decision, release and local receipt as distinct stages.".into();
    input.invitations = (0..roster)
        .map(|index| {
            let actor = ((b'a' + index as u8) as char).to_string();
            Invitation {
                id: format!("offer-{id}-{actor}"),
                to: actor.clone(),
                terms: w::CommitmentTerms {
                    mandate_revision: 1,
                    exact_terms: format!(
                        "Synthetic distinct responsibility accepted by participant {actor}."
                    ),
                    outcomes: if index == 0 {
                        BTreeSet::from([w::OutcomeId("result".into())])
                    } else {
                        BTreeSet::new()
                    },
                    review_roles: BTreeSet::new(),
                },
            }
        })
        .collect();
    input
}

struct DTrace {
    case: DCase,
    task: DTask,
    temp: Option<TempDir>,
    root: PathBuf,
    app: Option<Runtime>,
    checkpoints: Vec<Value>,
    checkpoint_bytes: usize,
    assertions: Vec<String>,
    persona_preparation_attempts: usize,
    mock_submission_attempts: usize,
    mock_slots: usize,
}
impl DTrace {
    fn new(case: DCase, task: DTask) -> DResult<Self> {
        let temp = tempfile::Builder::new()
            .prefix("personas-local-d-")
            .tempdir_in("/tmp")
            .map_err(d_error)?;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).map_err(d_error)?;
        d_private_path(temp.path(), true, false)?;
        let root = temp.path().join("state");
        Ok(Self {
            case,
            task,
            temp: Some(temp),
            root,
            app: None,
            checkpoints: Vec::new(),
            checkpoint_bytes: 0,
            assertions: Vec::new(),
            persona_preparation_attempts: 0,
            mock_submission_attempts: 0,
            mock_slots: 0,
        })
    }
    fn app(&mut self) -> DResult<&mut Runtime> {
        self.app
            .as_mut()
            .ok_or_else(|| "runtime unavailable".into())
    }
    fn state(&self) -> DResult<State> {
        d_state(&self.root)
    }
    fn check(&mut self, condition: bool, message: &str) -> DResult<()> {
        d_require(condition, message)?;
        d_require(self.assertions.len() < 512, "assertion diagnostic count")?;
        self.assertions.push(message.to_owned());
        Ok(())
    }
    fn record(&mut self, value: Value) -> DResult<()> {
        let bytes = serde_json::to_vec(&value).map_err(d_error)?;
        if self.checkpoints.len() >= 192
            || bytes.len() > 1024 * 1024
            || self.checkpoint_bytes.saturating_add(bytes.len()) > D_LOG_BYTES
        {
            if self.checkpoints.len() < 193 {
                self.checkpoints.push(
                    json!({"diagnostic_bound_refusal":true,"omitted_bytes":bytes.len(),
                    "omitted_sha256":s::sha256_bytes(&bytes),"transition":value["transition"]}),
                );
            }
            return Err(
                "diagnostic ceiling reached; exact oversized evidence was not claimed retained"
                    .into(),
            );
        }
        self.checkpoint_bytes += bytes.len();
        self.checkpoints.push(value);
        Ok(())
    }
    fn op<T>(
        &mut self,
        label: &str,
        input: Value,
        operation: impl FnOnce(&mut Self) -> DResult<T>,
        describe: impl FnOnce(&T) -> DResult<Value>,
    ) -> DResult<T> {
        let before = d_projection(&self.root)?;
        let outcome = operation(self);
        let after = d_projection(&self.root);
        let actual = match &outcome {
            Ok(value) => match describe(value) {
                Ok(value) => json!({"disposition":"returned","value":value}),
                Err(error) => json!({"disposition":"diagnostic_error","error":error}),
            },
            Err(error) => json!({"disposition":"refused","error":error}),
        };
        self.record(json!({"index":self.checkpoints.len(),"transition":label,"input":input,
            "actual":actual,"before":before,"after":after.as_ref().ok(),"after_error":after.as_ref().err(),
            "preadmission_counters":{"persona_preparation_attempts":self.persona_preparation_attempts,
                "mock_submission_attempts":self.mock_submission_attempts,"mock_slots":self.mock_slots}}))?;
        let after = after?;
        d_require(
            after["synthetic_persona_admissions"].as_u64().unwrap_or(0) <= 32,
            "persona admission ceiling after transition",
        )?;
        outcome
    }
    fn call<T: Serialize>(
        &mut self,
        label: &str,
        input: Value,
        operation: impl FnOnce(&mut Self) -> DResult<T>,
    ) -> DResult<T> {
        self.op(label, input, operation, |value| d_json(value))
    }
    fn authority(&self, work: &str) -> DResult<WorkAuthority> {
        let state = self.state()?;
        let current = state.board.work(&w::WorkId(work.into())).map_err(d_error)?;
        Ok(WorkAuthority {
            work: work.into(),
            version: current.version,
            authority: current.mandate().authority.clone(),
        })
    }
    fn setup(&mut self) -> DResult<()> {
        let actors: BTreeSet<String> = (0..self.case.roster_size)
            .map(|index| ((b'a' + index as u8) as char).to_string())
            .collect();
        let mut local_policy = policy();
        local_policy.readers = actors.clone();
        local_policy.audience = actors.clone();
        local_policy.audience.insert("human".into());
        let mut configuration = Config::local(f::PolicySnapshot {
            active_policies: vec![local_policy.clone()],
            revoked_policies: BTreeSet::new(),
        });
        configuration.local_recipients = set(&["human"]);
        self.call("create_local_runtime", d_json(&configuration)?, |trace| {
            trace.app =
                Some(Runtime::create(&trace.root, configuration, "operator").map_err(d_error)?);
            Ok(())
        })?;
        for actor in &actors {
            let mut persona = seed(actor);
            persona.label = format!("Synthetic participant {actor}");
            persona.text = format!("Synthetic operator-supplied participant {actor}; no profession or lived experience.");
            persona.audience = local_policy.audience.clone();
            persona.grants.communicate_with = local_policy.audience.clone();
            let (mut ocean, mut vad) = profiles();
            for value in ocean.values_mut().chain(vad.values_mut()) {
                *value = format!("Synthetic modeled description for participant {actor}.");
            }
            let voice = f::FileRevision { source: format!("voice-{actor}"), revision: String::new(),
                path: format!("voice-{actor}.txt"), scope: f::Scope::Fragments,
                bytes: format!("Synthetic participant {actor}: I distinguish exact observations from judgments.").into_bytes(),
                author: "operator:synthetic-fixture".into(), policy: local_policy.clone(), qualifications: Vec::new(), retired: false,
            }.seal().map_err(d_error)?;
            let import = PersonaImport {
                seed: persona,
                ocean,
                vad,
                voice,
            };
            self.call("import_distinct_persona", d_json(&import)?, |trace| {
                trace
                    .app()?
                    .import_persona("operator", import, NOW)
                    .map_err(d_error)
            })?;
        }
        let input = d_need(&self.task, self.case.roster_size, "work");
        self.call("intake_actual_task_request", d_json(&input)?, |trace| {
            trace.app()?.add_need("operator", input).map_err(d_error)
        })?;
        for actor in actors {
            self.accept(&actor, "work", actor == "a")?;
        }
        let state = self.state()?;
        let work = state
            .board
            .work(&w::WorkId("work".into()))
            .map_err(d_error)?;
        self.check(
            state.personas.len() == self.case.roster_size,
            "actual roster size",
        )?;
        self.check(
            state
                .personas
                .values()
                .all(|persona| persona.character.is_some() && persona.admission_serial == 1),
            "every roster member imported and independently admitted",
        )?;
        self.check(
            state
                .personas
                .keys()
                .all(|id| work.acceptance(&w::PersonaId(id.clone())).is_some()),
            "every roster member accepted its own responsibility",
        )?;
        let revisions: BTreeSet<_> = state
            .personas
            .values()
            .filter_map(|persona| {
                persona
                    .character
                    .as_ref()
                    .map(|character| &character.revision)
            })
            .collect();
        self.check(
            revisions.len() == self.case.roster_size,
            "distinct imported character identities",
        )
    }
    fn prepare(&mut self, id: &str, actor: &str, work: &str, scheduled: bool) -> DResult<Prepared> {
        let result = self.maybe_prepare(id, actor, work, scheduled)?;
        result.ok_or_else(|| {
            "requested fixture preparation was blocked; see scheduler checkpoint".into()
        })
    }
    fn maybe_prepare(
        &mut self,
        id: &str,
        actor: &str,
        work: &str,
        scheduled: bool,
    ) -> DResult<Option<Prepared>> {
        d_require(
            self.persona_preparation_attempts < 32,
            "preparation bound checked before admission",
        )?;
        self.persona_preparation_attempts += 1;
        let prepared = if scheduled {
            let request = SchedulerPrepareRequest {
                invocation_id: id.into(),
                max_considerations: self.case.roster_size,
            };
            self.call(
                "bounded_scheduler_preparation",
                d_json(&request)?,
                |trace| {
                    trace
                        .app()?
                        .scheduler_prepare(request, NOW)
                        .map_err(d_error)
                },
            )?
            .prepared
        } else {
            let request = PrepareRequest {
                id: id.into(),
                persona: actor.into(),
                work: work.into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            };
            Some(
                self.call("synthetic_preparation", d_json(&request)?, |trace| {
                    trace.app()?.prepare(request, NOW).map_err(d_error)
                })?,
            )
        };
        if let Some(prepared) = &prepared {
            self.check(
                prepared.request.persona == actor && prepared.request.work == work,
                "actual prepared actor and work",
            )?;
            self.check_prepared(prepared)?;
        }
        Ok(prepared)
    }
    fn check_prepared(&mut self, prepared: &Prepared) -> DResult<()> {
        self.check(
            prepared.request.profile == Profile::Synthetic
                && prepared.provider_preparation.is_none(),
            "preparation has no provider launch or provider preparation",
        )?;
        self.check(
            s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256,
            "actual prepared prompt hash",
        )?;
        let control = d_control(prepared)?;
        self.check(
            control["original_need"]["request"].as_str() == Some(self.task.request.as_str()),
            "actual NeedInput request supplied without relabeling",
        )?;
        let state = self.state()?;
        for entry in d_entries(prepared)? {
            let reference: f::Reference =
                serde_json::from_value(entry["reference"].clone()).map_err(d_error)?;
            let source = state
                .sources
                .get(&reference.source)
                .ok_or("supplied source absent")?;
            let revision = source
                .revisions
                .iter()
                .find(|revision| revision.record.revision == reference.revision)
                .ok_or("supplied revision absent")?;
            d_require(
                reference.representation == f::Representation::Utf8
                    && reference.start.is_none()
                    && reference.end.is_none(),
                "fixture uses complete exact UTF-8 bodies",
            )?;
            d_require(
                entry["text"].as_str().map(str::as_bytes) == Some(revision.record.bytes.as_slice()),
                "supplied body differs from exact persisted bytes",
            )?;
            d_require(
                prepared.sources.contains(&reference),
                "supplied body absent from prepared references",
            )?;
        }
        for observation in control["actual_observations"]
            .as_array()
            .ok_or("observation array absent")?
        {
            if let Some(id) = observation["id"].as_str() {
                d_require(
                    prepared
                        .supplied_observations
                        .iter()
                        .any(|supplied| supplied == id),
                    "observation supply receipt mismatch",
                )?;
                let retained = state
                    .observations
                    .get(id)
                    .ok_or("supplied observation not retained")?;
                if retained.kind != "host_accounting" {
                    d_require(
                        observation["payload"] == retained.payload,
                        "actual supplied observation body mismatch",
                    )?;
                }
            }
        }
        self.check(
            true,
            "all supplied bodies and observation payloads checked against persisted evidence",
        )
    }
    fn submit(
        &mut self,
        label: &str,
        prepared: &Prepared,
        response: &p::Decision,
        expected: AdoptionStatus,
    ) -> DResult<AdoptionReport> {
        let bytes = serde_json::to_vec(response).map_err(d_error)?;
        d_require(bytes.len() <= 32 * 1024, "synthetic response bound")?;
        let input = json!({"invocation_id":prepared.request.id,"authored_json":std::str::from_utf8(&bytes).map_err(d_error)?,
            "response_sha256":s::sha256_bytes(&bytes),"fixture_expected_status":expected});
        let report = self.call(label, input, |trace| {
            trace
                .app()?
                .submit_synthetic(&prepared.request.id, &bytes, NOW)
                .map_err(d_error)
        })?;
        self.check(
            report.status == expected,
            "actual response disposition matches fixture expectation",
        )?;
        self.check(
            report.synthetic && report.usage_is_unknown,
            "synthetic response retains unknown provider usage",
        )?;
        if expected == AdoptionStatus::Adopted {
            let expected_ids: Vec<_> = response
                .operations
                .iter()
                .map(|operation| operation.id().to_owned())
                .collect();
            self.check(
                report.applied == expected_ids && report.fenced.is_empty(),
                "exact authored operation IDs adopted",
            )?;
        } else if expected == AdoptionStatus::Held {
            self.check(
                report.applied.is_empty() && report.accepted_epoch.is_none(),
                "held response adopts no operation",
            )?;
        }
        Ok(report)
    }
    fn accept(&mut self, actor: &str, work: &str, continuing: bool) -> DResult<()> {
        let prepared = self.prepare(&format!("accept-{work}-{actor}"), actor, work, false)?;
        let control = d_control(&prepared)?;
        let offers = control["own_pending_offers"]
            .as_array()
            .ok_or("offers absent")?;
        d_require(offers.len() == 1, "one exact own offer")?;
        let operation = p::Operation::Responsibility {
            id: offers[0]["id"].as_str().ok_or("offer id")?.into(),
            work_id: work.into(),
            terms_revision: offers[0]["terms_revision"]
                .as_str()
                .ok_or("offer revision")?
                .into(),
            disposition: "accept".into(),
            reason: format!(
                "Synthetic participant {actor} explicitly accepts these exact supplied terms."
            ),
        };
        let response = d_response(
            &prepared,
            vec![operation],
            if continuing { d_immediate() } else { d_stop() },
        )?;
        self.submit(
            "distinct_responsibility_acceptance",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        Ok(())
    }
    fn resume(&mut self, work: &str) -> DResult<()> {
        self.call(
            "explicit_resume",
            json!({"operator":"operator","actor":"a","work":work}),
            |trace| trace.app()?.resume("operator", "a", work).map_err(d_error),
        )
    }
    fn reopen(&mut self) -> DResult<()> {
        self.call(
            "drop_and_reopen_persisted_runtime",
            json!({"recovery":"explicit local reopen; never redispatch"}),
            |trace| {
                drop(trace.app.take());
                trace.app = Some(Runtime::open(&trace.root).map_err(d_error)?);
                Ok(())
            },
        )
    }
    fn unchanged_replay(
        &mut self,
        label: &str,
        prepared: &Prepared,
        response: &p::Decision,
        expected: &AdoptionReport,
    ) -> DResult<()> {
        let before = d_cell(&self.root, "object", D_STATE_ID)?;
        let bytes = serde_json::to_vec(response).map_err(d_error)?;
        let report = self.call(
            label,
            json!({"invocation_id":prepared.request.id,"response_sha256":s::sha256_bytes(&bytes),
            "authored_json":std::str::from_utf8(&bytes).map_err(d_error)?}),
            |trace| {
                trace
                    .app()?
                    .adopt_response(&prepared.request.id, &bytes, NOW)
                    .map_err(d_error)
            },
        )?;
        self.check(
            d_json(&report)? == d_json(expected)?,
            "exact response replay returns original report",
        )?;
        self.check(
            before == d_cell(&self.root, "object", D_STATE_ID)?,
            "response replay does not rewrite accepted state",
        )
    }
    fn unfinished(&mut self, work: &str, reason: &str) -> DResult<()> {
        self.call(
            "explicit_unfinished_closeout",
            json!({"work":work,"reason":reason,"canceled":false}),
            |trace| {
                trace
                    .app()?
                    .closeout("operator", work, false, reason)
                    .map_err(d_error)
            },
        )
    }
    fn board_value(&self, work: &str) -> DResult<Value> {
        d_json(
            self.state()?
                .board
                .work(&w::WorkId(work.into()))
                .map_err(d_error)?,
        )
    }
    fn not_complete(&mut self) -> DResult<()> {
        self.check(
            !self
                .state()?
                .board
                .completion_supported(&w::WorkId("work".into())),
            "no completion before explicit supported closure",
        )
    }
    fn normal(&mut self) -> DResult<()> {
        self.not_complete()?;
        let prepared = self.prepare("normal-artifact", "a", "work", true)?;
        let mut response = d_response(
            &prepared,
            vec![
                write(
                    "save-artifact",
                    p::Scope::Assets {},
                    "artifact-source",
                    D_ARTIFACT,
                    None,
                ),
                effect(
                    "register-artifact",
                    WorkAction::PutArtifact {
                        artifact: "synthetic-artifact".into(),
                        base: None,
                        subject: proposed("artifact-source", "save-artifact"),
                    },
                    &["save-artifact"],
                ),
            ],
            d_immediate(),
        )?;
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Supply exact synthetic authored artifact.".into(),
            references: vec![proposed("artifact-source", "save-artifact")],
        };
        self.submit(
            "authored_file_and_artifact",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let state = self.state()?;
        let source = &state.sources["artifact-source"].revisions[0];
        self.check(source.record.bytes == D_ARTIFACT.as_bytes() && matches!(&source.origin,
            Some(RevisionOrigin::PersonaInvocation { persona, invocation_id, operation_id })
                if persona == "a" && invocation_id == "normal-artifact" && operation_id == "save-artifact"), "artifact has exact accepted primary authorship")?;
        let work = state
            .board
            .work(&w::WorkId("work".into()))
            .map_err(d_error)?;
        let native =
            &work.artifacts[&w::ArtifactId("synthetic-artifact".into())].revisions[&1].reference;
        let key = ArtifactKey {
            id: native.id.0.clone(),
            revision: native.revision,
            digest: hex::encode(native.digest),
        };
        self.call("synthetic_operator_testimony", json!({"artifact":key,"source":"human_judgment API; synthetic fixture testimony"}), |trace| {
            trace.app()?.record_human_evidence("operator", "work", "synthetic-testimony",
                "Synthetic fixture-authored operator testimony concerning exact bytes only; no conceptual quality claim.",
                vec![key.clone()], set(&["result"])).map_err(d_error)
        })?;
        self.not_complete()?;
        self.check(
            !d_contains_id(&self.board_value("work")?, "d-individual")
                && !d_contains_id(&self.board_value("work")?, "d-shared"),
            "artifact and testimony are not individual or shared judgments",
        )?;
        let artifacts = BTreeMap::from([("result".into(), key)]);
        let prepared = self.prepare("normal-individual", "a", "work", true)?;
        let mut response = d_response(&prepared, vec![effect("judge", WorkAction::Judge {
            id: "d-individual".into(), artifacts: artifacts.clone(), sufficient: true,
            claims: BTreeMap::from([("result".into(), ClaimInput { met: true,
                explanation: "Synthetic attributed claim concerning the exact fixture artifact.".into(), evidence: set(&["synthetic-testimony"]) })]),
            explanation: "Synthetic individual sufficiency judgment under the fixture mandate; no task quality verdict.".into(),
        }, &[])], d_immediate())?;
        response.work = p::WorkDisposition::IndividualSufficient {
            reason: "Synthetic attributed individual judgment only.".into(),
            evidence: vec!["synthetic-testimony".into()],
        };
        self.submit(
            "individual_sufficiency",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            d_contains_id(&self.board_value("work")?, "d-individual")
                && !d_contains_id(&self.board_value("work")?, "d-shared"),
            "individual sufficiency persists separately from shared decision",
        )?;
        self.not_complete()?;
        let prepared = self.prepare("normal-shared", "a", "work", true)?;
        let mut response = d_response(
            &prepared,
            vec![effect(
                "decide",
                WorkAction::DecideShared {
                    id: "d-shared".into(),
                    artifacts,
                },
                &[],
            )],
            d_immediate(),
        )?;
        response.work = p::WorkDisposition::SharedSufficient {
            reason: "Synthetic shared disposition under the designated decision rule.".into(),
            evidence: Vec::new(),
        };
        self.submit(
            "shared_decision",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            d_contains_id(&self.board_value("work")?, "d-shared"),
            "actual shared record retained",
        )?;
        self.not_complete()?;
        let prepared = self.prepare("normal-release", "a", "work", true)?;
        let response = d_response(
            &prepared,
            vec![effect(
                "release",
                WorkAction::Release {
                    id: "d-release".into(),
                    shared: "d-shared".into(),
                },
                &[],
            )],
            d_immediate(),
        )?;
        self.submit(
            "exact_release",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            self.state()?
                .board
                .release_current(&w::WorkId("work".into()), &w::RecordId("d-release".into())),
            "release is supported by actual shared state",
        )?;
        self.check(
            self.state()?.local_deliveries.is_empty(),
            "release does not manufacture delivery",
        )?;
        self.not_complete()?;
        let prepared = self.prepare("normal-delivery", "a", "work", true)?;
        let response = d_response(
            &prepared,
            vec![effect(
                "deliver",
                WorkAction::DeliverRelease {
                    release: "d-release".into(),
                    recipients: set(&["human"]),
                },
                &[],
            )],
            d_immediate(),
        )?;
        self.submit(
            "exact_atomic_local_delivery",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let state = self.state()?;
        self.check(
            state.local_deliveries.len() == 1,
            "one actual local delivery",
        )?;
        let delivery = state
            .local_deliveries
            .values()
            .next()
            .ok_or("delivery absent")?;
        let artifact = &delivery["artifacts"][0];
        let bytes: Vec<u8> = serde_json::from_value(artifact["bytes"].clone()).map_err(d_error)?;
        self.check(
            bytes == D_ARTIFACT.as_bytes() && delivery["external_export"] == false,
            "local receipt contains exact authored bytes and no export",
        )?;
        let bundle = artifact["qualified_bundle_json"]
            .as_str()
            .ok_or("delivery bundle absent")?;
        self.check(
            artifact["qualified_bundle_sha256"] == s::sha256_bytes(bundle.as_bytes()),
            "local delivered bundle hash",
        )?;
        self.not_complete()?;
        let prepared = self.prepare("normal-close", "a", "work", true)?;
        let mut response = d_response(
            &prepared,
            vec![effect(
                "close",
                WorkAction::CloseGoal {
                    release: "d-release".into(),
                },
                &[],
            )],
            d_stop(),
        )?;
        response.work = p::WorkDisposition::Closed {
            reason: "Synthetic closure proposal under the exact local fixture mandate.".into(),
            evidence: Vec::new(),
        };
        self.submit(
            "closure_under_mandate",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            self.state()?
                .board
                .completion_supported(&w::WorkId("work".into())),
            "runtime supports the synthetic mandate closure",
        )
    }
}

#[derive(Clone)]
struct DMock {
    request: m::MockRequest,
    basis: s::ObjectRef,
    admitted: m::AdmissionReceipt,
    registration: PendingMockRegistration,
}
fn d_mock_bounds() -> m::Bounds {
    m::Bounds {
        response_bytes: 32 * 1024,
        input_bytes: 4096,
        result_bytes: 4096,
        evidence_bytes: 16 * 1024,
    }
}
fn d_access(epoch: u64, basis: &s::ObjectRef, dispatch: bool, submit: bool) -> m::HostAdmission {
    m::HostAdmission::from_trusted_host(
        epoch,
        "a".into(),
        "work".into(),
        vec![basis.clone()],
        m::Rights {
            submit_fixture: submit,
            dispatch,
            cancel: false,
            observe: true,
        },
    )
}
impl DTrace {
    fn admit_mock(&mut self) -> DResult<(s::Store, DMock)> {
        d_require(
            self.mock_slots < 2,
            "mock effect bound checked before admission",
        )?;
        self.mock_slots += 1;
        let request = m::MockRequest {
            invocation_id: "mock-fixture-invocation".into(),
            id: "mock-intent".into(),
            actor: "a".into(),
            work: "work".into(),
            interface_version: m::INTERFACE_VERSION.into(),
            connection: "combined-fixture".into(),
            request_text: "Observe an explicit bounded synthetic local token.".into(),
            inputs: "Synthetic opaque token input.".into(),
        };
        let policy = self.state()?.config.policies.active_policies[0].clone();
        let basis_request = MockEvidenceBasisRequest { binding: self.authority("work")?, mock_request: request.clone(), policy,
            lineage: Vec::new(), qualifications: Vec::new(),
            attestation: "Synthetic host fixture attests this exact abstract input/result policy; no independent authentication or task-success claim.".into() };
        let basis = self.call("bind_actual_mock_basis", d_json(&basis_request)?, |trace| {
            trace
                .app()?
                .bind_mock_evidence("operator", basis_request, NOW)
                .map_err(d_error)
        })?;
        let root = self.root.clone();
        let mut store = self.op(
            "open_mock_store_for_inspection",
            json!({"owner_acquired":false}),
            |_| {
                s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits())
                    .map_err(d_error)
            },
            |store| d_json(store.ledger().map_err(d_error)?),
        )?;
        let admitted;
        {
            let mut mock = self.op(
                "attach_sealed_mock_owner",
                json!({"connection":"combined-fixture","bounds":d_mock_bounds()}),
                |_| {
                    m::MockHarness::attach(&mut store, "combined-fixture".into(), d_mock_bounds())
                        .map_err(d_error)
                },
                |mock| d_json(mock.ledger().map_err(d_error)?),
            )?;
            let spec = m::IntentSpec::new(request.clone(), vec![basis.clone()], 4096);
            let epoch = mock.epoch().map_err(d_error)?;
            let response = json!({"version":1,"invocation_id":request.invocation_id,"epoch":epoch,
                "work":{"kind":"ongoing","reason":"Synthetic fixture observation remains pending.","evidence":[]},
                "fragment_maintenance":{"kind":"no_change","reason":"Synthetic fixture leaves persona state unchanged."},
                "asset_maintenance":{"kind":"no_change","reason":"Synthetic fixture leaves assets unchanged."},
                "next_fragments":{"kind":"preserve","reason":"Preserve fixture context."},
                "next_assets":{"kind":"preserve","reason":"Preserve fixture context."},
                "continuation":{"kind":"stop","reason":"Explicit synthetic boundary."},
                "reply":{"kind":"none","reason":"No communication."},"operations":[spec.operation()]});
            let bytes = serde_json::to_vec(&response).map_err(d_error)?;
            d_require(
                self.mock_submission_attempts < 2,
                "mock Store attempt envelope checked before submission",
            )?;
            self.mock_submission_attempts += 1;
            let access = d_access(epoch, &basis, false, true);
            admitted = self.call("mock_store_invocation_and_intent_admission", json!({"request":request,"basis":basis,
                "authored_json":std::str::from_utf8(&bytes).map_err(d_error)?,"store_attempt_envelope":{"invocation":1,"intent":1}}),
                |_| mock.submit_fixture(&access, &spec, &bytes).map_err(d_error))?;
        }
        let registration_request = RegisterPendingMockRequest {
            binding: self.authority("work")?,
            id: "mock-dependency".into(),
            basis: basis.clone(),
            admitted_request: admitted.request.clone(),
            participants: set(&["a"]),
            accountability: w::Accountability::Owner(w::PersonaId("a".into())),
        };
        let registration = self.call(
            "register_pending_actual_mock_request",
            d_json(&registration_request)?,
            |trace| {
                trace
                    .app()?
                    .register_pending_mock_dependency("operator", registration_request, NOW)
                    .map_err(d_error)
            },
        )?;
        self.check(
            self.state()?.local_evidence.ingestions.is_empty(),
            "pending registration supplies no fabricated result",
        )?;
        Ok((
            store,
            DMock {
                request,
                basis,
                admitted,
                registration,
            },
        ))
    }
    fn wait_for_mock(&mut self, id: &str, dependency: &str) -> DResult<Prepared> {
        let prepared = self.prepare(id, "a", "work", true)?;
        let response = d_response(&prepared, Vec::new(), d_wait(dependency))?;
        self.submit(
            "accept_exact_wait",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        Ok(prepared)
    }
    fn dispatch_epoch(&mut self, context: &DMock) -> DResult<u64> {
        let binding = self.authority("work")?;
        self.call(
            "current_mock_dispatch_authority_check",
            json!({"binding":binding,"basis":context.basis}),
            |trace| {
                trace
                    .app()?
                    .check_mock_evidence_dispatch("operator", binding, context.basis.clone(), NOW)
                    .map_err(d_error)
            },
        )
    }
    fn terminal(
        &mut self,
        store: &mut s::Store,
        context: &DMock,
        late: bool,
    ) -> DResult<m::MockTerminal> {
        let epoch = if late {
            None
        } else {
            Some(self.dispatch_epoch(context)?)
        };
        let mut mock = self.op(
            "attach_mock_for_observation",
            json!({"late":late}),
            |_| {
                m::MockHarness::attach(store, "combined-fixture".into(), d_mock_bounds())
                    .map_err(d_error)
            },
            |mock| d_json(mock.ledger().map_err(d_error)?),
        )?;
        if let Some(epoch) = epoch {
            self.check(
                mock.epoch().map_err(d_error)? == epoch,
                "dispatch uses exact returned bridge epoch",
            )?;
            let access = d_access(epoch, &context.basis, true, false);
            let view = self.call(
                "one_actual_mock_dispatch",
                json!({"intent":context.request.id,"epoch":epoch}),
                |_| mock.dispatch(&access, &context.request.id).map_err(d_error),
            )?;
            self.check(
                view.state == m::MockState::Pending && view.observed_mock_executions == Some(1),
                "actual single synthetic mock entry",
            )?;
        } else {
            let access = d_access(mock.epoch().map_err(d_error)?, &context.basis, false, false);
            let view = self.call(
                "inspect_original_uncertain_intent",
                json!({"intent":context.request.id}),
                |_| {
                    mock.inspect_closeout(&access, &context.request.id)
                        .map_err(d_error)
                },
            )?;
            self.check(
                view.state == m::MockState::EffectUnknown
                    && view.observed_mock_executions == Some(1)
                    && view.terminal.is_none(),
                "late result retains original uncertain execution",
            )?;
        }
        let access = d_access(mock.epoch().map_err(d_error)?, &context.basis, false, false);
        let view = self.call("actual_synthetic_terminal_observation", json!({"intent":context.request.id,"outcome":"completed","exact_result":D_RESULT,"late":late}),
            |_| mock.simulate_terminal(&access, &context.request.id, m::MockOutcome::Completed, D_RESULT).map_err(d_error))?;
        self.check(
            view.observed_mock_executions == Some(1) && view.state == m::MockState::Completed,
            "terminal observation never adds execution",
        )?;
        let terminal = view.terminal.ok_or("actual applied terminal missing")?;
        self.check(
            terminal.result() == D_RESULT
                && terminal.receipt().evidence.as_ref() == Some(terminal.evidence()),
            "exact terminal result and receipt binding",
        )?;
        Ok(terminal)
    }
    fn interrupted(&mut self, store: &mut s::Store, context: &DMock) -> DResult<()> {
        let epoch = self.dispatch_epoch(context)?;
        {
            let mut mock = self.op(
                "attach_before_interruption",
                json!({"epoch":epoch}),
                |_| {
                    m::MockHarness::attach(store, "combined-fixture".into(), d_mock_bounds())
                        .map_err(d_error)
                },
                |mock| d_json(mock.ledger().map_err(d_error)?),
            )?;
            self.check(
                mock.epoch().map_err(d_error)? == epoch,
                "interrupted dispatch keeps current authority fence",
            )?;
            let access = d_access(epoch, &context.basis, true, false);
            let view = self.call(
                "dispatch_then_lose_owner",
                json!({"intent":context.request.id}),
                |_| mock.dispatch(&access, &context.request.id).map_err(d_error),
            )?;
            self.check(
                view.observed_mock_executions == Some(1),
                "interrupted mock entered exactly once",
            )?;
        }
        self.reopen()?;
        let intent: s::IntentRecord =
            serde_json::from_value(d_cell(&self.root, "intent", &context.request.id)?)
                .map_err(d_error)?;
        self.check(
            intent.state == s::IntentState::Uncertain,
            "reopened dispatched intent becomes uncertain",
        )?;
        let hold = self.call(
            "inspect_uncertain_intent_hold",
            json!({"intent":context.request.id}),
            |_| store.reservation(&intent.reservation).map_err(d_error),
        )?;
        self.check(
            hold.state == s::ReservationState::Uncertain && hold.measured.is_none(),
            "interruption preserves unknown usage hold",
        )?;
        Ok(())
    }
    fn ingestion_request(
        &self,
        context: &DMock,
        evidence: s::ObjectRef,
        id: &str,
    ) -> DResult<IngestMockEvidenceRequest> {
        Ok(IngestMockEvidenceRequest {
            binding: self.authority("work")?,
            id: id.into(),
            actor: "a".into(),
            intent_id: context.request.id.clone(),
            basis: context.basis.clone(),
            admitted_request: context.admitted.request.clone(),
            evidence,
            source: format!("source-{id}"),
            path: format!("{id}.json"),
            max_evidence_bytes: 16 * 1024,
        })
    }
    fn events(&mut self, before_wait: bool, interrupted: bool) -> DResult<()> {
        let (mut store, context) = self.admit_mock()?;
        if !before_wait {
            self.wait_for_mock("wait-before-terminal", &context.registration.id)?;
        }
        if interrupted {
            self.interrupted(&mut store, &context)?;
            let absent = s::ObjectRef {
                id: format!(
                    "mock-terminal:{}",
                    s::sha256_bytes(context.request.id.as_bytes())
                ),
                revision: 1,
                sha256: "0".repeat(64),
            };
            let request = self.ingestion_request(&context, absent, "absent-terminal")?;
            let result = self.call(
                "refuse_unapplied_or_absent_terminal",
                d_json(&request)?,
                |trace| {
                    trace
                        .app()?
                        .ingest_retained_mock_terminal("operator", request, NOW)
                        .map_err(d_error)
                },
            );
            self.check(
                result.is_err() && self.state()?.local_evidence.ingestions.is_empty(),
                "uncertainty cannot manufacture ingestible terminal evidence",
            )?;
        }
        let terminal = self.terminal(&mut store, &context, interrupted)?;
        let actual = self.call(
            "inspect_exact_terminal_object",
            d_json(terminal.evidence())?,
            |_| store.read_exact(terminal.evidence()).map_err(d_error),
        )?;
        let request =
            self.ingestion_request(&context, terminal.evidence().clone(), "actual-terminal")?;
        let receipt = if interrupted {
            self.reopen()?;
            self.call(
                "ingest_verified_retained_terminal",
                d_json(&request)?,
                |trace| {
                    trace
                        .app()?
                        .ingest_retained_mock_terminal("operator", request.clone(), NOW)
                        .map_err(d_error)
                },
            )?
        } else {
            self.call(
                "ingest_sealed_actual_terminal",
                json!({"request":request,"terminal":terminal}),
                |trace| {
                    trace
                        .app()?
                        .ingest_mock_terminal("operator", request.clone(), terminal.clone(), NOW)
                        .map_err(d_error)
                },
            )?
        };
        let state = self.state()?;
        let retained = &state.sources[&receipt.file.source].revisions[0];
        let copied: s::StoredObject =
            serde_json::from_slice(&retained.record.bytes).map_err(d_error)?;
        self.check(
            copied == actual && receipt.bytes_sha256 == s::sha256_bytes(&retained.record.bytes),
            "bridge retains complete actual terminal bytes losslessly",
        )?;
        self.check(
            retained.record.computed_revision().map_err(d_error)? == receipt.file.revision,
            "ingested exact file revision",
        )?;
        self.check(
            state
                .local_evidence
                .pending
                .values()
                .all(|pending| pending.event.is_none()),
            "ingestion alone delivers no event",
        )?;
        if !before_wait {
            self.check(
                state
                    .board
                    .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
                    .ok_or("fixture participant absent")?
                    .wake
                    .is_none(),
                "ingestion alone creates no wait wake",
            )?;
        }
        let event = DeliverPendingMockEventRequest {
            binding: self.authority("work")?,
            id: "actual-event".into(),
            dependency_id: context.registration.id.clone(),
            binding_revision: context.registration.revision.clone(),
            ingestion_id: receipt.id.clone(),
            max_delivery_bytes: 32 * 1024,
        };
        let delivered = self.call(
            "separately_authorized_event_delivery",
            d_json(&event)?,
            |trace| {
                trace
                    .app()?
                    .deliver_pending_mock_event("operator", event.clone(), NOW)
                    .map_err(d_error)
            },
        )?;
        self.check(
            !delivered.duplicate
                && delivered.delivered_to == set(&["a"])
                && !delivered.independent_authentication
                && !delivered.external_subscription,
            "actual local event attribution and audience",
        )?;
        let prepared = self.prepare(
            if before_wait {
                "wait-after-event"
            } else {
                "supply-after-event"
            },
            "a",
            "work",
            true,
        )?;
        let entry = d_entries(&prepared)?
            .into_iter()
            .find(|entry| entry["reference"]["source"] == receipt.file.source)
            .ok_or("actual event body was not supplied")?;
        let supplied: s::StoredObject =
            serde_json::from_str(entry["text"].as_str().ok_or("event body text absent")?)
                .map_err(d_error)?;
        self.check(
            supplied == actual,
            "fresh preparation actually supplies exact terminal body",
        )?;
        let response = d_response(&prepared, Vec::new(), d_wait(&context.registration.id))?;
        self.submit(
            "acknowledge_supplied_body_with_wait",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let state = self.state()?;
        let participant = state
            .board
            .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
            .ok_or("fixture participant absent")?;
        self.check(
            participant
                .observed_events
                .contains(&context.registration.key),
            "actual supplied event acknowledged by accepted response",
        )?;
        self.check(
            state.personas["a"].focus["work"]
                .assets
                .as_ref()
                .is_some_and(Vec::is_empty),
            "event supply does not silently select an asset",
        )?;
        self.check(
            state.local_evidence.pending.values().all(|pending| {
                d_json(&pending.registration).ok() == d_json(&context.registration).ok()
            }),
            "immutable pending registration survives result arrival",
        )?;
        self.reopen()?;
        let head = d_cell(&self.root, "object", D_STATE_ID)?;
        let replay = self.call(
            "exact_ingestion_receipt_replay",
            d_json(&request)?,
            |trace| {
                trace
                    .app()?
                    .ingest_retained_mock_terminal("operator", request, NOW + 1)
                    .map_err(d_error)
            },
        )?;
        self.check(
            replay == receipt,
            "ingestion replay returns original exact receipt",
        )?;
        let replay = self.call("exact_event_receipt_replay", d_json(&event)?, |trace| {
            trace
                .app()?
                .deliver_pending_mock_event("operator", event, NOW + 1)
                .map_err(d_error)
        })?;
        self.check(
            replay.duplicate,
            "event replay is an actual duplicate disposition",
        )?;
        self.check(
            head == d_cell(&self.root, "object", D_STATE_ID)?,
            "bridge replay changes no state, wake or work usage",
        )?;
        let duplicate_wake = self.maybe_prepare("no-duplicate-wake", "a", "work", true)?;
        self.check(
            duplicate_wake.is_none(),
            "acknowledged event has no duplicate scheduler wake",
        )?;
        let intent: s::IntentRecord =
            serde_json::from_value(d_cell(&self.root, "intent", &context.request.id)?)
                .map_err(d_error)?;
        self.check(
            intent
                .history
                .iter()
                .filter(|receipt| receipt.status == s::ReceiptStatus::Dispatched)
                .count()
                == 1,
            "event and late recovery never redispatch intent",
        )?;
        if interrupted {
            self.check(
                intent.reconciliation.is_some(),
                "late terminal uses original intent reconciliation",
            )?;
        }
        self.not_complete()
    }
    fn protected_pause(&mut self, reason: &str) -> DResult<()> {
        self.call(
            "protected_pause",
            json!({"work":"work","reason":reason}),
            |trace| {
                let mut closeout = Runtime::open_for_closeout(&trace.root).map_err(d_error)?;
                closeout.pause("operator", "work", reason).map_err(d_error)
            },
        )
    }
    fn freeze(&mut self) -> DResult<()> {
        // This ordinary continuation is demonstrated before the irreversible
        // fixture freeze. No account is replenished or frozen flag cleared.
        self.protected_pause("Synthetic ordinary-capacity pause before freeze.")?;
        let slots = d_rows(&self.root, "runtime_control", 16)?.len();
        let usage = self.state()?.work_meta["work"].attempts_left;
        self.resume("work")?;
        self.check(
            d_rows(&self.root, "runtime_control", 16)?.len() == slots + 1,
            "ordinary resume prepays one finite successor pause slot",
        )?;
        self.check(
            self.state()?.work_meta["work"].attempts_left == usage,
            "resume grants no work attempts",
        )?;
        let prepared = self.prepare("ordinary-resumed-continuation", "a", "work", true)?;
        let response = d_response(&prepared, Vec::new(), d_immediate())?;
        self.submit(
            "ordinary_admitted_continuation",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let (mut store, context) = self.admit_mock()?;
        self.wait_for_mock("freeze-pending-wait", &context.registration.id)?;
        self.interrupted(&mut store, &context)?;
        let original_intent = d_cell(&self.root, "intent", &context.request.id)?;
        let trip = self.call(
            "synthetic_overrun_reservation",
            json!({"output_bytes":1,"synthetic_fault":true}),
            |_| {
                store
                    .reserve(
                        "combined-freeze-trip",
                        s::Resources {
                            output_bytes: 1,
                            ..s::Resources::default()
                        },
                        s::BudgetClass::Ordinary,
                    )
                    .map_err(d_error)
            },
        )?;
        let ledger = store.ledger().map_err(d_error)?;
        let ordinary = ledger
            .ordinary
            .exposure()
            .map_err(d_error)?
            .navigation_bytes;
        let closeout = ledger
            .closeout
            .exposure()
            .map_err(d_error)?
            .navigation_bytes;
        let available = (ledger.limits.total.navigation_bytes - ordinary - closeout).min(
            ledger.limits.total.navigation_bytes
                - ledger.limits.protected_closeout.navigation_bytes
                - ordinary,
        );
        self.check(
            available > 0,
            "ordinary capacity exists before deliberate exhaustion",
        )?;
        self.call(
            "synthetic_logical_ordinary_exhaustion",
            json!({"navigation_bytes":available,"synthetic_fault":true,
            "authority":"none; deliberate resource-pressure fixture"}),
            |_| {
                store
                    .reserve(
                        "combined-navigation-pressure",
                        s::Resources {
                            navigation_bytes: available,
                            ..s::Resources::default()
                        },
                        s::BudgetClass::Ordinary,
                    )
                    .map_err(d_error)
            },
        )?;
        let refused = self.call(
            "ordinary_pause_at_exhaustion",
            json!({"expected":"ordinary quota refusal"}),
            |trace| {
                trace
                    .app()?
                    .pause("operator", "work", "Synthetic exhausted ordinary pause.")
                    .map_err(d_error)
            },
        );
        self.check(
            refused.is_err(),
            "logically exhausted ordinary pause refuses",
        )?;
        let overrun = self.call(
            "synthetic_overrun_trips_real_freeze",
            json!({"reservation":trip.id,"measured_output_bytes":2,"synthetic_fault":true}),
            |_| {
                store
                    .settle(
                        &trip.id,
                        s::Usage::Measured(s::Resources {
                            output_bytes: 2,
                            ..s::Resources::default()
                        }),
                    )
                    .map_err(d_error)
            },
        );
        self.check(
            overrun.is_err() && store.ledger().map_err(d_error)?.frozen,
            "actual Store freeze persists after synthetic overrun",
        )?;
        self.protected_pause(
            "Synthetic protected pause under actual ordinary exhaustion and freeze.",
        )?;
        let state = self.state()?;
        self.check(
            matches!(
                state
                    .board
                    .work(&w::WorkId("work".into()))
                    .map_err(d_error)?
                    .phase,
                w::Phase::Paused { .. }
            ),
            "pre-funded pause succeeds under freeze",
        )?;
        let head = d_cell(&self.root, "object", D_STATE_ID)?;
        let holds = d_rows(&self.root, "reservation", 128)?;
        let result = self.resume("work");
        self.check(
            result
                .as_ref()
                .err()
                .is_some_and(|error| error == &s::StoreError::Frozen.to_string()),
            "unsupported frozen resume returns exact Frozen blocker",
        )?;
        self.check(
            head == d_cell(&self.root, "object", D_STATE_ID)?
                && holds == d_rows(&self.root, "reservation", 128)?
                && original_intent == d_cell(&self.root, "intent", &context.request.id)?,
            "frozen resume preserves accepted state, all holds and original intent",
        )?;
        self.check(
            store.ledger().map_err(d_error)?.frozen,
            "freeze was never cleared or replenished",
        )?;
        self.not_complete()
    }
}

impl DTrace {
    fn warning_operation(
        target: &p::ExactRef,
        basis: &p::ExactRef,
        qualification: &p::ExactRef,
    ) -> p::Operation {
        p::Operation::WarningAdopt { id: "adopt-warning".into(), warning_id: "combined-warning".into(), scope: p::Scope::Assets {},
            targets: vec![target.clone()], uses: p::WarningUses::AllRepresentations {},
            meaning: "Synthetic primary-authored applicability judgment; no semantic correctness verdict.".into(),
            basis: vec![basis.clone()], disposition: p::WarningDisposition::Qualify { required: vec![qualification.clone()] },
            policy_id: "local-policy".into(), depends_on: Vec::new() }
    }
    fn warnings(&mut self) -> DResult<()> {
        let prepared = self.prepare("warning-author-sources", "a", "work", true)?;
        let mut response = d_response(
            &prepared,
            vec![
                write(
                    "save-target",
                    p::Scope::Assets {},
                    "warning-target",
                    D_TARGET,
                    None,
                ),
                write(
                    "save-basis",
                    p::Scope::Assets {},
                    "warning-basis",
                    D_BASIS,
                    None,
                ),
                write(
                    "save-qualification",
                    p::Scope::Assets {},
                    "warning-qualification",
                    D_QUALIFICATION,
                    None,
                ),
            ],
            d_immediate(),
        )?;
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Supply accepted synthetic target, basis and accompaniment on a fresh call."
                .into(),
            references: vec![
                proposed("warning-target", "save-target"),
                proposed("warning-basis", "save-basis"),
                proposed("warning-qualification", "save-qualification"),
            ],
        };
        self.submit(
            "primary_authorship_of_warning_material",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let state = self.state()?;
        let target_record = state.sources["warning-target"].revisions[0].record.clone();
        let target = reference(&target_record);
        let basis = reference(&state.sources["warning-basis"].revisions[0].record);
        let qualification = reference(&state.sources["warning-qualification"].revisions[0].record);
        self.check(["warning-target", "warning-basis", "warning-qualification"].iter().all(|id| matches!(
            &state.sources[*id].revisions[0].origin, Some(RevisionOrigin::PersonaInvocation { persona, .. }) if persona == "a")), "warning target, basis and accompaniment have accepted primary authorship")?;
        let prepared = self.prepare("warning-failed-patch", "a", "work", true)?;
        self.check(
            d_has_body(&prepared, &basis, D_BASIS)?,
            "failed patch invocation actually supplied exact basis",
        )?;
        let mut warning = Self::warning_operation(&target, &basis, &qualification);
        if let p::Operation::WarningAdopt { depends_on, .. } = &mut warning {
            depends_on.push("bad-patch".into());
        }
        let response = d_response(
            &prepared,
            vec![
                write(
                    "bad-patch",
                    p::Scope::Assets {},
                    "warning-target",
                    "Synthetic unaccepted replacement.",
                    Some("0".repeat(64)),
                ),
                warning,
            ],
            d_immediate(),
        )?;
        let held = self.submit(
            "held_failed_patch_and_dependent_warning",
            &prepared,
            &response,
            AdoptionStatus::Held,
        )?;
        self.check(
            held.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("stale file base revision")),
            "failed patch has actual stale-base refusal",
        )?;
        self.check(
            self.state()?.semantic_warnings.is_empty()
                && self.state()?.sources["warning-target"].revisions[0].record == target_record,
            "failed whole response creates no warning or content edit",
        )?;
        self.resume("work")?;
        let adoption_prepared = self.prepare("warning-separate-adoption", "a", "work", true)?;
        self.check(
            d_has_body(&adoption_prepared, &basis, D_BASIS)?,
            "separately funded warning receives exact supplied basis",
        )?;
        let adoption_response = d_response(
            &adoption_prepared,
            vec![Self::warning_operation(&target, &basis, &qualification)],
            d_immediate(),
        )?;
        let adoption_report = self.submit(
            "separate_explicit_warning_adoption",
            &adoption_prepared,
            &adoption_response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            self.state()?.semantic_warnings["combined-warning"].active(),
            "actual warning active after independent acceptance",
        )?;
        let prepared = self.prepare("warning-correction-content", "a", "work", true)?;
        let response = d_response(
            &prepared,
            vec![write(
                "correct-content",
                p::Scope::Assets {},
                "warning-target",
                "Synthetic later token zeta; old exact bytes remain immutable.",
                Some(target.revision.clone()),
            )],
            d_immediate(),
        )?;
        self.submit(
            "accepted_later_content_revision",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            self.state()?.semantic_warnings["combined-warning"].active()
                && self.state()?.semantic_warnings["combined-warning"]
                    .history
                    .is_empty(),
            "content change does not resolve applicability warning",
        )?;
        self.unfinished("work", "Synthetic correction-work closeout remains unfinished; no warning resolution is inferred.")?;
        self.reopen()?;
        self.check(
            self.state()?.semantic_warnings["combined-warning"].active(),
            "warning survives correction closeout and restart",
        )?;
        let ended_resume = self.resume("work");
        self.check(
            ended_resume.is_err(),
            "ended correction work cannot be silently reopened",
        )?;
        let mut input = d_need(&self.task, self.case.roster_size, "reuse");
        input.invitations.retain(|invitation| invitation.to == "a");
        self.call(
            "new_explicit_authority_for_reuse",
            d_json(&input)?,
            |trace| trace.app()?.add_need("operator", input).map_err(d_error),
        )?;
        self.accept("a", "reuse", true)?;
        let prepared = self.prepare("reuse-discover-old", "a", "reuse", false)?;
        let response = d_response(
            &prepared,
            vec![p::Operation::Search {
                id: "discover-old".into(),
                scope: p::Scope::Assets {},
                query: D_TARGET.into(),
                regex: false,
                limit: 8,
            }],
            d_immediate(),
        )?;
        self.submit(
            "rediscover_historical_exact_source",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let prepared = self.prepare("reuse-observe-discovery", "a", "reuse", false)?;
        let control = d_control(&prepared)?;
        let mut discovered = None;
        for observation in control["actual_observations"]
            .as_array()
            .ok_or("discovery observations absent")?
        {
            if observation["kind"] == "discovery" {
                if let Some(page) = observation["payload"]["page"].as_str() {
                    let page: Value = serde_json::from_str(page).map_err(d_error)?;
                    for hit in page["hits"].as_array().ok_or("discovery hits absent")? {
                        if hit["reference"]["source"] == target.source
                            && hit["reference"]["revision"] == target.revision
                        {
                            discovered = Some(p::ExactRef {
                                source: hit["reference"]["source"]
                                    .as_str()
                                    .ok_or("hit source")?
                                    .into(),
                                revision: hit["reference"]["revision"]
                                    .as_str()
                                    .ok_or("hit revision")?
                                    .into(),
                                representation: "utf8".into(),
                                start: None,
                                end: None,
                                lifetime: Some("work".into()),
                            });
                        }
                    }
                }
            }
        }
        let discovered =
            discovered.ok_or("old exact revision not present in actually supplied discovery")?;
        self.check(
            discovered == target,
            "fresh supplied search rediscovers the exact old revision",
        )?;
        let mut response = d_response(
            &prepared,
            vec![
                p::Operation::Read {
                    id: "read-old".into(),
                    reference: discovered.clone(),
                },
                p::Operation::Read {
                    id: "read-basis".into(),
                    reference: basis.clone(),
                },
            ],
            d_immediate(),
        )?;
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Explicitly retain the discovered exact revision and basis.".into(),
            references: vec![discovered, basis.clone()],
        };
        self.submit(
            "read_old_revision_under_warning",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        let resolution_prepared = self.prepare("reuse-explicit-resolution", "a", "reuse", false)?;
        self.check(
            d_has_body(&resolution_prepared, &target, D_TARGET)?
                && d_has_body(&resolution_prepared, &qualification, D_QUALIFICATION)?,
            "old revision supply enforces exact required accompaniment",
        )?;
        self.check(
            d_has_body(&resolution_prepared, &basis, D_BASIS)?,
            "resolution receives a fresh permitted exact basis body",
        )?;
        let current = self.state()?.semantic_warnings["combined-warning"].clone();
        self.check(
            d_entries(&resolution_prepared)?.iter().any(|entry| {
                entry["reference"]["source"] == current.marker.source
                    && entry["reference"]["revision"] == current.marker.revision
            }),
            "old revision supply includes accepted warning receipt",
        )?;
        let control = d_control(&resolution_prepared)?;
        let warning_view = control["semantic_warnings"]
            .as_array()
            .ok_or("warning context missing")?
            .iter()
            .find(|warning| warning["id"] == "combined-warning")
            .ok_or("current exact warning not supplied")?;
        let revision = warning_view["revision"]
            .as_str()
            .ok_or("warning revision absent")?
            .to_owned();
        self.check(
            revision == current.current_revision() && warning_view["active"] == true,
            "resolution copies actual current supplied warning identity",
        )?;
        let resolution_response = d_response(&resolution_prepared, vec![p::Operation::WarningResolve {
            id: "resolve-warning".into(), warning_id: "combined-warning".into(), warning_revision: revision,
            scope: p::Scope::Assets {}, basis: vec![basis], disposition: p::WarningResolutionDisposition::Release {},
            reason: "Synthetic original-primary resolution under new explicit work authority; no quality conclusion.".into(), depends_on: Vec::new(),
        }], d_immediate())?;
        let resolution_report = self.submit(
            "explicit_authorized_warning_resolution",
            &resolution_prepared,
            &resolution_response,
            AdoptionStatus::Adopted,
        )?;
        let resolved = self.state()?.semantic_warnings["combined-warning"].clone();
        self.check(
            !resolved.active()
                && resolved.history.len() == 1
                && resolved.history[0].authored.work == "reuse"
                && resolved.authored.work == "work",
            "resolution is append-only and attributed to actual fresh authority",
        )?;
        self.unchanged_replay(
            "historical_warning_adoption_replay",
            &adoption_prepared,
            &adoption_response,
            &adoption_report,
        )?;
        self.unchanged_replay(
            "exact_warning_resolution_replay",
            &resolution_prepared,
            &resolution_response,
            &resolution_report,
        )?;
        let prepared = self.prepare("reuse-resolution-receipt", "a", "reuse", false)?;
        self.check(
            d_has_body(&prepared, &target, D_TARGET)?,
            "released old exact bytes remain available without rewriting",
        )?;
        let marker = &resolved.history[0].marker;
        self.check(
            d_entries(&prepared)?.iter().any(|entry| {
                entry["reference"]["source"] == marker.source
                    && entry["reference"]["revision"] == marker.revision
            }),
            "released use still supplies exact required resolution receipt",
        )?;
        let response = d_response(&prepared, Vec::new(), d_stop())?;
        self.submit(
            "stop_after_resolution_receipt_supply",
            &prepared,
            &response,
            AdoptionStatus::Adopted,
        )?;
        self.check(
            self.state()?.sources["warning-target"].revisions[0].record == target_record,
            "warned immutable bytes never rewritten",
        )?;
        self.not_complete()
    }
    fn run(&mut self) -> DResult<()> {
        self.setup()?;
        match self.case.schedule.as_str() {
            "normal_completion" => self.normal(),
            "event_before_wait" => self.events(true, false),
            "event_after_wait" => self.events(false, false),
            "interrupted_dispatch_late_reconciliation" => self.events(false, true),
            "freeze_pause_resume" => self.freeze(),
            "warning_correction_reuse" => self.warnings(),
            _ => Err("unknown schedule".into()),
        }
    }
    fn audit(&mut self) -> DResult<(u64, usize, usize)> {
        let state = self.state()?;
        let actual_admissions: u64 = state
            .personas
            .values()
            .map(|persona| persona.admission_serial)
            .sum();
        self.check(
            actual_admissions <= 32 && self.persona_preparation_attempts <= 32,
            "all persona admissions bounded before and after admission",
        )?;
        self.check(
            state.personas.len() == self.case.roster_size,
            "final actual roster identity count",
        )?;
        self.check(
            !state.config.allow_nonconformant_research,
            "live research remains disabled",
        )?;
        self.check(
            state
                .work_meta
                .values()
                .all(|meta| meta.original.request == self.task.request),
            "every actual need uses configured private request bytes",
        )?;
        let mut persona_invocations = 0usize;
        let mut mock_invocations = 0usize;
        let mut accepted_by_actor = BTreeMap::<String, BTreeSet<String>>::new();
        for (_, value) in d_rows(&self.root, "invocation", 34)? {
            let invocation: s::InvocationRecord = serde_json::from_value(value).map_err(d_error)?;
            let request = invocation
                .request
                .as_ref()
                .ok_or("retained invocation request absent")?;
            if request["request"]["profile"] == "synthetic" {
                persona_invocations += 1;
                let prepared: Prepared =
                    serde_json::from_value(request.clone()).map_err(d_error)?;
                d_require(
                    prepared.request.profile == Profile::Synthetic
                        && prepared.provider_preparation.is_none(),
                    "no provider invocation",
                )?;
                d_require(
                    prepared.prompt_sha256 == s::sha256_bytes(prepared.prompt.as_bytes()),
                    "persisted exact prepared prompt hash",
                )?;
                d_require(
                    d_control(&prepared)?["original_need"]["request"].as_str()
                        == Some(self.task.request.as_str()),
                    "persisted prompt uses actual task request",
                )?;
                if invocation.state == s::InvocationState::Adopted {
                    let archive = d_cell(
                        &self.root,
                        "object",
                        &format!("adoption:{}", s::sha256_bytes(invocation.id.as_bytes())),
                    )?;
                    let accepted: AdoptedResponse =
                        serde_json::from_value(archive["body"].clone()).map_err(d_error)?;
                    let authored = accepted
                        .authored_json
                        .as_ref()
                        .ok_or("accepted exact response absent")?;
                    d_require(
                        s::sha256_bytes(authored.as_bytes()) == accepted.response_hash
                            && accepted.report.synthetic,
                        "actual accepted synthetic response bytes",
                    )?;
                    accepted_by_actor
                        .entry(accepted.actor)
                        .or_default()
                        .insert(accepted.response_hash);
                }
            } else {
                d_require(
                    request["mock_only"] == true,
                    "unexpected invocation transport",
                )?;
                mock_invocations += 1;
            }
        }
        let intents = d_rows(&self.root, "intent", 2)?;
        self.check(
            persona_invocations as u64 == actual_admissions,
            "persona counters equal actual retained invocation admissions",
        )?;
        self.check(
            mock_invocations == self.mock_submission_attempts
                && intents.len() == self.mock_slots
                && intents.len() <= 2,
            "separate mock invocation and intent attempt accounting",
        )?;
        self.check(
            state.personas.keys().all(|actor| {
                accepted_by_actor
                    .get(actor)
                    .is_some_and(|responses| !responses.is_empty())
            }),
            "every persona has its own distinct accepted response",
        )?;
        for (_, value) in &intents {
            let intent: s::IntentRecord = serde_json::from_value(value.clone()).map_err(d_error)?;
            d_require(
                intent.intent.body["mock_only"] == true,
                "non-mock intent forbidden",
            )?;
            d_require(
                intent
                    .history
                    .iter()
                    .filter(|receipt| receipt.status == s::ReceiptStatus::Dispatched)
                    .count()
                    == 1,
                "one admitted mock execution maximum per intent",
            )?;
        }
        self.check(
            state
                .local_deliveries
                .values()
                .all(|delivery| delivery["external_export"] == false),
            "all deliveries are local-only",
        )?;
        let meta = d_meta(&self.root)?;
        self.check(
            meta["ordinary"]["consumed"]["attempts"].as_u64()
                == Some(actual_admissions + mock_invocations as u64 + intents.len() as u64),
            "Store attempt usage equals persona plus separate mock invocation and intent attempts",
        )?;
        self.check(
            !self.checkpoints.is_empty(),
            "nonempty actual transition evidence",
        )?;
        Ok((actual_admissions, mock_invocations, intents.len()))
    }
}

fn d_run_case(case: DCase, task: DTask, output: Option<&Path>, keep: bool) -> DResult<()> {
    let mut trace = DTrace::new(case.clone(), task)?;
    let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| trace.run()));
    let mut failure = match execution {
        Ok(result) => result.err(),
        Err(_) => {
            Some("unexpected fixture panic; bounded checkpoints retained where available".into())
        }
    };
    // Check resource and local-only invariants even after an aborted trace.
    let audit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| trace.audit()));
    let counters = match audit {
        Ok(Ok(counters)) => Some(counters),
        Ok(Err(error)) => {
            if failure.is_none() {
                failure = Some(error);
            }
            None
        }
        Err(_) => {
            if failure.is_none() {
                failure = Some("audit panic".into());
            }
            None
        }
    };
    drop(trace.app.take());
    let final_projection = match d_projection(&trace.root) {
        Ok(value) => value,
        Err(error) => {
            if failure.is_none() {
                failure = Some(error.clone());
            }
            json!({"projection_error":error})
        }
    };
    let actual_admissions = final_projection["synthetic_persona_admissions"]
        .as_u64()
        .unwrap_or(0);
    let mock_invocations = d_rows(&trace.root, "invocation", 34).ok().map(|rows| {
        rows.iter()
            .filter(|(_, invocation)| invocation["request"]["mock_only"] == true)
            .count()
    });
    let mock_effects = d_rows(&trace.root, "intent", 2).ok().map(|rows| rows.len());
    let retained_root = if keep {
        let temp = trace.temp.take().ok_or("temporary state owner absent")?;
        // The Runtime and all schedule-local Store owners have been dropped.
        // Deliberate retention is restricted to the explicitly configured run.
        std::mem::forget(temp);
        Some(trace.root.to_string_lossy().into_owned())
    } else {
        None
    };
    let assertions_passed = failure.is_none() && counters.is_some();
    let report = json!({"version":1,"case":case,
        "task_request_sha256":s::sha256_bytes(trace.task.request.as_bytes()),
        "synthetic_persona_admissions":actual_admissions,
        "mock_effects":mock_effects,"mock_invocation_attempts":mock_invocations,"mock_intent_attempts":mock_effects,
        "persona_preparation_attempts":trace.persona_preparation_attempts,"mock_submission_attempts":trace.mock_submission_attempts,
        "provider_calls":0,"external_effects":false,"assertions_passed":assertions_passed,
        "assertions":trace.assertions,"failure":failure,"checkpoints":trace.checkpoints,"final":final_projection,
        "retained_state_root":retained_root,
        "evidence_scope":"Synthetic deterministic local mechanism assertions only. Completion, when supported, concerns the explicit synthetic mandate.",
        "claims_excluded":["conceptual quality","voluntary collaboration","runtime behavior of an LLM","maturation from elapsed time","empirical comparative benefit"]});
    d_require(
        serde_json::to_vec(&report).map_err(d_error)?.len() <= D_REPORT_BYTES,
        "final report byte ceiling",
    )?;
    if let Some(directory) = output {
        d_write_report(directory, &case, &report)?;
    }
    eprintln!("local D {}: assertions={}, persona_admissions={}, mock_invocations={:?}, mock_intents={:?}, checkpoints={}",
        case.id, assertions_passed, actual_admissions, mock_invocations, mock_effects,
        report["checkpoints"].as_array().map_or(0, Vec::len));
    if assertions_passed {
        Ok(())
    } else {
        Err(format!(
            "{}: {}",
            case.id,
            report["failure"]
                .as_str()
                .unwrap_or("resource or evidence audit failed")
        ))
    }
}
fn d_run_cases(
    cases: Vec<DCase>,
    tasks: &[DTask],
    output: Option<&Path>,
    keep: bool,
) -> DResult<()> {
    // Refuse identity collisions before executing any case; never overwrite
    // evidence from a previous successful or failed run.
    if let Some(directory) = output {
        for case in &cases {
            let path = directory.join(format!("{}.json", case.id));
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(d_error(error)),
                Ok(_) => return Err("output already contains a case report".into()),
            }
        }
    }
    let mut failures = Vec::new();
    for case in cases {
        let task = tasks
            .iter()
            .find(|task| task.id == case.task_id)
            .ok_or("case task absent")?
            .clone();
        if let Err(error) = d_run_case(case, task, output, keep) {
            failures.push(d_error(error));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} trace(s) failed: {}",
            failures.len(),
            failures.join(" | ")
        ))
    }
}

#[test]
fn combined_local_default_six_schedules() {
    let task = DTask { id: "default".into(), private_description: "Domain-neutral synthetic mechanism fixture.".into(),
        request: "Retain an abstract exact local result and account for each attributed lifecycle transition.".into() };
    let cases = D_SCHEDULES
        .iter()
        .map(|schedule| DCase {
            id: format!("default-3-{schedule}"),
            task_id: task.id.clone(),
            roster_size: 3,
            schedule: (*schedule).into(),
        })
        .collect();
    let output = d_output().expect("valid private output directory when configured");
    let result = d_run_cases(cases, &[task], output.as_deref(), false);
    assert!(result.is_ok(), "{}", result.err().unwrap_or_default());
}

#[test]
#[ignore = "explicit private manifest and private output directory required; deterministic local fixtures only"]
fn combined_local_private_matrix() {
    let manifest_path = std::env::var_os("PERSONAS_FIXTURE_MANIFEST")
        .expect("PERSONAS_FIXTURE_MANIFEST is required");
    let manifest_path = d_private_path(Path::new(&manifest_path), false, false)
        .expect("private bounded manifest path");
    let manifest: DManifest = read_json_file(&manifest_path, 256 * 1024)
        .expect("complete unique-key bounded private manifest");
    manifest
        .validate()
        .expect("all 24 private combinations must be valid");
    let output = d_output()
        .expect("valid output directory")
        .expect("PERSONAS_FIXTURE_OUTPUT_DIR is required");
    let keep = d_keep().expect("valid state-retention setting");
    let result = d_run_cases(
        manifest.traces,
        &manifest.task_configurations,
        Some(&output),
        keep,
    );
    assert!(result.is_ok(), "{}", result.err().unwrap_or_default());
}
