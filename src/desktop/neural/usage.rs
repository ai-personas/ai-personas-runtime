use super::*;
use ai_personas::neural::{accounting, footprint};
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

#[derive(Default)]
pub(super) struct Panel {
    storage: bool,
    scope: Option<String>,
    pending: Option<Receiver<Result<Value, String>>>,
    report: Option<Value>,
    error: Option<String>,
}

impl Panel {
    fn load(&mut self, ctx: &egui::Context, root: &Path, work: &str) {
        let root = root.to_owned();
        let work = work.to_owned();
        let storage = self.storage;
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        self.report = None;
        self.error = None;
        std::thread::spawn(move || {
            let report = data::open(&root)
                .and_then(|db| {
                    if storage {
                        footprint::observe(&db, &root, &[])
                    } else {
                        accounting::activity(&db, &work)
                    }
                })
                .map_err(|e| e.to_string());
            let _ = sender.send(report);
            ctx.request_repaint();
        });
    }

    pub(super) fn show(&mut self, ui: &mut Ui, root: &Path, work: &str) -> Option<String> {
        ui.heading("Usage");
        let refresh = ui
            .horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.storage, false, "Selected activity");
                ui.selectable_value(&mut self.storage, true, "Workspace storage");
                ui.add_space(8.);
                ui.small_button("Refresh").clicked()
            })
            .inner;
        if !self.storage && work.is_empty() {
            self.scope = None;
            self.pending = None;
            self.report = None;
            ui.weak("Select an activity to inspect its calls.");
            return None;
        }
        let scope = if self.storage {
            String::from("workspace storage")
        } else {
            work.to_owned()
        };
        if self.scope.as_ref() != Some(&scope) || refresh {
            self.scope = Some(scope);
            self.load(ui.ctx(), root, work);
        }
        if let Some(receiver) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => {
                    ui.spinner();
                    return None;
                }
                Err(TryRecvError::Disconnected) => Err("Measurement worker closed".to_owned()),
            };
            self.pending = None;
            match result {
                Ok(value) => self.report = Some(value),
                Err(reason) => self.error = Some(reason),
            }
        }
        if let Some(reason) = &self.error {
            ui.colored_label(theme::ACCENT, reason);
        }
        let report = self.report.as_ref()?;
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt(("neural-usage", &self.scope))
            .show(ui, |ui| {
                ui.add_space(7.);
                if self.storage {
                    storage_view(ui, report)
                } else {
                    open = activity_view(ui, report);
                }
            });
        open
    }
}

fn value(v: &Value) -> String {
    v.as_u64()
        .map(number)
        .unwrap_or_else(|| "Unreported".to_owned())
}

fn fact(ui: &mut Ui, label: &str, detail: String, color: Color32) {
    card(ui).show(ui, |ui| {
        ui.set_min_width((ui.available_width() - 2.).max(0.));
        ui.label(RichText::new(detail).size(23.).strong().color(color));
        ui.small(label);
    });
}

