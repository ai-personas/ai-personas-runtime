use super::*;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};

mod feedback;

#[derive(Clone)]
enum Basis {
    Matched(Option<String>),
    Preview { owner: String, work: String, input: String, accepted: String },
}

#[derive(Default)]
pub(super) struct Encounter {
    selection: Option<(String, Basis)>,
    pending: Option<Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
    tab: usize,
}

impl Encounter {
    pub(super) fn preview(&mut self, ctx: &egui::Context, root: &Path, choice: &Value) {
        self.load(ctx, root, format!("experience:{}@1", s(choice, "invocation")), Basis::Preview {
            owner: s(choice, "actor").to_owned(), work: s(choice, "work").to_owned(), input: s(choice, "request").to_owned(),
            accepted: s(choice, "accepted_utc").to_owned(),
        });
    }

    fn load(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        decision: String,
        basis: Basis,
    ) {
        self.tab = 1;
        self.selection = Some((decision.clone(), basis.clone()));
        self.result = None;
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let root = root.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match basis {
                Basis::Matched(matched) => data::comparison_encounter(&root, &decision, matched.as_deref()),
                Basis::Preview { owner, work, input, .. } => data::open(&root).and_then(|db| {
                    let preview = ai_personas::neural::replay_input(&db, &owner, ai_personas::neural::tools::ReplayInput {
                        decision, references: Vec::new(), reappraisal: None,
                    })?;
                    if preview["origin"]["original_input"] != input || preview["origin"]["work"] != work {
                        return Err(ai_personas::neural::failure("The input differs from the selected journal entry."));
                    }
                    Ok(preview)
                }),
            };
            let _ = sender.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, root: &Path, status: &Value) -> Option<String> {
        let (decision, basis) = self.selection.as_ref()?;
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
        let preview = matches!(basis, Basis::Preview { .. });
        let viewport = ctx.content_rect().shrink(6.);
        let room = (viewport.size() - theme::titled_window_chrome(ctx)).max(egui::vec2(1., 1.));
        egui::Window::new(if preview { "Comparison preview" } else { "Encounter record" }).open(&mut open)
            .default_size(egui::vec2(650., 450.).min(room)).min_size(egui::vec2(160., 64.).min(room))
            .max_size(room).constrain_to(viewport).collapsible(false)
            .show(ctx, |ui| {
                let Some(result) = &self.result else { ui.spinner(); return; };
                let report = match result {
                    Ok(report) => report,
                    Err(error) => { ui.strong("Encounter unavailable"); ui.label(error); refresh = ui.button("Try again").clicked(); return; }
                };
                if let Basis::Preview { owner, accepted, .. } = basis {
                    egui::ScrollArea::vertical().id_salt(("preview", decision)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        eyebrow(ui, "Decision journal  /  Comparison preview");
                        ui.horizontal_wrapped(|ui| {
                            let who = persona_label(status, owner);
                            avatar(ui, who, persona_color(owner), 24.);
                            ui.heading(format!("{who}’s encounter"));
                            ui.small(accepted);
                        });
                        ui.label("Default input for a new comparison of this decision. No model is called and no command is run.");
                        let count = report["available_resources"].as_array().map_or(0, Vec::len);
                        ui.weak(format!("Recorded appraisal · No extra resources selected ({count} views available)"));
                        ui.separator();
                        feedback::show(ui, status, &report["situation"]);
                        egui::CollapsingHeader::new("Brief, exchanges and other inputs").id_salt(decision)
                            .default_open(report["situation"].get("edition_feedback").is_none())
                            .show(ui, |ui| before_choice(ui, report, None, &mut inspect));
                        ui.collapsing("Input provenance", |ui| {
                            ui.small(format!("Replay projection {} · {decision}", report["replay_projection"]));
                            if count > 0 { ui.strong("Available to select, not added as resources"); }
                            for r in report["available_resources"].as_array().into_iter().flatten() {
                                let range = r.get("range").map(|v| format!(" · bytes {v}")).unwrap_or_default();
                                ui.add(egui::Label::new(format!("{} · {}{range}", s(r, "via"), s(r, "reference"))).wrap());
                            }
                            record_link(ui, "Inspect original input outside preview ↗", report["origin"]["original_input"].as_str(), &mut inspect);
                        });
                    });
                    return;
                }
                let Basis::Matched(matched) = basis else { return; };
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.tab, 0, "Before the choice");
                    ui.selectable_value(&mut self.tab, 1, "Actual response");
                    let count = report["feedback"].as_array().map_or(0, Vec::len);
                    ui.selectable_value(&mut self.tab, 2, format!("Linked feedback · {count}"));
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt(("encounter",decision,self.tab)).show(ui, |ui| {
                    match self.tab {
                        0 if matched.is_some() => {
                            ui.weak("Shared input retained for this sampled comparison, not a new preview.");
                            feedback::show(ui, status, &report["situation"]);
                            before_choice(ui, report, matched.as_deref(), &mut inspect);
                        }
                        0 => { ui.weak("This comparison has no retained shared-input object."); }
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
            self.load(ctx, root, decision.clone(), basis.clone());
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
    let scenario = &report["situation"];
    let images = report.get("images").or_else(|| report.get("ordered_image_blobs"))
        .and_then(Value::as_array)
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
    for result in scenario["local_command_results"].as_array().into_iter().flatten().filter(|r| !r["receiving"].is_object()) {
        ui.push_id(&result["experience"].as_str(), |ui| {
            ui.collapsing(format!("Observed command · {} · exit {}", s(result,"argv0"), result["exit_code"]), |ui| {
                feedback::streams(ui, result);
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
                                                        Basis::Matched(case["matched_input"].as_str().map(str::to_owned)),
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
