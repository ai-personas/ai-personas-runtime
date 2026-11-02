//! Lossless sharing of exact work text, qualified bodies and reference metadata.
//! Only known host-rendered fields participate. Roles, source attribution,
//! restrictions, qualification edges and observation identities stay in place.
use super::*;
use serde::ser::SerializeMap;
use std::borrow::Cow;
mod control_text;
mod references;

// Present the persona and current work before the detailed interface contract.
// This is ordering only: no summary, inferred task plan, or omitted field.
const CONTROL_ORDER: &[&str] = &[
    "profile", "actor", "persona_label", "lifecycle_role",
    "current_character", "character_development", "seed", "transient_affect",
    "work_id", "work_phase", "work_version", "task_amendment", "mandate",
    "original_need", "requested_result_context", "requester", "own_acceptance", "arrangements", "own_pending_offers",
    "last_attributed_work_judgment", "previous_authored_continuation", "context_accounting",
    "actual_observations", "collaborators",
    "invocation_id", "epoch", "owner_epoch", "context_at_unix",
];

struct OrderedControl<'a>(&'a serde_json::Map<String, Value>);

impl Serialize for OrderedControl<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut fields: Vec<_> = self.0.iter().collect();
        fields.sort_by_key(|(key, _)| (
            CONTROL_ORDER.iter().position(|name| *name == key.as_str()).unwrap_or(CONTROL_ORDER.len()),
            key.as_str(),
        ));
        let mut map = serializer.serialize_map(Some(fields.len()))?;
        for (key, value) in fields {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub(super) fn render_control(control: &Value) -> Result<String> {
    let object = control.as_object().ok_or_else(|| invalid("request control must be an object"))?;
    Ok(serde_json::to_string(&OrderedControl(object))?)
}

pub(super) const ENCODING: &str = "shared-qualified-text-v1";
const WORK_ENCODING: &str = "shared-request-text-v2";
const REFERENCE_ENCODING: &str = "shared-request-context-v3";
const POOL: &str = "shared_texts";
const RULE: &str = "For each qualified entry with text_ref, its complete text is shared_texts.bodies[text_ref] in this QUALIFIED_CONTEXT. That text is supplied here, not a retrieval request. Keep that entry's exact reference, roles, attribution, policies and required qualifications; sharing text merges none of those records. Other entries supply text inline.";
struct Counter(usize);
impl std::io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("expanded request context overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn size(value: &Value) -> Result<usize> {
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}
fn sections_size(control: &Value, qualified: &Value) -> Result<usize> {
    size(control)?
        .checked_add(size(qualified)?)
        .ok_or_else(|| held("expanded request context size overflow"))
}
fn identity(reference: &Value, text: &str) -> Result<String> {
    digest(&(ENCODING, reference, text))
}

// Enumerate only host-rendered qualified bundles, never arbitrary text-like
// fields in authored documents, messages, tool payloads or Decision JSON.
fn observation_bundle_paths(observation: &Value, prefix: &[String], paths: &mut Vec<Vec<String>>) {
    if matches!(
        observation["kind"].as_str(),
        Some("qualified_read" | "selection_repair_read" | "local_dependency_event")
    ) && observation["payload"]["result"]["profile"] == "qualified-exact-context-json-v1"
    {
        let mut path = prefix.to_vec();
        path.extend(["payload".into(), "result".into()]);
        paths.push(path);
    }
    if let Some(bundles) = observation["current_required_qualifications"].as_array() {
        for (index, bundle) in bundles.iter().enumerate() {
            if bundle["profile"] == "qualified-exact-context-json-v1" {
                let mut path = prefix.to_vec();
                path.extend(["current_required_qualifications".into(), index.to_string()]);
                paths.push(path);
            }
        }
    }
}

// Current operative supply uses only these paths. Encoding historical bundles
// below must not expose a capability, reactivate an event or add current roots.
fn bundle_paths(control: &Value) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    if control["provider_continuation"]["current_required_qualifications"]["profile"]
        == "qualified-exact-context-json-v1" {
        paths.push(vec!["provider_continuation".into(), "current_required_qualifications".into()]);
    }
    if let Some(observations) = control["actual_observations"].as_array() {
        for (index, observation) in observations.iter().enumerate() {
            observation_bundle_paths(observation, &["actual_observations".into(), index.to_string()], &mut paths);
        }
    }
    paths
}

