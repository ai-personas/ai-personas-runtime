use super::{card, counted, document, egui, s, theme, App, Icon, RichText, Ui, Value};
use std::collections::BTreeMap;

const VISIBLE: usize = 4;

#[derive(Clone, Copy)]
enum Contribution {
    Reply,
    Message,
    File(FileChange),
    Print,
    Issue,
    ReceivingCheck,
    Feedback,
    Review,
    Request,
    Withdrawal,
    Return,
}

#[derive(Clone, Copy)]
enum FileChange {
    Save,
    Register,
    Import,
}

impl FileChange {
    fn verb(self) -> &'static str {
        match self {
            Self::Save => "saved a file",
            Self::Register => "registered a file",
            Self::Import => "imported a file",
        }
    }
}

fn edition<'a>(reference: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let (id, revision) = reference.strip_prefix(prefix)?.rsplit_once('@')?;
    (!id.is_empty() && revision.parse::<u64>().ok()? > 0).then_some((id, revision))
}

impl Contribution {
    fn from_event(event: &Value) -> Option<Self> {
        if event["event"].as_i64()? <= 0
            || s(event, "work").is_empty()
            || s(event, "actor").is_empty()
        {
            return None;
        }
        let body = &event["body"];
        let kind = s(event, "kind");
        let target = s(event, "target");
        let has_text = !s(body, "text").trim().is_empty();
        if event["invocation"].is_null() {
            return match kind {
                "human_request_progress" | "user_message"
                    if !target.is_empty() && target != "*" =>
                {
                    if !has_text
                        && !body["attachments"]
                            .as_array()
                            .is_some_and(|files| !files.is_empty())
                    {
                        return None;
                    }
                    if !body["human_request"].is_null() {
                        edition(s(body, "human_request"), "human_request:")?;
                        Some(Self::Return)
                    } else if kind == "user_message" && !body["delivery"].is_null() {
                        edition(s(body, "delivery"), "delivery:")?;
                        Some(Self::Feedback)
                    } else {
                        (kind == "user_message").then_some(Self::Message)
                    }
                }
                "user_acceptance"
                    if body["decision"]["user"] == event["actor"]
                        && body["decision"]["accepted"].is_boolean() =>
                {
                    edition(s(&body["decision"], "delivery"), "delivery:")?;
                    Some(Self::Review)
                }
                _ => None,
            };
        }
        if event["invocation"].as_str()?.is_empty() {
            return None;
        }
        match kind {
            "delivery_check" if body["receipt"].is_string() => {
                edition(s(body, "delivery"), "delivery:")?;
                Some(Self::ReceivingCheck)
            }
            "reply" if body["operational_persona"] == true && has_text => Some(Self::Reply),
            "message" if has_text => Some(Self::Message),
            "request_message_sent"
                if event["target"] == event["actor"] && !s(body, "to").is_empty() && has_text =>
            {
                Some(Self::Message)
            }
            // An issue also broadcasts a summary. Only its addressed event carries the issued edition.
            "delivery"
                if !target.is_empty()
                    && target != "*"
                    && body["record"]["owner"] == event["actor"]
                    && body["record"]["work"] == event["work"] =>
            {
                edition(s(body, "reference"), "delivery:")?;
                Some(Self::Issue)
            }
            "human_request" | "human_request_packet" | "human_request_withdrawn" if target == "user" => {
                edition(s(body, "reference"), "human_request:")?;
                Some(if kind != "human_request_withdrawn" {
                    Self::Request
                } else {
                    Self::Withdrawal
                })
            }
            "tool_result"
                if event["target"] == event["actor"]
                    && body["bootstrap"] != true
                    && event["tool_outcome"]["problem"] == false =>
            {
                let outcome = &event["tool_outcome"];
                let change = match s(body, "tool") {
                    "asset.save" => FileChange::Save,
                    "asset.register" => FileChange::Register,
                    "asset.import" => FileChange::Import,
                    "document.print_html" if s(outcome, "pdf").starts_with("object:") => {
                        return Some(Self::Print)
                    }
                    _ => return None,
                };
                edition(s(outcome, "file_reference"), "asset:")?;
                (!s(outcome, "file_path").is_empty()).then_some(Self::File(change))
            }
            _ => None,
        }
    }
}

// An operator can use the same identifier as a persona. Keep the actor's recorded role in the key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Author {
    Persona(String),
    Human(String),
}

