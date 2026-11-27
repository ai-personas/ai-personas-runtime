//! Browse what participants actually decided. Selecting an edition pins it
//! while new work arrives; this projection never schedules a persona.
use super::*;

#[derive(Default)]
pub(super) struct Journal {
    person: Option<String>,
    pinned: Option<String>,
}

impl Board {
    pub(in super::super) fn holding_choice(&self, reference: &str) -> bool {
        self.view == View::Journal && self.journal.pinned.as_deref().is_some_and(|id| reference == format!("experience:{id}@1"))
    }
}

enum Action {
    Inspect(String),
    Images,
    Preview,
    Character,
    Older,
    Newer,
    Latest,
}

impl App {
    /// Follow a receipt to the actual choice, even when sender and receiver
    /// worked in different activities. Later choices must not move this view.
    pub(in super::super) fn open_choice_or_record(&mut self, ctx: &egui::Context, reference: String) {
        let call = reference.strip_prefix("experience:").and_then(|r| r.strip_suffix("@1"));
        let choice = call.and_then(|id| {
            self.character_decisions.iter().find(|d| d["invocation"] == id && d["character_use"].is_string())
        });
        if let Some(choice) = choice {
            let work = s(choice, "work").to_owned();
            let person = s(choice, "actor").to_owned();
            let pinned = s(choice, "invocation").to_owned();
            self.open_home_activity(&work);
            self.persona.clone_from(&person);
            self.live_board.view = View::Journal;
            self.live_board.journal = Journal { person: Some(person), pinned: Some(pinned) };
        } else {
            self.open_resource(ctx, reference);
        }
    }

