use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Action {
    Message,
    Amend,
    Invite,
    Resume,
}

impl Action {
    fn title(self) -> &'static str {
        match self {
            Self::Message => "Write a message",
            Self::Amend => "Amend the brief",
            Self::Invite => "Invite a persona",
            Self::Resume => "Resume a participant",
        }
    }
}

pub(super) enum Dialog {
    Activity {
        social: bool,
        text: String,
        invitees: BTreeSet<String>,
    },
    Participation {
        work: String,
        title: String,
        action: Action,
        person: String,
        text: String,
        reply_to: Option<(String, String)>,
    },
    Persona {
        name: String,
        seed: String,
        provider: String,
    },
    Review {
        reference: String,
        title: String,
        reason: String,
    },
    Reasoning {
        persona: String,
        expected: String,
        model: String,
        effort: String,
        reason: String,
    },
    Settings,
}
impl Dialog {
    pub(super) fn activity(social: bool) -> Self {
        Self::Activity {
            social,
            text: String::new(),
            invitees: BTreeSet::new(),
        }
    }
    pub(super) fn persona(provider: &str) -> Self {
        Self::Persona {
            name: String::new(),
            seed: String::new(),
            provider: provider.into(),
        }
    }
    fn title(&self) -> &str {
        match self {
            Self::Activity { social: true, .. } => "New conversation",
            Self::Activity { .. } => "New task",
            Self::Participation { action, .. } => action.title(),
            Self::Persona { .. } => "New persona",
            Self::Review { .. } => "Review delivery",
            Self::Reasoning { .. } => "Change reasoning settings",
            Self::Settings => "Workspace settings",
        }
    }
}

fn multiline(ui: &mut Ui, value: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::multiline(value)
            .hint_text(hint)
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
}

impl App {
    pub(super) fn participation_dialog(&self, action: Action) -> Dialog {
        Dialog::Participation {
            work: self.work.clone(),
            title: self.task_title().into(),
            action,
            person: self.persona.clone(),
            text: String::new(),
            reply_to: None,
        }
    }

