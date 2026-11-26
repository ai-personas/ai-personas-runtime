//! Follow an unadopted response without putting the activity back to work.
mod inputs;
mod resources;
use super::*;

#[derive(Clone)]
struct Target {
    seq: i64,
    work: String,
    call: String,
    source: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Response,
    Input,
    Following,
}

type Report = Result<Value, String>;
pub(super) struct Reader {
    target: Target,
    back: Vec<(Target, Page)>,
    page: Page,
    away: Option<(String, Editor)>,
    pending: Option<mpsc::Receiver<Report>>,
    report: Option<Report>,
    open_after_check: Option<(String, String)>,
    input_changed: bool,
}

enum Action {
    Read(String),
    LaterInput(String, String),
    Follow(Target),
    Journal(String),
    Back,
    Refresh,
}

impl Reader {
    fn load(&mut self, ctx: &egui::Context, root: &std::path::Path) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.report = None;
        self.open_after_check = None;
        self.input_changed = false;
        let (ctx, root, t) = (ctx.clone(), root.to_owned(), self.target.clone());
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| ai_personas::neural::live::rejected_decision(&db, t.seq))
                .map_err(|e| e.to_string())
                .and_then(|r| {
                    if r["source"] != t.source || r["invocation"] != t.call || r["work"] != t.work {
                        Err("The recorded notice changed. Reopen it from the timeline.".into())
                    } else {
                        Ok(r)
                    }
                });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) -> Option<Action> {
        let rx = self.pending.as_ref()?;
        self.report = match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Decision reader stopped".into())),
            Err(mpsc::TryRecvError::Empty) => return None,
        };
        self.pending = None;
        let (call, input) = self.open_after_check.take()?;
        let report = self.report.as_ref()?.as_ref().ok()?;
        if report["following"]
            .as_array()?
            .iter()
            .any(|r| r["invocation"] == call && r["input"] == input)
        {
            Some(Action::Read(input))
        } else {
            self.input_changed = true;
            None
        }
    }
    fn draw(&mut self, ui: &mut Ui, app: &App) -> Option<Action> {
        let seq = self.target.seq;
        let mut action = None;
        if let Some((t, _)) = self.back.last() {
            if ui.link(format!("← Response at #{}", t.seq)).clicked() {
                return Some(Action::Back);
            }
        }
        let Some(result) = &self.report else {
            ui.spinner();
            return None;
        };
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                ui.strong("Response record unavailable");
                ui.label(e);
                return ui
                    .button("Check records again")
                    .clicked()
                    .then_some(Action::Refresh);
            }
        };
        let person = app.label(s(r, "persona"));
        eyebrow(ui, &format!("ACTIVITY RECORD   /   EVENT #{seq}"));
        ui.heading(format!("{person}’s rejected response"));
        ui.label("The provider returned a response. The runtime did not adopt it.");
        ui.add_space(6.);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.page, Page::Response, "Proposal");
            ui.selectable_value(&mut self.page, Page::Input, "Input & citations");
            ui.selectable_value(&mut self.page, Page::Following, "What followed");
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt(("refusal-page",seq,self.page as u8)).auto_shrink([false,false]).show(ui, |ui| {
            match self.page {
                Page::Response => {
                    let p = theme::palette(ui);
                    egui::Frame::new().fill(p.header).stroke(Stroke::new(1.,p.border)).corner_radius(7).inner_margin(10).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        eyebrow(ui,"Runtime’s reason"); ui.label(s(r,"reason"));
                    });
                    ui.add_space(8.);
                    eyebrow(ui,"Proposed · not applied");
                    if r["proposed"].is_object() {
                        choices(ui, &r["proposed"], true, s(r,"invocation"));
                        if r["proposed"]["named_invocation"] != r["invocation"] {
                            ui.small("The response does not name the call identified by its provider receipt.");
                        }
                    }
                    else { ui.label("No unambiguous response object. Inspect the original bytes below."); }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        for (label, key) in [("Original response ↗","response"),("Provider receipt ↗","receipt")] {
                            if ui.link(label).clicked() { action=Some(Action::Read(s(&r["records"],key).into())); }
                        }
                        if ui.link("Notice body ↗").clicked() { action=Some(Action::Read(s(r,"source").into())); }
                    });
                }
                Page::Input => {
                    eyebrow(ui,"Experiences cited for character maintenance");
                    if let Some(rows)=r["cited_for_character"].as_array() {
                        if rows.is_empty() { ui.weak("The response cited none."); }
                        for row in rows {
                            ui.horizontal_wrapped(|ui| {
                                address(ui,s(row,"reference"));
                                ui.label(match s(row,"observation") {
                                    "previous_choice" => "Previous choice", "record" => "Supplied record", "incoming_event" => "Incoming event",
                                    "recalled_pair" => "Recalled pair", "action_with_return" => "Action with its return", "before_action" => "Carried with appraisal",
                                    _ if row["in_previous_judgment"] == true => "Earlier citation · no observation here",
                                    _ => "Not supplied as an observation",
                                });
                            });
                        }
                    } else {ui.weak("No citation list in the response.");}
                    ui.add_space(7.); ui.separator();
                    let count=r["incoming_count"].as_u64().map_or_else(|| "unknown".to_owned(),|n|n.to_string());
                    ui.strong(format!("In this input · {count} inbox entries"));
                    if let Some(rows)=r["selected"].as_array() {
                        ui.label(format!("Selected resources: {}",rows.len()));
                        for row in rows {
                            ui.horizontal_wrapped(|ui| {
                                if let Some(path)=row["path"].as_str() {ui.strong(path).on_hover_text(s(row,"reference"));} else {address(ui,s(row,"reference"));}
                                if let (Some(a),Some(b),Some(n))=(row["range"][0].as_u64(),row["range"][1].as_u64(),row["bytes"].as_u64()) {
                                    let form=title(s(row,"form"));
                                    if a == 0 && b == n { ui.weak(format!("{form} · all {n} bytes")); }
                                    else { ui.weak(format!("{form} · bytes {a}–{b} of {n}")).on_hover_text("End exclusive"); }
                                } else { ui.weak(match s(row,"form") {
                                    "binary_metadata" => "Binary file · metadata only", "recovery_metadata" => "Withheld in context recovery",
                                    "pointer" => "Pointer to another input slot", "record" => "Record", _ => "Other supplied view",
                                }); }
                            });
                        }
                    } else {ui.weak("Selection register unavailable.");}
                    ui.add_space(6.); ui.small("An event address in a note, or a file named by that event, is not the event itself.");
                    if ui.link("Open frozen input ↗").clicked() {action=Some(Action::Read(s(&r["records"],"input").into()));}
                }
                Page::Following => {
                    ui.small("Later responses from this persona in this activity, through the next accepted choice. Receipt means the full notice appeared in that input’s inbox.");
                    if self.input_changed { ui.label("The later input could not be verified again. Nothing was opened."); }
                    for row in r["following"].as_array().into_iter().flatten() {
                        ui.add_space(7.);
                        if let Some(gap)=row["unavailable"].as_str() { ui.strong("Later response unavailable"); ui.label(gap); continue; }
                        let accepted=row["state"]=="accepted";
                        ui.horizontal_wrapped(|ui| {
                            theme::badge(ui,if accepted {"Accepted choice"} else {"Rejected again"},if accepted {TEAL} else {theme::palette(ui).accent});
                            ui.label(if row["received_notice"]==true { format!("Received notice #{seq}") } else { format!("No full notice #{seq} in its inbox") });
                        });
                        choices(ui,&row["choice"],false,s(row,"invocation"));
                        if let Some(n)=row["notice_event"].as_i64() {
                            if ui.link(format!("Inspect rejection #{n} →")).clicked() { action=Some(Action::Follow(Target{seq:n,work:self.target.work.clone(),call:s(row,"invocation").into(),source:s(row,"notice_source").into()})); }
                        } else if let Some(choice)=row["decision"].as_str() { if ui.link("Read accepted choice →").clicked() {action=Some(Action::Journal(choice.into()));} }
                        if inputs::show(ui,row,seq) { action=Some(Action::LaterInput(s(row,"invocation").into(),s(row,"input").into())); }
                        ui.separator();
                    }
                    if r["following"].as_array().is_some_and(Vec::is_empty) {ui.label("No later accepted or rejected response recorded for this participation.");}
                    ui.small("Acceptance applies to that later choice, not to the refused proposal. It does not establish that a tool succeeded.");
                    if ui.small_button("Refresh later responses").clicked() { action=Some(Action::Refresh); }
                }
            }
        });
        action
    }
}

