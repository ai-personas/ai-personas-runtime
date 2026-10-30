//! Inspect recorded paths and exact qualification edges, without semantic inference.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

struct Node<'a> {
    reference: Value,
    entry: Option<&'a Value>,
    records: Vec<&'a Value>,
    scope: &'a str,
}

pub(super) fn entry_scope<'a>(entry: &'a Value, revision: &Value) -> &'a str {
    match entry["scope"].as_str() {
        Some(scope) => scope,
        None if !revision["source"].is_null()
            && !revision["revision"].is_null()
            && entry["reference"]["source"] == revision["source"]
            && entry["reference"]["revision"] == revision["revision"] =>
        {
            "fragments"
        }
        None => "unavailable",
    }
}

fn key(reference: &Value) -> String {
    reference.to_string()
}
fn caption(reference: &Value) -> String {
    let revision = reference["revision"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| reference["revision"].to_string());
    let short: String = revision.chars().take(12).collect();
    let suffix = if revision.chars().count() > 12 {
        "…"
    } else {
        ""
    };
    let range = match (reference["start"].as_u64(), reference["end"].as_u64()) {
        (Some(start), Some(end)) => format!(" · bytes {start}..{end}"),
        _ => String::new(),
    };
    let representation = &reference["representation"];
    let format = match representation
        .as_str()
        .or_else(|| representation["kind"].as_str())
    {
        Some("utf8") => "UTF-8".to_owned(),
        Some("json_string") => format!("JSON string {}", s(representation, "pointer")),
        Some("bytes_metadata") => "Byte metadata".to_owned(),
        Some("unsupported") => format!("Unsupported representation {}", s(representation, "name")),
        _ => "Representation unavailable".to_owned(),
    };
    format!(
        "{} · {short}{suffix} · {format}{range}",
        s(reference, "source")
    )
}
fn scope_label(scope: &str) -> &str {
    match scope {
        "fragments" => "Characteristic fragment",
        "assets" => "Supporting asset",
        _ => "Scope unavailable",
    }
}

#[derive(Default)]
struct PathTree {
    folders: BTreeMap<String, PathTree>,
    entries: Vec<String>,
}
impl PathTree {
    fn insert(&mut self, path: &str, reference: String) {
        let parts: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
        let mut branch = self;
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            branch = branch.folders.entry((*part).to_owned()).or_default();
        }
        branch.entries.push(reference);
    }
    fn show(&self, ui: &mut Ui, nodes: &BTreeMap<String, Node<'_>>, focus: &mut String) {
        for (folder, tree) in &self.folders {
            ui.push_id(folder, |ui| {
                egui::CollapsingHeader::new(folder)
                    .default_open(true)
                    .show(ui, |ui| tree.show(ui, nodes, focus));
            });
        }
        for reference in &self.entries {
            node_button(ui, nodes, reference, focus);
        }
    }
}

fn node_button(
    ui: &mut Ui,
    nodes: &BTreeMap<String, Node<'_>>,
    reference: &str,
    focus: &mut String,
) {
    let Some(node) = nodes.get(reference) else {
        return;
    };
    let path = node
        .entry
        .map(|entry| s(entry, "path"))
        .filter(|path| !path.is_empty());
    let label = format!(
        "{}\n{} · {}",
        path.unwrap_or_else(|| s(&node.reference, "source")),
        scope_label(node.scope),
        caption(&node.reference)
    );
    ui.push_id(reference, |ui| {
        if ui
            .add(
                egui::Button::new(label)
                    .selected(focus.as_str() == reference)
                    .wrap(),
            )
            .on_hover_text(reference)
            .clicked()
        {
            *focus = reference.to_owned();
        }
    });
}

