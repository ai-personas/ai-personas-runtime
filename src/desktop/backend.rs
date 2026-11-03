//! One journal-owning controller, with separate long-lived login/run workers.
//! UI actions are typed; provider execution always passes through native helpers.
use super::{
    helpers::{self, Cancel, Helpers},
    journal::{self, CodexPaths, Journal, Progress, Result, Runtime, Session},
    providers::{self, ApiProvider, Selection},
};
use eframe::egui;
mod provider_change;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PersonaDraft {
    pub name: String,
    pub profile: String,
    pub model: String,
    pub effort: String,
    #[serde(default)]
    pub provider: Option<ApiProvider>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub manual: bool,
}
#[derive(Clone)]
pub struct Setup {
    pub operator: String,
    pub workspace: String,
    pub model: String,
    pub effort: String,
    pub personas: Vec<PersonaDraft>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Dark,
    Light,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub zoom: f32,
    pub width: f32,
    pub height: f32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::Dark,
            zoom: 1.0,
            width: 1180.0,
            height: 800.0,
        }
    }
}
#[derive(Clone, Default)]
pub struct View {
    pub progress: Option<Progress>,
    pub root: PathBuf,
    pub codex: Option<CodexPaths>,
    pub native: bool,
    pub account: Value,
    pub models: Value,
    pub api_provider: Option<ApiProvider>,
    pub configured_providers: std::collections::BTreeSet<ApiProvider>,
    pub provider_defaults: std::collections::BTreeMap<ApiProvider, Selection>,
    pub provider_catalogs: std::collections::BTreeMap<ApiProvider, Value>,
    pub provider_status: std::collections::BTreeMap<ApiProvider, String>,
    pub saved_settings: Value,
    pub preview: Value,
    pub inspection: Value,
    pub selected: Option<String>,
    // Retained message provenance can dwarf its display text. Share complete
    // immutable snapshots between the controller and frames, without copying
    // that evidence on each publication or repaint.
    pub result: Arc<Value>,
    pub conversation: Arc<Value>,
    pub questions: Value,
    pub development: Value,
    pub session_status: Value,
    pub selected_session: Option<String>,
    pub session_detail: Value,
    pub amendment: Value,
    pub running: Option<String>,
    pub login: bool,
    pub loading_models: bool,
    pub refreshing: bool,
    pub closing: bool,
    pub pending_control: bool,
    pub pending_provider_change: bool,
    pub ready: bool,
    pub read_only: bool,
    pub files: Option<helpers::ResultFiles>,
}
pub enum Action {
    ApplyProvider { provider: Option<ApiProvider>, selection: Selection, preserve_models: bool, effort_override: Option<String> },
    FinishProviderChange,
    SelectProvider(Option<ApiProvider>),
    SaveProvider { provider: ApiProvider, key: String, selection: Selection },
    RemoveProvider(ApiProvider),
    CheckProvider(ApiProvider),
    Discover(PathBuf),
    Open {
        root: PathBuf,
        helpers: Option<PathBuf>,
    },
    Refresh,
    Select(String),
    SelectSession(String),
    Account {
        native: bool,
        codex: Option<CodexPaths>,
    },
    Models,
    SignIn {
        device: bool,
    },
    CancelLogin,
    Prepare(Setup),
    EditSetup,
    Create,
    Attach {
        root: PathBuf,
        operator: String,
    },
    NewTask {
        text: String,
        optional: bool,
    },
    ConfirmPendingNeed(String),
    Run(String),
    RecoverWork(String),
    Control {
        work: String,
        action: String,
    },
    ResumePersona {
        work: String,
        persona: String,
    },
    Feedback {
        work: String,
        persona: String,
        text: String,
        supersedes: Vec<String>,
    },
    Answer {
        work: String,
        question: Value,
        text: String,
        basis: String,
    },
    AmendSnapshot(String),
    Amend {
        snapshot: Value,
        scope: Value,
    },
    ConfirmPendingInput,
    ConfirmPendingControl,
    ArchiveSession,
    Development(String),
    Conversation(String),
    Export {
        work: String,
        parent: PathBuf,
    },
    Preferences(Preferences),
    Close,
    CancelClose,
}
pub enum Event {
    Workspaces(Vec<journal::SavedWorkspace>),
    View(Box<View>),
    Error(String),
    Notice(String),
    Authorization { url: String, code: Option<String> },
    Idle,
    SafeToClose,
    Preferences(Preferences),
}
pub enum Input {
    Action(Action),
    SnapshotDone { job: String, generation: u64, result: Result<Value> },
    RunDone(Result<Value>),
    LoginDone(Result<Value>),
    MetadataDone {
        job: String,
        provider: Option<ApiProvider>,
        account: Result<Value>,
        models: Result<Value>,
    },
}
#[derive(Clone)]
struct Sender {
    tx: mpsc::Sender<Event>,
    ctx: egui::Context,
}
impl Sender {
    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}
