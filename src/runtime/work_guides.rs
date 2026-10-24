//! Primary-selected internal work definitions, discovered as ordinary assets.
//! A registered exact guide routes to a native definition. Authored file text
//! alone cannot register a command, grant authority, or establish execution.
use super::*;

pub(crate) const DIRECTORY: &str = "runtime/work-commands/";
const AUTHOR: &str = "ai-personas-runtime";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalog {
    pub version: u32,
    pub operator: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    pub guides: BTreeMap<String, p::ExactRef>,
}

pub(in crate::runtime) fn directory(catalog: &Catalog) -> &str {
    catalog.directory.as_deref().unwrap_or(DIRECTORY)
}

fn validate_catalog(catalog: &Catalog) -> Result<()> {
    let supported = match (catalog.version, catalog.directory.as_deref()) {
        (1, None) => true,
        (2, Some(directory)) => directory
            .strip_prefix(DIRECTORY)
            .and_then(|suffix| suffix.strip_suffix('/'))
            .is_some_and(|profile| {
                profile.len() == 64
                    && profile
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }),
        _ => false,
    };
    require(
        supported && !catalog.guides.is_empty(),
        "native work guide catalog unsupported",
    )
}

fn variants(schema: &Value) -> Result<&Vec<Value>> {
    let variants = schema["oneOf"]
        .as_array()
        .ok_or_else(|| invalid("native work schema is not a tagged union"))?;
    require(
        !variants.is_empty(),
        "native work schema contains no command definitions",
    )?;
    Ok(variants)
}

fn kind(variant: &Value) -> Result<&str> {
    variant["properties"]["kind"]["const"]
        .as_str()
        .ok_or_else(|| invalid("native work command identity unavailable"))
}

/// Keep only named variants and the definitions reachable from those variants.
/// Definition supply is independent of the complete admitted invocation contract.
pub(super) fn selected_schema(commands: &BTreeSet<String>) -> Result<Value> {
    if commands.is_empty() {
        return Ok(Value::Null);
    }
    let mut schema = work_schema();
    let known = variants(&schema)?
        .iter()
        .map(kind)
        .collect::<Result<BTreeSet<_>>>()?;
    require(
        commands
            .iter()
            .all(|command| known.contains(command.as_str())),
        "selected native work definition unavailable",
    )?;
    schema["oneOf"].as_array_mut().unwrap().retain(|variant| {
        variant["properties"]["kind"]["const"]
            .as_str()
            .is_some_and(|command| commands.contains(command))
    });
    p::retain_reachable_definitions(&mut schema);
    Ok(schema)
}

fn purpose(command: &str) -> &'static str {
    match command {
        "invite" => "Offer exact voluntary responsibility within the current mandate. An offer does not establish the receiver's acceptance.",
        "handoff" => "Offer transfer of your exact accepted responsibility. Responsibility stays with its current holder until an authorized disposition or actual receiver acceptance.",
        "put_artifact" => "Register an exact saved supporting-file revision as a work artifact candidate. A draft is not evidence, release, delivery or closure.",
        "assess" => "Record a criteria-bound assessment under an actually accepted review role, using exact available evidence and explicit limitations.",
        "release" => "Bind a currently supported shared decision to an exact release. Release does not perform delivery or establish requester satisfaction.",
        "deliver_release" => "Deposit an exact permitted release for authorized local recipients and retain a delivery receipt. This is local delivery, not an external export or proof of reading.",
        _ => "Use the current exact native input schema and the admitted work's authority and exact evidence.",
    }
}

fn related(command: &str) -> &'static [&'static str] {
    match command {
        "ask_question" => &["read_question", "answer_question", "conclude_question"],
        "read_question" => &["answer_question", "conclude_question"],
        "answer_question" => &["read_question", "conclude_question"],
        "conclude_question" => &["ask_question", "read_question"],
        "resolve_requester_feedback" | "account_peer_feedback" => {
            &["put_artifact", "record_model_evidence"]
        }
        "register_message_wait" => &["ask_question", "invite", "handoff"],
        "invite" | "handoff" => &["accept_arranged_offer", "propose_arrangement"],
        "propose_arrangement" => &["accept_arrangement", "decline_arrangement"],
        "accept_arrangement" | "decline_arrangement" => &["propose_arrangement"],
        "put_artifact" => &["propose_arrangement", "record_model_evidence", "judge"],
        "accept_arranged_offer" => &["propose_arrangement"],
        "record_model_evidence" | "assess" => &["propose_arrangement", "judge"],
        "judge" => &["record_model_evidence", "assess", "decide_shared"],
        "decide_shared" => &["propose_arrangement", "judge", "release"],
        "release" => &["decide_shared", "deliver_release"],
        "deliver_release" => &["release", "close_goal"],
        "close_goal" => &["deliver_release", "resolve_requester_feedback"],
        _ => &[],
    }
}

