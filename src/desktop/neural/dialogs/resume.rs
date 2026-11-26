//! Inspect the chosen wait or stop separately from sending a message, requesting
//! a decision, or starting a listener. A link never falls back to another member.
use super::*;
use ai_personas::neural::live::RunnerObservation;

pub(in crate::desktop::neural) enum RecipientState {
    Review(Choice),
    Note(&'static str),
}

impl RecipientState {
    pub(in crate::desktop::neural) fn read(record: &Value, open: bool) -> Option<Self> {
        let body = &record["body"];
        if body["closed_by_activity"].is_string() {
            Some(Self::Note("Activity ended"))
        } else if body["departure"].is_string() {
            Some(Self::Note("Left this activity"))
        } else if body["declined"] == true {
            Some(Self::Note("Invitation declined"))
        } else if body["pending_resume"].is_object() {
            Some(Self::Note("Resume requested"))
        } else if open {
            Choice::read(record).map(Self::Review)
        } else {
            None
        }
    }

    pub(in crate::desktop::neural) fn show(&self, ui: &mut Ui, available: bool) -> Option<Choice> {
        match self {
            Self::Review(choice) if choice.link(ui, available) => Some(choice.clone()),
            Self::Note(status) => {
                ui.weak(RichText::new(*status).small());
                None
            }
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq)]
pub(in crate::desktop::neural) struct Choice {
    work: String,
    person: String,
    participation: String,
    stop: bool,
}

impl Choice {
    fn read(record: &Value) -> Option<Self> {
        let part = &record["body"];
        if part["declined"] != false
            || !part["departure"].is_null()
            || !part["closed_by_activity"].is_null()
            || !part["pending_resume"].is_null()
        {
            return None;
        }
        let stop = match part["continuation"]["kind"].as_str()? {
            "stop" => true,
            "wait" => false,
            _ => return None,
        };
        let work = part["work"].as_str()?;
        let person = part["persona"].as_str()?;
        let r = &record["reference"];
        if r["kind"] != "participation"
            || r["id"] != format!("{work}/{person}")
            || r["revision"].as_u64()? == 0
        {
            return None;
        }
        Some(Self {
            work: work.into(),
            person: person.into(),
            participation: inbox::handle(record),
            stop,
        })
    }

    fn current<'a>(&self, app: &'a App) -> Option<&'a Value> {
        if !app
            .records("work")
            .iter()
            .any(|r| r["body"]["id"] == self.work && r["body"]["phase"] == "open")
        {
            return None;
        }
        app.records("participation").iter().find(|r| {
            r["body"]["work"] == self.work
                && r["body"]["persona"] == self.person
                && Self::read(r).as_ref() == Some(self)
        })
    }

    fn link(&self, ui: &mut Ui, available: bool) -> bool {
        ui.small(if self.stop {
            "Stopped · a message won’t resume work."
        } else {
            "Last choice: wait"
        });
        ui.add_enabled(
            available,
            egui::Button::new(if self.stop {
                "Review stop…"
            } else {
                "Review wait…"
            })
            .small(),
        )
        .on_hover_text("Read this participant’s choice. Nothing is sent or started by opening it.")
        .on_disabled_hover_text("Finish the current action or close its form first.")
        .clicked()
    }
}

struct Target {
    choice: Choice,
    label: String,
    reason: String,
    wake: Vec<String>,
    time: Option<i64>,
    draft: String,
}

impl Target {
    fn new(app: &App, record: &Value, choice: Choice) -> Self {
        let continuation = &record["body"]["continuation"];
        Self {
            label: app.label(&choice.person).into(),
            choice,
            reason: s(continuation, "reason").into(),
            wake: continuation["on_events"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .chain(live_board::peer_wakes(continuation))
                .collect(),
            time: continuation["wake_at_unix"].as_i64(),
            draft: String::new(),
        }
    }
}

pub(in crate::desktop::neural) struct Form {
    title: String,
    targets: Vec<Target>,
    selected: usize,
}

impl Form {
    pub(super) fn new(app: &App) -> Self {
        let targets: Vec<_> = app
            .records("participation")
            .iter()
            .filter(|r| r["body"]["work"] == app.work)
            .filter_map(|r| Some(Target::new(app, r, Choice::read(r)?)))
            .collect();
        let selected = targets
            .iter()
            .position(|t| t.choice.person == app.persona)
            .unwrap_or(0);
        Self {
            title: app.task_title().into(),
            targets,
            selected,
        }
    }

    pub(super) fn review(app: &App, choice: Choice) -> Option<Self> {
        let record = choice.current(app)?;
        Some(Self {
            title: app.work_name(&choice.work),
            targets: vec![Target::new(app, record, choice)],
            selected: 0,
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui, app: &App) -> Option<Command> {
        ui.add(egui::Label::new(RichText::new(&self.title).small().weak()).truncate())
            .on_hover_text(&self.title);
        if self.targets.is_empty() {
            ui.label("No participant has a wait or stop available to resume.");
            return None;
        }
        if self.targets.len() > 1 {
            egui::ComboBox::from_id_salt("resume-target")
                .selected_text(&self.targets[self.selected].label)
                .show_ui(ui, |ui| {
                    for (i, target) in self.targets.iter().enumerate() {
                        ui.selectable_value(&mut self.selected, i, &target.label);
                    }
                });
        } else {
            ui.label(RichText::new(&self.targets[0].label).strong().size(19.));
        }
        let target = &mut self.targets[self.selected];
        let choice = &target.choice;
        ui.label(if choice.stop {
            "Last choice: stop"
        } else {
            "Last choice: wait"
        });
        ui.label(&target.reason);
        if !choice.stop {
            let triggers = if target.wake.is_empty() {
                "any new arrival".into()
            } else {
                target
                    .wake
                    .iter()
                    .map(|s| {
                        if s.contains(':') {
                            s.clone()
                        } else {
                            s.replace('_', " ")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui.small(format!("Wake on: {triggers}"));
            if let Some(at) = target.time {
                ui.small(format!("Or at: {} (Unix time {at})", time_label(at)));
            }
        }
        ui.small(&choice.participation);
        let current = choice.current(app).is_some();
        let deciding = app.snapshot.active.iter().any(|call| {
            call["actor"] == choice.person
                && call["work"] == choice.work
                && matches!(s(call, "state"), "prepared" | "dispatched" | "responded")
        });
        if !current {
            ui.colored_label(theme::ACCENT, "This choice is no longer current or available. Close and inspect the current participation.");
        } else if deciding {
            ui.weak("A decision is already in flight. Wait for its result.");
        }
        ui.separator();
        ui.strong("Request another decision");
        ui.small("The participant chooses what follows. This request does not send a message, change the brief or start a runner.");
        if !app.running && matches!(app.runner_observation, RunnerObservation::Unobserved) {
            ui.small("No runner observed. Use Run activity separately when you want eligible work to run.");
        }
        multiline(
            ui,
            &mut target.draft,
            "Your reason for requesting another decision…",
        );
        ui.add_enabled(
            current && !deciding && !target.draft.trim().is_empty(),
            theme::primary("Request decision"),
        )
        .clicked()
        .then(|| Command::Resume {
            work: choice.work.clone(),
            persona: choice.person.clone(),
            reason: target.draft.clone(),
            expected: choice.participation.clone(),
        })
    }
}
