//! New desktop workspaces use the same initialization and task interface as the CLI.
use super::{design, file_desk, helpers, journal, neural, theme};
mod library;
mod restore;
use ai_personas::{
    auth,
    neural::{cli, store::Store},
    provider::Cancellation,
};
use eframe::egui::{self, Color32, RichText};
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
    gl: Option<std::sync::Arc<eframe::glow::Context>>,
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
    operator: String,
    saved: Vec<SavedWorkspace>,
    library_read: Option<mpsc::Receiver<Vec<SavedWorkspace>>>,
    codex: journal::CodexPaths,
    models: Value,
    model: String,
    effort: String,
    provider_file: String,
    provider_route: usize,
    platform_key: String,
    personas: Vec<PersonaDraft>,
    create_mode: bool,
    library_dark: bool,
    open_path: bool,
    restore_desk: Option<restore::Form>,
    library_filter: String,
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

fn recent_directory() -> PathBuf {
    base_directory("XDG_STATE_HOME", ".local/state").join("ai-personas/recent-workspaces")
}

fn remember_workspace(root: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::{ffi::OsStrExt,fs::DirBuilderExt};
    let directory=recent_directory();
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&directory)?;
    let path=root.canonicalize()?;
    let id=ai_personas::store::sha256_bytes(path.as_os_str().as_bytes());
    let mut file=tempfile::NamedTempFile::new_in(&directory)?;
    serde_json::to_writer(file.as_file_mut(),&path)?;
    file.persist(directory.join(format!("{id}.json")))?;
    Ok(())
}

struct SavedWorkspace {
    path: PathBuf,
    opened: Option<std::time::SystemTime>,
    editable: bool,
    people: Vec<(String, String)>,
}

fn read_library(ctx: &egui::Context) -> mpsc::Receiver<Vec<SavedWorkspace>> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(saved_workspaces());
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    });
    rx
}

