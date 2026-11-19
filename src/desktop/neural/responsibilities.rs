use super::*;
use ai_personas::neural::membership;
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

pub(super) enum Action {
    Inspect(String),
    Message { person: String, text: String },
    Invite(String),
    ChangeActivity { reopen: bool, brief: String },
}

#[derive(Default)]
pub(super) struct Board {
    work: String,
    cursor: i64,
    pending: Option<Receiver<Result<Value, String>>>,
    report: Option<Value>,
    error: Option<String>,
}

impl Board {
    fn refresh(&mut self, ctx: &egui::Context, root: &Path, work: &str, cursor: i64) {
        if self.work != work {
            self.report = None;
        }
        self.work = work.to_owned();
        self.cursor = cursor;
        self.error = None;
        let work = work.to_owned();
        let root = root.to_owned();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| membership::responsibilities(&db, &work))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn receive(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Responsibility board worker closed".into()),
        };
        self.pending = None;
        match result {
            Ok(report) => self.report = Some(report),
            Err(error) => self.error = Some(error),
        }
    }
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        root: &Path,
        work: &str,
        cursor: i64,
    ) -> Option<Action> {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Responsibility board");
            if !work.is_empty() && ui.small_button("Refresh").clicked() {
                self.refresh(ui.ctx(), root, work, cursor);
            }
        });
        if work.is_empty() {
            ui.weak("Select an activity in the outliner.");
            return None;
        }
        self.receive();
        if self.work != work || (self.cursor != cursor && self.pending.is_none()) {
            self.refresh(ui.ctx(), root, work, cursor);
        }
        if let Some(error) = &self.error {
            ui.colored_label(theme::ACCENT, error);
        }
        if self.pending.is_some() {
            ui.spinner();
        }
        let report = self.report.as_ref()?;
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt(("responsibility-board", work))
            .show(ui, |ui| {
                let members = items(report, "members");
                let ended = report["phase"] == "dissolved";
                ui.horizontal_wrapped(|ui| {
                    if ended { theme::badge(ui,"Activity ended",theme::ACCENT); }
                    if ui.small_button(if ended { "Reopen activity…" } else { "End activity…" }).clicked() {
                        action = Some(Action::ChangeActivity { reopen: ended, brief: s(report,"brief").into() });
                    }
                });
                ui.add_space(3.);
                ui.horizontal_wrapped(|ui| {
                    for member in members {
                        let actor = s(member, "actor");
                        let status = s(member, "state");
                        let button =
                            ui.small_button(format!("{} · {}", person(members, actor), status));
                        if button.on_hover_text("Inspect this participation").clicked() {
                            action = Some(Action::Inspect(s(member, "participation").into()));
                        }
                    }
                });
                ui.add_space(10.);
                let headers = [
                    ("Needs follow-through", theme::ACCENT),
                    ("With a participant", TEAL),
                    ("Claimed complete", PURPLE),
                ];
                let mut lanes: [Vec<&Value>; 3] = Default::default();
                for outcome in items(report, "outcomes") {
                    let reference = handle(outcome);
                    let index = match s(&report["ownership"][&reference], "state") {
                        "claimed_complete" => 2,
                        "ready_owner" | "waiting_owner" => 1,
                        _ => 0,
                    };
                    lanes[index].push(outcome);
                }
                let wide = ui.available_width() >= 630.;
                let mut lane = |ui: &mut Ui, n: usize| {
                    let (label, color) = headers[n];
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(color, RichText::new(label).strong());
                        ui.small(lanes[n].len().to_string());
                    });
                    ui.separator();
                    if lanes[n].is_empty() {
                        ui.weak("No outcomes");
                    }
                    for outcome in &lanes[n] {
                        if let Some(a) = outcome_card(ui, outcome, report, members) {
                            action = Some(a);
                        }
                        ui.add_space(4.);
                    }
                };
                if wide {
                    ui.columns(3, |cols| {
                        for (i, column) in cols.iter_mut().enumerate() {
                            lane(column, i);
                        }
                    });
                } else {
                    for i in 0..3 {
                        lane(ui, i);
                        ui.add_space(8.);
                    }
                }
                ui.add_space(10.);
                for closure in items(report,"dissolutions").iter().rev() {
                    let body=&closure["body"];
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui|{
                            ui.strong(format!("Activity ended by {}",s(body,"operator")));
                            if ui.small_button("Closure record ↗").clicked() { action=Some(Action::Inspect(handle(closure))); }
                        });
                        ui.label(s(body,"reason"));
                        ui.small(format!("At closure · Unfinished outcomes {} · Effects observed {} · Open jobs {} · Outstanding calls {}",
                            items(body,"unfinished_outcomes").len(),items(body,"effect_observations").len(),items(body,"open_jobs_observed").len(),items(body,"inference_observations").len()));
                        ui.push_id(handle(closure), |ui| ui.collapsing("Participation at closure", |ui| {
                            for member in items(body,"members") {
                                let name=person(members,s(member,"actor"));
                                if let Some(commitment)=member["commitment"]["record"]["commitment"].as_str() {
                                    ui.label(format!("{name} · {commitment}"));
                                } else { ui.small(format!("{name} · No current commitment")); }
                            }
                        }));
                    });
                }
                for departure in items(report, "departures").iter().rev() {
                    let body = &departure["body"];
                    let who = s(body, "actor");
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!("{} left the activity", person(members, who)));
                            if ui.small_button("Departure ↗").clicked() {
                                action = Some(Action::Inspect(handle(departure)));
                            }
                            if !ended && members
                                .iter()
                                .any(|m| m["actor"] == who && m["departure"] == handle(departure))
                                && ui.small_button("Invite again…").clicked()
                            {
                                action = Some(Action::Invite(who.into()));
                            }
                        });
                        ui.label(s(body, "reason"));
                        if let Some(prior) =
                            body["commitment_at_departure"]["record"]["commitment"].as_str()
                        {
                            ui.small(format!("Prior commitment · {prior}"));
                        }
                        for row in items(body, "released_outcomes") {
                            let reference = s(row, "outcome");
                            if ui
                                .small_button("Released version ↗")
                                .on_hover_text(reference)
                                .clicked()
                            {
                                action = Some(Action::Inspect(reference.into()));
                            }
                        }
                    });
                }
                if !items(report, "offers").is_empty() {
                    ui.add_space(8.);
                    section(ui, "Handoff offers", "");
                    for offer in items(report, "offers") {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "{} → {}",
                                person(members, s(offer, "from")),
                                person(members, s(offer, "to"))
                            ));
                            theme::badge(ui, &title(s(offer, "state")), PURPLE);
                            if offer["basis_current"] == false {
                                ui.weak("Scope changed");
                            }
                            if ui.small_button("Offer ↗").clicked() {
                                action = Some(Action::Inspect(s(offer, "reference").into()));
                            }
                        });
                    }
                }
                let effects = items(report, "unsettled_effects");
                if !effects.is_empty() {
                    ui.add_space(8.);
                    section(
                        ui,
                        "Effects awaiting resolution",
                        &effects.len().to_string(),
                    );
                    for effect in effects {
                        ui.label(format!(
                            "{} · {} · {}",
                            person(members, s(effect, "actor")),
                            s(effect, "tool"),
                            s(effect, "state")
                        ));
                    }
                }
                for (field, heading) in [("unsettled_inference","Outstanding provider calls"),("open_jobs","Open jobs")] {
                    let rows=items(report,field);
                    if rows.is_empty() {continue;}
                    section(ui,heading,&rows.len().to_string());
                    for row in rows {
                        ui.horizontal_wrapped(|ui|{
                            let who=row["actor"].as_str().or(row["owner"].as_str()).unwrap_or("");
                            ui.label(person(members,who));
                            ui.small(s(row,"state"));
                            if let Some(reference)=row["reference"].as_str().or(row["receipt"].as_str()) {
                                if ui.small_button("Record ↗").clicked() {action=Some(Action::Inspect(reference.into()));}
                            } else { ui.small(s(row,"invocation")); }
                        });
                    }
                }
            });
        action
    }
}
fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or_default()
}
fn handle(record: &Value) -> String {
    let r = &record["reference"];
    format!("{}:{}@{}", s(r, "kind"), s(r, "id"), r["revision"])
}
fn person<'a>(members: &'a [Value], actor: &'a str) -> &'a str {
    members
        .iter()
        .find(|m| m["actor"] == actor)
        .and_then(|m| m["label"].as_str())
        .unwrap_or(actor)
}
fn outcome_card(ui: &mut Ui, record: &Value, report: &Value, members: &[Value]) -> Option<Action> {
    let reference = handle(record);
    let body = &record["body"];
    let ownership = &report["ownership"][&reference];
    let owner = s(body, "owner");
    let mut action = None;
    card(ui).show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            let status = match s(ownership, "state") {
                "unowned" => "Unowned",
                "follow_through_gap" => "Owner stopped or unavailable",
                "waiting_owner" => "Waiting",
                "ready_owner" => "Ready",
                "claimed_complete" => "Claimed complete",
                _ => "Status unavailable",
            };
            let description = s(body, "description");
            ui.strong(excerpt(description.lines().next().unwrap_or(""), 100));
            ui.horizontal_wrapped(|ui| {
                ui.small(status);
                if !owner.is_empty() { ui.label(person(members, owner)); }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Outcome ↗").clicked() {
                    action = Some(Action::Inspect(reference.clone()));
                }
                if let Some(proof) = ownership["accepted_responsibility"].as_str()
                    .or_else(|| ownership["accepted_handoff"].as_str()) {
                    if ui.small_button("Acceptance ↗").clicked() {
                        action = Some(Action::Inspect(proof.into()));
                    }
                }
                if ownership["state"] == "unowned" && report["phase"] != "dissolved" {
                    let volunteer = members.iter().find(|m| m["state"] == "accepted")
                        .or_else(|| members.iter().find(|m| m["state"] == "invited"));
                    if let Some(member) = volunteer {
                        if ui.small_button("Ask a volunteer…").clicked() {
                            action = Some(Action::Message {
                                person: s(member, "actor").into(),
                                text: format!("Would you like to take responsibility for the remaining work in {reference}?"),
                            });
                        }
                    } else if ui.small_button("Invite a volunteer…").clicked() {
                        action = Some(Action::Invite(String::new()));
                    }
                }
            });
            let remaining = items(body, "remaining");
            let label = match remaining.len() {
                0 => "Full scope".to_owned(),
                1 => "Scope and 1 remaining duty".to_owned(),
                n => format!("Scope and {n} remaining duties"),
            };
            ui.push_id(&reference, |ui| ui.collapsing(label, |ui| {
                ui.label(description);
                for duty in remaining.iter().filter_map(Value::as_str) {
                    ui.label(format!("• {duty}"));
                }
            }));
        });
    });
    action
}