    pub(super) fn delivery_review(&mut self, ui: &mut Ui, delivery: &Value) {
        let r = &delivery["reference"];
        if let (Some(id), Some(revision)) = (r["id"].as_str(), r["revision"].as_i64()) {
            self.review_controls(
                ui,
                &format!("delivery:{id}@{revision}"),
                s(&delivery["body"], "title"),
            );
        }
    }
    pub(super) fn review_controls(&mut self, ui: &mut Ui, reference: &str, title: &str) {
        let basis = &self.snapshot.status["delivery_mandates"][reference];
        if basis["same_revision"] == false {
            ui.small("The brief changed after this delivery.")
                .on_hover_text(format!("Delivered under {}\nCurrent brief {}",s(basis,"delivered_under"),s(basis,"current_brief")));
        }
        let review = &self.snapshot.status["delivery_reviews"][reference];
        if let Some(accepted) = review["accepted"].as_bool() {
            ui.small(format!(
                "{} · {}",
                if accepted {
                    "Accepted"
                } else {
                    "Changes requested"
                },
                s(review, "reviewer")
            ));
            ui.label(s(review, "reason"));
        } else {
            ui.small("Awaiting review");
        }
        if ui.small_button("Review delivery…").clicked() {
            self.dialog = Some(Dialog::Review {
                reference: reference.into(),
                title: title.into(),
                reason: String::new(),
            });
        }
    }

    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.dialog.take() else {
            return;
        };
        let mut open = true;
        let mut command = None;
        egui::Window::new(draft.title())
            .id(egui::Id::new("studio-dialog"))
            .open(&mut open)
            .default_width(510.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| match &mut draft {
                    Dialog::Activity {
                        social,
                        text,
                        invitees,
                    } => {
                        ui.strong(if *social {
                            "Opening topic (optional)"
                        } else {
                            "What would you like to work on?"
                        });
                        multiline(
                            ui,
                            text,
                            if *social {
                                "Leave open for the participants to choose…"
                            } else {
                                "Describe the result you want…"
                            },
                        );
                        ui.add_space(8.);
                        ui.strong("Invite personas");
                        for p in self.records("personas") {
                            let id = s(&p["body"], "id");
                            let mut selected = invitees.contains(id);
                            if ui.checkbox(&mut selected, s(&p["body"], "label")).changed() {
                                if selected {
                                    invitees.insert(id.into());
                                } else {
                                    invitees.remove(id);
                                }
                            }
                        }
                        let enabled = !invitees.is_empty() && (*social || !text.trim().is_empty());
                        if ui
                            .add_enabled(
                                enabled,
                                theme::primary(if *social {
                                    "Open conversation"
                                } else {
                                    "Create task"
                                }),
                            )
                            .clicked()
                        {
                            let invite = invitees.iter().cloned().collect();
                            command = Some(if *social {
                                Command::Social {
                                    topic: text.clone(),
                                    invite,
                                }
                            } else {
                                Command::Request {
                                    need: text.clone(),
                                    invite,
                                }
                            });
                        }
                    }
                    Dialog::Persona {
                        name,
                        seed,
                        provider,
                    } => {
                        ui.strong("Display name");
                        ui.text_edit_singleline(name);
                        ui.strong("Initial character");
                        multiline(
                            ui,
                            seed,
                            "Describe temperament, personality and tendencies…",
                        );
                        ui.strong("Use the provider settings of");
                        self.choose_person(ui, provider, |_| true);
                        if let Some(p) = self
                            .records("personas")
                            .iter()
                            .find(|p| p["body"]["id"] == *provider)
                        {
                            ui.small(format!(
                                "{} · {}",
                                s(&p["body"]["provider"], "model"),
                                s(&p["body"]["provider"], "effort")
                            ));
                        }
                        ui.add_space(8.);
                        if ui
                            .add_enabled(
                                !name.trim().is_empty()
                                    && !seed.trim().is_empty()
                                    && !provider.is_empty(),
                                theme::primary("Create persona"),
                            )
                            .clicked()
                        {
                            command = Some(Command::AddPersona {
                                id: None,
                                label: name.clone(),
                                seed: seed.clone(),
                                provider_from: provider.clone(),
                            });
                        }
                    }
                    Dialog::Participation {
                        work,
                        title,
                        action,
                        person,
                        text,
                        reply_to,
                    } => {
                        ui.strong(title.as_str());
                        if let Some((_,quote))=reply_to.as_ref() {
                            ui.label(format!("Replying to {}",self.label(person)));
                            ui.add(egui::Label::new(egui::RichText::new(quote.as_str()).italics().weak()).wrap());
                        } else if *action != Action::Amend {
                            self.choose_person(ui, person, |id| {
                                let member = self.records("participation").iter().find(|r| {
                                    r["body"]["work"] == *work && r["body"]["persona"] == id
                                });
                                match action {
                                    Action::Invite => member.is_none(),
                                    Action::Resume => member.is_some_and(|r| {
                                        r["body"]["declined"] != true
                                            && matches!(
                                                r["body"]["continuation"]["kind"].as_str(),
                                                Some("wait" | "stop")
                                            )
                                    }),
                                    _ => member.is_some(),
                                }
                            });
                        }
                        let (hint, label) = match action {
                            Action::Message => ("Message or feedback…", "Send message"),
                            Action::Amend => (
                                "Describe the change to the original brief…",
                                "Save amendment",
                            ),
                            Action::Invite => (
                                "What would you like to invite them to consider?",
                                "Send invitation",
                            ),
                            Action::Resume => (
                                "Why should this participation resume?",
                                "Resume participant",
                            ),
                        };
                        multiline(ui, text, hint);
                        let enabled = !text.trim().is_empty()
                            && (*action == Action::Amend || !person.is_empty());
                        if ui.add_enabled(enabled, theme::primary(label)).clicked() {
                            command = Some(match action {
                                Action::Message => Command::Message {
                                    work: work.clone(),
                                    persona: person.clone(),
                                    text: text.clone(),
                                    reply_to: reply_to.as_ref().map(|(reference,_)|reference.clone()),
                                },
                                Action::Amend => Command::Amend {
                                    work: work.clone(),
                                    text: text.clone(),
                                },
                                Action::Invite => Command::Invite {
                                    work: work.clone(),
                                    persona: person.clone(),
                                    reason: text.clone(),
                                },
                                Action::Resume => Command::Resume {
                                    work: work.clone(),
                                    persona: person.clone(),
                                    reason: text.clone(),
                                },
                            });
                        }
                    }
                    Dialog::Review {
                        reference,
                        title,
                        reason,
                    } => {
                        ui.heading(title.as_str());
                        ui.small(reference.as_str());
                        multiline(ui, reason, "Which parts meet the brief or need changes?");
                        ui.horizontal(|ui| {
                            for (reject, label) in
                                [(false, "Accept delivery"), (true, "Request changes")]
                            {
                                if ui
                                    .add_enabled(
                                        !reason.trim().is_empty(),
                                        egui::Button::new(label),
                                    )
                                    .clicked()
                                {
                                    command = Some(Command::Accept {
                                        delivery: reference.clone(),
                                        reject,
                                        reason: reason.clone(),
                                    });
                                }
                            }
                        });
                    }
                    Dialog::Reasoning { persona, expected, model, effort, reason } => {
                        ui.heading(self.label(persona));
                        ui.label("Model ID");
                        ui.text_edit_singleline(model);
                        ui.label("Reasoning effort");
                        ui.text_edit_singleline(effort);
                        multiline(ui, reason, "Why are you changing this connection setting?");
                        if ui.add_enabled(!model.trim().is_empty() && !effort.trim().is_empty() && !reason.trim().is_empty(),
                            theme::primary("Save reasoning settings")).clicked() {
                            command = Some(Command::Configure { persona: persona.clone(), model: Some(model.trim().into()),
                                effort: Some(effort.trim().into()), expected: Some(expected.clone()), reason: reason.clone() });
                        }
                    }
                    Dialog::Settings => self.workspace_settings(ui, ctx),
                });
                if self.submitting_dialog {
                    ui.label("Saving…");
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        if open {
            self.dialog = Some(draft);
            if let Some(command) = command {
                self.submitting_dialog = true;
                self.command(ctx, command);
            }
        } else if matches!(draft, Dialog::Settings) {
            self.save_layout();
        }
    }