fn saved_people(path: &std::path::Path) -> rusqlite::Result<Vec<(String, String)>> {
    let database = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    database.busy_timeout(std::time::Duration::from_millis(80))?;
    let mut query = database.prepare(
        "SELECT h.id, COALESCE(NULLIF(json_extract(CAST(o.bytes AS TEXT),'$.label'),''),h.id)
         FROM heads h JOIN objects o ON o.digest=h.digest WHERE h.kind='persona'
         ORDER BY 2 COLLATE NOCASE, h.id",
    )?;
    let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

fn saved_workspaces() -> Vec<SavedWorkspace> {
    let base=base_directory("XDG_STATE_HOME",".local/state").join("ai-personas/editable-workspaces");
    let mut paths=std::collections::BTreeMap::new();
    for root in std::fs::read_dir(base).into_iter().flatten().flatten().map(|entry|entry.path()).chain(journal::candidates()) {
        if root.join("neural.sqlite").is_file() || root.join("progress.json").is_file() {
            let modified=std::fs::metadata(&root).and_then(|m|m.modified()).ok();
            paths.insert(std::fs::canonicalize(&root).unwrap_or(root),(modified,None));
        }
    }
    for entry in std::fs::read_dir(recent_directory()).into_iter().flatten().flatten() {
        if entry.path().extension().is_none_or(|ext|ext!="json") {continue;}
        let Some(path)=std::fs::read(entry.path()).ok().and_then(|b|serde_json::from_slice::<PathBuf>(&b).ok()) else {continue;};
        if path.join("neural.sqlite").is_file() {
            let opened = entry.metadata().and_then(|m|m.modified()).ok();
            paths.insert(path,(opened,opened));
        }
    }
    let mut items:Vec<_>=paths.into_iter().collect();
    items.sort_by_key(|(_,modified)|std::cmp::Reverse(*modified));
    items.into_iter().map(|(path, (_, opened))| {
        let database = path.join("neural.sqlite");
        let editable = database.is_file();
        let people = if editable { saved_people(&database).unwrap_or_default() } else { Vec::new() };
        SavedWorkspace { path, opened, editable, people }
    }).collect()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, requested: Option<PathBuf>, operator: String) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        theme::apply(ctx, true, 1.0);
        let (tx, rx) = mpsc::channel();
        let base = base_directory("XDG_STATE_HOME", ".local/state")
            .join("ai-personas/editable-workspaces");
        let name = journal::id("workspace");
        let library_read = Some(read_library(ctx));
        let codex = helpers::codex(None).unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            journal::CodexPaths {
                executable: String::new(),
                host_home: home.clone(),
                codex_home: std::env::var("CODEX_HOME")
                    .unwrap_or_else(|_| format!("{home}/.codex")),
            }
        });
        let existing=requested.as_ref().filter(|p|p.join("neural.sqlite").is_file()).cloned();
        let state_root = requested.clone().filter(|_|existing.is_none())
            .unwrap_or_else(|| base.join(name.as_deref().unwrap_or("new-workspace")));
        let working = base_directory("XDG_DATA_HOME", ".local/share")
            .join("ai-personas/workspaces")
            .join(name.as_deref().unwrap_or("new-workspace"));
        let mut app=Self {
            gl: cc.gl.clone(),
            workspace: None,
            tx,
            rx,
            busy: false,
            operator,
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
            saved: Vec::new(),
            library_read,
            codex,
            models: Value::Null,
            model: String::new(),
            effort: "low".into(),
            provider_file: String::new(),
            provider_route: 0,
            platform_key: String::new(),
            personas: vec![PersonaDraft {
                name: "Persona 1".into(),
                profile: String::new(),
            }],
            library_filter: String::new(),
            library_dark: true,
            open_path: false,
            restore_desk: None,
            create_mode: requested.is_some() && existing.is_none(),
        };
        if let Some(root)=existing {app.open(ctx,root);}
        app
    }

    fn open(&mut self, ctx: &egui::Context, root: PathBuf) {
        if !root.join("neural.sqlite").is_file() && root.join("progress.json").is_file() {
            let opened = std::env::current_exe().and_then(|exe| {
                std::process::Command::new(exe)
                    .arg("--progress")
                    .arg(root)
                    .spawn()
            });
            if let Err(error) = opened {
                self.error = Some(error.to_string());
            }
            return;
        }
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
        let operator = self.operator.trim().to_owned();
        let root = journal::expand(&PathBuf::from(&self.state_root));
        let directory = journal::expand(&PathBuf::from(&self.working_directory));
        let seeds = json!({"personas":self.personas.iter().enumerate().map(|(i,p)| {
            json!({"id":format!("persona-{}",i+1),"label":p.name,"seed":p.profile})
        }).collect::<Vec<_>>()});
        let c = self.codex.clone();
        let model = self.model.clone();
        let effort = self.effort.clone();
        let platform_key_file =
            (self.provider_route == 1).then(|| journal::expand(&PathBuf::from(&self.platform_key)));
        let provider = (self.provider_route == 2)
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
                let codex = provider.is_none() && platform_key_file.is_none();
                let choose_model = provider.is_none();
                let command = cli::Command::Init {
                    seeds: file.path().into(),
                    working_directory: directory,
                    provider_config: provider,
                    platform_key_file,
                    codex_executable: codex.then(|| PathBuf::from(c.executable)),
                    host_home: codex.then(|| PathBuf::from(c.host_home)),
                    codex_home: codex.then(|| PathBuf::from(c.codex_home)),
                    model: choose_model.then_some(model),
                    effort: choose_model.then_some(effort),
                };
                tokio::runtime::Runtime::new()
                    .map_err(|e| e.to_string())?
                    .block_on(cli::execute(&root, &operator, command))
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
                        Ok(root) => {
                            self.restore_desk = None;
                            let _=remember_workspace(&root);
                            self.workspace = Some(neural::App::new(ctx, root, self.gl.as_ref(), self.operator.clone()))
                        }
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
        if ui.available_width() >= 850. {
            let gap = ui.spacing().item_spacing.x;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 22.;
                ui.columns(2, |columns| {
                    columns[0].spacing_mut().item_spacing.x = gap;
                    columns[1].spacing_mut().item_spacing.x = gap;
                    self.setup_people(&mut columns[0]);
                    let p = theme::palette(&columns[1]);
                    egui::Frame::new()
                        .fill(p.panel)
                        .stroke(egui::Stroke::new(1., p.border))
                        .corner_radius(10)
                        .inner_margin(18)
                        .show(&mut columns[1], |ui| {
                            ui.set_min_width(ui.available_width());
                            self.setup_connection(ui, ctx);
                        });
                });
            });
        } else {
            self.setup_people(ui);
            ui.add_space(16.);
            self.setup_connection(ui, ctx);
        }
    }

    fn setup_people(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui);
        design::section_heading(
            ui,
            "Starting ensemble",
            &format!(
                "{} {}",
                self.personas.len(),
                if self.personas.len() == 1 {
                    "persona"
                } else {
                    "personas"
                }
            ),
        );
        field(ui, "Your name on these profiles", &mut self.operator);
        ui.add_space(6.);
        let mut remove = None;
        for (i, persona) in self.personas.iter_mut().enumerate() {
            let color = neural::persona_color(&format!("persona-{}", i + 1));
            ui.push_id(i, |ui| {
                egui::Frame::new().fill(p.panel.lerp_to_gamma(color,0.07)).stroke(egui::Stroke::new(1.,p.border))
                    .corner_radius(9).inner_margin(14).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(52.),egui::Sense::hover());
                            design::persona_mark(ui,mark,&persona.name,color);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                field(ui,"Persona name",&mut persona.name);
                            });
                        });
                        ui.add_space(7.);
                        ui.label("Starting character");
                        ui.add(egui::TextEdit::multiline(&mut persona.profile).desired_rows(4).desired_width(f32::INFINITY)
                            .hint_text("Describe their traits, voice and tendencies, including OCEAN or VAD if useful."));
                        if ui.small_button("Remove persona").clicked() { remove = Some(i); }
                    });
            });
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
    }

    fn setup_connection(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        design::section_heading(ui, "Model connection", "");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.provider_route, 0, "Codex account");
            ui.selectable_value(&mut self.provider_route, 1, "OpenAI API key");
            ui.selectable_value(&mut self.provider_route, 2, "Configuration file");
        });
        if self.provider_route == 1 {
            field(ui, "Private key file", &mut self.platform_key);
            if ui.button("Choose key file…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.platform_key = path.display().to_string();
                }
            }
            ui.weak("Use a private text file containing your key or an existing Studio credential file.");
            field(ui, "Model", &mut self.model);
            field(ui, "Reasoning effort", &mut self.effort);
        } else if self.provider_route == 2 {
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
        ui.label("Choose the folder containing the files this group can import. Deliveries can be exported from the studio.");
        ui.collapsing("Saved progress location", |ui| {
            field(ui, "New saved-progress folder", &mut self.state_root);
        });
        ui.label(
            "Creating a workspace makes no reasoning calls. Add a task and start work when ready.",
        );
        let valid = !self.operator.trim().is_empty() && !self.personas.is_empty()
            && self
                .personas
                .iter()
                .all(|p| !p.name.trim().is_empty() && !p.profile.trim().is_empty())
            && !self.state_root.trim().is_empty()
            && !self.working_directory.trim().is_empty()
            && if self.provider_route == 1 {
                !self.platform_key.trim().is_empty()
                    && !self.model.trim().is_empty()
                    && !self.effort.trim().is_empty()
            } else if self.provider_route == 2 {
                !self.provider_file.trim().is_empty()
            } else {
                !self.model.trim().is_empty()
                    && !self.effort.trim().is_empty()
                    && !self.codex.executable.trim().is_empty()
            };
        if ui
            .add_enabled(valid, theme::primary("Create workspace"))
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
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(workspace) = &mut self.workspace {
            workspace.on_exit(gl);
        }
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.drain(ctx);
        if let Some(reader) = &self.library_read {
            match reader.try_recv() {
                Ok(saved) => { self.saved = saved; self.library_read = None; }
                Err(mpsc::TryRecvError::Disconnected) => self.library_read = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(workspace) = &mut self.workspace {
            workspace.update(ctx, frame);
            if workspace.show_library {
                self.operator.clone_from(&workspace.operator);
                workspace.on_exit(self.gl.as_deref());
                self.workspace=None;
                self.library_read=Some(read_library(ctx));
                self.create_mode=false;
                self.error=None;
                theme::apply(ctx,self.library_dark,1.);
            }
            return;
        }
        self.library(ctx);
    }
}

impl App {
    fn connection_status(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working…");
                if self.cancellable && ui.button("Cancel connection").clicked() {
                    self.cancellation.cancel();
                }
            });
        }
        if let Some(url) = ["url", "auth_url", "verification_url"]
            .into_iter()
            .find_map(|key| self.login_event[key].as_str())
        {
            if url.starts_with("https://") {
                ui.hyperlink_to("Open sign-in", url);
            }
        }
        if let Some(code) = self.login_event["user_code"]
            .as_str()
            .or(self.login_event["code"].as_str())
        {
            ui.monospace(code);
        }
    }

}
