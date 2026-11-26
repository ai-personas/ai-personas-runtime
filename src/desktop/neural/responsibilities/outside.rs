//! Outside work keeps its own holder and lifecycle when an activity ends.
use super::*;

fn address(step: &Value) -> Option<(&str, &str)> {
    step["reference"]
        .as_str()?
        .strip_prefix("human_request:")?
        .rsplit_once('@')
}

impl Board {
    pub(in super::super) fn closing_outside(&self, ui: &mut Ui, work: &str) {
        if let Some(report) = self.report.as_ref().filter(|r| r["work"] == work) {
            let steps = items(report, "outside_steps");
            let count = |state: &str| steps.iter().filter(|s| s["state"] == state).count();
            ui.label(format!(
                "Outside work last observed · {} not taken · {} in progress",
                count("open"),
                count("in_progress")
            ));
            let missing = steps
                .iter()
                .filter(|s| s["unavailable"].is_string())
                .count();
            if missing > 0 {
                ui.weak(format!(
                    "Additional requests with unknown status: {missing}"
                ));
            }
        }
        ui.weak("Ending the activity records the request editions at that moment. It does not cancel outside work or release its holder.");
    }
}

fn status(ui: &mut Ui, members: &[Value], step: &Value) {
    ui.horizontal_wrapped(|ui| {
        let (status, color) = inbox::phase(s(step, "state"));
        theme::badge(ui, status, color);
        if let Some((_, edition)) = address(step) {
            ui.small(format!("Edition {edition}"))
                .on_hover_text(s(step, "reference"));
        }
        ui.small(format!(
            "Requested by {}",
            person(members, s(step, "requester"))
        ));
        if let Some(who) = step["claimed_by"].as_str() {
            let verb = if step["state"] == "in_progress" {
                "Held by"
            } else {
                "Reported by"
            };
            ui.small(format!("{verb} {who}"));
        } else if step["state"] == "open" {
            ui.small("Not yet taken");
        }
    });
}

pub(super) fn current(ui: &mut Ui, members: &[Value], steps: &[Value]) -> Option<Action> {
    if steps.is_empty() {
        return None;
    }
    let mut action = None;
    ui.add_space(10.);
    card(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        eyebrow(ui, "Field desk  /  This activity");
        ui.label(RichText::new("Current outside steps").size(19.).strong());
        for step in steps {
            ui.separator();
            let Some((id, _)) = address(step) else {
                unavailable(ui, step, "Current request unavailable");
                continue;
            };
            ui.push_id(id, |ui| {
                if ui
                    .link(s(step, "title"))
                    .on_hover_text(s(step, "reference"))
                    .clicked()
                {
                    action = Some(Action::OpenRequest(id.into()));
                }
                status(ui, members, step);
            });
        }
    });
    action
}

fn unavailable(ui: &mut Ui, step: &Value, label: &str) {
    ui.weak(label).on_hover_text(s(step, "request_id"));
    ui.small(s(step, "unavailable"));
}

pub(super) fn at_end(
    ui: &mut Ui,
    members: &[Value],
    closure: &Value,
    current: &[Value],
) -> Option<Action> {
    let Some(steps) = closure["outside_steps_at_closure"].as_array() else {
        ui.weak(if closure.get("outside_steps_at_closure").is_none() {
            "Outside steps were not recorded at this closure."
        } else {
            "The outside-step record for this closure is unreadable."
        });
        return None;
    };
    if steps.is_empty() {
        ui.small("No outside steps at closure.");
        return None;
    }
    let mut action = None;
    egui::CollapsingHeader::new(format!("Outside steps at closure · {}", steps.len()))
        .default_open(steps.len() == 1)
        .show(ui, |ui| {
            for step in steps {
                let (Some((id, edition)), Some(source)) = (address(step), step["source"].as_str())
                else {
                    unavailable(ui, step, "Outside step unavailable at closure");
                    continue;
                };
                ui.push_id(id, |ui| {
                    ui.strong(s(step, "title"));
                    status(ui, members, step);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .small_button(format!("Read edition {edition} at closure ↗"))
                            .on_hover_text(format!("{}\n{source}", s(step, "reference")))
                            .clicked()
                        {
                            action = Some(Action::Inspect(source.into()));
                        }
                        match step["file_count"].as_u64() {
                            Some(0) => (),
                            Some(n) => {
                                if ui.small_button("Save files at closure…").on_hover_text(format!("{n} attached to this edition. A later packet and a human’s return are not included.")).clicked() {
                                    action = Some(Action::SaveOutside { reference: s(step, "reference").into(), source: source.into(), title: s(step, "title").into() });
                                }
                            }
                            None => { ui.weak("File list unavailable"); }
                        }
                        if let Some((_, now)) = current
                            .iter()
                            .filter_map(address)
                            .find(|(other, _)| *other == id)
                        {
                            if ui
                                .link(format!("Current request · edition {now} →"))
                                .clicked()
                            {
                                action = Some(Action::OpenRequest(id.into()));
                            }
                        }
                    });
                });
            }
        });
    action
}
