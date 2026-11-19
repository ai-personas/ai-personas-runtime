//! Human messages use the same attributed, idempotent write as the operator CLI.
use super::{excerpt, input_files::Picker, operator_call, s, theme, App, Command, Icon, Update};
use ai_personas::neural::input_files::{CapturedFile, InputFiles};
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Composer {
    activities: BTreeMap<String, Activity>,
}

#[derive(Default)]
struct Activity {
    selected: Destination,
    drafts: BTreeMap<Destination, Draft>,
}

#[derive(Clone, Default, Eq, PartialEq, Ord, PartialOrd)]
struct Destination {
    person: String,
    delivery: Option<String>,
}

impl Destination {
    fn conversation(person: &str) -> Self {
        Self {
            person: person.into(),
            delivery: None,
        }
    }
}

impl Activity {
    fn select(&mut self, destination: Destination) -> &mut Draft {
        self.selected.clone_from(&destination);
        let draft = self.drafts.entry(destination).or_default();
        draft.focus = true;
        draft
    }

    fn feedback_drafts(&mut self, ui: &mut Ui) {
        let person = &self.selected.person;
        let mut drafts = self
            .drafts
            .iter()
            .filter(|(to, _)| to.person == *person && to.delivery.is_some())
            .filter_map(|(to, draft)| {
                let status = draft
                    .status()
                    .or_else(|| draft.saved_event.map(|_| "recorded"))?;
                Some((to, draft, status))
            })
            .peekable();
        if drafts.peek().is_none() {
            return;
        }
        let mut selected = None;
        ui.menu_button("Package feedback", |ui| {
            ui.set_max_width((ui.ctx().content_rect().width().min(340.) - 30.).max(1.));
            egui::ScrollArea::vertical()
                .max_height(220.)
                .show(ui, |ui| {
                    for (to, draft, status) in drafts {
                        let reference = to.delivery.as_deref().unwrap_or_default();
                        let label = format!(
                            "{} · {status}",
                            package_title(&draft.delivery_title, reference)
                        );
                        if ui.button(label).on_hover_text(reference).clicked() {
                            selected = Some(to.clone());
                            ui.close();
                        }
                    }
                });
        });
        if let Some(to) = selected {
            self.select(to);
        }
    }
}

fn package_title(title: &str, reference: &str) -> String {
    let title = if title.trim().is_empty() {
        "Issued package"
    } else {
        title
    };
    let revision = reference
        .rsplit_once('@')
        .and_then(|(_, r)| r.parse::<u64>().ok());
    match revision {
        Some(revision) => format!("{title} · revision {revision}"),
        None => title.into(),
    }
}

pub(super) fn feedback_link(
    ui: &mut Ui,
    title: &str,
    reference: &str,
    max_rows: usize,
) -> egui::Response {
    let title = package_title(title, reference);
    let mut label = egui::text::LayoutJob::simple_format(
        format!("Feedback on {}", excerpt(&title, 180)),
        egui::TextFormat {
            font_id: egui::TextStyle::Body.resolve(ui.style()),
            color: ui.visuals().hyperlink_color,
            ..Default::default()
        },
    );
    label.wrap.max_rows = max_rows;
    ui.scope(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.link(label).on_hover_text(format!("{title}\n{reference}"))
    })
    .inner
}

#[derive(Default)]
struct Draft {
    delivery_title: String,
    input: Message,
    picker: Picker,
    focus: bool,
    pending: Option<Attempt>,
    failed: Vec<(Attempt, String)>,
    issue: Option<String>,
    saved_event: Option<i64>,
}

#[derive(Clone, Default)]
struct Message {
    text: String,
    reply: Option<(String, String)>,
    files: Vec<CapturedFile>,
}

impl Message {
    fn parent(&self) -> Option<&str> {
        self.reply.as_ref().map(|(reference, _)| reference.as_str())
    }

