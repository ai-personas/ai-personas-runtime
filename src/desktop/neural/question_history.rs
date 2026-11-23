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
        egui::Window::new("Question history")
            .id(egui::Id::new("question-history"))
            .open(&mut visible)
            .default_width(680.)
            .default_height(510.)
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
                                &format!("Explicitly applied in {}", report["decisions_applied"]),
                                TEAL,
                            );
                            refresh = ui.small_button("Refresh").clicked();
                        });
                        let rows = report["decisions"]
                            .as_array()
                            .map(Vec::as_slice)
                            .unwrap_or_default();
                        const PAGE: usize = 8;
                        ui.horizontal(|ui| {
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
                        egui::ScrollArea::vertical()
                            .id_salt(("question-history-rows", reference, self.page))
                            .show(ui, |ui| {
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
                                                    .small_button("Applied in this choice ↗")
                                                    .clicked()
                                                {
                                                    inspect = Some(application.into());
                                                }
                                            }
                                        });
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
                            });
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