    fn choose_person(&self, ui: &mut Ui, person: &mut String, include: impl Fn(&str) -> bool) {
        let choices: Vec<_> = self
            .records("personas")
            .iter()
            .map(|r| &r["body"])
            .filter(|p| include(s(p, "id")))
            .collect();
        if !choices.iter().any(|p| p["id"] == *person) {
            *person = choices
                .first()
                .map(|p| s(p, "id").to_owned())
                .unwrap_or_default();
        }
        egui::ComboBox::from_id_salt("dialog-person")
            .selected_text(if person.is_empty() {
                "No eligible personas"
            } else {
                self.label(person)
            })
            .show_ui(ui, |ui| {
                for p in choices {
                    ui.selectable_value(person, s(p, "id").into(), s(p, "label"));
                }
            });
    }

    fn workspace_settings(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.strong("Operator name");
        ui.text_edit_singleline(&mut self.operator);
        ui.small("Used for new messages, invitations, reviews and settings changes in this window.");
        ui.separator();
        ui.strong("Appearance");
        let dark = ui
            .checkbox(&mut self.layout.dark, "Dark workspace")
            .changed();
        let zoom = ui
            .add(
                egui::Slider::new(&mut self.layout.scale, theme::MIN_ZOOM..=theme::MAX_ZOOM)
                    .text("Interface scale")
                    .step_by(0.05),
            )
            .changed();
        if dark || zoom {
            theme::apply(ctx, self.layout.dark, self.layout.scale);
            self.save_layout();
        }
        ui.separator();
        ui.strong("Panels");
        ui.checkbox(&mut self.layout.outliner, "Outliner");
        ui.checkbox(&mut self.layout.inspector, "Inspector");
        ui.checkbox(&mut self.layout.activity, "Activity dock");
        ui.horizontal(|ui| {
            if ui.button("Reset layout").clicked() {
                self.layout = Layout::default();
                theme::apply(ctx, self.layout.dark, self.layout.scale);
                self.save_layout();
            }
            if ui.button("Save layout").clicked() {
                self.save_layout();
                self.notice = "Workspace layout saved".into();
            }
        });
        ui.separator();
        ui.strong("Workspace location");
        ui.label(self.root.display().to_string());
        if ui.button("Copy location").clicked() {
            ctx.copy_text(self.root.display().to_string());
        }
        if ui.add_enabled(!self.busy, egui::Button::new("Back up workspace…")).clicked() {
            if let Some(archive) = rfd::FileDialog::new().set_title("Save workspace archive")
                .set_file_name("personas-workspace.sqlite").save_file() {
                self.command(ctx, Command::Backup { archive });
            }
        }
    }
}
