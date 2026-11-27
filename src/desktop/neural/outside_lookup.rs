//! A saved register page is not today's field desk, nor a report delivered by a human.
use super::*;
use ai_personas::neural::{human_requests::entries_at_lookup, store};
use rusqlite::Connection;
use serde_json::json;

pub(super) fn outcome(db: &Connection, work: &str, body: &Value) -> Value {
    let reference = body["result"].as_str().filter(|r| r.starts_with("object:"));
    let saved = reference
        .and_then(|r| r.strip_prefix("object:"))
        .and_then(|digest| store::object(db, digest).ok());
    match saved
        .as_ref()
        .and_then(|v| Some((v, entries_at_lookup(work, v)?)))
    {
        Some((page, rows)) => {
            let count = rows.len();
            let unknown = page["status_unavailable_count"].as_u64().unwrap_or(0);
            let mut label = format!("{} on page", counted(count as u64, "entry", "entries"));
            if unknown > 0 {
                label.push_str(&format!(" · {unknown} unknown"));
            }
            json!({"label":label,"problem":unknown>0,"register_page":page,"result":reference})
        }
        None => json!({"label":"Saved page unavailable","problem":true,"result":reference}),
    }
}

pub(super) fn show(ui: &mut Ui, app: &App, event: &Value, page: &Value) -> Option<String> {
    let rows = page["requests"].as_array()?;
    let mut inspect = None;
    egui::CollapsingHeader::new("Status at lookup")
        .id_salt((
            "outside-lookup",
            s(event, "digest"),
            event["event"].as_i64(),
        ))
        .show(ui, |ui| {
            ui.weak(if page["include_closed"] == true {
                "Active and closed requests · saved page"
            } else {
                "Active requests only · saved page"
            });
            if rows.is_empty() {
                ui.label("No entries on this page.");
            }
            for r in rows {
                let reference = s(r, "reference");
                ui.push_id(reference, |ui| {
                    ui.strong(s(r, "title"));
                    ui.horizontal_wrapped(|ui| {
                        let (phase, color) = inbox::phase(s(r, "state"));
                        theme::badge(ui, phase, color);
                        ui.small(format!("Requested by {}", app.label(s(r, "requester"))));
                        if let Some(who) = r["claimed_by"].as_str() {
                            let role = if r["state"] == "in_progress" {
                                "Held by"
                            } else {
                                "Reported by"
                            };
                            ui.small(format!("{role} {who}"));
                        }
                        if let Some((_, n)) = reference.rsplit_once('@') {
                            if ui
                                .link(format!("Read edition {n} ↗"))
                                .on_hover_text(reference)
                                .clicked()
                            {
                                inspect = Some(reference.into());
                            }
                        }
                    });
                });
            }
            if let Some(n) = page["next_offset"].as_u64() {
                ui.small(format!("More results were available · next offset {n}"));
            }
            if let Some(n) = page["status_unavailable_count"].as_u64().filter(|n| *n > 0) {
                ui.label(format!(
                    "Additional requests with unknown status in this activity: {n}"
                ));
            }
            ui.weak("This lookup returned status, not a human report or file contents.");
        });
    inspect
}
