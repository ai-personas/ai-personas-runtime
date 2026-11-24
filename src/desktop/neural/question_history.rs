use super::*;
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

#[derive(Default)]
pub(super) struct History {
    selection: Option<(String, String)>,
    pending: Option<Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
    page: usize,
    trail_only: bool,
}

impl History {
    pub(super) fn open(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        reference: String,
        title: String,
    ) {
        let (sender, receiver) = mpsc::channel();
        if self.selection.as_ref().is_none_or(|(r, _)| r != &reference) {
            self.trail_only = false;
        }
        self.selection = Some((reference.clone(), title));
        self.pending = Some(receiver);
        self.result = None;
        self.page = 0;
        let root = root.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| ai_personas::neural::question_history::inspect(&db, &reference));
            let _ = sender.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        status: &Value,
    ) -> Option<String> {
        let (reference, title) = self.selection.as_ref()?;
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.result = Some(result);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.result = Some(Err("The history reader stopped.".into()));
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut visible = true;
        let mut inspect = None;
        let mut refresh = false;
        let bounds = (ctx.content_rect().size() - egui::vec2(46., 92.)).max(egui::vec2(1., 1.));
        // In a short or highly zoomed window the labels and navigation must
        // scroll too. Use a single scroll surface in that layout.
        let compact = bounds.x < 360. || bounds.y < 400.;
        egui::Window::new("Question history")
            .id(egui::Id::new("question-history"))
            .open(&mut visible)
            .min_size(egui::vec2(1., 1.))
            .max_size(bounds)
            .default_size(egui::vec2(680., 510.).min(bounds))
            .vscroll(compact)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add(egui::Label::new(RichText::new(title).strong().size(17.)).wrap());
                ui.add_space(5.);
                match &self.result {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Reading accepted decisions…");
                        });
                    }
                    Some(Err(error)) => {
                        ui.colored_label(theme::ACCENT, error);
                        refresh = ui.small_button("Try again").clicked();
                    }
                    Some(Ok(report)) => {
                        ui.horizontal_wrapped(|ui| {
                            theme::badge(
                                ui,
                                &format!("Supplied in {} decisions", report["decisions_supplied"]),
                                PURPLE,
                            );
                            theme::badge(
                                ui,
                                &format!("Explicit uses · {}", report["decisions_applied"]),
                                TEAL,
                            );
                            refresh = ui.small_button("Refresh").clicked();
                        });
                        ui.horizontal_wrapped(|ui| {
                            let all = ui.selectable_value(&mut self.trail_only, false, "All decisions").changed();
                            let trail = ui.selectable_value(&mut self.trail_only, true, "Practice trail").changed();
                            if all || trail { self.page = 0; }
                        });
                        if self.trail_only { ui.weak("Question revisions, explicit uses and later full reads of an application."); }
                        let rows: Vec<_> = report["decisions"].as_array().into_iter().flatten().filter(|r| {
                            !self.trail_only || r["application"].is_string() || ["application_reads", "revisions_written"].iter()
                                .any(|key| r[*key].as_array().is_some_and(|v| !v.is_empty()))
                        }).collect();
                        const PAGE: usize = 8;
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(self.page > 0, egui::Button::new("← Newer"))
                                .clicked()
                            {
                                self.page -= 1;
                            }
                            let start = self.page * PAGE;
                            ui.small(format!(
                                "{}–{} of {} decisions",
                                usize::from(!rows.is_empty()) + start,
                                (start + PAGE).min(rows.len()),
                                rows.len()
                            ));
                            if ui
                                .add_enabled(
                                    start + PAGE < rows.len(),
                                    egui::Button::new("Older →"),
                                )
                                .clicked()
                            {
                                self.page += 1;
                            }
                        });
                        ui.separator();
                        let mut cards = |ui: &mut Ui| {
                                for row in rows.iter().skip(self.page * PAGE).take(PAGE) {
                                    let work = status["work"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .find(|w| w["body"]["id"] == row["work"]);
                                    let activity = work
                                        .map(|w| activity_title(&w["body"]))
                                        .unwrap_or(s(row, "work"));
                                    card(ui).show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(excerpt(activity, 92)).strong(),
                                            )
                                            .wrap(),
                                        )
                                        .on_hover_text(activity);
                                        if let Some(utc) = row["accepted_utc"].as_str() { ui.small(utc); }
                                        ui.horizontal_wrapped(|ui| {
                                            for item in
                                                row["supplied"].as_array().into_iter().flatten()
                                            {
                                                let label = match item["source"].as_str() {
                                                    Some("reminder") => "Reminder supplied",
                                                    Some("revisit_cue") => "Revisit cue supplied",
                                                    Some("interpreted_prediction") => "Own prediction supplied",
                                                    Some("search_result") => "Found in search",
                                                    Some("selected_record") => {
                                                        "Selected full record"
                                                    }
                                                    Some("activity_feedback") => {
                                                        "Activity feedback pointer"
                                                    }
                                                    _ => "Received full record",
                                                };
                                                if ui
                                                    .small_button(label)
                                                    .on_hover_text(s(item, "reference"))
                                                    .clicked()
                                                {
                                                    inspect = Some(s(item, "reference").into());
                                                }
                                            }
                                            for revision in row["revisions_written"]
                                                .as_array()
                                                .into_iter()
                                                .flatten()
                                                .filter_map(Value::as_str)
                                            {
                                                let label = if revision.ends_with("@1") {
                                                    "Opened question"
                                                } else {
                                                    "Revised question"
                                                };
                                                if ui
                                                    .small_button(label)
                                                    .on_hover_text(revision)
                                                    .clicked()
                                                {
                                                    inspect = Some(revision.into());
                                                }
                                            }
                                            if let Some(application) = row["application"].as_str() {
                                                if ui
                                                    .small_button("Application record ↗")
                                                    .clicked()
                                                {
                                                    inspect = Some(application.into());
                                                }
                                            }
                                        });
                                        let detail = &row["application_detail"];
                                        if let Some(watch) = detail["watch_for"].as_str() {
                                            let action = &detail["action"];
                                            let role = match action["kind"].as_str() {
                                                Some("operation") => format!("Named action · {} ({})", s(action, "tool"), s(action, "id")),
                                                Some("reply") => "Tried in the reply".into(),
                                                _ => "Examined in this choice".into(),
                                            };
                                            ui.strong(role);
                                            ui.add(egui::Label::new(format!("Watching for · {watch}")).wrap());
                                        }
                                        for read in row["application_reads"].as_array().into_iter().flatten() {
                                            ui.strong("Received an earlier application record");
                                            ui.horizontal_wrapped(|ui| {
                                                if ui.small_button("Open application ↗").clicked() { inspect = Some(s(read, "reference").into()); }
                                                for reply in read["replies_supplied"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                                                    let number = reply.strip_prefix("experience:event-").and_then(|r| r.strip_suffix("@1")).unwrap_or(reply);
                                                    if ui.small_button(format!("Reply #{number} also supplied ↗")).clicked() { inspect = Some(reply.into()); }
                                                }
                                            });
                                            ui.weak("Receiving the records is not a finding or a character change.");
                                        }
                                        if let Some(reason) = row["reason"].as_str() {
                                            ui.add(egui::Label::new(reason).wrap());
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            if ui.small_button("Supplied input ↗").clicked() {
                                                inspect = Some(s(row, "input").into());
                                            }
                                            if ui.small_button("Accepted decision ↗").clicked() {
                                                inspect = Some(s(row, "decision").into());
                                            }
                                            if let Some(usage) = row["character_use"].as_str() {
                                                if ui
                                                    .small_button(format!(
                                                        "Character · epoch {} ↗",
                                                        row["epoch"]
                                                    ))
                                                    .clicked()
                                                {
                                                    inspect = Some(usage.into());
                                                }
                                            }
                                        });
                                    });
                                    ui.add_space(6.);
                                }
                        };
                        if compact { cards(ui); } else {
                            egui::ScrollArea::vertical()
                                .id_salt(("question-history-rows", reference, self.trail_only, self.page))
                                .show(ui, cards);
                        }
                    }
                }
            });
        if refresh {
            self.open(ctx, root, reference.clone(), title.clone());
        }
        if !visible || inspect.is_some() {
            self.selection = None;
            self.pending = None;
            self.result = None;
        }
        inspect
    }
}
