//! Sender, reader and replies are separate links in a peer's exchange.
use super::*;

pub(super) enum Action {
    Inspect(String),
    Follow(i64),
    Refresh,
}

pub(in crate::desktop::neural) fn is_peer_message(e: &Value) -> bool {
    e["kind"] == "message"
        && e["invocation"].is_string()
        && e["actor"] != e["target"]
        && e["target"]
            .as_str()
            .is_some_and(|t| !matches!(t, "user" | "*"))
}

fn person(ui: &mut Ui, app: &App, id: &str) {
    let name = app.label(id);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(23., 23.), egui::Sense::hover());
    design::persona_mark(ui, rect, name, persona_color(id));
    ui.strong(name).on_hover_text(id);
}

fn link(ui: &mut Ui, title: &str, reference: &Value, action: &mut Option<Action>) {
    if let Some(r) = reference.as_str() {
        if ui.link(title).on_hover_text(r).clicked() {
            *action = Some(Action::Inspect(r.into()));
        }
    }
}

fn words(ui: &mut Ui, text: &str, id: &str) {
    let long = text.chars().count() > 320 || text.lines().count() > 4;
    if long {
        ui.label(home::short_text(text, 240));
        egui::CollapsingHeader::new("Full message")
            .id_salt(id)
            .show(ui, |ui| {
                ui.add(egui::Label::new(text).wrap().selectable(true));
            });
    } else {
        ui.add(egui::Label::new(text).wrap().selectable(true));
    }
}

pub(super) fn show(ui: &mut Ui, app: &App, event: i64, report: &Value) -> Option<Action> {
    let from = s(report, "sender");
    let to = s(report, "recipient");
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        person(ui, app, from);
        ui.weak("→");
        person(ui, app, to);
    });
    ui.small(format!(
        "Message #{event}  ·  {}",
        time_label(report["sent_at"].as_i64().unwrap_or_default())
    ));
    ui.add_space(4.);
    words(ui, s(report, "text"), "original-peer-message");
    ui.horizontal_wrapped(|ui| {
        link(
            ui,
            "Sending decision",
            &report["sent"]["decision"],
            &mut action,
        );
        link(ui, "Inbox receipt", &report["sent"]["receipt"], &mut action);
    });
    if let Some(refs) = report["references"].as_array().filter(|r| !r.is_empty()) {
        egui::CollapsingHeader::new(format!("Shared references · {}", refs.len())).show(ui, |ui| {
            ui.small(
                "Sending these addresses does not establish that the files or records were read.",
            );
            for r in refs {
                if let Some(name) = r.as_str() {
                    link(ui, name, r, &mut action);
                }
            }
        });
    }
    ui.separator();
    ui.strong(format!("On {}’s desk", app.label(to)));
    let read = &report["received"];
    if read.is_object() {
        ui.horizontal_wrapped(|ui| {
            theme::badge(ui, "Message received", TEAL);
            link(ui, "Receiving decision", &read["decision"], &mut action);
        });
        ui.label(match s(read, "via") {
            "incoming_event" => "The full message arrived in this decision’s input.",
            "selection" | "experience_read" => "The full exchange was supplied as a chosen record.",
            _ => "The full exchange returned as an earlier observation.",
        });
        if read["cited_in_work"] == true || read["considered_for_character"] == true {
            let mut reasons = Vec::new();
            if read["cited_in_work"] == true {
                reasons.push("cited in work evidence");
            }
            if read["considered_for_character"] == true {
                reasons.push("considered for character maintenance");
            }
            ui.small(format!("In that decision: {}.", reasons.join("; ")));
        }
    } else {
        theme::badge(ui, "In the inbox", theme::palette(ui).muted);
        ui.label("No full message verified in an accepted recipient decision in this activity.");
    }
    if let Some(gaps) = report["unverified_inputs"]
        .as_array()
        .filter(|v| !v.is_empty())
    {
        egui::CollapsingHeader::new(format!("Inputs with incomplete evidence · {}", gaps.len()))
            .show(ui, |ui| {
                for gap in gaps {
                    link(
                        ui,
                        "Inspect candidate decision",
                        &gap["decision"],
                        &mut action,
                    );
                    ui.small(s(gap, "reason"));
                }
            });
    }
    ui.separator();
    let replies = report["replies"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    ui.strong(format!("Direct replies here · {}", replies.len()));
    if replies.is_empty() {
        ui.weak("No verified peer message explicitly replies to this one.");
    } else {
        ui.small("Only peer messages addressed back to this exact exchange.");
    }
    for reply in replies {
        let Some(n) = reply["event"].as_i64() else {
            continue;
        };
        ui.push_id(n, |ui| {
            ui.add_space(4.);
            ui.strong(format!("{} · #{n}", app.label(to)));
            words(ui, s(reply, "text"), "peer-reply");
            ui.small(match s(reply, "parent_via") {
                "incoming_event" => "The original message arrived with this replying decision.",
                "selection" | "experience_read" => "The replying decision received the original exchange as a chosen record.",
                "reply_address" => "Linked by reply address; a full read in this replying decision was not verified.",
                _ => "The replying decision received the original exchange as an earlier observation.",
            });
            ui.horizontal_wrapped(|ui| {
                if ui.link("Follow reply →").clicked() { action = Some(Action::Follow(n)); }
                link(ui, "Replying decision", &reply["sent"]["decision"], &mut action);
            });
        });
    }
    for gap in report["unverified_replies"]
        .as_array()
        .into_iter()
        .flatten()
    {
        ui.weak(format!(
            "Unverified reply #{} · {}",
            gap["event"],
            s(gap, "reason")
        ));
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.small(format!(
            "Checked through event #{}",
            report["through_event"]
        ));
        if ui.small_button("Refresh").clicked() {
            action = Some(Action::Refresh);
        }
    });
    action
}