impl App {
    pub(super) fn persona_relationships(&mut self, ui: &mut Ui, snapshot: &Value) {
        theme::header(ui, Icon::People, "Fragment relationships", "RECORDED LINKS");
        ui.weak("Paths show recorded file organization. Arrows show exact required qualifications or current character references. Shared wording, topics, and folder placement do not establish semantic links.");
        ui.checkbox(&mut self.profile_history, "Include historical revisions");
        let mut nodes = BTreeMap::new();
        let mut edges = BTreeSet::new();
        let mut character = Vec::new();
        for revision in models_at(snapshot, "characteristic_revisions")
            .iter()
            .filter(|revision| self.profile_history || revision["current"] == true)
        {
            for entry in models_at(&revision["context"], "entries") {
                let reference = &entry["reference"];
                if reference.is_null() {
                    continue;
                }
                let target = key(reference);
                let node = nodes.entry(target.clone()).or_insert_with(|| Node {
                    reference: reference.clone(),
                    entry: None,
                    records: Vec::new(),
                    scope: "unavailable",
                });
                node.entry = Some(entry);
                node.scope = entry_scope(entry, revision);
                node.records.push(entry);
                for role in models_at(entry, "roles")
                    .iter()
                    .filter(|role| role["role"] == "qualification")
                {
                    let parent = &role["required_by"];
                    if parent.is_null() {
                        continue;
                    }
                    let from = key(parent);
                    nodes.entry(from.clone()).or_insert_with(|| Node {
                        reference: parent.clone(),
                        entry: None,
                        records: Vec::new(),
                        scope: "unavailable",
                    });
                    edges.insert((from, target.clone()));
                }
            }
        }
        for reference in models_at(&snapshot["character"], "own_voice") {
            let id = key(reference);
            nodes.entry(id.clone()).or_insert_with(|| Node {
                reference: reference.clone(),
                entry: None,
                records: Vec::new(),
                scope: "unavailable",
            });
            character.push(id);
        }
        let state_id = ui.id().with(("fragment-focus", s(snapshot, "persona")));
        let retained = ui.data(|data| data.get_temp::<String>(state_id));
        let outside_view = retained
            .as_ref()
            .filter(|id| !nodes.contains_key(*id))
            .cloned();
        if let Some(id) = &outside_view {
            if let Ok(reference) = serde_json::from_str::<Value>(id) {
                nodes.insert(
                    id.clone(),
                    Node {
                        reference,
                        entry: None,
                        records: Vec::new(),
                        scope: "unavailable",
                    },
                );
            }
        }
        if nodes.is_empty() {
            ui.label("No characteristic references or qualification links are available in this inspection.");
            return;
        }
        let mut focus = retained
            .filter(|id| nodes.contains_key(id))
            .unwrap_or_else(|| {
                character
                    .first()
                    .cloned()
                    .unwrap_or_else(|| nodes.keys().next().unwrap().clone())
            });
        ui.collapsing(
            format!(
                "Current character → exact own-voice references ({})",
                character.len()
            ),
            |ui| {
                if character.is_empty() {
                    ui.weak("No own-voice reference is available in the current character record.");
                }
                for reference in &character {
                    node_button(ui, &nodes, reference, &mut focus);
                }
            },
        );
        let mut tree = PathTree::default();
        for (id, node) in &nodes {
            if let Some(entry) = node.entry {
                tree.insert(s(entry, "path"), id.clone());
            }
        }
        egui::CollapsingHeader::new("Recorded path hierarchy")
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("fragment-paths")
                    .max_height(190.0)
                    .show(ui, |ui| {
                        tree.show(ui, &nodes, &mut focus);
                    });
            });
        let selected = focus.clone();
        let node = &nodes[&selected];
        ui.add_space(8.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            if outside_view.as_ref() == Some(&selected) {
                ui.weak("The selected exact reference is outside the visible snapshot or history filter. Its identity is retained; no other revision or range is substituted.");
            }
            ui.horizontal_wrapped(|ui| {
                let color = if node.scope == "fragments" { theme::BLUE } else { theme::ACCENT };
                theme::badge(ui, scope_label(node.scope), color);
                ui.label(caption(&node.reference)).on_hover_text(selected.clone());
            });
            if character.contains(&selected) { ui.label("Current character → this exact reference"); }
            let incoming: Vec<_> = edges.iter().filter(|(_, to)| to == &selected).collect();
            let outgoing: Vec<_> = edges.iter().filter(|(from, _)| from == &selected).collect();
            if incoming.is_empty() && outgoing.is_empty() { ui.weak("No qualification edge is recorded for this exact reference in the visible snapshot."); }
            for (from, _) in incoming {
                ui.horizontal_wrapped(|ui| {
                    node_button(ui, &nodes, from, &mut focus);
                    ui.label("→ requires this qualification");
                });
            }
            for (_, to) in outgoing {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Requires qualification →");
                    node_button(ui, &nodes, to, &mut focus);
                });
            }
            if let Some(entry) = node.entry {
                ui.separator();
                ui.strong(s(entry, "path"));
                if !s(entry, "author").is_empty() {
                    ui.horizontal_wrapped(|ui| { ui.weak("Recorded author"); self.persona_link(ui, s(entry, "author")); });
                }
                if entry["retired"] == true { theme::badge(ui, "RETIRED", theme::ACCENT); }
                egui::ScrollArea::vertical().id_salt(("linked-entry", &selected)).max_height(280.0).show(ui, |ui| {
                    theme::message_body(ui, s(entry, "text"));
                });
                ui.collapsing("Exact reference, roles & all visible context records", |ui| {
                    for (index, record) in node.records.iter().enumerate() {
                        ui.label(format!("Context record {}", index + 1));
                        json_view(ui, record);
                    }
                });
            } else {
                ui.label("This exact reference is recorded, but its body is not included under the current filter. Include historical revisions to inspect any retained older bodies. A different range or revision is not substituted.");
                ui.collapsing("Exact reference", |ui| json_view(ui, &node.reference));
            }
        });
        ui.data_mut(|data| data.insert_temp(state_id, focus));
        ui.weak("Supporting assets remain separate from characteristic fragments. These links do not establish that a later invocation supplied or used the referenced body.");
    }
}