fn encoding_bundle_paths(control: &Value) -> Vec<Vec<String>> {
    let mut paths = bundle_paths(control);
    let mut pending: Vec<_> = control["actual_observations"].as_array().into_iter()
        .flatten().enumerate().map(|(index, observation)|
            (vec!["actual_observations".into(), index.to_string()], observation)).collect();
    while let Some((mut prefix, observation)) = pending.pop() {
        let payload = &observation["payload"];
        let record = &payload["record"];
        if observation["kind"] != "history_record_read"
            || payload["profile"] != history::records::INTERFACE
            || payload["body_supplied"] != true
            || record["historical_only"] != true
            || record["operative_evidence_or_event"] != false
            || record["reference"] != payload["reference"]
        {
            continue;
        }
        let historical = &record["historical_record"];
        prefix.extend(["payload".into(), "record".into(), "historical_record".into()]);
        match record["reference"]["kind"].as_str() {
            Some("observation") if record["reference"]["observation_id"].is_string()
                && historical["id"] == record["reference"]["observation_id"] =>
            {
                observation_bundle_paths(historical, &prefix, &mut paths);
                pending.push((prefix, historical));
            }
            Some("adopted_decision") if record["reference"]["invocation_id"].is_string()
                && historical["invocation"] == record["reference"]["invocation_id"]
                && historical["current_accompaniment"]["profile"] == "qualified-exact-context-json-v1" =>
            {
                prefix.push("current_accompaniment".into());
                paths.push(prefix);
            }
            _ => (),
        }
    }
    paths
}
fn pointer(path: &[String]) -> String {
    format!("/{}", path.join("/"))
}
fn entries(bundle: &Value) -> Result<&Vec<Value>> {
    require(
        bundle["profile"] == "qualified-exact-context-json-v1",
        "unknown qualified context profile",
    )?;
    bundle["entries"]
        .as_array()
        .ok_or_else(|| held("qualified entries unavailable"))
}
fn entries_mut(bundle: &mut Value) -> Result<&mut Vec<Value>> {
    require(
        bundle["profile"] == "qualified-exact-context-json-v1",
        "unknown qualified context profile",
    )?;
    bundle["entries"]
        .as_array_mut()
        .ok_or_else(|| held("qualified entries unavailable"))
}

struct Group<'a> {
    reference: &'a Value,
    text: &'a str,
    count: usize,
}
fn collect<'a>(bundle: &'a Value, groups: &mut BTreeMap<String, Group<'a>>) -> Result<()> {
    for entry in entries(bundle)? {
        require(
            entry.get("text_ref").is_none(),
            "qualified text is already encoded",
        )?;
        let text = entry["text"]
            .as_str()
            .ok_or_else(|| held("qualified text unavailable"))?;
        if text.len() < 256 {
            continue;
        }
        let reference = &entry["reference"];
        let key = identity(reference, text)?;
        if let Some(group) = groups.get_mut(&key) {
            require(
                group.reference == reference && group.text == text,
                "qualified text identity conflict",
            )?;
            group.count += 1;
        } else {
            groups.insert(
                key,
                Group {
                    reference,
                    text,
                    count: 1,
                },
            );
        }
    }
    Ok(())
}
fn replace(bundle: &mut Value, bodies: &serde_json::Map<String, Value>) -> Result<()> {
    for entry in entries_mut(bundle)? {
        let text = entry["text"]
            .as_str()
            .ok_or_else(|| held("qualified text unavailable"))?;
        if text.len() < 256 {
            continue;
        }
        let key = identity(&entry["reference"], text)?;
        if bodies.contains_key(&key) {
            let object = entry
                .as_object_mut()
                .ok_or_else(|| held("qualified entry unavailable"))?;
            object.remove("text");
            object.insert("text_ref".into(), Value::String(key));
        }
    }
    Ok(())
}

