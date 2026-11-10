//! Native workspace for editable persona models. Inspection uses a read-only
//! snapshot; controls retain the same runtime entry points as the terminal.
use super::{
    artifacts, document,
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
    collections::BTreeSet,
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
mod character;
mod data;
mod export;
mod workspace;

const TEAL: Color32 = Color32::from_rgb(103, 198, 185);
const PURPLE: Color32 = Color32::from_rgb(172, 150, 221);
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
enum Editor {
    #[default]
    Studio,
    Conversations,
    Artifacts,
    Character,
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Layout {
    editor: Editor,
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
            editor: Editor::Studio,
            outliner: true,
            inspector: true,
            activity: false,
            left_width: 210.,
            right_width: 310.,
            bottom_height: 180.,
            dark: true,
            scale: 1.,
        }
    }
}
enum Dialog {
    Task,
    Message,
    Settings,
}
enum Update {
    Status(Result<data::Snapshot, String>),
    Done(Result<Value, String>),
    RunDone(Result<Value, String>),
    Preview(u64, Result<data::Preview, String>),
    Saved(u64, Result<String, String>),
}
pub struct App {
    root: PathBuf,
    snapshot: data::Snapshot,
    events: Vec<Value>,
    cursor: i64,
    tx: mpsc::Sender<Update>,
    rx: mpsc::Receiver<Update>,
    polling: bool,
    last_poll: Instant,
    running: bool,
    busy: bool,
    closing: bool,
    error: Option<String>,
    notice: String,
    layout: Layout,
    work: String,
    persona: String,
    need: String,
    invited: BTreeSet<String>,
    message: String,
    dialog: Option<Dialog>,
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
    preview_generation: u64,
    preview_pending: bool,
    preview_metadata: Value,
    viewer: artifacts::Viewer,
    files: artifacts::Viewer,
    character_tab: usize,
    contextual: bool,
    parameter_filter: String,
    tensor_filter: String,
    inspector_window: bool,
    export_dialog: Option<export::Dialog>,
    export_generation: u64,
}
impl App {
    pub fn new(
        ctx: &egui::Context,
        root: PathBuf,
        gl: Option<&Arc<eframe::glow::Context>>,
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
            root,
            snapshot: Default::default(),
            events: vec![],
            cursor: 0,
            tx,
            rx,
            polling: false,
            last_poll: Instant::now() - Duration::from_secs(5),
            running: false,
            busy: false,
            closing: false,
            error: None,
            notice: String::new(),
            layout,
            work: String::new(),
            persona: String::new(),
            need: String::new(),
            invited: BTreeSet::new(),
            message: String::new(),
            dialog: None,
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
            preview_generation: 0,
            preview_pending: false,
            preview_metadata: Value::Null,
            viewer: artifacts::Viewer::new(ctx, gl),
            files: artifacts::Viewer::new(ctx, gl),
            character_tab: 0,
            contextual: true,
            parameter_filter: String::new(),
            tensor_filter: String::new(),
            inspector_window: false,
            export_dialog: None,
            export_generation: 0,
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if self.polling || self.last_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.polling = true;
        self.last_poll = Instant::now();
        let (root, after, tx, ctx) = (self.root.clone(), self.cursor, self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = data::snapshot(&root, after).map_err(|e| e.to_string());
            let _ = tx.send(Update::Status(result));
            ctx.request_repaint();
        });
    }
    fn command(&mut self, ctx: &egui::Context, command: Command) {
        let run = matches!(command, Command::Run { .. });
        if run {
            self.running = true
        } else {
            self.busy = true
        }
        self.error = None;
        let (root, tx, ctx) = (self.root.clone(), self.tx.clone(), ctx.clone());
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
    fn open_resource(&mut self, ctx: &egui::Context, reference: String) {
        self.selected_reference = reference.clone();
        self.reference_input = reference.clone();
        self.layout.editor = Editor::Artifacts;
        self.artifact_mode = 0;
        self.preview_generation += 1;
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
            let result = data::preview(&root, &reference).map_err(|e| e.to_string());
            let _ = tx.send(Update::Preview(serial, result));
            ctx.request_repaint();
        });
    }
    fn drain(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::Status(result) => {
                    self.polling = false;
                    match result {
                        Ok(mut snapshot) => {
                            self.cursor = snapshot.cursor;
                            self.events.append(&mut snapshot.events);
                            self.snapshot = snapshot;
                            if self.work.is_empty() {
                                self.work = self
                                    .records("work")
                                    .first()
                                    .map(|r| s(&r["body"], "id").into())
                                    .unwrap_or_default();
                            }
                            if self.persona.is_empty() {
                                self.persona = self
                                    .records("personas")
                                    .first()
                                    .map(|r| s(&r["body"], "id").into())
                                    .unwrap_or_default();
                            }
                        }
                        Err(e) => self.error = Some(e),
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
                            );
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Update::Preview(_, _) => {}
                Update::Saved(serial, result) => match result {
                    Ok(note) => {
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
                Update::Done(result) => {
                    self.busy = false;
                    self.command_finished(result);
                }
                Update::RunDone(result) => {
                    self.running = false;
                    self.busy = false;
                    self.command_finished(result);
                }
            }
        }
    }
    fn command_finished(&mut self, result: Result<Value, String>) {
        match result {
            Ok(v) => {
                self.notice = if let Some(need) = v["need"].as_str() {
                    format!("Created: {need}")
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
        self.records("work")
            .iter()
            .find(|p| p["body"]["id"] == self.work)
            .and_then(|p| p["body"]["original_need"].as_str())
            .unwrap_or("Your workspace")
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
        let palette = [
            TEAL,
            PURPLE,
            theme::ACCENT,
            Color32::from_rgb(121, 166, 230),
            Color32::from_rgb(219, 134, 158),
        ];
        let index = self
            .records("personas")
            .iter()
            .position(|p| p["body"]["id"] == id)
            .unwrap_or(0);
        palette[index % palette.len()]
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
        self.drain();
        self.poll(ctx);
        ctx.request_repaint_after(Duration::from_secs(2));
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
        self.header(ctx);
        self.status_bar(ctx);
        if self.layout.activity {
            self.activity_dock(ctx);
        }
        let wide = ctx.content_rect().width() > 1100.;
        if self.layout.inspector && wide {
            self.inspector_panel(ctx);
        }
        if self.layout.outliner {
            self.outliner(ctx);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::Palette::new(self.layout.dark).canvas)
                    .inner_margin(16),
            )
            .show(ctx, |ui| match self.layout.editor {
                Editor::Studio => self.studio(ui),
                Editor::Conversations => self.conversations(ui),
                Editor::Artifacts => self.artifacts(ui),
                Editor::Character => self.character(ui),
            });
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
        self.dialogs(ctx);
        self.export_window(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_layout();
        self.viewer.destroy();
        self.files.destroy();
    }
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
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
fn eyebrow(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.)
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
    ui.painter()
        .circle_filled(rect.center(), diameter / 2., color.gamma_multiply(0.2));
    ui.painter().circle_stroke(
        rect.center(),
        diameter / 2. - 1.,
        Stroke::new(1., color.gamma_multiply(0.7)),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string(),
        FontId::proportional(diameter * 0.4),
        color,
    );
    response
}
