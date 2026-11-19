use super::{card, document, egui, s, theme, App, Icon, RichText, Ui, Value};
use std::collections::VecDeque;

const RECENT: usize = 48;
const VISIBLE: usize = 4;

#[derive(Clone, Copy)]
enum Contribution {
    Reply,
    Message,
    File(&'static str),
}

impl Contribution {
    fn from_event(event: &Value) -> Option<Self> {
        if event["invocation"].as_str()?.is_empty()
            || event["event"].as_i64()? <= 0
            || event["work"].as_str()?.is_empty()
            || event["actor"].as_str()?.is_empty()
        {
            return None;
        }
        let body = &event["body"];
        match event["kind"].as_str()? {
            "reply"
                if body["operational_persona"] == true
                    && !body["text"].as_str()?.trim().is_empty() =>
            {
                Some(Self::Reply)
            }
            "message" if !body["text"].as_str()?.trim().is_empty() => Some(Self::Message),
            "request_message_sent"
                if event["target"] == event["actor"]
                    && !body["to"].as_str()?.is_empty()
                    && !body["text"].as_str()?.trim().is_empty() =>
            {
                Some(Self::Message)
            }
            "tool_result" if event["target"] == event["actor"] && body["bootstrap"] != true => {
                let verb = match body["tool"].as_str()? {
                    "asset.save" => "saved a file",
                    "asset.register" => "registered a file",
                    "asset.import" => "imported a file",
                    _ => return None,
                };
                let outcome = &event["tool_outcome"];
                (outcome["problem"] == false
                    && outcome["file_reference"].as_str()?.starts_with("asset:")
                    && !outcome["file_path"].as_str()?.is_empty())
                .then_some(Self::File(verb))
            }
            _ => None,
        }
    }
}

#[derive(Default)]
pub(super) struct Updates {
    recent: VecDeque<(usize, Contribution)>,
    offset: usize,
}

impl Updates {
    pub(super) fn observe(&mut self, events: &[Value], first_index: usize) {
        for (index, event) in events.iter().enumerate() {
            if let Some(kind) = Contribution::from_event(event) {
                if self.recent.len() == RECENT {
                    self.recent.pop_back();
                }
                self.recent.push_front((first_index + index, kind));
                if self.offset > 0 {
                    self.offset = (self.offset + 1).min(self.recent.len() - 1);
                }
            }
        }
    }
}

/// Preserve brief authored line breaks while bounding the text sent to layout.
fn preview(text: &str, mut remaining: usize, mut line_breaks: usize) -> String {
    let mut out = String::new();
    let mut separator = None;
    for c in text.chars() {
        if c.is_whitespace() {
            if !out.is_empty() {
                if c == '\n' && line_breaks > 0 {
                    separator = Some('\n');
                } else {
                    separator.get_or_insert(' ');
                }
            }
            continue;
        }
        let needed = 1 + usize::from(separator.is_some());
        if remaining < needed {
            out.push('…');
            break;
        }
        if let Some(c) = separator.take() {
            out.push(c);
            if c == '\n' {
                line_breaks -= 1;
            }
        }
        out.push(c);
        remaining -= needed;
    }
    out
}

enum Open {
    Event(String, i64),
    File { work: String, reference: String },
}

impl App {
    pub(super) fn workspace_updates(&mut self, ui: &mut Ui) {
        let mut open = None;
        let p = theme::palette(ui);
        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Activity, p.muted);
                ui.strong("Recent contributions");
                let updates = &mut self.home_updates;
                if updates.recent.len() > VISIBLE {
                    if ui
                        .add_enabled(updates.offset > 0, egui::Button::new("Newer").small())
                        .clicked()
                    {
                        updates.offset = updates.offset.saturating_sub(VISIBLE);
                    }
                    let older = updates.offset + VISIBLE;
                    if ui
                        .add_enabled(
                            older < updates.recent.len(),
                            egui::Button::new("Older").small(),
                        )
                        .clicked()
                    {
                        updates.offset = older;
                    }
                }
            });
            if self.home_updates.recent.is_empty() {
                ui.weak("No persona replies, messages or saved files yet.");
                return;
            }
            for &(index, kind) in self
                .home_updates
                .recent
                .iter()
                .skip(self.home_updates.offset)
                .take(VISIBLE)
            {
                let event = &self.events[index];
                let work = s(event, "work");
                let seq = event["event"].as_i64().unwrap();
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.strong(preview(self.label(s(event, "actor")), 64, 0));
                    match kind {
                        Contribution::File(verb) => {
                            ui.label(verb);
                        }
                        Contribution::Reply | Contribution::Message => {
                            let to = if event["kind"] == "request_message_sent" {
                                s(&event["body"], "to")
                            } else {
                                s(event, "target")
                            };
                            let verb = if matches!(kind, Contribution::Reply) {
                                "replied"
                            } else {
                                "sent a message"
                            };
                            if to.is_empty() || to == "*" {
                                ui.label(verb);
                            } else {
                                let recipient = if to == "user" { "you" } else { self.label(to) };
                                ui.label(format!("{verb} to {}", preview(recipient, 64, 0)));
                            }
                        }
                    }
                });
                match kind {
                    Contribution::File(_) => {
                        let file = &event["tool_outcome"];
                        let path = s(file, "file_path");
                        if ui
                            .link(preview(path, 140, 0))
                            .on_hover_text("Open this file revision")
                            .clicked()
                        {
                            open = Some(Open::File {
                                work: work.to_owned(),
                                reference: s(file, "file_reference").into(),
                            });
                        }
                    }
                    Contribution::Reply | Contribution::Message => {
                        document::message_markdown(ui, &preview(s(&event["body"], "text"), 280, 2));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    let title = self
                        .home_index
                        .activity(work)
                        .map_or(work, |a| a.title.as_str());
                    ui.small(preview(title, 72, 0));
                    if let Some(utc) = event["created_utc"].as_str() {
                        ui.small(utc);
                    }
                    let label = match kind {
                        Contribution::Reply => "Open reply",
                        Contribution::Message => "Open message",
                        Contribution::File(_) => "View event",
                    };
                    if ui.link(RichText::new(label).small()).clicked() {
                        open = Some(Open::Event(work.to_owned(), seq));
                    }
                });
            }
        });
        match open {
            Some(Open::Event(work, seq)) => self.open_home_event(&work, seq),
            Some(Open::File { work, reference }) => {
                self.select_activity(&work);
                self.open_resource(ui.ctx(), reference);
            }
            None => {}
        }
    }
}