pub(super) struct Encoded<'a> {
    pub control: Cow<'a, Value>,
    pub qualified: Cow<'a, Value>,
    pub encoding: Option<String>,
}
// Inspect the resolved current context, not the file's container bytes. A
// nonempty JSON document or file can select an empty string/whitespace range.
// Such a projection cannot carry characteristic prose. Supporting assets may
// legitimately be empty; observation/history reads remain diagnostic, and the
// decoder must still reproduce earlier inputs without rewriting their evidence.
// This is a nonblank-body invariant, not a semantic character/quality evaluator.
fn require_characteristic_text(qualified: &Value) -> Result<()> {
    for entry in entries(qualified)? {
        if entry["scope"] != "fragments" {
            continue;
        }
        let text = entry["text"]
            .as_str()
            .ok_or_else(|| held("characteristic context text unavailable"))?;
        require(
            !text.trim().is_empty(),
            &format!(
                "characteristic context is blank at exact reference {}; choose a nonblank UTF-8 or JSON-string projection with an exact range when needed",
                entry["reference"]
            ),
        )?;
    }
    Ok(())
}

pub(super) fn encode<'a>(control: &'a Value, qualified: &'a Value) -> Result<Encoded<'a>> {
    require_characteristic_text(qualified)?;
    sections_size(control, qualified)?;
    let mut annotated = control.clone();
    annotated["context_accounting"] = context_accounting(control, qualified)?;
    let compact = control_text::encode(&annotated)?;
    let encoded = encode_qualified(compact.as_ref().unwrap_or(&annotated), qualified)?;
    let encoding = if compact.is_some() { Some(WORK_ENCODING.into()) } else { encoded.encoding };
    let mut control = encoded.control.into_owned();
    let mut qualified = encoded.qualified.into_owned();
    let reference_sharing = references::encode(&mut control, &mut qualified)?;
    Ok(Encoded {
        control: Cow::Owned(control),
        qualified: Cow::Owned(qualified),
        encoding: if reference_sharing { Some(REFERENCE_ENCODING.into()) } else { encoding },
    })
}

fn qualified_accounting(bundles: &[&Value]) -> Result<Value> {
    let mut entry_count = 0usize;
    let mut root_entries = 0usize;
    let mut text_bytes = 0usize;
    let mut metadata_bytes = 0usize;
    let mut qualification_edges = 0usize;
    for bundle in bundles {
        for entry in entries(bundle)? {
            entry_count += 1;
            let mut metadata = entry.clone();
            let text = metadata.as_object_mut().ok_or_else(|| held("qualified entry unavailable"))?
                .remove("text").ok_or_else(|| held("qualified entry text unavailable"))?;
            text_bytes += text.as_str().ok_or_else(|| held("qualified entry text invalid"))?.len();
            metadata_bytes += size(&metadata)?;
            let roles = entry["roles"].as_array().ok_or_else(|| held("qualified roles unavailable"))?;
            root_entries += usize::from(roles.iter().any(|role| role["role"] == "root"));
            qualification_edges += roles.iter().filter(|role| role["role"] == "qualification").count();
        }
    }
    Ok(json!({
        "bundles":bundles.len(),
        "entries":entry_count,
        "root_entries":root_entries,
        "qualification_edges":qualification_edges,
        "text_utf8_bytes":text_bytes,
        "entry_metadata_json_bytes":metadata_bytes,
    }))
}

