//! Human feedback and attributable replies for the selected, immutable edition.
use super::*;
use ai_personas::neural::delivery_feedback;
use std::path::Path;

const PAGE: usize = 4;

fn message_preview(ui: &mut Ui, reference: &str, event: i64, text: &str) {
    let key = egui::Id::new(("edition-message-preview", reference, event));
    let preview = ui.ctx().data_mut(|cache| {
        cache
            .get_temp_mut_or_insert_with(key, || document::excerpt(text, 360))
            .clone()
    });
    ui.add(egui::Label::new(preview).wrap());
}

#[derive(Default)]
pub(super) struct Panel {
    reference: String,
    offset: usize,
    sequence: Option<i64>,
    reader: Option<mpsc::Receiver<Result<Value, String>>>,
    report: Option<Result<Value, String>>,
}

impl Panel {
    fn read(&mut self, ctx: &egui::Context, root: &Path, sequence: Option<i64>) {
        self.sequence = sequence;
        let (tx, rx) = mpsc::channel();
        self.reader = Some(rx);
        let (ctx, root, reference, offset) = (
            ctx.clone(),
            root.to_owned(),
            self.reference.clone(),
            self.offset,
        );
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| delivery_feedback::history(&db, &reference, offset, PAGE));
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn update(&mut self, ctx: &egui::Context, root: &Path, reference: &str, sequence: Option<i64>) {
        if self.reference != reference {
            *self = Self {
                reference: reference.into(),
                ..Self::default()
            };
        }
        if let Some(rx) = &self.reader {
            match rx.try_recv() {
                Ok(page) => {
                    self.report = Some(page);
                    self.reader = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.report = Some(Err("The feedback reader stopped.".into()));
                    self.reader = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.reader.is_none() && (self.report.is_none() || self.sequence != sequence) {
            self.read(ctx, root, sequence);
        }
    }
}

impl App {
    pub(super) fn delivery_followup_summary(&mut self, ui: &mut Ui, reference: &str) {
        let sequence = self.home_index.latest_event(&self.work);
        self.delivery_followups
            .update(ui.ctx(), &self.root, reference, sequence);
        let panel = &self.delivery_followups;
        let Some(result) = &panel.report else {
            ui.spinner();
            return;
        };
        let report = match result {
            Ok(report) => report,
            Err(problem) => {
                ui.colored_label(ui.visuals().error_fg_color, problem);
                if ui.button("Retry feedback").clicked() {
                    self.delivery_followups.read(ui.ctx(), &self.root, sequence);
                }
                return;
            }
        };
        let total = report["total"].as_u64().unwrap_or(0) as usize;
        if total == 0 {
            ui.weak("No human feedback on this edition yet.");
            return;
        }
        let mut receipt = None;
        let mut open = None;
        let mut file = None;
        for message in report["messages"].as_array().into_iter().flatten() {
            let Some(event) = message["event"].as_i64() else {
                continue;
            };
            ui.push_id(("delivery-feedback", event), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(s(message, "sender"));
                    ui.weak(s(message, "utc"));
                });
                let text = s(message, "text");
                if !text.is_empty() {
                    message_preview(ui, reference, event, text);
                }
                let source = s(message, "experience");
                if let Some((r, name)) =
                    overview::source_file_links(ui, &message["attachments"], source)
                {
                    file = Some((r, (source.to_owned(), name)));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.link("Open message").clicked() {
                        open = Some(event);
                    }
                    if ui.link("Check receipt").clicked() {
                        receipt = Some(event);
                    }
                });
                for reply in message["replies"].as_array().into_iter().flatten() {
                    let Some(reply_event) = reply["event"].as_i64() else {
                        continue;
                    };
                    ui.indent(reply_event, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!("Reply from {}", self.label(s(reply, "sender"))));
                            ui.weak(s(reply, "utc"));
                        });
                        message_preview(ui, reference, reply_event, s(reply, "text"));
                        if ui.link("Open reply").clicked() {
                            open = Some(reply_event);
                        }
                    });
                }
                ui.add_space(3.);
                ui.separator();
            });
        }
        let mut offset = panel.offset;
        if total > PAGE {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(offset > 0, egui::Button::new("Newer"))
                    .clicked()
                {
                    offset = offset.saturating_sub(PAGE);
                }
                ui.small(format!(
                    "{}–{} of {total}",
                    panel.offset + 1,
                    total.min(panel.offset + PAGE)
                ));
                if ui
                    .add_enabled(panel.offset + PAGE < total, egui::Button::new("Older"))
                    .clicked()
                {
                    offset = panel.offset + PAGE;
                }
            });
        }
        if offset != panel.offset {
            self.delivery_followups.offset = offset;
            self.delivery_followups.report = None;
            self.delivery_followups.read(ui.ctx(), &self.root, sequence);
        }
        if let Some(event) = receipt {
            self.open_message_receipt(ui.ctx(), event);
        }
        if let Some(event) = open {
            self.open_home_event(&self.work.clone(), event);
        }
        if let Some((r, source)) = file {
            self.open_named_resource(ui.ctx(), r, Some(source));
        }
    }
}
