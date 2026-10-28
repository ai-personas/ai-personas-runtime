use super::{
    backend::{self, Action, Appearance, Event, Input, PersonaDraft, Preferences, Setup, View},
    journal::{self, CodexPaths},
    theme::{self, Icon},
};
use eframe::egui::{self, Color32, RichText, Ui};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

#[path = "browser.rs"]
mod browser;
#[path = "studio.rs"]
mod studio;

#[derive(Clone)]
enum Folder {
    Progress,
    Helpers,
    Workspace,
    Runtime,
    Codex,
    HostHome,
    CodexHome,
    Export(String),
}
enum Dialog {
    Task,
    Feedback,
    Answer(Value),
    Amend,
    Development,
    Account,
    OpenWorkspace,
}
#[derive(Clone, Copy, PartialEq)]
enum LibraryScope {
    All,
    Recent,
    Unfinished,
}
pub struct App {
    _signals: Option<tokio::runtime::Runtime>,
    tx: mpsc::Sender<Input>,
    rx: mpsc::Receiver<Event>,
    view: View,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    progress: String,
    helper_dir: String,
    workspace: String,
    existing_runtime: String,
    operator: String,
    codex: CodexPaths,
    native: bool,
    model: String,
    effort: String,
    manual: bool,
    personas: Vec<PersonaDraft>,
    setup_step: usize,
    dialog: Option<Dialog>,
    text: String,
    basis: String,
    feedback_persona: String,
    replaced: Vec<String>,
    amend_scope: String,
    tab: usize,
    focus_files: bool,
    file_viewer: super::artifacts::Viewer,
    preferences: Preferences,
    show_preferences: bool,
    auth_url: Option<String>,
    auth_code: Option<String>,
    picker_tx: mpsc::Sender<(Folder, Option<PathBuf>)>,
    picker_rx: mpsc::Receiver<(Folder, Option<PathBuf>)>,
    picker_busy: bool,
    closing: bool,
    allow_close: bool,
    running_since: Option<Instant>,
    choose_progress: bool,
    choices: Vec<journal::SavedWorkspace>,
    workspace_filter: String,
    library_scope: LibraryScope,
    browse_selected: Option<PathBuf>,
    task_filter: String,
    inspected_persona: Option<String>,
    show_session_strip: bool,
    initial_discovery: bool,
    initialized: bool,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        progress: Option<PathBuf>,
        helpers: Option<PathBuf>,
    ) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let (tx, rx) = backend::start(cc.egui_ctx.clone());
        // A terminal interrupt uses the same durable pause-and-close path as
        // the window button. Runtime work lives in independently owned helpers.
        let signals = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .ok();
        if let Some(runtime) = &signals {
            let tx = tx.clone();
            runtime.spawn(async move {
                use tokio::signal::unix::{signal, SignalKind};
                if let (Ok(mut interrupt), Ok(mut terminate)) = (
                    signal(SignalKind::interrupt()),
                    signal(SignalKind::terminate()),
                ) {
                    tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
                    let _ = tx.send(Input::Action(Action::Close));
                }
            });
        }
        let (picker_tx, picker_rx) = mpsc::channel();
        let choices = journal::candidates();
        let existing: Vec<_> = choices
            .iter()
            .filter(|p| p.join("progress.json").exists())
            .collect();
        let choose_progress =
            progress.is_none() && std::env::var_os("PERSONAS_LAUNCHER_HOME").is_none();
        let root = progress
            .or_else(|| existing.first().map(|p| (*p).clone()))
            .or_else(|| choices.first().cloned())
            .unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        let codex = super::helpers::codex(None).unwrap_or(CodexPaths {
            executable: String::new(),
            host_home: home.clone(),
            codex_home: format!("{home}/.codex"),
        });
        let mut app = Self {
            _signals: signals,
            tx,
            rx,
            view: View::default(),
            busy: false,
            error: None,
            notice: None,
            progress: root.to_string_lossy().into_owned(),
            helper_dir: helpers
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            workspace: root.join("workspace").to_string_lossy().into_owned(),
            existing_runtime: String::new(),
            operator: "local-operator".into(),
            native: codex.executable.is_empty(),
            codex,
            model: String::new(),
            effort: String::new(),
            manual: false,
            personas: vec![
                PersonaDraft {
                    name: "Persona 1".into(),
                    ..Default::default()
                },
                PersonaDraft {
                    name: "Persona 2".into(),
                    ..Default::default()
                },
            ],
            setup_step: 0,
            dialog: None,
            text: String::new(),
            basis: String::new(),
            feedback_persona: String::new(),
            replaced: vec![],
            amend_scope: String::new(),
            tab: 0,
            focus_files: false,
            file_viewer: super::artifacts::Viewer::new(&cc.egui_ctx, cc.gl.as_ref()),
            preferences: Preferences::default(),
            show_preferences: false,
            auth_url: None,
            auth_code: None,
            picker_tx,
            picker_rx,
            picker_busy: false,
            closing: false,
            allow_close: false,
            running_since: None,
            choose_progress,
            choices: vec![],
            workspace_filter: String::new(),
            library_scope: LibraryScope::All,
            browse_selected: None,
            task_filter: String::new(),
            inspected_persona: None,
            show_session_strip: true,
            initial_discovery: choose_progress,
            initialized: false,
        };
        app.apply_style(&cc.egui_ctx);
        if choose_progress {
            app.discover();
        } else if !app.progress.is_empty() {
            app.open();
        }
        app
    }
    fn send(&mut self, a: Action) {
        self.busy = true;
        self.error = None;
        let _ = self.tx.send(Input::Action(a));
    }
    fn open(&mut self) {
        self.choose_progress = false;
        self.initialized = false;
        self.view = View::default();
        self.notice = None;
        self.dialog = None;
        self.tab = 0;
        self.task_filter.clear();
        self.inspected_persona = None;
        self.setup_step = 0;
        self.workspace = PathBuf::from(&self.progress)
            .join("workspace")
            .to_string_lossy()
            .into_owned();
        self.auth_url = None;
        self.auth_code = None;
        self.model.clear();
        self.effort.clear();
        self.manual = false;
        self.send(Action::Open {
            root: journal::expand(&PathBuf::from(&self.progress)),
            helpers: (!self.helper_dir.is_empty())
                .then(|| journal::expand(&PathBuf::from(&self.helper_dir))),
        });
    }
    fn discover(&mut self) {
        self.choose_progress = true;
        if self.view.progress.is_some() {
            self.browse_selected = Some(self.view.root.clone());
        }
        self.send(Action::Discover(journal::expand(&PathBuf::from(
            &self.progress,
        ))));
    }
    fn apply_style(&self, ctx: &egui::Context) {
        theme::apply(
            ctx,
            matches!(self.preferences.appearance, Appearance::Dark),
            self.preferences.zoom,
        );
    }
    fn picker(&mut self, kind: Folder, file: bool) {
        if self.picker_busy {
            return;
        }
        self.picker_busy = true;
        let tx = self.picker_tx.clone();
        let start = PathBuf::from(&self.workspace);
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_directory(start);
            let result = if file {
                dialog.pick_file()
            } else {
                dialog.pick_folder()
            };
            let _ = tx.send((kind, result));
        });
    }
    fn events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Workspaces(choices) => {
                    self.busy = false;
                    self.choices = choices;
                    if !self
                        .choices
                        .iter()
                        .any(|w| Some(&w.root) == self.browse_selected.as_ref())
                    {
                        self.browse_selected = self.choices.first().map(|w| w.root.clone());
                    }
                    if self.initial_discovery {
                        self.initial_discovery = false;
                        let ready: Vec<_> = self.choices.iter().filter(|w| w.ready).collect();
                        if ready.len() == 1 {
                            self.progress = ready[0].root.to_string_lossy().into_owned();
                            self.open();
                        } else if self.choices.is_empty() {
                            self.open();
                        } else if ready.is_empty() {
                            self.library_scope = LibraryScope::Unfinished;
                        }
                    }
                }
                Event::View(view) => {
                    if !self.initialized {
                        if let Some(p) = &view.progress {
                            self.operator = p
                                .operator
                                .clone()
                                .unwrap_or_else(|| "local-operator".into());
                            if let Some(c) = &view.codex {
                                self.codex = c.clone();
                            }
                            self.native = view.native;
                            self.progress = view.root.to_string_lossy().into_owned();
                            if let Some(model) = view.saved_settings["model"].as_str() {
                                self.model = model.into();
                                self.effort = s(&view.saved_settings, "effort").into();
                            }
                            if let Some(seeds) = view.preview["persona_seeds"].as_object() {
                                self.personas = seeds
                                    .values()
                                    .map(|p| PersonaDraft {
                                        name: s(p, "label").into(),
                                        profile: s(p, "text").into(),
                                        model: s(p, "model").into(),
                                        effort: s(p, "effort").into(),
                                    })
                                    .collect();
                                if let Some(path) =
                                    view.preview["setup"]["working_directory"].as_str()
                                {
                                    self.workspace = path.into();
                                }
                            }
                            if self.workspace.is_empty() {
                                self.workspace =
                                    view.root.join("workspace").to_string_lossy().into_owned();
                            }
                            self.initialized = true;
                        }
                    }
                    if self.model.is_empty() {
                        if let Some(m) = models(&view.models)
                            .iter()
                            .find(|m| m["is_default"] == true)
                            .or_else(|| models(&view.models).first())
                        {
                            self.model = s(m, "slug").into();
                            self.effort = m["default_effort"].as_str().unwrap_or("medium").into();
                        }
                    }
                    if self.view.running.is_none() && view.running.is_some() {
                        self.running_since = Some(Instant::now());
                    }
                    if view.running.is_none() {
                        self.running_since = None;
                    }
                    if !view.login {
                        self.auth_url = None;
                        self.auth_code = None;
                    }
                    if self.view.amendment.is_null() && !view.amendment.is_null() {
                        self.amend_scope =
                            serde_json::to_string_pretty(&view.amendment["current_scope"])
                                .unwrap_or_default();
                        self.text = s(&view.amendment["current_scope"], "request").into();
                    }
                    self.view = *view;
                    self.closing = self.view.closing;
                }
                Event::Error(e) => {
                    self.error = Some(e);
                }
                Event::Notice(n) => {
                    self.notice = Some(n);
                }
                Event::Idle => self.busy = false,
                Event::Authorization { url, code } => {
                    ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
                    self.auth_url = Some(url);
                    self.auth_code = code;
                }
                Event::SafeToClose => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Event::Preferences(p) => {
                    self.preferences = p;
                    self.apply_style(ctx);
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        self.preferences.width.clamp(780.0, 2400.0),
                        self.preferences.height.clamp(560.0, 1600.0),
                    )));
                }
            }
        }
        while let Ok((kind, path)) = self.picker_rx.try_recv() {
            self.picker_busy = false;
            if let Some(p) = path {
                let text = p.to_string_lossy().into_owned();
                match kind {
                    Folder::Progress => {
                        self.progress = text;
                    }
                    Folder::Helpers => self.helper_dir = text,
                    Folder::Workspace => self.workspace = text,
                    Folder::Runtime => self.existing_runtime = text,
                    Folder::Codex => self.codex.executable = text,
                    Folder::HostHome => self.codex.host_home = text,
                    Folder::CodexHome => self.codex.codex_home = text,
                    Folder::Export(work) => self.send(Action::Export { work, parent: p }),
                }
            }
        }
    }
    fn banner(&mut self, ui: &mut Ui) {
        if let Some(error) = self.error.clone() {
            egui::Frame::group(ui.style())
                .fill(if ui.visuals().dark_mode {
                    Color32::from_rgb(70, 32, 34)
                } else {
                    Color32::from_rgb(255, 238, 234)
                })
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(error);
                        if ui.small_button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                });
        }
        if let Some(notice) = self.notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(notice).small());
                if ui.small_button("Dismiss").clicked() {
                    self.notice = None;
                }
            });
        }
    }
    fn startup(&mut self, ui: &mut Ui) {
        theme::header(ui, Icon::Workspaces, "Open workspace", "");
        self.banner(ui);
        ui.add_space(16.0);
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Opening workspace…");
            });
            ui.add(egui::Label::new(RichText::new(&self.progress).small().weak()).wrap());
            return;
        }
        ui.label("Choose a saved progress folder or an empty folder for a new workspace.");
        self.locations(ui);
        if ui
            .add_enabled(
                !self.busy && !self.progress.is_empty(),
                egui::Button::new("Open / Retry"),
            )
            .clicked()
        {
            self.choose_progress = false;
            self.open();
        }
    }
    fn locations(&mut self, ui: &mut Ui) {
        field(ui, "Saved progress folder", &mut self.progress, false);
        if ui.button("Choose progress folder…").clicked() {
            self.picker(Folder::Progress, false);
        }
        ui.collapsing("Runtime installation", |ui| {
            field(
                ui,
                "Helper directory (leave empty for automatic discovery)",
                &mut self.helper_dir,
                false,
            );
            if ui.button("Choose helpers…").clicked() {
                self.picker(Folder::Helpers, false);
            }
        });
    }
    fn account(&mut self, ui: &mut Ui) {
        ui.heading("Connect your account");
        ui.label("Use your existing Codex ChatGPT login, or sign in here.");
        ui.add_enabled_ui(
            !self.busy
                && !self.view.login
                && self
                    .view
                    .progress
                    .as_ref()
                    .is_some_and(|p| p.bootstrap.is_none() && p.settings.is_none()),
            |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.native, false, "Codex ChatGPT");
                    ui.radio_value(&mut self.native, true, "Native browser sign-in");
                });
            },
        );
        let connected =
            self.view.account["connected"] == true || self.view.account["grant_connected"] == true;
        let same_route = self.view.native == self.native;
        if connected && same_route {
            ui.label(
                RichText::new(format!(
                    "Connected{}",
                    self.view.account["email"]
                        .as_str()
                        .map(|s| format!(" as {s}"))
                        .unwrap_or_default()
                ))
                .color(if ui.visuals().dark_mode {
                    Color32::LIGHT_GREEN
                } else {
                    Color32::from_rgb(32, 120, 80)
                }),
            );
            if let Some(plan) = self.view.account["plan"].as_str() {
                ui.label(format!("Plan: {plan}"));
            }
        } else if !self.view.login {
            ui.label("Sign in to load the account's available models.");
        }
        if !self.native {
            if self.codex.executable.is_empty() {
                ui.label("Codex was not found on this computer.");
                ui.hyperlink_to(
                    "Codex installation guide",
                    "https://learn.chatgpt.com/docs/quickstart",
                );
            } else {
                ui.label(
                    RichText::new(format!("Codex found: {}", self.codex.executable))
                        .small()
                        .weak(),
                );
            }
            ui.collapsing("Advanced Codex locations", |ui| {
                field(ui, "Codex executable", &mut self.codex.executable, false);
                if ui.button("Locate Codex…").clicked() {
                    self.picker(Folder::Codex, true);
                }
                field(ui, "Home directory", &mut self.codex.host_home, false);
                if ui.button("Choose home…").clicked() {
                    self.picker(Folder::HostHome, false);
                }
                field(
                    ui,
                    "Codex state directory",
                    &mut self.codex.codex_home,
                    false,
                );
                if ui.button("Choose Codex state…").clicked() {
                    self.picker(Folder::CodexHome, false);
                }
            });
        } else {
            ui.label("The browser must reach this computer's local sign-in callback.");
        }
        let changed =
            !same_route || (!self.native && self.view.codex.as_ref() != Some(&self.codex));
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy && !self.view.login,
                    egui::Button::new(if changed {
                        "Use this account route"
                    } else {
                        "Check connection"
                    }),
                )
                .clicked()
            {
                self.send(Action::Account {
                    native: self.native,
                    codex: (!self.native).then(|| self.codex.clone()),
                });
            }
            if !changed {
                if ui
                    .add_enabled(
                        !self.busy
                            && !self.view.login
                            && (self.native || !self.codex.executable.is_empty()),
                        egui::Button::new("Sign in with browser"),
                    )
                    .clicked()
                {
                    self.send(Action::SignIn { device: false });
                }
                if !self.native
                    && ui
                        .add_enabled(
                            !self.busy && !self.view.login && !self.codex.executable.is_empty(),
                            egui::Button::new("Use device code"),
                        )
                        .clicked()
                {
                    self.send(Action::SignIn { device: true });
                }
            }
        });
        if self.view.login {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Waiting for sign-in…");
                if ui.button("Cancel sign-in").clicked() {
                    self.send(Action::CancelLogin);
                }
            });
            if let Some(url) = &self.auth_url {
                ui.hyperlink_to("Open sign-in page", url);
            }
            if let Some(code) = &self.auth_code {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(code).monospace().size(24.0));
                    if ui.button("Copy code").clicked() {
                        ui.ctx().copy_text(code.clone());
                    }
                });
            }
        }
    }
    fn setup(&mut self, ui: &mut Ui) {
        self.banner(ui);
        if !self.view.preview.is_null() {
            self.review(ui);
            return;
        }
        theme::header(ui, Icon::Settings, "Workspace setup", "");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["Account & model", "Personas", "Workspace"]
                .iter()
                .enumerate()
            {
                if ui
                    .selectable_label(self.setup_step == i, format!("{}  {label}", i + 1))
                    .clicked()
                {
                    self.setup_step = i;
                }
            }
        });
        ui.separator();
        match self.setup_step {
            0 => {
                self.account(ui);
                ui.add_space(22.0);
                ui.heading("Choose a model");
                ui.label("Selections come from your installed Codex. You can choose a different model for each persona later in this setup.");
                model_picker(
                    ui,
                    "group",
                    &self.view.models,
                    &mut self.model,
                    &mut self.effort,
                    &mut self.manual,
                );
                if ui
                    .add_enabled(
                        !self.busy && !self.view.login,
                        egui::Button::new("Refresh models"),
                    )
                    .clicked()
                {
                    self.send(Action::Models);
                }
                ui.add_space(12.0);
                if ui
                    .add_enabled(
                        !self.model.is_empty() && !self.effort.is_empty(),
                        egui::Button::new("Next: personas"),
                    )
                    .clicked()
                {
                    self.setup_step = 1;
                }
            }
            1 => {
                ui.heading("Meet your personas");
                ui.label("Give each persona a name and starting tendencies. They will develop their own character through their work.");
                ui.label(RichText::new("Describe openness, conscientiousness, extraversion, agreeableness, and sensitivity to stress, plus initial mood, energy, and assertiveness.").weak());
                let mut remove = None;
                for (i, p) in self.personas.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.strong(format!("Persona {}", i + 1));
                                if i > 0 && ui.small_button("Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                            field(ui, "Name", &mut p.name, false);
                            field(ui, "Starting profile", &mut p.profile, true);
                            ui.collapsing("Model for this persona", |ui| {
                                let mut custom = !p.model.is_empty();
                                if ui
                                    .checkbox(&mut custom, "Choose an individual model")
                                    .changed()
                                {
                                    if custom {
                                        p.model = self.model.clone();
                                        p.effort = self.effort.clone();
                                    } else {
                                        p.model.clear();
                                        p.effort.clear();
                                    }
                                }
                                if custom {
                                    let mut manual = !models(&self.view.models)
                                        .iter()
                                        .any(|m| m["slug"] == p.model);
                                    model_picker(
                                        ui,
                                        &format!("persona-{i}"),
                                        &self.view.models,
                                        &mut p.model,
                                        &mut p.effort,
                                        &mut manual,
                                    );
                                } else {
                                    ui.label(format!("{} · {} reasoning", self.model, self.effort));
                                }
                            });
                        });
                    });
                }
                if let Some(i) = remove {
                    self.personas.remove(i);
                }
                if ui.button("+ Add persona").clicked() {
                    self.personas.push(PersonaDraft {
                        name: format!("Persona {}", self.personas.len() + 1),
                        ..Default::default()
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Back").clicked() {
                        self.setup_step = 0;
                    }
                    if ui
                        .add_enabled(
                            self.personas
                                .iter()
                                .all(|p| !p.name.trim().is_empty() && !p.profile.trim().is_empty()),
                            egui::Button::new("Next: workspace"),
                        )
                        .clicked()
                    {
                        self.setup_step = 2;
                    }
                });
            }
            _ => {
                ui.heading("A place for your work");
                field(ui, "Working folder", &mut self.workspace, false);
                if ui.button("Choose working folder…").clicked() {
                    self.picker(Folder::Workspace, false);
                }
                ui.label("Personas can create files, run host commands, and install tools using this computer's account and environment.");
                ui.collapsing("Advanced", |ui| {
                    field(ui, "Local operator name", &mut self.operator, false);
                    ui.label(format!("Saved progress: {}", self.progress));
                });
                ui.horizontal(|ui| {
                    if ui.button("Back").clicked() {
                        self.setup_step = 1;
                    }
                    if ui
                        .add_enabled(
                            !self.busy && !self.workspace.is_empty(),
                            egui::Button::new("Review workspace"),
                        )
                        .clicked()
                    {
                        self.send(Action::Prepare(Setup {
                            operator: self.operator.clone(),
                            workspace: self.workspace.clone(),
                            model: self.model.clone(),
                            effort: self.effort.clone(),
                            personas: self.personas.clone(),
                        }));
                    }
                });
            }
        }
        ui.add_space(22.0);
        ui.collapsing("Open an existing runtime", |ui| {
            field(
                ui,
                "Existing runtime folder",
                &mut self.existing_runtime,
                false,
            );
            if ui.button("Browse runtime…").clicked() {
                self.picker(Folder::Runtime, false);
            }
            field(ui, "Its operator name", &mut self.operator, false);
            if ui
                .add_enabled(
                    !self.busy && !self.existing_runtime.is_empty(),
                    egui::Button::new("Open existing runtime"),
                )
                .clicked()
            {
                self.send(Action::Attach {
                    root: PathBuf::from(&self.existing_runtime),
                    operator: self.operator.clone(),
                });
            }
        });
    }
    fn review(&mut self, ui: &mut Ui) {
        ui.heading("Your workspace is ready to create");
        let preview = self.view.preview.clone();
        ui.label(format!(
            "Working folder: {}",
            s(&preview["setup"], "working_directory")
        ));
        if let Some(seeds) = preview["persona_seeds"].as_object() {
            for (id, p) in seeds {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.strong(s(p, "label"));
                    ui.label(s(p, "text"));
                    let choice = &preview["persona_models"][id];
                    ui.label(
                        RichText::new(format!(
                            "{} · {} reasoning",
                            s(choice, "model"),
                            s(choice, "effort")
                        ))
                        .small(),
                    );
                });
            }
        }
        ui.label("Starting a task shares its context with OpenAI and the participating personas. Creating the workspace makes no model call.");
        if self
            .view
            .progress
            .as_ref()
            .is_some_and(|p| !p.setup_authorized)
            && ui
                .add_enabled(!self.busy, egui::Button::new("Edit setup"))
                .clicked()
        {
            self.send(Action::EditSetup);
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Create workspace"))
            .clicked()
        {
            self.send(Action::Create);
        }
        ui.collapsing("Configuration details", |ui| {
            json_view(ui, &preview);
        });
    }
    fn selected(&self) -> Option<&Value> {
        models_at(&self.view.inspection, "works")
            .iter()
            .find(|w| w["id"].as_str() == self.view.selected.as_deref())
    }
    fn pending(&mut self, ui: &mut Ui) {
        if let Some(p) = self.view.progress.clone() {
            for n in &p.needs {
                if n["work"].is_null() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("A saved task is waiting for submission confirmation.");
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Confirm saved task"))
                            .clicked()
                        {
                            self.send(Action::ConfirmPendingNeed(s(n, "id").into()));
                        }
                    });
                }
            }
            if p.work_input
                .as_ref()
                .is_some_and(|i| i["delivered"] == false)
            {
                ui.horizontal_wrapped(|ui| {
                    ui.label("An earlier input is awaiting its receipt.");
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Confirm saved input"))
                        .clicked()
                    {
                        self.send(Action::ConfirmPendingInput);
                    }
                });
            }
            if self.view.pending_control {
                ui.horizontal_wrapped(|ui| {
                    ui.label("An earlier task control is awaiting its receipt.");
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Confirm saved control"))
                        .clicked()
                    {
                        self.send(Action::ConfirmPendingControl);
                    }
                });
            }
            if p.session.is_some()
                && self.view.running.is_none()
                && matches!(
                    self.view.session_status["status"].as_str(),
                    Some("not_recorded" | "outcome_unrecorded")
                )
            {
                egui::Frame::group(ui.style()).show(ui,|ui|{ui.label("The previous session has no recorded final outcome. Opening this window has not repeated it.");
                    ui.collapsing("Review retained session",|ui|{json_view(ui,&self.view.session_status);});
                    if ui.add_enabled(!self.busy,egui::Button::new("Keep unresolved session in history; allow a new session")).clicked(){self.send(Action::ArchiveSession);}
                });
            }
        }
    }
    fn conversation(&self, ui: &mut Ui) {
        let messages = models_at(&self.view.result, "messages");
        if messages.is_empty() {
            ui.add_space(18.0);
            ui.label("Replies from your personas will appear here.");
        }
        self.messages(ui, messages);
        if self.view.result["truncated"] == true {
            ui.label("The runtime returned a partial view of the available messages.");
        }
    }
    fn messages(&self, ui: &mut Ui, messages: &[Value]) {
        let colors = theme::palette(ui);
        for m in messages {
            ui.push_id(s(m,"id"), |ui| {
                egui::Frame::new().fill(colors.panel).inner_margin(12).corner_radius(3).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let label = self.persona_label(s(m,"sender"));
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(24.0,24.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 4, colors.selection);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label.chars().next().unwrap_or('P'), egui::FontId::proportional(13.0), colors.text);
                        ui.label(RichText::new(&label).size(14.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(s(m,"invocation")).monospace().size(10.5).weak());
                        });
                    });
                    if !s(m,"subject").is_empty() { ui.label(RichText::new(s(m,"subject")).small().weak()); }
                    theme::message_body(ui, s(m,"text"));
                    if !models_at(m,"file_references").is_empty() || !models_at(m,"history_references").is_empty() {
                        ui.collapsing("Citations and source references", |ui| {
                            json_view(ui, &json!({"files":m["file_references"],"history":m["history_references"],"invocation":m["invocation"]}));
                        });
                    }
                });
                ui.add_space(3.0);
            });
        }
    }
    fn persona_label(&self, id: &str) -> String {
        models_at(&self.view.inspection, "works")
            .iter()
            .flat_map(|w| models_at(w, "participants"))
            .find(|p| p["persona"] == id)
            .map(|p| s(p, "label").to_owned())
            .unwrap_or_else(|| id.to_owned())
    }
    fn files(&mut self, ui: &mut Ui, work: &str) {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.busy && !self.picker_busy, egui::Button::new("Export recorded files…")).on_hover_text("Export exact delivered files and eligible message citations with their source records").clicked() {
                self.picker(Folder::Export(work.into()), false);
            }
            if ui.button(if self.focus_files { "Restore panels" } else { "Expand viewer" }).clicked() {
                self.focus_files = !self.focus_files;
            }
        });
        let root = self.view.inspection["settings"]["working_directory"]
            .as_str()
            .unwrap_or("");
        if root.is_empty() {
            ui.label("This runtime has not supplied a working folder.");
            return;
        }
        let context = format!("{}:{work}:{root}", self.view.root.display());
        self.file_viewer.ui(
            ui,
            std::path::Path::new(root),
            &context,
            models_at(&self.view.result, "local_deliveries"),
        );
    }
    fn questions(&mut self, ui: &mut Ui, _work: &str) {
        let items = models_at(&self.view.questions["shared_questions"], "items").to_vec();
        if items.is_empty() {
            ui.label("There are no shared questions for this task.");
        }
        for item in items {
            let text = s(&item["question"], "text");
            if text.is_empty() {
                continue;
            }
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong(self.persona_label(s(&item, "owner")));
                ui.add(egui::Label::new(text).wrap().selectable(true));
                for reply in models_at(&item, "replies") {
                    ui.label(format!("Reply: {}", s(&reply["input"], "text")));
                    ui.label(
                        RichText::new(format!("Basis: {}", s(&reply["input"], "basis"))).small(),
                    );
                }
                for conclusion in models_at(&item, "conclusions") {
                    ui.collapsing("Owner's conclusion", |ui| {
                        json_view(ui, conclusion);
                    });
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Answer question"))
                    .clicked()
                {
                    self.dialog = Some(Dialog::Answer(item.clone()));
                    self.text.clear();
                    self.basis.clear();
                }
            });
        }
        for delivery in models_at(&self.view.questions, "answer_delivery") {
            ui.collapsing("Answer delivery", |ui| {
                json_view(ui, delivery);
            });
        }
    }
    fn activity(&self, ui: &mut Ui, work: &Value) {
        if let Some(steps) = self.view.inspection["settings"]["steps"].as_u64() {
            ui.label(format!("{steps} recorded call attempts in this runtime"));
        }
        for a in models_at(&self.view.result, "persona_activity") {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong(s(a, "label"));
                let judgment = &a["judgment"];
                let label = match s(judgment, "kind") {
                    "ongoing" => "Continuing",
                    "blocked" => "Reports a blocker",
                    "individual_sufficient" => "Judges their contribution sufficient",
                    "shared_sufficient" => "Judges the shared result sufficient",
                    "closed" => "Reports work closed",
                    _ => "Recorded work update",
                };
                ui.label(label);
                ui.add(
                    egui::Label::new(s(judgment, "reason"))
                        .wrap()
                        .selectable(true),
                );
                if a["origin"]["mandate_revision"] != a["current_mandate_revision"] {
                    ui.label("This account was recorded before the current task amendment.");
                }
                ui.collapsing("Attribution and input time", |ui| {
                    json_view(ui, &a["origin"]);
                });
            });
        }
        if self
            .view
            .progress
            .as_ref()
            .and_then(|p| p.session.as_ref())
            .is_some_and(|session| work["id"] == session.work)
            && !self.view.session_status.is_null()
        {
            let outcome = &self.view.session_status["terminal"]["outcome"];
            if !outcome.is_null() {
                ui.label(format!(
                    "Session: {}",
                    if outcome["kind"] == "failed" {
                        s(outcome, "code").replace('_', " ")
                    } else {
                        outcome["stop"]
                            .as_str()
                            .unwrap_or("returned")
                            .replace('_', " ")
                    }
                ));
                if let Some(failure) = outcome["driver_failure"].as_str() {
                    ui.label(format!("Recorded failure: {}", failure.replace('_', " ")));
                }
            }
            ui.collapsing("Session outcome", |ui| {
                json_view(ui, &self.view.session_status);
            });
        }
        ui.collapsing("Task and participant status", |ui| {
            json_view(ui, work);
        });
        if !self.view.result["requester_feedback"].is_null() {
            ui.collapsing("Feedback and responses", |ui| {
                json_view(ui, &self.view.result["requester_feedback"]);
            });
        }
        ui.collapsing("Result evidence", |ui| {
            let mut v = self.view.result.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("local_deliveries");
                o.remove("messages");
            }
            json_view(ui, &v);
        });
    }
    fn sessions(&mut self, ui: &mut Ui, work: &str) {
        let sessions: Vec<_> = self
            .view
            .progress
            .as_ref()
            .map(|p| {
                p.session_history
                    .iter()
                    .chain(p.session.iter())
                    .filter(|s| s.work == work)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if sessions.is_empty() {
            ui.label("No saved sessions for this task yet.");
            return;
        }
        ui.label(format!(
            "{} saved session{} for this task",
            sessions.len(),
            if sessions.len() == 1 { "" } else { "s" }
        ));
        ui.label("Select a session to see its recorded outcome and replies.");
        for (i, session) in sessions.iter().enumerate().rev() {
            let label = format!("Session {}", i + 1);
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(label)
                        .selected(self.view.selected_session.as_deref() == Some(&session.id)),
                )
                .on_hover_text(&session.id)
                .clicked()
            {
                self.send(Action::SelectSession(session.id.clone()));
            }
        }
        if self.view.selected_session.is_none() {
            return;
        }
        ui.separator();
        let detail = &self.view.session_detail;
        let status = &detail["session"];
        match s(status, "status") {
            "finished" => {
                let outcome = &status["terminal"]["outcome"];
                let stop = if outcome["kind"] == "failed" {
                    s(outcome, "code")
                } else {
                    outcome["stop"].as_str().unwrap_or("returned")
                };
                ui.strong(format!("Recorded outcome: {}", stop.replace('_', " ")));
                if let (Some(before), Some(after)) = (
                    status["claim"]["steps_before"].as_u64(),
                    outcome["steps_after"].as_u64(),
                ) {
                    ui.label(format!(
                        "{} call attempts in this session",
                        after.saturating_sub(before)
                    ));
                }
            }
            "outcome_unrecorded" => {
                ui.label("The final outcome has not been recorded. This session may still be running or may have been interrupted.");
            }
            "not_recorded" => {
                ui.label("This saved session request has no execution record.");
            }
            _ => {}
        }
        let messages = models_at(detail, "messages");
        if messages.is_empty() {
            ui.label("No replies are available for this session.");
        }
        self.messages(ui, messages);
        ui.collapsing("Session record", |ui| {
            json_view(ui, status);
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut open = true;
        let mut done = false;
        let title = match &dialog {
            Dialog::Task => "Start a task",
            Dialog::Feedback => "Give feedback",
            Dialog::Answer(_) => "Answer question",
            Dialog::Amend => "Amend task",
            Dialog::Development => "Profile & development",
            Dialog::Account => "Account",
            Dialog::OpenWorkspace => "Open workspace",
        };
        egui::Window::new(title).open(&mut open).collapsible(false).default_width(660.0).max_height(660.0).vscroll(true).show(ctx,|ui|{
            self.banner(ui);
            let work=self.view.selected.clone().unwrap_or_default();
            match &dialog{
                Dialog::Task=>{field(ui,"What would you like your personas to work on?",&mut self.text,true);ui.label("Starting runs the selected models using your account.");
                    if ui.add_enabled(!self.busy&&!self.text.trim().is_empty(),egui::Button::new("Start task")).clicked(){self.send(Action::NewTask{text:self.text.clone(),optional:false});done=true;}
                },
                Dialog::Feedback=>{
                    let participants=self.selected().map(|w|models_at(w,"participants").to_vec()).unwrap_or_default();
                    egui::ComboBox::from_label("Persona").selected_text(self.persona_label(&self.feedback_persona)).show_ui(ui,|ui|{for p in participants{ui.selectable_value(&mut self.feedback_persona,s(&p,"persona").into(),s(&p,"label"));}});
                    field(ui,"Your feedback",&mut self.text,true);
                    ui.collapsing("Replace earlier feedback",|ui|{ui.label("Select only directions this feedback explicitly replaces.");for item in models_at(&self.view.result["requester_feedback"],"items").iter().filter(|i|i["persona"]==self.feedback_persona){let key=s(item,"observation").to_owned();if key.is_empty(){continue}let mut selected=self.replaced.contains(&key);if ui.checkbox(&mut selected,format!("{}: {}",key,s(item,"text"))).changed(){self.replaced.retain(|k|k!=&key);if selected{self.replaced.push(key);}}}});
                    if ui.add_enabled(!self.busy&&!self.text.trim().is_empty()&&!self.feedback_persona.is_empty(),egui::Button::new("Save feedback")).clicked(){self.send(Action::Feedback{work,persona:self.feedback_persona.clone(),text:self.text.clone(),supersedes:self.replaced.clone()});done=true;}
                },
                Dialog::Answer(q)=>{ui.label(s(&q["question"],"text"));field(ui,"Your answer",&mut self.text,true);field(ui,"Basis (for example, your preference or observation)",&mut self.basis,true);
                    if ui.add_enabled(!self.busy&&!self.text.trim().is_empty()&&!self.basis.trim().is_empty(),egui::Button::new("Save answer")).clicked(){self.send(Action::Answer{work,question:q.clone(),text:self.text.clone(),basis:self.basis.clone()});done=true;}
                },
                Dialog::Amend=>{
                    if self.view.amendment.is_null(){ui.spinner();}else{
                        field(ui,"Revised complete task",&mut self.text,true);
                        ui.label("The original task and earlier results are retained. Participants receive an invitation to consider the amended task.");
                        ui.collapsing("Full scope (advanced)",|ui|{ui.label("Custom outcomes, review terms, and other scope fields remain editable here.");ui.add(egui::TextEdit::multiline(&mut self.amend_scope).code_editor().desired_rows(14).desired_width(f32::INFINITY));});
                        if ui.add_enabled(!self.busy&&!self.text.trim().is_empty(),egui::Button::new("Save amendment")).clicked(){
                            match journal::decode(self.amend_scope.as_bytes()){
                                Ok(mut scope)=>{let old=s(&scope,"request").to_owned();if scope["outcomes"]==json!({"result":{"description":old,"criterion":old,"required_reviews":[]}}){scope["outcomes"]["result"]["description"]=self.text.clone().into();scope["outcomes"]["result"]["criterion"]=self.text.clone().into();}
                                    scope["request"]=self.text.clone().into();self.send(Action::Amend{snapshot:self.view.amendment.clone(),scope});done=true;},
                                Err(e)=>self.error=Some(e)
                            }
                        }
                    }
                },
                Dialog::Development=>self.development(ui),
                Dialog::Account=>self.account(ui),
                Dialog::OpenWorkspace=>{
                    ui.label("Choose the progress folder containing your saved workspace.");
                    self.locations(ui);
                    if ui.add_enabled(!self.busy && !self.progress.is_empty(),theme::primary("Open workspace")).clicked(){self.open();done=true;}
                },
            }
        });
        if open && !done {
            self.dialog = Some(dialog);
        }
    }
    fn settings(&mut self, ctx: &egui::Context) {
        if !self.show_preferences {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings").open(&mut open).default_width(480.0).vscroll(true).show(ctx,|ui|{
            theme::header(ui, Icon::Settings, "Interface", "");
            let mut dark=matches!(self.preferences.appearance,Appearance::Dark);
            let changed=ui.checkbox(&mut dark,"Dark appearance").changed()|ui.add(egui::Slider::new(&mut self.preferences.zoom,0.75..=1.75).text("Interface scale")).changed();
            if changed{self.preferences.appearance=if dark{Appearance::Dark}else{Appearance::Light};self.apply_style(ctx);let _=self.tx.send(Input::Action(Action::Preferences(self.preferences.clone())));}
            ui.checkbox(&mut self.show_session_strip,"Show session history panel");
            ui.separator();self.locations(ui);
            if ui.add_enabled(!self.busy&&self.view.running.is_none()&&!self.view.login,egui::Button::new("Open selected progress folder")).clicked(){self.open();}
            ui.label("The terminal interface uses the same saved progress. Close this window before opening it there.");
        });
        self.show_preferences = open;
    }

    fn development(&self, ui: &mut Ui) {
        let v = &self.view.development;
        if v.is_null() {
            ui.spinner();
            return;
        }
        ui.heading(s(v, "label"));
        ui.label(format!("Identity: {}", s(&v["lifecycle"], "status")));
        if v["lifecycle"]["quarantined"] == true {
            ui.label("This identity is quarantined.");
        }
        if v["character"].is_null() {
            ui.label("This persona has not authored its characteristic profile yet. Its first model call develops the supplied starting tendencies.");
        } else {
            for (key, label) in [
                ("ocean", "Stable tendencies"),
                ("vad", "Baseline mood and energy"),
            ] {
                ui.strong(label);
                if let Some(traits) = v["character"][key].as_object() {
                    for (name, value) in traits {
                        ui.label(format!(
                            "{}: {}",
                            name.replace('_', " "),
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        ));
                    }
                }
            }
        }
        for revision in models_at(v, "characteristic_revisions") {
            ui.collapsing(
                format!(
                    "{} · {}",
                    s(revision, "source"),
                    if revision["current"] == true {
                        "current"
                    } else {
                        "historical"
                    }
                ),
                |ui| {
                    for entry in models_at(&revision["context"], "entries") {
                        ui.strong(s(entry, "path"));
                        ui.add(egui::Label::new(s(entry, "text")).wrap().selectable(true));
                    }
                    ui.collapsing("Authorship and full qualifications", |ui| {
                        json_view(ui, revision);
                    });
                },
            );
        }
        if v["transient_affect"]["context"]["status"] != "none_retained" {
            ui.collapsing("Current modeled affect", |ui| {
                json_view(ui, &v["transient_affect"]);
            });
        }
        if v["truncated"] == true {
            ui.label("This view contains a subset of retained revisions.");
        }
        ui.collapsing("Exact development record", |ui| {
            json_view(ui, v);
        });
    }
}
impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.file_viewer.destroy();
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.events(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.closing {
                self.closing = true;
                if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
                    self.preferences.width = rect.width();
                    self.preferences.height = rect.height();
                }
                if self.view.progress.is_some() {
                    let _ = self
                        .tx
                        .send(Input::Action(Action::Preferences(self.preferences.clone())));
                }
                self.send(Action::Close);
            }
        }
        self.top(ctx);
        self.status_bar(ctx);
        if self.choose_progress {
            self.library_sidebar(ctx);
            self.workspace_details(ctx);
        }
        if self.view.ready && !self.choose_progress {
            if self.tab != 1 || !self.focus_files {
                self.sidebar(ctx);
                if self.tab != 1 || ctx.available_rect().width() > 800.0 {
                    self.people(ctx);
                }
            }
        }
        let colors = theme::Palette::new(ctx.style().visuals.dark_mode);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.canvas).inner_margin(
                if self.view.ready && !self.choose_progress {
                    0
                } else {
                    10
                },
            ))
            .show(ctx, |ui| {
                if self.choose_progress {
                    self.workspaces(ui);
                } else if self.view.ready {
                    self.workspace(ui);
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(900.0));
                        if self.view.progress.is_none() {
                            self.startup(ui);
                        } else {
                            self.setup(ui);
                        }
                    });
                }
            });
        self.dialogs(ctx);
        self.settings(ctx);
        if self.closing && !self.allow_close {
            egui::Window::new("Finishing current operation")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            "Pausing work and waiting for its recorded outcome before closing.",
                        );
                    });
                    self.banner(ui);
                    if ui.button("Cancel closing").clicked() {
                        self.closing = false;
                        self.send(Action::CancelClose);
                    }
                    if !self.busy && ui.button("Retry closing").clicked() {
                        self.send(Action::Close);
                    }
                });
        }
        if self.busy
            || self.view.loading_models
            || self.view.running.is_some()
            || self.view.login
            || self.picker_busy
            || self.closing
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}
fn field(ui: &mut Ui, label: &str, text: &mut String, multi: bool) {
    let label = ui.label(label);
    if multi {
        ui.add(
            egui::TextEdit::multiline(text)
                .desired_width(f32::INFINITY)
                .desired_rows(4),
        )
        .labelled_by(label.id);
    } else {
        ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY))
            .labelled_by(label.id);
    }
}
fn models(v: &Value) -> &[Value] {
    models_at(v, "models")
}
fn models_at<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn short(text: &str, max: usize) -> String {
    let s: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        format!("{s}…")
    } else {
        s
    }
}
fn json_view(ui: &mut Ui, v: &Value) {
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    ui.add(
        egui::Label::new(RichText::new(text).monospace().size(13.0))
            .wrap()
            .selectable(true),
    );
}
fn model_picker(
    ui: &mut Ui,
    id: &str,
    catalog: &Value,
    model: &mut String,
    effort: &mut String,
    manual: &mut bool,
) {
    ui.push_id(id,|ui|{
        let choices=models(catalog);
        if choices.is_empty(){ui.label("No model list is available yet. Refresh after connecting, or enter a model manually.");}
        ui.checkbox(manual,"Enter model manually");
        if *manual{field(ui,"Model name",model,false);field(ui,"Reasoning effort",effort,false);return}
        let before=model.clone();
        egui::ComboBox::from_label("Model").selected_text(choices.iter().find(|m|m["slug"]==*model).map(|m|s(m,"display_name")).unwrap_or(model)).width(320.0).show_ui(ui,|ui|{
            for m in choices{ui.selectable_value(model,s(m,"slug").into(),s(m,"display_name")).on_hover_text(s(m,"description"));}
        });
        if let Some(m)=choices.iter().find(|m|m["slug"]==*model){
            if *model!=before||effort.is_empty(){*effort=m["default_effort"].as_str().unwrap_or("medium").into();}
            ui.label(RichText::new(s(m,"description")).small().weak());
            let efforts=models_at(m,"efforts");
            egui::ComboBox::from_label("Reasoning").selected_text(effort.clone()).show_ui(ui,|ui|{
                if efforts.is_empty(){for label in ["low","medium","high"]{ui.selectable_value(effort,label.into(),label);}}
                else{for e in efforts{ui.selectable_value(effort,s(e,"label").into(),s(e,"label")).on_hover_text(s(e,"description"));}}
            });
            if efforts.is_empty(){ui.label(RichText::new("Reasoning suggestions; this catalogue does not advertise supported levels.").small());}
        }
    });
}