    fn same_content(&self, other: &Self) -> bool {
        self.text == other.text
            && self.parent() == other.parent()
            && self.files.len() == other.files.len()
            && self
                .files
                .iter()
                .zip(&other.files)
                .all(|(a, b)| a.name() == b.name() && a.sha256() == b.sha256())
    }

    fn can_send(&self) -> bool {
        !self.text.trim().is_empty() || !self.files.is_empty()
    }
}

#[derive(Clone)]
struct Attempt {
    work: String,
    destination: Destination,
    request: String,
    operator: String,
    message: Message,
}

pub(super) struct SubmissionResult {
    work: String,
    destination: Destination,
    request: String,
    event: Result<i64, String>,
}

struct Recipient {
    id: String,
    name: String,
    status: Option<&'static str>,
}

impl Composer {
    fn address(&mut self, work: &str, destination: Destination) -> Option<&mut Draft> {
        if work.is_empty() || destination.person.is_empty() {
            return None;
        }
        let activity = self.activities.entry(work.to_owned()).or_default();
        Some(activity.select(destination))
    }

    pub(super) fn receive(&mut self, result: SubmissionResult) -> bool {
        let Some(draft) = self
            .activities
            .get_mut(&result.work)
            .and_then(|activity| activity.drafts.get_mut(&result.destination))
        else {
            return false;
        };
        let Some(sent) = draft.pending.take_if(|sent| sent.request == result.request) else {
            return false;
        };
        match result.event {
            Ok(event) => {
                draft.saved_event = Some(event);
                true
            }
            Err(reason) => {
                // A new message may already be in the editor. Keep both in that case.
                if draft.input.text.is_empty()
                    && draft.input.reply.is_none()
                    && draft.input.files.is_empty()
                    && !draft.picker.busy()
                {
                    draft.input = sent.message.clone();
                }
                draft.failed.push((sent, reason));
                false
            }
        }
    }
}

