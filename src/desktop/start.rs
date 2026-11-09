//! New desktop workspaces use the same initialization and task interface as the CLI.
use super::{helpers, journal, neural, theme};
use ai_personas::{
    auth,
    neural::{cli, store::Store},
    provider::Cancellation,
};
use eframe::egui;
use serde_json::{json, Value};
use std::{io::Write, path::PathBuf, sync::mpsc};

#[derive(Clone)]
struct PersonaDraft {
    name: String,
    profile: String,
}

enum Update {
    Opened(Result<PathBuf, String>),
    Models(Result<Value, String>),
    Account(Result<Value, String>),
    LoginEvent(Value),
}

pub struct App {
    workspace: Option<neural::App>,
    tx: mpsc::Sender<Update>,
    rx: mpsc::Receiver<Update>,
    busy: bool,
    cancellable: bool,
    error: Option<String>,
    account: Value,
    login_event: Value,
    cancellation: Cancellation,
    state_root: String,
    working_directory: String,
    existing_root: String,
    saved: Vec<PathBuf>,
    codex: journal::CodexPaths,
    models: Value,
    model: String,
    effort: String,
    provider_file: String,
    use_provider_file: bool,
    personas: Vec<PersonaDraft>,
    create_mode: bool,
}

fn base_directory(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(fallback)
        })
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

