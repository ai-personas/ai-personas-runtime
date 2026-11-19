use super::*;

#[derive(Default)]
pub(super) struct Window {
    event: Option<i64>,
    decisions: usize,
    reader: Option<mpsc::Receiver<Result<Value, String>>>,
    report: Option<Result<Value, String>>,
}

impl App {
    pub(super) fn open_message_receipt(&mut self, ctx: &egui::Context, event: i64) {
        let (root, ctx) = (self.root.clone(), ctx.clone());
        let (tx, rx) = mpsc::channel();
        self.message_receipt = Window {
            event: Some(event),
            decisions: self.known_decisions.len(),
            reader: Some(rx),
            report: None,
        };
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| ai_personas::neural::live::message_receipt(&db, event));
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    pub(super) fn message_receipt_window(&mut self, ctx: &egui::Context) {
        let Some(event) = self.message_receipt.event else {
            return;
        };
        if let Some(reader) = &self.message_receipt.reader {
            match reader.try_recv() {
                Ok(result) => {
                    self.message_receipt.report = Some(result);
                    self.message_receipt.reader = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message_receipt.report =
                        Some(Err("The receipt reader stopped.".to_owned()));
                    self.message_receipt.reader = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut visible = true;
        let mut inspect = None;
        let mut refresh = false;
        egui::Window::new("Message delivery").id(egui::Id::new("live-message-receipt")).open(&mut visible)
            .default_width(400.).resizable(true).show(ctx, |ui| {
                let report = match &self.message_receipt.report {
                    Some(Ok(report)) => report,
                    Some(Err(error)) => {ui.label(error); refresh = ui.button("Retry").clicked(); return;}
                    None => {ui.spinner(); return;}
                };
                let recipient = self.label(s(report, "recipient"));
                ui.label(RichText::new(format!("Message to {recipient}")).strong().size(19.));
                ui.weak(format!("Sent by {} · {}", self.label(s(report,"sender")), time_label(report["sent_at"].as_i64().unwrap_or(0))));
                ui.add_space(6.);
                ui.label(excerpt(s(report, "text"), 360));
                ui.add_space(8.);
                ui.separator();
                if let Some(call) = report["received"]["decision"].as_str() {
                    theme::badge(ui, "Received in a decision", TEAL);
                    let when = time_label(report["received"]["accepted_at"].as_i64().unwrap_or(0));
                    ui.label(format!("{recipient} received this text in the input to a decision accepted at {when}."));
                    if ui.button("Inspect decision").clicked() {inspect=Some(call.to_owned());}
                } else {
                    theme::badge(ui, "Message recorded", PURPLE);
                    ui.label("Studio will show a receipt here when it can verify this incoming text in an accepted decision.");
                    refresh = self.message_receipt.decisions != self.known_decisions.len();
                }
            });
        if !visible {
            self.message_receipt = Default::default();
        } else if refresh {
            self.open_message_receipt(ctx, event);
        }
        if let Some(reference) = inspect {
            self.message_receipt = Default::default();
            self.open_resource(ctx, reference);
        }
    }
}