/// Attribute already supplied entries to their existing assembly bundles. Role
/// membership includes the complete qualification closure, including cycles;
/// counting each entry once avoids charging it once per incoming graph edge.
/// This performs no discovery, body load, semantic ranking or selection.
fn qualified_root_accounting(qualified: &Value) -> Result<Vec<Value>> {
    type Key = (String, u64);
    #[derive(Default)]
    struct Root {
        entry: Option<usize>,
        members: BTreeSet<usize>,
    }
    let mut roots = BTreeMap::<Key, Root>::new();
    let mut costs = Vec::new();
    for (index, entry) in entries(qualified)?.iter().enumerate() {
        let mut metadata = entry.clone();
        let text = metadata.as_object_mut().ok_or_else(|| held("qualified entry unavailable"))?
            .remove("text").ok_or_else(|| held("qualified entry text unavailable"))?;
        let text_bytes = text.as_str().ok_or_else(|| held("qualified entry text invalid"))?.len();
        let mut memberships = BTreeSet::new();
        for role in entry["roles"].as_array().ok_or_else(|| held("qualified roles unavailable"))? {
            let bundle = &role["bundle"];
            let kind = bundle["kind"].as_str().ok_or_else(|| held("qualified bundle kind unavailable"))?;
            let position = bundle["index"].as_u64().ok_or_else(|| held("qualified bundle index unavailable"))?;
            let key = (kind.to_owned(), position);
            memberships.insert(key.clone());
            let root = roots.entry(key).or_default();
            root.members.insert(index);
            match role["role"].as_str() {
                Some("root") => {
                    require(root.entry.is_none_or(|prior| prior == index), "qualified bundle has conflicting roots")?;
                    root.entry = Some(index);
                }
                Some("qualification") => {},
                _ => return Err(held("qualified entry role unavailable")),
            }
        }
        costs.push((text_bytes, size(&metadata)?, memberships.len()));
    }
    roots.into_iter().map(|((kind, index), root)| {
        let entry = root.entry.ok_or_else(|| held("qualified bundle root unavailable"))?;
        let mut text = 0usize;
        let mut metadata = 0usize;
        let mut exclusive_text = 0usize;
        let mut shared_entries = 0usize;
        for member in &root.members {
            let (body_bytes, metadata_bytes, memberships) = costs[*member];
            text = text.checked_add(body_bytes).ok_or_else(|| held("qualified root size overflow"))?;
            metadata = metadata.checked_add(metadata_bytes).ok_or_else(|| held("qualified root size overflow"))?;
            if memberships == 1 {
                exclusive_text = exclusive_text.checked_add(body_bytes)
                    .ok_or_else(|| held("qualified root size overflow"))?;
            } else {
                shared_entries += 1;
            }
        }
        Ok(json!({
            "bundle":{"kind":kind,"index":index},
            "root_entry_index":entry,
            "entries":root.members.len(),
            "root_text_utf8_bytes":costs[entry].0,
            "qualification_text_utf8_bytes":text - costs[entry].0,
            "text_utf8_bytes":text,
            "supplied_entry_metadata_json_bytes":metadata,
            "shared_entries":shared_entries,
            "exclusive_text_utf8_bytes":exclusive_text,
        }))
    }).collect()
}

/// Count every host-rendered qualified bundle, including observation and
/// historical bodies. A cleared optional selection can still accompany costly
/// required qualifications. Keep current and historical supply distinct.
fn context_accounting(control: &Value, qualified: &Value) -> Result<Value> {
    let current_paths: BTreeSet<_> = bundle_paths(control).into_iter().collect();
    let paths = encoding_bundle_paths(control);
    let mut current = Vec::new();
    let mut historical = Vec::new();
    let mut continuation = Vec::new();
    for path in &paths {
        let bundle = control.pointer(&pointer(path))
            .ok_or_else(|| held("qualified observation bundle unavailable"))?;
        if path.first().is_some_and(|part| part == "provider_continuation") { continuation.push(bundle); }
        else if current_paths.contains(path) { current.push(bundle); }
        else { historical.push(bundle); }
    }
    Ok(json!({
        "profile":"actual-request-context-cost-v3",
        "selected_asset_count":control["selection_state"]["assets"].as_array().map_or(0, Vec::len),
        "selected_and_required_context":qualified_accounting(&[qualified])?,
        "selected_and_required_roots":qualified_root_accounting(qualified)?,
        "root_cost_meaning":"Each row identifies a supplied root by its zero-based QUALIFIED_CONTEXT.entries index and existing bundle identity. It counts that root and its complete supplied qualification closure, each exact entry once even when graph paths repeat. Shared entries belong to more than one root in this context; their costs overlap across rows. Exclusive text belongs to one root here, but may still occur in observations or provider history. Metadata counts include each entry's complete current roles before sharing. These are observed byte counts, not relevance scores, predicted token savings or permission to omit mandatory qualifications. Use the root entry and its exact input-file reference to inspect or revise your own selection and supporting files when useful.",
        "current_observation_context":qualified_accounting(&current)?,
        "historical_observation_context":qualified_accounting(&historical)?,
        "provider_continuation_qualifications":qualified_accounting(&continuation)?,
        "observations_json_bytes":size(&control["actual_observations"])? ,
        "control_json_bytes_before_accounting_and_sharing":size(control)?,
        "qualified_json_bytes_before_sharing":size(qualified)?,
        "meaning":"Actual serialized sizes before lossless text and reference sharing; not token counts or limits. Bundle counts include repeated occurrences, not unique sources. Current and historical observation bundles are measured separately from selected and required context; historical text is not current authority. A cleared optional selection can still have large mandatory observation qualifications. Observations include their measured bundles, so these byte fields overlap. Repeated context adds cost even when no new task result is produced. Choose useful exact ranges, compact working notes and ordinary citations; retain genuine mandatory caveats. Model-call receipts report actual provider tokens. No selection is changed by these measurements."
    }))
}

