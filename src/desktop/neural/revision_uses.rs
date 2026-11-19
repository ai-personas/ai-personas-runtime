//! Browse a character generation or try an authored candidate on exact recorded inputs.
use super::*;
use ai_personas::neural::{
    character_paths,
    revision_uses::{inspect, Basis, Query},
};
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

#[derive(Default)]
pub(super) struct Panel {
    owner: String,
    query: Option<Query>,
    pending: Option<Receiver<Result<Value, String>>>,
    report: Option<Result<Value, String>>,
    selected: usize,
}

enum Action {
    Open(String),
    Paths {
        query: character_paths::Query,
        accepted: String,
    },
}

impl App {
    pub(super) fn character_revisions(&mut self, ui: &mut Ui, character: &data::Character) {
        if self.revision_uses.owner != self.persona {
            self.revision_uses = Panel {
                owner: self.persona.clone(),
                ..Default::default()
            };
        }
        if self.revision_uses.query.is_some() {
            match self
                .revision_uses
                .show(ui, &self.root, &self.snapshot.status)
            {
                Some(Action::Open(reference)) => self.open_resource(ui.ctx(), reference),
                Some(Action::Paths { query, accepted }) => self.character_paths.open_query(
                    ui.ctx(),
                    &self.root,
                    &self.persona,
                    &accepted,
                    query,
                ),
                None => {}
            }
            return;
        }
        let mut inspect = None;
        egui::ScrollArea::vertical()
            .id_salt(("revision-history", &self.persona))
            .show(ui, |ui| {
                section(ui, "Activation history", "Newest first");
                let events: Vec<_> = self
                    .events
                    .iter()
                    .rev()
                    .filter(|e| e["kind"] == "model_changed" && e["actor"] == self.persona)
                    .collect();
                if events.is_empty() {
                    ui.weak("No recorded activation for this persona.");
                }
                for event in events {
                    let body = &event["body"];
                    let reference = format!("experience:event-{}@1", event["event"]);
                    let change = if body["choice"]["kind"] == "rollback" {
                        "Rollback"
                    } else if body["epoch"] == 1 {
                        "Initial activation"
                    } else {
                        "Activation"
                    };
                    ui.push_id(&reference, |ui| {
                        card(ui).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                theme::badge(
                                    ui,
                                    &format!("Epoch {}", body["epoch"]),
                                    if change == "Rollback" { PURPLE } else { TEAL },
                                );
                                ui.strong(change);
                                ui.small(time_label(event["created"].as_i64().unwrap_or(0)));
                            });
                            if let Some(reason) = body["choice"]["reason"].as_str() {
                                ui.label(reason);
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Inspect later decisions").clicked() {
                                    inspect = Some(reference.clone());
                                }
                                ui.collapsing("Change record", |ui| {
                                    super::super::data_view::parameters(ui, body)
                                });
                            });
                        });
                    });
                    ui.add_space(6.);
                }
                ui.add_space(10.);
                section(ui, "Authored candidates", "Recorded proposals");
                let candidates: Vec<Value> = self
                    .snapshot
                    .candidates
                    .iter()
                    .filter(|c| c["body"]["owner"] == self.persona)
                    .cloned()
                    .collect();
                if candidates.is_empty() {
                    ui.weak("No candidates have been authored.");
                }
                for candidate in candidates {
                    let b = &candidate["body"];
                    let reference = s(&candidate, "reference");
                    ui.push_id(reference, |ui| {
                        card(ui).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            let active = b["model"] == character.baseline.model;
                            theme::badge(
                                ui,
                                if active {
                                    "Active package"
                                } else {
                                    "Retained candidate"
                                },
                                if active { TEAL } else { PURPLE },
                            );
                            ui.label(s(b, "intent"));
                            ui.horizontal_wrapped(|ui| {
                                if b["predecessor"].is_string()
                                    && ui.button("Try on recorded inputs").clicked()
                                {
                                    inspect = Some(reference.to_owned());
                                }
                                if ui.small_button("Open candidate record").clicked() {
                                    self.open_resource(ui.ctx(), reference.into());
                                }
                                if let Some(parent) = b["predecessor"].as_str() {
                                    if ui.small_button("Open predecessor").clicked() {
                                        self.open_resource(ui.ctx(), format!("object:{parent}"));
                                    }
                                }
                            });
                        })
                    });
                    ui.add_space(6.);
                }
                ui.add_space(10.);
                section(ui, "Active package", "Immutable fingerprints");
                ui.small("Package");
                ui.add(
                    egui::Label::new(
                        RichText::new(&character.baseline.model)
                            .monospace()
                            .size(12.),
                    )
                    .wrap(),
                );
                ui.small("Numerical model");
                ui.add(
                    egui::Label::new(RichText::new(&character.numerical).monospace().size(12.))
                        .wrap(),
                );
                if ui.small_button("Inspect model JSON").clicked() {
                    self.open_resource(ui.ctx(), format!("object:{}", character.baseline.model));
                }
            });
        if let Some(reference) = inspect {
            self.revision_uses.load(
                ui.ctx(),
                &self.root,
                Query {
                    reference,
                    work: None,
                    basis: None,
                    offset: 0,
                    page_size: 6,
                },
            );
        }
    }
}