fn guide_text(variant: &Value) -> Result<String> {
    let command = kind(variant)?;
    let description = variant["description"]
        .as_str()
        .unwrap_or_else(|| purpose(command));
    let names = variant["properties"]
        .as_object()
        .ok_or_else(|| invalid("native work properties unavailable"))?
        .keys()
        .filter(|name| name.as_str() != "kind")
        .cloned()
        .collect::<Vec<_>>();
    let links = related(command)
        .iter()
        .map(|name| format!("{name}.md"))
        .collect::<Vec<_>>()
        .join(", ");
    let command_rules = work_instructions::guide_text(command);
    Ok(format!("# Native internal work command: {command}\n\n{description}\n\nInput field names for discovery: {}.\n\nRelated guide filenames in this directory: {links}. Use ordinary file discovery to locate relevant prerequisites and their exact current revisions. These are navigation cues, not required selections or an assigned workflow. These guides select detailed current definitions and instructions into a fresh input; they are optional supporting material, not an invocation allowlist.\n\n{command_rules}This runtime-authored supporting guide is not a persona's character, experience, grant or execution record. Read or select its complete exact UTF-8 revision through the ordinary file interface. When this exact current guide is supplied completely, CONTROL.work_capability.inputs_schema supplies this command definition. All implemented native commands remain callable under the admitted contract without a guide. Use a known current shape directly, or obtain a useful guide when uncertain. Copy exact current work references; guide text is neither acceptance nor an execution result. Exact work records, whole-response consistency and actual execution outcomes still apply. No external software is invoked by this internal command.\n", names.join(", ")))
}

