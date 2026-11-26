//! Keep a chosen continuation, a matching wake and actual execution distinct.
use super::*;

pub(in crate::desktop::neural) fn peer_wakes(c: &Value) -> impl Iterator<Item = String> + '_ {
    c["on_peer_settled"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|reference| format!("peer waits or stops after {reference}"))
}

pub(super) fn current_wait(b: &Value) -> bool {
    b["continuation"]["kind"] == "wait"
        && b["declined"] == false
        && b["departure"].is_null()
        && b["closed_by_activity"].is_null()
        && b["pending_resume"].is_null()
}

impl App {
    pub(super) fn activity_work_status(&mut self, ui: &mut Ui) {
        let Some(work) = self
            .records("work")
            .iter()
            .find(|r| r["body"]["id"] == self.work)
        else {
            ui.weak("No activity selected.");
            return;
        };
        let open = work["body"]["phase"] == "open";
        let parts: Vec<_> = self
            .records("participation")
            .iter()
            .filter(|r| r["body"]["work"] == self.work)
            .collect();
        let waits: Vec<_> = parts
            .iter()
            .filter(|r| open && current_wait(&r["body"]))
            .collect();
        let matched = waits
            .iter()
            .filter(|r| r["wake_check"]["matched"] == true)
            .count();
        let checked = waits
            .iter()
            .filter(|r| r["wake_check"]["matched"].is_boolean())
            .count();
        let (unfinished, claims): (Vec<_>, Vec<_>) = self
            .records("outcomes")
            .iter()
            .filter(|r| r["body"]["work"] == self.work)
            .partition(|r| r["body"]["state"] != "claimed_complete");
        let qualified: Vec<_> = claims
            .into_iter()
            .filter(|r| {
                r["body"]["remaining"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
            })
            .collect();
        let requests: Vec<_> = self
            .records("human_requests")
            .iter()
            .filter(|r| {
                r["body"]["work"] == self.work
                    && matches!(s(&r["body"], "state"), "open" | "in_progress")
            })
            .collect();
        let (mut request, mut choice, mut inspect, mut receipt) = (None, None, None, None);
        let now = ai_personas::neural::now();
        egui::ScrollArea::vertical().id_salt(("work-status", &self.work)).show(ui, |ui| {
            eyebrow(ui, "Activity  /  Work status");
            ui.label(RichText::new(if !open { "Activity ended".into() } else if waits.len() == 1 {
                "One persona chose to wait".into()
            } else if !waits.is_empty() { format!("{} personas chose to wait", waits.len()) }
            else { "No current waits".into() }).strong().size(22.));
            ui.small(format!("Choices through workspace event #{} · Waiting is not completion.", self.cursor));
            ui.add_space(8.);
            card(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Execution");
                    match &self.runner_observation {
                        RunnerObservation::Observed { workers, .. } => {
                            ui.label(self.runner_status_text());
                            let here = workers.values().filter(|w| **w == self.work).count();
                            ui.weak(format!("{} here", counted(here as u64,"observed worker","observed workers")));
                        }
                        RunnerObservation::Unobserved if !self.running => { ui.label("No runner observed"); }
                        _ => { ui.label(self.runner_status_text()); }
                    }
                });
                if !waits.is_empty() {
                    if checked == waits.len() {
                        ui.label(format!("Matching arrival: {matched} of {checked} waits"));
                    } else {
                        ui.label(format!("Matching arrival: {matched} of {checked} checked"));
                        ui.weak(format!("Could not check {} waits.", waits.len() - checked));
                    }
                    ui.small("A matching arrival or a due timer permits reconsideration. Neither starts a runner or overrides a hold.");
                }
            });
            ui.add_space(9.);
            for (label, note, outcomes, show_empty) in [
                ("Unfinished outcomes", "Not claimed complete by their owners. These are individual commitments, not an exhaustive list of the original need.", &unfinished, true),
                ("Completion claims with remaining notes", "The owner marked this scope complete and also listed remaining work or qualifications. This is not user acceptance.", &qualified, false),
            ] {
                if outcomes.is_empty() && !show_empty { continue; }
                egui::CollapsingHeader::new(format!("{label} · {}", outcomes.len()))
                    .id_salt(("work-status-outcomes", &self.work, label)).show(ui, |ui| {
                ui.small(note);
                for r in outcomes {
                    let b = &r["body"]; let reference = inbox::handle(r);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(b["owner"].as_str().map(|id| self.label(id)).unwrap_or("No current owner"));
                        ui.weak(s(b, "state").replace('_', " "));
                        if ui.link("Open outcome").on_hover_text(&reference).clicked() { inspect = Some(reference); }
                    });
                    ui.label(s(b, "description"));
                    for note in b["remaining"].as_array().into_iter().flatten().filter_map(Value::as_str) { ui.label(format!("• {note}")); }
                }
                });
            }
            if !requests.is_empty() {
                egui::CollapsingHeader::new(format!("Outside steps · {}",requests.len())).id_salt(("work-status-requests", &self.work)).show(ui, |ui| {
                    ui.small("An open request does not suspend unrelated work.");
                    for r in &requests {
                        let b=&r["body"];
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(self.label(s(b,"requester")));
                            if ui.link(s(b,"title")).on_hover_text(s(b,"action")).clicked() { request=Some(s(&r["reference"],"id").to_owned()); }
                            if let Some(who)=b["claimed_by"].as_str() { ui.weak(format!("Taken by {who}")); }
                        });
                    }
                });
            }
            ui.add_space(8.);
            let history = self.home_index.decision_requests(&self.work);
            if !history.is_empty() {
                egui::CollapsingHeader::new(format!("Decision requests · {}", history.len())).id_salt(("status-resumes", &self.work)).show(ui, |ui| {
                    ui.small("Follow a request to the decisions made on it. Opening a receipt does not resume anyone.");
                    let key = ui.id().with("limit");
                    let limit = ui.data(|d| d.get_temp::<usize>(key)).unwrap_or(PAGE);
                    for seq in history.iter().rev().take(limit) {
                        if let Ok(i) = self.events.binary_search_by_key(seq, |e| e["event"].as_i64().unwrap_or_default()) {
                            let e = &self.events[i];
                            ui.horizontal_wrapped(|ui| {
                                if ui.link(format!("#{seq} · {}", self.label(s(e, "target")))).on_hover_text(s(&e["body"], "reason")).clicked() {receipt=Some(*seq);}
                                ui.small(format!("from {} · {}", s(e,"actor"),time_label(e["created"].as_i64().unwrap_or_default())));
                            });
                        }
                    }
                    if limit < history.len() && ui.small_button("Show earlier requests").clicked() { ui.data_mut(|d| d.insert_temp(key,limit.saturating_add(PAGE))); }
                });
            }
            eyebrow(ui, "Individual choices");
            for r in &parts {
                let b=&r["body"]; let person=s(b,"persona"); let c=&b["continuation"];
                ui.push_id(inbox::handle(r), |ui| card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let (rect,_)=ui.allocate_exact_size(egui::vec2(24.,24.),egui::Sense::hover());
                        design::persona_mark(ui,rect,self.label(person),persona_color(person));
                        ui.strong(self.label(person)); ui.separator(); ui.label(self.persona_state_in(person,&self.work));
                    });
                    if let Some(reason)=c["reason"].as_str().or(c["purpose"].as_str()) { ui.label(reason); }
                    if open && current_wait(b) {
                        let triggers: Vec<_> = c["on_events"].as_array().into_iter().flatten().filter_map(Value::as_str)
                            .map(|t| if t.contains(':') { t.to_owned() } else { t.replace('_'," ") }).chain(peer_wakes(c)).collect();
                        ui.small(format!("Wake on: {}", if triggers.is_empty() { "any new arrival".into() } else { triggers.join(" or ") }));
                        let signal = match r["wake_check"]["matched"].as_bool() {
                            Some(false) => "No matching arrival", Some(true) => "Matching arrival recorded", None => "Arrival check unavailable",
                        };
                        let timer = match c["wake_at_unix"].as_i64() {
                            Some(t) if t <= now => format!("Time reached · {}", time_label(t)),
                            Some(t) => format!("Wake at {}", time_label(t)), None => "No timer set".into(),
                        };
                        ui.small(format!("{signal}  ·  {timer}")).on_hover_text(r["wake_check"]["error"].as_str().unwrap_or("Checked against this participation’s inbox cursor and selected conditions."));
                    }
                    if let Some(state)=dialogs::RecipientState::read(r,open) {
                        if let Some(selected)=state.show(ui,self.dialog.is_none() && !self.busy) {choice=Some(selected);}
                    }
                }));
                ui.add_space(6.);
            }
        });
        if let Some(id) = request {
            self.show_human_request(&id);
        }
        if let Some(reference) = inspect {
            self.open_resource(ui.ctx(), reference);
        }
        if let Some(seq) = receipt {
            self.open_message_receipt(ui.ctx(), seq);
        }
        if let Some(selected) = choice {
            self.review_continuation(selected);
        }
    }
}
