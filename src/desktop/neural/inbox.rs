use super::*;
use ai_personas::neural::human_requests::Response;

#[derive(Default)]
pub(super) struct Inbox {
    tab: usize,
    selected: String,
    drafts: BTreeMap<String, Draft>,
}

#[derive(Default)]
struct Draft {
    basis: String,
    note: String,
    picker: super::input_files::Picker,
    attachments: Vec<ai_personas::neural::input_files::CapturedFile>,
}

impl Inbox {
    pub(super) fn list(&mut self) {
        self.selected.clear();
    }
    pub(super) fn response_saved(&mut self, reference: &str) {
        if let Some((key, _)) = reference.rsplit_once('@') {
            self.drafts.remove(key);
        }
    }
}

pub(super) fn handle(record: &Value) -> String {
    let r = &record["reference"];
    format!("{}:{}@{}", s(r, "kind"), s(r, "id"), r["revision"])
}

fn phase(state: &str) -> (&'static str, Color32) {
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
    pub(super) fn open_human_requests(&self, work: Option<&str>) -> usize {
        self.records("human_requests")
            .iter()
            .filter(|r| {
                let b = &r["body"];
                work.is_none_or(|w| b["work"] == w)
                    && matches!(s(b, "state"), "open" | "in_progress")
            })
            .count()
    }

    pub(super) fn show_human_request(&mut self, id: &str) {
        if let Some(body) = self
            .records("human_requests")
            .iter()
            .find(|r| r["reference"]["id"] == id)
            .map(|r| r["body"].clone())
        {
            self.work = s(&body, "work").into();
            self.persona = s(&body, "requester").into();
            self.conversation_page = 0;
        }
        self.inbox.selected = id.into();
        self.layout.editor = Editor::Inbox;
    }

    pub(super) fn work_name(&self, work: &str) -> String {
        self.home_index.activity(work).map(|a| a.title.clone()).unwrap_or_else(|| work.into())
    }

    pub(super) fn attachment_links(&mut self, ui: &mut Ui, references: &Value) {
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
                self.open_resource(ui.ctx(), reference.into());
            }
        }
    }

    pub(super) fn human_inbox(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt(("human-inbox",self.inbox.selected.clone(),self.inbox.tab)).show(ui, |ui| {
            if let Some(r)=self.records("human_requests").iter().find(|r|r["reference"]["id"]==self.inbox.selected).cloned() {
                if ui.small_button("← All human requests").clicked() { self.inbox.selected.clear(); }
                else { self.request_detail(ui,&r); }
                return;
            }
            ui.heading("Needs you");
            ui.label("Requests and messages addressed to a human across this workspace.");
            ui.add_space(12.);
            let mut requests=self.records("human_requests").to_vec();
            requests.sort_by_key(|r|std::cmp::Reverse((r["body"]["updated"].as_i64(),r["reference"]["id"].as_str().unwrap_or("").to_owned())));
            let messages: Vec<_>=self.events.iter().rev().filter(|e|e["target"]=="user" && e["invocation"].is_string()
                && matches!(s(e,"kind"),"message"|"reply")).collect();
            let counts=[requests.iter().filter(|r|r["body"]["state"]=="open").count(),
                requests.iter().filter(|r|r["body"]["state"]=="in_progress").count(),
                requests.iter().filter(|r|matches!(s(&r["body"],"state"),"completed"|"unable"|"withdrawn")).count(),messages.len()];
            ui.horizontal_wrapped(|ui|{
                for (idx,label) in ["Open","In progress","Closed","Messages"].iter().enumerate() {
                    ui.selectable_value(&mut self.inbox.tab,idx,format!("{label}  {}",counts[idx]));
                }
            });
            ui.add_space(14.);
            if self.inbox.tab==3 {
                let messages=messages.into_iter().cloned().collect::<Vec<_>>();
                if messages.is_empty() { ui.weak("No persona has sent you a message yet."); }
                for e in messages {
                    let b=&e["body"];
                    card(ui).show(ui,|ui|{
                        ui.set_min_width(ui.available_width());
                        self.message_heading(ui,&e);
                        ui.label(excerpt(s(b,"text"),260));
                        self.attachment_links(ui,&b["references"]);
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
            if counts[self.inbox.tab]==0 {
                card(ui).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.strong(["No open requests","No requests in progress","No closed requests"][self.inbox.tab]);
                    ui.weak(["When a persona asks for help, you can take the request and its files here.",
                        "Requests someone has taken will appear here.","Completed requests and reports of difficulties will appear here."][self.inbox.tab]);
                });
            }
            for r in requests {
                let b=&r["body"];
                let state=s(b,"state");
                if (self.inbox.tab==0 && state!="open") || (self.inbox.tab==1 && state!="in_progress")
                    || (self.inbox.tab==2 && !matches!(state,"completed"|"unable"|"withdrawn")) { continue; }
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
                theme::badge(ui, status, color);
                ui.strong(self.label(s(body, "requester")));
                let work = self.work_name(s(body, "work"));
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
                    ui.weak(format!("{n} attached {}", if n == 1 { "file" } else { "files" }));
                }
                if let Some(who) = body["claimed_by"].as_str() {
                    ui.weak(format!("Taken by {who}"));
                }
                if ui.button("Open request →").clicked() {
                    self.show_human_request(s(&record["reference"], "id"));
                }
            });
        });
    }

    fn request_detail(&mut self, ui: &mut Ui, r: &Value) {
        let b = &r["body"];
        let reference = handle(r);
        let (state, color) = phase(s(b, "state"));
        let requester = s(b, "requester");
        let files = b["artifacts"].as_array().map_or(0, Vec::len);
        ui.add_space(12.);
        ui.horizontal_wrapped(|ui| {
            avatar(ui, self.label(requester), self.color(requester), 32.);
            ui.strong(self.label(requester));
            theme::badge(ui, state, color);
            if let Some(who) = b["claimed_by"].as_str() {
                ui.weak(format!("Taken by {who}"));
            }
        });
        ui.heading(s(b, "title"));
        if ui
            .link(excerpt(&self.work_name(s(b, "work")), 100))
            .clicked()
        {
            self.work = s(b, "work").into();
            self.persona = requester.into();
            self.layout.editor = Editor::Studio;
        }
        ui.add_space(6.);
        ui.horizontal_wrapped(|ui| {
            let contents = if files == 0 {
                "Save Request.html in a new folder for offline reading"
            } else {
                "Save Request.html and the requester’s attached files together in a new folder"
            };
            if ui.add(theme::primary("Save handoff…")).on_hover_text(contents).clicked() {
                self.save_request_handoff(&reference, s(b, "title"), files);
            }
            if files > 0 && ui.button("Save files as ZIP…")
                .on_hover_text("Save the attached files with their original paths").clicked() {
                self.export_package(&reference, s(b, "title"));
            }
        });
        ui.add_space(12.);
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.strong("Requested action");
            ui.label(s(b, "action"));
            if let Some(reply) = b["return_requested"]
                .as_str()
                .filter(|v| !v.trim().is_empty())
            {
                ui.add_space(8.);
                ui.strong("What to send back");
                ui.label(reply);
            }
            self.request_packages(ui, b);
            if files > 0 {
                ui.add_space(10.);
                ui.separator();
                ui.strong(format!("Attached files · {files}"));
                self.attachment_links(ui, &b["artifacts"]);
            }
        });
        ui.add_space(12.);
        let active = matches!(s(b, "state"), "open" | "in_progress");
        if let Some(reason) = b["withdrawal"]["reason"].as_str() {
            ui.strong("Reason for withdrawal");
            ui.label(reason);
        }
        if active && (b["claimed_by"].is_null() || b["claimed_by"] == self.operator) {
            self.human_response_form(ui, &reference, b);
        }
        let key = reference.split_once('@').unwrap().0;
        let history: Vec<_> = self
            .events
            .iter()
            .rev()
            .filter(|e| {
                matches!(s(e, "kind"), "user_message" | "human_request_progress")
                    && e["body"]["human_request"]
                        .as_str()
                        .and_then(|r| r.split_once('@'))
                        .is_some_and(|(k, _)| k == key)
            })
            .cloned()
            .collect();
        if !history.is_empty() {
            ui.add_space(12.);
            section(ui, "Human updates", &history.len().to_string());
            for e in history {
                let report = &e["body"];
                ui.horizontal_wrapped(|ui| {
                    ui.strong(s(&e, "actor"));
                    let (status, c) = phase(s(report, "state"));
                    theme::badge(ui, status, c);
                    ui.weak(s(&e, "created_utc"));
                    if let Some(seq) = e["event"].as_i64().filter(|_| e["invocation"].is_null()) {
                        if ui.small_button("Delivery status")
                            .on_hover_text("Check whether the requester received this update in a decision")
                            .clicked()
                        {
                            self.open_message_receipt(ui.ctx(), seq);
                        }
                    }
                });
                ui.label(s(report, "text"));
                for file in report["attachments"].as_array().into_iter().flatten() {
                    if ui
                        .link(s(file, "name"))
                        .on_hover_text(s(file, "reference"))
                        .clicked()
                    {
                        self.open_named_resource(
                            ui.ctx(),
                            s(file, "reference").into(),
                            Some((s(report, "human_request").into(), s(file, "name").into())),
                        );
                    }
                }
                ui.separator();
            }
        }
        if ui.small_button("View requesting decision ↗").clicked() {
            self.open_resource(ui.ctx(), s(b, "decision").into());
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
        ui.add_space(8.);
        ui.separator();
        ui.strong(format!("Included packages · {}", references.len()));
        for reference in references {
            ui.push_id(reference, |ui| {
                let Some(&package) = summaries.get(reference) else {
                    if ui.link(reference).on_hover_text("Open this package revision").clicked() {
                        self.open_resource(ui.ctx(), reference.to_owned());
                    }
                    return;
                };
                ui.add_space(4.);
                ui.add(egui::Label::new(RichText::new(s(package, "title")).strong()).wrap());
                ui.horizontal_wrapped(|ui| {
                    let issuer = s(package, "owner");
                    ui.small(format!("Issued by {}", self.label(issuer))).on_hover_text(issuer);
                    let scope = s(package, "scope");
                    ui.small("Declared scope");
                    theme::badge(ui, ai_personas::neural::human_requests::declared_scope(scope),
                        if scope == "partial" { theme::ACCENT } else { PURPLE });
                    let revision = reference.rsplit_once('@').map_or_else(
                        || reference.to_owned(), |(_, n)| format!("Open revision {n}")
                    );
                    if ui.link(revision).on_hover_text(reference).clicked() {
                        self.open_resource(ui.ctx(), reference.to_owned());
                    }
                });
                ui.small("Supported use");
                ui.add(egui::Label::new(s(package, "supported_use")).wrap());
                if let Some(items) = package["remaining"].as_array().filter(|items| !items.is_empty()) {
                    ui.small("Remaining work");
                    for item in items.iter().filter_map(Value::as_str) {
                        ui.add(egui::Label::new(format!("• {item}")).wrap());
                    }
                } else {
                    ui.small("Remaining work · None listed");
                }
            });
        }
    }

    fn human_response_form(&mut self, ui: &mut Ui, reference: &str, b: &Value) {
        let key = reference.rsplit_once('@').unwrap().0.to_owned();
        let draft = self.inbox.drafts.entry(key).or_insert_with(|| Draft {
            basis: reference.into(),
            ..Default::default()
        });
        let mut response = None;
        draft.picker.poll(&mut draft.attachments);
        if draft.basis != reference
            && draft.note.is_empty()
            && !draft.picker.busy()
            && draft.attachments.is_empty()
        {
            draft.basis = reference.into();
        }
        if draft.basis != reference {
            ui.label("This request changed while you were writing.");
            if ui.button("Use the displayed revision").clicked() {
                draft.basis = reference.into();
            }
            return;
        }
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.strong("Your response");
            ui.add(
                egui::TextEdit::multiline(&mut draft.note)
                    .hint_text("Describe what you did, observed or still need…")
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            draft.picker.show(ui, &mut draft.attachments);
            ui.add_enabled_ui(!draft.picker.busy(), |ui| ui.horizontal_wrapped(|ui| {
                if b["state"] == "open" && ui.add(theme::primary("Take request")).clicked() {
                    response = Some(Response::Take);
                }
                if ui
                    .add_enabled(
                        !draft.note.trim().is_empty(),
                        egui::Button::new("Report completion"),
                    )
                    .clicked()
                {
                    response = Some(Response::Complete);
                }
                if ui
                    .add_enabled(
                        !draft.note.trim().is_empty(),
                        egui::Button::new("Unable to complete"),
                    )
                    .clicked()
                {
                    response = Some(Response::Unable);
                }
                if b["state"] == "in_progress"
                    && ui
                        .add_enabled(
                            !draft.note.trim().is_empty(),
                            egui::Button::new("Send progress"),
                        )
                        .clicked()
                {
                    response = Some(Response::Update);
                }
                if b["state"] == "in_progress" && ui.button("Release request").clicked() {
                    response = Some(Response::Release);
                }
            }));
        });
        if let Some(response) = response {
            let cmd = Command::HumanRespond {
                request: draft.basis.clone(),
                response,
                note: draft.note.clone(),
                files: draft.attachments.clone().into(),
            };
            self.command(ui.ctx(), cmd);
        }
    }
}