fn records(
    state: &State,
    owner: &str,
    policy_id: &str,
) -> Result<(String, Vec<(String, f::FileRevision)>)> {
    let policy = current_policy(state, policy_id)?;
    let schema = work_schema();
    let bodies = variants(&schema)?
        .iter()
        .map(|variant| Ok((kind(variant)?.to_owned(), guide_text(variant)?)))
        .collect::<Result<Vec<_>>>()?;
    // Each profile has its own ordinary directory. Historical guide revisions
    // remain addressable and keep their permissions; discovery at the current
    // locator cannot confuse them with the current registered definitions.
    let profile = digest(&("native-work-guide-profile-v3", owner, policy, &bodies))?;
    let directory = format!("{DIRECTORY}{profile}/");
    let files = bodies
        .into_iter()
        .map(|(command, body)| {
            let record = f::FileRevision {
                // Logical identity stays readable; the sealed revision binds
                // the exact content, path, author and source policy.
                source: format!("native-work-guide:{command}"),
                revision: String::new(),
                path: format!("{directory}{command}.md"),
                scope: f::Scope::Assets,
                bytes: body.into_bytes(),
                author: AUTHOR.into(),
                policy: policy.clone(),
                qualifications: Vec::new(),
                retired: false,
            }
            .seal()?;
            Ok((command, record))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((directory, files))
}

pub(in crate::runtime) fn installation_summary(
    state: &State,
    owner: &str,
    policy_id: &str,
) -> Result<Value> {
    let (directory, records) = records(state, owner, policy_id)?;
    Ok(json!({"selective_work_commands":true,"command_selection_limits_invocation":false,"directory":directory,
        "guide_count":records.len(),
        "guide_body_bytes":records.iter().map(|(_, record)|record.bytes.len()).sum::<usize>(),
        "author":AUTHOR,"owner":owner,"policy_id":policy_id,
        "inference_performed":false,"resources_added":false,
        "meaning":"Native supporting guides consume storage. Their actual complete supply selects detailed schemas and instructions for that input. Every implemented native command remains callable without a guide or grant; the admitted contract binds its current definition. A guide is not an execution record."}))
}

/// Stage in a private candidate; the caller must atomically save it with the
/// current authorization/fences. Failure must not save a partially built catalog.
pub(in crate::runtime) fn install(
    state: &mut State,
    operator: &str,
    owner: &str,
    policy_id: &str,
    now: i64,
) -> Result<()> {
    identity(operator)?;
    let persona = state
        .personas
        .get(owner)
        .ok_or_else(|| held("native guide owner unavailable"))?;
    require(
        persona.created_by == operator,
        "native guide owner has another creating operator",
    )?;
    let audience = admission(persona, now);
    require(
        policy_permitted(state, current_policy(state, policy_id)?, &audience),
        "native guide source policy unavailable",
    )?;
    if let Some(catalog) = &state.work_guides {
        validate_catalog(catalog)?;
        require(
            catalog.operator == operator && catalog.owner == owner,
            "native work guide catalog authority changed",
        )?;
        for guide in catalog.guides.values() {
            let reference = file_reference(guide)?;
            require(
                lineage_permitted(state, std::slice::from_ref(&reference), &audience),
                "current native guide withdrawn or restricted; installation cannot restore it",
            )?;
        }
    }
    let mut guides = BTreeMap::new();
    let event_index = state.operator_events.len();
    let (directory, files) = records(state, owner, policy_id)?;
    for (command, file) in files {
        let reference = file.reference(f::Representation::Utf8);
        let current = state
            .work_guides
            .as_ref()
            .and_then(|catalog| catalog.guides.get(&command))
            .cloned();
        if let Some(source) = state.sources.get_mut(&file.source) {
            require(
                source.owner == owner
                    && source.protected
                    && !source.retired
                    && source.imported_by.as_deref() == Some(operator),
                "native guide identity conflict or withdrawal; installation cannot restore it",
            )?;
            if let Some(existing) = source
                .revisions
                .iter()
                .find(|revision| revision.record.revision == file.revision)
            {
                require(
                    existing.record.as_ref() == &file
                        && source.head == file.revision
                        && matches!(&existing.origin, Some(RevisionOrigin::OperatorImport {operator: author, ..}) if author == operator),
                    "native guide identity conflict; installation cannot rewind an older revision",
                )?;
            } else {
                require(
                    current.as_ref().is_some_and(|registered| {
                        registered.source == file.source && registered.revision == source.head
                    }),
                    "native guide identity is not the current registered source",
                )?;
                source.head = file.revision.clone();
                source.revisions.push(ManagedRevision {
                    record: file.into(),
                    lineage: Vec::new(),
                    qualification_sources: None,
                    exposure: Some(Exposure::default()),
                    origin: Some(RevisionOrigin::OperatorImport {
                        operator: operator.into(),
                        event_index,
                    }),
                });
            }
        } else {
            state.sources.insert(
                file.source.clone(),
                Source {
                    owner: owner.into(),
                    protected: true,
                    retired: false,
                    head: file.revision.clone(),
                    imported_by: Some(operator.into()),
                    revisions: vec![ManagedRevision {
                        record: file.into(),
                        lineage: Vec::new(),
                        qualification_sources: None,
                        exposure: Some(Exposure::default()),
                        origin: Some(RevisionOrigin::OperatorImport {
                            operator: operator.into(),
                            event_index,
                        }),
                    }],
                },
            );
        }
        guides.insert(command, wire_reference(&reference));
    }
    let catalog = Catalog {
        version: 2,
        operator: operator.into(),
        owner: owner.into(),
        directory: Some(directory),
        guides,
    };
    if state.work_guides.as_ref() != Some(&catalog) {
        state
            .operator_events
            .push(json!({"kind":"native_work_guides","operator":operator,
            "owner":owner,"catalog_sha256":digest(&catalog)?,"at_unix":now,
            "inference_performed":false,"resources_added":false}));
        state.work_guides = Some(catalog);
    }
    validate_inventory(state)?;
    corpus(state, &audience)?;
    Ok(())
}

pub(super) fn control(state: &State) -> Option<Value> {
    state.work_guides.as_ref().map(|catalog|json!({
        "mode":"selected_native_work_guides_v1","scope":"assets","directory":directory(catalog),
        "rule":"Guides in this directory are optional supporting material. Read or select relevant complete exact guide bodies to receive their detailed current command schemas and instructions in a fresh input. Metadata links supply neither body nor schema. All implemented native work commands remain callable under CONTROL.work_capability's admitted contract without selecting a guide; if you already know the current input shape, use it directly. If uncertain, choose useful guides through ordinary file discovery, exact reads or next_assets. A guide or definition is not an execution result."
    }))
}

/// A file link for a command already named by a supplied host contract. This
/// carries metadata restrictions, but neither supplies a body nor enables it.
pub(super) fn control_guide(
    state: &State,
    command: &str,
    contract: &str,
    audience: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Option<Value>> {
    let Some(reference) = state.work_guides.as_ref()
        .and_then(|catalog| catalog.guides.get(command)) else {
        return Ok(None);
    };
    let exact = file_reference(reference)?;
    if !lineage_permitted(state, std::slice::from_ref(&exact), audience) {
        return Ok(None);
    }
    let file = &record(state, &exact)?.record;
    let value = json!({"command":command,"referenced_by":contract,
        "reference":reference,"scope":file.scope,"path":file.path,
        "metadata_only":true});
    exposure.sources.push(exact);
    Ok(Some(value))
}

pub(super) fn capability(
    state: &State,
    bodies: &BTreeSet<f::Reference>,
    interface: Option<&WorkInterface>,
) -> Result<Value> {
    let mut value = match interface {
        Some(interface) => json!({"id":interface.capability_id,
            "binding_revision":interface.binding_revision,
            "invocation_contract":interface.invocation_contract,
            "inputs_schema":interface.inputs_schema,"definition_commands":interface.selected_commands,
            "definition_selection_limits_invocation":false,
            "rule":"All implemented native work commands are callable under this exact admitted contract. inputs_schema supplies only definitions selected through complete registered guide bodies; null means no detailed definition was selected. Missing schema supply is not a permission gate. Use a known current shape directly, or obtain a useful guide when uncertain. Work records, exact references, complete response validation and actual operation semantics still apply."}),
        None => json!({"enabled":false,
            "reason":"This invocation mode does not supply ordinary work commands. Guide registration or selection does not change that mode."}),
    };
    if let Some(interface) = interface {
        if let Some(instructions) = work_instructions::for_interface(interface)? {
            value["command_instructions"] = instructions;
        }
    }
    if let Some(catalog) = &state.work_guides {
        let registered = catalog
            .guides
            .values()
            .map(file_reference)
            .collect::<Result<BTreeSet<_>>>()?;
        let mut unavailable = BTreeMap::new();
        for reference in bodies {
            if registered.contains(reference) {
                continue;
            }
            let file = &record(state, reference)?.record;
            if file.scope != f::Scope::Assets
                || file.author != AUTHOR
                || !file.path.starts_with(DIRECTORY)
            {
                continue;
            }
            let current_revision = registered.iter().find(|current| {
                current.source == reference.source && current.revision == reference.revision
            });
            if current_revision.is_some_and(|current| bodies.contains(current)) {
                continue; // Another supplied route already includes the complete guide.
            }
            unavailable.insert(
                file.path.clone(),
                if current_revision.is_some() {
                    "current_guide_requires_complete_utf8_body"
                } else {
                    "guide_revision_is_not_currently_registered"
                },
            );
        }
        if !unavailable.is_empty() {
            value["guide_supply"] = json!({
                "unavailable_supplied_guides":unavailable.into_iter()
                    .map(|(path, reason)|json!({"path":path,"reason":reason})).collect::<Vec<_>>(),
                "truncated":false,
                "rule":"These supplied guide bodies do not match complete current registered references, so they select no detailed native schema. Find a current guide when its definition or explanation is useful. All implemented native commands remain callable under the admitted contract; this diagnostic changes no selection or execution record."});
        }
    }
    Ok(value)
}

pub(super) fn exposed(
    state: &State,
    _actor: &str,
    bodies: &BTreeSet<f::Reference>,
    _now: i64,
) -> Result<Option<WorkInterface>> {
    let mut commands = BTreeSet::new();
    if let Some(catalog) = &state.work_guides {
        for (command, reference) in &catalog.guides {
            if bodies.contains(&file_reference(reference)?) {
                commands.insert(command.clone());
            }
        }
    }
    Ok(Some(WorkInterface::selected(commands)?))
}
