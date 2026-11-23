use super::{resources::Catalog, Moment, Page};
use crate::neural::{
    commands, context, failure, geometry, html_print, operations, protocol::Operation,
    provider_evidence::Accepted, store, workspace, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) struct Receipt {
    seq: i64,
    created: i64,
    actor: String,
    tool: String,
    result: String,
    effect: Option<String>,
}
pub(super) type Receipts = HashMap<String, Option<Receipt>>;

fn external(tool: &str) -> bool {
    matches!(
        tool,
        "compute.run"
            | "command.run"
            | "command.local"
            | "image.render_svg"
            | "document.print_html"
            | "document.render_pdf"
            | "geometry.render"
    )
}

pub(super) fn receipts(
    db: &Connection,
    work: &str,
    actor: Option<&str>,
) -> Result<HashMap<String, Receipts>> {
    let mut stmt = db.prepare("SELECT seq,created,actor,invocation,digest FROM events WHERE work=?1
        AND (?2 IS NULL OR actor=?2) AND target=actor AND kind='tool_result' AND invocation IS NOT NULL ORDER BY seq")?;
    let mut rows = stmt.query(params![work, actor])?;
    let mut calls: HashMap<String, Receipts> = HashMap::new();
    while let Some(row) = rows.next()? {
        let body: Value = store::object(db, &row.get::<_, String>(4)?)?;
        let (Some(tool), Some(op), Some(result)) = (
            body["tool"].as_str(),
            body["operation"].as_str(),
            body["result"].as_str(),
        ) else {
            continue;
        };
        if !external(tool) && !operations::returns_alias(tool) {
            continue;
        }
        let entry = Receipt {
            seq: row.get(0)?,
            created: row.get(1)?,
            actor: row.get(2)?,
            tool: tool.into(),
            result: result.into(),
            effect: body["effect"].as_str().map(str::to_owned),
        };
        calls
            .entry(row.get(3)?)
            .or_default()
            .entry(op.into())
            .and_modify(|r| *r = None)
            .or_insert(Some(entry));
    }
    Ok(calls)
}

pub(super) fn observe(
    catalog: &mut Catalog<'_>,
    accepted: &Accepted,
    events: &Receipts,
    page: &mut Page<'_>,
) -> Result<()> {
    let snapshot = &accepted.snapshot;
    let mut aliases = BTreeMap::new();
    for (ordinal, raw) in accepted.response["operations"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(id) = raw["id"].as_str() else {
            continue;
        };
        let Some(entry) = events.get(id) else {
            continue;
        };
        let Some(event) = entry else {
            page.unverified_receipts += 1;
            continue;
        };
        if event.actor != snapshot.actor || raw["tool"] != event.tool {
            page.unverified_receipts += 1;
            continue;
        }
        let mut op: Operation = serde_json::from_value(raw.clone())?;
        let Some(digest) = event.result.strip_prefix("object:") else {
            page.unverified_receipts += 1;
            continue;
        };
        let result: Value = store::object(catalog.db, digest)?;
        if external(&op.tool) || matches!(op.tool.as_str(), "result.deliver" | "human.request" | "human.issue_packet") {
            if operations::resolve_operation(&mut op, &aliases).is_err() {
                page.unverified_receipts += 1;
                continue;
            }
            let moment = Moment {
                snapshot,
                created: event.created,
                event: Some(event.seq),
                evidence: &event.result,
            };
            if external(&op.tool) {
                let Some(effect) = event.effect.as_deref() else {
                    page.unverified_receipts += 1;
                    continue;
                };
                let queued: Option<(String,String,Option<String>)> = catalog.db.query_row(
                    "SELECT state,operation,result FROM outbox WHERE id=?1 AND invocation=?2 AND actor=?3 AND work=?4 AND ordinal=?5",
                    params![effect,snapshot.invocation,snapshot.actor,snapshot.work,i64::try_from(ordinal)?],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
                let Some((state, operation, recorded)) = queued else {
                    page.unverified_receipts += 1;
                    continue;
                };
                if state != "completed" {
                    continue;
                }
                if recorded.as_deref() != Some(digest)
                    || store::object::<Value>(catalog.db, &operation)? != serde_json::to_value(&op)?
                {
                    page.unverified_receipts += 1;
                    continue;
                }
                match execution(catalog, &moment, &op, &result) {
                    Ok(proof) => {
                        for member in proof.members {
                            if let Some(asset) = catalog.source(&member.reference)? {
                                page.add(
                                    &moment,
                                    &asset,
                                    "execution",
                                    proof.status,
                                    member.role,
                                    &op.tool,
                                    None,
                                    member.detail,
                                );
                            }
                        }
                    }
                    Err(_) => page.unverified_receipts += 1,
                }
            } else if event.effect.is_some()
                || package(catalog, &moment, &op, &result, page).is_err()
            {
                page.unverified_receipts += 1;
            }
        }
        if operations::returns_alias(&op.tool) && event.effect.is_none() {
            if let Some(reference) = result["reference"].as_str() {
                aliases.insert(op.id, reference.into());
            }
        }
    }
    Ok(())
}

fn package(
    catalog: &mut Catalog<'_>,
    time: &Moment<'_>,
    op: &Operation,
    result: &Value,
    page: &mut Page<'_>,
) -> Result<()> {
    let (kind, author_key) = if op.tool == "result.deliver" {
        ("delivery", "owner")
    } else {
        ("human_request", "requester")
    };
    let reference = result["reference"]
        .as_str()
        .ok_or_else(|| failure("Missing issued package"))?;
    let package = store::exact(catalog.db, reference)?;
    let packet = op.tool == "human.issue_packet";
    if package.reference.handle() != reference
        || package.reference.kind != kind
        || (!packet && package.reference.revision != 1)
        || package.body[author_key] != time.snapshot.actor
        || package.body["work"] != time.snapshot.work
        || (!packet && package.body["title"] != op.input["title"])
        || !store::written_by(catalog.db, &package, &time.snapshot.invocation)?
    {
        return Err(failure(
            "Package identity differs from the accepted operation",
        ));
    }
    let mut requested: Vec<String> = op
        .input
        .get("artifacts")
        .map(|a| serde_json::from_value(a.clone()))
        .transpose()?
        .unwrap_or_default();
    if packet {
        let prior = store::exact(catalog.db, op.input["base"].as_str().ok_or_else(|| failure("Missing base request"))?)?;
        crate::neural::human_requests::packet::verify(catalog.db, &prior, &package)?;
        let delivery = store::exact(catalog.db, op.input["delivery"].as_str().ok_or_else(|| failure("Missing field packet delivery"))?)?;
        requested = serde_json::from_value(delivery.body["artifacts"].clone())?;
    } else if kind == "human_request" {
        let deliveries: Vec<String> = op
            .input
            .get("deliveries")
            .map(|r| serde_json::from_value(r.clone()))
            .transpose()?
            .unwrap_or_default();
        let (files, included) =
            crate::neural::human_requests::attachments(catalog.db, &requested, &deliveries)?;
        let handles = included.iter().map(|p| &p.reference).collect::<Vec<_>>();
        if package.body.get("deliveries").cloned() != (!included.is_empty()).then(|| json!(handles))
            || package.body.get("delivery_details").cloned()
                != (!included.is_empty()).then(|| json!(included))
        {
            return Err(failure(
                "Included deliveries differ from the accepted human request",
            ));
        }
        requested = files;
    }
    let members: Vec<String> = serde_json::from_value(package.body["artifacts"].clone())?;
    let paths = operations::package_paths(catalog.db, &members)?;
    if paths != operations::package_paths(catalog.db, &requested)? {
        return Err(failure(
            "Issued package members differ from the accepted operation",
        ));
    }
    let moment = Moment {
        evidence: reference,
        ..*time
    };
    let mut files = Vec::new();
    for (path, reference) in paths {
        if let Some(asset) = catalog.source(&reference)? {
            catalog.bytes(&asset.blob)?;
            files.push((asset, path));
        }
    }
    for (asset, path) in files {
        page.add(
            &moment,
            &asset,
            kind,
            "packaged",
            "artifact",
            &op.tool,
            None,
            json!({"package_path":path}),
        );
    }
    Ok(())
}

struct Member {
    reference: String,
    role: &'static str,
    detail: Value,
}
fn member(reference: &str, role: &'static str, detail: Value) -> Member {
    Member {
        reference: reference.into(),
        role,
        detail,
    }
}

struct Execution {
    status: &'static str,
    members: Vec<Member>,
}
impl Execution {
    fn new(success: bool, members: Vec<Member>) -> Self {
        Self {
            status: if success { "succeeded" } else { "failed" },
            members,
        }
    }
}

fn named(value: &Value) -> Result<&str> {
    value
        .as_str()
        .ok_or_else(|| failure("Missing exact execution input"))
}

fn execution(
    catalog: &mut Catalog<'_>,
    time: &Moment<'_>,
    op: &Operation,
    result: &Value,
) -> Result<Execution> {
    if op.tool == "document.print_html" {
        let input: html_print::PrintHtml = serde_json::from_value(op.input.clone())?;
        html_print::verify(catalog.db, &input, result)?;
        let success = result["success"] == true;
        let detail = if success {
            json!({"source_bytes":result["source_bytes"],"pdf":result["reference"],"page_count":result["page_count"]})
        } else {
            json!({"input_bytes_confirmed":result["source_bytes"].is_string()})
        };
        let mut files = vec![member(
            &input.reference,
            if success {
                "print_source"
            } else {
                "requested_source"
            },
            detail,
        )];
        for (index, resource) in input.resources.iter().enumerate() {
            let object = &result["resources"][index]["object"];
            let detail = if success {
                json!({"mount":resource.path,"object":object,"pdf":result["reference"],"page_count":result["page_count"]})
            } else {
                json!({"mount":resource.path,"input_bytes_confirmed":object.is_string()})
            };
            files.push(member(
                &resource.reference,
                if success {
                    "print_resource"
                } else {
                    "requested_resource"
                },
                detail,
            ));
        }
        return Ok(Execution::new(success, files));
    }
    if result["success"] == false && result["error"].is_string() {
        // Failure was durably recorded, but the worker did not report mounted
        // bytes. Only the inputs named in the accepted attempt can be attributed.
        let requested = json!({"input_bytes_confirmed":false});
        let members = match op.tool.as_str() {
            "compute.run" => vec![member(
                named(&op.input["program"])?,
                "requested_program",
                requested,
            )],
            "image.render_svg" | "document.render_pdf" => vec![member(
                named(&op.input["reference"])?,
                "requested_source",
                requested,
            )],
            "geometry.render" => {
                let input: geometry::Render = serde_json::from_value(op.input.clone())?;
                let mut files = vec![member(
                    &input.reference,
                    "requested_source",
                    requested.clone(),
                )];
                files.extend(
                    input
                        .companions
                        .iter()
                        .map(|r| member(r, "requested_companion", requested.clone())),
                );
                files
            }
            "command.run" | "command.local" => {
                let mut files = Vec::new();
                let (inputs, stdin) = if op.tool == "command.run" {
                    let input: commands::Run = serde_json::from_value(op.input.clone())?;
                    let installation = store::exact(catalog.db, &input.installation)?;
                    if installation.reference.handle() == input.installation
                        && installation.body["format"] == "portable-command-v1"
                    {
                        if let Some(source) = installation.body["source"].as_str() {
                            files.push(member(source, "requested_program", requested.clone()));
                        }
                    }
                    (input.files, input.stdin)
                } else {
                    let input: commands::local::Run = serde_json::from_value(op.input.clone())?;
                    (input.files, input.stdin)
                };
                for (path, r) in inputs {
                    files.push(member(
                        &r,
                        "requested_file",
                        json!({"mount":path,"input_bytes_confirmed":false}),
                    ))
                }
                if let Some(stdin) = stdin {
                    files.push(member(&stdin, "requested_stdin", requested))
                }
                files
            }
            _ => return Err(failure("Unknown execution receipt")),
        };
        return Ok(Execution::new(false, members));
    }
    match op.tool.as_str() {
        "compute.run" => compute(catalog, op, result),
        "command.run" | "command.local" => command(catalog, time, op, result),
        "image.render_svg" => svg(catalog, op, result),
        "document.render_pdf" => {
            let page = crate::neural::pdf_evidence::verify(catalog.db, &op.input, result)?;
            let detail = json!({
                "source_bytes": page.source_bytes,
                "page": page.page,
                "page_count": page.page_count,
                "image": page.image,
                "width": page.width,
                "height": page.height
            });
            Ok(Execution::new(
                true,
                vec![member(&page.source, "render_source", detail)],
            ))
        }
        "geometry.render" => render(catalog, op, result),
        _ => Err(failure("Unknown execution receipt")),
    }
}

fn checked_bytes(
    catalog: &mut Catalog<'_>,
    reference: &str,
    object: &Value,
    size: &Value,
) -> Result<()> {
    let digest = workspace::byte_digest(catalog.db, reference)?;
    if object != &format!("object:{digest}")
        || size.as_u64() != Some(catalog.bytes(&digest)?.len() as u64)
    {
        return Err(failure(
            "Recorded execution bytes differ from the named file",
        ));
    }
    Ok(())
}

fn compute(catalog: &mut Catalog<'_>, op: &Operation, result: &Value) -> Result<Execution> {
    let program = named(&op.input["program"])?;
    let input = named(&op.input["input"])?;
    let outcome: crate::computation::Outcome = serde_json::from_value(result["outcome"].clone())?;
    if result["program"] != program
        || !result["engine"].is_string()
        || outcome.succeeded != outcome.output.is_some()
        || outcome.succeeded == outcome.failure.is_some()
    {
        return Err(failure("Inconsistent compute receipt"));
    }
    let mut detail = json!({"input_digest_verified":false});
    if let Some(run) = result.get("execution") {
        checked_bytes(catalog, program, &run["source"], &run["bytes"])?;
        let digest = crate::store::sha256_bytes(input.as_bytes());
        if run["input_sha256"] != digest || run["input_bytes"].as_u64() != Some(input.len() as u64)
        {
            return Err(failure("Compute input differs from the retained execution"));
        }
        detail =
            json!({"input_digest_verified":true,"input_sha256":digest,"input_bytes":input.len()});
    } else {
        // Earlier receipts name the exact program and its outcome. They do
        // not independently record the input digest.
        let digest = workspace::byte_digest(catalog.db, program)?;
        catalog.bytes(&digest)?;
    }
    Ok(Execution::new(
        outcome.succeeded,
        vec![member(program, "program", detail)],
    ))
}

fn command(
    catalog: &mut Catalog<'_>,
    time: &Moment<'_>,
    op: &Operation,
    result: &Value,
) -> Result<Execution> {
    let reference = named(&result["receipt"])?;
    let receipt = store::exact(catalog.db, reference)?;
    let r = &receipt.body;
    if receipt.reference.kind != "command_run"
        || receipt.reference.handle() != reference
        || !store::written_by(catalog.db, &receipt, &time.snapshot.invocation)?
        || r != &result["result"]
        || r["origin"] != "persona"
        || r["actor"] != time.snapshot.actor
        || r["work"] != time.snapshot.work
        || r["invocation"] != time.snapshot.invocation
        || r["operation"] != op.id
    {
        return Err(failure(
            "Command receipt differs from the accepted operation",
        ));
    }
    let mut members = Vec::new();
    let local = op.tool == "command.local";
    let (files, stdin, expected_outputs) = if local {
        let input: commands::local::Run = serde_json::from_value(op.input.clone())?;
        if input.directory == commands::local::Directory::Workspace
            && (r["host"]["directory_mode"] != "workspace" || !r["host"]["working_directory"].is_string()
                || r["host"]["working_directory"] != time.snapshot.data["host_working_directory"]) {
            return Err(failure("Shared command directory differs from the admitted decision"));
        }
        if r["timed_out"] == true && (input.timeout_seconds.is_none() || r["process_started"] != true
            || r["cancelled"] != false || r["success"] != false || !r["error"].is_string()) {
            return Err(failure("Local timeout differs from the selected deadline or process outcome"));
        }
        if r["format"] != "local-project-run-v1"
            || r["engine"] != "local"
            || r["intent"] != json!(input)
            || r["argv0"] != input.program
            || r["arguments"] != json!(input.arguments)
            || !r["process_started"].is_boolean()
            || !r["cancelled"].is_boolean()
            || (r["process_started"] == true && !r["host"]["executable"].is_string())
        {
            return Err(failure(
                "Local project receipt differs from the selected program or intent",
            ));
        }
        for item in [&r["stdout"], &r["stderr"]].into_iter().chain(
            r["outputs"]
                .as_object()
                .into_iter()
                .flat_map(|map| map.values()),
        ) {
            checked_bytes(
                catalog,
                named(&item["object"])?,
                &item["object"],
                &item["bytes"],
            )?;
        }
        (input.files, input.stdin, input.outputs)
    } else {
        let input: commands::Run = serde_json::from_value(op.input.clone())?;
        if r["format"] != "portable-command-run-v1"
            || r["intent"] != json!(input)
            || r["arguments"] != json!(input.arguments)
            || r["installation"] != input.installation
        {
            return Err(failure(
                "Portable command receipt differs from the accepted intent",
            ));
        }
        let installed = store::exact(catalog.db, &input.installation)?;
        if installed.reference.kind != "installation"
            || installed.reference.handle() != input.installation
            || installed.body["format"] != "portable-command-v1"
            || installed.body["program"] != r["program"]
            || installed.body["engine"] != r["engine"]
        {
            return Err(failure(
                "Portable command differs from its installed program",
            ));
        }
        let object = named(&r["program"])?;
        checked_bytes(catalog, object, &r["program"], &installed.body["bytes"])?;
        if let Some(source) = installed.body["source"]
            .as_str()
            .filter(|s| s.starts_with("asset:"))
        {
            checked_bytes(catalog, source, &r["program"], &installed.body["bytes"])?;
            members.push(member(
                source,
                "program",
                json!({"installation":input.installation,"receipt":reference}),
            ));
        }
        (input.files, input.stdin, input.outputs)
    };
    let mounted = r["files_supplied"]
        .as_object()
        .ok_or_else(|| failure("Missing portable project files"))?;
    if mounted.len() != files.len() {
        return Err(failure("Mounted project files differ"));
    }
    let launched = !local || r["process_started"] == true;
    for (path, file) in &files {
        let item = &r["files_supplied"][path];
        if item["reference"] != *file {
            return Err(failure("Different mounted file"));
        }
        checked_bytes(catalog, file, &item["object"], &item["bytes"])?;
        members.push(member(
            file,
            if launched {
                "project_file"
            } else {
                "staged_file"
            },
            json!({"mount":path,"receipt":reference}),
        ));
    }
    if let Some(stdin) = stdin {
        if r["stdin"]["reference"] != stdin {
            return Err(failure("Different command stdin"));
        }
        checked_bytes(catalog, &stdin, &r["stdin"]["object"], &r["stdin"]["bytes"])?;
        members.push(member(
            &stdin,
            if launched { "stdin" } else { "staged_stdin" },
            json!({"receipt":reference}),
        ));
    } else if !r["stdin"].is_null() {
        return Err(failure("Unexpected command stdin"));
    }
    let process = launched
        && r["exit_code"] == 0
        && r["error"].is_null()
        && (!local || (r["cancelled"] == false && r["signal"].is_null()));
    let outputs = r["outputs"]
        .as_object()
        .ok_or_else(|| failure("Missing command outputs"))?;
    if outputs.keys().any(|path| !expected_outputs.contains(path)) {
        return Err(failure("Receipt contains an unrequested output"));
    }
    let missing: Vec<_> = expected_outputs
        .iter()
        .filter(|path| !outputs.contains_key(*path))
        .collect();
    let success = process && missing.is_empty();
    if r["process_succeeded"] != process
        || r["outputs_complete"] != missing.is_empty()
        || r["missing_outputs"] != json!(missing)
        || r["success"] != success
    {
        return Err(failure("Inconsistent command outcome"));
    }
    Ok(Execution::new(success, members))
}

fn svg(catalog: &mut Catalog<'_>, op: &Operation, result: &Value) -> Result<Execution> {
    let source = named(&op.input["reference"])?;
    let blob = workspace::byte_digest(catalog.db, source)?;
    let image = named(&result["image_blob"])?;
    let bytes = catalog.bytes(image)?;
    if result["source"] != source
        || result["source_bytes"] != format!("object:{blob}")
        || result["reference"] != format!("object:{image}")
        || result["mime"] != "image/png"
        || context::image_media(&bytes) != Some("image/png")
        || result["bytes"].as_u64() != Some(bytes.len() as u64)
        || !result["renderer"].is_string()
        || result["width"].as_u64().unwrap_or(0) == 0
        || result["height"].as_u64().unwrap_or(0) == 0
    {
        return Err(failure("Different SVG render input or output"));
    }
    catalog.bytes(&blob)?;
    Ok(Execution::new(
        true,
        vec![member(
            source,
            "render_source",
            json!({"image":result["reference"]}),
        )],
    ))
}

fn render(catalog: &mut Catalog<'_>, op: &Operation, result: &Value) -> Result<Execution> {
    let input: geometry::Render = serde_json::from_value(op.input.clone())?;
    if result["source"] != input.reference || result["parameters"] != json!(input.parameters) {
        return Err(failure("Different render source or parameters"));
    }
    let mut references = input.companions;
    references.push(input.reference.clone());
    let paths = operations::package_paths(catalog.db, &references)?;
    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    for item in result["files_supplied"]
        .as_array()
        .ok_or_else(|| failure("Missing render inputs"))?
    {
        let reference = named(&item["reference"])?;
        let path = std::path::Path::new(named(&item["path"])?);
        if !seen.insert(path) || paths.get(path).map(String::as_str) != Some(reference) {
            return Err(failure("Different render project files"));
        }
        checked_bytes(catalog, reference, &item["content"], &item["bytes"])?;
        let role = if reference == input.reference {
            if item["path"] != result["entry"] {
                return Err(failure("Different render entry"));
            }
            "render_source"
        } else {
            "render_companion"
        };
        members.push(member(reference, role, json!({"mount":path})));
    }
    if seen.len() != paths.len() {
        return Err(failure("Missing render project file"));
    }
    let success = result["success"]
        .as_bool()
        .ok_or_else(|| failure("Missing render disposition"))?;
    if success {
        if result["exit_code"] != 0
            || !result["worker_error"].is_null()
            || result["errors"].as_array().is_none_or(|e| !e.is_empty())
        {
            return Err(failure("Inconsistent render outcome"));
        }
        checked_bytes(
            catalog,
            named(&result["mesh"])?,
            &result["mesh"],
            &result["bytes"],
        )?;
    }
    Ok(Execution::new(success, members))
}
