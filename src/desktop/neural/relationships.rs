//! Inspect an account from its author's perspective and trace it back to the exchange.
mod map;
mod reader;
use super::*;
use ai_personas::neural::{
    self,
    relationships::{self as records, Contact, History, Kind, Page},
};
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
};

const PAGE: usize = 6;

struct View {
    contacts: Arc<Vec<Value>>,
    selected: Option<Contact>,
    history: Value,
    account: Value,
    trace: Option<Value>,
}

fn fetch(
    root: &Path,
    owner: &str,
    chosen: Option<Contact>,
    cached: Option<Arc<Vec<Value>>>,
    encounter_offset: usize,
    revision_offset: usize,
    trace_reference: Option<&str>,
) -> neural::Result<View> {
    let db = data::open(root)?;
    let contacts = match cached {
        Some(items) => items,
        None => {
            let index = records::contacts(
                &db,
                owner,
                &Page {
                    offset: 0,
                    page_size: usize::MAX,
                },
            )?;
            Arc::new(serde_json::from_value(index["items"].clone())?)
        }
    };
    let selected = match chosen {
        Some(person) => Some(person),
        None => contacts
            .iter()
            .find(|r| r["note"]["status"] == "current")
            .or_else(|| contacts.first())
            .map(|r| serde_json::from_value(r["counterpart"].clone()))
            .transpose()?,
    };
    let mut history = Value::Null;
    let mut account = Value::Null;
    if let Some(person) = &selected {
        history = records::history(
            &db,
            owner,
            &History {
                counterpart: person.clone(),
                page: Page {
                    offset: encounter_offset,
                    page_size: PAGE,
                },
                revision_offset,
            },
        )?;
        if let Some(reference) = history["current"]["reference"].as_str() {
            account = neural::context::read(&db, reference, None, None)?["record"].take();
        }
    }
    Ok(View {
        contacts,
        selected,
        history,
        account,
        trace: trace_reference
            .map(|r| records::trace(&db, r))
            .transpose()?,
    })
}

#[derive(Default)]
pub(super) struct Panel {
    map: map::Map,
    seen_cursor: Option<i64>,
    owner: String,
    selected: Option<Contact>,
    contacts: Option<Arc<Vec<Value>>>,
    result: Option<Result<View, String>>,
    pending: Option<Receiver<Result<View, String>>>,
    filter: String,
    tab: usize,
    encounters: usize,
    revisions: usize,
    trace_reference: Option<String>,
    trace_start: usize,
    recipient_start: usize,
    exchanges: Vec<String>,
}

enum Action {
    Agreement { work: String, edition: String },
    Refresh,
    Select(Contact),
    Reverse(String),
    Inspect(String),
    Raw(String),
    Trace(String),
    Page { encounters: usize, revisions: usize },
}

