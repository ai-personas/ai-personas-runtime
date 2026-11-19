//! Human messages use the same attributed, idempotent write as the operator CLI.
use super::{excerpt, operator_call, s, theme, App, Command, Icon, Update};
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Composer {
    activities: BTreeMap<String, Activity>,
}

#[derive(Default)]
struct Activity {
    recipient: String,
    drafts: BTreeMap<String, Draft>,
}

#[derive(Default)]
struct Draft {
    input: Message,
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
}

impl Message {
    fn parent(&self) -> Option<&str> {
        self.reply.as_ref().map(|(reference, _)| reference.as_str())
    }

    fn same_content(&self, other: &Self) -> bool {
        self.text == other.text && self.parent() == other.parent()
    }
}

#[derive(Clone)]
struct Attempt {
    request: String,
    operator: String,
    message: Message,
}

pub(super) struct SubmissionResult {
    work: String,
    person: String,
    request: String,
    event: Result<i64, String>,
}

struct Recipient {
    id: String,
    name: String,
    status: Option<&'static str>,
}

impl Composer {
    fn address(&mut self, work: &str, person: &str) -> Option<&mut Draft> {
        if work.is_empty() || person.is_empty() {
            return None;
        }
        let activity = self.activities.entry(work.to_owned()).or_default();
        person.clone_into(&mut activity.recipient);
        let draft = activity.drafts.entry(person.to_owned()).or_default();
        draft.focus = true;
        Some(draft)
    }

    pub(super) fn receive(&mut self, result: SubmissionResult) -> bool {
        let Some(draft) = self
            .activities
            .get_mut(&result.work)
            .and_then(|activity| activity.drafts.get_mut(&result.person))
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
                if draft.input.text.is_empty() && draft.input.reply.is_none() {
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
        } else if !self.failed.is_empty() {
            Some("send unconfirmed")
        } else if !self.input.text.is_empty() || self.input.reply.is_some() {
            Some("draft")
        } else {
            None
        }
    }

    fn prepare(&mut self, operator: &str, retry: Option<usize>) -> Option<Attempt> {
        if self.pending.is_some() || operator.is_empty() {
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
            }
            sent
        } else {
            if self.input.text.trim().is_empty() {
                return None;
            }
            let request = match ai_personas::neural::id("studio-message") {
                Ok(request) => request,
                Err(reason) => {
                    self.issue = Some(format!("Could not prepare the message. {reason}"));
                    return None;
                }
            };
            Attempt {
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
        person: &Recipient,
        operator: &str,
    ) -> Option<Attempt> {
        let p = theme::palette(ui);
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
        let id = ui.make_persistent_id(("operator-message", work, &person.id));
        if std::mem::take(&mut self.focus) {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
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
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.input.text)
                                        .id(id)
                                        .hint_text(format!("Write to {}…", person.name))
                                        .margin(egui::Margin::symmetric(9, 6))
                                        .desired_rows(2)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                    },
                );
                let enabled = !operator.is_empty()
                    && self.pending.is_none()
                    && !self.input.text.trim().is_empty();
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
                                ui.label(
                                    RichText::new(excerpt(&sent.message.text, 54))
                                        .small()
                                        .italics(),
                                )
                                .on_hover_text(&sent.message.text);
                                if ui
                                    .add_enabled(
                                        self.pending.is_none() && operator == sent.operator,
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
            return self.prepare(operator, retry);
        }
        None
    }
}

impl App {
    pub(super) fn address_in_live(&mut self, person: &str) {
        self.live_composer.address(&self.work, person);
    }

    pub(super) fn reply_in_live(&mut self, person: &str, reference: &str, quote: &str) {
        if let Some(draft) = self.live_composer.address(&self.work, person) {
            draft.input.reply = Some((reference.to_owned(), excerpt(quote, 180)));
        }
    }

