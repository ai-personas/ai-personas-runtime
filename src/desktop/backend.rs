//! One journal-owning controller, with separate long-lived login/run workers.
//! UI actions are typed; provider execution always passes through native helpers.
use super::{
    helpers::{self, Cancel, Helpers},
    journal::{self, CodexPaths, Journal, Progress, Result, Runtime, Session},
};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PersonaDraft {
    pub name: String,
    pub profile: String,
    pub model: String,
    pub effort: String,
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
    pub saved_settings: Value,
    pub preview: Value,
    pub inspection: Value,
    pub selected: Option<String>,
    pub result: Value,
    pub questions: Value,
    pub development: Value,
    pub session_status: Value,
    pub selected_session: Option<String>,
    pub session_detail: Value,
    pub amendment: Value,
    pub running: Option<String>,
    pub login: bool,
    pub loading_models: bool,
    pub closing: bool,
    pub pending_control: bool,
    pub ready: bool,
}
pub enum Action {
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
    RunDone(Result<Value>),
    LoginDone(Result<Value>),
    MetadataDone {
        job: String,
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
                    account,
                    models,
                }) => {
                    if let Some(c) = controller.as_mut() {
                        if c.metadata.as_ref().is_some_and(|(id, _)| *id == job) {
                            c.metadata = None;
                            c.view.loading_models = false;
                            if !c.view.closing {
                                match account {
                                    Ok(v) => c.view.account = v,
                                    Err(e) => sender.send(Event::Notice(e)),
                                }
                                match models {
                                    Ok(v) => c.view.models = v,
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
                if let Some(c) = controller.as_mut().filter(|c| c.run.is_some()) {
                    if let Err(e) = c.refresh() {
                        if c.last_refresh_error.as_deref() != Some(&e) {
                            sender.send(Event::Error(e.clone()));
                            c.last_refresh_error = Some(e);
                        }
                    } else {
                        c.last_refresh_error = None;
                    }
                    c.publish();
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
    close_ready: bool,
}
impl Controller {
    fn open(
        root: PathBuf,
        helpers: Option<PathBuf>,
        events: Sender,
        internal: mpsc::Sender<Input>,
    ) -> Result<Self> {
        let mut j = Journal::open(&journal::expand(&root))?;
        let h = Helpers::discover(helpers.as_deref(), j.data.installation.as_ref())?;
        j.data.installation = Some(h.installation.clone());
        j.save()?;
        let codex = helpers::codex(j.data.codex_auth.as_ref());
        let native = j.data.codex_auth.is_none() && (j.data.vault.is_some() || codex.is_none());
        let view = View {
            root: j.root.clone(),
            codex,
            native,
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
            close_ready: true,
        };
        let existing =
            c.j.data
                .runtime
                .as_ref()
                .is_some_and(|r| PathBuf::from(&r.path).join("state.sqlite3").is_file());
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
        self.view.progress = Some(self.j.data.clone());
        self.view.pending_control = self.j.pending_control().ok().flatten().is_some();
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
                account,
                models,
            });
        });
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        if self.j.data.runtime.is_none() || self.j.data.operator.is_none() {
            return Ok(());
        }
        if self.j.data.runtime.as_ref().is_some_and(|r| r.owned) && !self.j.data.setup_authorized {
            return Ok(());
        }
        let mut after = None;
        let mut seen = std::collections::BTreeSet::new();
        let mut all = vec![];
        loop {
            let mut args = vec!["inspect".into(), "--limit".into(), "50".into()];
            if let Some(cursor) = after {
                args.extend(["--after".into(), cursor]);
            }
            let page = self.session(args)?;
            all.extend(
                page["works"]
                    .as_array()
                    .ok_or("The task list response is invalid.")?
                    .clone(),
            );
            after = page["next_after"].as_str().map(str::to_owned);
            self.view.inspection = page;
            if let Some(ref cursor) = after {
                journal::require(
                    seen.insert(cursor.clone()),
                    "The task list repeated a page.",
                )?;
            } else {
                break;
            }
        }
        self.view.inspection["works"] = all.into();
        self.view.saved_settings = self.view.inspection["settings"].clone();
        self.view.ready = true;
        if self.view.selected.is_none() {
            self.view.selected = self.view.inspection["works"]
                .as_array()
                .and_then(|v| v.first())
                .and_then(|v| v["id"].as_str())
                .map(str::to_owned);
        }
        if let Some(work) = &self.view.selected {
            self.view.result = self.session(vec!["result".into(), work.clone()])?;
            self.view.questions = self.session(vec!["questions".into(), work.clone()])?;
        }
        if let Some(s) = &self.j.data.session {
            let status =
                self.session(vec!["session-status".into(), "--id".into(), s.id.clone()])?;
            if status["status"] != "not_recorded" {
                journal::require(
                    status["claim"]["request"]
                        == json!({"id":s.id,"operator":self.j.data.operator,"work":s.work,"route":s.route})
                        && status["claim"]["launch"] == s.expected_launch,
                    "The saved session binding changed. No request was replayed.",
                )?;
            }
            self.view.session_status = status;
        }
        Ok(())
    }
    fn current(&self, work: &str) -> Result<(Value, Value)> {
        let mut after = None;
        loop {
            let mut args = vec!["inspect".into(), "--limit".into(), "50".into()];
            if let Some(c) = after {
                args.extend(["--after".into(), c]);
            }
            let page = self.session(args)?;
            if let Some(item) = page["works"]
                .as_array()
                .and_then(|v| v.iter().find(|w| w["id"] == work))
            {
                return Ok((item.clone(), page));
            }
            after = page["next_after"].as_str().map(str::to_owned);
            if after.is_none() {
                return Err("This task is unavailable in the selected workspace.".into());
            }
        }
    }
    fn handle(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Open { .. } | Action::Discover(_) => unreachable!(),
            Action::Refresh => {
                self.refresh()?;
            }
            Action::Select(work) => {
                self.view.selected = Some(work);
                self.view.result = Value::Null;
                self.view.questions = Value::Null;
                self.view.development = Value::Null;
                self.view.selected_session = None;
                self.view.session_detail = Value::Null;
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
            Action::Control { work, action } => {
                self.control(&work, &action)?;
                self.refresh()?;
            }
            Action::ResumePersona { work, persona } => {
                let (current, page) = self.current(&work)?;
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
            Action::Preferences(p) => self.j.write("desktop-window.json", &p)?,
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
        let mut args = self.route_args("configure")?;
        args.extend([
            "--model".into(),
            s.model.clone(),
            "--effort".into(),
            s.effort.clone(),
        ]);
        let settings = self.h.call("personas-provider", &args)?;
        self.j.data.operator = Some(s.operator.clone());
        if self.j.data.runtime.is_none() {
            self.j.data.runtime = Some(Runtime {
                path: self.j.root.join("runtime").to_string_lossy().into_owned(),
                owned: true,
            });
        }
        if self.view.native {
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
            seeds.insert(format!("persona-{:04}", i + 1), seed);
        }
        let request = json!({"id":journal::id("bootstrap")?,"operator":s.operator,"persona_seeds":seeds,
            "setup":{"cohort":s.personas.len(),"destination":"openai","provider":settings,"working_directory":workspace}});
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
        if let Some(s) = &self.j.data.session {
            let status =
                self.session(vec!["session-status".into(), "--id".into(), s.id.clone()])?;
            journal::require(status["status"]=="finished","The previous session has no recorded outcome. Inspect it and retain it in history before explicitly starting a new session.")?;
        }
        let (current, page) = self.current(&work)?;
        journal::require(
            current["phase"] == "open",
            "This task is paused or ended. Use Resume work for a pause.",
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
        let route = if settings["mode"] == "siwc_oauth" {
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
        } else {
            args.extend(helpers::codex_args(self.view.codex.as_ref().unwrap()));
        }
        let args = self.session_args(args)?;
        let token = helpers::cancel();
        self.run = Some(token.clone());
        self.view.running = Some(work);
        let h = self.h.clone();
        let tx = self.internal.clone();
        thread::spawn(move || {
            let result = h.call_events(
                "personas-session",
                &args,
                Some((token, libc::SIGTERM)),
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
            let (current, page) = self.current(work)?;
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
                "The task changed before resuming. Refresh and choose Resume work again.".into(),
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
        if let Some((_, token)) = &self.metadata {
            token.store(true, Ordering::Release);
        }
    }
}
