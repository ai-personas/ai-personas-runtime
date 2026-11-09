//! Desktop view for the indexed runtime. Uses the same Rust entry points as the
//! terminal; no desktop-only orchestration, model calls or state translation.
use ai_personas::neural::{
    cli::{self, Command},
    context,
    store::{self, Store},
};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

enum Update {
    Status(Value, Vec<Value>),
    Done(Result<Value, String>),
    RunDone(Result<Value, String>),
    Read(Result<Value, String>),
}
pub struct App {
    root: PathBuf,
    status: Value,
    events: Vec<Value>,
    cursor: i64,
    tx: mpsc::Sender<Update>,
    rx: mpsc::Receiver<Update>,
    polling: bool,
    last_poll: Instant,
    running: bool,
    busy: bool,
    error: Option<String>,
    notice: String,
    need: String,
    invited: std::collections::BTreeSet<String>,
    work: String,
    persona: String,
    message: String,
    reference: String,
    resource: Value,
    tab: usize,
    closing: bool,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> Self {
        super::theme::install_fonts(&cc.egui_ctx);
        super::theme::apply(&cc.egui_ctx, true, 1.0);
        let (tx, rx) = mpsc::channel();
        Self {
            root,
            status: Value::Null,
            events: vec![],
            cursor: 0,
            tx,
            rx,
            polling: false,
            last_poll: Instant::now() - Duration::from_secs(5),
            running: false,
            busy: false,
            error: None,
            notice: String::new(),
            need: String::new(),
            invited: Default::default(),
            work: String::new(),
            persona: String::new(),
            message: String::new(),
            reference: String::new(),
            resource: Value::Null,
            tab: 0,
            closing: false,
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if self.polling || self.last_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.polling = true;
        self.last_poll = Instant::now();
        let root = self.root.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let after = self.cursor;
        std::thread::spawn(move || {
            let result = (|| -> ai_personas::neural::Result<_> {
                let mut store = Store::open(&root)?;
                let status = store.status()?;
                let mut events = Vec::new();
                for work in store::list(&store.db, "work")? {
                    events.extend(store::events(&store.db, &work.reference.id, "user", after)?);
                }
                events.sort_by_key(|v| v["event"].as_i64());
                Ok((status, events))
            })();
            let update = match result {
                Ok((status, events)) => Update::Status(status, events),
                Err(e) => Update::Done(Err(e.to_string())),
            };
            let _ = tx.send(update);
            ctx.request_repaint();
        });
    }
    fn command(&mut self, ctx: &egui::Context, command: Command) {
        let run = matches!(command, Command::Run { .. });
        if run {
            self.running = true;
        } else {
            self.busy = true;
        }
        self.error = None;
        let root = self.root.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|e| e.to_string())
                .and_then(|rt| {
                    rt.block_on(cli::execute(&root, "user", command))
                        .map_err(|e| e.to_string())
                });
            let _ = tx.send(if run {
                Update::RunDone(result)
            } else {
                Update::Done(result)
            });
            ctx.request_repaint();
        });
    }
    fn drain(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::Status(status, events) => {
                    self.polling = false;
                    self.status = status;
                    for event in events {
                        self.cursor = self.cursor.max(event["event"].as_i64().unwrap_or(0));
                        self.events.push(event);
                    }
                    if self.work.is_empty() {
                        self.work = self.status["work"][0]["body"]["id"]
                            .as_str()
                            .unwrap_or("")
                            .into();
                    }
                    if self.persona.is_empty() {
                        self.persona = self.status["personas"][0]["body"]["id"]
                            .as_str()
                            .unwrap_or("")
                            .into();
                    }
                }
                Update::Read(result) => match result {
                    Ok(v) => self.resource = v,
                    Err(e) => self.error = Some(e),
                },
                Update::Done(result) | Update::RunDone(result) => {
                    self.busy = false;
                    self.polling = false;
                    match result {
                        Ok(value) => {
                            if value.get("invocations").is_some() {
                                self.running = false;
                                self.status = value;
                            } else {
                                self.notice = serde_json::to_string(&value).unwrap_or_default();
                            }
                        }
                        Err(error) => {
                            self.running = false;
                            self.error = Some(error);
                        }
                    }
                    self.last_poll = Instant::now() - Duration::from_secs(3);
                }
            }
        }
    }
    fn conversation(&mut self, ui: &mut egui::Ui) {
        for event in &self.events {
            if event["kind"] == "reply"
                || event["kind"] == "message"
                || event["kind"] == "delivery"
                || event["kind"] == "initialization_note"
            {
                ui.group(|ui| {
                    ui.label(RichText::new(event["actor"].as_str().unwrap_or("Persona")).strong());
                    if event["kind"] == "initialization_note" {
                        ui.label("Initialization note — character model not yet active");
                    }
                    if let Some(text) = event["body"]["text"].as_str() {
                        ui.label(text);
                    } else if let Some(text) = event["body"]["record"]["title"].as_str() {
                        ui.label(text);
                        ui.label(event["body"]["reference"].as_str().unwrap_or(""));
                    }
                });
                ui.add_space(6.0);
            }
        }
        for delivery in self.status["deliveries"].as_array().into_iter().flatten() {
            let body = &delivery["body"];
            ui.collapsing(body["title"].as_str().unwrap_or("Delivery"), |ui| {
                ui.label(format!(
                    "{} · {}",
                    body["scope"].as_str().unwrap_or(""),
                    body["owner"].as_str().unwrap_or("")
                ));
                ui.label(body["supported_use"].as_str().unwrap_or(""));
                for artifact in body["artifacts"].as_array().into_iter().flatten() {
                    if let Some(reference) = artifact.as_str() {
                        if ui.button(reference).clicked() {
                            self.reference = reference.into();
                            self.tab = 2;
                        }
                    }
                }
                for gap in body["remaining"].as_array().into_iter().flatten() {
                    ui.label(format!("Remaining: {}", gap.as_str().unwrap_or("")));
                }
            });
        }
    }
    fn personas(&mut self, ui: &mut egui::Ui) {
        for person in self.status["personas"].as_array().into_iter().flatten() {
            let body = &person["body"];
            ui.group(|ui| {
                ui.heading(body["label"].as_str().unwrap_or("Persona"));
                ui.label(format!(
                    "{} · {} · {}",
                    body["lifecycle"].as_str().unwrap_or(""),
                    body["provider"]["model"].as_str().unwrap_or(""),
                    body["provider"]["effort"].as_str().unwrap_or("")
                ));
                ui.label(format!("Character revision {}", body["epoch"]));
                if let Some(model) = body["model"].as_str() {
                    if ui.button("Inspect character parameters").clicked() {
                        self.reference = format!("object:{model}");
                        self.tab = 2;
                    }
                }
                for participation in self.status["participation"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if participation["body"]["persona"] == body["id"] {
                        ui.label(format!(
                            "{} · {} accepted decisions",
                            participation["body"]["continuation"]["kind"]
                                .as_str()
                                .unwrap_or(""),
                            participation["body"]["turn"]
                        ));
                        if let Some(reason) =
                            participation["body"]["last_judgment"]["reason"].as_str()
                        {
                            ui.label(reason);
                        }
                    }
                }
            });
            ui.add_space(8.0);
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        self.poll(ctx);
        ctx.request_repaint_after(Duration::from_millis(500));
        if ctx.input(|i| i.viewport().close_requested()) && self.running {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.closing {
                self.closing = true;
                self.command(
                    ctx,
                    Command::Control {
                        scope: "global".into(),
                        state: "paused".into(),
                        reason: "Desktop window closed by user".into(),
                    },
                );
            }
        }
        if self.closing && !self.running {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::TopBottomPanel::top("editable_header").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Personas");
                let in_flight = self.status["invocations"]["dispatched"]
                    .as_u64()
                    .unwrap_or(0);
                if in_flight > 0 {
                    ui.label(format!("{in_flight} model calls in progress"));
                }
                if ui
                    .add_enabled(
                        !self.running && !self.busy && in_flight == 0,
                        egui::Button::new("Continue eligible work"),
                    )
                    .clicked()
                {
                    self.command(ctx, Command::Run { watch: false });
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Pause all"))
                    .clicked()
                {
                    self.command(
                        ctx,
                        Command::Control {
                            scope: "global".into(),
                            state: "paused".into(),
                            reason: "User paused the desktop workspace".into(),
                        },
                    );
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Enable scheduling"))
                    .clicked()
                {
                    self.command(
                        ctx,
                        Command::Control {
                            scope: "global".into(),
                            state: "running".into(),
                            reason: "User enabled scheduling".into(),
                        },
                    );
                }
            });
            ui.small(self.root.display().to_string());
            for control in self.status["controls"].as_array().into_iter().flatten() {
                if control["state"] != "running" {
                    ui.label(format!(
                        "{}: {} — {}",
                        control["scope"].as_str().unwrap_or(""),
                        control["state"].as_str().unwrap_or(""),
                        control["reason"].as_str().unwrap_or("")
                    ));
                }
            }
            ui.horizontal(|ui| {
                for (i, label) in [
                    "Conversation & deliveries",
                    "Personas",
                    "Resources",
                    "New task",
                ]
                .iter()
                .enumerate()
                {
                    ui.selectable_value(&mut self.tab, i, *label);
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            if !self.notice.is_empty() {
                ui.label(&self.notice);
            }
            if self.running {
                ui.label("Personas are working. Each decides when to continue, wait or stop.");
            }
        });
        egui::TopBottomPanel::bottom("editable_feedback").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let selected = self.status["work"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|w| w["body"]["id"] == self.work)
                    .and_then(|w| w["body"]["original_need"].as_str())
                    .unwrap_or("Choose task");
                egui::ComboBox::from_id_salt("work")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for w in self.status["work"].as_array().into_iter().flatten() {
                            if let Some(id) = w["body"]["id"].as_str() {
                                ui.selectable_value(
                                    &mut self.work,
                                    id.into(),
                                    w["body"]["original_need"].as_str().unwrap_or(id),
                                );
                            }
                        }
                    });
                egui::ComboBox::from_id_salt("persona")
                    .selected_text(&self.persona)
                    .show_ui(ui, |ui| {
                        for p in self.status["personas"].as_array().into_iter().flatten() {
                            if let Some(id) = p["body"]["id"].as_str() {
                                ui.selectable_value(
                                    &mut self.persona,
                                    id.into(),
                                    p["body"]["label"].as_str().unwrap_or(id),
                                );
                            }
                        }
                    });
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.message)
                    .hint_text("Feedback, clarification or a reason to resume")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                let enabled = !self.busy
                    && !self.message.trim().is_empty()
                    && !self.work.is_empty()
                    && !self.persona.is_empty();
                if ui
                    .add_enabled(enabled, egui::Button::new("Send message"))
                    .clicked()
                {
                    let text = std::mem::take(&mut self.message);
                    self.command(
                        ctx,
                        Command::Message {
                            work: self.work.clone(),
                            persona: self.persona.clone(),
                            text,
                        },
                    );
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Resume participant"))
                    .clicked()
                {
                    let reason = std::mem::take(&mut self.message);
                    self.command(
                        ctx,
                        Command::Resume {
                            work: self.work.clone(),
                            persona: self.persona.clone(),
                            reason,
                        },
                    );
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Amend task"))
                    .clicked()
                {
                    let text = std::mem::take(&mut self.message);
                    self.command(
                        ctx,
                        Command::Amend {
                            work: self.work.clone(),
                            text,
                        },
                    );
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                0 => self.conversation(ui),
                1 => self.personas(ui),
                2 => {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.reference);
                        if ui.button("Read exact resource").clicked() {
                            let root = self.root.clone();
                            let reference = self.reference.clone();
                            let tx = self.tx.clone();
                            let ctx = ctx.clone();
                            std::thread::spawn(move || {
                                let result = Store::open(&root)
                                    .and_then(|s| context::read(&s.db, &reference, None, None))
                                    .map_err(|e| e.to_string());
                                let _ = tx.send(Update::Read(result));
                                ctx.request_repaint();
                            });
                        }
                    });
                    if let Some(text) = self.resource["text"].as_str() {
                        ui.add(
                            egui::TextEdit::multiline(&mut text.to_owned())
                                .code_editor()
                                .desired_width(f32::INFINITY),
                        );
                    } else {
                        ui.label(serde_json::to_string_pretty(&self.resource).unwrap_or_default());
                    }
                    if ui.button("Export delivered files…").clicked() {
                        if let Some(parent) = rfd::FileDialog::new().pick_folder() {
                            self.command(
                                ctx,
                                Command::Export {
                                    directory: parent.join(format!(
                                        "personas-delivery-{}",
                                        ai_personas::neural::now()
                                    )),
                                    work: if self.work.is_empty() {
                                        None
                                    } else {
                                        Some(self.work.clone())
                                    },
                                },
                            );
                        }
                    }
                }
                _ => {
                    ui.heading("What would you like to work on?");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.need)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    ui.label("Invite personas. Each chooses whether and how to contribute.");
                    for p in self.status["personas"].as_array().into_iter().flatten() {
                        if let Some(id) = p["body"]["id"].as_str() {
                            let mut selected = self.invited.contains(id);
                            if ui
                                .checkbox(&mut selected, p["body"]["label"].as_str().unwrap_or(id))
                                .changed()
                            {
                                if selected {
                                    self.invited.insert(id.into());
                                } else {
                                    self.invited.remove(id);
                                }
                            }
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.busy && !self.need.trim().is_empty() && !self.invited.is_empty(),
                            egui::Button::new("Create task"),
                        )
                        .clicked()
                    {
                        let need = std::mem::take(&mut self.need);
                        self.command(
                            ctx,
                            Command::Request {
                                need,
                                invite: self.invited.iter().cloned().collect(),
                            },
                        );
                    }
                }
            });
        });
    }
}
