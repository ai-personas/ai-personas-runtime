use super::*;

pub(super) fn switch_heading(ui: &mut Ui, row: &Value) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(if row["next_model_choice"] == "rollback" {
            "Rollback choice"
        } else {
            "Activation choice"
        });
        if let Some(n) = row["model_transition"]["reference"]
            .as_str()
            .and_then(|r| r.strip_prefix("experience:event-"))
            .and_then(|r| r.strip_suffix("@1"))
        {
            ui.small(format!("Switch #{n}"));
        }
    });
}

/// The choice to try a practice (or to change a question) is not a switch.
/// Conversely, the switch decision itself still received the earlier package.
pub(super) fn at_choice(
    ui: &mut Ui,
    row: &Value,
    owner: &str,
    origin: impl FnOnce() -> Return,
    action: &mut Option<Action>,
) {
    ui.horizontal_wrapped(|ui| {
        if let Some(epoch) = row["epoch"].as_u64() {
            theme::badge(ui, &format!("Input · epoch {epoch}"), TEAL);
            if ui.small_button("Numerical paths…").on_hover_text(s(row, "input_package")).clicked() {
                *action = Some(Action::Paths {
                    decision: s(row, "decision").into(),
                    owner: owner.into(),
                    utc: s(row, "accepted_utc").into(),
                });
            }
        } else {
            ui.weak("No active input model");
        }
        let change = &row["model_transition"];
        if let (Some(event), Some(epoch)) = (change["reference"].as_str(), change["epoch"].as_u64()) {
            let label = if change["kind"] == "rollback" { "Rolled back" } else { "Activated" };
            theme::badge(ui, &format!("{label} · epoch {epoch}"), PURPLE);
            if ui.small_button("Inspect later decisions →").on_hover_text(event).clicked() {
                // Resolve from the authorizing decision, not just the cached event number.
                *action = Some(Action::Generation {
                    owner: owner.into(),
                    decision: s(row, "decision").into(),
                    origin: origin(),
                });
            }
        } else if row["next_model_choice"] == "preserve" {
            ui.small("Chose to keep the model");
        } else if row["next_model_choice"] == "rollback" {
            ui.label("Chose rollback · switch unverified");
        } else {
            ui.label("Chose activation · switch unverified");
        }
    });
    if row["input_package"].is_string() && row["character_use"].is_null() && row["character_use_error"].is_null() {
        ui.weak("No separate character-use record is retained for this input.");
    }
    for key in ["model_transition_error", "character_use_error"] {
        if let Some(reason) = row[key].as_str() {
            ui.colored_label(ui.visuals().error_fg_color, reason);
        }
    }
}
