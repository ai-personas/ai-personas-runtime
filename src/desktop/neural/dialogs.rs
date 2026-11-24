use super::*;
use ai_personas::neural::input_files::{CapturedFile, InputFiles};
mod invitation;
mod connection;
mod resume;

#[derive(Default)]
pub(super) struct SourceFiles {
    picker: super::input_files::Picker,
    files: Vec<CapturedFile>,
}

pub(super) struct SourceEdition {
    reference: String,
    title: String,
    issuer: String,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Action {
    Message,
    Amend,
    Invite,
}

impl Action {
    fn title(self) -> &'static str {
        match self {
            Self::Message => "Write a message",
            Self::Amend => "Amend the brief",
            Self::Invite => "Invite a persona",
        }
    }
}

pub(super) enum Dialog {
    Resume(resume::Form),
    Agreement(agreements::Form),
    ActivityState { work: String, expected: String, reopen: bool, reason: String },
    Activity {
        social: bool,
        text: String,
        edition: Option<SourceEdition>,
        roster: invitation::Roster,
        sources: SourceFiles,
        retry: Option<(String, String)>,
    },
    Participation {
        work: String,
        title: String,
        action: Action,
        person: String,
        text: String,
        reply_to: Option<(String, String)>,
        sources: SourceFiles,
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
    Settings(connection::Form),
    Render(super::render::RenderDialog),
}
impl Dialog {
    pub(super) fn activity(social: bool) -> Self {
        Self::Activity {
            social,
            text: String::new(),
            edition: None,
            roster: Default::default(),
            sources: SourceFiles::default(),
            retry: None,
        }
    }
    pub(super) fn from_edition(reference: &str, title: &str, issuer: &str) -> Self {
        let mut dialog = Self::activity(false);
        if let Self::Activity { edition, .. } = &mut dialog {
            *edition = Some(SourceEdition { reference: reference.into(), title: title.into(), issuer: issuer.into() });
        }
        dialog
    }
    pub(super) fn persona(provider: &str) -> Self {
        Self::Persona {
            name: String::new(),
            seed: String::new(),
            provider: provider.into(),
        }
    }
    fn scroll_identity(&self) -> egui::Id {
        let id = egui::Id::new(std::mem::discriminant(self));
        match self {
            Self::Review { reference, .. } => id.with(reference),
            Self::Reasoning { persona, expected, .. } => id.with((persona, expected)),
            Self::Participation { work, person, action, reply_to, .. } =>
                id.with((work, person, action.title(), reply_to.as_ref().map(|r| &r.0))),
            Self::ActivityState { work, expected, reopen, .. } => id.with((work, expected, reopen)),
            Self::Activity { edition, .. } => id.with(edition.as_ref().map(|e| &e.reference)),
            Self::Agreement(form) => id.with(form.title()),
            _ => id,
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::ActivityState {reopen:true,..} => "Reopen activity",
            Self::ActivityState {..} => "End activity",
            Self::Activity { social: true, .. } => "New conversation",
            Self::Activity { edition: Some(_), .. } => "New brief from edition",
            Self::Activity { .. } => "New task",
            Self::Participation { action, .. } => action.title(),
            Self::Resume(_) => "Resume a participant",
            Self::Persona { .. } => "New persona",
            Self::Review { .. } => "Review delivery",
            Self::Reasoning { .. } => "Change reasoning settings",
            Self::Agreement(form) => form.title(),
            Self::Settings(_) => "Workspace settings",
            Self::Render(_) => "Render OpenSCAD project",
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

#[cfg(test)]
mod layout_tests;

fn dialog_contents(ui: &mut Ui, scroll: bool, identity: egui::Id, fresh: bool, show: impl FnOnce(&mut Ui)) {
    if scroll {
        let mut area = egui::ScrollArea::vertical().id_salt(identity);
        if fresh { area = area.vertical_scroll_offset(0.); }
        area.max_height((ui.ctx().content_rect().height() - 100.).max(80.))
            .auto_shrink([false, true])
            .show(ui, show);
    } else {
        show(ui);
    }
}

impl App {
    pub(super) fn open_workspace_settings(&mut self) {
        self.dialog = Some(Dialog::Settings(connection::Form::new(self)));
    }

    pub(super) fn resume_dialog(&self) -> Dialog {
        Dialog::Resume(resume::Form::new(self))
    }

    fn can_address(&self, work: &str, action: Action, id: &str) -> bool {
        let member = self.records("participation").iter().map(|r| &r["body"])
            .find(|p| p["work"] == work && p["persona"] == id);
        match action {
            Action::Invite => member.is_none_or(|p| p["declined"] == true || p["departure"].is_string() || p["closed_by_activity"].is_string()),
            Action::Message => member.is_some(),
            Action::Amend => false,
        }
    }

    pub(super) fn participation_dialog(&self, action: Action) -> Dialog {
        // Pick a starting address once. A live refresh may disable the draft,
        // but must not decide that its text or attachments are for somebody else.
        let person = self.records("personas").iter().map(|r| s(&r["body"], "id"))
            .filter(|id| self.can_address(&self.work, action, id))
            .min_by_key(|id| *id != self.persona).unwrap_or_default().to_owned();
        Dialog::Participation {
            work: self.work.clone(),
            title: self.task_title().into(),
            action,
            person,
            text: String::new(),
            reply_to: None,
            sources: SourceFiles::default(),
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
            ui.small("No operator review recorded");
        }
        if ui.small_button("Package findings…").clicked() {
            self.package_reviews.open(ui.ctx(),&self.root,&self.work,Some(reference));
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
            self.dialog_scroll_identity = None;
            return;
        };
        let scroll_identity = draft.scroll_identity();
        let fresh = self.dialog_scroll_identity != Some(scroll_identity);
        self.dialog_scroll_identity = Some(scroll_identity);
        let mut open = true;
        let mut command = None;
        let mut start_workspace = false;
        let invitation = matches!(draft, Dialog::Activity { .. });
        // Invitation owns its normal-height body scrolling; other forms must
        // remain scrollable even when a draft grows after the window opens.
        let scroll = !invitation || ctx.content_rect().height() < 370.;
        // Resize width excludes the frame, margins and vertical scrollbar.
        let width=(ctx.content_rect().width()-40.).max(1.);
        let title = if width < 360. {
            match &draft {
                Dialog::Settings(_) => "Settings",
                Dialog::Reasoning { .. } => "Reasoning",
                Dialog::Resume(_) => "Resume",
                Dialog::Review { .. } => "Review",
                Dialog::Agreement(_) => "Agreement",
                Dialog::Activity { social: true, .. } => "Conversation",
                Dialog::Participation { action: Action::Message, .. } => "Message",
                Dialog::Participation { action: Action::Amend, .. } => "Amend brief",
                Dialog::Participation { action: Action::Invite, .. } => "Invite",
                Dialog::Activity { edition: Some(_), .. } => "New brief",
                Dialog::Render(_) => "Render project",
                _ => draft.title(),
            }
        } else { draft.title() };
        // Persisted dimensions were measured at the previous zoom. Clamp every
        // dialog, not only settings, so its close control stays reachable.
        let mut window=egui::Window::new(title).min_width(1.).max_width(width);
        if invitation {
            let bounds = ctx.content_rect();
            let initial = egui::vec2(552.0_f32.min(width), 575.0_f32.min((bounds.height()-84.).max(64.)));
            window = window.default_pos(bounds.center()-initial*0.5).default_height(initial.y);
        }
        window
            .id(egui::Id::new(if invitation {"studio-invitation"} else {"studio-dialog"}))
            .open(&mut open)
            .default_width(510.0_f32.min(width))
            .resizable(true)
            .show(ctx, |ui| dialog_contents(ui, scroll, scroll_identity, fresh, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| match &mut draft {
                    Dialog::Activity {
                        social,
                        text,
                        roster,
                        sources,
                        edition,
                        ..
                    } => {
                        if let Some(start) = invitation::form(self, ui, *social, text, roster, sources, edition.as_ref()) {
                            start_workspace = start;
                            let invite = roster.selected.iter().cloned().collect();
                            command = Some(if *social {
                                Command::Social {
                                    topic: text.clone(),
                                    invite,
                                    files: InputFiles::from(sources.files.clone()),
                                }
                            } else {
                                Command::Request {
                                    need: text.clone(),
                                    from_edition: edition.as_ref().map(|source| source.reference.clone()),
                                    invite,
                                    files: InputFiles::from(sources.files.clone()),
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
                        ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                        ui.strong("Initial character");
                        multiline(
                            ui,
                            seed,
                            "Describe temperament, personality and tendencies…",
                        );
                        ui.strong("Use the provider settings of");
                        let has_provider = self.choose_person(ui, provider, |_| true);
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
                                    && has_provider,
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
                    Dialog::ActivityState { work, expected, reopen, reason } => {
                        ui.label(if *reopen {
                            "Reopening allows fresh invitations. Each participant chooses whether to return."
                        } else {
                            "Ending this activity closes participation and releases unfinished outcomes. The closure records remaining duties and outstanding effects."
                        });
                        ui.small(expected.as_str());
                        multiline(ui,reason,"Reason for this change…");
                        if ui.add_enabled(!reason.trim().is_empty(),theme::primary(if *reopen {"Reopen activity"} else {"End activity"})).clicked() {
                            command=Some(if *reopen {
                                Command::Reopen { work:work.clone(),reason:reason.clone(),expected:Some(expected.clone()) }
                            } else {
                                Command::Dissolve { work:work.clone(),reason:reason.clone(),expected:Some(expected.clone()) }
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
                        sources,
                    } => {
                        ui.strong(title.as_str());
                        let mut address_valid = true;
                        if let Some((_,quote))=reply_to.as_ref() {
                            ui.label(format!("Replying to {}",self.label(person)));
                            ui.add(egui::Label::new(egui::RichText::new(quote.as_str()).italics().weak()).wrap());
                        } else if *action != Action::Amend {
                            address_valid = self.choose_person(ui, person, |id| self.can_address(work, *action, id));
                        }
                        let (hint, label) = match action {
                            Action::Message => ("Write a message or attach a file…", "Send message"),
                            Action::Amend => (
                                "Describe the change to the original brief…",
                                "Save amendment",
                            ),
                            Action::Invite => (
                                "What would you like to invite them to consider?",
                                "Send invitation",
                            ),
                        };
                        multiline(ui, text, hint);
                        if matches!(action, Action::Message | Action::Amend) {
                            sources.picker.show(ui, &mut sources.files);
                        }
                        let enabled = !sources.picker.busy()
                            && (!text.trim().is_empty() || (*action == Action::Message && !sources.files.is_empty()))
                            && address_valid && (*action == Action::Amend || !person.is_empty());
                        if ui.add_enabled(enabled, theme::primary(label)).clicked() {
                            command = Some(match action {
                                Action::Message => Command::Message {
                                    work: work.clone(),
                                    persona: person.clone(),
                                    text: text.clone(),
                                    reply_to: reply_to.as_ref().map(|(reference,_)|reference.clone()),
                                    delivery: None,
                                    files: InputFiles::from(sources.files.clone()),
                                },
                                Action::Amend => Command::Amend {
                                    work: work.clone(),
                                    text: text.clone(),
                                    files: InputFiles::from(sources.files.clone()),
                                },
                                Action::Invite => Command::Invite {
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
                        ui.add(egui::Label::new(egui::RichText::new(title.as_str()).heading()).wrap());
                        ui.small(reference.as_str());
                        multiline(ui, reason, "Which parts meet the brief or need changes?");
                        ui.horizontal_wrapped(|ui| {
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
                        ui.add(egui::Label::new(egui::RichText::new(self.label(persona)).heading()).wrap());
                        ui.label("Model ID");
                        ui.add(egui::TextEdit::singleline(model).desired_width(f32::INFINITY));
                        ui.label("Reasoning effort");
                        ui.add(egui::TextEdit::singleline(effort).desired_width(f32::INFINITY));
                        multiline(ui, reason, "Why are you changing this connection setting?");
                        if ui.add_enabled(!model.trim().is_empty() && !effort.trim().is_empty() && !reason.trim().is_empty(),
                            theme::primary("Save reasoning settings")).clicked() {
                            command = Some(Command::Configure { persona: persona.clone(), model: Some(model.trim().into()),
                                effort: Some(effort.trim().into()), expected: Some(expected.clone()), reason: reason.clone() });
                        }
                    }
                    Dialog::Resume(form) => command = form.show(ui,self),
                    Dialog::Agreement(form) => command = form.show(ui,self),
                    Dialog::Settings(form) => command = self.workspace_settings(ui, ctx, form),
                    Dialog::Render(form) => command=form.show(ui,self),
                });
                if self.submitting_dialog {
                    ui.label(if matches!(draft,Dialog::Render(_)) {"Running compiler…"} else {"Saving…"});
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            }));
        if open {
            let request = if let Some(submission) = &command {
                if let Dialog::Agreement(form) = &mut draft {
                    match form.request_id() {
                        Ok(id) => Some(id),
                        Err(error) => { self.error=Some(error.to_string()); command=None; None }
                    }
                } else if let Dialog::Activity {retry, ..} = &mut draft {
                    // Same submission, same receipt. Editing the brief, roster or
                    // attachments after a failure creates a new intent.
                    match invitation::request_key(submission, retry) {
                        Ok(id)=>Some(id),
                        Err(e)=>{self.error=Some(e.to_string()); command=None; None}
                    }
                } else { None }
            } else { None };
            self.dialog = Some(draft);
            if let Some(command) = command {
                self.submitting_dialog = true;
                self.dispatch(ctx, command, request, start_workspace);
            }
        } else {
            self.dialog_scroll_identity = None;
            if matches!(draft, Dialog::Settings(_)) { self.save_layout(); }
        }
    }

    fn choose_person(&self, ui: &mut Ui, person: &mut String, include: impl Fn(&str) -> bool) -> bool {
        let choices: Vec<_> = self
            .records("personas")
            .iter()
            .map(|r| &r["body"])
            .filter(|p| include(s(p, "id")))
            .collect();
        egui::ComboBox::from_id_salt("dialog-person")
            .selected_text(if person.is_empty() {
                if choices.is_empty() { "No eligible personas" } else { "Choose a persona…" }
            } else {
                self.label(person)
            })
            .show_ui(ui, |ui| {
                for p in &choices {
                    ui.selectable_value(person, s(p, "id").into(), s(p, "label"));
                }
            });
        let eligible = choices.iter().any(|p| p["id"] == *person);
        if !eligible && !person.is_empty() {
            ui.colored_label(theme::ACCENT, "This person is no longer eligible. The draft has not been redirected.");
        }
        eligible
    }

    fn workspace_settings(&mut self, ui: &mut Ui, ctx: &egui::Context, connection: &mut connection::Form) -> Option<Command> {
        ui.strong("Operator name");
        ui.add(egui::TextEdit::singleline(&mut self.operator).desired_width(f32::INFINITY));
        ui.small("Used for new messages, invitations, reviews and settings changes in this window.");
        ui.separator();
        let command = connection.show(ui, self);
        ui.separator();
        ui.strong("Appearance");
        let dark = ui.horizontal(|ui| {
            let graphite = ui.selectable_value(&mut self.layout.dark, true, "Graphite").changed();
            let paper = ui.selectable_value(&mut self.layout.dark, false, "Paper").changed();
            graphite || paper
        }).inner;
        let zoom = ui
            .add(
                egui::Slider::new(&mut self.layout.scale, theme::MIN_ZOOM..=theme::MAX_ZOOM)
                    .text(if ui.available_width() < 360. { "Scale" } else { "Interface scale" })
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
        ui.horizontal_wrapped(|ui| {
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
        command
    }
}
