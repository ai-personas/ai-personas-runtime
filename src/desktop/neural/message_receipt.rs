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
        egui::Window::new("Delivery status").id(egui::Id::new("live-message-receipt")).open(&mut visible)
            .default_width(425.).resizable(true).vscroll(true).show(ctx, |ui| {
                let report = match &self.message_receipt.report {
                    Some(Ok(report)) => report,
                    Some(Err(error)) => {ui.label(error); refresh = ui.button("Retry").clicked(); return;}
                    None => {ui.spinner(); return;}
                };
                let recipient = self.label(s(report, "recipient"));
                let (heading, noun) = if report["kind"] == "human_request_progress" {
                    if report["state"] == "in_progress" {("Request claim","claim")} else {("Request release","release")}
                } else if report["request"].is_string() {("Human report","report")} else {("Message","message")};
                ui.label(RichText::new(format!("{heading} to {recipient}")).strong().size(19.));
                ui.weak(format!("Sent by {} · {}", self.label(s(report,"sender")), time_label(report["sent_at"].as_i64().unwrap_or(0))));
                ui.add_space(6.);
                ui.label(excerpt(s(report, "text"), 360));
                ui.add_space(8.);
                ui.separator();
                if let Some(call) = report["received"]["decision"].as_str() {
                    theme::badge(ui, "Received in a decision", TEAL);
                    let when = time_label(report["received"]["accepted_at"].as_i64().unwrap_or(0));
                    ui.label(format!("{recipient} received this {noun} in a decision accepted at {when}."));
                    if ui.button("Inspect decision").clicked() {inspect=Some(call.to_owned());}
                } else {
                    theme::badge(ui, &format!("{heading} recorded"), PURPLE);
                    ui.label(format!("Studio will show a receipt when it verifies this {noun} in an accepted decision."));
                }
                let files=report["attachments"].as_array().map(Vec::as_slice).unwrap_or_default();
                if !files.is_empty() {
                    ui.add_space(8.);
                    ui.separator();
                    ui.strong("Returned files");
                }
                for (index,file) in files.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        let size=file["bytes"].as_u64().unwrap_or(0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.link(s(file,"name")).clicked() {inspect=Some(s(file,"reference").to_owned());}
                            ui.weak(format!("{size} bytes"));
                        });
                        let supplied=&file["supplied"];
                        if let Some(call)=supplied["decision"].as_str() {
                            let start=supplied["range"][0].as_u64().unwrap_or(0);
                            let end=supplied["range"][1].as_u64().unwrap_or(0);
                            let label=if supplied["kind"]=="image" {"Image supplied".to_owned()}
                                else if start==0 && end==size {"Whole text supplied".into()}
                                else {format!("Text supplied · bytes {start}..{end}")};
                            ui.horizontal_wrapped(|ui| {
                                theme::badge(ui,&label,TEAL);
                                if ui.small_button("Inspect file receipt").clicked() {inspect=Some(call.to_owned());}
                            });
                            if let Some(later)=file["whole"]["decision"].as_str().filter(|_|end-start<size) {
                                if ui.small_button("Inspect whole-file receipt").clicked() {inspect=Some(later.to_owned());}
                            }
                        } else if report["received"].is_object() {ui.weak("File reference received");}
                          else {ui.weak("Attachment recorded");}
                    });
                }
                let pending=report["received"].is_null() || files.iter().any(|f|f["whole"].is_null());
                let unverified=report["unverified_candidates"].as_u64().unwrap_or(0);
                if unverified>0 {ui.weak(format!("Unverified candidate inputs · {unverified}"));}
                refresh = pending && self.message_receipt.decisions != self.known_decisions.len();
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
