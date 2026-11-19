use super::*;
use std::collections::BTreeMap;

impl App {
    pub(super) fn life_history(&mut self, ui: &mut Ui) {
        ui.heading(format!("{}’s life history", self.label(&self.persona)));
        ui.horizontal_wrapped(|ui| {
            for (i, title) in [
                "All",
                "Tasks",
                "Social",
                "Character",
                "Feedback",
                "Participation",
            ]
            .into_iter()
            .enumerate()
            {
                ui.selectable_value(&mut self.life_filter, i, title);
            }
        });
        let activities: BTreeMap<_, _> = self
            .records("work")
            .iter()
            .map(|r| {
                (
                    s(&r["body"], "id").to_owned(),
                    (
                        activity_title(&r["body"]).to_owned(),
                        r["body"]["kind"] == "social",
                    ),
                )
            })
            .collect();
        let mut invitations: BTreeMap<&str, i64> = BTreeMap::new();
        for e in &self.events {
            if e["kind"] == "invitation" && e["target"] == self.persona {
                if let Some(seq) = e["event"].as_i64() {
                    invitations
                        .entry(s(e, "work"))
                        .and_modify(|first| *first = (*first).min(seq))
                        .or_insert(seq);
                }
            }
        }
        let events: Vec<_> = self
            .events
            .iter()
            .rev()
            .filter(|e| {
                let kind = s(e, "kind");
                let work = s(e, "work");
                let visible = e["actor"] == self.persona
                    || e["target"] == self.persona
                    || (e["target"] == "*"
                        && invitations.get(work).is_some_and(|joined| {
                            e["event"].as_i64().is_some_and(|seq| seq >= *joined)
                        }));
                let social = activities.get(work).is_some_and(|(_, social)| *social);
                let membership = matches!(
                    kind,
                    "invitation"
                        | "invitation_response"
                        | "commitment_changed"
                        | "contribution_proposal"
                        | "participation_left"
                        | "handoff_offered"
                        | "handoff_resolved"
                        | "responsibility_accepted"
                        | "activity_dissolved"
                        | "activity_reopened"
                        | "explicit_resume"
                );
                visible
                    && (membership
                        || matches!(
                            kind,
                            "message"
                                | "reply"
                                | "user_message"
                                | "user_acceptance"
                                | "requirements_amended"
                                | "delivery"
                                | "model_changed"
                        ))
                    && !(kind == "delivery" && e["target"] == "user")
                    && match self.life_filter {
                        1 => !social,
                        2 => social,
                        3 => kind == "model_changed",
                        4 => matches!(
                            kind,
                            "user_message" | "user_acceptance" | "requirements_amended"
                        ),
                        5 => membership,
                        _ => true,
                    }
            })
            .collect();
        if events.is_empty() {
            ui.weak("No recorded experiences in this view.");
            return;
        }
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt(("persona-life", &self.persona, self.life_filter))
            .show_rows(ui, 83., events.len(), |ui, visible| {
                for index in visible {
                    let e = events[index];
                    let b = &e["body"];
                    let work = s(e, "work");
                    let kind = s(e, "kind");
                    let (activity, social) = activities
                        .get(work)
                        .map(|(label, s)| (label.as_str(), *s))
                        .unwrap_or((work, false));
                    let color = if kind == "model_changed" {
                        TEAL
                    } else if social {
                        PURPLE
                    } else {
                        theme::ACCENT
                    };
                    let sequence = e["event"].as_i64().unwrap_or_default();
                    ui.push_id(sequence, |ui| {
                        ui.horizontal(|ui| {
                            let (rail, _) =
                                ui.allocate_exact_size(egui::vec2(15., 72.), egui::Sense::hover());
                            let x = rail.center().x;
                            ui.painter().line_segment(
                                [egui::pos2(x, rail.top()), egui::pos2(x, rail.bottom())],
                                Stroke::new(1., color.gamma_multiply(0.45)),
                            );
                            ui.painter()
                                .circle_filled(egui::pos2(x, rail.top() + 11.), 4., color);
                            ui.vertical(|ui| {
                                ui.set_max_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.strong(title(kind));
                                    ui.small(s(e, "created_utc"));
                                    if ui.small_button("Inspect").clicked() {
                                        selected = Some(format!("experience:event-{sequence}@1"));
                                    }
                                });
                                let text = match kind {
                                    "invitation" | "contribution_proposal" => {
                                        s(b, "contribution").into()
                                    }
                                    "invitation_response" => {
                                        if b["accept"] == true {
                                            format!("Accepted · {}", s(b, "commitment"))
                                        } else {
                                            format!("Declined · {}", s(b, "reason"))
                                        }
                                    }
                                    "model_changed" => format!(
                                        "Epoch {} · {}",
                                        b["epoch"],
                                        s(&b["choice"], "reason")
                                    ),
                                    "user_acceptance" => format!(
                                        "{} · {}",
                                        if b["decision"]["accepted"] == true {
                                            "Accepted"
                                        } else {
                                            "Changes requested"
                                        },
                                        s(&b["decision"], "reason")
                                    ),
                                    _ => b["text"]
                                        .as_str()
                                        .or(b["reason"].as_str())
                                        .or(b["title"].as_str())
                                        .unwrap_or("")
                                        .into(),
                                };
                                ui.add(egui::Label::new(text).truncate());
                                if let Some(parent) = b["reply_to"].as_str() {
                                    if ui.small_button("In reply to ↗").clicked() {
                                        selected = Some(parent.to_owned());
                                    }
                                }
                                let author = self.label(s(e, "actor"));
                                let route = match s(e, "target") {
                                    "*" => author.to_owned(),
                                    "user" => format!("{author} → You"),
                                    to => format!("{author} → {}", self.label(to)),
                                };
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("{route} · {activity}"))
                                            .small()
                                            .color(theme::palette(ui).muted),
                                    )
                                    .truncate(),
                                );
                            });
                        })
                    });
                }
            });
        if let Some(reference) = selected {
            self.open_resource(ui.ctx(), reference);
        }
    }
}
