//! Primary-selected internal work definitions, discovered as ordinary assets.
//! A registered exact guide routes to a native definition. Authored file text
//! alone cannot register a command, grant authority, or establish execution.
use super::*;

pub(crate) const DIRECTORY: &str = "runtime/work-commands/";
const AUTHOR: &str = "ai-personas-runtime";
const MAX_COMMANDS: usize = 64;

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
        supported && !catalog.guides.is_empty() && catalog.guides.len() <= MAX_COMMANDS,
        "native work guide catalog unsupported",
    )
}

fn variants(schema: &Value) -> Result<&Vec<Value>> {
    let variants = schema["oneOf"]
        .as_array()
        .ok_or_else(|| invalid("native work schema is not a tagged union"))?;
    require(
        !variants.is_empty() && variants.len() <= MAX_COMMANDS,
        "native work definition count exceeds its bound",
    )?;
    Ok(variants)
}

fn kind(variant: &Value) -> Result<&str> {
    variant["properties"]["kind"]["const"]
        .as_str()
        .ok_or_else(|| invalid("native work command identity unavailable"))
}

/// Keep only named variants and the definitions reachable from those variants.
/// Neither names nor a valid schema bypass the sealed admitted interface.
pub(super) fn selected_schema(commands: &BTreeSet<String>) -> Result<Value> {
    require(
        !commands.is_empty() && commands.len() <= MAX_COMMANDS,
        "empty or oversized native work selection",
    )?;
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
        _ => "Use the current exact native input schema and the admitted work's authority, evidence and resource limits.",
    }
}

fn related(command: &str) -> &'static [&'static str] {
    match command {
        "ask_question" => &["answer_question", "conclude_question"],
        "answer_question" => &["conclude_question"],
        "conclude_question" => &["ask_question"],
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
    Ok(format!("# Native internal work command: {command}\n\n{description}\n\nInput field names for discovery: {}.\n\nRelated guide filenames in this directory: {links}. Use ordinary file discovery to locate relevant prerequisites and their exact current revisions. These are navigation cues, not required selections or an assigned workflow. If a needed command is absent from the current interface, discover its guide before treating the missing definition as an unavailable capability.\n\n{command_rules}This runtime-authored supporting guide is not a persona's character, experience, grant or execution record. Read or select its complete exact UTF-8 revision through the ordinary file interface. Only an actually supplied registered guide exposes its current native input schema in CONTROL.work_capability on a fresh admission. Copy that admitted schema and current work references; the guide alone cannot authorize a command. Existing responsibility, consent, source, complete-response and resource checks still apply. No external software is invoked by this internal command.\n", names.join(", ")))
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
            require(
                body.len() <= 8192,
                "native work guide exceeds its byte bound",
            )?;
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
    Ok(json!({"selective_work_commands":true,"directory":directory,
        "guide_count":records.len(),
        "guide_body_bytes":records.iter().map(|(_, record)|record.bytes.len()).sum::<usize>(),
        "author":AUTHOR,"owner":owner,"policy_id":policy_id,
        "inference_performed":false,"resources_added":false,
        "meaning":"Native supporting guides consume existing storage. Their bodies and command schemas enter model context only through actually supplied qualified selections or reads. A guide supplies no new authority."}))
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
                    existing.record == file
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
                    record: file,
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
                        record: file,
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
        "rule":"Discover native work guides in this current directory with the ordinary bounded file list/search interface. Related guide filenames provide further search cues. When a needed command is absent from the supplied subset, locate its guide before concluding that the capability is unavailable; definition selection and work authority are separate. Read or select exact complete UTF-8 guide revisions under next_assets. Their currently supported input definitions are exposed only when those bodies actually reach this admission. Metadata, names, lineage, future selections and old receipts alone expose no command. You choose searches and relevant guides; no work-command catalogue is loaded automatically. Primitive file/communication operations remain in APPLICATION_DECISION_SCHEMA. All existing authority and resource checks still apply."
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
            "binding_revision":interface.binding_revision,"inputs_schema":interface.inputs_schema}),
        None => json!({"enabled":false,
            "reason":"No native work command is exposed in this admission. Discover relevant guides in the exact current CONTROL.work_guide_discovery.directory and read or select their complete UTF-8 revisions. Older profile revisions remain historical resources; merely preserving them or retrying cannot expose their commands. Missing definitions alone do not establish unavailable capabilities. Existing work-command grants and authority are still required."}),
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
            let truncated = unavailable.len() > MAX_COMMANDS;
            value["guide_supply"] = json!({
                "unavailable_supplied_guides":unavailable.into_iter().take(MAX_COMMANDS)
                    .map(|(path, reason)|json!({"path":path,"reason":reason})).collect::<Vec<_>>(),
                "truncated":truncated,
                "rule":"These actually supplied guide bodies do not match complete current registered guide references. Find relevant guides in the exact CONTROL.work_guide_discovery.directory and select or read their complete UTF-8 revisions. Repeating an admission with the same nonregistered or partial bodies cannot supply their command definitions. This diagnostic changes no selection or authority."});
        }
    }
    Ok(value)
}

pub(super) fn exposed(
    state: &State,
    actor: &str,
    bodies: &BTreeSet<f::Reference>,
    now: i64,
) -> Result<Option<WorkInterface>> {
    let Some(catalog) = &state.work_guides else {
        return Ok(Some(WorkInterface::current()?));
    };
    validate_catalog(catalog)?;
    let persona = state
        .personas
        .get(actor)
        .ok_or_else(|| held("work guide reader unavailable"))?;
    if !persona.seed.grants.work_commands {
        return Ok(None);
    }
    let audience = admission(persona, now);
    let schema = work_schema();
    let known = variants(&schema)?
        .iter()
        .map(kind)
        .collect::<Result<BTreeSet<_>>>()?;
    let mut commands = BTreeSet::new();
    for (command, guide) in &catalog.guides {
        let reference = file_reference(guide)?;
        if known.contains(command.as_str())
            && bodies.contains(&reference)
            && reference.representation == f::Representation::Utf8
            && reference.start.is_none()
            && reference.end.is_none()
            && state
                .sources
                .get(&reference.source)
                .is_some_and(|source| source.protected && source.owner == catalog.owner)
            && lineage_permitted(state, std::slice::from_ref(&reference), &audience)
        {
            commands.insert(command.clone());
        }
    }
    if commands.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WorkInterface::selected(commands)?))
    }
}