impl Author {
    fn label(&self, app: &App) -> String {
        match self {
            Self::Persona(id) => preview(app.label(id), 64, 0),
            Self::Human(id) => format!("Human · {}", preview(id, 64, 0)),
        }
    }
}

#[derive(Clone, Copy)]
struct Row {
    index: usize,
    kind: Contribution,
}

// Both boundaries count forward from the first retained contribution for the selected author.
#[derive(Clone, Copy)]
struct Page {
    end: usize,
    paused_at: usize,
}

struct Anchor {
    first: usize,
    top_in_content: f32,
    viewport: egui::Vec2,
    frame: u64,
    generation: u64,
}

#[derive(Default)]
pub(super) struct Updates {
    all: Vec<Row>,
    authors: BTreeMap<Author, Vec<Row>>,
    selected: Option<Author>,
    page: Option<Page>,
    anchor: Option<Anchor>,
}

impl Updates {
    pub(super) fn observe(&mut self, events: &[Value], first_index: usize) {
        for (offset, event) in events.iter().enumerate() {
            let Some(kind) = Contribution::from_event(event) else {
                continue;
            };
            let row = Row {
                index: first_index + offset,
                kind,
            };
            let id = s(event, "actor").to_owned();
            let author = if event["invocation"].is_null() {
                Author::Human(id)
            } else {
                Author::Persona(id)
            };
            self.all.push(row);
            self.authors.entry(author).or_default().push(row);
        }
    }

    fn rows(&self) -> &[Row] {
        match &self.selected {
            Some(author) => self.authors.get(author).map_or(&[], Vec::as_slice),
            None => &self.all,
        }
    }

    // The live-person cards above this feed can also change height on a snapshot.
    // Use content coordinates so a reader's own scrolling is never undone.
    fn keep_reading_position(
        &mut self,
        ui: &Ui,
        rect: egui::Rect,
        first: usize,
        top: f32,
        generation: u64,
    ) {
        let previous = self.anchor.take();
        if self.page.is_none() {
            return;
        }
        let Some(scroller) = ui
            .stack()
            .iter()
            .find(|node| node.kind() == Some(egui::UiKind::ScrollArea))
        else {
            return;
        };
        let anchor = Anchor {
            first,
            top_in_content: top - scroller.max_rect.top(),
            viewport: ui.clip_rect().size(),
            frame: ui.ctx().cumulative_frame_nr(),
            generation,
        };
        if let Some(old) = previous.filter(|old| {
            old.first == first
                && old.generation != generation
                && old.frame.abs_diff(anchor.frame) <= 1
                && old.viewport == anchor.viewport
        }) {
            let delta = old.top_in_content - anchor.top_in_content;
            if delta.abs() > 0.5 {
                ui.scroll_with_delta_animation(
                    egui::vec2(0., delta),
                    egui::style::ScrollAnimation::none(),
                );
            }
        }
        if ui.clip_rect().intersects(rect) {
            self.anchor = Some(anchor);
        }
    }

