use super::*;

pub(super) fn show(
    ui: &mut Ui,
    frame: &Value,
    status: &Value,
    open: &mut Option<String>,
    refresh: &mut bool,
) {
    let Some(rows) = frame["action_returns"].as_array() else {
        return;
    };
    let caption = if rows.len() == 1 {
        "Action and return".to_owned()
    } else {
        format!("Actions and returns · {}", rows.len())
    };
    ui.collapsing(caption, |ui| {
        for row in rows {
            ui.push_id(s(row, "outcome"), |ui| {
                let endpoint = &row["endpoint"];
                let kind = match s(row, "return_kind") {
                    "addressed_response" => "Addressed response",
                    "issued_edition_reviewed" => "Peer finding",
                    "issued_files_checked" => "Receiving check",
                    "request_reported" => "Outside report",
                    "dispatched_result" => "Tool return",
                    "in_transaction_receipt" => "Operation receipt",
                    _ => "Observed return",
                };
                ui.horizontal_wrapped(|ui| {
                    if let Some(id) = endpoint["id"].as_str() {
                        ui.strong(title(&excerpt(id, 48)))
                            .on_hover_text(format!("{} · {id}", s(endpoint, "tool")));
                    } else {
                        ui.strong("Reply");
                    }
                    ui.weak("→");
                    let from = s(row, "author_of_return");
                    if row["return_kind"] == "request_reported" {
                        ui.label(format!("{kind} from {from}"));
                    } else if matches!(
                        s(row, "return_kind"),
                        "addressed_response" | "issued_edition_reviewed" | "issued_files_checked"
                    ) {
                        ui.label(format!("{kind} from {}", persona_label(status, from)))
                            .on_hover_text(from);
                    } else {
                        ui.label(kind);
                    }
                    let run = &row["process_as_supplied"];
                    if run["started"] == false {
                        ui.small("Process did not start");
                    } else if run["started"] == true {
                        if let Some(code) = run["exit_code"].as_i64() {
                            ui.small(format!("Exit {code}"));
                        } else {
                            ui.small("No exit code reported");
                        }
                    }
                });
                ui.collapsing("Judgment before the action", |ui| {
                    ui.add(egui::Label::new(s(row, "judgment_before")).wrap());
                });
                for (paired, label) in [
                    (true, "Cited action and return"),
                    (false, "Cited return only"),
                ] {
                    let features = row["citations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|c| c["also_cited_action"] == paired)
                        .map(|c| title(s(c, "feature")))
                        .collect::<Vec<_>>();
                    if !features.is_empty() {
                        ui.add(
                            egui::Label::new(format!("{label}: {}", features.join(", "))).wrap(),
                        );
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    link(ui, "Acting decision ↗", s(row, "decision"), open);
                    link(ui, "Return record ↗", s(row, "outcome"), open);
                });
                if let Some(streams) = row["streams"].as_array() {
                    if streams.is_empty() {
                        ui.small("No verified command stream text to open.");
                    }
                    for stream in streams {
                        ui.horizontal_wrapped(|ui| {
                            let name = s(stream, "stream");
                            let n = stream["text_bytes"].as_u64().unwrap_or_default();
                            let extent = if stream["whole"] == true {
                                "whole stream"
                            } else if stream["excerpt"] == true {
                                "excerpt"
                            } else {
                                "extent unverified"
                            };
                            ui.small(format!(
                                "{name} · {} of text supplied · {extent}",
                                counted(n, "byte", "bytes")
                            ));
                            if stream["whole"] == true && n > 0 {
                                if let Some(r) = stream["original"].as_str() {
                                    link(ui, &format!("Open {name} ↗"), r, open);
                                } else {
                                    ui.small("Original unavailable");
                                    *refresh |= ui.small_button("Check again").clicked();
                                }
                            }
                        });
                    }
                }
                ui.separator();
            });
        }
    });
}

fn link(ui: &mut Ui, caption: &str, r: &str, open: &mut Option<String>) {
    let response = ui.link(caption).on_hover_text(r);
    if response.clicked() {
        *open = Some(r.to_owned());
    }
    response.context_menu(|ui| {
        if ui.button("Copy reference").clicked() {
            ui.ctx().copy_text(r.to_owned());
            ui.close();
        }
    });
}