impl Panel {
    fn load(&mut self, ctx: &egui::Context, root: &Path, query: Query) {
        self.query = Some(query.clone());
        self.report = None;
        self.selected = 0;
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let (ctx, root, owner) = (ctx.clone(), root.to_owned(), self.owner.clone());
        std::thread::spawn(move || {
            let result = data::open(&root).and_then(|db| inspect(&db, Some(&owner), &query));
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn show(&mut self, ui: &mut Ui, root: &Path, status: &Value) -> Option<Action> {
        if ui.small_button("← All revisions").clicked() {
            self.query = None;
            self.report = None;
            self.pending = None;
            return None;
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(report) => {
                    self.report = Some(report);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.report = Some(Err("The revision reader stopped.".into()));
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut query = self.query.clone()?;
        let mut refresh = false;
        let mut action = None;
        let area = egui::ScrollArea::vertical().id_salt((
            "revision-use-report",
            &query.reference,
            query.offset,
        ));
        area.show(ui, |ui| {
            if let Some(Ok(report)) = &self.report {
                subject_header(ui, report);
            }
            ui.add_space(6.);
            let old_basis = query.basis;
            let old_work = query.work.clone();
            ui.horizontal_wrapped(|ui| {
                for (basis, label) in [
                    (None, "All inputs"),
                    (Some(Basis::Baseline), "Baseline"),
                    (Some(Basis::Contextual), "Contextual"),
                ] {
                    ui.selectable_value(&mut query.basis, basis, label);
                }
                let selected = query
                    .work
                    .as_deref()
                    .map(|id| excerpt(relationships::work_name(status, id), 28))
                    .unwrap_or_else(|| "All activities".into());
                egui::ComboBox::from_id_salt("revision-activity")
                    .width(160.)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut query.work, None, "All activities");
                        for item in status["work"].as_array().into_iter().flatten() {
                            let b = &item["body"];
                            let name = activity_title(b);
                            ui.selectable_value(
                                &mut query.work,
                                Some(s(b, "id").to_owned()),
                                excerpt(name, 52),
                            )
                            .on_hover_text(name);
                        }
                    });
                refresh |= ui.small_button("Refresh").clicked();
            });
            if old_basis != query.basis || old_work != query.work {
                query.offset = 0;
                refresh = true;
            }
            ui.separator();
            let report = match &self.report {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking the recorded decisions…");
                    });
                    return;
                }
                Some(Err(error)) => {
                    ui.colored_label(theme::ACCENT, error);
                    return;
                }
                Some(Ok(report)) => report,
            };
            if refresh {
                return;
            }
            let Some(rows) = report["decisions"].as_array() else {
                return;
            };
            refresh = page_controls(ui, report, rows.len(), &mut query);
            if !rows.is_empty() {
                let width = ui.available_width();
                ui.add_space(4.);
                if width >= 640. {
                    let list_width = (width * 0.33).clamp(205., 320.);
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(list_width, 0.),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                decision_list(ui, rows, status, &mut self.selected);
                            },
                        );
                        if let Some(row) = rows.get(self.selected) {
                            ui.vertical(|ui| decision_card(ui, report, row, status, &mut action));
                        }
                    });
                } else {
                    decision_list(ui, rows, status, &mut self.selected);
                    ui.add_space(6.);
                    if let Some(row) = rows.get(self.selected) {
                        decision_card(ui, report, row, status, &mut action);
                    }
                }
            }
            ui.add_space(6.);
            ui.collapsing("Model and transition records", |ui| {
                for (label, reference) in [
                    ("Origin", &report["subject"]["reference"]),
                    ("Supplied package", &report["supplied_package"]),
                    ("Comparison package", &report["comparison_package"]),
                    (
                        "Authoring decision",
                        &report["subject"]["proposal_decision"],
                    ),
                    ("Switch decision", &report["subject"]["decision"]),
                ] {
                    open_link(ui, label, reference, &mut action);
                }
            });
        });
        if refresh {
            self.load(ui.ctx(), root, query);
        }
        action
    }
}