    fn paging(&mut self, ui: &mut Ui) {
        let len = self.rows().len();
        let page = self.page.unwrap_or(Page {
            end: len,
            paused_at: len,
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(page) = self.page {
                let new = len - page.paused_at;
                let latest = if new == 0 {
                    "Latest".into()
                } else {
                    format!("Latest · {new} new")
                };
                if ui.small_button(latest).clicked() {
                    self.page = None;
                }
                if page.end < page.paused_at && ui.small_button("Newer").clicked() {
                    self.page = Some(Page {
                        end: (page.end + VISIBLE).min(page.paused_at),
                        ..page
                    });
                }
            } else if ui
                .small_button("Pause feed")
                .on_hover_text("Keep these updates in view as new activity arrives")
                .clicked()
            {
                self.page = Some(page);
            }
            if page.paused_at <= VISIBLE {
                return;
            }
            if ui
                .add_enabled(page.end > VISIBLE, egui::Button::new("Older").small())
                .clicked()
            {
                self.page = Some(Page {
                    end: page.end - VISIBLE,
                    ..page
                });
            }
            let first = page.paused_at - page.end + 1;
            ui.small(format!(
                "{first}–{} of {}",
                (first + VISIBLE - 1).min(page.paused_at),
                page.paused_at
            ));
        });
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
    Resource(String, String),
    Package(String),
    Request(String),
}

impl App {
    pub(super) fn workspace_updates(&mut self, ui: &mut Ui) {
        let mut open = None;
        let drawn = card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Activity, theme::palette(ui).accent);
                ui.label(RichText::new("Workspace activity").size(19.).strong());
                if self.home_updates.authors.len() > 1 {
                    self.feed_authors(ui);
                }
            });
            if self.home_updates.rows().is_empty() {
                ui.weak("No messages, files, requests or packages yet.");
                return None;
            }
            self.home_updates.paging(ui);
            let updates = &self.home_updates;
            let rows = updates.rows();
            let end = updates.page.map_or(rows.len(), |page| page.end);
            let top = ui.next_widget_position().y;
            let visible = &rows[end.saturating_sub(VISIBLE)..end];
            for &row in visible.iter().rev() {
                ui.separator();
                if let Some(action) = ui.push_id(row.index, |ui| self.feed_row(ui, row)).inner {
                    open = Some(action);
                }
            }
            visible.last().map(|row| (row.index, top))
        });
        if let Some((first, top)) = drawn.inner {
            self.home_updates.keep_reading_position(
                ui,
                drawn.response.rect,
                first,
                top,
                self.home_index.generation,
            );
        }
        match open {
            Some(Open::Event(work, sequence)) => self.open_home_event(&work, sequence),
            Some(Open::Resource(work, reference)) => {
                self.select_activity(&work);
                self.open_resource(ui.ctx(), reference);
            }
            Some(Open::Request(id)) => self.show_human_request(&id),
            Some(Open::Package(reference)) => self.open_issued_package(ui.ctx(), &reference),
            None => {}
        }
    }

    fn feed_authors(&mut self, ui: &mut Ui) {
        let updates = &self.home_updates;
        let mut selected = updates.selected.clone();
        egui::ComboBox::from_id_salt("workspace-activity-author")
            .selected_text(
                selected
                    .as_ref()
                    .map_or_else(|| "All authors".into(), |a| a.label(self)),
            )
            .width(132.)
            .height(340.)
            .truncate()
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, None, "All authors");
                for author in updates.authors.keys() {
                    let mut name = author.label(self);
                    if let Author::Persona(id) = author {
                        if !self.label(id).eq_ignore_ascii_case(id) {
                            name = format!("{name} · {id}");
                        }
                    }
                    if ui
                        .selectable_label(selected.as_ref() == Some(author), &name)
                        .clicked()
                    {
                        selected = Some(author.clone());
                    }
                }
            })
            .response
            .on_hover_text("Filter workspace activity by its recorded author");
        if selected != updates.selected {
            self.home_updates.selected = selected;
            self.home_updates.page = None;
        }
    }

    fn feed_row(&self, ui: &mut Ui, row: Row) -> Option<Open> {
        use Contribution::*;
        let event = &self.events[row.index];
        let body = &event["body"];
        let file = &event["tool_outcome"];
        let actor = s(event, "actor");
        let work = s(event, "work");
        let sequence = event["event"].as_i64().unwrap();
        let human = event["invocation"].is_null();
        let actor_name = if human { actor } else { self.label(actor) };
        let ico = match row.kind {
            File(_) | Print | Issue | Review | Feedback => Icon::Files,
            ReceivingCheck => Icon::Activity,
            Request | Return | Withdrawal => Icon::Questions,
            Reply | Message => Icon::Conversation,
        };
        let verb = match row.kind {
            File(change) => change.verb().into(),
            Print => "printed a PDF".into(),
            ReceivingCheck => "checked an issued package".into(),
            Issue if event["target"] == "user" => "issued a package".into(),
            Issue => format!(
                "issued a package for {}",
                preview(self.label(s(event, "target")), 64, 0)
            ),
            Return => {
                let verb = match (s(event, "kind"), s(body, "state")) {
                    ("human_request_progress", "in_progress") => "took a request from",
                    ("human_request_progress", "open") => "released a request from",
                    (_, "unable") => "reported a difficulty to",
                    (_, "completed") => "reported completion to",
                    _ => "sent a request update to",
                };
                format!("{verb} {}", preview(self.label(s(event, "target")), 64, 0))
            }
            _ => self.event_heading(event),
        };
        let p = theme::palette(ui);
        let accent = if human {
            p.accent
        } else {
            p.ink(self.color(actor))
        };
        ui.horizontal_wrapped(|ui| {
            theme::glyph(ui, ico, accent);
            let who = if human {
                format!("Human · {}", preview(actor_name, 60, 0))
            } else {
                preview(actor_name, 60, 0)
            };
            ui.strong(who).on_hover_text(actor);
            ui.label(verb);
        });

        let package = match row.kind {
            Issue => Some((s(body, "reference"), s(&body["record"], "title"))),
            ReceivingCheck => Some((s(body, "delivery"), self.feedback_title(event, s(body, "delivery")))),
            Feedback | Review => {
                let reference = if matches!(row.kind, Feedback) {
                    s(body, "delivery")
                } else {
                    s(&body["decision"], "delivery")
                };
                Some((reference, self.feedback_title(event, reference)))
            }
            _ => None,
        };
        let request = match row.kind {
            Request | Withdrawal => Some(s(body, "reference")),
            Return => Some(s(body, "human_request")),
            _ => None,
        };
        let title = package
            .map(|(_, title)| title)
            .unwrap_or_else(|| s(body, "title"));
        if !title.is_empty() {
            ui.label(RichText::new(preview(title, 120, 0)).strong())
                .on_hover_text(title);
        }
        let mut open = None;
        match row.kind {
            File(_) => {
                let reference = s(file, "file_reference");
                let path = s(file, "file_path");
                if let Some((_, revision)) = edition(reference, "asset:") {
                    if ui
                        .link(format!("{} · rev {revision}", preview(path, 120, 0)))
                        .on_hover_text(format!("Open this file revision\n{path}\n{reference}"))
                        .clicked()
                    {
                        open = Some(Open::Resource(work.into(), reference.into()));
                    }
                }
            }
            Print => {
                if ui
                    .link("Open printed PDF")
                    .on_hover_text(s(file, "pdf"))
                    .clicked()
                {
                    open = Some(Open::Resource(work.into(), s(file, "pdf").into()));
                }
            }
            Issue => {
                let record = &body["record"];
                let scope = ai_personas::neural::human_requests::declared_scope(s(record, "scope"));
                let count = record["artifacts"].as_array().map_or(0, Vec::len);
                ui.small(format!(
                    "{} · {scope}",
                    counted(count as u64, "file", "files")
                ));
            }
            _ => {
                let text = match row.kind {
                    Request if event["kind"] == "human_request_packet" => s(body, "text"),
                    ReceivingCheck => s(body, "purpose"),
                    Request => s(body, "action"),
                    Withdrawal => s(body, "reason"),
                    Review => s(&body["decision"], "reason"),
                    _ => s(body, "text"),
                };
                if !text.is_empty() && event["kind"] != "human_request_progress" {
                    let (limit, line_breaks) = if ui.available_width() < 415. {
                        (150, 1)
                    } else {
                        (215, 2)
                    };
                    document::message_markdown(ui, &preview(text, limit, line_breaks));
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            if let Some((reference, _)) = package {
                if ui
                    .link(RichText::new("Open package").small())
                    .on_hover_text(reference)
                    .clicked()
                {
                    open = Some(Open::Package(reference.into()));
                }
            }
            if let Some(reference) = request {
                if ui
                    .link(RichText::new("Request thread").small())
                    .on_hover_text(format!(
                        "Open the request and its correspondence\n{reference}"
                    ))
                    .clicked()
                {
                    if let Some((id, _)) = edition(reference, "human_request:") {
                        open = Some(Open::Request(id.into()));
                    }
                }
            }
            let label = match row.kind {
                Reply => "Open reply",
                Message => "Open message",
                _ => "View event",
            };
            if ui
                .link(RichText::new(label).small())
                .on_hover_text(format!("Activity timeline · event {sequence}"))
                .clicked()
            {
                open = Some(Open::Event(work.into(), sequence));
            }
            if let Some(files) = body["attachments"]
                .as_array()
                .filter(|files| !files.is_empty())
            {
                ui.small(counted(files.len() as u64, "attachment", "attachments"));
            }
        });
        let title = self
            .home_index
            .activity(work)
            .map_or(work, |activity| activity.title.as_str());
        let utc = s(event, "created_utc");
        ui.horizontal_wrapped(|ui| {
            if !utc.is_empty() {
                ui.small(utc);
                ui.small("·");
            }
            ui.small(preview(title, 52, 0)).on_hover_text(title);
        });
        open
    }
}
