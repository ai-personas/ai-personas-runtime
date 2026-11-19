use super::*;

#[derive(Default)]
pub(super) struct Window {
    event: Option<i64>,
    decisions: usize,
    cursor: i64,
    reader: Option<mpsc::Receiver<Result<Value, String>>>,
    report: Option<Result<Value, String>>,
}

impl App {
    pub(super) fn open_message_receipt(&mut self, ctx: &egui::Context, event: i64) {
        self.message_receipt = Window {
            event: Some(event),
            ..Default::default()
        };
        self.refresh_message_receipt(ctx, event);
    }

    fn refresh_message_receipt(&mut self, ctx: &egui::Context, event: i64) {
        let (root, ctx) = (self.root.clone(), ctx.clone());
        let (tx, rx) = mpsc::channel();
        self.message_receipt.decisions = self.known_decisions.len();
        self.message_receipt.cursor = self.cursor;
        self.message_receipt.reader = Some(rx);
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
        let mut open_file = None;
        let mut open_package = None;
        let mut refresh = false;
        let feedback = self.message_receipt.report.as_ref()
            .and_then(|r| r.as_ref().ok())
            .is_some_and(|r| r["delivery"].is_string());
        let title = if feedback { "Feedback receipt" } else { "Delivery status" };
        let viewport = ctx.content_rect().shrink(6.);
        let style = ctx.style();
        let frame = egui::Frame::window(&style);
        let heading = egui::TextStyle::Heading.resolve(&style);
        let title_height = ctx.fonts_mut(|fonts| fonts.row_height(&heading)).max(style.spacing.interact_size.y);
        let chrome = frame.total_margin().sum()
            + egui::vec2(0., title_height + frame.inner_margin.sum().y + frame.stroke.width);
        let available = (viewport.size() - chrome).max(egui::vec2(1., 1.));
        let initial = egui::vec2(520., 620.).min(available);
        egui::Window::new(title).id(egui::Id::new("human-message-receipt")).open(&mut visible)
            .default_size(initial).default_pos(viewport.center() - (initial + chrome) / 2.)
            .min_size(egui::vec2(220., 110.).min(available)).max_size(available)
            .constrain_to(viewport).resizable(true).vscroll(true).show(ctx, |ui| {
                let report = match &self.message_receipt.report {
                    Some(Ok(report)) => report,
                    Some(Err(error)) => {ui.label(error); refresh = ui.button("Retry").clicked(); return;}
                    None => {ui.spinner(); return;}
                };
                let recipient = self.label(s(report, "recipient"));
                let delivery = report["delivery"].as_str();
                let (heading, noun) = if delivery.is_some() {
                    ("Package feedback", "feedback")
                } else if report["kind"] == "human_request_progress" {
                    if report["state"] == "in_progress" {("Request claim","claim")} else {("Request release","release")}
                } else if report["request"].is_string() {("Human report","report")} else {("Message","message")};
                ui.label(RichText::new(format!("{heading} to {recipient}")).strong().size(19.));
                let action = if delivery.is_some() { "Recorded" } else { "Sent" };
                ui.weak(format!("{action} by {} · {}", self.label(s(report,"sender")), time_label(report["sent_at"].as_i64().unwrap_or(0))));
                if let Some(reference) = delivery {
                    if live_composer::feedback_link(ui, s(report, "delivery_title"), reference, usize::MAX).clicked() {
                        open_package = Some((reference.to_owned(), s(report, "work").to_owned()));
                    }
                }
                ui.add_space(6.);
                if let Some(text) = report["text"].as_str().filter(|text| !text.trim().is_empty()) {
                    ui.label(excerpt(text, 360));
                }
                ui.add_space(8.);
                ui.separator();
                if let Some(call) = report["received"]["decision"].as_str() {
                    theme::badge(ui, "Received in a decision", TEAL);
                    let when = time_label(report["received"]["accepted_at"].as_i64().unwrap_or(0));
                    ui.label(format!("{recipient} received this {noun} in a decision accepted at {when}."));
                    if ui.button("Inspect decision").clicked() {inspect=Some(call.to_owned());}
                } else {
                    theme::badge(ui, &format!("{heading} recorded"), PURPLE);
                    if delivery.is_some() {
                        ui.label(format!("Studio has yet to verify this feedback in one of {recipient}’s accepted decisions."));
                    } else {
                        ui.label(format!("Studio will show a receipt when it verifies this {noun} in an accepted decision."));
                    }
                }
                let files=report["attachments"].as_array().map(Vec::as_slice).unwrap_or_default();
                let source = format!("experience:event-{event}@1");
                let repeated = overview::repeated_file_names(files);
                if !files.is_empty() {
                    ui.add_space(8.);
                    ui.separator();
                    ui.strong(if report["request"].is_string() { "Returned files" } else { "Attached files" });
                }
                for (index,file) in files.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        let size=file["bytes"].as_u64().unwrap_or(0);
                        if let Some(selected) = overview::source_file_link(ui, file, &source, &repeated) {
                            open_file = Some(selected);
                        }
                        let supplied=&file["supplied"];
                        if let Some(call)=supplied["decision"].as_str() {
                            let start=supplied["range"][0].as_u64().unwrap_or(0);
                            let end=supplied["range"][1].as_u64().unwrap_or(0);
                            let label=if file["pdf_pages"].is_array() && supplied["kind"]=="text" {
                                    format!("Source bytes supplied as text · {start}..{end}")
                                } else if supplied["kind"]=="image" {"Image supplied".to_owned()}
                                else if start==0 && end==size {"Whole text supplied".into()}
                                else {format!("Text supplied · bytes {start}..{end}")};
                            ui.horizontal_wrapped(|ui| {
                                theme::badge(ui,&label,TEAL);
                                if ui.small_button("Inspect file receipt").clicked() {inspect=Some(call.to_owned());}
                            });
                            if let Some(later)=file["whole"]["decision"].as_str().filter(|_|end-start<size) {
                                let label = if file["pdf_pages"].is_array() {"Inspect source-byte receipt"} else {"Inspect whole-file receipt"};
                                if ui.small_button(label).clicked() {inspect=Some(later.to_owned());}
                            }
                        } else if report["received"].is_object() {ui.weak("File reference received");}
                          else {ui.weak("Attachment recorded");}
                        if let Some(pages) = file["pdf_pages"].as_array() {
                            if let Some(reference) = page_receipts(ui, file, pages, recipient) {
                                inspect = Some(reference);
                            }
                        }
                    });
                }
                let pending=report["received"].is_null() || files.iter().any(|f|f["whole"].is_null());
                let unverified=report["unverified_candidates"].as_u64().unwrap_or(0);
                if unverified>0 {ui.weak(format!("Unverified candidate inputs · {unverified}"));}
                refresh = (pending && self.message_receipt.decisions != self.known_decisions.len())
                    || (self.message_receipt.cursor != self.cursor && files.iter().any(|f|f["pdf_pages"].is_array()));
            });
        if !visible {
            self.message_receipt = Default::default();
        } else if refresh && self.message_receipt.reader.is_none() {
            self.refresh_message_receipt(ctx, event);
        }
        if let Some((reference, work)) = open_package {
            self.message_receipt = Default::default();
            self.open_home_delivery(&reference, &work);
        } else if let Some(reference) = inspect {
            self.message_receipt = Default::default();
            self.open_resource(ctx, reference);
        } else if let Some((reference, name)) = open_file {
            self.message_receipt = Default::default();
            self.open_named_resource(ctx, reference, Some((format!("experience:event-{event}@1"), name)));
        }
    }
}

