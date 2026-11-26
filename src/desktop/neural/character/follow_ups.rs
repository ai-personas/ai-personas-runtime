use super::*;
use ai_personas::neural::{character_paths::Query, revision_uses::inspect_follow_up};
use std::{path::Path, sync::OnceLock};

type Read = Arc<OnceLock<Result<Value, String>>>;

// The question revision and entry index identify immutable input. Only the
// selected entry of an expanded question starts a read.
fn comparison(ui: &Ui, root: &Path, owner: &str, question: &str, index: usize) -> (egui::Id, Read) {
    let id = egui::Id::new(("development-follow-up", root, owner, question, index));
    let result = ui.data_mut(|data| {
        data.get_temp_mut_or_insert_with(id, || {
            let result = Read::default();
            let slot = result.clone();
            let (root, owner, question, ctx) = (
                root.to_owned(),
                owner.to_owned(),
                question.to_owned(),
                ui.ctx().clone(),
            );
            std::thread::spawn(move || {
                let read = super::data::open(&root)
                    .and_then(|db| inspect_follow_up(&db, &owner, &question, index));
                let _ = slot.set(read.map_err(|e| e.to_string()));
                ctx.request_repaint();
            });
            result
        })
        .clone()
    });
    (id, result)
}

fn relation(value: &str) -> String {
    match value {
        "later_in_activity" => "Later event in the same activity".into(),
        "later_elsewhere" => "Later event in another activity".into(),
        "direct_reply" => "Direct reply to this choice".into(),
        "outside_report" => "Human report on the original request".into(),
        "operation_result" => "Result of this choice’s operation".into(),
        "delivery_feedback" => "Feedback on the issued package".into(),
        "receiving_check" => "Receiving run on the issued package".into(),
        "delivery_review" | "requester_review" => "Review of the issued package".into(),
        "asset_feedback" => "Feedback on the exact file".into(),
        "feedback_reply" => "Reply to the file feedback".into(),
        value => title(value),
    }
}

fn origin(report: &Value) -> (String, bool) {
    let kind = s(report, "outcome_kind");
    if report["outcome_invocation"].is_null() {
        match kind {
            "user_message" => return ("Human message".into(), true),
            "user_acceptance" => return ("Human review".into(), true),
            "requirements_amended" => return ("Human amendment".into(), true),
            "human_request_progress" => return ("Human update".into(), true),
            _ => {}
        }
    }
    (title(kind), false)
}

fn paragraph(ui: &mut Ui, label: &str, value: &Value) {
    if let Some(text) = value.as_str().filter(|s| !s.trim().is_empty()) {
        ui.small(RichText::new(label).strong());
        ui.add(egui::Label::new(text).wrap());
    }
}