    pub(super) fn live_composer(&mut self, ui: &mut Ui) {
        if self.work.is_empty() {
            return;
        }
        let members: Vec<_> = self
            .records("participation")
            .iter()
            .map(|row| &row["body"])
            .filter(|body| body["work"] == self.work)
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
                .any(|row| row["body"]["id"] == self.work && row["body"]["phase"] != "dissolved");
        let activity = self
            .live_composer
            .activities
            .entry(self.work.clone())
            .or_default();
        if activity.recipient.is_empty() {
            if let Some(person) = members
                .iter()
                .find(|person| person.id == self.persona)
                .or(members.first())
            {
                activity.recipient.clone_from(&person.id);
            }
        }
        let p = theme::palette(ui);
        let operator = self.operator.trim();
        let mut receipt = None;
        let mut invite = false;
        let sent = egui::Frame::new()
            .fill(p.panel)
            .stroke(egui::Stroke::new(1., p.border))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let width = ui.available_width();
                ui.horizontal_wrapped(|ui| {
                    if width >= 245. {
                        theme::glyph(ui, Icon::Conversation, p.accent);
                    }
                    if members.is_empty() || width >= 330. {
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
                        .find(|person| person.id == activity.recipient);
                    if members.len() == 1 && selected.is_some() {
                        ui.add(
                            egui::Label::new(RichText::new(&members[0].name).color(p.text))
                                .truncate(),
                        )
                        .on_hover_text(&members[0].id);
                    } else {
                        egui::ComboBox::from_id_salt(("message-to", &self.work))
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
                                    let label = activity
                                        .drafts
                                        .get(&person.id)
                                        .and_then(Draft::status)
                                        .map(|status| format!("{} · {status}", person.name));
                                    if ui
                                        .selectable_value(
                                            &mut activity.recipient,
                                            person.id.clone(),
                                            label.as_deref().unwrap_or(&person.name),
                                        )
                                        .on_hover_text(&person.id)
                                        .clicked()
                                    {
                                        activity
                                            .drafts
                                            .entry(person.id.clone())
                                            .or_default()
                                            .focus = true;
                                    }
                                }
                            });
                    }
                    let selected = members
                        .iter()
                        .find(|person| person.id == activity.recipient);
                    if let Some(status) = selected.and_then(|person| person.status) {
                        ui.label(RichText::new(status).small().color(p.muted))
                            .on_hover_text("You can still record a message for this participant.");
                    }
                    if let Some(draft) = activity.drafts.get(&activity.recipient) {
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
                    .find(|person| person.id == activity.recipient)?;
                if operator.is_empty() {
                    ui.weak("Set your operator name in Workspace settings to send.");
                }
                let draft = activity.drafts.entry(person.id.clone()).or_default();
                draft
                    .editor(ui, &self.work, person, operator)
                    .map(|attempt| (person.id.clone(), attempt))
            })
            .inner;
        if let Some(event) = receipt {
            self.open_message_receipt(ui.ctx(), event);
        }
        if invite {
            self.dialog = Some(self.participation_dialog(super::dialogs::Action::Invite));
        }
        let Some((person, attempt)) = sent else {
            return;
        };
        let (work, root, tx, ctx) = (
            self.work.clone(),
            self.root.clone(),
            self.tx.clone(),
            ui.ctx().clone(),
        );
        std::thread::spawn(move || {
            let command = Command::Message {
                work: work.clone(),
                persona: person.clone(),
                reply_to: attempt.message.parent().map(str::to_owned),
                text: attempt.message.text,
            };
            let event = operator_call(&root, &attempt.operator, command, Some(&attempt.request)).and_then(|receipt| {
                receipt["event"].as_i64().filter(|event| *event > 0)
                    .ok_or_else(|| "The workspace returned no message event. Retry to check this submission.".to_owned())
            });
            let _ = tx.send(Update::LiveMessage(SubmissionResult {
                work,
                person,
                request: attempt.request,
                event,
            }));
            ctx.request_repaint();
        });
    }
}