fn choices(ui: &mut Ui, row: &Value, detail: bool, invocation: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(title(
            row["status"].as_str().unwrap_or("judgment_unavailable"),
        ));
        ui.label(format!(
            "Next: {}",
            row["continuation"].as_str().unwrap_or("unavailable")
        ));
    });
    if let Some(ops) = row["operations"].as_array() {
        if ops.is_empty() {
            ui.weak("No operations listed");
        } else {
            ui.horizontal_wrapped(|ui| {
                for op in ops {
                    ui.code(op["tool"].as_str().unwrap_or("(unnamed tool)"))
                        .on_hover_text(s(op, "id"));
                }
            });
        }
    } else {
        ui.weak("Operation list unavailable");
    }
    resources::show(ui, row, invocation);
    if detail {
        if let Some(text) = row["next_step"].as_str() {
            ui.add_space(4.);
            ui.label(text);
        }
    }
}
fn address(ui: &mut Ui, reference: &str) {
    let brief = if reference.len() > 42 {
        format!(
            "{}…{}",
            reference.chars().take(26).collect::<String>(),
            reference
                .chars()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        )
    } else {
        reference.to_owned()
    };
    ui.code(brief).on_hover_text(reference);
}

impl App {
    pub(super) fn open_rejection(&mut self, ctx: &egui::Context, seq: i64) {
        let Ok(i) = self
            .events
            .binary_search_by_key(&seq, |e| e["event"].as_i64().unwrap_or_default())
        else {
            return;
        };
        let e = &self.events[i];
        let target = Target {
            seq,
            work: s(e, "work").into(),
            call: s(e, "invocation").into(),
            source: format!("object:{}", s(e, "digest")),
        };
        let mut reader = Reader {
            target,
            back: vec![],
            page: Page::Response,
            report: None,
            pending: None,
            away: None,
            open_after_check: None,
            input_changed: false,
        };
        reader.load(ctx, &self.root);
        self.rejection = Some(reader);
    }
    pub(super) fn rejection_return(&mut self, ui: &mut Ui) {
        let Some(reader) = self.rejection.as_mut() else {
            return;
        };
        let Some((reference, editor)) = &reader.away else {
            return;
        };
        let journal =
            self.layout.editor == Editor::Studio && self.live_board.holding_choice(reference);
        let record =
            self.layout.editor == Editor::Artifacts && self.selected_reference == *reference;
        if !(journal || record) || self.work != reader.target.work {
            self.rejection = None;
            return;
        }
        if ui
            .link(format!("← Response at #{}", reader.target.seq))
            .clicked()
        {
            self.layout.editor = *editor;
            if journal {
                self.live_board
                    .show_event(&reader.target.work, reader.target.seq);
            }
            reader.away = None;
            reader.load(ui.ctx(), &self.root);
        }
    }
    pub(super) fn rejection_window(&mut self, ctx: &egui::Context) {
        if self.rejection.as_ref().is_none_or(|r| r.away.is_some()) {
            return;
        }
        let mut r = self.rejection.take().unwrap();
        let checked = r.poll();
        let mut open = true;
        let mut action = None;
        let viewport = ctx.content_rect().shrink(6.);
        let available =
            (viewport.size() - theme::titled_window_chrome(ctx)).max(egui::vec2(1., 1.));
        egui::Window::new("Response record")
            .id(egui::Id::new("refused-response"))
            .open(&mut open)
            .constrain_to(viewport)
            .default_size(egui::vec2(540., 430.).min(available))
            .min_size(egui::vec2(210., 150.).min(available))
            .max_size(available)
            .show(ctx, |ui| {
                action = r.draw(ui, self);
            });
        if !open {
            return;
        }
        match action.or(checked) {
            Some(Action::LaterInput(call, input)) => {
                r.load(ctx, &self.root);
                r.open_after_check = Some((call, input));
            }
            Some(Action::Read(reference)) => {
                r.away = Some((reference.clone(), self.layout.editor));
                self.open_resource(ctx, reference);
            }
            Some(Action::Journal(reference)) => {
                r.away = Some((reference.clone(), self.layout.editor));
                self.open_choice_or_record(ctx, reference);
            }
            Some(Action::Follow(t)) => {
                r.back.push((std::mem::replace(&mut r.target, t), r.page));
                r.page = Page::Response;
                r.load(ctx, &self.root);
            }
            Some(Action::Back) => {
                if let Some((t, p)) = r.back.pop() {
                    r.target = t;
                    r.page = p;
                    r.load(ctx, &self.root);
                }
            }
            Some(Action::Refresh) => r.load(ctx, &self.root),
            None => (),
        }
        self.rejection = Some(r);
    }
}