fn encode_qualified<'a>(control: &'a Value, qualified: &'a Value) -> Result<Encoded<'a>> {
    require(
        qualified.get(POOL).is_none(),
        "request context is already encoded",
    )?;
    let original_bytes = sections_size(control, qualified)?;
    let paths = encoding_bundle_paths(control);
    let mut groups = BTreeMap::new();
    collect(qualified, &mut groups)?;
    for path in &paths {
        collect(
            control
                .pointer(&pointer(path))
                .ok_or_else(|| held("qualified bundle unavailable"))?,
            &mut groups,
        )?;
    }
    let mut bodies = serde_json::Map::new();
    for (key, group) in groups {
        // Each replacement adds an explicit hash reference and the pool retains
        // one complete JSON string. Avoid individually unprofitable entries;
        // the final whole-section comparison also accounts for the pool header.
        let text_bytes = serde_json::to_vec(group.text)?.len();
        if group.count > 1 && (text_bytes + 7) * group.count > text_bytes + 68 + 77 * group.count {
            bodies.insert(key, Value::String(group.text.into()));
        }
    }
    if !bodies.is_empty() {
        let mut compact_control = control.clone();
        let mut compact_qualified = qualified.clone();
        replace(&mut compact_qualified, &bodies)?;
        for path in &paths {
            replace(
                compact_control
                    .pointer_mut(&pointer(path))
                    .ok_or_else(|| held("qualified bundle unavailable"))?,
                &bodies,
            )?;
        }
        compact_qualified[POOL] = json!({"profile":ENCODING,"rule":RULE,"bodies":bodies});
        if sections_size(&compact_control, &compact_qualified)? < original_bytes {
            return Ok(Encoded {
                control: Cow::Owned(compact_control),
                qualified: Cow::Owned(compact_qualified),
                encoding: Some(ENCODING.into()),
            });
        }
    }
    Ok(Encoded {
        control: Cow::Borrowed(control),
        qualified: Cow::Borrowed(qualified),
        encoding: None,
    })
}

#[derive(Default, Serialize)]
pub(super) struct Accounting {
    pub shared_bodies: usize,
    pub body_references: usize,
    pub shared_work_texts: usize,
    pub work_text_references: usize,
    pub shared_qualification_references: usize,
    pub qualification_reference_occurrences: usize,
    pub encoded_sections_utf8_bytes: usize,
    pub expanded_sections_utf8_bytes: usize,
    pub serialized_bytes_saved: usize,
}

fn expansion(
    bundle: &Value,
    bodies: &serde_json::Map<String, Value>,
    used: &mut BTreeMap<String, usize>,
    expanded: &mut usize,
) -> Result<()> {
    for entry in entries(bundle)? {
        let Some(reference) = entry.get("text_ref") else {
            continue;
        };
        require(
            entry.get("text").is_none(),
            "qualified text has conflicting encodings",
        )?;
        let key = reference
            .as_str()
            .ok_or_else(|| held("invalid qualified text reference"))?;
        let text = bodies
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| held("shared qualified text unavailable"))?;
        require(
            identity(&entry["reference"], text)? == key,
            "shared qualified text binding mismatch",
        )?;
        let added = serde_json::to_vec(text)?.len() + 7;
        let removed = serde_json::to_vec(reference)?.len() + 11;
        *expanded = expanded
            .checked_add(added)
            .and_then(|bytes| bytes.checked_sub(removed))
            .ok_or_else(|| held("expanded request context size overflow"))?;
        *used.entry(key.into()).or_default() += 1;
    }
    Ok(())
}
fn restore(bundle: &mut Value, bodies: &serde_json::Map<String, Value>) -> Result<()> {
    for entry in entries_mut(bundle)? {
        if let Some(key) = entry.get("text_ref").and_then(Value::as_str) {
            let text = bodies
                .get(key)
                .ok_or_else(|| held("shared qualified text unavailable"))?
                .clone();
            let object = entry
                .as_object_mut()
                .ok_or_else(|| held("qualified entry unavailable"))?;
            object.remove("text_ref");
            object.insert("text".into(), text);
        }
    }
    Ok(())
}