impl App {
    pub(super) fn development_follow_ups(&mut self, ui: &mut Ui, question: &str, body: &Value) {
        let rows = body["follow_ups"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        if !rows.is_empty() {
            ui.add_space(5.);
            let selection = ui.make_persistent_id(("selected-development-follow-up", question));
            let mut index = ui
                .data(|d| d.get_temp::<usize>(selection))
                .unwrap_or(rows.len() - 1)
                .min(rows.len() - 1);
            ui.horizontal_wrapped(|ui| {
                ui.strong("Follow-up after adoption");
                if rows.len() > 1 {
                    if ui
                        .add_enabled(index > 0, egui::Button::new("Earlier"))
                        .clicked()
                    {
                        index -= 1;
                    }
                    ui.small(format!("{} of {}", index + 1, rows.len()));
                    if ui
                        .add_enabled(index + 1 < rows.len(), egui::Button::new("Later"))
                        .clicked()
                    {
                        index += 1;
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(selection, index));
            ui.push_id((question, index), |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.development_follow_up(ui, question, body, index, &rows[index]);
                });
            });
        }
        for rollback in body["rollbacks"].as_array().into_iter().flatten() {
            ui.horizontal_wrapped(|ui| {
                ui.small("Recorded rollback");
                if let Some(call) = rollback["decision"].as_str() {
                    let reference = format!("experience:{call}@1");
                    if ui
                        .link("Rollback decision")
                        .on_hover_text(&reference)
                        .clicked()
                    {
                        self.open_resource(ui.ctx(), reference);
                    }
                }
            });
        }
        if !rows.is_empty() {
            ui.add_space(6.);
        }
    }

    fn development_follow_up(
        &mut self,
        ui: &mut Ui,
        question: &str,
        body: &Value,
        index: usize,
        row: &Value,
    ) {
        let observation = &row["observation"];
        let note = &row["interpretation"];
        let author = s(observation, "judgment_author");
        let (id, read) = comparison(ui, &self.root, s(body, "owner"), question, index);
        ui.horizontal_wrapped(|ui| {
            if !author.is_empty() && matches!(read.get(), Some(Ok(_))) {
                ui.strong(format!("{}’s judgment", self.label(author)));
            } else {
                ui.strong("Recorded judgment");
                if !author.is_empty() {
                    ui.small(format!("Attributed to {}", self.label(author)));
                }
            }
            if let Some(finding) = row["disposition"].as_str() {
                theme::badge(ui, &title(finding), PURPLE);
            }
        });
        if let Some(text) = note["observed_behavior"].as_str() {
            ui.add(egui::Label::new(text).wrap());
        }
        ui.add_space(4.);
        match read.get() {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Checking the recorded choice and outcome…");
                });
            }
            Some(Err(error)) => {
                ui.weak("The link between this judgment, its numerical input and its outcome could not be verified.");
                ui.collapsing("Verification details", |ui| {
                    ui.label(error);
                });
                if ui.small_button("Retry verification").clicked() {
                    ui.data_mut(|d| d.remove::<Read>(id));
                    ui.ctx().request_repaint();
                }
            }
            Some(Ok(report)) => self.follow_up_evidence(ui, body, row, report),
        }
        ui.collapsing("Scope, retained strengths and other explanations", |ui| {
            paragraph(ui, "Scope", &note["scope"]);
            paragraph(ui, "Retained strengths", &note["retained_strengths"]);
            paragraph(ui, "Other explanations", &row["alternative_explanations"]);
            for (label, work) in [
                ("Later choice’s activity", &observation["decision_work"]),
                ("Judgment’s activity", &row["work"]),
            ] {
                if let Some(work) = work.as_str() {
                    let name = self.work_name(work);
                    ui.horizontal_wrapped(|ui| {
                        ui.small(label);
                        if ui.link(excerpt(&name, 70)).on_hover_text(&name).clicked() {
                            self.open_home_activity(work);
                        }
                    });
                }
            }
        });
    }

    fn follow_up_evidence(&mut self, ui: &mut Ui, body: &Value, row: &Value, report: &Value) {
        let o = &row["observation"];
        let choice = format!("experience:{}@1", s(o, "decision"));
        let outcome = s(o, "outcome");
        let sequence = outcome
            .strip_prefix("experience:event-")
            .and_then(|r| r.strip_suffix("@1"))
            .and_then(|r| r.parse::<i64>().ok());
        ui.horizontal_wrapped(|ui| {
            let (origin, human) = origin(report);
            let caption = match sequence {
                Some(n) => format!("{origin} #{n}"),
                None => origin,
            };
            if ui.link(caption).on_hover_text(outcome).clicked() {
                if let (Some(n), Some(work)) = (sequence, o["outcome_work"].as_str()) {
                    self.open_home_event(work, n);
                } else {
                    self.open_resource(ui.ctx(), outcome.to_owned());
                }
            }
            if let Some(by) = o["outcome_author"].as_str() {
                let by = if human { by } else { self.label(by) };
                ui.small(format!(
                    "from {by} · {}",
                    relation(s(report, "outcome_relation"))
                ));
            } else {
                ui.small(relation(s(report, "outcome_relation")));
            }
            if let Some(target) = report["linked_target"].as_str() {
                if ui.link("Linked record").on_hover_text(target).clicked() {
                    self.open_resource(ui.ctx(), target.to_owned());
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let basis = match report["basis"].as_str() {
                Some("baseline") => "Baseline input",
                Some("contextual") => "Contextual input",
                _ => "Recorded input",
            };
            let mut summary = report["epoch"].as_u64().map(|e|format!("Epoch {e} · {basis}")).unwrap_or_else(||basis.to_owned());
            if let Some(changes) = report["changed_outputs"].as_array() {
                let number = counted(changes.len() as u64, "inclination", "inclinations");
                summary.push_str(&format!(" · {number} changed on the same input"));
            }
            ui.add(egui::Label::new(RichText::new(summary).small()).wrap());
            if report["changed_outputs"].is_array() {
                if let Some(before) = report["before"].as_str() {
                    if ui.link("Inspect comparison")
                        .on_hover_text("Compare both character packages on the input supplied to this decision")
                        .clicked() {
                        let accepted = self.character_decisions.iter()
                            .find(|d| d["invocation"] == o["decision"])
                            .and_then(|d| d["accepted_utc"].as_str())
                            .unwrap_or("Recorded choice");
                        self.character_paths.open_query(
                            ui.ctx(),
                            &self.root,
                            s(body, "owner"),
                            accepted,
                            Query {
                                decision: choice.clone(),
                                axis: None,
                                hidden_unit: None,
                                compare: Some(format!("object:{before}")),
                            },
                        );
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let judgment = format!("experience:{}@1", s(row, "decision"));
            for (label, reference) in [
                ("Adoption", s(report, "transition")),
                ("Later decision", choice.as_str()),
                ("Character use", s(o, "character_use")),
                ("Judgment decision", judgment.as_str()),
                ("Question at judgment", s(report, "question_revision")),
            ] {
                if ui.small_button(label).on_hover_text(reference).clicked() {
                    self.open_resource(ui.ctx(), reference.to_owned());
                }
            }
        });
    }
}