fn subject_header(ui: &mut Ui, report: &Value) {
    let subject = &report["subject"];
    let (label, summary) = match s(subject, "kind") {
        "candidate" => (
            "Candidate on recorded inputs",
            "Compare the candidate with the preceding package that each decision received.",
        ),
        "initial_activation" => (
            "Use after initial activation",
            "These decisions received the first active package.",
        ),
        "rollback" => (
            "Use after rollback",
            "Compare the restored package supplied to each decision with the package it replaced.",
        ),
        _ => (
            "Use after activation",
            "Compare the adopted package supplied to each decision with the package it replaced.",
        ),
    };
    ui.horizontal_wrapped(|ui| {
        ui.heading(label);
        if let Some(epoch) = subject["epoch"].as_u64() {
            theme::badge(ui, &format!("Epoch {epoch}"), TEAL);
        }
    });
    ui.label(summary);
}

fn page_controls(ui: &mut Ui, report: &Value, rows: usize, query: &mut Query) -> bool {
    let total = report["total_decisions"].as_u64().unwrap_or(0) as usize;
    let page = &report["on_page"];
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        if rows == 0 {
            ui.label("No decisions match these filters.");
        } else {
            ui.strong(format!(
                "Decisions {}–{} of {total}",
                query.offset + 1,
                query.offset + rows
            ));
        }
        if report["comparison_package"].is_string() && rows > 0 {
            ui.small(format!(
                "{} changed · {} equal on this page",
                page["different_outputs"], page["equal_outputs"]
            ));
        }
        if ui
            .add_enabled(query.offset > 0, egui::Button::new("Newer").small())
            .clicked()
        {
            query.offset = query.offset.saturating_sub(query.page_size);
            changed = true;
        }
        if ui
            .add_enabled(
                query.offset + rows < total,
                egui::Button::new("Older").small(),
            )
            .clicked()
        {
            query.offset += query.page_size;
            changed = true;
        }
    });
    changed
}

fn decision_list(ui: &mut Ui, rows: &[Value], status: &Value, selected: &mut usize) {
    let width = ui.available_width();
    for (i, row) in rows.iter().enumerate() {
        let activity = relationships::work_name(status, s(row, "work"));
        let time = s(row, "accepted_utc")
            .get(11..19)
            .unwrap_or_else(|| s(row, "accepted_utc"));
        let changes = row["changed_outputs"]
            .as_array()
            .map(|v| format!(" · {} changed", v.len()))
            .unwrap_or_default();
        let events = row["decision_events"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let action = if events.iter().any(|e| e["kind"] == "delivery") {
            "Issued delivery"
        } else if events.iter().any(|e| e["kind"] == "reply") {
            "Sent reply"
        } else if events.iter().any(|e| e["kind"] == "message") {
            "Sent message"
        } else {
            events
                .iter()
                .find_map(|e| e["tool"].as_str())
                .unwrap_or_else(|| s(row, "judgment"))
        };
        let label = format!(
            "{}   {}{changes}\n{}",
            time,
            title(s(row, "basis")),
            title(action)
        );
        let button =
            egui::Button::selectable(*selected == i, label).min_size(egui::vec2(width, 36.));
        if ui
            .add(button)
            .on_hover_text(format!("{}\n{activity}", s(row, "accepted_utc")))
            .clicked()
        {
            *selected = i;
        }
    }
}

fn decision_card(
    ui: &mut Ui,
    report: &Value,
    row: &Value,
    status: &Value,
    action: &mut Option<Action>,
) {
    card(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        decision(ui, report, row, status, action);
    });
}
fn open_link(ui: &mut Ui, label: &str, reference: &Value, action: &mut Option<Action>) {
    if let Some(id) = reference.as_str() {
        if ui.small_button(label).on_hover_text(id).clicked() {
            *action = Some(Action::Open(id.into()));
        }
    }
}

fn paths(
    ui: &mut Ui,
    report: &Value,
    row: &Value,
    axis: Option<&str>,
    action: &mut Option<Action>,
) {
    if ui
        .small_button(if axis.is_some() {
            "Weight path"
        } else {
            "Inspect decision paths"
        })
        .clicked()
    {
        *action = Some(Action::Paths {
            query: character_paths::Query {
                decision: s(row, "decision").into(),
                axis: axis.map(str::to_owned),
                hidden_unit: None,
                compare: report["comparison_package"].as_str().map(str::to_owned),
            },
            accepted: s(row, "accepted_utc").into(),
        });
    }
}

