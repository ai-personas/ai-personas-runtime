//! Native workspace for editable persona models. Inspection uses a read-only
//! snapshot; controls retain the same runtime entry points as the terminal.
use super::{
    artifacts, design, document, file_desk,
    theme::{self, Icon},
};
use ai_personas::{
    neural::cli::{self, Command},
    persona_model::{FeatureContract, Tensor},
};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Ui};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
mod character;
mod atlas;
mod character_paths;
mod revision_uses;
mod comparisons;
mod explorer;
mod data;
mod home;
mod home_index;
mod home_live;
mod society;
mod home_navigation;
mod home_updates;
mod delivery_desk;
mod pdf_pages;
mod print_preview;
mod file_history;
mod delivery_followups;
mod dialogs;
mod export;
mod life;
mod question_history;
mod relationships;
mod library_search;
mod asset_feedback;
mod package_reviews;
mod requested_activities;
mod inbox;
mod input_files;
mod intake;
mod overview;
mod live_board;
mod live_composer;
mod message_receipt;
mod activity_sources;
mod people;
mod mesh_profile;
mod workspace;
mod workspace_find;
mod usage;
mod responsibilities;
mod agreements;
mod render;
mod command_bench;

const TEAL: Color32 = Color32::from_rgb(103, 198, 185);
const PURPLE: Color32 = Color32::from_rgb(172, 150, 221);
const COMPACT_BROWSER_WIDTH: f32 = 820.;
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
enum Editor {
    #[default]
    Home,
    Studio,
    Delivery,
    People,
    Inbox,
    Responsibilities,
    Agreements,
    Conversations,
    Artifacts,
    Character,
    Atlas,
    Usage,
}
impl Editor {
    fn is_workspace_view(self) -> bool { matches!(self, Self::Home | Self::Atlas) }
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Layout {
    editor: Editor,
    delivery: String,
    outliner: bool,
    inspector: bool,
    activity: bool,
    left_width: f32,
    right_width: f32,
    bottom_height: f32,
    dark: bool,
    scale: f32,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            editor: Editor::Home,
            delivery: String::new(),
            outliner: true,
            inspector: false,
            activity: false,
            left_width: 210.,
            right_width: 310.,
            bottom_height: 180.,
            dark: true,
            scale: 1.,
        }
    }
}
use dialogs::Dialog;
enum Update {
    Status(data::Monitor, Result<data::Refresh, String>),
    Done { result: Result<Value, String>, start_workspace: bool },
    BenchDone(Result<Value, String>),
    LiveMessage(live_composer::SubmissionResult),
    RunDone(Result<Value, String>),
    Preview(u64, Result<data::Preview, String>),
    Saved(u64, Result<String, String>),
}
pub struct App {
    pub(super) show_library: bool,
    pub(super) operator: String,
    root: PathBuf,
    snapshot: data::Snapshot,
    events: Vec<Value>,
    program_runs: BTreeMap<String, Vec<usize>>,
    model_renders: BTreeMap<String, Vec<usize>>,
    home_index: home_index::Index,
    home: home::Home,
    home_updates: home_updates::Updates,
    workspace_find: workspace_find::Finder,
    delivery_desk: delivery_desk::Desk,
    file_history: file_history::History,
    delivery_followups: delivery_followups::Panel,
    runner_observation: ai_personas::neural::live::RunnerObservation,
    character_decisions: Vec<Value>,
    known_decisions: Arc<BTreeSet<String>>,
    cursor: i64,
    tx: mpsc::Sender<Update>,
    rx: mpsc::Receiver<Update>,
    polling: bool,
    monitor: data::Monitor,
    last_poll: Instant,
    running: bool,
    following: Option<(Option<String>, Arc<std::sync::atomic::AtomicBool>)>,
    busy: bool,
    closing: bool,
    error: Option<String>,
    notice: String,
    layout: Layout,
    work: String,
    persona: String,
    profile_view: people::ProfileView,
    profile_files: people::AuthoredFiles,
    inbox: inbox::Inbox,
    live_board: live_board::Board,
    live_composer: live_composer::Composer,
    message_receipt: message_receipt::Window,
    activity_sources: activity_sources::Window,
    intake: Option<intake::Window>,
    activity_filter_text: String,
    dialog: Option<Dialog>,
    submitting_dialog: bool,
    conversation_filter: String,
    conversation_persona: String,
    conversation_channel: usize,
    conversation_page: usize,
    activity_filter: usize,
    asset_filter: String,
    asset_owner: String,
    artifact_scope: usize,
    artifact_mode: usize,
    selected_reference: String,
    reference_input: String,
    library_search: library_search::Window,
    asset_feedback: asset_feedback::Window,
    package_reviews: package_reviews::Desk,
    requested_activities: requested_activities::Window,
    mesh_profile: mesh_profile::Window,
    command_bench: command_bench::Bench,
    usage: usage::Panel,
    responsibilities: responsibilities::Board,
    agreement_book: agreements::Book,
    preview_generation: u64,
    preview_zip_member: Option<(usize, String)>,
    preview_pending: bool,
    preview_metadata: Value,
    print_preview: Option<print_preview::Preview>,
    print_worker: print_preview::Worker,
    viewer: artifacts::Viewer,
    files: artifacts::Viewer,
    character_tab: usize,
    atlas: atlas::Panel,
    comparison_filter: usize,
    encounter: comparisons::Encounter,
    explorer: explorer::Explorer,
    life_filter: usize,
    development_anchor: String,
    development_filter: usize,
    development_all_work: bool,
    development_scope: (String, String),
    question_history: question_history::History,
    relationships: relationships::Panel,
    character_paths: character_paths::Window,
    revision_uses: revision_uses::Panel,
    character_input: character::InputView,
    parameter_filter: String,
    tensor_filter: String,
    inspector_window: bool,
    compact_browser: bool,
    export_dialog: Option<export::Dialog>,
    export_generation: u64,
}
fn operator_call(root:&std::path::Path,operator:&str,command:Command,request:Option<&str>)->Result<Value,String>{
    let runtime=tokio::runtime::Runtime::new().map_err(|e|e.to_string())?;
    runtime.block_on(cli::execute_with_request(root,operator,command,request)).map_err(|e|e.to_string())
}
impl App {
    pub fn new(
        ctx: &egui::Context,
        root: PathBuf,
        gl: Option<&Arc<eframe::glow::Context>>,
        operator: String,
    ) -> Self {
        theme::install_fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        let layout = std::fs::read(root.join("desktop-layout.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Layout>(&b).ok())
            .unwrap_or_default();
        theme::apply(ctx, layout.dark, layout.scale);
        let (tx, rx) = mpsc::channel();
        Self {
            show_library: false,
            operator,
            root,
            snapshot: Default::default(),
            events: vec![],
            program_runs: BTreeMap::new(),
            model_renders: BTreeMap::new(),
            home_index: Default::default(),
            home: Default::default(),
            home_updates: Default::default(),
            workspace_find: Default::default(),
            delivery_desk: delivery_desk::Desk::new(gl),
            file_history: Default::default(),
            delivery_followups: Default::default(),
            runner_observation: Default::default(),
            character_decisions: vec![],
            known_decisions: Default::default(),
            cursor: 0,
            tx,
            rx,
            polling: false,
            monitor: Default::default(),
            last_poll: Instant::now() - Duration::from_secs(5),
            running: false,
            following: None,
            busy: false,
            closing: false,
            error: None,
            notice: String::new(),
            layout,
            work: String::new(),
            persona: String::new(),
            profile_view: Default::default(),
            profile_files: Default::default(),
            inbox: Default::default(),
            live_board: Default::default(),
            live_composer: Default::default(),
            message_receipt: Default::default(),
            activity_sources: Default::default(),
            intake: None,
            activity_filter_text: String::new(),
            dialog: None,
            submitting_dialog: false,
            conversation_filter: String::new(),
            conversation_persona: String::new(),
            conversation_channel: 0,
            conversation_page: 0,
            activity_filter: 0,
            asset_filter: String::new(),
            asset_owner: String::new(),
            artifact_scope: 0,
            artifact_mode: 0,
            selected_reference: String::new(),
            reference_input: String::new(),
            library_search: Default::default(),
            asset_feedback: Default::default(),
            package_reviews: Default::default(),
            requested_activities: Default::default(),
            mesh_profile: Default::default(),
            command_bench: Default::default(),
            usage: Default::default(),
            responsibilities: Default::default(),
            agreement_book: Default::default(),
            preview_generation: 0,
            preview_zip_member: None,
            preview_pending: false,
            preview_metadata: Value::Null,
            print_preview: None,
            print_worker: Default::default(),
            viewer: artifacts::Viewer::new(ctx, gl),
            files: artifacts::Viewer::new(ctx, gl),
            character_tab: 0,
            atlas: Default::default(),
            comparison_filter: 0,
            encounter: Default::default(),
            explorer: explorer::Explorer::default(),
            life_filter: 0,
            development_anchor: String::new(),
            development_filter: 0,
            development_all_work: false,
            development_scope: (String::new(), String::new()),
            question_history: Default::default(),
            relationships: Default::default(),
            character_paths: Default::default(),
            revision_uses: Default::default(),
            character_input: Default::default(),
            parameter_filter: String::new(),
            tensor_filter: String::new(),
            inspector_window: false,
            compact_browser: false,
            export_dialog: None,
            export_generation: 0,
        }
    }
    fn poll_interval(&self) -> Duration {
        let receiving = self.snapshot.active.iter().any(|call| {
            call["state"] == "dispatched" && call["stage"] == "ordinary"
                && self.runner_observation.has_worker(s(call, "work"), s(call, "actor"))
        });
        Duration::from_millis(if receiving { 120 } else { 450 })
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if self.polling || self.last_poll.elapsed() < self.poll_interval() {
            return;
        }
        self.polling = true;
        self.last_poll = Instant::now();
        let (root, after, tx, ctx) = (self.root.clone(), self.cursor, self.tx.clone(), ctx.clone());
        let known_decisions = self.known_decisions.clone();
        let active = self.snapshot.active.clone();
        let mut monitor = std::mem::take(&mut self.monitor);
        std::thread::spawn(move || {
            let result = monitor.refresh(&root, after, &known_decisions, &active).map_err(|e| e.to_string());
            let _ = tx.send(Update::Status(monitor, result));
            ctx.request_repaint();
        });
    }
    fn follow_activity(&mut self, ctx: &egui::Context) {
        if !self.work.is_empty() { self.start_follow(ctx, Some(self.work.clone())); }
    }

    fn start_follow(&mut self, ctx: &egui::Context, work: Option<String>) {
        if !self.can_start_run() { return; }
        let finish=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (root,tx,ctx)=(self.root.clone(),self.tx.clone(),ctx.clone());
        self.running=true;
        self.notice.clear();
        self.error=None;
        self.following=Some((work.clone(),finish.clone()));
        std::thread::spawn(move ||{
            let result=tokio::runtime::Runtime::new().map_err(|e|e.to_string()).and_then(|runtime|
                runtime.block_on(async {
                    match work {
                        Some(work) => ai_personas::neural::runner::follow_activity(&root,&work,&finish).await,
                        None => ai_personas::neural::runner::follow_workspace(&root,&finish).await,
                    }
                }).map_err(|e|e.to_string()));
            let _=tx.send(Update::RunDone(result));ctx.request_repaint();
        });
    }

    fn finish_following(&self) {
        if let Some((_,finish))=&self.following { finish.store(true,std::sync::atomic::Ordering::Release); }
    }

    fn command(&mut self, ctx: &egui::Context, command: Command) {
        self.command_with_request(ctx, command, None);
    }
    fn command_with_request(&mut self, ctx: &egui::Context, command: Command, request: Option<String>) {
        self.dispatch(ctx, command, request, false);
    }
    fn dispatch(&mut self, ctx: &egui::Context, command: Command, request: Option<String>, start_workspace: bool) {
        let operator = self.operator.trim().to_owned();
        let run = matches!(command, Command::Run { .. });
        let bench = matches!(&command,Command::InstallCommand{..}|Command::RunCommand{..}|Command::CommandHistory(_));
        if run {
            self.notice.clear();
            self.running = true
        } else {
            self.busy = true
        }
        self.error = None;
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result=operator_call(&root,&operator,command,request.as_deref());
            let _ = tx.send(if run {
                Update::RunDone(result)
            } else if bench {
                Update::BenchDone(result)
            } else {
                Update::Done { result, start_workspace }
            });
            ctx.request_repaint();
        });
    }
    fn open_resource(&mut self, ctx: &egui::Context, reference: String) {
        self.open_named_resource(ctx, reference, None);
    }
    fn open_named_resource(&mut self, ctx: &egui::Context, reference: String, output: Option<(String,String)>) {
        self.print_preview = None;
        self.delivery_desk.record_origin(self.layout.editor == Editor::Delivery, &self.layout.delivery, &self.work, &reference);
        self.selected_reference = reference.clone();
        self.reference_input = reference.clone();
        self.layout.editor = Editor::Artifacts;
        self.artifact_mode = 0;
        self.preview_generation += 1;
        self.preview_zip_member = None;
        self.preview_pending = true;
        self.preview_metadata = Value::Null;
        self.viewer.clear_selection();
        self.error = None;
        let (root, serial, tx, ctx) = (
            self.root.clone(),
            self.preview_generation,
            self.tx.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let result = data::preview_output(&root, &reference, output.as_ref()).map_err(|e| e.to_string());
            let _ = tx.send(Update::Preview(serial, result));
            ctx.request_repaint();
        });
    }
    fn apply_snapshot(&mut self, mut snapshot: data::Snapshot) {
        self.cursor = snapshot.cursor;
        self.home_index.refresh(&snapshot.status, &snapshot.events);
        self.home_updates.observe(&snapshot.events, self.events.len());
        self.home.loom.observe(&snapshot.events, self.events.len());
        self.home.society.observe(&snapshot.status, &snapshot.events, self.events.len());
        for (offset,event) in snapshot.events.iter().enumerate() {
            if event["body"]["tool"]=="compute.run" {
                if let Some(program)=event["tool_outcome"]["program"].as_str() {
                    self.program_runs.entry(program.to_owned()).or_default().push(self.events.len()+offset);
                }
            } else if event["body"]["tool"]=="geometry.render" {
                for key in ["source","mesh","result"] {
                    if let Some(reference)=event["tool_outcome"][key].as_str() {
                        self.model_renders.entry(reference.to_owned()).or_default().push(self.events.len()+offset);
                    }
                }
            }
        }
        self.events.append(&mut snapshot.events);
        for decision in snapshot.character_decisions.drain(..) {
            Arc::make_mut(&mut self.known_decisions).insert(s(&decision, "invocation").into());
            self.character_decisions.push(decision);
        }
        self.character_decisions.sort_by_key(|d| (d["accepted"].as_i64().unwrap_or(0), d["order"].as_i64().unwrap_or(0)));
        self.snapshot = snapshot;
        if self.work.is_empty() {
            self.work = (self.layout.editor == Editor::Delivery).then(|| self.selected_home_package()).flatten()
                .and_then(|d| d["body"]["work"].as_str().map(str::to_owned))
                .or_else(||self.home_index.activities.first().map(|a|a.id.clone()))
                .unwrap_or_default();
        }
        if self.persona.is_empty() {
            self.persona = self.records("participation").iter().find(|r|r["body"]["work"]==self.work)
                .map(|r|s(&r["body"],"persona").into()).or_else(||self.records("personas").first().map(|r|s(&r["body"],"id").into()))
                .unwrap_or_default();
        }
        if self.layout.editor == Editor::People {
            self.align_profile_activity();
        }
    }

    fn drain(&mut self,ctx:&egui::Context) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::Status(monitor, result) => {
                    self.monitor = monitor;
                    self.polling = false;
                    match result {
                        Ok(fresh) => {
                            self.runner_observation = fresh.runner;
                            if let Some(snapshot) = fresh.snapshot { self.apply_snapshot(snapshot); }
                            for call in &mut self.snapshot.active {
                                if let Some(fields) = call.as_object_mut() {
                                    fields.remove("reply_draft");
                                }
                                if let Some(progress) = fresh.streams.get(s(call, "id")) {
                                    call["stream_bytes"] = serde_json::json!(progress.stream_bytes);
                                    call["observed_at"] = serde_json::json!(progress.observed_at);
                                    if let Some(draft) = &progress.reply_draft {
                                        call["reply_draft"] = serde_json::json!(draft);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.runner_observation = ai_personas::neural::live::RunnerObservation::Unavailable { reason: e.clone() };
                            for call in &mut self.snapshot.active {
                                if let Some(fields) = call.as_object_mut() {
                                    fields.remove("reply_draft");
                                }
                            }
                            self.error = Some(e);
                        }
                    }
                }
                Update::Preview(serial, result) if serial == self.preview_generation => {
                    self.preview_pending = false;
                    match result {
                        Ok(preview) => {
                            self.preview_metadata = preview.metadata;
                            self.viewer.open_snapshot(
                                preview.directory,
                                preview.path,
                                preview.digest,
                                self.preview_zip_member.take(),
                            );
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Update::Preview(_, _) => {}
                Update::Saved(serial, result) => match result {
                    Ok(note) => {
                        if let Some(preview) = &mut self.print_preview {
                            preview.finish_export(serial, Some(&note));
                        }
                        self.notice = note;
                        if self
                            .export_dialog
                            .as_ref()
                            .is_some_and(|d| d.serial == serial)
                        {
                            self.export_dialog = None;
                        }
                    }
                    Err(e) => {
                        if let Some(dialog) =
                            self.export_dialog.as_mut().filter(|d| d.serial == serial)
                        {
                            dialog.busy = false;
                            dialog.error = Some(e);
                        } else {
                            self.error = Some(e);
                        }
                    }
                },
                Update::LiveMessage(result) => {
                    if self.live_composer.receive(result) {
                        self.last_poll = Instant::now() - Duration::from_secs(3);
                    }
                }
                Update::BenchDone(result) => {
                    self.busy=false;
                    self.command_bench.receive(result);
                    self.last_poll=Instant::now()-Duration::from_secs(3);
                }
                Update::Done { result, start_workspace } => {
                    self.busy = false;
                    if self.submitting_dialog {
                        let keep_project=matches!(self.dialog,Some(Dialog::Render(_))) && result.as_ref().is_ok_and(|v|v["already_installed"].is_boolean());
                        if result.is_ok() && !keep_project { self.dialog = None; }
                        self.submitting_dialog = false;
                    }
                    if let Ok(value)=&result {
                        if let Some(receipt)=value["operator_receipt"].as_str() {
                            let target=value["mesh"].as_str().filter(|_|value["success"]==true).unwrap_or(receipt);
                            self.open_resource(ctx,target.into());
                        }
                    }
                    let invited = result.as_ref().is_ok_and(|v|v["work"].is_string() && v["invited"].is_array());
                    self.command_finished(result);
                    if start_workspace && invited && !self.closing { self.start_follow(ctx, None); }
                }
                Update::RunDone(result) => {
                    self.running = false;
                    let followed=self.following.take();
                    match result {
                        Ok(_) => self.notice=match followed {
                            Some((None,_)) => "Workspace run finished".into(),
                            Some((Some(_),_)) => "Activity run finished".into(),
                            None => "Run finished".into(),
                        },
                        Err(error) => self.error=Some(error),
                    }
                    self.last_poll=Instant::now()-Duration::from_secs(3);
                }
            }
        }
    }
    fn command_finished(&mut self, result: Result<Value, String>) {
        match result {
            Ok(v) => {
                self.notice = if v["session"].is_string() && v["requested_by"].is_string() {
                    "Finish requested for this run".into()
                } else if let (Some(kind), Some(work)) = (v["kind"].as_str(), v["work"].as_str()) {
                    self.open_home_activity(work);
                    if let Some(invited) = v["invited"].as_array().and_then(|p|p.first()).and_then(Value::as_str) { self.persona = invited.into(); }
                    if kind == "social" { "Conversation opened".into() } else { "Task created".into() }
                } else if v["reference"].as_str().is_some_and(|r| r.starts_with("agreement:") || r.starts_with("agreement_position:")) {
                    let position = v["agreement"].as_str();
                    let edition = position.or(v["reference"].as_str());
                    if self.layout.editor == Editor::Agreements && v["work"] == self.work {
                        self.show_agreements(edition.map(str::to_owned));
                        self.agreement_book.request_refresh();
                    }
                    if position.is_some() { "Your signed position was recorded".into() } else { "Agreement edition published".into() }
                } else if let Some(persona) = v["persona"].as_str() {
                    self.persona = persona.to_owned();
                    "Persona created".into()
                } else if let Some(accepted) = v["accepted"].as_bool() {
                    if accepted { "Delivery accepted".into() } else { "Changes requested".into() }
                } else if v["configuration"].is_string() {
                    format!("Saved reasoning settings · {} at {} effort", s(&v["selected"],"model"), s(&v["selected"],"effort"))
                } else if v["workspace_connection"] == "codex_oauth" {
                    "Codex account connected · each persona kept its model and reasoning effort".into()
                } else if v["operator_receipt"].is_string() {
                    if v["success"]!=true {"Render failed · compiler receipt opened".into()}
                    else if v["warnings"].as_array().is_some_and(|w|!w.is_empty()) {"Mesh rendered with compiler warnings".into()}
                    else {"Mesh rendered".into()}
                } else if v["installation"].is_string() {
                    format!("Workspace compiler available · {}",s(&v["record"],"compiler_version"))
                } else if let Some(archive) = v["archive"].as_str() {
                    format!("Backup saved to {archive}")
                } else if v["reference"].as_str().is_some_and(|r|r.starts_with("human_request:")) {
                    self.inbox.response_saved(v["reference"].as_str().unwrap());
                    "Human update recorded".into()
                } else if let Some(state) = v["state"].as_str() {
                    format!("Scheduling {state}")
                } else {
                    "Workspace updated".into()
                };
            }
            Err(e) => self.error = Some(e),
        }
        self.last_poll = Instant::now() - Duration::from_secs(3);
    }
    fn records(&self, key: &str) -> &[Value] {
        self.snapshot.status[key]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    fn label<'a>(&'a self, id: &'a str) -> &'a str {
        self.records("personas")
            .iter()
            .find(|p| p["body"]["id"] == id)
            .and_then(|p| p["body"]["label"].as_str())
            .unwrap_or(id)
    }
    fn selected_person(&self) -> Value {
        self.records("personas")
            .iter()
            .find(|p| p["body"]["id"] == self.persona)
            .map(|p| p["body"].clone())
            .unwrap_or(Value::Null)
    }
    fn participation(&self, id: &str) -> Value {
        self.records("participation")
            .iter()
            .find(|p| p["body"]["persona"] == id && p["body"]["work"] == self.work)
            .map(|p| p["body"].clone())
            .unwrap_or(Value::Null)
    }
    fn task_title(&self) -> &str {
        self.home_index.activity(&self.work).map(|a| a.title.as_str()).unwrap_or("Your workspace")
    }
    fn delivered_refs(&self) -> BTreeSet<String> {
        self.records("deliveries")
            .iter()
            .filter(|d| d["body"]["work"] == self.work)
            .flat_map(|d| d["body"]["artifacts"].as_array().into_iter().flatten())
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    }
    fn color(&self, id: &str) -> Color32 {
        persona_color(id)
    }
    fn save_layout(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.layout) {
            let path = self.root.join("desktop-layout.json");
            let temp = self
                .root
                .join(format!(".desktop-layout-{}.json", std::process::id()));
            if std::fs::write(&temp, bytes).is_ok() {
                let _ = std::fs::rename(temp, path);
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let _timing = super::timings::frame(ctx, frame);
        self.drain(ctx);
        self.poll(ctx);
        self.find_shortcut(ctx);
        ctx.request_repaint_after(self.poll_interval());
        if ctx.input(|i| i.viewport().close_requested()) && (self.running || self.command_bench.executing()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.closing {
                self.closing = true;
                self.finish_following();
                if self.following.is_none() || self.command_bench.executing() { self.command(
                    ctx,
                    Command::Control {
                        scope: "global".into(),
                        state: "paused".into(),
                        reason: "Desktop window closed by user".into(),
                    },
                ); }
            }
        }
        if self.closing && !self.running && !self.command_bench.executing() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.wide_navigation(ctx) { self.navigation_rail(ctx); }
        self.header(ctx);
        self.status_bar(ctx);
        if self.layout.activity {
            self.activity_dock(ctx);
        }
        let wide = ctx.content_rect().width() > 1100.;
        let workspace_view = self.layout.editor.is_workspace_view();
        if self.layout.inspector && wide && !workspace_view {
            self.inspector_panel(ctx);
        }
        let show_browser = if ctx.content_rect().width() < COMPACT_BROWSER_WIDTH { self.compact_browser } else { self.layout.outliner };
        if show_browser && !workspace_view {
            self.outliner(ctx);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::Palette::new(self.layout.dark).canvas)
                    .inner_margin(if Self::constrained_activity_layout(ctx) { 4 } else { 16 }),
            )
            .show(ctx, |ui| {
                self.file_history_return(ui);
                match self.layout.editor {
                Editor::Home => self.workspace_home(ui),
                Editor::Studio => self.studio(ui),
                Editor::Delivery => self.home_package_page(ui),
                Editor::People => self.people(ui),
                Editor::Inbox => self.human_inbox(ui),
                Editor::Agreements => self.agreements(ui),
                Editor::Responsibilities => {
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("← Activity").clicked(){self.layout.editor=Editor::Studio;}
                        if !self.work.is_empty() && ui.small_button("Agreement book").clicked(){self.show_agreements(None);}
                    });
                    let cursor=self.events.iter().rev().find(|e|e["work"]==self.work).and_then(|e|e["event"].as_i64()).unwrap_or(0);
                    match self.responsibilities.show(ui,&self.root,&self.work,cursor){
                        Some(responsibilities::Action::ChangeActivity{reopen,brief})=>self.dialog=Some(Dialog::ActivityState{
                            work:self.work.clone(),expected:brief,reopen,reason:String::new()}),
                        Some(responsibilities::Action::Inspect(reference))=>self.open_resource(ctx,reference),
                        Some(responsibilities::Action::Message{person,text})=>self.dialog=Some(Dialog::Participation{
                            work:self.work.clone(),title:self.task_title().into(),action:dialogs::Action::Message,person,text,reply_to:None,sources:Default::default()}),
                        Some(responsibilities::Action::Invite(person))=>self.dialog=Some(Dialog::Participation{
                            work:self.work.clone(),title:self.task_title().into(),action:dialogs::Action::Invite,person,text:String::new(),reply_to:None,sources:Default::default()}),
                        None=>{}
                    }
                },
                Editor::Conversations => self.conversations(ui),
                Editor::Artifacts => self.artifacts(ui),
                Editor::Character => self.character(ui),
                Editor::Atlas => self.character_atlas(ui),
                Editor::Usage => {
                    if let Some(reference) = self.usage.show(ui, &self.root, &self.work) {
                        self.open_resource(ctx,reference);
                    }
                },
                }
            });
        self.file_history_window(ctx);
        self.print_preview_window(ctx);
        if self.inspector_window {
            let mut open = true;
            egui::Window::new("Inspector")
                .open(&mut open)
                .default_width(330.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
                });
            self.inspector_window = open;
        }
        let mut bench = std::mem::take(&mut self.command_bench);
        bench.window(ctx,self);
        self.command_bench = bench;
        self.dialogs(ctx);
        self.sources_window(ctx);
        self.intake_window(ctx);
        self.message_receipt_window(ctx);
        self.export_window(ctx);
        self.workspace_find_window(ctx);
        if let Some(action)=self.requested_activities.show(ctx,&self.root,&self.snapshot.status,self.cursor) {
            match action {
                requested_activities::Action::Inspect(r) if r.starts_with("delivery:")=>self.open_issued_package(ctx,&r),
                requested_activities::Action::Inspect(r)=>self.open_resource(ctx,r),
                requested_activities::Action::OpenWork(id)=>{self.work=id;self.conversation_page=0;self.layout.editor=Editor::Studio;}
            }
        }
        if let Some(reference)=self.mesh_profile.show(ctx,&self.root,&self.operator){self.open_resource(ctx,reference);}
        if let Some(reference)=self.package_reviews.show(ctx,&self.root,&self.operator,&self.snapshot.status) {
            self.open_resource(ctx,reference);
        }
        if let Some(reference) = self.asset_feedback.show(ctx, &self.root, &self.operator, &self.snapshot.status) {
            self.open_resource(ctx, reference);
        }
        if let Some(reference)=self.library_search.show(ctx,&self.root){
            self.open_resource(ctx,reference);
        }
        if let Some(reference) = self.question_history.show(ctx, &self.root, &self.snapshot.status) {
            self.open_resource(ctx, reference);
        }
        if let Some(reference) = self.character_paths.show(ctx, &self.root, &self.snapshot.candidates) {
            self.open_resource(ctx, reference);
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.finish_following();
        self.print_preview = None;
        self.print_worker.shutdown();
        self.save_layout();
        self.viewer.destroy();
        self.files.destroy();
        self.delivery_desk.release();
    }
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn activity_title(work: &Value) -> &str {
    match work["original_need"].as_str().filter(|s| !s.trim().is_empty()) {
        Some(need) => need,
        None if work["kind"] == "social" => "Open conversation",
        None => "Untitled activity",
    }
}
fn title(s: &str) -> String {
    let mut c = s.replace('_', " ").chars().collect::<Vec<_>>();
    if let Some(first) = c.first_mut() {
        first.make_ascii_uppercase();
    }
    c.into_iter().collect()
}
fn excerpt(text: &str, max: usize) -> String {
    let mut v = text.chars().take(max).collect::<String>();
    if text.chars().count() > max {
        v.push('…');
    }
    v
}
fn number(value: u64) -> String {
    let s = value.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',')
        }
        out.push(c);
    }
    out
}
fn counted(n: u64, singular: &str, plural: &str) -> String {
    format!("{} {}", number(n), if n == 1 { singular } else { plural })
}
fn size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}
fn time_label(ts: i64) -> String {
    format!(
        "{:02}:{:02}:{:02} UTC",
        ts.rem_euclid(86400) / 3600,
        ts.rem_euclid(3600) / 60,
        ts.rem_euclid(60)
    )
}
/// A continuing identity keeps its hue when someone joins, leaves or is
/// filtered out. Names and exact IDs, not colours, distinguish people.
pub(super) fn persona_color(id: &str) -> Color32 {
    let colors = [theme::ACCENT, Color32::from_rgb(219, 134, 158), PURPLE,
        Color32::from_rgb(121, 166, 230), TEAL, theme::GREEN];
    let hash = id.bytes().fold(0x811c9dc5u32, |h, byte| (h ^ u32::from(byte)).wrapping_mul(16777619));
    let slot = hash as usize % (colors.len() * 256);
    let index = slot / 256;
    colors[index].lerp_to_gamma(colors[(index + 1) % colors.len()], (slot % 256) as f32 / 256.)
}
fn eyebrow(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .monospace().size(11.)
            .color(theme::palette(ui).muted)
            .extra_letter_spacing(1.2),
    );
}
fn section(ui: &mut Ui, label: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.small(detail);
        });
    });
    ui.add_space(7.);
}
fn card(ui: &Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::palette(ui).panel)
        .stroke(Stroke::new(1., theme::palette(ui).hover))
        .corner_radius(6)
        .inner_margin(12)
}
fn avatar(ui: &mut Ui, label: &str, color: Color32, diameter: f32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::click());
    design::persona_mark(ui, rect, label, color);
    response
}