pub(super) struct Decoded<'a> {
    pub instructions: &'a str,
    pub control_bytes: usize,
    pub qualified_bytes: usize,
    pub control: Value,
    pub qualified: Value,
    pub accounting: Accounting,
}
pub(super) fn decode(prepared: &Prepared) -> Result<Decoded<'_>> {
    require(
        s::sha256_bytes(prepared.prompt.as_bytes()) == prepared.prompt_sha256,
        "retained prompt hash mismatch",
    )?;
    let (prefix, qualified_text) = prepared
        .prompt
        .rsplit_once("\nQUALIFIED_CONTEXT\n")
        .ok_or_else(|| held("qualified input evidence unavailable"))?;
    let (instructions, control_text) = prefix
        .rsplit_once("\nCONTROL\n")
        .ok_or_else(|| held("control input evidence unavailable"))?;
    let mut control: Value = serde_json::from_str(control_text)?;
    let mut qualified: Value = serde_json::from_str(qualified_text)?;
    let encoded_bytes = sections_size(&control, &qualified)?;
    let reference_sharing = references::decode(&mut control, &mut qualified)?;
    let reference_encoding = prepared.prompt_encoding.as_deref() == Some(REFERENCE_ENCODING);
    require(reference_sharing.is_some() == reference_encoding,
        "retained qualification reference encoding mismatch")?;
    let work_sharing = control_text::decode(&mut control)?;
    require(
        reference_encoding || work_sharing.is_some() == (prepared.prompt_encoding.as_deref() == Some(WORK_ENCODING)),
        "retained work text encoding mismatch",
    )?;
    let paths = encoding_bundle_paths(&control);
    let mut accounting = Accounting {
        encoded_sections_utf8_bytes: encoded_bytes,
        expanded_sections_utf8_bytes: encoded_bytes,
        ..Accounting::default()
    };
    let pool = qualified
        .as_object_mut()
        .ok_or_else(|| held("qualified context unavailable"))?
        .remove(POOL);
    match pool {
        None => require(
            prepared.prompt_encoding.is_none() || work_sharing.is_some() || reference_encoding,
            "retained prompt encoding mismatch",
        )?,
        Some(pool) => {
            require(
                (prepared.prompt_encoding.as_deref() == Some(ENCODING) || work_sharing.is_some() || reference_encoding)
                    && pool["profile"] == ENCODING
                    && pool["rule"] == RULE
                    && pool.as_object().is_some_and(|object| object.len() == 3),
                "unknown shared qualified text encoding",
            )?;
            let bodies = pool["bodies"]
                .as_object()
                .ok_or_else(|| held("shared text pool unavailable"))?;
            require(
                !bodies.is_empty() && bodies.values().all(Value::is_string),
                "invalid shared text pool",
            )?;
            let mut expanded = sections_size(&control, &qualified)?;
            let mut used = BTreeMap::new();
            expansion(&qualified, bodies, &mut used, &mut expanded)?;
            for path in &paths {
                expansion(
                    control
                        .pointer(&pointer(path))
                        .ok_or_else(|| held("qualified bundle unavailable"))?,
                    bodies,
                    &mut used,
                    &mut expanded,
                )?;
            }
            require(
                used.len() == bodies.len() && used.values().all(|count| *count > 1),
                "shared qualified text pool has unused or unshared entries",
            )?;
            restore(&mut qualified, bodies)?;
            for path in &paths {
                restore(
                    control
                        .pointer_mut(&pointer(path))
                        .ok_or_else(|| held("qualified bundle unavailable"))?,
                    bodies,
                )?;
            }
            require(
                sections_size(&control, &qualified)? == expanded && expanded > encoded_bytes,
                "shared qualified text accounting mismatch",
            )?;
            accounting = Accounting {
                shared_bodies: bodies.len(),
                body_references: used.values().sum(),
                encoded_sections_utf8_bytes: encoded_bytes,
                expanded_sections_utf8_bytes: expanded,
                serialized_bytes_saved: expanded - encoded_bytes,
                ..Accounting::default()
            };
        }
    }
    if let Some(work) = work_sharing {
        accounting.shared_work_texts = work.shared_texts;
        accounting.work_text_references = work.references;
    }
    if let Some(references) = reference_sharing {
        accounting.shared_qualification_references = references.shared;
        accounting.qualification_reference_occurrences = references.occurrences;
    }
    if accounting.shared_work_texts > 0 || accounting.shared_qualification_references > 0 {
        accounting.expanded_sections_utf8_bytes = sections_size(&control, &qualified)?;
        accounting.serialized_bytes_saved = accounting.expanded_sections_utf8_bytes
            .checked_sub(encoded_bytes)
            .ok_or_else(|| held("work text sharing accounting mismatch"))?;
    }
    // A missing pool cannot turn a dangling pointer into a supplied-body claim.
    for bundle in std::iter::once(&qualified)
        .chain(
            paths
                .iter()
                .filter_map(|path| control.pointer(&pointer(path))),
        )
        .filter(|bundle| bundle["profile"] == "qualified-exact-context-json-v1")
    {
        require(
            entries(bundle)?
                .iter()
                .all(|entry| entry.get("text_ref").is_none()),
            "unresolved qualified text reference",
        )?;
    }
    Ok(Decoded {
        instructions,
        control_bytes: control_text.len(),
        qualified_bytes: qualified_text.len(),
        control,
        qualified,
        accounting,
    })
}