fn activity_view(ui: &mut Ui, report: &Value) -> Option<String> {
    let totals = &report["totals"];
    let p = theme::palette(ui);
    ui.columns(3, |cols| {
        fact(
            &mut cols[0],
            "Input tokens",
            value(&totals["input_tokens"]["sum"]),
            TEAL,
        );
        fact(
            &mut cols[1],
            "Output tokens",
            value(&totals["output_tokens"]["sum"]),
            PURPLE,
        );
        fact(
            &mut cols[2],
            "Recorded calls",
            value(&report["call_count"]),
            p.text,
        );
    });
    let cached = &totals["cached_input_tokens"];
    let cost = &totals["actual_cost_microusd"];
    ui.add_space(5.);
    ui.label(format!(
        "{} cached input tokens. {} of {} inputs verified against completed receipts.",
        value(&cached["sum"]),
        report["verified_completed_inputs"],
        report["call_count"]
    ));
    if let Some(amount) = cost["sum"].as_u64() {
        ui.small(format!(
            "Reported charge of {:.6} USD across {} calls. {} calls have no reported charge.",
            amount as f64 / 1_000_000.,
            cost["reported"],
            cost["missing"]
        ));
    } else {
        ui.small("The provider supplied no monetary charge for these calls.");
    }
    for &key in &["input_tokens", "output_tokens", "cached_input_tokens"] {
        let missing = totals[key]["missing"].as_u64().unwrap_or_default();
        if missing > 0 {
            ui.small(format!("{} unreported on {missing} calls", title(key)));
        }
    }
    ui.separator();
    let mut open = None;
    let sources = report["text_sources"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    section(
        ui,
        "Text supplied to verified calls",
        &format!("{} exact sources", sources.len()),
    );
    for source in sources {
        let reference = s(source, "reference");
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Open ↗").on_hover_text(reference).clicked() {
                open = Some(reference.into());
            }
            ui.monospace(excerpt(reference, 44))
                .on_hover_text(reference);
        });
        ui.small(format!(
            "{} appearances  ·  {} bytes supplied  ·  {} distinct bytes covered",
            value(&source["appearances"]),
            value(&source["text_bytes_with_repeats"]),
            value(&source["unique_covered_bytes"])
        ));
    }
    if sources.is_empty() {
        ui.weak("No text resource bodies in the verified inputs.");
    }
    let images = report["verified_image_bytes_with_repeats"]
        .as_u64()
        .unwrap_or_default();
    if images > 0 {
        ui.label(format!("{} image bytes supplied", number(images)));
    }
    if let Some(tools) = report["tool_results"].as_object().filter(|o| !o.is_empty()) {
        ui.collapsing("Local tool results and scans", |ui| {
            for (tool, result) in tools {
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(tool);
                    ui.label(format!("{} results", result["results"]));
                    for (key, unit) in [
                        ("scanned_text_bytes", "text bytes scanned"),
                        ("scanned_revisions", "revisions examined"),
                    ] {
                        if let Some(measure) = result.get(key) {
                            ui.weak(format!("{} {unit}", value(&measure["sum"])));
                            if measure["missing"] != 0 {
                                ui.weak(format!("{} missing counts", measure["missing"]));
                            }
                        }
                    }
                });
            }
        });
    }
    ui.separator();
    section(
        ui,
        "Call receipts",
        &format!("{} recorded", report["call_count"]),
    );
    for call in report["calls"].as_array().into_iter().flatten().rev() {
        ui.push_id(s(call, "id"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(s(call, "actor"));
                ui.label(title(s(call, "mode")));
                theme::badge(ui, &title(s(call, "state")), p.muted);
                ui.label(format!("{} tokens", value(&call["usage"]["total_tokens"])));
                if ui.small_button("Input ↗").clicked() {
                    open = Some(s(call, "snapshot").into());
                }
                if let Some(receipt) = call["receipt"].as_str() {
                    if ui.small_button("Receipt ↗").clicked() {
                        open = Some(receipt.into());
                    }
                }
            });
            if call["input_evidence"] != "verified_completed_input" {
                ui.weak(title(s(call, "input_evidence")));
            }
        });
    }
    open
}

fn storage_view(ui: &mut Ui, report: &Value) {
    let all = &report["distinct_object_payload"];
    ui.columns(2, |cols| {
        fact(
            &mut cols[0],
            "Workspace file bytes",
            value(&report["known_workspace_file_bytes"]),
            TEAL,
        );
        fact(
            &mut cols[1],
            "Retained payload bytes",
            value(&all["payload_bytes"]),
            PURPLE,
        );
    });
    ui.small(format!("{} distinct objects. File sizes include SQLite indexes, journals and observed staging files.",value(&all["distinct_objects"])));
    let unknown = report["workspace_files_with_unknown_size"]
        .as_u64()
        .unwrap_or(0);
    if unknown > 0 {
        ui.weak(format!("{unknown} workspace file sizes are unavailable."));
    }
    ui.separator();
    section(ui, "Retained by use", "Roles can share the same bytes");
    for (key, label) in [
        ("head_asset_content", "Current asset contents"),
        ("all_retained_asset_content", "All retained asset contents"),
        (
            "asset_revision_metadata",
            "Asset names, versions and qualifications",
        ),
        ("request_snapshots", "Recorded inputs"),
        ("provider_responses", "Provider responses"),
        ("provider_receipts", "Provider receipts"),
        ("event_records", "Event records"),
    ] {
        let role = &report["overlapping_payload_roles"][key];
        ui.horizontal_wrapped(|ui| {
            ui.strong(label);
            ui.label(format!("{} bytes", value(&role["payload_bytes"])));
            ui.weak(format!("{} objects", value(&role["distinct_objects"])));
        });
    }
    ui.collapsing("Database files", |ui| {
        for file in report["database_files_and_observed_staging"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let name = Path::new(s(file, "path"))
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            ui.label(format!(
                "{name}  ·  {}  ·  {} bytes",
                s(file, "state"),
                value(&file["bytes"])
            ));
        }
    });
    ui.add_space(6.);
    ui.small("External working files, unlisted backups, provider-side storage and earlier peak temporary space are unmeasured.");
}