fn page_receipts(ui: &mut Ui, file: &Value, pages: &[Value], recipient: &str) -> Option<String> {
    if pages.is_empty() {
        ui.weak("No page render verified for this PDF.");
        return None;
    }
    let supplied = pages.iter().filter(|p| p["supplied"].is_object()).count();
    let mut inspect = None;
    egui::CollapsingHeader::new(format!("PDF pages · {} rendered · {supplied} supplied", pages.len()))
        .id_salt(("pdf-pages", s(file, "reference"))).default_open(pages.len() <= 5)
        .show(ui, |ui| {
            for page in pages {
                ui.push_id(page["page"].as_u64(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("Page {} of {}", page["page"], page["page_count"]))
                            .on_hover_text(format!("{}\n{}\n{}", s(file, "name"), s(page,"source_bytes"), s(page,"image")));
                        if page["supplied"].is_object() {
                            theme::badge(ui, "Page pixels supplied", TEAL);
                        } else {
                            theme::badge(ui, "Page rendered", theme::palette(ui).muted);
                        }
                    });
                    if let Some(decision) = page["supplied"]["decision"].as_str() {
                        let when = time_label(page["supplied"]["accepted_at"].as_i64().unwrap_or_default());
                        ui.small(format!("First verified in {recipient}’s decision · {when}"));
                        if ui.small_button("Inspect receiving decision").clicked() {
                            inspect = Some(decision.into());
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.link("Page image").on_hover_text(s(page, "image")).clicked() {
                            inspect = Some(s(page, "image").into());
                        }
                        if ui.link("Render receipt").on_hover_text(s(&page["render"], "event")).clicked() {
                            inspect = Some(s(&page["render"], "event").into());
                        }
                        if page["source"] != page["source_bytes"]
                            && ui.link("Source revision").on_hover_text(s(page, "source")).clicked() {
                            inspect = Some(s(page, "source").into());
                        }
                    });
                });
            }
        });
    inspect
}
