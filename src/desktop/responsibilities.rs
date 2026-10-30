//! Readable summaries of the existing accepted responsibility projection.
use super::*;

impl App {
    pub(super) fn responsibilities(&mut self, ui: &mut Ui, snapshot: &Value) {
        theme::header(ui, Icon::Task, "Responsibilities", "CURRENT ACCEPTANCE");
        let participants = models_at(snapshot, "participants");
        if participants.is_empty() {
            ui.weak("No participants are recorded in this task snapshot.");
        }
        for person in participants {
            ui.push_id(s(person, "persona"), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        self.persona_link(ui, s(person, "persona"));
                        let accepted = person["responsibility_accepted"] == true;
                        let color = if accepted {
                            theme::GREEN
                        } else {
                            theme::palette(ui).muted
                        };
                        theme::badge(
                            ui,
                            if accepted {
                                "Accepted responsibility"
                            } else {
                                "No current acceptance"
                            },
                            color,
                        );
                    });
                    let acceptance = &person["acceptance"];
                    if !acceptance.is_null() {
                        let terms = &acceptance["record"]["terms"];
                        theme::message_body(ui, s(terms, "exact_terms"));
                        ui.label(
                            RichText::new(format!(
                                "Acceptance {} · mandate revision {}",
                                s(acceptance, "id"),
                                terms["mandate_revision"],
                            ))
                            .small()
                            .weak(),
                        );
                        ui.collapsing("Outcomes, review roles & acceptance provenance", |ui| {
                            json_view(ui, acceptance);
                        });
                    }
                });
            });
        }
        ui.add_space(10.0);
        theme::header(
            ui,
            Icon::People,
            "Arrangement proposals",
            "AUTHORSHIP ≠ ADOPTION",
        );
        if let Some(proposals) = snapshot["arrangement_proposals"]
            .as_object()
            .filter(|p| !p.is_empty())
        {
            for (id, proposal) in proposals {
                ui.push_id(("proposal", id), |ui| {
                    egui::CollapsingHeader::new(format!(
                        "{id} · {}",
                        if proposal["adopted"].is_null() {
                            "no adoption recorded"
                        } else {
                            "adopted"
                        },
                    ))
                    .default_open(proposal["adopted"].is_null())
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Proposed by");
                            self.persona_link(ui, s(&proposal["by"], "persona"));
                        });
                        theme::message_body(ui, s(&proposal["terms"], "reason"));
                        if let Some(changes) = proposal["terms"]["changes"].as_object() {
                            for (persona, change) in changes {
                                ui.push_id(persona, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.weak("Proposed terms for");
                                        self.persona_link(ui, persona);
                                    });
                                    theme::message_body(ui, s(&change["terms"], "exact_terms"));
                                });
                            }
                        }
                        ui.collapsing(
                            "Exact proposal, consent, decline & adoption records",
                            |ui| {
                                json_view(ui, proposal);
                            },
                        );
                    });
                });
            }
        } else {
            ui.weak("No arrangement proposals are recorded.");
        }
        ui.add_space(10.0);
        theme::header(ui, Icon::Task, "Adopted arrangements", "CURRENT & HISTORY");
        if let Some(arrangements) = snapshot["arrangements"]
            .as_object()
            .filter(|a| !a.is_empty())
        {
            for (id, arrangement) in arrangements {
                let current = snapshot["current_arrangement"].as_str() == Some(id);
                ui.push_id(("arrangement", id), |ui| {
                    egui::CollapsingHeader::new(format!(
                        "{id} · {} · revision {}",
                        if current {
                            "current"
                        } else {
                            "accepted history"
                        },
                        arrangement["revision"],
                    ))
                    .default_open(current)
                    .show(ui, |ui| {
                        self.arrangement_summary(ui, arrangement);
                        egui::CollapsingHeader::new("Exact arrangement & provenance")
                            .default_open(false)
                            .show(ui, |ui| json_view(ui, arrangement));
                    });
                });
            }
        } else {
            ui.weak("No adopted arrangement is recorded.");
        }
        let dependencies = models_at(snapshot, "dependencies");
        ui.add_space(10.0);
        theme::header(
            ui,
            Icon::Task,
            "Dependencies",
            &dependencies.len().to_string(),
        );
        if dependencies.is_empty() {
            ui.weak("No task dependencies are recorded.");
        }
        for (index, dependency) in dependencies.iter().enumerate() {
            ui.push_id(("dependency", index), |ui| {
                if let Some(owner) = dependency["accountability"]["Owner"].as_str() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Recorded owner");
                        self.persona_link(ui, owner);
                    });
                } else if let Some(gap) = dependency["accountability"]["Gap"].as_str() {
                    ui.colored_label(theme::ACCENT, format!("Accountability gap: {gap}"));
                } else {
                    ui.weak("Accountability is unavailable in this record.");
                }
                ui.collapsing("Dependency reference & evidence", |ui| {
                    json_view(ui, dependency)
                });
            });
        }
        if let Some(predecessor) = snapshot["predecessor"].as_str() {
            let available = models_at(&self.view.inspection, "works")
                .iter()
                .any(|work| s(work, "id") == predecessor);
            if ui
                .add_enabled(
                    !self.busy && available,
                    egui::Button::new(format!("Predecessor task: {predecessor}")),
                )
                .clicked()
            {
                self.open_task(predecessor.to_owned(), 0, None);
            }
        }
        ui.add_space(8.0);
        ui.weak("A message records communication; acceptance records responsibility. Neither establishes that work is complete.");
    }

    fn arrangement_summary(&mut self, ui: &mut Ui, arrangement: &Value) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Adoption recorded by");
            self.persona_link(ui, s(&arrangement["by"], "persona"));
        });
        if let Some(proposal) = arrangement["proposal"]["id"].as_str() {
            ui.label(format!(
                "Proposal: {proposal} · revision {}",
                arrangement["proposal"]["revision"]
            ));
        } else {
            ui.weak("Proposal reference unavailable in this record.");
        }

        let rule = &arrangement["decision_rule"];
        if rule.is_null() {
            ui.weak("No result decision rule established in this arrangement.");
        } else if let Some(persona) = rule["Designated"].as_str() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Result decision rule: designated persona");
                self.persona_link(ui, persona);
            });
        } else if let (Some(minimum), Some(eligible)) = (
            rule["Quorum"]["minimum"].as_u64(),
            rule["Quorum"]["eligible"].as_array(),
        ) {
            ui.label(format!(
                "Result decision rule: quorum of {minimum} from {} eligible personas",
                eligible.len()
            ));
            ui.horizontal_wrapped(|ui| {
                ui.weak("Eligible:");
                for persona in eligible {
                    if let Some(persona) = persona.as_str() {
                        self.persona_link(ui, persona);
                    } else {
                        ui.weak("Persona identity unavailable");
                    }
                }
            });
        } else {
            ui.weak("Unrecognized result decision rule; see the exact arrangement record.");
        }

        if let Some(acceptances) = arrangement["acceptances"].as_object() {
            if acceptances.is_empty() {
                ui.weak("No acceptance links in this arrangement record.");
            } else {
                ui.collapsing(format!("Acceptance links at adoption ({})", acceptances.len()), |ui| {
                    ui.weak("Includes unchanged contributions. These links retain their adoption-time identities.");
                    for (persona, acceptance) in acceptances {
                        ui.horizontal_wrapped(|ui| {
                            self.persona_link(ui, persona);
                            ui.label(format!(
                                "Acceptance: {}",
                                acceptance.as_str().unwrap_or("record identity unavailable")
                            ));
                        });
                    }
                });
            }
        } else {
            ui.weak("Acceptance links unavailable in this record.");
        }

        if let Some(consents) = arrangement["consents"].as_object() {
            if consents.is_empty() {
                ui.weak("No consents in this arrangement record.");
            } else {
                ui.collapsing(format!("Consents recorded for adoption ({})", consents.len()), |ui| {
                    ui.weak("Each consent binds the participant's acceptance before this arrangement.");
                    for (persona, consent) in consents {
                        ui.horizontal_wrapped(|ui| {
                            self.persona_link(ui, persona);
                            ui.label(format!(
                                "Prior acceptance: {}",
                                consent["acceptance"].as_str().unwrap_or("record identity unavailable")
                            ));
                        });
                    }
                });
            }
        } else {
            ui.weak("Consent records unavailable in this record.");
        }
    }
}