fn decision(ui: &mut Ui, report: &Value, row: &Value, status: &Value, action: &mut Option<Action>) {
    let activity = relationships::work_name(status, s(row, "work"));
    ui.horizontal_wrapped(|ui| {
        theme::badge(ui, &title(s(row, "basis")), TEAL);
        ui.strong("Selected decision");
        ui.small(s(row, "accepted_utc"));
    });
    ui.label(excerpt(activity, 160)).on_hover_text(activity);
    ui.horizontal_wrapped(|ui| {
        open_link(ui, "Accepted decision", &row["decision"], action);
        for event in row["decision_events"].as_array().into_iter().flatten() {
            let label = match s(event, "kind") {
                "reply" => "Sent reply",
                "message" => "Sent message",
                "delivery" => "Issued delivery",
                _ => continue,
            };
            open_link(ui, label, &event["experience"], action);
        }
        open_link(ui, "Recorded input", &row["input"], action);
        open_link(ui, "Appraisal", &row["evaluation"], action);
        paths(ui, report, row, None, action);
    });
    if let Some(changes) = row["changed_outputs"].as_array() {
        ui.add_space(6.);
        if changes.is_empty() {
            ui.weak("All twelve inclination values agree on this input.");
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Changed inclinations");
                ui.colored_label(PURPLE, "● Before");
                ui.colored_label(
                    TEAL,
                    if report["subject"]["kind"] == "candidate" {
                        "● Candidate"
                    } else {
                        "● Supplied after"
                    },
                );
            });
            for change in changes {
                ui.push_id(s(change, "axis"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(title(s(change, "axis")));
                        let (a, b) = (
                            change["before"].as_f64().unwrap_or_default(),
                            change["after"].as_f64().unwrap_or_default(),
                        );
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(105., 18.), egui::Sense::hover());
                        let y = rect.center().y;
                        let x = |v: f64| {
                            rect.left() + 4. + ((v as f32 + 1.) / 2.) * (rect.width() - 8.)
                        };
                        let p = ui.painter();
                        p.line_segment(
                            [egui::pos2(x(-1.), y), egui::pos2(x(1.), y)],
                            Stroke::new(1., ui.visuals().widgets.noninteractive.bg_stroke.color),
                        );
                        p.line_segment(
                            [egui::pos2(x(0.), y - 4.), egui::pos2(x(0.), y + 4.)],
                            Stroke::new(1., ui.visuals().weak_text_color()),
                        );
                        p.line_segment(
                            [egui::pos2(x(a), y), egui::pos2(x(b), y)],
                            Stroke::new(2., TEAL),
                        );
                        p.circle_stroke(egui::pos2(x(a), y), 4., Stroke::new(1.5, PURPLE));
                        p.circle_filled(egui::pos2(x(b), y), 2.6, TEAL);
                        response.on_hover_text("Inclination scale from −1 through 0 to +1");
                        ui.monospace(format!("{a:+.4} → {b:+.4}"))
                            .on_hover_text(format!(
                                "Before {a:+.16}\nAfter {b:+.16}\nChange {:+.12e}",
                                b - a
                            ));
                        paths(ui, report, row, Some(s(change, "axis")), action);
                    });
                });
            }
        }
    }
    let linked = row["linked_responses"].as_array();
    ui.add_space(7.);
    ui.strong("Replies and reviews");
    if linked.is_none_or(|r| r.is_empty()) {
        ui.weak("No directly linked reply or review has been recorded.");
    }
    for response in linked.into_iter().flatten() {
        ui.horizontal_wrapped(|ui| {
            let label = format!(
                "{} · {}",
                title(s(response, "relation")),
                s(response, "author")
            );
            open_link(ui, &label, &response["experience"], action);
            open_link(ui, "In response to", &response["target"], action);
        });
    }
    ui.collapsing("Decision events and provider records", |ui| {
        ui.horizontal_wrapped(|ui| {
            open_link(ui, "Character use", &row["character_use"], action);
            open_link(ui, "Provider receipt", &row["receipt"], action);
            open_link(ui, "Final choice", &row["response"], action);
        });
        for event in row["decision_events"].as_array().into_iter().flatten() {
            let label = event["tool"].as_str().unwrap_or_else(|| s(event, "kind"));
            open_link(ui, &title(label), &event["experience"], action);
        }
    });
}
