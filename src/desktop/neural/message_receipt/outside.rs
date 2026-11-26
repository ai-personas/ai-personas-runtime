use super::*;

#[derive(Default)]
pub(super) struct Asking {
    open_after_check: Option<Value>,
    changed: bool,
}

impl Asking {
    /// Only navigate if a fresh read verifies the same report body,
    /// publication, operation and accepted response.
    pub(super) fn checked(&mut self, report: &Result<Value, String>) -> Option<String> {
        let expected = self.open_after_check.take()?;
        let current = report.as_ref().ok().map(|r| &r["requesting_action"]["episode"]);
        self.changed = current != Some(&expected);
        if self.changed { return None; }
        expected["decision"].as_str().map(str::to_owned)
    }

    pub(super) fn show(&mut self, ui: &mut Ui, report: &Value, recipient: &str, loading: bool, refresh: &mut bool) {
        if self.changed {
            ui.label("The requesting choice could not be verified again. Nothing was opened.");
        }
        if let Some(reason) = report["requesting_action_error"].as_str() {
            ui.weak("Requesting choice unavailable").on_hover_text(reason);
            if ui.small_button("Check requesting choice again").clicked() {
                self.changed = false;
                *refresh = true;
            }
            return;
        }
        let trace = &report["requesting_action"];
        if !trace.is_object() { return; }
        egui::CollapsingHeader::new(format!("Why {recipient} asked"))
            .id_salt((s(report, "message"), "asking-choice"))
            .show(ui, |ui| {
                let episode = &trace["episode"];
                let acted = &episode["performed"];
                let issue = s(acted, "publication");
                let number = issue.strip_prefix("experience:event-").and_then(|v| v.strip_suffix("@1")).unwrap_or(issue);
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("Request #{number} · {}", s(&acted["endpoint"], "id"))).on_hover_text(issue);
                    if ui.add_enabled(!loading, egui::Button::new("Open asking decision").small()).clicked() {
                        self.open_after_check = Some(episode.clone());
                        *refresh = true;
                    }
                    if ui.small_button("Recheck link").clicked() {
                        self.changed = false;
                        *refresh = true;
                    }
                });
                ui.label(s(episode, "judgment_before")).on_hover_text(s(episode, "decision"));
                terms(ui, acted);
                ui.label(match s(trace, "card_in_received_input") {
                    "supplied" => "This action was paired with the report in the receiving decision’s input.",
                    "not_supplied" => "The receiving decision did not get this action card with the report. The link is reconstructed here.",
                    "differs" => "The receiving input’s action card differs from this verified link.",
                    _ => "No verified receiving input to compare with this link.",
                });
                if let Some(reason) = trace["character_error"].as_str() {
                    ui.weak("Character at request unavailable").on_hover_text(reason);
                    *refresh |= ui.small_button("Recheck character").clicked();
                } else if trace["character_at_request"].is_object() {
                    ui.collapsing("Character when asking", |ui| numbers(ui, &trace["character_at_request"]));
                }
            });
    }
}

fn terms(ui: &mut Ui, acted: &Value) {
    ui.collapsing("What was requested", |ui| {
        ui.label(s(acted, "asked"));
        if let Some(text) = acted["return_requested"].as_str() {
            ui.small("Requested return");
            ui.label(text);
        }
    });
    let revision = |key| s(acted, key).rsplit_once('@').map(|(_, n)| n).unwrap_or("?");
    let (base, edition) = (revision("response_base"), revision("reported_request"));
    let result = match s(acted, "operator_action") {
        "complete" => "Completion reported",
        "unable" => "Unable to complete",
        "contribute" => "Contribution",
        "take" => "Taken with a note or file",
        "release" => "Released with a note or file",
        _ => "Progress report",
    };
    ui.small(format!("{result} · request edition {base} → {edition}. This is not a verdict on the files."))
        .on_hover_text(s(acted, "reported_request"));
}

fn numbers(ui: &mut Ui, character: &Value) {
    ui.small(format!("Epoch {} · {} · supplied to the asking decision", character["epoch"], title(s(character, "basis"))))
        .on_hover_text(s(character, "character_use"));
    let bars = ui.available_width() >= 280.;
    egui::Grid::new("asking-inclinations").striped(true).show(ui, |ui| {
        for axis in FeatureContract::standard().outputs {
            if let Some(value) = character["outputs"][&axis.id].as_f64() {
                ui.label(title(&axis.id)).on_hover_text(axis.meaning);
                ui.monospace(format!("{value:+.4}")).on_hover_text(value.to_string());
                if bars { character_paths::inclination_bar(ui, value); }
                ui.end_row();
            }
        }
    });
    ui.weak("Inclinations, not proof of what caused the choice.");
}