impl Panel {
    fn select(&mut self, owner: &str, person: Option<Contact>) {
        if self.owner != owner {
            self.map = Default::default();
            self.contacts = None;
        }
        self.owner = owner.into();
        self.selected = person;
        self.encounters = 0;
        self.revisions = 0;
        self.trace_reference = None;
        self.trace_start = 0;
        self.recipient_start = 0;
        self.filter.clear();
        self.exchanges.clear();
        self.tab = 0;
    }
    fn load(&mut self, ctx: &egui::Context, root: &Path) {
        let (sender, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.result = None;
        let (ctx, root, owner, chosen, cached, start, revision) = (
            ctx.clone(),
            root.to_owned(),
            self.owner.clone(),
            self.selected.clone(),
            self.contacts.clone(),
            self.encounters,
            self.revisions,
        );
        let trace = if self.tab == 3 {
            self.trace_reference.clone()
        } else {
            None
        };
        std::thread::spawn(move || {
            let answer = fetch(
                &root,
                &owner,
                chosen,
                cached,
                start,
                revision,
                trace.as_deref(),
            )
            .map_err(|e| e.to_string());
            let _ = sender.send(answer);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        let answer = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The relationship reader closed.".into()),
        };
        if let Ok(view) = &answer {
            self.selected.clone_from(&view.selected);
            self.contacts = Some(view.contacts.clone());
        }
        self.result = Some(answer);
        self.pending = None;
    }
    fn show(
        &mut self,
        ui: &mut Ui,
        status: &Value,
        owner_color: Color32,
        page_scrolls: bool,
    ) -> Option<Action> {
        self.poll();
        let view = match &self.result {
            Some(Ok(report)) => report,
            Some(Err(message)) => {
                ui.colored_label(theme::ACCENT, message);
                return ui.button("Try again").clicked().then_some(Action::Refresh);
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading relationship history…");
                });
                return None;
            }
        };
        let mut action = None;
        let selected = view.selected.as_ref();
        ui.horizontal_wrapped(|ui| {
            ui.label("Counterpart");
            egui::ComboBox::from_id_salt(("relationship-contact", &self.owner))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .selected_text(
                    selected
                        .map(|p| contact_label(status, p))
                        .unwrap_or_else(|| "Choose a person".into()),
                )
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("Find a name or role"),
                    );
                    let term = self.filter.trim().to_lowercase();
                    for row in view.contacts.iter() {
                        let Ok(p) = serde_json::from_value::<Contact>(row["counterpart"].clone())
                        else {
                            continue;
                        };
                        let label = contact_label(status, &p);
                        if (label.to_lowercase().contains(&term)
                            || p.id.to_lowercase().contains(&term))
                            && ui.selectable_label(selected == Some(&p), label).clicked()
                        {
                            action = Some(Action::Select(p));
                            ui.close();
                        }
                    }
                });
            if ui.small_button("Refresh").clicked() {
                action = Some(Action::Refresh);
            }
        });
        let Some(person) = selected else {
            ui.add_space(10.);
            ui.label("No exchanges with a named counterpart have been recorded for this persona.");
            return action;
        };
        let owner = name(status, &self.owner);
        let partner = if person.kind == Kind::Persona {
            name(status, &person.id)
        } else {
            &person.id
        };
        let row = view.contacts.iter().find(|r| {
            r["counterpart"]["id"] == person.id && r["counterpart"]["kind"] == role(person.kind)
        });
        ui.add_space(7.);
        let section_id = (
            "relationship-content",
            &self.owner,
            &person.id,
            role(person.kind),
            self.tab,
        );
        people::scroll_section(ui, section_id, page_scrolls, |ui| {
            card(ui).show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 1.).max(0.));
                ui.horizontal_wrapped(|ui| {
                    avatar(ui, owner, owner_color, 32.);
                    ui.label(
                        RichText::new(format!("{owner}’s view of {partner}"))
                            .size(20.)
                            .strong(),
                    );
                    if person.kind == Kind::Persona
                        && ui
                            .small_button("Switch viewpoint")
                            .on_hover_text(format!("Read {partner}’s own account of {owner}"))
                            .clicked()
                    {
                        action = Some(Action::Reverse(person.id.clone()));
                    }
                });
                if let Some(row) = row {
                    ui.add_space(5.);
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(ui, &format!("{} sent", row["sent"]), TEAL);
                        theme::badge(ui, &format!("{} received", row["received"]), PURPLE);
                        ui.small(format!(
                            "{} activity-wide  ·  {} {}",
                            row["shared_in_activity"],
                            row["activity_count"],
                            if row["activity_count"] == 1 {
                                "activity"
                            } else {
                                "activities"
                            }
                        ));
                        if ui.available_width() > 225. {
                            ui.add_space(12.);
                            reader::markers(ui, &view.history, &mut action);
                        }
                    });
                }
            });
            ui.add_space(6.);
            let history = &view.history;
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.tab, 0, "Account");
                ui.selectable_value(
                    &mut self.tab,
                    1,
                    format!("Encounters ({})", history["total"]),
                );
                ui.selectable_value(
                    &mut self.tab,
                    2,
                    format!("Revisions ({})", history["revision_count"]),
                );
                if let Some(r) = history["current"]["reference"].as_str() {
                    if ui
                        .selectable_label(self.tab == 3, "Decision trace")
                        .clicked()
                    {
                        action = Some(Action::Trace(r.to_owned()));
                    }
                }
            });
            ui.separator();
            match self.tab {
                1 => {
                    for entry in items(history, "encounters") {
                        ui.push_id(s(entry, "reference"), |ui| {
                            let direction = match s(entry, "direction") {
                                "received" => format!("From {partner}"),
                                "sent" => format!("To {partner}"),
                                _ => format!("Shared by {partner}"),
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(direction);
                                ui.label(title(s(entry, "kind")));
                                ui.small(s(entry, "created_utc"));
                            });
                            ui.label(excerpt(work_name(status, s(entry, "work")), 110))
                                .on_hover_text(work_name(status, s(entry, "work")));
                            ui.horizontal_wrapped(|ui| {
                                link(ui, "Open exchange ↗", &entry["reference"], &mut action);
                                link(ui, "In reply to ↗", &entry["linked_to"], &mut action);
                            });
                            ui.separator();
                        });
                    }
                    if let Some(offset) =
                        pager(ui, self.encounters, history["total"].as_u64().unwrap_or(0))
                    {
                        action = Some(Action::Page {
                            encounters: offset,
                            revisions: self.revisions,
                        });
                    }
                }
                2 => {
                    if history["revision_count"] == 0 {
                        ui.weak("No personal account has been recorded.");
                    }
                    for revision in items(history, "revisions") {
                        ui.push_id(s(revision, "reference"), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!("Revision {}", revision["revision"]));
                                ui.label(if revision["status"] == "retired" {
                                    "Retired"
                                } else {
                                    "Authored account"
                                });
                                if revision["reference"] == history["current"]["reference"] {
                                    theme::badge(ui, "Latest", TEAL);
                                }
                                if ui.small_button("Decision trace").clicked() {
                                    action = Some(Action::Trace(s(revision, "reference").into()));
                                }
                                link(
                                    ui,
                                    "Read this revision ↗",
                                    &revision["reference"],
                                    &mut action,
                                );
                            });
                            ui.label(excerpt(work_name(status, s(revision, "work")), 110));
                            ui.separator();
                        });
                    }
                    if let Some(offset) = pager(
                        ui,
                        self.revisions,
                        history["revision_count"].as_u64().unwrap_or(0),
                    ) {
                        action = Some(Action::Page {
                            encounters: self.encounters,
                            revisions: offset,
                        });
                    }
                }
                3 => {
                    if let Some(trace) = &view.trace {
                        let rev = s(trace, "reference").rsplit('@').next().unwrap_or_default();
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!("Revision {rev}"));
                            theme::badge(
                                ui,
                                &format!("{} cue decisions", trace["cue_decisions"]),
                                PURPLE,
                            );
                            theme::badge(
                                ui,
                                &format!("{} full-body decisions", trace["body_decisions"]),
                                TEAL,
                            );
                        });
                        if let Some(reads) = trace["recipient_inputs"]
                            .as_array()
                            .filter(|v| !v.is_empty())
                        {
                            egui::CollapsingHeader::new(format!(
                                "Shared edition received in {} peer {}",
                                reads.len(),
                                if reads.len() == 1 {
                                    "decision"
                                } else {
                                    "decisions"
                                }
                            ))
                            .id_salt(("shared-edition-inputs", s(trace, "reference")))
                            .show(ui, |ui| {
                                for input in reads.iter().skip(self.recipient_start).take(PAGE) {
                                    ui.push_id(s(input, "decision"), |ui| {
                                        ui.strong(format!(
                                            "{} · {}",
                                            name(status, s(input, "reader")),
                                            s(input, "accepted_utc")
                                        ));
                                        let work = work_name(status, s(input, "work"));
                                        ui.label(excerpt(work, 90)).on_hover_text(work);
                                        ui.horizontal_wrapped(|ui| {
                                            link(
                                                ui,
                                                "Owner’s message ↗",
                                                &input["shared_via"],
                                                &mut action,
                                            );
                                            link(
                                                ui,
                                                "Recipient input ↗",
                                                &input["input"],
                                                &mut action,
                                            );
                                            link(
                                                ui,
                                                "Recipient decision ↗",
                                                &input["decision"],
                                                &mut action,
                                            );
                                        });
                                    });
                                }
                                if let Some(next) =
                                    pager(ui, self.recipient_start, reads.len() as u64)
                                {
                                    self.recipient_start = next;
                                }
                            });
                        }
                        let decisions = trace["decisions"]
                            .as_array()
                            .map(Vec::as_slice)
                            .unwrap_or_default();
                        for row in decisions.iter().skip(self.trace_start).take(PAGE) {
                            ui.push_id(s(row, "decision"), |ui| {
                                ui.add_space(5.);
                                let work = work_name(status, s(row, "work"));
                                ui.label(RichText::new(excerpt(work, 100)).strong())
                                    .on_hover_text(work);
                                ui.small(s(row, "accepted_utc"));
                                ui.horizontal_wrapped(|ui| {
                                    for signal in items(row, "signals").filter_map(Value::as_str) {
                                        let label = match signal {
                                            "authored" => "Authored revision",
                                            "cue" => "Address supplied",
                                            "selected_body" => "Selected body received",
                                            "read_body" => "Read result received",
                                            "returned_body" => "Write result received",
                                            "selected_for_next" => "Selected for next input",
                                            "requested_read" => "Requested read",
                                            "revised_from" => "Revised from this version",
                                            "cited_in_message" => "Cited in peer message",
                                            _ => signal,
                                        };
                                        ui.label(label);
                                    }
                                });
                                ui.horizontal_wrapped(|ui| {
                                    link(ui, "Open input ↗", &row["input"], &mut action);
                                    link(ui, "Open decision ↗", &row["decision"], &mut action);
                                });
                                ui.separator();
                            });
                        }
                        if let Some(next) = pager(ui, self.trace_start, decisions.len() as u64) {
                            self.trace_start = next;
                        }
                    }
                }
                _ => {
                    let note = &view.account;
                    if note.is_null() {
                        ui.label(format!(
                            "{owner} has no saved account of working with {partner}."
                        ));
                        if ui.button("Browse their encounters").clicked() {
                            self.tab = 1;
                        }
                        return;
                    }
                    ui.horizontal_wrapped(|ui| {
                        eyebrow(
                            ui,
                            if note["status"] == "retired" {
                                "Retired account"
                            } else {
                                "Personal account"
                            },
                        );
                        ui.small(format!("Revision {}", history["current"]["revision"]));
                    });
                    if note["status"] == "retired" {
                        ui.label(s(note, "reason"));
                    } else {
                        ui.add(
                            egui::Label::new(RichText::new(s(note, "interpretation")).size(16.))
                                .wrap(),
                        );
                        ui.add_space(8.);
                        eyebrow(ui, "Scope and uncertainty");
                        ui.label(s(note, "scope"));
                        ui.add_space(8.);
                        eyebrow(ui, "Cited experiences");
                        for citation in items(note, "evidence") {
                            let reference = s(citation, "experience");
                            let label = format!(
                                "{} · {}",
                                title(s(citation, "kind")),
                                reference
                                    .trim_start_matches("experience:")
                                    .trim_end_matches("@1")
                            );
                            link(ui, &label, &citation["experience"], &mut action);
                        }
                    }
                    if let Some(call) = note["decision"].as_str() {
                        ui.add_space(6.);
                        if ui.small_button("Open authoring decision ↗").clicked() {
                            action = Some(Action::Inspect(format!("experience:{call}@1")));
                        }
                    }
                }
            }
        });
        action
    }
}

