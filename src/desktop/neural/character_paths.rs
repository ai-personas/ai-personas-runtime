use super::*;
use ai_personas::neural::character_paths::{inspect, Query};
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

#[derive(Default)]
pub(super) struct Window {
    query: Option<Query>,
    owner: String,
    accepted: String,
    pending: Option<Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
}

impl Window {
    pub(super) fn open(&mut self, ctx: &egui::Context, root: &Path, decision: &Value) {
        self.open_query(
            ctx,
            root,
            s(decision, "actor"),
            s(decision, "accepted_utc"),
            Query {
                decision: format!("experience:{}@1", s(decision, "invocation")),
                axis: None,
                hidden_unit: None,
                compare: None,
            },
        );
    }

    pub(super) fn open_query(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        owner: &str,
        accepted: &str,
        query: Query,
    ) {
        self.owner = owner.into();
        self.accepted = accepted.into();
        self.query = Some(query);
        self.load(ctx, root);
    }

    fn load(&mut self, ctx: &egui::Context, root: &Path) {
        let Some(query) = self.query.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.result = None;
        let (ctx, root, owner) = (ctx.clone(), root.to_owned(), self.owner.clone());
        std::thread::spawn(move || {
            let result = data::open(&root).and_then(|db| inspect(&db, Some(&owner), &query));
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        candidates: &[Value],
    ) -> Option<String> {
        let query = self.query.as_mut()?;
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.result = Some(result);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.result = Some(Err("The decision reader stopped.".into()));
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut visible = true;
        let mut refresh = false;
        let mut open = None;
        let contract = FeatureContract::standard();
        egui::Window::new("Decision paths")
            .id(egui::Id::new("decision-paths"))
            .open(&mut visible)
            .default_width(710.)
            .default_height(530.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading(title(&self.owner));
                    ui.weak(&self.accepted);
                    if let Some(Ok(report)) = &self.result {
                        theme::badge(ui, &title(s(report, "basis")), TEAL);
                        ui.small(format!("Epoch {}", report["epoch"]));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("paths-axis")
                        .width(168.)
                        .selected_text(
                            query
                                .axis
                                .as_deref()
                                .map(title)
                                .unwrap_or_else(|| "All inclinations".into()),
                        )
                        .show_ui(ui, |ui| {
                            let mut changed = ui
                                .selectable_value(&mut query.axis, None, "All inclinations")
                                .changed();
                            for axis in &contract.outputs {
                                changed |= ui
                                    .selectable_value(
                                        &mut query.axis,
                                        Some(axis.id.clone()),
                                        title(&axis.id),
                                    )
                                    .on_hover_text(&axis.meaning)
                                    .changed();
                            }
                            if changed {
                                query.hidden_unit = None;
                                refresh = true;
                            }
                        });
                    let choices = candidates
                        .iter()
                        .filter(|c| c["body"]["owner"] == self.owner);
                    let caption = |c: &Value| {
                        let intent = s(&c["body"], "intent");
                        format!(
                            "{} · {}",
                            excerpt(intent, 28),
                            s(&c["body"], "model").chars().take(8).collect::<String>()
                        )
                    };
                    let comparison = query.compare.as_deref();
                    let epoch = self.result.as_ref().and_then(|r| r.as_ref().ok())
                        .and_then(|r| r["epoch"].as_u64());
                    let selected = choices.clone().find(|c| {
                        comparison == Some(s(c, "reference")) || comparison
                            .and_then(|v| v.strip_prefix("object:"))
                            .is_some_and(|v| c["body"]["model"] == v)
                    });
                    egui::ComboBox::from_id_salt("paths-package")
                        .width(236.)
                        .selected_text(
                            if comparison == Some("previous-epoch") {
                                "Previous active epoch".into()
                            } else {
                                selected.map(&caption)
                                    .or_else(|| query.compare.as_deref().map(|p| excerpt(p,25)))
                                    .unwrap_or_else(|| "Compare another package…".into())
                            },
                        )
                        .show_ui(ui, |ui| {
                            refresh |= ui
                                .selectable_value(&mut query.compare, None, "Supplied package only")
                                .changed();
                            if epoch.is_some_and(|n| n > 1) || query.compare.as_deref() == Some("previous-epoch") {
                                refresh |= ui.selectable_value(
                                    &mut query.compare, Some("previous-epoch".into()), "Previous active epoch",
                                ).on_hover_text("Use the package in effect immediately before this recorded epoch.").changed();
                            }
                            for c in choices {
                                refresh |= ui
                                    .selectable_value(
                                        &mut query.compare,
                                        Some(s(c, "reference").into()),
                                        caption(c),
                                    )
                                    .on_hover_text(s(&c["body"], "intent"))
                                    .changed();
                            }
                        });
                });
                ui.separator();
                match &self.result {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Verifying the recorded input…");
                        });
                    }
                    Some(Err(error)) => {
                        ui.colored_label(theme::ACCENT, error);
                        refresh |= ui.button("Retry").clicked();
                    }
                    Some(Ok(report)) => {
                        let supplied = &report["supplied"]["paths"];
                        let alternate = &report["alternate"]["paths"];
                        if !alternate.is_null() {
                            let prior = &report["alternate"]["previous"];
                            if let (Some(epoch), Some(change)) = (prior["epoch"].as_u64(), prior["change"].as_str()) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.strong(format!("Compared with epoch {epoch}"));
                                    if ui.link("Open switch record").clicked() { open = Some(change.into()); }
                                });
                            }
                            let count = report["alternate"]["changed_outputs"]
                                .as_array()
                                .map_or(0, Vec::len);
                            ui.small(format!(
                                "{count} of {} inclinations change on this recorded appraisal.",
                                contract.outputs.len()
                            ));
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("paths-body")
                            .show(ui, |ui| {
                                let choice = &report["choice"];
                                let judgment = s(&choice["judgment"], "reason");
                                let caption = choice["reply"].as_str().unwrap_or(judgment);
                                ui.collapsing(
                                    format!(
                                        "Recorded choice · {}",
                                        excerpt(caption.lines().next().unwrap_or(""), 58)
                                    ),
                                    |ui| {
                                        if let Some(reply) = choice["reply"].as_str() {
                                            ui.label(reply);
                                        }
                                        if !judgment.is_empty() {
                                            ui.weak(judgment);
                                        }
                                        if ui.small_button("Open exact response").clicked() {
                                            open = choice["response"].as_str().map(str::to_owned);
                                        }
                                    },
                                );
                                if let Some(axis) = query.axis.as_deref() {
                                    if let Some(axis) =
                                        contract.outputs.iter().find(|a| a.id == axis)
                                    {
                                        ui.weak(&axis.meaning);
                                    }
                                    let units = supplied["hidden_units"]
                                        .as_u64()
                                        .unwrap_or(0)
                                        .max(alternate["hidden_units"].as_u64().unwrap_or(0));
                                    if units > 0 {
                                        ui.horizontal_wrapped(|ui| {
                                            let mut focus = query.hidden_unit.is_some();
                                            if ui
                                                .checkbox(&mut focus, "Inspect one hidden unit")
                                                .changed()
                                            {
                                                query.hidden_unit = focus.then_some(0);
                                                refresh = true;
                                            }
                                            if let Some(unit) = query.hidden_unit.as_mut() {
                                                refresh |= ui
                                                    .add(
                                                        egui::DragValue::new(unit)
                                                            .range(0..=units as usize - 1),
                                                    )
                                                    .changed();
                                            }
                                        });
                                    }
                                    path_card(
                                        ui,
                                        "Supplied to the decision",
                                        supplied,
                                        TEAL,
                                        query.hidden_unit,
                                    );
                                    if !alternate.is_null() {
                                        path_card(
                                            ui,
                                            "Local comparison",
                                            alternate,
                                            PURPLE,
                                            query.hidden_unit,
                                        );
                                    }
                                } else {
                                    egui::Grid::new("paths-outputs")
                                        .num_columns(if alternate.is_null() { 3 } else { 4 })
                                        .striped(true)
                                        .min_col_width(60.)
                                        .show(ui, |ui| {
                                            ui.strong("Inclination");
                                            ui.strong("Supplied");
                                            ui.label("");
                                            if !alternate.is_null() {
                                                ui.strong("Compared");
                                            }
                                            ui.end_row();
                                            for axis in &contract.outputs {
                                                if ui
                                                    .link(title(&axis.id))
                                                    .on_hover_text(&axis.meaning)
                                                    .clicked()
                                                {
                                                    query.axis = Some(axis.id.clone());
                                                    refresh = true;
                                                }
                                                let value = supplied["outputs"][&axis.id]
                                                    .as_f64()
                                                    .unwrap_or_default();
                                                ui.monospace(format!("{value:+.4}"));
                                                inclination_bar(ui, value);
                                                if !alternate.is_null() {
                                                    let other = alternate["outputs"][&axis.id]
                                                        .as_f64()
                                                        .unwrap_or_default();
                                                    ui.label(
                                                        RichText::new(format!("{other:+.4}"))
                                                            .monospace()
                                                            .color(if other != value {
                                                                PURPLE
                                                            } else {
                                                                ui.visuals().weak_text_color()
                                                            }),
                                                    );
                                                }
                                                ui.end_row();
                                            }
                                        });
                                }
                                ui.add_space(8.);
                                ui.horizontal_wrapped(|ui| {
                                    for (key, label) in [
                                        ("character_use", "Character receipt"),
                                        ("evaluation", "Appraisal & evidence"),
                                        ("provider_receipt", "Provider receipt"),
                                    ] {
                                        if let Some(r) = report[key].as_str() {
                                            if ui.small_button(label).clicked() {
                                                open = Some(r.to_owned());
                                            }
                                        }
                                    }
                                });
                            });
                    }
                }
            });
        if !visible || open.is_some() {
            self.query = None;
            self.pending = None;
            self.result = None;
        } else if refresh {
            self.load(ctx, root);
        }
        open
    }
}

