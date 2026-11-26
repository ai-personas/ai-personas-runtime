use super::*;

impl App {
    /// The digital edition and physical claim have different authors and
    /// lifetimes. Opening or saving the packet does not answer the request.
    pub(super) fn field_packet(&mut self, ui: &mut Ui, record: &Value) {
        let b = &record["body"];
        let packet = &b["field_packet"];
        let Some(delivery) = packet["delivery"].as_str() else {
            return;
        };
        let p = theme::palette(ui);
        let issuer = s(b, "requester");
        let accent = p.ink(self.color(issuer));
        let files = b["artifacts"].as_array().map_or(0, Vec::len);
        let edition = s(packet, "edition");
        let package = b["delivery_details"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["reference"] == delivery);
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1., accent.gamma_multiply(0.65)))
            .corner_radius(7)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    theme::glyph(ui, Icon::Files, accent);
                    ui.label(design::caption("Field packet", accent));
                    if let Some((_, n)) = edition.rsplit_once('@') {
                        ui.small(format!("Issue {n}")).on_hover_text(edition);
                    }
                    ui.small(format!("Prepared by {}", self.label(issuer)));
                });
                if let Some(summary) = package {
                    ui.add(egui::Label::new(design::display(s(summary, "title"), 26.)).wrap());
                    ui.label(s(summary, "supported_use"));
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(
                            ui,
                            ai_personas::neural::human_requests::declared_scope(s(
                                summary, "scope",
                            )),
                            accent,
                        );
                        ui.small(counted(files as u64, "issued file", "issued files"));
                    });
                }
                ui.add_space(6.);
                ui.label(design::caption("Instructions for this issue", p.muted));
                ui.add(
                    egui::Label::new(s(packet, "instructions"))
                        .wrap()
                        .selectable(true),
                );
                ui.add_space(5.);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(theme::primary("Save field packet…"))
                        .on_hover_text(
                            "Save this request sheet and its exact current files together",
                        )
                        .clicked()
                    {
                        self.save_request_handoff(&handle(record), s(b, "title"), files);
                    }
                    if ui
                        .link("Inspect delivery →")
                        .on_hover_text(delivery)
                        .clicked()
                    {
                        self.open_issued_package(ui.ctx(), delivery);
                    }
                    if let Some(seq) = self.home_index.request_origin(edition) {
                        if ui
                            .link("What changed?")
                            .on_hover_text(format!("Compare attachments at issue #{seq}"))
                            .clicked()
                        {
                            self.open_packet_changes(ui.ctx(), seq, edition, s(b, "work"));
                        }
                    }
                });
            });
    }
}