    pub(super) fn decision_journal(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .id_salt(("decision-journal", &self.work))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let p = theme::palette(ui);
                self.finding_return_link(ui);
                eyebrow(ui, "Activity  /  Decision journal");
                ui.heading("The work in their own words");
                ui.label("Read what each persona judged sufficient, what was still open, and why they chose to continue, wait or stop.");
                ui.add_space(7.);
                let mut person = self.live_board.journal.person.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut person, None, "Everyone");
                    for member in self.records("participation").iter().map(|r| &r["body"]).filter(|m| m["work"] == self.work) {
                        let id = s(member, "persona");
                        ui.selectable_value(&mut person, Some(id.to_owned()), RichText::new(self.label(id)).color(p.ink(self.color(id))));
                    }
                });
                if person != self.live_board.journal.person {
                    self.live_board.journal = Journal { person, pinned: None };
                }
                let journal = &self.live_board.journal;
                let mut choices = self.character_decisions.iter().rev().filter(|d| {
                    d["work"] == self.work && d["character_use"].is_string()
                        && journal.person.as_ref().is_none_or(|id| d["actor"] == id.as_str())
                }).peekable();
                let Some(&latest) = choices.peek() else {
                    ui.separator();
                    ui.weak("No accepted work decision under an active character in this selection yet.");
                    return;
                };
                // Walk only as far back as the selected call. Store its identity,
                // not its list offset, so an arrival cannot replace the open record.
                let mut newer = None;
                let selected = loop {
                    let Some(item) = choices.next() else {
                        ui.weak("The selected decision is unavailable.");
                        if ui.button("Show latest").clicked() { self.live_board.journal.pinned = None; }
                        return;
                    };
                    if journal.pinned.as_ref().is_none_or(|id| item["invocation"] == id.as_str()) { break item; }
                    newer = Some(item);
                };
                let older = choices.next();
                let mut action = None;
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(older.is_some(), egui::Button::new("← Older")).clicked() { action = Some(Action::Older); }
                    if ui.add_enabled(newer.is_some(), egui::Button::new("Newer →")).clicked() { action = Some(Action::Newer); }
                    if self.live_board.journal.pinned.is_none() {
                        ui.small("Following latest");
                        if ui.small_button("Hold this decision").clicked() {
                            self.live_board.journal.pinned = selected["invocation"].as_str().map(str::to_owned);
                        }
                    } else {
                        ui.small(if latest["invocation"] == selected["invocation"] { "Held at latest" } else { "Reading history" });
                        if ui.small_button("Follow latest").clicked() { action = Some(Action::Latest); }
                    }
                });
                ui.add_space(7.);
                let id = s(selected, "actor");
                let tint = p.ink(self.color(id));
                let assessment = &selected["judgment"];
                let (label, color) = match s(assessment, "status") {
                    "ongoing" => ("Work in progress", tint),
                    "blocked" => ("Reports a blocker", p.ink(theme::ACCENT)),
                    "partial" => ("Partial result", p.ink(theme::ACCENT)),
                    "individual_sufficient" => ("Judges own contribution sufficient", tint),
                    "shared_sufficient" => ("Judges shared work sufficient", tint),
                    _ => ("Recorded assessment", p.muted),
                };
                let panel = egui::Frame::new().fill(p.header).stroke(Stroke::new(1., p.border))
                    .corner_radius(8).inner_margin(14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(self.label(id)).size(22.).strong().color(tint));
                        theme::badge(ui, label, color);
                        ui.small(s(selected, "accepted_utc"));
                    });
                    ui.add_space(4.);
                    ui.label(s(assessment, "reason"));
                });
                let rect = panel.response.rect.shrink(1.);
                ui.painter().line_segment([rect.left_top()+egui::vec2(1.,8.),rect.left_bottom()+egui::vec2(1.,-8.)],Stroke::new(3.,tint));
                ui.add_space(8.);
                let remaining = |ui: &mut Ui| {
                    eyebrow(ui, "Still needed at this decision");
                    let items = assessment["remaining"].as_array().map(Vec::as_slice).unwrap_or_default();
                    if items.is_empty() { ui.weak("No remaining items listed by this persona."); }
                    for text in items.iter().filter_map(Value::as_str) { ui.label(format!("•  {text}")); }
                };
                let next = |ui: &mut Ui| {
                    let step = &selected["continuation"];
                    eyebrow(ui, "Chosen next step");
                    let (label, text) = match s(step, "kind") {
                        "ready" => ("Continue", s(step,"purpose")),
                        "wait" => ("Wait", s(step,"reason")),
                        "stop" => ("Stop", s(step,"reason")),
                        _ => ("Unrecorded", ""),
                    };
                    ui.strong(label);
                    ui.label(text);
                    if step["kind"] == "wait" {
                        let events = wake_labels(step, |id| self.label(id));
                        ui.small(if events.is_empty() { "Wake on any incoming event".into() } else { format!("Wake on: {}",events.join(" or ")) });
                        if let Some(time) = step["wake_at_unix"].as_i64() { ui.small(format!("Scheduled wake · {time} (Unix time)")); }
                    }
                };
                if ui.available_width() >= 630. {
                    ui.columns(2, |cols| {
                        let left = egui::Layout::top_down(egui::Align::Min);
                        cols[0].with_layout(left, remaining);
                        cols[1].with_layout(left, next);
                    });
                } else {
                    remaining(ui); ui.add_space(8.); next(ui);
                }
                ui.add_space(10.);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Character at this decision…").clicked() { action = Some(Action::Character); }
                    if selected["has_image_inputs"] == true && ui.button("Images at this decision…").clicked() {
                        action = Some(Action::Images);
                    }
                    if ui.button("Preview comparison input…").on_hover_text("Inspect the frozen encounter. Does not sample a response or resume anyone.").clicked() {
                        action = Some(Action::Preview);
                    }
                    if ui.button("Open decision record").clicked() {
                        action = Some(Action::Inspect(format!("experience:{}@1",s(selected,"invocation"))));
                    }
                });
                let evidence = assessment["evidence"].as_array().map(Vec::as_slice).unwrap_or_default();
                if !evidence.is_empty() {
                    egui::CollapsingHeader::new(format!("Evidence cited · {}", evidence.len()))
                        .id_salt(("journal-evidence",s(selected,"invocation")))
                        .show(ui, |ui| {
                            for reference in evidence.iter().filter_map(Value::as_str) {
                                if ui.link(reference).clicked() { action = Some(Action::Inspect(reference.into())); }
                            }
                        });
                }
                match action {
                    Some(Action::Preview) => self.encounter.preview(ui.ctx(), &self.root, selected),
                    Some(Action::Character) => self.character_paths.open(ui.ctx(), &self.root, selected),
                    Some(Action::Images) => {
                        self.profile_images.open_decision(ui.ctx(), &self.root, id, s(selected, "invocation"));
                        let person = id.to_owned();
                        self.show_person(&person);
                        self.profile_view = people::ProfileView::Images;
                    }
                    Some(Action::Inspect(r)) => self.open_resource(ui.ctx(), r),
                    Some(Action::Newer | Action::Older) => {
                        let target = if matches!(action, Some(Action::Older)) { older } else { newer };
                        self.live_board.journal.pinned = target.and_then(|d| d["invocation"].as_str()).map(str::to_owned);
                    }
                    Some(Action::Latest) => self.live_board.journal.pinned = None,
                    None => {}
                }
            });
    }
}