/// Exact file bodies in the sealed host-rendered request. Permission ancestry,
/// discovery metadata, authored previews and arbitrary nested history are not
/// body supply. Sharing changes storage of text, never its source roles.
pub(super) fn supplied_file_references(prepared: &Prepared) -> Result<Vec<f::Reference>> {
    let decoded = decode(prepared)?;
    let mut references = BTreeSet::new();
    let paths = bundle_paths(&decoded.control);
    for bundle in std::iter::once(&decoded.qualified).chain(
        paths.iter().filter_map(|path| decoded.control.pointer(&pointer(path))),
    ) {
        for entry in entries(bundle)? {
            require(entry["text"].is_string(), "qualified body text unavailable")?;
            let reference: p::ExactRef = serde_json::from_value(entry["reference"].clone())?;
            let reference = file_reference(&reference)?;
            require(
                prepared_exposure(prepared)?.sources.contains(&reference),
                "qualified body absent from admitted source restrictions",
            )?;
            references.insert(reference);
        }
    }
    Ok(references.into_iter().collect())
}

/// Source bodies actually present in native qualified bundles, including
/// historical accompaniment. This does not turn history into a current event.
/// Permission ancestry and lookup-only references are deliberately excluded.
pub(super) fn historical_qualification_sources(prepared: &Prepared) -> Result<Vec<f::Reference>> {
    let decoded = decode(prepared)?;
    let mut references = BTreeSet::new();
    let paths = encoding_bundle_paths(&decoded.control);
    for bundle in std::iter::once(&decoded.qualified).chain(
        paths.iter().filter_map(|path| decoded.control.pointer(&pointer(path))),
    ) {
        for entry in entries(bundle)? {
            require(entry["text"].is_string(), "qualified historical input body unavailable")?;
            let reference: p::ExactRef = serde_json::from_value(entry["reference"].clone())?;
            let reference = file_reference(&reference)?;
            require(prepared_exposure(prepared)?.sources.contains(&reference),
                "historical input body absent from admitted provenance")?;
            references.insert(reference);
        }
    }
    Ok(references.into_iter().collect())
}
