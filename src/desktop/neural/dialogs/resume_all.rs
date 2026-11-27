use super::*;
use ai_personas::neural::{
    live::RunnerObservation, membership::ResumePlan, protocol::Continuation,
};

pub(in crate::desktop::neural) struct Form {
    work: String,
    title: String,
    plan: Result<ResumePlan, String>,
    reason: String,
}

impl Form {
    pub(super) fn open(app: &App) -> Self {
        Self {
            work: app.work.clone(),
            title: app.work_name(&app.work),
            plan: ResumePlan::inspect(&app.root, &app.work).map_err(|e| e.to_string()),
            reason: "Operator selected Resume all.".into(),
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, app: &mut App) -> Option<Command> {
        ui.label(RichText::new(excerpt(&self.title, 120)).strong())
            .on_hover_text(&self.title);
        if app.work != self.work {
            ui.label(
                "The selected activity changed. Close this form and review the intended activity.",
            );
            return None;
        }
        if ui.small_button("Refresh list").clicked() {
            self.plan = ResumePlan::inspect(&app.root, &self.work).map_err(|e| e.to_string());
            app.error = None;
        }
        let plan = match &self.plan {
            Ok(plan) => plan,
            Err(e) => {
                ui.colored_label(
                    theme::ACCENT,
                    "The participants could not be verified. Nothing was requested.",
                );
                ui.small(e);
                return None;
            }
        };
        if plan.members.is_empty() {
            ui.label("No waiting or stopped participants are available to resume.");
            return None;
        }
        let stopped = plan
            .members
            .iter()
            .filter(|m| matches!(m.continuation, Continuation::Stop { .. }))
            .count();
        ui.label(format!(
            "{} waiting · {stopped} stopped",
            plan.members.len() - stopped
        ));
        ui.small("Each person listed will be asked for one decision. They can continue, wait or stop again.");
        for member in &plan.members {
            let (state, reason) = match &member.continuation {
                Continuation::Wait { reason, .. } => ("Waiting", reason),
                Continuation::Stop { reason } => ("Stopped", reason),
                Continuation::Ready { .. } => unreachable!("ready participants are not resumable"),
            };
            ui.horizontal_wrapped(|ui| {
                let label = app.label(&member.persona);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::hover());
                design::persona_mark(ui, rect, label, persona_color(&member.persona));
                ui.strong(label)
                    .on_hover_text(member.participation.handle());
                ui.small(state).on_hover_text(reason);
            });
        }
        if plan.other_participants > 0 {
            ui.small(format!(
                "Not included: {} in this activity.",
                counted(
                    plan.other_participants as u64,
                    "other participant",
                    "other participants"
                )
            ));
        }
        ui.separator();
        ui.label("Reason for the requests");
        ui.add(egui::TextEdit::singleline(&mut self.reason).desired_width(f32::INFINITY));
        ui.small("Scheduling holds stay in effect. An active runner can start eligible calls immediately.");
        if !app.running && matches!(app.runner_observation, RunnerObservation::Unobserved) {
            ui.small(
                "No runner observed. Use Run activity after confirming to process the requests.",
            );
        }
        let count = plan.members.len();
        let confirm = format!("Resume {}", counted(count as u64, "persona", "personas"));
        ui.add_enabled(!self.reason.trim().is_empty(), theme::primary(&confirm))
            .clicked()
            .then(|| Command::ResumeAll {
                work: self.work.clone(),
                reason: self.reason.clone(),
                expected: plan.fingerprint.clone(),
            })
    }
}
