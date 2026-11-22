use super::*;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Default)]
pub(super) struct Encounter {
    selection: Option<(String, Option<String>)>,
    pending: Option<Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
    tab: usize,
}

impl Encounter {
    fn load(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        decision: String,
        matched: Option<String>,
    ) {
        self.tab = 1;
        self.selection = Some((decision.clone(), matched.clone()));
        self.result = None;
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let root = root.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = data::comparison_encounter(&root, &decision, matched.as_deref());
            let _ = sender.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn show(&mut self, ctx: &egui::Context, root: &Path) -> Option<String> {
        let (decision, matched) = self.selection.as_ref()?;
        if let Some(reader) = &self.pending {
            match reader.try_recv() {
                Ok(value) => {
                    self.result = Some(value);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.result = Some(Err("The encounter reader stopped.".into()));
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut open = true;
        let mut inspect = None;
        let mut refresh = false;
        egui::Window::new("Encounter record").open(&mut open).default_width(650.).default_height(450.)
            .show(ctx, |ui| {
                let Some(result) = &self.result else { ui.spinner(); return; };
                let report = match result {
                    Ok(report) => report,
                    Err(error) => { ui.label(error); refresh = ui.button("Try again").clicked(); return; }
                };
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.tab, 0, "Before the choice");
                    ui.selectable_value(&mut self.tab, 1, "Actual response");
                    let count = report["feedback"].as_array().map_or(0, Vec::len);
                    ui.selectable_value(&mut self.tab, 2, format!("Linked feedback · {count}"));
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt(("encounter",decision,self.tab)).show(ui, |ui| {
                    match self.tab {
                        0 => before_choice(ui, report, matched.as_deref(), &mut inspect),
                        1 => actual_response(ui, report, decision, &mut inspect),
                        _ => {
                            let responses = report["feedback"].as_array().map(Vec::as_slice).unwrap_or_default();
                            if responses.is_empty() { ui.weak("No direct response or delivery review is linked to this decision."); }
                            for event in responses { exchange(ui, event, &mut inspect); }
                            refresh = ui.small_button("Refresh linked feedback").clicked();
                        }
                    }
                });
            });
        if !open || inspect.is_some() {
            self.selection = None;
            self.pending = None;
            self.result = None;
        } else if refresh {
            let tab = self.tab;
            self.load(ctx, root, decision.clone(), matched.clone());
            self.tab = tab;
        }
        inspect
    }
}

fn record_link(ui: &mut Ui, label: &str, reference: Option<&str>, inspect: &mut Option<String>) {
    if let Some(reference) = reference {
        if ui.small_button(label).clicked() {
            *inspect = Some(reference.to_owned());
        }
    }
}

fn attributed_text(ui: &mut Ui, heading: String, text: Option<&str>, time: Option<i64>) {
    if let Some(text) = text {
        ui.add_space(5.);
        ui.horizontal_wrapped(|ui| {
            ui.strong(heading);
            if let Some(time) = time {
                ui.weak(time_label(time));
            }
        });
        ui.label(text);
    }
}

fn before_choice(ui: &mut Ui, report: &Value, matched: Option<&str>, inspect: &mut Option<String>) {
    if matched.is_none() {
        ui.weak("This comparison has no retained shared-input object.");
        return;
    }
    let scenario = &report["situation"];
    let images = report["images"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (index, image) in images.iter().enumerate() {
        if let Some(digest) = image[0].as_str() {
            let label = if images.len() == 1 {
                "Open shared image ↗".to_owned()
            } else {
                format!("Open shared image {} ↗", index + 1)
            };
            if ui.button(label).clicked() {
                *inspect = Some(format!("object:{digest}"));
            }
        }
    }
    attributed_text(
        ui,
        "Opening brief".into(),
        scenario["original_need"].as_str(),
        None,
    );
    for amendment in scenario["amendments"].as_array().into_iter().flatten() {
        attributed_text(
            ui,
            format!("Brief amendment · {}", s(amendment, "author")),
            amendment["text"].as_str(),
            amendment["time"].as_i64(),
        );
    }
    let previous = &scenario["previous_message_to_user"];
    attributed_text(
        ui,
        format!("Previously sent to the user · {}", s(previous, "author")),
        previous["record"]["text"].as_str(),
        previous["created"].as_i64(),
    );
    for e in scenario["incoming_exchanges"]
        .as_array()
        .into_iter()
        .flatten()
    {
        ui.push_id(e["experience"].as_str().unwrap_or_default(), |ui| {
            let text = exchange_text(&e["record"]).or(e["text"].as_str());
            attributed_text(
                ui,
                format!("{} · {}", title(s(e, "kind")), s(e, "from")),
                text,
                e["time"].as_i64(),
            );
            record_link(ui, "Open exchange ↗", e["experience"].as_str(), inspect);
        });
    }
    for result in scenario["local_command_results"].as_array().into_iter().flatten() {
        ui.push_id(&result["experience"].as_str(), |ui| {
            ui.collapsing(format!("Observed command · {} · exit {}", s(result,"argv0"), result["exit_code"]), |ui| {
                for name in ["stdout", "stderr"] {
                    let stream = &result[name];
                    if let Some(text) = stream["text"].as_str().or(stream["excerpt"].as_str()) {
                        let scope = if stream["text"].is_string() || stream["excerpt_complete"] == true { "complete" } else { "excerpt" };
                        ui.small(format!("{name} · {scope}"));
                        if !text.is_empty() { ui.add(egui::Label::new(text).wrap()); }
                    } else if stream["bytes"].is_number() { ui.small(format!("{name} · {} bytes, no text supplied", stream["bytes"])); }
                }
                record_link(ui, "Original command result ↗", result["experience"].as_str(), inspect);
            });
        });
    }
    let note = &scenario["prior_working_note"];
    attributed_text(
        ui,
        format!("Working note · {}", s(note, "author")),
        note["text"].as_str(),
        note["time"].as_i64(),
    );
    attributed_text(
        ui,
        "Purpose at this decision".into(),
        scenario["purpose_at_decision"].as_str(),
        None,
    );
    record_link(ui, "Full shared input ↗", matched, inspect);
}

fn actual_response(ui: &mut Ui, report: &Value, decision: &str, inspect: &mut Option<String>) {
    section(ui, "Accepted decision", s(report, "actor"));
    if let Some(reason) = report["work"]["reason"].as_str() {
        ui.label(reason);
    }
    let outgoing = report["outgoing"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if outgoing.is_empty() {
        ui.weak("No outgoing communication or delivery on this decision.");
    }
    for event in outgoing {
        exchange(ui, event, inspect);
    }
    ui.horizontal_wrapped(|ui| {
        record_link(ui, "Full decision ↗", Some(decision), inspect);
        record_link(
            ui,
            "Original provider receipt ↗",
            report["receipt"].as_str(),
            inspect,
        );
    });
}

fn exchange_text(body: &Value) -> Option<&str> {
    [
        "text",
        "contribution",
        "description",
        "commitment",
        "reason",
        "title",
        "need",
    ]
    .into_iter()
    .find_map(|key| body[key].as_str())
    .or(body["decision"]["reason"].as_str())
}

fn choose_case<'a>(ui: &mut Ui, cases: &'a [Value]) -> Option<(usize, &'a Value)> {
    let mut index = 0;
    if cases.len() > 1 {
        let id = ui.make_persistent_id("comparison-case");
        index = ui
            .data(|state| state.get_temp::<usize>(id))
            .unwrap_or(0)
            .min(cases.len() - 1);
        ui.horizontal_wrapped(|ui| {
            for (i, case) in cases.iter().enumerate() {
                let label = match case["image_count"].as_u64().unwrap_or(0) {
                    0 => format!("Case {}", i + 1),
                    1 => format!("Case {} · image", i + 1),
                    n => format!("Case {} · {n} images", i + 1),
                };
                ui.selectable_value(&mut index, i, label);
            }
        });
        ui.data_mut(|state| state.insert_temp(id, index));
    }
    cases.get(index).map(|case| (index, case))
}

fn exchange(ui: &mut Ui, event: &Value, inspect: &mut Option<String>) {
    ui.add_space(6.);
    ui.strong(format!(
        "{} → {} · {}",
        s(event, "actor"),
        s(event, "target"),
        title(s(event, "kind"))
    ));
    let body = &event["body"];
    if let Some(text) = exchange_text(body) {
        ui.label(text);
    }
    if let Some(reference) = event["reference"].as_str() {
        if ui.small_button("Open record ↗").clicked() {
            *inspect = Some(reference.to_owned());
        }
    }
}

impl App {
    pub(super) fn comparisons(&mut self, ui: &mut Ui) {
        let records: Vec<_> = self
            .snapshot
            .assessments
            .iter()
            .filter(|r| r["owner"] == self.persona)
            .cloned()
            .collect();
        section(
            ui,
            "Behavioral comparisons",
            &format!("{} recorded", records.len()),
        );
        if records.is_empty() {
            ui.weak("This persona has no recorded comparison.");
            return;
        }
        let current = records
            .iter()
            .filter(|r| r["kind"] == "current_character")
            .count();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.comparison_filter, 0, "All");
            ui.selectable_value(
                &mut self.comparison_filter,
                1,
                format!("Current character · {current}"),
            );
            ui.selectable_value(
                &mut self.comparison_filter,
                2,
                format!("Proposals · {}", records.len() - current),
            );
        });
        let mut index = 0;
        egui::ScrollArea::vertical()
            .id_salt(("comparisons", &self.persona))
            .show(ui, |ui| {
                for record in records {
                    let current = record["kind"] == "current_character";
                    if (self.comparison_filter == 1 && !current)
                        || (self.comparison_filter == 2 && current)
                    {
                        continue;
                    }
                    let name = if current {
                        "Current character and ordinary reasoning"
                    } else {
                        self.snapshot
                            .candidates
                            .iter()
                            .find(|c| c["reference"] == record["candidate"])
                            .and_then(|c| c["body"]["intent"].as_str())
                            .unwrap_or("Character proposal")
                    };
                    let heading = excerpt(name, 94);
                    ui.push_id(s(&record, "reference"), |ui| {
                        egui::CollapsingHeader::new(heading)
                            .default_open(index == 0)
                            .show(ui, |ui| {
                                if let Some(reason) = record["interruption"].as_str() {
                                    ui.label(format!("Incomplete comparison · {reason}"));
                                }
                                let cases = record["cases"]
                                    .as_array()
                                    .map(Vec::as_slice)
                                    .unwrap_or_default();
                                if let Some((i, case)) = choose_case(ui, cases) {
                                    ui.push_id(i, |ui| {
                                        ui.add_space(6.);
                                        if let Some(decision) = case["origin"]["decision"].as_str()
                                        {
                                            ui.horizontal_wrapped(|ui| {
                                                let label = if case["origin"]["kind"] == "prior_encounter_reappraised" { "Prior encounter · retrospective appraisal" } else { "Prior encounter" };
                                                theme::badge(ui, label, theme::ACCENT);
                                                if ui
                                                    .small_button("View encounter & feedback")
                                                    .clicked()
                                                {
                                                    self.encounter.load(
                                                        ui.ctx(),
                                                        &self.root,
                                                        decision.to_owned(),
                                                        case["matched_input"]
                                                            .as_str()
                                                            .map(str::to_owned),
                                                    );
                                                }
                                            });
                                        } else {
                                            ui.strong("Hypothetical situation");
                                            if let Some(text) = case["situation"].as_str() {
                                                ui.label(text);
                                            }
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            if let Some(n) =
                                                case["image_count"].as_u64().filter(|n| *n > 0)
                                            {
                                                ui.small(format!("{n} attached image(s) per arm"));
                                            }
                                            if let Some(r) = case["matched_input"].as_str() {
                                                if ui
                                                    .small_button("Inspect shared inputs ↗")
                                                    .clicked()
                                                {
                                                    self.open_resource(ui.ctx(), r.into());
                                                }
                                            }
                                        });
                                        let arms = case["arms"]
                                            .as_array()
                                            .map(Vec::as_slice)
                                            .unwrap_or(&[]);
                                        if let (Some(left), Some(right)) = (
                                            arms.first().and_then(|a| a["outputs"].as_object()),
                                            arms.get(1).and_then(|a| a["outputs"].as_object()),
                                        ) {
                                            let changes = left
                                                .iter()
                                                .filter_map(|(axis, a)| {
                                                    let a = a.as_f64()?;
                                                    let b = right.get(axis)?.as_f64()?;
                                                    (a != b).then(|| {
                                                        format!("{} {a:+.3} → {b:+.3}", title(axis))
                                                    })
                                                })
                                                .collect::<Vec<_>>();
                                            ui.small(if changes.is_empty() {
                                                "Equal numerical outputs for this situation".into()
                                            } else {
                                                changes.join("   ·   ")
                                            });
                                        }
                                        ui.add_space(5.);
                                        if !arms.is_empty()
                                            && ui.available_width() >= 240. * arms.len() as f32
                                        {
                                            ui.columns(arms.len(), |columns| {
                                                for (column, arm) in columns.iter_mut().zip(arms) {
                                                    column.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                                        self.comparison_arm(ui, arm);
                                                    });
                                                }
                                            });
                                        } else {
                                            for arm in arms {
                                                self.comparison_arm(ui, arm);
                                            }
                                        }
                                    });
                                }
                                if ui.small_button("Open complete assessment ↗").clicked() {
                                    self.open_resource(ui.ctx(), s(&record, "reference").into());
                                }
                            });
                    });
                    ui.separator();
                    index += 1;
                }
                if index == 0 {
                    ui.weak("No comparisons in this view.");
                }
            });
        if let Some(reference) = self.encounter.show(ui.ctx(), &self.root) {
            self.open_resource(ui.ctx(), reference);
        }
    }

    fn comparison_arm(&mut self, ui: &mut Ui, arm: &Value) {
        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (label, color) = match arm["arm"].as_str() {
                Some("active") => ("Active inclinations", theme::ACCENT),
                Some("candidate") => ("Proposed character", PURPLE),
                _ => ("Numerical inclinations omitted", theme::palette(ui).muted),
            };
            theme::badge(ui, label, color);
            if let Some(error) = arm["error"].as_str() {
                ui.label(error);
            }
            for (field, label) in [
                ("judgment", "Judgment"),
                ("proposed_action", "Proposed action"),
                ("communication", "Communication"),
                ("remaining_uncertainty", "Remaining uncertainty"),
            ] {
                if let Some(text) = arm["response"][field].as_str() {
                    ui.add_space(6.);
                    ui.strong(label);
                    ui.add(egui::Label::new(text).wrap());
                }
            }
            if let Some(r) = arm["receipt"].as_str() {
                if ui.small_button("Provider receipt ↗").clicked() {
                    self.open_resource(ui.ctx(), r.into());
                }
            }
        });
    }
}