impl Draft {
    fn status(&self) -> Option<&'static str> {
        if self.pending.is_some() {
            Some("saving")
        } else if self.picker.busy() {
            Some("adding files")
        } else if !self.failed.is_empty() {
            Some("send unconfirmed")
        } else if !self.input.text.is_empty()
            || self.input.reply.is_some()
            || !self.input.files.is_empty()
        {
            Some("draft")
        } else {
            None
        }
    }

    fn prepare(
        &mut self,
        work: &str,
        destination: &Destination,
        operator: &str,
        retry: Option<usize>,
    ) -> Option<Attempt> {
        if self.pending.is_some() || operator.is_empty() || self.picker.busy() {
            return None;
        }
        let retry = retry.or_else(|| {
            self.failed.iter().position(|(sent, _)| {
                sent.operator == operator && sent.message.same_content(&self.input)
            })
        });
        let sent = if let Some(index) = retry {
            if self.failed.get(index)?.0.operator != operator {
                return None;
            }
            let (sent, _) = self.failed.remove(index);
            if sent.message.same_content(&self.input) {
                self.input = Message::default();
                self.picker = Picker::default();
            }
            sent
        } else {
            if !self.input.can_send() {
                return None;
            }
            let request = match ai_personas::neural::id("studio-message") {
                Ok(request) => request,
                Err(reason) => {
                    self.issue = Some(format!("Could not prepare the message. {reason}"));
                    return None;
                }
            };
            self.picker = Picker::default();
            Attempt {
                work: work.to_owned(),
                destination: destination.clone(),
                request,
                operator: operator.to_owned(),
                message: std::mem::take(&mut self.input),
            }
        };
        self.issue = None;
        self.saved_event = None;
        self.focus = true;
        self.pending = Some(sent.clone());
        Some(sent)
    }

    fn editor(
        &mut self,
        ui: &mut Ui,
        work: &str,
        destination: &Destination,
        person: &Recipient,
        operator: &str,
    ) -> Option<Attempt> {
        let p = theme::palette(ui);
        ui.push_id(
            ("message-files", work, &person.id, &destination.delivery),
            |ui| {
                self.picker.details(ui, &mut self.input.files);
            },
        );
        if let Some((reference, quote)) = &self.input.reply {
            let clear = ui
                .allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 22.),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let clear = ui.small_button("Clear reply").clicked();
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width().max(0.), 22.),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("Reply · {quote}"))
                                            .small()
                                            .color(p.accent),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(format!("{quote}\n{reference}"));
                            },
                        );
                        clear
                    },
                )
                .inner;
            if clear {
                self.input.reply = None;
                self.focus = true;
            }
        }
        let id =
            ui.make_persistent_id(("operator-message", work, &person.id, &destination.delivery));
        let focus = std::mem::take(&mut self.focus);
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Enter);
        let enter = ui.memory(|memory| memory.has_focus(id))
            && ui.input_mut(|input| input.consume_shortcut(&shortcut));
        let stacked = ui.available_width() < 270.;
        let width = if stacked {
            ui.available_width()
        } else {
            ui.available_width() - 74. - ui.spacing().item_spacing.x
        };
        let layout = if stacked {
            egui::Layout::top_down(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        let send = ui
            .allocate_ui_with_layout(egui::vec2(ui.available_width(), 0.), layout, |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(width.max(1.), 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(id)
                            .max_height(66.)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                let editor = ui.add(
                                    egui::TextEdit::multiline(&mut self.input.text)
                                        .id(id)
                                        .hint_text(if !self.input.files.is_empty() {
                                            "Add a message (optional)…".into()
                                        } else if destination.delivery.is_some() {
                                            format!("Write feedback to {}…", person.name)
                                        } else {
                                            format!("Write to {}…", person.name)
                                        })
                                        .margin(egui::Margin::symmetric(9, 6))
                                        .desired_rows(2)
                                        .desired_width(f32::INFINITY),
                                );
                                if focus {
                                    editor.request_focus();
                                    // A menu or Conversation button can still own this click.
                                    // Apply the request once more after its popup has closed.
                                    if ui.input(|input| input.pointer.any_click()) {
                                        self.focus = true;
                                        ui.ctx().request_repaint();
                                    }
                                }
                            });
                    },
                );
                let enabled = !operator.is_empty()
                    && self.pending.is_none()
                    && !self.picker.busy()
                    && self.input.can_send();
                let retry = self.failed.iter().any(|(sent, _)| {
                    sent.operator == operator && sent.message.same_content(&self.input)
                });
                let button = ui
                    .add_enabled(
                        enabled,
                        theme::primary(if retry { "Retry" } else { "Send" })
                            .min_size(egui::vec2(74., 32.)),
                    )
                    .on_hover_text(format!(
                        "Send to {} ({})",
                        person.name,
                        ui.ctx().format_shortcut(&shortcut)
                    ));
                enabled && (button.clicked() || enter)
            })
            .inner;
        if let Some(error) = &self.issue {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let mut retry = None;
        let mut dismiss = None;
        if !self.failed.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt((id, "unsent"))
                .max_height(80.)
                .show(ui, |ui| {
                    for (index, (sent, reason)) in self.failed.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Send unconfirmed")
                                    .small()
                                    .color(ui.visuals().error_fg_color),
                            );
                            ui.label(RichText::new(reason).small());
                            if !sent.message.same_content(&self.input) || sent.operator != operator
                            {
                                if !sent.message.text.is_empty() {
                                    ui.label(
                                        RichText::new(excerpt(&sent.message.text, 54))
                                            .small()
                                            .italics(),
                                    )
                                    .on_hover_text(&sent.message.text);
                                }
                                if let Some(first) = sent.message.files.first() {
                                    let label = match sent.message.files.len() {
                                        1 => first.name().to_owned(),
                                        count => format!("{} + {}", first.name(), count - 1),
                                    };
                                    ui.add(
                                        egui::Label::new(RichText::new(label).small()).truncate(),
                                    )
                                    .on_hover_ui(|ui| {
                                        for file in &sent.message.files {
                                            ui.label(format!(
                                                "{} · {}",
                                                file.name(),
                                                super::size(file.len() as u64)
                                            ));
                                        }
                                    });
                                }
                                if ui
                                    .add_enabled(
                                        self.pending.is_none()
                                            && !self.picker.busy()
                                            && operator == sent.operator,
                                        egui::Button::new("Retry").small(),
                                    )
                                    .clicked()
                                {
                                    retry = Some(index);
                                }
                                if ui.small_button("Dismiss").clicked() {
                                    dismiss = Some(index);
                                }
                            }
                        });
                    }
                });
        }
        if let Some(index) = dismiss {
            self.failed.remove(index);
            return None;
        }
        if send || retry.is_some() {
            return self.prepare(work, destination, operator, retry);
        }
        None
    }
}