pub fn start(ctx: egui::Context) -> (mpsc::Sender<Input>, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let (events, output) = mpsc::channel();
    let internal = tx.clone();
    thread::spawn(move || {
        let sender = Sender { tx: events, ctx };
        let mut controller: Option<Controller> = None;
        let mut next = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Input::Action(Action::Discover(current))) => {
                    sender.send(Event::Workspaces(journal::discover(&current)));
                }
                Ok(Input::Action(Action::Open { root, helpers })) => {
                    if controller
                        .as_ref()
                        .is_some_and(|c| c.run.is_some() || c.login.is_some())
                    {
                        sender.send(Event::Error(
                            "Pause current work before switching progress folders.".into(),
                        ));
                    } else {
                        controller.take();
                        match Controller::open(root, helpers, sender.clone(), internal.clone()) {
                            Ok(mut c) => {
                                if let Err(e) = c.refresh() {
                                    sender.send(Event::Error(e));
                                }
                                if !c.view.ready {
                                    if let Err(e) = c.metadata() {
                                        sender.send(Event::Notice(e));
                                    }
                                }
                                c.publish();
                                controller = Some(c);
                            }
                            Err(e) => sender.send(Event::Error(e)),
                        }
                    }
                    sender.send(Event::Idle);
                }
                Ok(Input::Action(action)) => {
                    if let Some(c) = controller.as_mut() {
                        if let Err(e) = c.handle(action) {
                            sender.send(Event::Error(e));
                        }
                        c.publish();
                        if c.maybe_close() {
                            sender.send(Event::Idle);
                            break;
                        }
                    } else if matches!(action, Action::Close) {
                        sender.send(Event::SafeToClose);
                    }
                    sender.send(Event::Idle);
                }
                Ok(Input::RunDone(result)) => {
                    if let Some(c) = controller.as_mut() {
                        c.snapshot_generation += 1;
                        c.run = None;
                        c.view.running = None;
                        match result {
                            Ok(v) => {
                                c.view.session_status = v;
                                sender.send(Event::Notice("Session finished. Its recorded outcome and results are available below.".into()));
                            }
                            Err(e) => sender.send(Event::Error(e)),
                        }
                        if let Err(e) = c.refresh() {
                            sender.send(Event::Error(e));
                        }
                        c.publish();
                        if c.maybe_close() {
                            break;
                        }
                    }
                }
                Ok(Input::SnapshotDone { job, generation, result }) => {
                    if let Some(c) = controller.as_mut() {
                        if c.snapshot_job.as_ref().is_some_and(|(id, _)| *id == job) {
                            c.snapshot_job = None;
                            c.view.refreshing = false;
                            let mut publish = c.snapshot_loading_published;
                            if generation == c.snapshot_generation {
                                let result = result.and_then(|value| c.apply_snapshot(value));
                                match result {
                                    Err(error) => {
                                        if c.last_refresh_error.as_ref() != Some(&error) {
                                            sender.send(Event::Error(error.clone()));
                                        }
                                        c.last_refresh_error = Some(error);
                                    }
                                    Ok(changed) => {
                                        publish |= changed;
                                        c.last_refresh_error = None;
                                    }
                                }
                            }
                            if std::mem::take(&mut c.snapshot_pending) {
                                if let Err(error) = c.refresh() { sender.send(Event::Error(error)); }
                                publish = true;
                            }
                            if publish { c.publish(); }
                            next = Instant::now() + Duration::from_secs(1);
                        }
                    }
                }
                Ok(Input::LoginDone(result)) => {
                    if let Some(c) = controller.as_mut() {
                        c.login = None;
                        c.view.login = false;
                        match result {
                            Ok(v) => {
                                c.view.account = v;
                                if !c.view.closing {
                                    if let Err(e) = c.metadata() {
                                        sender.send(Event::Error(e));
                                    }
                                }
                            }
                            Err(e) => sender.send(Event::Error(e)),
                        }
                        c.publish();
                        if c.maybe_close() {
                            break;
                        }
                    }
                }
                Ok(Input::MetadataDone {
                    job,
                    provider,
                    account,
                    models,
                }) => {
                    if let Some(c) = controller.as_mut() {
                        if c.metadata.as_ref().is_some_and(|(id, _)| *id == job) {
                            c.metadata = None;
                            c.view.loading_models = false;
                            if !c.view.closing {
                                if let Some(provider) = provider {
                                    c.view.provider_status.insert(provider, match &models {
                                        Ok(_) => "Connection check passed · model list loaded. This check does not test inference.".into(),
                                        Err(error) => error.clone(),
                                    });
                                    if let Ok(catalogue) = &models { c.view.provider_catalogs.insert(provider, catalogue.clone()); }
                                }
                                match account {
                                    Ok(v) if provider == c.view.api_provider => c.view.account = v,
                                    Ok(_) => (),
                                    Err(e) => sender.send(Event::Notice(e)),
                                }
                                match models {
                                    Ok(v) if provider == c.view.api_provider => c.view.models = v,
                                    Ok(_) => (),
                                    Err(e) => sender.send(Event::Notice(e)),
                                }
                            }
                            c.publish();
                            if c.maybe_close() {
                                break;
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= next {
                if let Some(c) = controller.as_mut().filter(|c|
                    (c.run.is_some() || c.j.read_only) && c.snapshot_job.is_none()) {
                    if let Err(e) = c.refresh() {
                        if c.last_refresh_error.as_deref() != Some(&e) {
                            sender.send(Event::Error(e.clone()));
                            c.last_refresh_error = Some(e);
                        }
                    }
                }
                next = Instant::now() + Duration::from_secs(1);
            }
        }
    });
    (tx, output)
}
struct Controller {
    j: Journal,
    h: Helpers,
    view: View,
    events: Sender,
    internal: mpsc::Sender<Input>,
    run: Option<Cancel>,
    login: Option<Cancel>,
    metadata: Option<(String, Cancel)>,
    last_refresh_error: Option<String>,
    snapshot_job: Option<(String, Cancel)>,
    snapshot_generation: u64,
    snapshot_pending: bool,
    conversation_requested: Option<String>,
    snapshot_digest: Option<String>,
    snapshot_loading_published: bool,
    close_ready: bool,
    provider_config: providers::Configuration,
    credentials: providers::Credentials,
}
impl Controller {
    fn open(
        root: PathBuf,
        helpers: Option<PathBuf>,
        events: Sender,
        internal: mpsc::Sender<Input>,
    ) -> Result<Self> {
        let j = Journal::open(&journal::expand(&root))?;
        let existing = j.data.runtime.as_ref()
            .filter(|r| PathBuf::from(&r.path).join("state.sqlite3").is_file());
        let inspection = existing.map(|r| -> Result<Vec<String>> {
            Ok(vec!["--state-root".into(), r.path.clone(), "--operator".into(),
                j.data.operator.clone().ok_or("The workspace has no saved operator.")?,
                "desktop-snapshot-server".into()])
        }).transpose()?;
        let existing = existing.is_some();
        let h = Helpers::discover(helpers.as_deref(), j.data.installation.as_ref(), inspection.as_deref())?;
        let codex = helpers::codex(j.data.codex_auth.as_ref());
        let native = j.data.codex_auth.is_none() && (j.data.vault.is_some() || codex.is_none());
        let provider_config = providers::Configuration::load(&j)?;
        let credentials = providers::Credentials::load(&j)?;
        let view = View {
            api_provider: provider_config.selected,
            configured_providers: credentials.configured(),
            provider_defaults: provider_config.defaults.clone(),
            root: j.root.clone(),
            codex,
            native,
            read_only: j.read_only,
            ready: existing,
            progress: Some(j.data.clone()),
            ..View::default()
        };
        if let Some(raw) = j.read("desktop-window.json")? {
            if let Ok(p) = serde_json::from_slice::<Preferences>(&raw) {
                events.send(Event::Preferences(p));
            }
        }
        let mut c = Self {
            j,
            h,
            view,
            events,
            internal,
            run: None,
            login: None,
            metadata: None,
            last_refresh_error: None,
            snapshot_job: None,
            snapshot_generation: 0,
            snapshot_pending: false,
            conversation_requested: None,
            snapshot_digest: None,
            snapshot_loading_published: false,
            close_ready: true,
            provider_config,
            credentials,
        };
        if !c.j.read_only {
            c.j.data.installation = Some(c.h.installation.clone());
            c.j.save()?;
        }
        if !existing {
            if let Some(doc) = &c.j.data.settings {
                c.view.saved_settings = c.j.document(doc)?.1;
            }
            if let Some(doc) = &c.j.data.bootstrap {
                let (path, _) = c.j.document(doc)?;
                c.view.preview = c.session(vec![
                    "bootstrap".into(),
                    path.to_string_lossy().into_owned(),
                    "--preview".into(),
                ])?;
            }
        }
        Ok(c)
    }
    fn publish(&mut self) {
        self.snapshot_loading_published = self.view.refreshing;
        self.view.api_provider = self.provider_config.selected;
        self.view.configured_providers = self.credentials.configured();
        self.view.provider_defaults = self.provider_config.defaults.clone();
        self.view.progress = Some(self.j.data.clone());
        self.view.pending_control = self.j.pending_control().ok().flatten().is_some();
        self.view.pending_provider_change = self.provider_change_pending().unwrap_or(true);
        self.events.send(Event::View(Box::new(self.view.clone())));
    }
    fn session_args(&self, args: Vec<String>) -> Result<Vec<String>> {
        let r = self
            .j
            .data
            .runtime
            .as_ref()
            .ok_or("Choose or create a workspace first.")?;
        let op = self
            .j
            .data
            .operator
            .as_ref()
            .ok_or("Choose an operator in workspace setup.")?;
        let mut all = vec![
            "--state-root".into(),
            r.path.clone(),
            "--operator".into(),
            op.clone(),
        ];
        all.extend(args);
        Ok(all)
    }
    fn session(&self, args: Vec<String>) -> Result<Value> {
        self.h.call("personas-session", &self.session_args(args)?)
    }
    fn route_args(&self, command: &str) -> Result<Vec<String>> {
        if self.view.native {
            Ok(vec![
                "--vault".into(),
                self.j.data.vault.clone().unwrap_or_else(|| {
                    self.j
                        .root
                        .join("personas-account.json")
                        .to_string_lossy()
                        .into_owned()
                }),
                command.into(),
            ])
        } else {
            let c = self.view.codex.as_ref().ok_or(
                "Codex was not found. Locate it under Advanced, or choose native browser sign-in.",
            )?;
            let mut args = vec![format!("{command}-codex")];
            args.extend(helpers::codex_args(c));
            Ok(args)
        }
    }
    fn metadata(&mut self) -> Result<()> {
        if let Some(provider) = self.provider_config.selected { return self.provider_metadata(provider); }
        if let Some((_, cancel)) = self.metadata.take() {
            cancel.store(true, Ordering::Release);
        }
        let args = self.route_args(if self.view.native {
            "status"
        } else {
            "account"
        })?;
        let model_args = self.route_args("catalogue")?;
        let name = if self.view.native {
            "personas-auth"
        } else {
            "personas-provider"
        };
        let h = self.h.clone();
        let tx = self.internal.clone();
        let job = journal::id("catalogue")?;
        let token = helpers::cancel();
        self.metadata = Some((job.clone(), token.clone()));
        self.view.loading_models = true;
        thread::spawn(move || {
            let account = h.call_events(name, &args, Some((token.clone(), libc::SIGINT)), |_| {});
            let models = if token.load(Ordering::Acquire) {
                Err("Model loading cancelled.".into())
            } else {
                h.call_events(
                    "personas-provider",
                    &model_args,
                    Some((token, libc::SIGINT)),
                    |_| {},
                )
            };
            let _ = tx.send(Input::MetadataDone {
                job,
                provider: None,
                account,
                models,
            });
        });
        Ok(())
    }
    fn provider_metadata(&mut self, provider: ApiProvider) -> Result<()> {
        if let Some((_, cancel)) = self.metadata.take() { cancel.store(true, Ordering::Release); }
        self.view.loading_models = false;
        self.view.provider_catalogs.remove(&provider);
        if self.provider_config.selected == Some(provider) { self.view.models = Value::Null; self.view.account = Value::Null; }
        let key = self.credentials.key(provider)?;
        let job = journal::id("provider-catalogue")?;
        let token = helpers::cancel();
        self.metadata = Some((job.clone(), token.clone()));
        self.view.loading_models = true;
        self.view.provider_status.insert(provider, "Checking connection…".into());
        let tx = self.internal.clone();
        thread::spawn(move || {
            let models = providers::catalogue(provider, key, token);
            let account = Ok(json!({"connected":models.is_ok(),"provider":provider}));
            let _ = tx.send(Input::MetadataDone { job, provider:Some(provider), account, models });
        });
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        if self.snapshot_job.is_some() {
            self.snapshot_pending = true;
            return Ok(());
        }
        if self.j.read_only {
            self.j.reload()?;
        }
        if self.j.data.runtime.is_none() || self.j.data.operator.is_none() {
            return Ok(());
        }
        if self.j.data.runtime.as_ref().is_some_and(|r| r.owned) && !self.j.data.setup_authorized {
            return Ok(());
        }
        let conversation = self.view.selected.as_deref().is_some_and(|work|
            self.conversation_requested.as_deref() == Some(work)
                || self.view.conversation["work"].as_str() == Some(work));
        let request = json!({"kind":"snapshot","work":self.view.selected,
            "session":self.j.data.session.as_ref().map(|session| &session.id),"conversation":conversation});
        let args = self.session_args(vec!["desktop-snapshot-server".into()])?;
        let job = journal::id("snapshot")?;
        let token = helpers::cancel();
        self.snapshot_job = Some((job.clone(), token.clone()));
        self.view.refreshing = true;
        let generation = self.snapshot_generation;
        let helpers = self.h.clone();
        let tx = self.internal.clone();
        thread::spawn(move || {
            let result = helpers.snapshot(&args, &request, &token);
            let _ = tx.send(Input::SnapshotDone { job, generation, result });
        });
        Ok(())
    }
    fn apply_snapshot(&mut self, mut snapshot: Value) -> Result<bool> {
        journal::require(snapshot["snapshot_atomic"] == true
            && snapshot["inspection"]["works"].is_array(), "The desktop snapshot is invalid.")?;
        // Validate the entire reply before replacing any visible panel.
        if let Some(session) = &self.j.data.session {
            let status = &snapshot["session_status"];
            journal::require(status["status"] == "not_recorded"
                || (status["claim"]["request"]
                    == json!({"id":session.id,"operator":self.j.data.operator,"work":session.work,"route":session.route})
                    && status["claim"]["launch"] == session.expected_launch),
                "The saved session binding changed. No request was replayed.")?;
        }
        // A polling timestamp alone changes no displayed work. Keep checking
        // full fresh snapshots, but avoid waking the renderer for identical data.
        let inspected_at = snapshot["inspection"].as_object_mut().unwrap().remove("inspected_at_unix");
        let digest = journal::digest(&serde_json::to_vec(&snapshot)
            .map_err(|_| "Could not identify the desktop snapshot.")?);
        if let Some(at) = inspected_at { snapshot["inspection"]["inspected_at_unix"] = at; }
        let changed = self.snapshot_digest.as_ref() != Some(&digest);
        self.view.inspection = snapshot["inspection"].clone();
        self.view.saved_settings = self.view.inspection["settings"].clone();
        self.view.ready = true;
        self.view.selected = snapshot["selected"].as_str().map(str::to_owned);
        if let Some(work) = &self.view.selected {
            self.view.result = snapshot["result"].take().into();
            let messages: Vec<_> = self.view.result["messages"].as_array().into_iter().flatten()
                .filter(|m| m["file_references"].as_array().is_some_and(|v| !v.is_empty())
                    || m["history_references"].as_array().is_some_and(|v| !v.is_empty()))
                .map(|m| json!([m["id"], m["file_references"], m["history_references"]])).collect();
            let deliveries: Vec<_> = self.view.result["local_deliveries"].as_array()
                .into_iter().flatten().map(|d| &d["receipt"]).collect();
            let revision = journal::digest(&serde_json::to_vec(&json!([messages, deliveries]))
                .map_err(|_| "Could not identify the recorded file list.")?);
            self.view.files = Some(self.h.result_files(
                self.session_args(vec!["result".into(), work.clone()])?, revision));
            self.view.questions = snapshot["questions"].clone();
            if !snapshot["conversation"].is_null() {
                self.view.conversation = snapshot["conversation"].take().into();
            }
        }
        if self.j.data.session.is_some() {
            self.view.session_status = snapshot["session_status"].take();
        }
        self.snapshot_digest = Some(digest);
        Ok(changed)
    }
    fn current(&self, work: &str) -> Result<(Value, Value)> {
        // Ask the persistent reader for fresh state, rather than starting
        // another full history decode for a control or run preflight. The
        // session status and task references come from the same transaction.
        let snapshot = self.h.snapshot(
            &self.session_args(vec!["desktop-snapshot-server".into()])?,
            &json!({"kind":"control","session":self.j.data.session.as_ref().map(|s| &s.id)}),
            &helpers::cancel(),
        )?;
        journal::require(snapshot["snapshot_atomic"] == true,
            "The desktop snapshot is invalid.")?;
        let current = snapshot["inspection"]["works"].as_array()
            .and_then(|works| works.iter().find(|item| item["id"] == work))
            .ok_or("This task is unavailable in the selected workspace.")?.clone();
        Ok((current, snapshot))
    }
    fn handle(&mut self, action: Action) -> Result<()> {
        // A snapshot started before an action must not overwrite its newer view
        // or control result. The read worker never owns the action queue.
        self.snapshot_generation += 1;
        self.snapshot_digest = None;
        if self.snapshot_job.is_some() { self.snapshot_pending = true; }
        if self.j.read_only {
            journal::require(matches!(&action,
                Action::Refresh | Action::Select(_) | Action::SelectSession(_)
                | Action::Models | Action::Development(_) | Action::Conversation(_)
                | Action::Export { .. } | Action::Preferences(_) | Action::Close
                | Action::CancelClose | Action::AmendSnapshot(_)),
                "This window is viewing live progress. Close the controlling launcher and reopen this workspace to use its controls.")?;
        }
        match action {
            Action::SaveProvider { provider, key, selection } => {
                journal::require(self.run.is_none() && self.login.is_none(), "Pause current work before changing provider settings.")?;
                if !key.is_empty() {
                    self.credentials.save_key(&self.j, provider, key)?;
                    if let Some((_, token)) = self.metadata.take() { token.store(true, Ordering::Release); }
                    self.view.loading_models = false;
                    self.view.provider_catalogs.remove(&provider);
                    if self.provider_config.selected == Some(provider) { self.view.models = Value::Null; self.view.account = Value::Null; }
                }
                self.provider_config.defaults.insert(provider, selection);
                self.provider_config.save(&self.j)?;
                self.view.provider_status.insert(provider, "Saved locally; connection not yet checked".into());
                self.events.send(Event::Notice(format!("{} settings saved.", provider.label())));
            }
            Action::RemoveProvider(provider) => {
                journal::require(self.run.is_none(), "Pause current work before removing a provider key.")?;
                self.credentials.remove(&self.j, provider)?;
                if let Some((_, token)) = self.metadata.take() { token.store(true, Ordering::Release); }
                self.view.loading_models = false;
                self.view.provider_status.remove(&provider);
                self.view.provider_catalogs.remove(&provider);
                if self.provider_config.selected == Some(provider) {
                    self.view.account = Value::Null; self.view.models = Value::Null;
                }
            }
            Action::CheckProvider(provider) => { self.provider_metadata(provider)?; }
            Action::SelectProvider(provider) => {
                journal::require(self.run.is_none() && self.login.is_none(), "Finish the active operation before changing provider.")?;
                journal::require(self.j.data.settings.is_none() && self.j.data.bootstrap.is_none(), "This workspace has retained provider selections. Use its original providers or create a new workspace.")?;
                self.provider_config.selected = provider;
                self.provider_config.save(&self.j)?;
                self.view.api_provider = provider;
                self.view.account = Value::Null;
                self.view.models = provider.and_then(|p| self.view.provider_catalogs.get(&p).cloned()).unwrap_or(Value::Null);
                if provider.is_none() || provider.is_some_and(|p| self.credentials.configured().contains(&p)) { self.metadata()?; }
            }
            Action::ApplyProvider { provider, selection, preserve_models, effort_override } => {
                self.change_provider(provider, selection, preserve_models, effort_override)?;
            }
            Action::FinishProviderChange => self.finish_provider_change()?,
            Action::Open { .. } | Action::Discover(_) => unreachable!(),
            Action::Refresh => {
                self.refresh()?;
            }
            Action::Select(work) => {
                // The task opens on its conversation. Include that panel in
                // this same snapshot, instead of another full history decode.
                self.conversation_requested = Some(work.clone());
                self.view.selected = Some(work);
                self.view.result = Value::Null.into();
                self.view.conversation = Value::Null.into();
                self.view.questions = Value::Null;
                self.view.files = None;
                self.view.selected_session = None;
                self.view.session_detail = Value::Null;
                // Discard an obsolete reply by generation, while preserving
                // the reader's decoded history for the pending selection.
                self.refresh()?;
            }
            Action::Conversation(work) => {
                journal::require(
                    self.view.selected.as_ref() == Some(&work),
                    "Select this task first.",
                )?;
                self.conversation_requested = Some(work);
                self.refresh()?;
            }
            Action::SelectSession(id) => {
                let session = self
                    .j
                    .data
                    .session_history
                    .iter()
                    .chain(self.j.data.session.iter())
                    .find(|s| s.id == id && Some(&s.work) == self.view.selected.as_ref())
                    .ok_or("This session is not part of the selected task.")?;
                let detail =
                    self.session(vec!["session-messages".into(), "--id".into(), id.clone()])?;
                let status = &detail["session"];
                if status["status"] != "not_recorded" {
                    journal::require(
                        status["claim"]["request"]
                            == json!({"id":session.id,"operator":self.j.data.operator,"work":session.work,"route":session.route})
                            && status["claim"]["launch"] == session.expected_launch,
                        "The saved session binding changed. No request was replayed.",
                    )?;
                }
                self.view.selected_session = Some(id);
                self.view.session_detail = detail;
            }
            Action::Account { native, codex } => {
                journal::require(self.provider_config.selected.is_none(), "Choose ChatGPT sign-in in provider settings first.")?;
                journal::require(
                    self.login.is_none(),
                    "Finish or cancel the current sign-in first.",
                )?;
                if self.j.data.settings.is_some() || self.j.data.bootstrap.is_some() {
                    journal::require(native==self.view.native,"An existing workspace is bound to its account route. Use its original account.")?;
                }
                if !native {
                    journal::require(codex.is_some(), "Select a Codex executable first.")?;
                }
                self.view.native = native;
                self.view.codex = codex;
                self.j.data.codex_auth = if native {
                    None
                } else {
                    self.view.codex.clone()
                };
                if native && self.j.data.vault.is_none() {
                    self.j.data.vault = Some(
                        self.j
                            .root
                            .join("personas-account.json")
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
                self.j.save()?;
                self.view.account = Value::Null;
                self.view.models = Value::Null;
                self.metadata()?;
            }
            Action::Models => {
                self.metadata()?;
            }
            Action::SignIn { device } => {
                journal::require(self.provider_config.selected.is_none(), "Choose ChatGPT sign-in before signing in.")?;
                journal::require(
                    self.login.is_none() && self.run.is_none(),
                    "Another operation is still active.",
                )?;
                if let Some((_, token)) = self.metadata.take() {
                    token.store(true, Ordering::Release);
                    self.view.loading_models = false;
                }
                let native = self.view.native;
                let mut args = self.route_args(if native { "sign-in" } else { "account" })?;
                if native {
                    args.push("--json-events".into());
                    self.j.data.vault = Some(args[1].clone());
                } else {
                    args.push("--sign-in".into());
                    if device {
                        args.push("--device-code".into());
                    }
                    self.j.data.codex_auth = self.view.codex.clone();
                }
                self.j.save()?;
                let token = helpers::cancel();
                self.login = Some(token.clone());
                self.view.login = true;
                let h = self.h.clone();
                let tx = self.internal.clone();
                let e = self.events.clone();
                thread::spawn(move || {
                    let result = h.call_events(
                        if native {
                            "personas-auth"
                        } else {
                            "personas-provider"
                        },
                        &args,
                        Some((token, libc::SIGINT)),
                        move |v| {
                            if let Some(url) = v["url"].as_str() {
                                e.send(Event::Authorization {
                                    url: url.into(),
                                    code: v["user_code"].as_str().map(str::to_owned),
                                });
                            }
                        },
                    );
                    let _ = tx.send(Input::LoginDone(result));
                });
            }
            Action::CancelLogin => {
                if let Some(c) = &self.login {
                    c.store(true, Ordering::Release);
                }
            }
            Action::Prepare(setup) => self.prepare(setup)?,
            Action::EditSetup => {
                journal::require(!self.j.data.setup_authorized, "This setup was already submitted. Retry its saved request to confirm the outcome.")?;
                // Preview never dispatches a bootstrap. Keep its immutable file
                // as history while allowing these unsubmitted choices to change.
                self.j.data.bootstrap = None;
                self.j.save()?;
                self.view.preview = Value::Null;
            }
            Action::Create => {
                let doc = self
                    .j
                    .data
                    .bootstrap
                    .as_ref()
                    .ok_or("Review the workspace setup first.")?;
                let (path, _) = self.j.document(doc)?;
                self.j.data.setup_authorized = true;
                self.j.save()?;
                self.session(vec![
                    "bootstrap".into(),
                    path.to_string_lossy().into_owned(),
                ])?;
                self.view.preview = Value::Null;
                self.refresh()?;
            }
            Action::Attach { root, operator } => {
                journal::require(
                    self.j.data.bootstrap.is_none() && self.run.is_none(),
                    "Use another progress folder to open a different runtime.",
                )?;
                journal::require(
                    journal::identifier(&operator),
                    "Operator names use letters, numbers, hyphens, or underscores.",
                )?;
                let root = std::fs::canonicalize(root).map_err(journal::io)?;
                self.h.call(
                    "personas-session",
                    &vec![
                        "--state-root".into(),
                        root.to_string_lossy().into_owned(),
                        "--operator".into(),
                        operator.clone(),
                        "inspect".into(),
                    ],
                )?;
                self.j.data.operator = Some(operator);
                self.j.data.runtime = Some(Runtime {
                    path: root.to_string_lossy().into_owned(),
                    owned: false,
                });
                self.j.save()?;
                self.refresh()?;
            }
            Action::NewTask { text, optional } => {
                journal::require(
                    self.run.is_none(),
                    "Pause the running task before starting another one.",
                )?;
                journal::require(
                    !self.j.data.needs.iter().any(|n| n["work"].is_null()),
                    "Confirm the retained task submission before creating another task.",
                )?;
                if !optional {
                    journal::require(!text.trim().is_empty(), "Enter a task first.")?;
                }
                let key = journal::id(if optional { "activity" } else { "need" })?;
                self.j.data.needs.push(if optional{json!({"id":key,"kind":"optional_activity","work":null})}else{json!({"id":key,"text":text,"require_acceptance":false,"shared":true,"work":null})});
                self.j.save()?;
                let work = self.deliver_need(&key)?;
                self.view.selected = Some(work.clone());
                self.refresh()?;
                self.run_work(work)?;
            }
            Action::ConfirmPendingNeed(id) => {
                let work = self.deliver_need(&id)?;
                self.view.selected = Some(work);
                self.refresh()?;
                self.events.send(Event::Notice(
                    "Task submission confirmed. Choose Start / Continue when ready.".into(),
                ));
            }
            Action::Run(work) => self.run_work(work)?,
            Action::RecoverWork(work) => {
                journal::require(self.run.is_none(), "Wait for the running session to finish before recovery.")?;
                let receipt = self.session(vec!["recover-work".into(), work])?;
                journal::require(receipt["inference_performed"] == false
                    && receipt["replay_authorized"] == false && receipt["work_resumed"] == false,
                    "The recovery receipt is incomplete. Inspect the retained task state.")?;
                self.refresh()?;
                self.events.send(Event::Notice(format!(
                    "Recovered local status: {} abandoned calls released, {} calls still held. Existing stops and pauses remain; remote outcomes and missing usage remain unresolved.",
                    receipt["released_local_calls"].as_array().map_or(0, Vec::len),
                    receipt["still_held_calls"].as_array().map_or(0, Vec::len),
                )));
            }
            Action::Control { work, action } => {
                self.control(&work, &action)?;
                self.refresh()?;
            }
            Action::ResumePersona { work, persona } => {
                let (current, snapshot) = self.current(&work)?;
                let page = &snapshot["inspection"];
                journal::require(
                    current["participants"].as_array().is_some_and(|v| {
                        v.iter()
                            .any(|p| p["persona"] == persona && p["resume_candidate"] == true)
                    }),
                    "This participant cannot currently be resumed. Refresh to inspect its status.",
                )?;
                self.input("resume-participant",json!({"id":journal::id("resume")?,"operator":self.j.data.operator,"work":work,"persona":persona,"expected_runtime":page["runtime"],"expected_launch":page["launch"],"authorize_resume":true}))?;
            }
            Action::Feedback {
                work,
                persona,
                text,
                supersedes,
            } => {
                journal::require(!text.trim().is_empty(), "Enter feedback first.")?;
                self.input("feedback",json!({"id":journal::id("feedback")?,"work":work,"persona":persona,"text":text,"supersedes":supersedes}))?;
            }
            Action::Answer {
                work,
                question,
                text,
                basis,
            } => {
                journal::require(
                    !text.trim().is_empty() && !basis.trim().is_empty(),
                    "Provide an answer and its basis.",
                )?;
                let target = question["question_work"].as_str().unwrap_or(&work);
                self.input("answer-question",json!({"work":target,"reply":{"id":journal::id("answer")?,"question":question["question"]["id"],"question_work":target,"question_revision":question["question_revision"],"text":text,"basis":basis}}))?;
            }
            Action::AmendSnapshot(work) => {
                self.view.amendment = self.session(vec!["amend-work-snapshot".into(), work])?;
            }
            Action::Amend { snapshot, scope } => {
                let key = journal::id("amend")?;
                let actors = snapshot["participants"]
                    .as_array()
                    .ok_or("The amendment snapshot is unavailable.")?;
                let invitations:Vec<_>=actors.iter().enumerate().map(|(i,p)|json!({"id":format!("{key}-{i}"),"to":p,"terms":{"mandate_revision":snapshot["next_mandate_revision"],"exact_terms":"Invitation to consider the amended mandate voluntarily; responsibility requires fresh persona acceptance.","outcomes":[],"review_roles":[]}})).collect();
                self.input("amend-work",json!({"id":key,"operator":self.j.data.operator,"work":snapshot["work"],"scope":scope,"invitations":invitations,"resume":actors,"expected_runtime":snapshot["runtime"],"expected_launch":snapshot["launch"],"authorize_amend":true}))?;
                self.view.amendment = Value::Null;
            }
            Action::ConfirmPendingInput => self.deliver_input()?,
            Action::ConfirmPendingControl => {
                self.deliver_control()?;
                self.refresh()?;
            }
            Action::ArchiveSession => {
                journal::require(
                    self.run.is_none(),
                    "Wait for the running session to finish recording.",
                )?;
                self.refresh()?;
                if let Some(s) = self.j.data.session.take() {
                    self.j.data.session_history.push(s);
                    self.j.save()?;
                }
                self.view.session_status = Value::Null;
                self.events.send(Event::Notice("The earlier session remains in history. A new session will have its own identity; the earlier outcome remains unresolved.".into()));
            }
            Action::Development(persona) => {
                self.view.development = Value::Null;
                self.view.development = self.session(vec!["development".into(), persona])?;
            }
            Action::Export { work, parent } => {
                let dest = parent.join(journal::id("personas-export")?);
                let value = self.session(vec![
                    "result".into(),
                    work,
                    "--export-directory".into(),
                    dest.to_string_lossy().into_owned(),
                ])?;
                self.events
                    .send(Event::Notice(if value["status"] == "no_exportable_files" {
                        "There are no exportable files in this task yet.".into()
                    } else {
                        format!(
                            "Exported files and their source records to {}",
                            dest.display()
                        )
                    }));
            }
            Action::Preferences(p) => {
                if !self.j.read_only {
                    self.j.write("desktop-window.json", &p)?;
                }
            }
            Action::Close => {
                self.view.closing = true;
                self.close_ready = false;
                if let Some((_, token)) = &self.metadata {
                    token.store(true, Ordering::Release);
                }
                if let Some(token) = &self.login {
                    token.store(true, Ordering::Release);
                }
                if let Some(work) = self.view.running.clone() {
                    self.control(&work, "pause")?;
                }
                self.close_ready = true;
            }
            Action::CancelClose => {
                self.view.closing = false;
                self.close_ready = false;
            }
        }
        Ok(())
    }
    fn prepare(&mut self, s: Setup) -> Result<()> {
        journal::require(!self.j.data.setup_authorized&&self.j.data.bootstrap.is_none(),"A workspace setup is already retained. Create it with the saved choices, or use a new progress folder.")?;
        journal::require(
            journal::identifier(&s.operator),
            "Operator names use letters, numbers, hyphens, or underscores.",
        )?;
        journal::require(
            !s.personas.is_empty()
                && s.personas
                    .iter()
                    .all(|p| !p.name.trim().is_empty() && !p.profile.trim().is_empty()),
            "Give each persona a name and a starting profile.",
        )?;
        journal::require(
            [&s.model, &s.effort]
                .iter()
                .all(|s| !s.is_empty() && !s.chars().any(char::is_whitespace)),
            "Choose a model and reasoning effort.",
        )?;
        let mut args = Vec::new();
        let settings = if let Some(provider) = self.provider_config.selected {
            let max_output_tokens = self.provider_config.defaults.get(&provider).and_then(|s| s.max_output_tokens);
            serde_json::to_value(self.credentials.settings(provider, &Selection { model:s.model.clone(), effort:s.effort.clone(), max_output_tokens })?).map_err(|_| "Could not serialize provider settings.")?
        } else {
        args = self.route_args("configure")?;
        args.extend([
            "--model".into(),
            s.model.clone(),
            "--effort".into(),
            s.effort.clone(),
        ]);
        self.h.call("personas-provider", &args)?
        };
        self.j.data.operator = Some(s.operator.clone());
        if self.j.data.runtime.is_none() {
            self.j.data.runtime = Some(Runtime {
                path: self.j.root.join("runtime").to_string_lossy().into_owned(),
                owned: true,
            });
        }
        if self.provider_config.selected.is_some() {
            self.j.data.codex_auth = None;
            self.j.data.vault = None;
        } else if self.view.native {
            self.j.data.codex_auth = None;
            self.j.data.vault = Some(args[1].clone());
        } else {
            self.j.data.codex_auth = self.view.codex.clone();
        }
        self.j.keep("settings", &settings)?;
        let workspace = journal::expand(&PathBuf::from(&s.workspace));
        std::fs::create_dir_all(&workspace).map_err(journal::io)?;
        let workspace = std::fs::canonicalize(workspace).map_err(journal::io)?;
        let mut seeds = serde_json::Map::new();
        for (i, p) in s.personas.iter().enumerate() {
            let mut seed = json!({"label":p.name,"text":p.profile,"provenance":format!("Starting profile supplied by {} during setup; no prior experience asserted.",s.operator)});
            if !p.model.is_empty() {
                seed["model"] = p.model.clone().into();
                seed["effort"] = p.effort.clone().into();
            }
            if let Some(provider) = p.provider.or_else(|| p.max_output_tokens.and(self.provider_config.selected)) {
                journal::require(self.provider_config.selected.is_some(), "Choose an API-key workspace to mix providers.")?;
                let defaults = self.provider_config.defaults.get(&provider).cloned().unwrap_or_default();
                let selection = Selection {
                    model: if p.model.is_empty() { defaults.model } else { p.model.clone() },
                    effort: if p.effort.is_empty() { defaults.effort } else { p.effort.clone() },
                    max_output_tokens:p.max_output_tokens.or(defaults.max_output_tokens),
                };
                seed["provider"] = serde_json::to_value(self.credentials.settings(provider, &selection)?).map_err(|_| "Could not serialize persona provider.")?;
            }
            seeds.insert(format!("persona-{:04}", i + 1), seed);
        }
        let request = json!({"id":journal::id("bootstrap")?,"operator":s.operator,"persona_seeds":seeds,
            "setup":{"cohort":s.personas.len(),"destination":self.provider_config.selected.map_or("openai", |p| p.mode().destination()),"provider":settings,"working_directory":workspace}});
        // Validate before retaining the immutable bootstrap; a typo should remain editable.
        self.j.write("setup-preview.json", &request)?;
        self.view.preview = self.session(vec![
            "bootstrap".into(),
            self.j
                .root
                .join("setup-preview.json")
                .to_string_lossy()
                .into_owned(),
            "--preview".into(),
        ])?;
        self.j.keep("bootstrap", &request)?;
        Ok(())
    }
    fn deliver_need(&mut self, id: &str) -> Result<String> {
        let i = self
            .j
            .data
            .needs
            .iter()
            .position(|n| n["id"] == id)
            .ok_or("The retained task request is missing.")?;
        let n = &self.j.data.needs[i];
        if let Some(work) = n["work"].as_str() {
            return Ok(work.into());
        }
        let optional = n["kind"] == "optional_activity";
        let mut args = vec![
            if optional {
                "optional-activity".into()
            } else {
                "intake".into()
            },
            "--id".into(),
            id.into(),
        ];
        if !optional {
            if n["require_acceptance"] == true {
                args.push("--require-acceptance".into());
            }
            args.extend([
                "--".into(),
                n["text"]
                    .as_str()
                    .ok_or("Saved task text is missing.")?
                    .into(),
            ]);
        }
        let receipt = self.session(args)?;
        let work = if optional {
            receipt["work"].as_str()
        } else {
            receipt["receipt"]["work"].as_str()
        }
        .ok_or("The task receipt is incomplete; its request ID remains saved.")?
        .to_owned();
        journal::require(
            if optional {
                receipt["trigger"] == id && receipt["request"]["id"] == id
            } else {
                receipt["receipt"]["id"] == id
            },
            "Task receipt identity differs; its request ID remains saved.",
        )?;
        self.j.data.needs[i]["work"] = work.clone().into();
        if !optional {
            self.j.data.needs[i]["shared"] = true.into();
        }
        self.j.save()?;
        Ok(work)
    }
    fn run_work(&mut self, work: String) -> Result<()> {
        journal::require(
            self.run.is_none() && self.login.is_none(),
            "Another operation is active.",
        )?;
        let (current, snapshot) = self.current(&work)?;
        let page = &snapshot["inspection"];
        if self.j.data.session.is_some() {
            let status = &snapshot["session_status"];
            journal::require(status["status"]=="finished","The previous session has no recorded outcome. Inspect it and retain it in history before explicitly starting a new session.")?;
        }
        journal::require(
            current["phase"] == "open",
            "This task is paused or ended. Use Unpause task for a pause, then Start / Continue.",
        )?;
        journal::require(
            current["participants"].as_array().is_some_and(|v| {
                v.iter().any(|p| {
                    p["eligible_candidate"] == true
                        || p["continuation_recovery_candidate"] == true
                        || p["scheduled_for_unix"].is_number()
                })
            }),
            "No participant can currently continue. Review their status or use Resume participant.",
        )?;
        let settings = &page["settings"];
        journal::require(!self.provider_change_pending()?, "Finish the saved provider change in Settings before running personas.")?;
        let api_key = providers::ApiProvider::ALL.into_iter().any(|p| serde_json::to_value(p.mode()).ok().as_ref() == Some(&settings["mode"]));
        let mut environment = std::collections::BTreeMap::new();
        let route = if api_key {
            let required: Vec<ai_personas::provider::DirectSettings> = serde_json::from_value(settings["api_key_settings"].clone())
                .map_err(|_| "Current provider credential references are unavailable. Use matching runtime helpers.")?;
            journal::require(!required.is_empty(), "Current API-key provider settings are unavailable.")?;
            environment = self.credentials.environment(&required)?;
            json!({"kind":"saved_environment"})
        } else if settings["mode"] == "siwc_oauth" {
            json!({"kind":"native_siwc","vault":self.j.data.vault.as_ref().ok_or("Select the workspace's native account.")?})
        } else {
            journal::require(
                settings["codex_auth_only_supported"] == true,
                "This workspace uses a provider route that the desktop cannot run.",
            )?;
            let c = self
                .view
                .codex
                .as_ref()
                .ok_or("Locate the workspace's Codex installation in Account settings.")?;
            json!({"kind":"codex_auth_only","executable":c.executable,"host_home":c.host_home,"codex_home":c.codex_home,"authorize_codex_auth_only":false})
        };
        if let Some(old) = self.j.data.session.take() {
            self.j.data.session_history.push(old);
        }
        let s = Session {
            id: journal::id("session")?,
            work: work.clone(),
            route,
            expected_launch: page["launch"].clone(),
        };
        self.j.data.session = Some(s.clone());
        self.j.save()?;
        self.j.write("session-launch.json", &s.expected_launch)?;
        let mut args = vec![
            "run".into(),
            "--id".into(),
            s.id.clone(),
            "--work".into(),
            work.clone(),
            "--expected-launch".into(),
            self.j
                .root
                .join("session-launch.json")
                .to_string_lossy()
                .into_owned(),
        ];
        if s.route["kind"] == "native_siwc" {
            args.extend(["--vault".into(), s.route["vault"].as_str().unwrap().into()]);
        } else if s.route["kind"] == "codex_auth_only" {
            args.extend(helpers::codex_args(self.view.codex.as_ref().unwrap()));
        }
        let args = self.session_args(args)?;
        let token = helpers::cancel();
        self.run = Some(token.clone());
        self.view.running = Some(work);
        let h = self.h.clone();
        let tx = self.internal.clone();
        thread::spawn(move || {
            let result = h.call_events_with_environment(
                "personas-session",
                &args,
                Some((token, libc::SIGTERM)),
                &environment,
                |_| {},
            );
            let _ = tx.send(Input::RunDone(result));
        });
        Ok(())
    }
    fn input(&mut self, kind: &str, request: Value) -> Result<()> {
        journal::require(
            !self
                .j
                .data
                .work_input
                .as_ref()
                .is_some_and(|v| v["delivered"] == false),
            "Confirm the retained input before submitting another one.",
        )?;
        self.j.data.work_input = Some(journal::request(kind, request));
        self.j.save()?;
        self.deliver_input()
    }
    fn deliver_input(&mut self) -> Result<()> {
        let item = self
            .j
            .data
            .work_input
            .clone()
            .filter(|v| v["delivered"] == false)
            .ok_or("No input is waiting for confirmation.")?;
        self.j.write("work-input.json", &item["request"])?;
        let kind = item["kind"]
            .as_str()
            .ok_or("The saved input kind is unavailable.")?;
        let receipt = self.session(vec![
            kind.into(),
            self.j
                .root
                .join("work-input.json")
                .to_string_lossy()
                .into_owned(),
        ])?;
        journal::require(
            receipt["work"] == item["request"]["work"] && receipt["inference_performed"] == false,
            "The input receipt did not match. Its identity remains retained.",
        )?;
        let request = &item["request"];
        journal::require(
            match kind {
                "feedback" => {
                    receipt["persona"] == request["persona"] && receipt["persona_resumed"] == false
                }
                "answer-question" => {
                    receipt["question"] == request["reply"]["question"]
                        && receipt["reply"] == request["reply"]["id"]
                        && receipt["persona_resumed"] == false
                }
                "amend-work" => {
                    receipt["id"] == request["id"]
                        && receipt["scope_saved"].is_boolean()
                        && receipt["snapshot_changed"] == !receipt["scope_saved"].as_bool().unwrap()
                }
                "resume-participant" => {
                    receipt["id"] == request["id"]
                        && receipt["persona"] == request["persona"]
                        && receipt["resume_recorded"].is_boolean()
                        && receipt["replay_authorized"] == false
                }
                _ => false,
            },
            "The input receipt did not match its saved request. Its identity remains retained.",
        )?;
        self.j.data.work_input.as_mut().unwrap()["delivered"] = true.into();
        self.j.save()?;
        self.events.send(Event::Notice(if receipt["snapshot_changed"]==true{"The task changed before this action could apply. Review the current state and submit it again.".into()}
            else if receipt["queued"]==true{"Input saved and queued for the next eligible decision.".into()}
            else {"Input saved. Choose Start / Continue when you want eligible personas to work.".into()}));
        self.refresh()
    }
    fn control(&mut self, work: &str, action: &str) -> Result<()> {
        if let Some(pending) = self.j.pending_control()? {
            self.deliver_control()?;
            if pending["request"]["work"] == work && pending["request"]["action"] == action {
                return Ok(());
            }
        }
        let mut request =
            json!({"id":journal::id("control")?,"operator":self.j.data.operator,"work":work});
        let kind = if action == "resume" {
            let (current, snapshot) = self.current(work)?;
            let page = &snapshot["inspection"];
            journal::require(current["phase"] == "paused", "The task is not paused.")?;
            request["expected_runtime"] = page["runtime"].clone();
            request["expected_launch"] = page["launch"].clone();
            "resume-work"
        } else {
            journal::require(
                matches!(action, "pause" | "cancel"),
                "Unknown task control.",
            )?;
            request["action"] = action.into();
            request["reason"] = format!(
                "{} by the requester in Personas Desktop.",
                if action == "pause" {
                    "Paused"
                } else {
                    "Canceled"
                }
            )
            .into();
            "stop-work"
        };
        self.j
            .write("work-control.json", &journal::request(kind, request))?;
        self.deliver_control()?;
        if action != "resume" && self.view.running.as_deref() == Some(work) {
            if let Some(token) = &self.run {
                token.store(true, Ordering::Release);
            }
        }
        Ok(())
    }
    fn deliver_control(&mut self) -> Result<()> {
        let mut item = self
            .j
            .pending_control()?
            .ok_or("No control is waiting for confirmation.")?;
        self.j.write("control-request.json", &item["request"])?;
        let receipt = self.session(vec![
            item["kind"]
                .as_str()
                .ok_or("The saved control is invalid.")?
                .into(),
            self.j
                .root
                .join("control-request.json")
                .to_string_lossy()
                .into_owned(),
        ])?;
        journal::require(
            receipt["request"] == item["request"]
                && receipt["inference_performed"] == false
                && receipt["control_recorded"].is_boolean(),
            "The control receipt did not match. Its identity remains retained.",
        )?;
        item["delivered"] = true.into();
        self.j.write("work-control.json", &item)?;
        if receipt["snapshot_changed"] == true {
            self.events.send(Event::Notice(
                "The task changed before unpausing. Refresh and choose Unpause task again.".into(),
            ));
        }
        Ok(())
    }
    fn maybe_close(&self) -> bool {
        let ready = self.view.closing
            && self.close_ready
            && self.run.is_none()
            && self.login.is_none()
            && self.metadata.is_none();
        if ready {
            self.events.send(Event::SafeToClose);
        }
        ready
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        if let Some((_, token)) = &self.snapshot_job {
            token.store(true, Ordering::Release);
        }
        self.h.close_snapshot_reader();
        if let Some((_, token)) = &self.metadata {
            token.store(true, Ordering::Release);
        }
    }
}
