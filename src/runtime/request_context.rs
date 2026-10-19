//! Lossless sharing of repeated exact qualified text within one model request.
//! Only host-rendered file entries participate. Roles, source attribution,
//! restrictions, qualification edges and observation identities stay in place.
use super::*;
use std::borrow::Cow;

pub(super) const ENCODING: &str = "shared-qualified-text-v1";
const POOL: &str = "shared_texts";
const RULE: &str = "For each qualified entry with text_ref, its complete text is shared_texts.bodies[text_ref] in this QUALIFIED_CONTEXT. That text is supplied here, not a retrieval request. Keep that entry's exact reference, roles, attribution, policies and required qualifications; sharing text merges none of those records. Other entries supply text inline.";
// The encoded request remains subject to its normal much smaller allowance.
// Also bound the host's expanded representation before accepting or decoding it.
const EXPANDED_LIMIT: usize = 64 * 1024 * 1024;

struct Counter(usize);
impl std::io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= EXPANDED_LIMIT)
            .ok_or_else(|| std::io::Error::other("expanded request context limit"))?;
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
        .filter(|size| *size <= EXPANDED_LIMIT)
        .ok_or_else(|| held("expanded request context exceeds its bound"))
}
fn identity(reference: &Value, text: &str) -> Result<String> {
    digest(&(ENCODING, reference, text))
}

// Never recursively search arbitrary payloads, authored documents, messages or
// historical records for text-like fields. Only these current qualified bundles
// have the file-entry contract needed for a shared-body supply claim.
fn bundle_paths(control: &Value) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    if let Some(observations) = control["actual_observations"].as_array() {
        for (index, observation) in observations.iter().enumerate() {
            let prefix = vec!["actual_observations".into(), index.to_string()];
            if matches!(
                observation["kind"].as_str(),
                Some("qualified_read" | "selection_repair_read" | "local_dependency_event")
            ) && observation["payload"]["result"]["profile"] == "qualified-exact-context-json-v1"
            {
                let mut path = prefix.clone();
                path.extend(["payload".into(), "result".into()]);
                paths.push(path);
            }
            if let Some(bundles) = observation["current_required_qualifications"].as_array() {
                for (bundle_index, bundle) in bundles.iter().enumerate() {
                    if bundle["profile"] == "qualified-exact-context-json-v1" {
                        let mut path = prefix.clone();
                        path.extend([
                            "current_required_qualifications".into(),
                            bundle_index.to_string(),
                        ]);
                        paths.push(path);
                    }
                }
            }
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
pub(super) fn encode<'a>(control: &'a Value, qualified: &'a Value) -> Result<Encoded<'a>> {
    require(
        qualified.get(POOL).is_none(),
        "request context is already encoded",
    )?;
    let original_bytes = sections_size(control, qualified)?;
    let paths = bundle_paths(control);
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
            .filter(|bytes| *bytes <= EXPANDED_LIMIT)
            .ok_or_else(|| held("expanded request context exceeds its bound"))?;
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
        prepared.prompt.len() <= INPUT_LIMIT,
        "retained request exceeds its input bound",
    )?;
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
    let paths = bundle_paths(&control);
    let encoded_bytes = sections_size(&control, &qualified)?;
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
            prepared.prompt_encoding.is_none(),
            "retained prompt encoding mismatch",
        )?,
        Some(pool) => {
            require(
                prepared.prompt_encoding.as_deref() == Some(ENCODING)
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
            };
        }
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
