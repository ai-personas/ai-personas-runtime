use super::*;
use ai_personas::neural::human_requests::{FieldReport, Response, ReturnForm};
mod changes;
mod correspondence;
mod field_report;
mod packet;

#[derive(Default)]
pub(super) struct Inbox {
    tab: InboxTab,
    selected: String,
    drafts: BTreeMap<String, Draft>,
    detail_tab: Option<DetailTab>,
    correspondence: correspondence::Panel,
    changes: Option<changes::Reader>,
}

#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
enum InboxTab {
    #[default]
    Active,
    Open,
    InProgress,
    Closed,
    Messages,
}

impl InboxTab {
    fn includes(self, state: &str) -> bool {
        match self {
            Self::Active => matches!(state, "open" | "in_progress"),
            Self::Open => state == "open",
            Self::InProgress => state == "in_progress",
            Self::Closed => matches!(state, "completed" | "unable" | "withdrawn"),
            Self::Messages => false,
        }
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum DetailTab {
    Request,
    Conversation,
    Response,
}

#[derive(Default)]
struct Draft {
    basis: String,
    note: String,
    picker: super::input_files::Picker,
    attachments: Vec<ai_personas::neural::input_files::CapturedFile>,
    field: field_report::Reader,
    attempt: Option<Attempt>,
}

#[derive(Clone)]
struct Attempt {
    id: String,
    basis: String,
    operator: String,
    action: Response,
    note: String,
    attachments: Vec<ai_personas::neural::input_files::CapturedFile>,
    field: Option<Arc<FieldReport>>,
}

impl Attempt {
    fn same_draft(&self, draft: &Draft) -> bool {
        self.basis == draft.basis
            && self.note == draft.note
            && self.field.as_ref().map(|f| (f.source().name(), f.source().sha256()))
                == draft.field.report.as_ref().map(|f| (f.source().name(), f.source().sha256()))
            && self.attachments.len() == draft.attachments.len()
            && self
                .attachments
                .iter()
                .zip(&draft.attachments)
                .all(|(a, b)| a.sha256() == b.sha256() && a.name() == b.name())
    }
}

impl Inbox {
    pub(super) fn showing(&self, id: &str) -> bool {
        self.selected == id
    }

    pub(super) fn active_list(&mut self) {
        self.tab = InboxTab::Active;
        self.list();
    }

    pub(super) fn list(&mut self) {
        self.selected.clear();
        self.detail_tab = None;
    }
    pub(super) fn response_saved(&mut self, reference: &str) {
        let Some((key, rev)) = reference.rsplit_once('@') else {
            return;
        };
        let Some(prior) = rev.parse::<u64>().ok().and_then(|n| n.checked_sub(1)) else {
            return;
        };
        let Some(draft) = self.drafts.get_mut(key) else {
            return;
        };
        let Some(sent) = draft
            .attempt
            .as_ref()
            .filter(|sent| sent.basis == format!("{key}@{prior}"))
        else {
            return;
        };
        if sent.same_draft(draft) {
            self.drafts.remove(key);
        } else {
            draft.attempt = None;
        }
    }
}

pub(super) fn handle(record: &Value) -> String {
    let r = &record["reference"];
    format!("{}:{}@{}", s(r, "kind"), s(r, "id"), r["revision"])
}

pub(super) fn phase(state: &str) -> (&'static str, Color32) {
    match state {
        "open" => ("Open", theme::ACCENT),
        "in_progress" => ("In progress", TEAL),
        "completed" => ("Reported complete", theme::GREEN),
        "unable" => ("Unable to complete", PURPLE),
        "withdrawn" => ("Withdrawn by requester", Color32::GRAY),
        _ => ("Unknown state", Color32::GRAY),
    }
}

impl App {
    pub(super) fn show_human_request(&mut self, id: &str) {
        if self.inbox.selected != id {
            self.inbox.detail_tab = None;
        }
        if let Some(body) = self
            .records("human_requests")
            .iter()
            .find(|r| r["reference"]["id"] == id)
            .map(|r| r["body"].clone())
        {
            self.work = s(&body, "work").into();
            self.persona = s(&body, "requester").into();
            self.conversation_page = 0;
            let state = s(&body, "state");
            if !self.inbox.tab.includes(state) {
                self.inbox.tab = if InboxTab::Closed.includes(state) { InboxTab::Closed } else { InboxTab::Active };
            }
        }
        self.inbox.selected = id.into();
        self.layout.editor = Editor::Inbox;
    }

    pub(super) fn work_name(&self, work: &str) -> String {
        self.home_index
            .activity(work)
            .map(|a| a.title.clone())
            .unwrap_or_else(|| work.into())
    }

    pub(super) fn attachment_links(&mut self, ui: &mut Ui, references: &Value, edition: Option<&str>) {
        for reference in references
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let name = self
                .snapshot
                .assets
                .iter()
                .find(|a| a["reference"] == reference)
                .map(|a| s(&a["body"], "path"))
                .unwrap_or(reference)
                .to_owned();
            if ui.link(&name).on_hover_text(reference).clicked() {
                if let Some(edition) = edition { self.open_package_file(ui.ctx(), edition, reference); }
                else { self.open_resource(ui.ctx(), reference.into()); }
            }
        }
    }

    pub(super) fn human_inbox(&mut self, ui: &mut Ui) {
        let tab = (ui.available_width() < 870.).then_some(self.inbox.detail_tab).flatten();
        egui::ScrollArea::vertical().id_salt(("human-inbox",&self.inbox.selected,self.inbox.tab,tab)).show(ui, |ui| {
            if !self.inbox.selected.is_empty() {
                if ui.small_button("← All human requests").clicked() { self.inbox.list(); }
                else if let Some(r)=self.records("human_requests").iter().find(|r|r["reference"]["id"]==self.inbox.selected).cloned() {
                    self.request_detail(ui,&r);
                } else {
                    ui.heading("Request unavailable");
                    if let Some(step) = self.records("unavailable_human_requests").iter()
                        .find(|step| step["request_id"] == self.inbox.selected) {
                        ui.label("The current status could not be verified. No response can be filed against this view.");
                        ui.weak(s(step, "unavailable"));
                    } else {
                        ui.weak("This workspace view has no retained request matching the selection.");
                    }
                }
                return;
            }
            let palette = theme::palette(ui);
            egui::Frame::new().fill(palette.header).corner_radius(8).inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                eyebrow(ui, "Persona Studio  /  Outside work");
                ui.label(design::display("Field desk", 28.));
                ui.label("Take an outside step or contribute an observation. Each persona chooses what to do while a request is open.");
            });
            ui.add_space(10.);
            let mut requests=self.records("human_requests").to_vec();
            requests.sort_by_key(|r|std::cmp::Reverse((r["body"]["updated"].as_i64(),r["reference"]["id"].as_str().unwrap_or("").to_owned())));
            let messages: Vec<_>=self.events.iter().rev().filter(|e|e["target"]=="user" && e["invocation"].is_string()
                && matches!(s(e,"kind"),"message"|"reply")).collect();
            let open = requests.iter().filter(|r| r["body"]["state"] == "open").count();
            let in_progress = requests.iter().filter(|r| r["body"]["state"] == "in_progress").count();
            let closed = requests.iter().filter(|r| InboxTab::Closed.includes(s(&r["body"], "state"))).count();
            ui.horizontal_wrapped(|ui|{
                for (tab, label, count) in [
                    (InboxTab::Active, "Active", open + in_progress),
                    (InboxTab::Open, "Open", open),
                    (InboxTab::InProgress, "In progress", in_progress),
                    (InboxTab::Closed, "Closed", closed),
                    (InboxTab::Messages, "Messages", messages.len()),
                ] {
                    ui.selectable_value(&mut self.inbox.tab, tab, format!("{label}  {count}"));
                }
            });
            ui.add_space(14.);
            if self.inbox.tab == InboxTab::Messages {
                let messages=messages.into_iter().cloned().collect::<Vec<_>>();
                ui.weak("Messages to the shared workspace inbox, not a private operator mailbox.");
                if messages.is_empty() { ui.weak("No messages recorded here yet."); }
                for e in messages {
                    let b=&e["body"];
                    card(ui).show(ui,|ui|{
                        ui.set_min_width(ui.available_width());
                        self.message_heading(ui,&e);
                        ui.label(excerpt(s(b,"text"),260));
                        self.attachment_links(ui,&b["references"], None);
                        ui.horizontal_wrapped(|ui|{
                            if ui.small_button("Read message").clicked() {
                                self.open_resource(ui.ctx(),format!("experience:event-{}@1",e["event"]));
                            }
                            if ui.small_button("Reply…").clicked() {
                                self.dialog=Some(Dialog::Participation{work:s(&e,"work").into(),title:self.work_name(s(&e,"work")),
                                    action:dialogs::Action::Message,person:s(&e,"actor").into(),text:String::new(),
                                    sources:Default::default(),
                                    reply_to:Some((format!("experience:event-{}@1",e["event"]),excerpt(s(b,"text"),180)))});
                            }
                        });
                    });
                    ui.add_space(6.);
                }
                return;
            }
            let unknown = self.records("unavailable_human_requests");
            if !unknown.is_empty() {
                egui::CollapsingHeader::new(format!("Status unavailable · {}", unknown.len()))
                    .show(ui, |ui| {
                        ui.weak("These requests are not counted as active or closed. Their current editions could not be verified.");
                        for step in unknown {
                            ui.label(s(step, "unavailable")).on_hover_text(s(step, "request_id"));
                        }
                    });
            }
            let filter = self.inbox.tab;
            let mut visible = requests.into_iter().filter(|r| filter.includes(s(&r["body"], "state"))).peekable();
            if visible.peek().is_none() {
                let (label, detail) = match filter {
                    InboxTab::Active => ("No active human requests", "Requests waiting for someone and requests already taken appear here."),
                    InboxTab::Open => ("No open requests", "When a persona asks for help, you can take the request and its files here."),
                    InboxTab::InProgress => ("No requests in progress", "Requests someone has taken will appear here."),
                    InboxTab::Closed => ("No closed requests", "Completed requests and reports of difficulties will appear here."),
                    InboxTab::Messages => return,
                };
                card(ui).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.strong(if unknown.is_empty() { label } else { "No verified requests in this view" });
                    ui.weak(detail);
                });
            }
            for r in visible {
                self.request_card(ui, &r);
                ui.add_space(8.);
            }
        });
    }

    pub(super) fn request_card(&mut self, ui: &mut Ui, record: &Value) {
        let body = &record["body"];
        let (status, color) = phase(s(body, "state"));
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let requester = s(body, "requester");
                let name = self.label(requester);
                let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(28.), egui::Sense::hover());
                design::persona_mark(ui, mark, name, persona_color(requester));
                ui.strong(name).on_hover_text(requester);
                theme::badge(ui, status, color);
                let id = s(body, "work");
                if self.home_index.activity(id).is_some_and(|a| a.phase == "dissolved") {
                    ui.small("Activity ended");
                }
                let work = self.work_name(id);
                ui.weak(excerpt(&work, 62)).on_hover_text(work);
            });
            ui.add_space(5.);
            if ui.link(RichText::new(s(body, "title")).strong().size(18.)).clicked() {
                self.show_human_request(s(&record["reference"], "id"));
            }
            ui.label(excerpt(s(body, "action"), 165));
            ui.horizontal_wrapped(|ui| {
                let n = body["artifacts"].as_array().map_or(0, Vec::len);
                if n > 0 {
                    theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                    if body["field_packet"].is_object() { ui.strong("Field packet"); }
                    ui.weak(counted(n as u64, "attached file", "attached files"));
                }
                if let Some(who) = body["claimed_by"].as_str() {
                    let verb = if body["state"] == "in_progress" { "Taken by" } else { "Reported by" };
                    ui.weak(format!("{verb} {who}"));
                } else if body["state"] == "open" {
                    ui.weak("Not yet taken");
                }
                if body["last_report"]["action"] == "contribute" {
                    ui.small(format!("Observation from {}", s(&body["last_report"], "human")));
                }
                if ui.button("Open request →").clicked() {
                    self.show_human_request(s(&record["reference"], "id"));
                }
            });
        });
    }

    fn request_detail(&mut self, ui: &mut Ui, record: &Value) {
        let body = &record["body"];
        let reference = handle(record);
        let (key, revision) = reference.rsplit_once('@').unwrap();
        let requester = s(body, "requester");
        let work = s(body, "work");
        let state = s(body, "state");
        let (status, color) = phase(state);
        let active = matches!(state, "open" | "in_progress");
        let ended = self.home_index.activity(work).is_some_and(|a| a.phase == "dissolved");
        let has_draft = self.inbox.drafts.get(key).is_some_and(|d| {
            !d.note.is_empty()
                || !d.attachments.is_empty()
                || d.picker.busy()
                || d.field.busy()
                || d.field.report.is_some()
                || d.attempt.is_some()
        });
        let response_visible = active || has_draft;
        let files = body["artifacts"].as_array().map_or(0, Vec::len);
        let p = theme::palette(ui);
        ui.add_space(8.);
        egui::Frame::new()
            .fill(p.header)
            .stroke(Stroke::new(1., p.border))
            .corner_radius(8)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    theme::badge(ui, status, color);
                    let mut context = Vec::new();
                    if !active { context.push("Closed".to_owned()); }
                    if let Some(who) = body["claimed_by"].as_str() {
                        let verb = if active { "Taken by" } else { "Reported by" };
                        context.push(format!("{verb} {who}"));
                    }
                    context.push(format!("Revision {revision}"));
                    ui.small(context.join("  ·  ")).on_hover_text(&reference);
                });
                ui.add_space(3.);
                ui.add(egui::Label::new(design::display(s(body, "title"), 29.)).wrap());
                self.requester_continuation(ui, &reference, requester, work);
                if ended {
                    ui.weak(if active {
                        "Activity ended · The outside step is still active. A report changes the request, not the ended participation."
                    } else {
                        "Activity ended · The request’s own disposition is shown above."
                    });
                }
                ui.add_space(4.);
                ui.horizontal_wrapped(|ui| {
                    if ended {
                        if ui.button("Open activity record").clicked() { self.open_home_activity(work); }
                    } else {
                        let open_activity = if body["field_packet"].is_object() {
                            egui::Button::new("Open live activity")
                        } else { theme::primary("Open live activity") };
                        if ui.add(open_activity).on_hover_text(format!("Open this activity with the composer addressed to {}", self.label(requester))).clicked() {
                            self.open_home_message(work, requester);
                        }
                    }
                    let handoff = "Save the printable request and its exact attachments. Active requests with a verified origin include a fillable field report.";
                    if !body["field_packet"].is_object() && ui.button("Save handoff…").on_hover_text(handoff).clicked() {
                        self.save_current_request(record, true);
                    }
                    if files > 0
                        && ui
                            .button("Save files as ZIP…")
                            .on_hover_text("Save the request files with their original paths")
                            .clicked()
                    {
                        self.save_current_request(record, false);
                    }
                });
            });
        ui.add_space(12.);
        if body["field_packet"].is_object() {
            self.field_packet(ui, record);
            ui.add_space(10.);
        }
        let wide = ui.available_width() >= 870.;
        if wide {
            let gap = 16.;
            let width = ui.available_width() - gap;
            let thread_width = width * 0.56;
            let has_returns = body["last_report"].is_object();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.allocate_ui_with_layout(
                    egui::vec2(thread_width, 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(thread_width);
                        if has_returns {
                            self.request_correspondence(ui, &reference, work, requester, true);
                        } else {
                            self.request_instructions(ui, body);
                            self.request_materials(ui, &reference, body);
                        }
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(width - thread_width, 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width - thread_width);
                        if response_visible {
                            self.request_response(ui, &reference, body, active);
                        }
                        if has_returns {
                            if response_visible { ui.add_space(10.); }
                            self.request_instructions(ui, body);
                            self.request_materials(ui, &reference, body);
                        } else {
                            ui.add_space(10.);
                            self.request_correspondence(ui, &reference, work, requester, true);
                        }
                    },
                );
            });
        } else {
            let default = if body["last_report"].is_object() {
                DetailTab::Conversation
            } else {
                DetailTab::Request
            };
            let mut tab = self.inbox.detail_tab.unwrap_or(default);
            if tab == DetailTab::Response && !response_visible {
                tab = default;
            }
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut tab, DetailTab::Request, "Request & files");
                ui.selectable_value(&mut tab, DetailTab::Conversation, "Correspondence");
                if response_visible {
                    ui.selectable_value(&mut tab, DetailTab::Response, "Your response");
                }
            });
            self.inbox.detail_tab = Some(tab);
            ui.add_space(7.);
            match tab {
                DetailTab::Request => {
                    self.request_instructions(ui, body);
                    self.request_materials(ui, &reference, body);
                }
                DetailTab::Conversation => {
                    self.request_correspondence(ui, &reference, work, requester, false)
                }
                DetailTab::Response => self.request_response(ui, &reference, body, active),
            }
        }
    }

    fn request_instructions(&mut self, ui: &mut Ui, body: &Value) {
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Requested action").size(18.).strong());
            ui.add(egui::Label::new(s(body, "action")).wrap());
            if let Some(text) = body["return_requested"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
            {
                ui.add_space(6.);
                ui.strong("What to send back");
                ui.add(egui::Label::new(text).wrap());
            }
            if let Some(reason) = body["withdrawal"]["reason"].as_str() {
                ui.add_space(6.);
                ui.strong("Reason for withdrawal");
                ui.add(egui::Label::new(reason).wrap());
            }
            if let Some(decision) = body["decision"].as_str() {
                if ui
                    .link("Requesting decision")
                    .on_hover_text(decision)
                    .clicked()
                {
                    self.open_resource(ui.ctx(), decision.to_owned());
                }
            }
        });
    }

    fn requester_continuation(&self, ui: &mut Ui, reference: &str, actor: &str, work: &str) {
        let next = self.records("participation").iter().map(|r| &r["body"])
            .find(|p| p["persona"] == actor && p["work"] == work)
            .map(|p| &p["continuation"]);
        let name = self.label(actor);
        let activity = self.work_name(work);
        let request_key = reference.rsplit_once('@').map(|pair| pair.0);
        let tracks_request = next.filter(|c| c["kind"] == "wait")
            .and_then(|c| c["on_events"].as_array()).into_iter().flatten().filter_map(Value::as_str)
            .any(|t| t.rsplit_once('@').is_some_and(|(key, _)| Some(key) == request_key));
        ui.add_space(4.);
        ui.horizontal_wrapped(|ui| {
            let (portrait, _) = ui.allocate_exact_size(egui::vec2(24., 24.), egui::Sense::hover());
            design::persona_mark(ui, portrait, name, persona_color(actor));
            ui.strong(format!("Requested by {name}")).on_hover_text(actor);
            if tracks_request {
                ui.small("Following updates on this request");
            } else if next.is_some() {
                ui.small(self.persona_state_in(actor, work));
            }
        });
        let limit = if ui.available_width() < 520. { 48 } else { 74 };
        ui.weak(home::short_text(&activity, limit)).on_hover_text(activity);
        if let Some(reason) = next.and_then(|c| c["purpose"].as_str().or_else(|| c["reason"].as_str())) {
            egui::CollapsingHeader::new(format!("{name}’s continuation")).id_salt(("requester-continuation", reference))
                .show(ui, |ui| { ui.add(egui::Label::new(reason).wrap()); });
        }
        let Some((key, revision)) = reference.rsplit_once('@') else { return };
        let Some(id) = key.strip_prefix("human_request:") else { return };
        let mut followers = Vec::new();
        for p in self.records("participation").iter().map(|r| &r["body"]) {
            let next = &p["continuation"];
            if p["work"] != work || p["persona"] == actor || next["kind"] != "wait" { continue; }
            if let Some(seen) = p["peer_request_cursors"][id].as_i64() {
                if next["on_events"].as_array().into_iter().flatten().filter_map(Value::as_str)
                    .any(|s| s.rsplit_once('@').is_some_and(|(k, _)| k == key)) {
                    followers.push((s(p,"persona"), seen, s(next,"reason")));
                }
            }
        }
        if !followers.is_empty() {
            egui::CollapsingHeader::new(format!("Peers following this step · {}", followers.len()))
                .id_salt(("outside-step-followers", key)).show(ui, |ui| {
                    ui.weak("A newer edition can wake these peers. Following does not assign the outside step or forward the human’s report.");
                    for (person, seen, reason) in followers {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(self.label(person));
                            ui.label(format!("Watching for changes since edition {seen}"));
                            if revision.parse::<i64>().is_ok_and(|n| n > seen) { ui.small("New edition available"); }
                        });
                        ui.add(egui::Label::new(reason).wrap());
                    }
                });
        }
    }

    fn request_materials(&mut self, ui: &mut Ui, reference: &str, body: &Value) {
        let files = body["artifacts"].as_array().map_or(0, Vec::len);
        if files == 0 && !body["deliveries"].as_array().is_some_and(|d| !d.is_empty()) {
            return;
        }
        ui.add_space(10.);
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.request_packages(ui, body);
            if files > 0 {
                if body["deliveries"].as_array().is_some_and(|d| !d.is_empty()) {
                    ui.separator();
                }
                ui.label(
                    RichText::new(format!("Attached files · {files}"))
                        .size(16.)
                        .strong(),
                );
                self.attachment_links(ui, &body["artifacts"], Some(reference));
            }
        });
    }

    fn request_response(&mut self, ui: &mut Ui, reference: &str, body: &Value, active: bool) {
        let mut retry = None;
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if active {
                self.human_response_form(ui, reference, body);
            } else {
                ui.strong("Your unsent response");
                if let Some(draft) = reference
                    .rsplit_once('@')
                    .and_then(|(key, _)| self.inbox.drafts.get_mut(key))
                {
                    draft.picker.poll(&mut draft.attachments);
                    if let Some(basis) = draft.field.poll() {
                        draft.basis = basis;
                    }
                    if let Some(report) = &draft.field.report {
                        ui.label(format!("Field report from {}", report.observer()));
                        ui.label(excerpt(report.observations(), 300));
                    }
                    ui.add_enabled_ui(!self.busy, |ui| {
                        retry = retry_return(ui, draft, self.operator.trim());
                    });
                    if !draft.note.is_empty() {
                        ui.add(egui::Label::new(&draft.note).wrap().selectable(true));
                    }
                    for file in &draft.attachments {
                        ui.small(format!("{} · {}", file.name(), size(file.len() as u64)));
                    }
                    ui.weak("This request is closed.");
                }
            }
        });
        if let Some(attempt) = retry {
            self.submit_human_return(ui.ctx(), attempt);
        }
    }

    fn request_packages(&mut self, ui: &mut Ui, request: &Value) {
        let references: Vec<_> = request["deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if references.is_empty() {
            return;
        }
        let summaries: BTreeMap<_, _> = request["delivery_details"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|package| Some((package["reference"].as_str()?, package)))
            .collect();
        ui.label(
            RichText::new(format!("Included packages · {}", references.len()))
                .size(16.)
                .strong(),
        );
        for reference in references {
            ui.push_id(reference, |ui| {
                let Some(&package) = summaries.get(reference) else {
                    if ui
                        .link(reference)
                        .on_hover_text("Open this package revision")
                        .clicked()
                    {
                        self.open_issued_package(ui.ctx(), reference);
                    }
                    return;
                };
                ui.add_space(4.);
                if ui
                    .link(RichText::new(s(package, "title")).strong())
                    .on_hover_text(reference)
                    .clicked()
                {
                    self.open_issued_package(ui.ctx(), reference);
                }
                ui.horizontal_wrapped(|ui| {
                    let issuer = s(package, "owner");
                    ui.small(format!("Issued by {}", self.label(issuer)))
                        .on_hover_text(issuer);
                    let scope = s(package, "scope");
                    ui.small("Declared scope");
                    theme::badge(
                        ui,
                        ai_personas::neural::human_requests::declared_scope(scope),
                        if scope == "partial" {
                            theme::ACCENT
                        } else {
                            PURPLE
                        },
                    );
                    let revision = reference.rsplit_once('@').map_or_else(
                        || reference.to_owned(),
                        |(_, n)| format!("Open revision {n}"),
                    );
                    if ui.link(revision).on_hover_text(reference).clicked() {
                        self.open_issued_package(ui.ctx(), reference);
                    }
                });
                ui.small("Supported use");
                ui.add(egui::Label::new(s(package, "supported_use")).wrap());
                if let Some(items) = package["remaining"]
                    .as_array()
                    .filter(|items| !items.is_empty())
                {
                    ui.small("Open at issue");
                    for item in items.iter().filter_map(Value::as_str) {
                        ui.add(egui::Label::new(format!("• {item}")).wrap());
                    }
                } else {
                    ui.small("Open at issue · None listed");
                }
            });
        }
    }

    fn human_response_form(&mut self, ui: &mut Ui, reference: &str, body: &Value) {
        let key = reference.rsplit_once('@').unwrap().0.to_owned();
        let operator = self.operator.trim().to_owned();
        let draft = self
            .inbox
            .drafts
            .entry(key.clone())
            .or_insert_with(|| Draft {
                basis: reference.into(),
                ..Default::default()
            });
        draft.picker.poll(&mut draft.attachments);
        if let Some(basis) = draft.field.poll() {
            draft.basis = basis;
        }
        if draft.basis != reference
            && draft.note.is_empty()
            && !draft.picker.busy()
            && !draft.field.busy()
            && draft.field.report.is_none()
            && draft.attachments.is_empty()
            && draft.attempt.is_none()
        {
            draft.basis = reference.to_owned();
        }
        let holder = body["claimed_by"].as_str();
        let may_manage = holder.is_none() || holder == Some(operator.as_str());
        eyebrow(ui, "Field desk  /  Human return");
        ui.label(RichText::new(format!("Response from {operator}")).size(18.).strong());
        if let Some(name) = holder.filter(|_| !may_manage) {
            ui.label(format!("{name} holds this step. You can send evidence without changing who is responsible."));
        } else if holder.is_none() {
            ui.weak("Take responsibility for this step, or send an observation without taking it.");
        }
        let mut action = None;
        let mut retry = None;
        ui.add_enabled_ui(!self.busy, |ui| {
            retry = retry_return(ui, draft, &operator);
            draft.field.show(ui, &self.root, reference, &draft.attachments);
            if draft.basis != reference {
                let edition = draft.basis.rsplit_once('@').map_or("?", |v| v.1);
                ui.label(format!("This draft uses request edition {edition}. Review the displayed revision before sending.")).on_hover_text(&draft.basis);
                if ui.button("Use the displayed revision").clicked() {
                    draft.basis = reference.into();
                }
            }
            ui.add(
                egui::TextEdit::multiline(&mut draft.note)
                    .id(egui::Id::new(("human-return", &self.root, key)))
                    .hint_text(if draft.field.report.is_some() { "Add a note as the submitting operator (optional)…" }
                        else { "Describe what you did, observed or still need…" })
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            draft.picker.show(ui, &mut draft.attachments);
            ui.add_enabled_ui(
                draft.basis == reference && !operator.is_empty() && !draft.picker.busy() && !draft.field.busy()
                    && draft.field.report.as_ref().is_none_or(|report| report.missing_evidence(&draft.attachments).is_empty()),
                |ui| {
                    let imported = draft.field.report.is_some();
                    let written = imported || !draft.note.trim().is_empty();
                    ui.horizontal_wrapped(|ui| {
                        if body["state"] == "open"
                            && ui.add(theme::primary(if imported { "Take & send report" } else { "Take request" })).clicked()
                        {
                            action = Some(Response::Take);
                        }
                        if holder != Some(operator.as_str()) {
                            let send = if may_manage { egui::Button::new("Send observation") } else { theme::primary("Send observation") };
                            if ui.add_enabled(written || !draft.attachments.is_empty(), send)
                                .on_hover_text("Send your report to the requesting persona. Keep the current status and holder.").clicked() {
                                action = Some(Response::Contribute);
                            }
                        }
                        if !may_manage { return; }
                        if body["state"] == "in_progress"
                            && ui
                                .add_enabled(written, theme::primary("Send progress"))
                                .clicked()
                        {
                            action = Some(Response::Update);
                        }
                        if ui
                            .add_enabled(written, egui::Button::new("Report completion"))
                            .clicked()
                        {
                            action = Some(Response::Complete);
                        }
                        if ui
                            .add_enabled(written, egui::Button::new("Unable to complete"))
                            .clicked()
                        {
                            action = Some(Response::Unable);
                        }
                        if body["state"] == "in_progress" && ui.button(if imported { "Release & send report" } else { "Release request" }).clicked()
                        {
                            action = Some(Response::Release);
                        }
                    });
                },
            );
        });
        if let Some(action) = action {
            if let Some(attempt) = draft.attempt.as_ref().filter(|old| {
                old.operator == operator
                    && std::mem::discriminant(&old.action) == std::mem::discriminant(&action)
                    && old.same_draft(draft)
            }) {
                retry = Some(attempt.clone());
            } else {
                let id = match ai_personas::neural::id("studio-human-return") {
                    Ok(id) => id,
                    Err(e) => {
                        self.error = Some(e.to_string());
                        return;
                    }
                };
                let attempt = Attempt {
                    id,
                    basis: draft.basis.clone(),
                    operator,
                    action,
                    note: draft.note.clone(),
                    attachments: draft.attachments.clone(),
                    field: draft.field.report.clone(),
                };
                draft.attempt = Some(attempt.clone());
                retry = Some(attempt);
            }
        }
        if let Some(attempt) = retry {
            self.submit_human_return(ui.ctx(), attempt);
        }
    }

    fn submit_human_return(&mut self, ctx: &egui::Context, attempt: Attempt) {
        self.busy = true;
        self.error = None;
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let cmd = Command::HumanRespond {
                request: attempt.basis,
                response: attempt.action,
                note: attempt.note,
                files: attempt.attachments.into(),
                field_report: ReturnForm::from_report(attempt.field.as_deref()),
            };
            let result = operator_call(&root, &attempt.operator, cmd, Some(&attempt.id));
            let _ = tx.send(Update::Done { result, start_workspace: false });
            ctx.request_repaint();
        });
    }
}

fn retry_return(ui: &mut Ui, draft: &Draft, operator: &str) -> Option<Attempt> {
    let attempt = draft.attempt.as_ref()?;
    if !ui.is_enabled() {
        return None;
    }
    ui.small("Previous submission unconfirmed");
    if attempt.operator == operator
        && ui
            .small_button("Retry exact submission")
            .on_hover_text(format!(
                "Resend the same recorded operation for {}",
                attempt.basis
            ))
            .clicked()
    {
        Some(attempt.clone())
    } else {
        None
    }
}
