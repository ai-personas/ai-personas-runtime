use super::*;
use ai_personas::neural::{accounting, footprint};
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

#[derive(Default)]
pub(super) struct Panel {
    storage: bool,
    selected_call: Option<String>,
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
            if self.scope.as_ref() != Some(&scope) {
                self.selected_call = None;
            }
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
                    open = activity_view(ui, report, &mut self.selected_call);
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

fn activity_view(
    ui: &mut Ui,
    report: &Value,
    selected_call: &mut Option<String>,
) -> Option<String> {
    let totals = &report["totals"];
    let p = theme::palette(ui);
    let cards = [
        ("Total tokens", &totals["total_tokens"]["sum"], p.text),
        ("Input tokens", &totals["input_tokens"]["sum"], TEAL),
        ("Output tokens", &totals["output_tokens"]["sum"], PURPLE),
        ("Recorded calls", &report["call_count"], p.muted),
    ];
    let columns = if ui.available_width() < 560. { 2 } else { 4 };
    for group in cards.chunks(columns) {
        ui.columns(group.len(), |cols| {
            for (column, (label, quantity, color)) in cols.iter_mut().zip(group) {
                fact(column, label, value(quantity), *color);
            }
        });
    }
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
            if let Some(path) = source["path"].as_str() {
                ui.strong(path).on_hover_text(reference);
            } else {
                ui.monospace(excerpt(reference, 44))
                    .on_hover_text(reference);
            }
        });
        ui.small(format!(
            "{} {}  ·  {} bytes supplied  ·  {} distinct bytes covered",
            value(&source["appearances"]),
            if source["appearances"] == 1 {
                "appearance"
            } else {
                "appearances"
            },
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
    let calls = report["calls"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let max = calls
        .iter()
        .filter_map(|c| {
            c["usage"]["input_tokens"]
                .as_u64()?
                .checked_add(c["usage"]["output_tokens"].as_u64()?)
        })
        .max()
        .unwrap_or(1)
        .max(1);
    if !calls.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (label, color) in [
                ("Input", TEAL),
                ("Cached input", CACHED),
                ("Output", PURPLE),
            ] {
                ui.colored_label(color, "■");
                ui.small(label);
            }
        });
    }
    for (i, call) in calls.iter().enumerate().rev() {
        let id = s(call, "id");
        ui.push_id(id, |ui| {
            let expanded = selected_call.as_deref() == Some(id);
            ui.horizontal_wrapped(|ui| {
                let choose = ui
                    .selectable_label(expanded, format!("{}   {}", i + 1, title(s(call, "actor"))))
                    .on_hover_text(id)
                    .clicked();
                let bar = token_bar(ui, &call["usage"], max);
                if choose || bar {
                    *selected_call = if expanded { None } else { Some(id.to_owned()) };
                }
                ui.label(format!("{} tokens", value(&call["usage"]["total_tokens"])));
                theme::badge(ui, &title(s(call, "state")), p.muted);
                if ui.small_button("Input ↗").clicked() {
                    open = Some(s(call, "snapshot").into());
                }
                if let Some(receipt) = call["receipt"].as_str() {
                    if ui.small_button("Receipt ↗").clicked() {
                        open = Some(receipt.into());
                    }
                }
            });
            if selected_call.as_deref() == Some(id) {
                ui.indent("call-details", |ui| {
                    ui.small(format!(
                        "{}  ·  {}  ·  {}",
                        title(s(call, "mode")),
                        time_label(call["created"].as_i64().unwrap_or_default()),
                        s(call, "reported_model")
                    ));
                    ui.label(format!(
                        "{} input  ·  {} cached  ·  {} output tokens",
                        value(&call["usage"]["input_tokens"]),
                        value(&call["usage"]["cached_input_tokens"]),
                        value(&call["usage"]["output_tokens"])
                    ));
                    for source in call["sources"].as_array().into_iter().flatten() {
                        let reference = s(source, "reference");
                        let range = if let Some(range) = source["range"].as_array() {
                            format!("  ·  bytes {}–{}", range[0], range[1])
                        } else {
                            String::new()
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(s(source, "via"));
                            ui.small(format!("{}{range}", excerpt(reference, 37)))
                                .on_hover_text(reference);
                        });
                    }
                });
            }
            if call["input_evidence"] != "verified_completed_input" {
                ui.weak(title(s(call, "input_evidence")));
            }
        });
    }
    open
}

const CACHED: Color32 = Color32::from_rgb(102, 154, 208);

fn token_bar(ui: &mut Ui, usage: &Value, max: u64) -> bool {
    let width = (ui.available_width() * 0.29).clamp(64., 190.);
    let (bounds, response) = ui.allocate_exact_size(egui::vec2(width, 12.), egui::Sense::click());
    let track = bounds.shrink2(egui::vec2(0., 2.));
    ui.painter()
        .rect_filled(track, 2., theme::palette(ui).field);
    if let (Some(input), Some(output)) = (
        usage["input_tokens"].as_u64(),
        usage["output_tokens"].as_u64(),
    ) {
        let cached = usage["cached_input_tokens"]
            .as_u64()
            .filter(|n| *n <= input);
        let mut x = track.left();
        for (tokens, color) in [
            (input - cached.unwrap_or(0), TEAL),
            (cached.unwrap_or(0), CACHED),
            (output, PURPLE),
        ] {
            if tokens == 0 {
                continue;
            }
            let right = x + width * (tokens as f64 / max as f64) as f32;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x, track.top()),
                    egui::pos2(right, track.bottom()),
                ),
                0.,
                color,
            );
            x = right;
        }
    }
    response
        .on_hover_text(format!(
            "{} input tokens, including {} cached. {} output tokens. Select to inspect this call.",
            value(&usage["input_tokens"]),
            value(&usage["cached_input_tokens"]),
            value(&usage["output_tokens"])
        ))
        .clicked()
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