fn inclination_bar(ui: &mut Ui, value: f64) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(76., 12.), egui::Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        center.y,
        Stroke::new(1., ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    painter.vline(
        center.x,
        rect.y_range(),
        Stroke::new(1., ui.visuals().weak_text_color()),
    );
    let end = center + egui::vec2(value as f32 * rect.width() / 2., 0.);
    painter.line_segment(
        [center, end],
        Stroke::new(3., if value < 0. { theme::ACCENT } else { TEAL }),
    );
}

fn path_card(ui: &mut Ui, caption: &str, paths: &Value, color: Color32, focus: Option<usize>) {
    let axis = &paths["selected"];
    let total = &axis["total"];
    ui.push_id(caption, |ui| {
        card(ui).show(ui, |ui| {
            section(ui, caption, &format!("Output row {}", axis["row"]));
            let number = |item: &Value| format!("{:+.5}", item.as_f64().unwrap_or_default());
            ui.horizontal_wrapped(|ui| {
                for (name, key) in [
                    ("Bias", "bias"),
                    ("Direct", "direct"),
                    ("Hidden", "interaction"),
                ] {
                    ui.label(format!("{name} {}", number(&total[key])))
                        .on_hover_text(total[key].to_string());
                }
                ui.strong(format!("→ tanh({})", number(&axis["preactivation"])))
                    .on_hover_text(format!(
                        "tanh({}) = {}",
                        axis["preactivation"], total["value"]
                    ));
                theme::badge(ui, &number(&total["value"]), color);
            });
            if axis["tanh_at_limit"] == true {
                ui.small("tanh rounded to its numerical limit.");
            }
            if let Some(terms) = axis["direct_terms"].as_array().filter(|v| !v.is_empty()) {
                ui.collapsing(format!("Direct input terms · {}", terms.len()), |ui| {
                    input_terms(ui, terms)
                });
            }
            let units = axis["hidden_paths"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            if focus.is_some_and(|i| i as u64 >= paths["hidden_units"].as_u64().unwrap_or(0)) {
                ui.weak("This package has no unit at the selected index.");
            } else if units.is_empty() {
                ui.weak("No hidden unit connects to this output.");
            }
            for unit in units {
                let status = if unit["output_weight"].as_f64() == Some(0.) {
                    "disconnected"
                } else if unit["activation"].as_f64() == Some(0.) {
                    "inactive"
                } else {
                    "active"
                };
                ui.collapsing(
                    format!(
                        "Unit {} · {status} · contribution {}",
                        unit["unit"],
                        number(&unit["contribution"])
                    ),
                    |ui| {
                        ui.monospace(format!(
                            "Bias {}     Sum {}     ReLU {}     Output weight {}",
                            number(&unit["bias"]),
                            number(&unit["preactivation"]),
                            number(&unit["activation"]),
                            number(&unit["output_weight"])
                        ));
                        if let Some(terms) = unit["inputs"].as_array() {
                            input_terms(ui, terms);
                        }
                    },
                );
            }
        })
    });
    ui.add_space(6.);
}

fn input_terms(ui: &mut Ui, terms: &[Value]) {
    for t in terms {
        let state = if t["known"] == true {
            "known"
        } else {
            "unknown"
        };
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "Column {} · {} · {} ({state})",
                t["column"],
                title(s(t, "axis")),
                s(t, "channel")
            ));
            ui.monospace(format!(
                "{} × {} = {}",
                t["weight"], t["input"], t["product"]
            ));
        });
    }
}
