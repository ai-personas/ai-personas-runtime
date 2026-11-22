//! A draft is bound to the participation the operator inspected, not to
//! whichever person happens to be waiting when the next refresh arrives.
use super::*;

struct Target {
    person: String,
    label: String,
    participation: String,
    choice: String,
    reason: String,
    draft: String,
}

pub(in crate::desktop::neural) struct Form {
    work: String,
    title: String,
    targets: Vec<Target>,
    selected: usize,
}

impl Form {
    pub(super) fn new(app: &App) -> Self {
        let targets: Vec<_> = app
            .records("participation")
            .iter()
            .filter_map(|record| {
                let part = &record["body"];
                let choice = s(&part["continuation"], "kind");
                if part["work"] != app.work
                    || !matches!(choice, "stop" | "wait")
                    || part["declined"] == true
                    || part["departure"].is_string()
                    || part["closed_by_activity"].is_string()
                    || !part["pending_resume"].is_null()
                {
                    return None;
                }
                let person = part["persona"].as_str()?;
                Some(Target {
                    person: person.to_owned(),
                    label: app.label(person).into(),
                    participation: inbox::handle(record),
                    choice: choice.into(),
                    reason: s(&part["continuation"], "reason").into(),
                    draft: String::new(),
                })
            })
            .collect();
        let selected = targets
            .iter()
            .position(|t| t.person == app.persona)
            .unwrap_or(0);
        Self {
            work: app.work.clone(),
            title: app.task_title().into(),
            targets,
            selected,
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, app: &App) -> Option<Command> {
        ui.strong(&self.title);
        if self.targets.is_empty() {
            ui.label("No participant has a wait or stop available to resume.");
            return None;
        }
        egui::ComboBox::from_id_salt("resume-target")
            .selected_text(&self.targets[self.selected].label)
            .show_ui(ui, |ui| {
                for (i, target) in self.targets.iter().enumerate() {
                    ui.selectable_value(&mut self.selected, i, &target.label);
                }
            });
        let target = &mut self.targets[self.selected];
        ui.label(format!("Last choice: {}", target.choice));
        ui.label(&target.reason);
        ui.small(&target.participation);
        let current = app
            .records("participation")
            .iter()
            .any(|r| inbox::handle(r) == target.participation);
        let deciding = app.snapshot.active.iter().any(|call| {
            call["actor"] == target.person
                && call["work"] == self.work
                && matches!(s(call, "state"), "prepared" | "dispatched" | "responded")
        });
        if !current {
            ui.colored_label(
                theme::ACCENT,
                "This participation changed. Close and reopen to inspect the new choice.",
            );
        } else if deciding {
            ui.weak("A decision is already in flight. Wait for its result.");
        }
        multiline(ui, &mut target.draft, "Why request another decision?");
        ui.small(
            "The participant chooses what follows. The brief and run controls stay unchanged.",
        );
        ui.add_enabled(
            current && !deciding && !target.draft.trim().is_empty(),
            theme::primary("Resume participant"),
        )
        .clicked()
        .then(|| Command::Resume {
            work: self.work.clone(),
            persona: target.person.clone(),
            reason: target.draft.clone(),
            expected: target.participation.clone(),
        })
    }
}