fn name<'a>(status: &'a Value, id: &'a str) -> &'a str {
    items(status, "personas")
        .find(|p| p["body"]["id"] == id)
        .and_then(|p| p["body"]["label"].as_str())
        .unwrap_or(id)
}
pub(super) fn work_name<'a>(status: &'a Value, id: &'a str) -> &'a str {
    items(status, "work")
        .find(|p| p["body"]["id"] == id)
        .map(|p| activity_title(&p["body"]))
        .unwrap_or(id)
}
fn role(kind: Kind) -> &'static str {
    match kind {
        Kind::Persona => "persona",
        Kind::Operator => "operator",
    }
}
fn contact_label(status: &Value, p: &Contact) -> String {
    match p.kind {
        Kind::Persona => format!("{} · Persona", name(status, &p.id)),
        Kind::Operator => format!("{} · Operator", p.id),
    }
}
fn items<'a>(value: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    value[field].as_array().into_iter().flatten()
}
fn link(ui: &mut Ui, label: &str, reference: &Value, action: &mut Option<Action>) {
    if let Some(reference) = reference.as_str() {
        if ui.small_button(label).on_hover_text(reference).clicked() {
            *action = Some(Action::Inspect(reference.into()));
        }
    }
}
fn pager(ui: &mut Ui, start: usize, total: u64) -> Option<usize> {
    if total <= PAGE as u64 {
        return None;
    }
    let mut offset = None;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(start > 0, egui::Button::new("← Newer"))
            .clicked()
        {
            offset = Some(start.saturating_sub(PAGE));
        }
        ui.small(format!(
            "{}–{} of {total}",
            start + 1,
            (start as u64 + PAGE as u64).min(total)
        ));
        if ui
            .add_enabled(
                (start + PAGE) < total as usize,
                egui::Button::new("Older →"),
            )
            .clicked()
        {
            offset = Some(start + PAGE);
        }
    });
    offset
}