impl App {
    pub(super) fn address_in_live(&mut self, person: &str) {
        self.live_composer
            .address(&self.work, Destination::conversation(person));
    }

    pub(super) fn address_on_overview(&mut self, work: String, person: &str) {
        self.live_composer
            .address(&work, Destination::conversation(person));
        self.home.message_work = Some(work);
    }

    pub(super) fn overview_composer(&mut self, ui: &mut Ui) {
        let Some(work) = self.home.message_work.clone() else {
            return;
        };
        let available = ui.available_height();
        let title = self.work_name(&work);
        let mut close = false;
        let mut open = false;
        egui::TopBottomPanel::bottom("overview-message-composer")
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                top: 8,
                ..Default::default()
            }))
            .default_height(145.)
            .resizable(false)
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui
                            .small_button("Close message")
                            .on_hover_text("Hide the composer and keep this draft")
                            .clicked();
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width().max(1.), 23.),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                theme::glyph(ui, Icon::Conversation, theme::palette(ui).muted);
                                open = ui
                                    .link(&title)
                                    .on_hover_text(format!("Open this conversation\n{title}"))
                                    .clicked();
                            },
                        );
                    });
                });
                // Measure the fields independently of last frame's dock height.
                // A newly attached file or a failed send may add another row.
                ui.scope(|ui| {
                    ui.set_max_height((available * 0.48).max(40.));
                    egui::ScrollArea::vertical()
                        .id_salt(("overview-message-fields", &work))
                        .auto_shrink([false, true])
                        .show(ui, |ui| self.compose_for_work(ui, &work));
                });
            });
        ui.set_clip_rect(ui.clip_rect().intersect(ui.available_rect_before_wrap()));
        if close {
            self.home.message_work = None;
        }
        if open {
            self.open_home_activity(&work);
            if let Some(activity) = self.live_composer.activities.get(&work) {
                self.persona.clone_from(&activity.selected.person);
            }
        }
    }

    pub(super) fn open_delivery_feedback(
        &mut self,
        reference: &str,
        work: &str,
        owner: &str,
        title: &str,
    ) {
        if reference.is_empty() || work.is_empty() || owner.is_empty() {
            return;
        }
        self.open_home_activity(work);
        owner.clone_into(&mut self.persona);
        let to = Destination {
            person: owner.into(),
            delivery: Some(reference.into()),
        };
        if let Some(draft) = self.live_composer.address(work, to) {
            title.clone_into(&mut draft.delivery_title);
        }
    }

    pub(super) fn reply_in_live(&mut self, person: &str, reference: &str, quote: &str) {
        let to = self
            .live_composer
            .activities
            .get(&self.work)
            .filter(|activity| activity.selected.person == person)
            .map_or_else(
                || Destination::conversation(person),
                |activity| activity.selected.clone(),
            );
        if let Some(draft) = self.live_composer.address(&self.work, to) {
            draft.input.reply = Some((reference.to_owned(), excerpt(quote, 180)));
        }
    }

    pub(super) fn live_composer(&mut self, ui: &mut Ui) {
        self.compose_for_work(ui, &self.work.clone());
    }

    fn compose_for_work(&mut self, ui: &mut Ui, work: &str) {
        if work.is_empty() {
            return;
        }
        let members: Vec<_> = self
            .records("participation")
            .iter()
            .map(|row| &row["body"])
            .filter(|body| body["work"] == work)
            .filter_map(|body| {
                let id = body["persona"].as_str()?;
                let status = if body["closed_by_activity"].is_string() {
                    Some("Activity ended")
                } else if body["departure"].is_string() {
                    Some("Left this activity")
                } else if body["declined"] == true {
                    Some("Invitation declined")
                } else if matches!(s(&body["continuation"], "kind"), "stop" | "stopped") {
                    Some("Stopped on this activity")
                } else {
                    None
                };
                Some(Recipient {
                    id: id.to_owned(),
                    name: self.label(id).to_owned(),
                    status,
                })
            })
            .collect();
        let can_invite = members.is_empty()
            && !self.records("personas").is_empty()
            && self
                .records("work")
                .iter()
                .any(|row| row["body"]["id"] == work && row["body"]["phase"] != "dissolved");
        let activity = self
            .live_composer
            .activities
            .entry(work.to_owned())
            .or_default();
        for draft in activity.drafts.values_mut() {
            draft.picker.poll(&mut draft.input.files);
        }
        if activity.selected.person.is_empty() {
            if let Some(person) = members
                .iter()
                .find(|person| person.id == self.persona)
                .or(members.first())
            {
                activity.selected.person.clone_from(&person.id);
            }
        }
        let p = theme::palette(ui);
        let operator = self.operator.trim();
        let mut receipt = None;
        let mut open_package = None;
        let mut invite = false;
        let sent = egui::Frame::new()
            .fill(p.panel)
            .stroke(egui::Stroke::new(1., p.border))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let width = ui.available_width();
                if let Some(reference) = &activity.selected.delivery {
                    let title = activity.drafts.get(&activity.selected)
                        .map_or("", |draft| draft.delivery_title.as_str());
                    let mut conversation = false;
                    ui.allocate_ui_with_layout(egui::vec2(width, 22.), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        conversation = ui.small_button("Conversation")
                            .on_hover_text("Return to the regular conversation and keep this feedback draft.")
                            .clicked();
                        ui.allocate_ui_with_layout(egui::vec2(ui.available_width().max(1.), 22.), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    if feedback_link(ui, title, reference, 2).clicked() {
                                open_package = Some(reference.clone());
                            }
                        });
                    });
                    if conversation {
                        activity.select(Destination::conversation(&activity.selected.person));
                    }
                }
                let bound = activity.selected.delivery.is_some();
                ui.horizontal_wrapped(|ui| {
                    if width >= 245. {
                        theme::glyph(ui, Icon::Conversation, p.accent);
                    }
                    if bound {
                        ui.strong("To");
                    } else if members.is_empty() || width >= 330. {
                        ui.strong("Your message");
                        if members.is_empty() {
                            return;
                        }
                        ui.add_space(4.);
                        ui.label(RichText::new("To").small().color(p.muted));
                    } else {
                        ui.strong(if width >= 177. { "Message to" } else { "To" });
                    }
                    let selected = members
                        .iter()
                        .find(|person| person.id == activity.selected.person);
                    if let Some(person) = selected.filter(|_| bound || members.len() == 1) {
                        ui.add(
                            egui::Label::new(RichText::new(&person.name).color(p.text))
                                .truncate(),
                        )
                        .on_hover_text(&person.id);
                    } else if bound {
                        ui.weak("Issuer unavailable");
                    } else {
                        egui::ComboBox::from_id_salt(("message-to", work))
                            .width(75.)
                            .truncate()
                            .selected_text(
                                selected
                                    .map_or("Choose a recipient", |person| person.name.as_str()),
                            )
                            .show_ui(ui, |ui| {
                                ui.set_max_width(width.min(320.));
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                for person in &members {
                                    let to = Destination::conversation(&person.id);
                                    let label = activity
                                        .drafts
                                        .get(&to)
                                        .and_then(Draft::status)
                                        .map(|status| format!("{} · {status}", person.name));
                                    if ui
                                        .selectable_label(
                                            activity.selected == to,
                                            label.as_deref().unwrap_or(&person.name),
                                        )
                                        .on_hover_text(&person.id)
                                        .clicked()
                                    {
                                        activity.select(to);
                                        ui.close();
                                    }
                                }
                            });
                    }
                    let selected = members
                        .iter()
                        .find(|person| person.id == activity.selected.person);
                    if let Some(person) = selected {
                        let draft = activity.drafts.entry(activity.selected.clone()).or_default();
                        ui.push_id(("attach-to", work, &person.id, &activity.selected.delivery), |ui| {
                            draft.picker.button(ui, draft.input.files.len())
                        });
                    }
                    if !bound {
                        activity.feedback_drafts(ui);
                    }
                    if let Some(status) = selected.and_then(|person| person.status) {
                        ui.label(RichText::new(status).small().color(p.muted))
                            .on_hover_text(if status == "Stopped on this activity" {
                                "Recording a message leaves this participant stopped."
                            } else {
                                "You can still record a message for this participant."
                            });
                    }
                    if let Some(draft) = activity.drafts.get(&activity.selected) {
                        if draft.pending.is_some() {
                            ui.add(egui::Spinner::new().size(13.));
                            ui.small("Saving…");
                        } else if let Some(event) = draft.saved_event {
                            if ui
                                .link(RichText::new(format!("Saved · event {event}")).small())
                                .on_hover_text("Open the message receipt")
                                .clicked()
                            {
                                receipt = Some(event);
                            }
                        }
                    }
                });
                if members.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak("This activity has no participants.");
                        if can_invite {
                            invite = ui
                                .add_enabled(
                                    !self.busy,
                                    egui::Button::new("Invite a persona…").small(),
                                )
                                .clicked();
                        }
                    });
                    return None;
                }
                let person = members
                    .iter()
                    .find(|person| person.id == activity.selected.person)?;
                if operator.is_empty() {
                    ui.weak("Set your operator name in Workspace settings to send.");
                }
                let draft = activity.drafts.entry(activity.selected.clone()).or_default();
                draft.editor(ui, work, &activity.selected, person, operator)
            })
            .inner;
        if let Some(reference) = open_package {
            self.open_home_delivery(&reference, work);
        }
        if let Some(event) = receipt {
            self.open_message_receipt(ui.ctx(), event);
        }
        if invite {
            self.select_activity(work);
            self.dialog = Some(self.participation_dialog(super::dialogs::Action::Invite));
        }
        let Some(attempt) = sent else {
            return;
        };
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ui.ctx().clone());
        std::thread::spawn(move || {
            let work = attempt.work;
            let destination = attempt.destination;
            let command = Command::Message {
                work: work.clone(),
                persona: destination.person.clone(),
                delivery: destination.delivery.clone(),
                reply_to: attempt.message.parent().map(str::to_owned),
                text: attempt.message.text,
                files: InputFiles::from(attempt.message.files),
            };
            let event = operator_call(&root, &attempt.operator, command, Some(&attempt.request)).and_then(|receipt| {
                if receipt["delivery"].as_str() != destination.delivery.as_deref() {
                    return Err("The workspace did not confirm this package reference. Retry to check this submission.".into());
                }
                receipt["event"].as_i64().filter(|event| *event > 0)
                    .ok_or_else(|| "The workspace returned no message event. Retry to check this submission.".to_owned())
            });
            let _ = tx.send(Update::LiveMessage(SubmissionResult {
                work,
                destination,
                request: attempt.request,
                event,
            }));
            ctx.request_repaint();
        });
    }
}