impl App {
    pub fn new(ctx: &egui::Context, requested: Option<PathBuf>) -> Self {
        theme::install_fonts(ctx);
        theme::apply(ctx, true, 1.0);
        let (tx, rx) = mpsc::channel();
        let base = base_directory("XDG_STATE_HOME", ".local/state")
            .join("ai-personas/editable-workspaces");
        let name = journal::id("workspace");
        let mut saved: Vec<_> = std::fs::read_dir(&base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join("neural.sqlite").is_file())
            .collect();
        saved.sort();
        let codex = helpers::codex(None).unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            journal::CodexPaths {
                executable: String::new(),
                host_home: home.clone(),
                codex_home: std::env::var("CODEX_HOME")
                    .unwrap_or_else(|_| format!("{home}/.codex")),
            }
        });
        let state_root = requested
            .clone()
            .unwrap_or_else(|| base.join(name.as_deref().unwrap_or("new-workspace")));
        let working = base_directory("XDG_DATA_HOME", ".local/share")
            .join("ai-personas/workspaces")
            .join(name.as_deref().unwrap_or("new-workspace"));
        Self {
            workspace: None,
            tx,
            rx,
            busy: false,
            cancellable: false,
            error: name.err(),
            account: Value::Null,
            login_event: Value::Null,
            cancellation: Cancellation::default(),
            state_root: state_root.to_string_lossy().into_owned(),
            working_directory: working.to_string_lossy().into_owned(),
            existing_root: requested
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            saved,
            codex,
            models: Value::Null,
            model: String::new(),
            effort: String::new(),
            provider_file: String::new(),
            use_provider_file: false,
            personas: vec![PersonaDraft {
                name: "Persona 1".into(),
                profile: String::new(),
            }],
            create_mode: requested.is_some(),
        }
    }

    fn open(&mut self, ctx: &egui::Context, root: PathBuf) {
        self.busy = true;
        self.cancellable = false;
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = Store::open(&root)
                .and_then(|mut store| {
                    store.status()?;
                    Ok(store.root)
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Update::Opened(result));
            ctx.request_repaint();
        });
    }

    fn account_action(&mut self, ctx: &egui::Context, login: Option<bool>) {
        self.busy = true;
        self.cancellable = true;
        self.error = None;
        self.login_event = Value::Null;
        self.cancellation = Cancellation::default();
        let cancel = self.cancellation.clone();
        let c = self.codex.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = auth::codex_account(
                c.executable.as_ref(),
                c.host_home.as_ref(),
                c.codex_home.as_ref(),
                login,
                cancel,
                |event| {
                    let _ = tx.send(Update::LoginEvent(event));
                    ctx.request_repaint();
                },
            )
            .map_err(|e| format!("Account connection: {e:?}"));
            let _ = tx.send(Update::Account(result));
            ctx.request_repaint();
        });
    }

    fn load_models(&mut self, ctx: &egui::Context) {
        self.busy = true;
        self.cancellable = true;
        self.error = None;
        self.cancellation = Cancellation::default();
        let cancel = self.cancellation.clone();
        let c = self.codex.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = auth::catalogue_codex_cancellable(
                c.executable.as_ref(),
                c.host_home.as_ref(),
                c.codex_home.as_ref(),
                cancel,
            )
            .map_err(|e| format!("Model discovery: {e:?}"))
            .and_then(|models| serde_json::to_value(models).map_err(|e| e.to_string()));
            let _ = tx.send(Update::Models(result));
            ctx.request_repaint();
        });
    }

    fn create(&mut self, ctx: &egui::Context) {
        self.busy = true;
        self.cancellable = false;
        self.error = None;
        let root = journal::expand(&PathBuf::from(&self.state_root));
        let directory = journal::expand(&PathBuf::from(&self.working_directory));
        let seeds = json!({"personas":self.personas.iter().enumerate().map(|(i,p)| {
            json!({"id":format!("persona-{}",i+1),"label":p.name,"seed":p.profile})
        }).collect::<Vec<_>>()});
        let c = self.codex.clone();
        let model = self.model.clone();
        let effort = self.effort.clone();
        let provider = self
            .use_provider_file
            .then(|| journal::expand(&PathBuf::from(&self.provider_file)));
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                if root.exists()
                    && std::fs::read_dir(&root)
                        .map_err(|e| e.to_string())?
                        .next()
                        .is_some()
                {
                    return Err(
                        "Choose an empty saved-progress folder, or open its existing workspace."
                            .into(),
                    );
                }
                std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let mut file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
                serde_json::to_writer(file.as_file_mut(), &seeds).map_err(|e| e.to_string())?;
                file.flush().map_err(|e| e.to_string())?;
                let codex = provider.is_none();
                let command = cli::Command::Init {
                    seeds: file.path().into(),
                    working_directory: directory,
                    provider_config: provider,
                    codex_executable: codex.then(|| PathBuf::from(c.executable)),
                    host_home: codex.then(|| PathBuf::from(c.host_home)),
                    codex_home: codex.then(|| PathBuf::from(c.codex_home)),
                    model: codex.then_some(model),
                    effort: codex.then_some(effort),
                };
                tokio::runtime::Runtime::new()
                    .map_err(|e| e.to_string())?
                    .block_on(cli::execute(&root, "user", command))
                    .map_err(|e| e.to_string())?;
                root.canonicalize().map_err(|e| e.to_string())
            })();
            let _ = tx.send(Update::Opened(result));
            ctx.request_repaint();
        });
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::LoginEvent(value) => self.login_event = value,
                Update::Opened(result) => {
                    self.busy = false;
                    match result {
                        Ok(root) => self.workspace = Some(neural::App::new(ctx, root)),
                        Err(error) => self.error = Some(error),
                    }
                }
                Update::Models(result) => {
                    self.busy = false;
                    match result {
                        Ok(value) => self.models = value,
                        Err(error) => self.error = Some(error),
                    }
                }
                Update::Account(result) => {
                    self.busy = false;
                    self.login_event = Value::Null;
                    match result {
                        Ok(value) => self.account = value,
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
    }

    fn setup(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Create your personas");
        ui.label("Each persona starts from your character profile and develops its own editable character model. Add as many as you want.");
        let mut remove = None;
        for (i, persona) in self.personas.iter_mut().enumerate() {
            ui.push_id(i, |ui| ui.group(|ui| {
                field(ui, "Name", &mut persona.name);
                ui.label("Starting character");
                ui.add(egui::TextEdit::multiline(&mut persona.profile).desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("Describe their traits, voice and tendencies, including OCEAN or VAD if useful."));
                if ui.small_button("Remove persona").clicked() { remove = Some(i); }
            }));
        }
        if let Some(i) = remove {
            self.personas.remove(i);
        }
        if ui.button("Add persona").clicked() {
            self.personas.push(PersonaDraft {
                name: format!("Persona {}", self.personas.len() + 1),
                profile: String::new(),
            });
        }
        ui.add_space(12.0);
        ui.heading("Model connection");
        ui.checkbox(
            &mut self.use_provider_file,
            "Use a saved provider configuration",
        );
        if self.use_provider_file {
            field(ui, "Provider configuration file", &mut self.provider_file);
            ui.label("Uses that file's model, reasoning and credential references from this process's environment.");
        } else {
            ui.label(
                "Use your Codex account connection. Persona reasoning calls the provider directly.",
            );
            ui.horizontal(|ui| {
                if ui.button("Check account").clicked() {
                    self.account_action(ctx, None);
                }
                if ui.button("Sign in").clicked() {
                    self.account_action(ctx, Some(false));
                }
                if ui.button("Device sign-in").clicked() {
                    self.account_action(ctx, Some(true));
                }
                if ui.button("Load models").clicked() {
                    self.load_models(ctx);
                }
            });
            if !self.account.is_null() {
                if self.account["connected"] == true {
                    ui.label(format!(
                        "Connected: {} {}",
                        self.account["email"].as_str().unwrap_or("Codex account"),
                        self.account["plan"].as_str().unwrap_or("")
                    ));
                } else {
                    ui.label("Sign in with a ChatGPT account to use this connection.");
                }
            }
            if let Some(models) = self.models["models"].as_array() {
                egui::ComboBox::from_id_salt("new-model")
                    .selected_text(if self.model.is_empty() {
                        "Choose a model"
                    } else {
                        &self.model
                    })
                    .show_ui(ui, |ui| {
                        for choice in models {
                            if let Some(slug) = choice["slug"].as_str() {
                                if ui
                                    .selectable_value(
                                        &mut self.model,
                                        slug.into(),
                                        choice["display_name"].as_str().unwrap_or(slug),
                                    )
                                    .changed()
                                {
                                    self.effort =
                                        choice["default_effort"].as_str().unwrap_or("").into();
                                }
                            }
                        }
                    });
                if let Some(choice) = models.iter().find(|m| m["slug"] == self.model) {
                    egui::ComboBox::from_id_salt("new-effort")
                        .selected_text(&self.effort)
                        .show_ui(ui, |ui| {
                            for effort in choice["efforts"].as_array().into_iter().flatten() {
                                if let Some(label) = effort["label"].as_str() {
                                    ui.selectable_value(&mut self.effort, label.into(), label);
                                }
                            }
                        });
                }
            }
            ui.collapsing("Connection and manual model settings", |ui| {
                field(ui, "Codex executable", &mut self.codex.executable);
                field(ui, "Account home", &mut self.codex.host_home);
                field(ui, "Codex home", &mut self.codex.codex_home);
                field(ui, "Model", &mut self.model);
                field(ui, "Reasoning effort", &mut self.effort);
            });
            if !self.model.is_empty() {
                ui.label(format!("{} · {} reasoning", self.model, self.effort));
            }
        }
        ui.add_space(12.0);
        ui.heading("Workspace");
        field(ui, "Working folder", &mut self.working_directory);
        ui.label("Personas can create files, run commands and install tools using this computer's account and environment.");
        ui.collapsing("Saved progress location", |ui| {
            field(ui, "New saved-progress folder", &mut self.state_root);
        });
        ui.label(
            "Creating a workspace makes no reasoning calls. Add a task and start work when ready.",
        );
        let valid = !self.personas.is_empty()
            && self
                .personas
                .iter()
                .all(|p| !p.name.trim().is_empty() && !p.profile.trim().is_empty())
            && !self.state_root.trim().is_empty()
            && !self.working_directory.trim().is_empty()
            && if self.use_provider_file {
                !self.provider_file.trim().is_empty()
            } else {
                !self.model.trim().is_empty()
                    && !self.effort.trim().is_empty()
                    && !self.codex.executable.trim().is_empty()
            };
        if ui
            .add_enabled(valid, egui::Button::new("Create workspace"))
            .clicked()
        {
            self.create(ctx);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.drain(ctx);
        if let Some(workspace) = &mut self.workspace {
            workspace.update(ctx, frame);
            return;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Personas");
                if let Some(error) = &self.error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
                if self.busy {
                    ui.label("Working…");
                    if self.cancellable && ui.button("Cancel connection").clicked() {
                        self.cancellation.cancel();
                    }
                }
                if !self.login_event.is_null() {
                    for key in ["url", "auth_url", "verification_url", "user_code", "code"] {
                        if let Some(value) = self.login_event[key].as_str() {
                            if value.starts_with("https://") { ui.hyperlink_to("Open sign-in", value); }
                            else { ui.label(value); }
                        }
                    }
                    ui.collapsing("Sign-in details", |ui| { ui.label(self.login_event.to_string()); });
                }
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.create_mode, false, "Open workspace");
                        ui.selectable_value(&mut self.create_mode, true, "New workspace");
                    });
                    ui.separator();
                    if self.create_mode { self.setup(ui, ctx); }
                    else {
                        field(ui, "Saved-progress folder", &mut self.existing_root);
                        if ui.add_enabled(!self.existing_root.trim().is_empty(), egui::Button::new("Open workspace")).clicked() {
                            self.open(ctx, journal::expand(&PathBuf::from(&self.existing_root)));
                        }
                        let mut chosen = None;
                        for root in &self.saved {
                            if ui.button(root.display().to_string()).clicked() { chosen = Some(root.clone()); }
                        }
                        if let Some(root) = chosen { self.open(ctx, root); }
                        if self.saved.is_empty() { ui.label("Create a workspace or enter the location of one you already saved."); }
                    }
                });
            });
        });
    }
}