impl App {
    pub(super) fn show_relationships(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.relationships.owner != self.persona {
            self.relationships.select(&self.persona, None);
            self.relationships.seen_cursor = Some(self.cursor);
            self.relationships.load(&ctx, &self.root);
        }
        self.relationships.poll();
        if self.relationships.seen_cursor != Some(self.cursor)
            && self.relationships.pending.is_none()
        {
            self.relationships.contacts = None;
            let prior = self.relationships.result.take();
            self.relationships.load(&ctx, &self.root);
            self.relationships.result = prior;
            self.relationships.seen_cursor = Some(self.cursor);
        }
        let profile = self.layout.editor == Editor::People
            && self.profile_view == people::ProfileView::Connections;
        let page_scrolls = profile && Self::constrained_activity_layout(&ctx);
        let on_map = profile && !self.relationships.map.detail;
        let selected = if on_map {
            match &self.relationships.result {
                Some(Ok(view)) => self.relationships.map.show(
                    ui,
                    &self.snapshot.status,
                    &self.persona,
                    &view.contacts,
                    &self.events,
                    page_scrolls,
                ),
                Some(Err(error)) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    ui.button("Retry").clicked().then_some(Action::Refresh)
                }
                None => {
                    ui.spinner();
                    None
                }
            }
        } else {
            if profile && ui.link("← All contacts").clicked() {
                self.relationships.map.detail = false;
                self.relationships.exchanges.clear();
                return;
            }
            let color = self.color(&self.persona);
            self.relationships
                .show(ui, &self.snapshot.status, color, page_scrolls)
        };
        let reading = reader::show(
            &ctx,
            &self.snapshot.status,
            &self.events,
            &mut self.relationships.exchanges,
        );
        match reading.or(selected) {
            Some(Action::Inspect(reference))
                if reader::event(&self.events, &reference).is_some() =>
            {
                if self.relationships.exchanges.last() != Some(&reference) {
                    self.relationships.exchanges.push(reference);
                }
                return;
            }
            Some(Action::Inspect(reference) | Action::Raw(reference)) => {
                self.relationships.exchanges.clear();
                self.open_resource(&ctx, reference);
                return;
            }
            Some(Action::Agreement { work, edition }) => {
                self.relationships.exchanges.clear();
                self.select_activity(&work);
                self.show_agreements(Some(edition));
                return;
            }
            Some(Action::Trace(reference)) => {
                self.relationships.tab = 3;
                self.relationships.trace_reference = Some(reference);
                self.relationships.trace_start = 0;
                self.relationships.recipient_start = 0;
            }
            Some(Action::Refresh) => {
                self.relationships.contacts = None;
            }
            Some(Action::Select(person)) => {
                self.relationships.select(&self.persona, Some(person));
                if on_map {
                    self.relationships.map.detail = true;
                    self.relationships.tab = 1;
                }
            }
            Some(Action::Reverse(next)) => {
                let old = Contact::persona(&self.persona);
                self.persona = next;
                self.relationships.select(&self.persona, Some(old));
                if profile {
                    self.relationships.map.detail = true;
                    self.relationships.tab = 1;
                }
            }
            Some(Action::Page {
                encounters,
                revisions,
            }) => {
                self.relationships.encounters = encounters;
                self.relationships.revisions = revisions;
            }
            None => return,
        }
        self.relationships.load(&ctx, &self.root);
    }
}
