//! Share identical text only in known host-rendered work fields. Keep the
//! original request inline and preserve every role, record and authority edge.
use super::*;

const FIELD: &str = "control_text_sharing";
const PROFILE: &str = "exact-control-text-v2";
const RULE: &str = "In the original need, current mandate, invitations, commitments and work-judgment meanings, a text field encoded as {\"text_at\":\"/path\"} supplies the complete exact string at that JSON Pointer within CONTROL. The target is inline text in this same input, not an external reference or retrieval request. When an operation requires that string, copy the target text, never the pointer object. Only identical text is shared. Each referring field keeps its own meaning, record, attribution, revision and authority; equal wording does not merge commitments or establish consent. The original_need.request remains inline in full.";
// Earlier retained requests remain inspectable as the inputs actually supplied.
// Their original header declares only the earlier set of work-text fields.
const EARLIER_PROFILE: &str = "exact-control-text-v1";
const EARLIER_RULE: &str = "In the original need, current mandate, invitations and commitments, a text field encoded as {\"text_at\":\"/path\"} supplies the complete exact string at that JSON Pointer within CONTROL. The target is inline text in this same input, not an external reference or retrieval request. When an operation requires that string, copy the target text, never the pointer object. Only identical text is shared. Each referring field keeps its own meaning, record, attribution, revision and authority; equal wording does not merge commitments or establish consent. The original_need.request remains inline in full.";

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

// No traversal of arbitrary authored prose, observations, historical records,
// code, metadata or tool payloads. These slots have a host-owned string contract.
fn paths(control: &Value, judgment_meanings: bool) -> Vec<String> {
    let mut paths = vec!["/original_need/request".into()];
    for field in ["assumptions", "authorization", "boundaries", "terms"] {
        paths.push(format!("/original_need/{field}"));
    }
    paths.push("/mandate/terms".into());
    for parent in ["original_need", "mandate"] {
        if let Some(outcomes) = control[parent]["outcomes"].as_object() {
            for key in outcomes.keys() {
                paths.push(format!("/{parent}/outcomes/{}/description", escape(key)));
                if parent == "original_need" {
                    paths.push(format!("/{parent}/outcomes/{}/criterion", escape(key)));
                }
            }
        }
    }
    for parent in [
        "/original_need/invitations",
        "/arrangements/current_commitments",
    ] {
        if let Some(items) = control.pointer(parent).and_then(Value::as_array) {
            for index in 0..items.len() {
                paths.push(format!("{parent}/{index}/terms/exact_terms"));
            }
        }
    }
    paths.push("/own_acceptance/1/terms/exact_terms".into());
    if let Some(items) = control["own_pending_offers"].as_array() {
        for index in 0..items.len() {
            paths.push(format!("/own_pending_offers/{index}/terms/exact_terms"));
        }
    }
    if judgment_meanings {
        paths.push("/last_attributed_work_judgment/meaning".into());
        if let Some(items) = control["collaborators"]["records"].as_array() {
            for index in 0..items.len() {
                paths.push(format!("/collaborators/records/{index}/last_attributed_work_judgment/meaning"));
            }
        }
    }
    paths
}

pub(super) fn encode(control: &Value) -> Result<Option<Value>> {
    require(
        control.get(FIELD).is_none(),
        "control text is already encoded",
    )?;
    let mut anchors: BTreeMap<&str, String> = BTreeMap::new();
    let mut replacements = Vec::new();
    for path in paths(control, true) {
        let Some(value) = control.pointer(&path) else {
            continue;
        };
        require(value.is_string(), "work text field is not a string")?;
        let text = value.as_str().unwrap();
        if text.len() < 128 {
            continue;
        }
        if let Some(anchor) = anchors.get(text) {
            let replacement = json!({"text_at":anchor});
            if size(&replacement)? < size(value)? {
                replacements.push((path, replacement));
            }
        } else {
            anchors.insert(text, path);
        }
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    let mut compact = control.clone();
    for (path, replacement) in replacements {
        *compact.pointer_mut(&path).unwrap() = replacement;
    }
    compact[FIELD] = json!({"profile":PROFILE,"rule":RULE});
    Ok((size(&compact)? < size(control)?).then_some(compact))
}

#[derive(Default)]
pub(super) struct Accounting {
    pub shared_texts: usize,
    pub references: usize,
}

pub(super) fn decode(control: &mut Value) -> Result<Option<Accounting>> {
    let Some(header) = control.get(FIELD) else {
        for path in paths(control, true) {
            require(
                control.pointer(&path).is_none_or(Value::is_string),
                "work text has an undeclared encoding",
            )?;
        }
        return Ok(None);
    };
    let judgment_meanings = if header == &json!({"profile":PROFILE,"rule":RULE}) {
        true
    } else if header == &json!({"profile":EARLIER_PROFILE,"rule":EARLIER_RULE}) {
        false
    } else {
        return Err(held("unknown control text encoding"));
    };
    require(
        control["original_need"]["request"].is_string(),
        "original request must remain inline",
    )?;
    let allowed: BTreeSet<_> = paths(control, judgment_meanings).into_iter().collect();
    let encoded = size(control)?;
    // The marker is not part of the decoded representation. Remove it before
    // accounting so a valid expansion at the limit is not charged for encoding
    // metadata. No strings are expanded until this bound is checked.
    control.as_object_mut().unwrap().remove(FIELD);
    let mut expanded = size(control)?;
    let mut replacements = Vec::new();
    let mut anchors = BTreeSet::new();
    for path in &allowed {
        let Some(value) = control.pointer(path) else {
            continue;
        };
        if value.is_string() {
            continue;
        }
        let anchor = value["text_at"]
            .as_str()
            .ok_or_else(|| held("invalid work text reference"))?;
        require(
            value.as_object().is_some_and(|object| object.len() == 1) && allowed.contains(anchor),
            "work text reference is outside the declared fields",
        )?;
        let text = control
            .pointer(anchor)
            .filter(|value| value.is_string())
            .ok_or_else(|| held("work text anchor must be inline text"))?;
        let text_bytes = size(text)?;
        let reference_bytes = size(value)?;
        require(
            text.as_str().is_some_and(|text| text.len() >= 128) && text_bytes > reference_bytes,
            "work text reference does not share a complete profitable string",
        )?;
        expanded = expanded
            .checked_add(text_bytes - reference_bytes)
            .ok_or_else(|| held("expanded control text size overflow"))?;
        // Anchors cannot be references, so chained expansion and cycles are
        // never accepted. The full expanded size is recorded without a quota.
        replacements.push((path.clone(), anchor.to_owned()));
        anchors.insert(anchor.to_owned());
    }
    require(
        !replacements.is_empty(),
        "control text encoding has no references",
    )?;
    let accounting = Accounting {
        shared_texts: anchors.len(),
        references: replacements.len(),
    };
    for (path, anchor) in replacements {
        let text = control.pointer(&anchor).unwrap().clone();
        *control.pointer_mut(&path).unwrap() = text;
    }
    require(
        size(control)? == expanded && expanded > encoded,
        "control text sharing accounting mismatch",
    )?;
    Ok(Some(accounting))
}
