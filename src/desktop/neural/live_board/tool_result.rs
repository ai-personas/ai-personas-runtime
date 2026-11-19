use super::*;

impl App {
    fn latest_tool_notice(&self, work: &str, person: &str) -> Option<(i64, &Value)> {
        let seq = self.home_index.latest_tool_event(work, person)?;
        let index = self
            .events
            .binary_search_by_key(&seq, |e| e["event"].as_i64().unwrap_or(0))
            .ok()?;
        Some((seq, &self.events[index]))
    }

    pub(in super::super) fn overview_tool_result(
        &self,
        ui: &mut Ui,
        work: &str,
        person: &str,
    ) -> Option<String> {
        let (seq, event) = self.latest_tool_notice(work, person)?;
        let receipt = &event["tool_outcome"];
        let reference = receipt["result"].as_str()?;
        let tool = s(&event["body"], "tool");
        let mut open = false;
        ui.horizontal_wrapped(|ui| {
            ui.weak(RichText::new("Last tool").small());
            open = ui.link(RichText::new(operation_name(tool)).small())
                .on_hover_text(format!("Open the {tool} receipt · event {seq}\n{}", s(event, "created_utc")))
                .clicked();
            if let Some(status) = result_status(receipt) {
                ui.colored_label(theme::palette(ui).ink(theme::ACCENT), RichText::new(status).small());
            }
        });
        open.then(|| reference.into())
    }

    pub(super) fn last_tool_result(&mut self, ui: &mut Ui, person: &str, now: i64) {
        let Some((seq, event)) = self.latest_tool_notice(&self.work, person) else {
            return;
        };
        let receipt = &event["tool_outcome"];
        let Some(reference) = receipt["result"].as_str() else {
            return;
        };
        let tool = s(&event["body"], "tool");
        let stamp = s(event, "created_utc");
        let time = if event["created"]
            .as_i64()
            .is_some_and(|s| s.div_euclid(86400) == now.div_euclid(86400))
        {
            stamp.get(11..).unwrap_or(stamp)
        } else {
            stamp
        };
        let mut open_result = false;
        let mut open_event = false;
        let mut open_pdf = None;
        ui.push_id(("latest-tool", seq), |ui| {
            ui.add_space(3.);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Last tool result").small().strong());
                if !time.is_empty() {
                    ui.label(RichText::new(time).small().weak())
                        .on_hover_text(stamp);
                }
            });
            ui.horizontal_wrapped(|ui| {
                open_result = ui
                    .link(operation_name(tool))
                    .on_hover_text(format!("Open the exact {tool} receipt\n{reference}"))
                    .clicked();
                if let Some(status) = result_status(receipt) {
                    ui.label(
                        RichText::new(status)
                            .small()
                            .color(theme::palette(ui).ink(theme::ACCENT)),
                    );
                }
                if let Some(pdf) = receipt["pdf"].as_str() {
                    if text_action(ui, "Open PDF").on_hover_text(s(receipt, "label")).clicked() {
                        open_pdf = Some(pdf.to_owned());
                    }
                }
                open_event = text_action(ui, &format!("Event #{seq}"))
                    .on_hover_text("Locate this recorded tool action in the activity timeline")
                    .clicked();
            });
        });
        if open_result {
            self.open_resource(ui.ctx(), reference.into());
        } else if let Some(pdf) = open_pdf {
            self.open_resource(ui.ctx(), pdf);
        } else if open_event {
            self.live_board.show_event(&self.work, seq);
        }
    }
}

// The card describes the recorded operation. Arguments, result bodies and
// error messages are available only through an explicit inspection action.
fn operation_name(tool: &str) -> &str {
    match tool {
        "tools.search" => "Tool catalogue search",
        "tools.describe" => "Tool definitions",
        "asset.search" => "File catalogue search",
        "asset.history" => "File history",
        "asset.find" => "File text search",
        "asset.read" => "File or record read",
        "asset.save" => "File save",
        "asset.import" => "File import",
        "asset.register" => "File registration",
        "asset.organize" => "File organization",
        "asset.feedback" => "File feedback",
        "asset.feedback_history" => "File feedback history",
        "workspace.list" => "Directory listing",
        "command.history" => "Command history",
        "command.install" => "Command installation",
        "command.run" | "host.command" => "Command run",
        "host.start" => "Background job",
        "host.cancel" => "Job cancellation",
        "history.search" => "Activity history search",
        "experience.search" => "Experience search",
        "persona.read" => "Character record read",
        "persona.evaluate" => "Character evaluation",
        "persona.revision_uses" => "Character revision history",
        "persona.paths" => "Character decision trace",
        "persona.patch" => "Character revision",
        "persona.assess" => "Character comparison",
        "persona.create" => "Persona creation",
        "relationship.contacts" => "Contact search",
        "relationship.history" => "Relationship history",
        "relationship.record" => "Relationship note",
        "relationship.retire" => "Relationship note retirement",
        "development.search" => "Development question search",
        "development.update" => "Development question update",
        "development.close" => "Development question closure",
        "compute.inspect" => "Computation interface",
        "compute.run" => "Computation",
        "geometry.inspect_stl" => "Mesh measurements",
        "geometry.profile_stl" => "Mesh profile",
        "geometry.history" => "Model render history",
        "geometry.install" => "Model compiler installation",
        "geometry.render" => "Model render",
        "image.read" => "Image read",
        "image.render_svg" => "SVG render",
        "document.render_pdf" => "PDF page render",
        "document.print_html" => "HTML print layout",
        "peer.message" => "Persona message",
        "peer.invite" => "Persona invitation",
        "peer.respond" => "Invitation response",
        "work.commitment" => "Commitment update",
        "work.leave" => "Activity departure",
        "work.assume" => "Outcome assignment",
        "work.handoff" => "Outcome handoff",
        "work.outcome" => "Outcome update",
        "work.request" => "Activity request",
        "work.requests" => "Activity request history",
        "work.sources" => "Earlier work lookup",
        "work.respond" => "Activity reply",
        "result.review" => "Package review",
        "result.reviews" => "Package review history",
        "result.deliver" => "Package delivery",
        "human.request" => "Human request",
        "human.withdraw" => "Human request withdrawal",
        _ => tool,
    }
}

fn result_status(receipt: &Value) -> Option<&'static str> {
    match s(receipt, "label") {
        "Superseded before dispatch" => Some("Not dispatched"),
        "Job submitted" => Some("Submitted"),
        "Cancellation requested" => Some("Cancellation requested"),
        "Canceled by control" => Some("Canceled"),
        "Print cancelled" => Some("Cancelled"),
        "Rendered with warnings" => Some("Warnings"),
        "Timed out" => Some("Timed out"),
        "Failed" | "Render failed" | "Print layout failed" | "Could not complete" => Some("Failed"),
        "Did not start" => Some("Did not start"),
        "Command outcome unavailable"
        | "Computation outcome unavailable"
        | "Outcome unavailable" => Some("Outcome unavailable"),
        "PDF page unavailable" => Some("Page unavailable"),
        _ if receipt["failure"].is_string() || receipt["error"].is_string() => Some("Failed"),
        _ if receipt["exit_code"].as_i64().is_some_and(|code| code != 0) => Some("Nonzero exit"),
        _ if receipt["missing_outputs"]
            .as_array()
            .is_some_and(|files| !files.is_empty()) =>
        {
            Some("Missing outputs")
        }
        _ if receipt["problem"] == true => Some("Check result"),
        _ => None,
    }
}
