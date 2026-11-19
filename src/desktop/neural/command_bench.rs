//! Native editor for portable command invocations and their immutable results.
use super::*;
use ai_personas::neural::commands::{Engine, History, Install, Run};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Run,
    Install,
    History,
}
#[derive(Clone, Copy, PartialEq)]
enum Pending {
    Install,
    Run,
    List,
    Inspect,
}

#[derive(Default)]
pub(super) struct Bench {
    pub open: bool,
    tab: Tab,
    installation: String,
    arguments: Vec<String>,
    files: Vec<(String, String)>,
    outputs: Vec<String>,
    stdin: String,
    asset_filter: String,
    wasi: bool,
    local: String,
    retained: String,
    from_library: bool,
    pending: Option<Pending>,
    error: Option<String>,
    notice: String,
    filter: String,
    history: Value,
    page_reference: Option<String>,
    detail: Value,
}

impl Bench {
    pub fn mount(&mut self, reference: &str, path: &str) {
        self.open = true;
        self.tab = Tab::Run;
        let name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("input");
        self.files.push((name.into(), reference.into()));
    }
    pub fn locate(&mut self, reference: String) -> Command {
        self.open = true;
        self.tab = Tab::History;
        self.filter = reference.clone();
        self.page_reference = Some(reference.clone());
        self.history = Value::Null;
        self.detail = Value::Null;
        self.error = None;
        self.notice.clear();
        self.pending = Some(if reference.starts_with("command_run:") {
            Pending::Inspect
        } else {
            Pending::List
        });
        Command::CommandHistory(History {
            reference: Some(reference),
            offset: 0,
            page_size: 12,
        })
    }
    pub fn executing(&self) -> bool {
        matches!(self.pending, Some(Pending::Install | Pending::Run))
    }
    pub fn receive(&mut self, result: Result<Value, String>) {
        let pending = self.pending.take();
        let v = match result {
            Ok(v) => v,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        match pending {
            Some(Pending::Install) => {
                self.installation = s(&v, "installation").into();
                self.notice = format!("{} available in this workspace", s(&v["record"], "name"));
                self.tab = Tab::Run;
            }
            Some(Pending::Run) => {
                self.detail = v;
                self.history = Value::Null;
                self.tab = Tab::History;
            }
            Some(Pending::List) => self.history = v,
            Some(Pending::Inspect) => self.detail = v["selected"].clone(),
            None => {}
        }
    }
    fn submit(&mut self, ui: &Ui, app: &mut App, action: Pending, command: Command) {
        self.pending = Some(action);
        self.error = None;
        self.notice.clear();
        app.command(ui.ctx(), command);
    }
    fn lookup(
        &mut self,
        ui: &Ui,
        app: &mut App,
        reference: Option<String>,
        offset: usize,
        detail: bool,
    ) {
        if !detail {
            self.page_reference = reference.clone();
        }
        self.submit(
            ui,
            app,
            if detail {
                Pending::Inspect
            } else {
                Pending::List
            },
            Command::CommandHistory(History {
                reference,
                offset,
                page_size: 12,
            }),
        );
    }
    pub fn window(&mut self, ctx: &egui::Context, app: &mut App) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Command bench")
            .id(egui::Id::new("command-bench"))
            .default_width(650.)
            .default_height(480.)
            .max_size(
                (ctx.content_rect().size() - egui::vec2(28., 60.)).max(egui::vec2(260., 160.)),
            )
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Run, "Run");
                    ui.selectable_value(&mut self.tab, Tab::Install, "Install");
                    if ui
                        .selectable_value(&mut self.tab, Tab::History, "Receipts")
                        .changed()
                        && self.history.is_null()
                        && !app.busy
                    {
                        self.lookup(ui, app, None, 0, false);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(format!("Operator · {}", app.operator));
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("command-form")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!app.busy && self.pending.is_none(), |ui| {
                            match self.tab {
                                Tab::Install => self.install_form(ui, app),
                                Tab::Run => self.run_form(ui, app),
                                Tab::History => self.receipts(ui, app),
                            }
                        });
                        if self.pending.is_some() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Waiting for the workspace…");
                            });
                        }
                        if let Some(error) = &self.error {
                            ui.colored_label(Color32::LIGHT_RED, error);
                        }
                        if !self.notice.is_empty() {
                            ui.colored_label(TEAL, &self.notice);
                        }
                    });
            });
        self.open &= open;
    }
    fn install_form(&mut self, ui: &mut Ui, app: &mut App) {
        ui.heading("Install a command");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.wasi, false, "ngspice circuit simulator");
            ui.selectable_value(&mut self.wasi, true, "WASI Preview 1 program");
        });
        ui.add_space(7.);
        if self.wasi {
            ui.label("Select a module with an exported _start function and memory.");
        } else {
            ui.label("Studio verifies the published simulator and runs its version command. An exact local copy can also supply the package.");
        }
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.from_library, false, "Local file");
            ui.radio_value(&mut self.from_library, true, "Retained bytes");
        });
        if self.from_library {
            ui.add(
                egui::TextEdit::singleline(&mut self.retained)
                    .hint_text("asset:…@revision or object:…")
                    .desired_width(f32::INFINITY),
            );
        } else {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.local)
                        .hint_text(if self.wasi {
                            "Path to a .wasm or .wat file"
                        } else {
                            "Leave empty to download ngspice"
                        })
                        .desired_width((ui.available_width() - 110.).clamp(80., 435.)),
                );
                if ui.button("Browse…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Select a command package")
                        .pick_file()
                    {
                        self.local = path.to_string_lossy().into_owned();
                    }
                }
            });
        }
        let retained = self
            .from_library
            .then(|| self.retained.trim().to_owned())
            .filter(|s| !s.is_empty());
        let local = (!self.from_library)
            .then(|| self.local.trim())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        if ui
            .add_enabled(
                !self.wasi || retained.is_some() || local.is_some(),
                theme::primary("Install command"),
            )
            .clicked()
        {
            self.submit(
                ui,
                app,
                Pending::Install,
                Command::InstallCommand {
                    package: Install {
                        engine: if self.wasi {
                            Engine::Wasi
                        } else {
                            Engine::Ngspice
                        },
                        source: retained,
                    },
                    from_file: local,
                },
            );
        }
    }
    fn run_form(&mut self, ui: &mut Ui, app: &mut App) {
        let installations: Vec<_> = app
            .records("installations")
            .iter()
            .filter(|record| record["body"]["format"] == "portable-command-v1")
            .filter_map(|record| {
                let reference: ai_personas::neural::store::Reference =
                    serde_json::from_value(record["reference"].clone()).ok()?;
                Some((reference.handle(), &record["body"]))
            })
            .collect();
        if self.installation.is_empty() {
            if let Some((reference, _)) = installations.first() {
                self.installation = reference.clone();
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.heading("Run an installed command");
            if (!self.arguments.is_empty()
                || !self.files.is_empty()
                || !self.outputs.is_empty()
                || !self.stdin.is_empty())
                && ui.small_button("Clear form").clicked()
            {
                self.arguments.clear();
                self.files.clear();
                self.outputs.clear();
                self.stdin.clear();
            }
        });
        if installations.is_empty() {
            ui.label("This workspace has no portable commands installed.");
            if ui.button("Choose a command to install").clicked() {
                self.tab = Tab::Install;
            }
            return;
        }
        egui::ComboBox::from_id_salt("command-installation")
            .width(ui.available_width().min(500.))
            .selected_text(
                installations
                    .iter()
                    .find(|(reference, _)| reference == &self.installation)
                    .map(|(_, record)| s(record, "name"))
                    .unwrap_or("Select command"),
            )
            .show_ui(ui, |ui| {
                for (reference, record) in installations {
                    ui.selectable_value(
                        &mut self.installation,
                        reference.clone(),
                        format!("{} · {}", s(record, "name"), s(record, "engine")),
                    )
                    .on_hover_text(reference);
                }
            });
        ui.add_space(4.);
        rows(
            ui,
            "Arguments",
            &mut self.arguments,
            "Add argument",
            "Literal argument",
        );
        ui.separator();
        ui.strong("Project files");
        let mut remove = None;
        for (i, (name, reference)) in self.files.iter_mut().enumerate() {
            ui.push_id(("command-file", i), |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(name)
                            .desired_width(145.)
                            .hint_text("Relative filename"),
                    );
                    ui.add(
                        egui::TextEdit::singleline(reference)
                            .desired_width((ui.available_width() - 85.).clamp(65., 310.))
                            .hint_text("Exact asset or object"),
                    );
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                })
            });
        }
        if let Some(i) = remove {
            self.files.remove(i);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Add file reference").clicked() {
                self.files.push(Default::default());
            }
            egui::containers::menu::MenuButton::new("Choose library file").ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.asset_filter).hint_text("Filter paths"),
                );
                let filter = self.asset_filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("bench-library")
                    .max_height(185.)
                    .show(ui, |ui| {
                        for asset in &app.snapshot.assets {
                            let path = s(&asset["body"], "path");
                            if path.to_lowercase().contains(&filter)
                                && ui
                                    .button(format!("{path} · v{}", asset["revision"]))
                                    .clicked()
                            {
                                self.mount(s(asset, "reference"), path);
                                ui.close();
                            }
                        }
                    });
            });
        });
        ui.collapsing("Standard input", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.stdin)
                    .desired_width(f32::INFINITY)
                    .hint_text("Optional exact asset or object"),
            );
        });
        ui.separator();
        rows(
            ui,
            "Files to capture",
            &mut self.outputs,
            "Add output",
            "Relative output filename",
        );
        let mut files = BTreeMap::new();
        let valid = self.files.iter().all(|(name, reference)| {
            !name.is_empty()
                && !reference.trim().is_empty()
                && files
                    .insert(name.clone(), reference.trim().to_owned())
                    .is_none()
        }) && self.outputs.iter().all(|path| !path.is_empty());
        if !valid {
            ui.weak("Give each mounted file a unique filename and reference, and name every requested output.");
        }
        ui.add_space(5.);
        if ui
            .add_enabled(
                valid && !self.installation.is_empty(),
                theme::primary("Run command"),
            )
            .clicked()
        {
            self.submit(
                ui,
                app,
                Pending::Run,
                Command::RunCommand {
                    installation: self.installation.clone(),
                    arguments: serde_json::json!(self.arguments).to_string(),
                    files: serde_json::json!(files).to_string(),
                    outputs: self.outputs.clone(),
                    stdin: (!self.stdin.trim().is_empty()).then(|| self.stdin.trim().into()),
                },
            );
        }
    }
    fn receipts(&mut self, ui: &mut Ui, app: &mut App) {
        ui.heading("Execution receipts");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width((ui.available_width() - 135.).clamp(80., 435.))
                    .hint_text("All activity, or filter by exact reference"),
            );
            if ui.button("Find receipts").clicked() {
                self.lookup(
                    ui,
                    app,
                    (!self.filter.trim().is_empty()).then(|| self.filter.trim().into()),
                    0,
                    false,
                );
            }
        });
        if !self.history.is_null() {
            let total = self.history["total"].as_u64().unwrap_or(0) as usize;
            let offset = self.history["offset"].as_u64().unwrap_or(0) as usize;
            let count = self.history["runs"].as_array().map_or(0, Vec::len);
            let mut selected = None;
            for row in self.history["runs"].as_array().into_iter().flatten() {
                let label = format!(
                    "{}  ·  {}  ·  {}   {}",
                    s(row, "engine"),
                    s(row, "actor"),
                    exit_label(row),
                    row["arguments"]
                );
                if ui
                    .selectable_label(row["receipt"] == self.detail["receipt"], label)
                    .on_hover_text(s(row, "receipt"))
                    .clicked()
                {
                    selected = Some(s(row, "receipt").to_owned());
                }
            }
            if let Some(receipt) = selected {
                self.lookup(ui, app, Some(receipt), 0, true);
            }
            ui.horizontal(|ui| {
                let anchor = self.page_reference.clone();
                if ui
                    .add_enabled(offset > 0, egui::Button::new("Previous"))
                    .clicked()
                {
                    self.lookup(ui, app, anchor.clone(), offset.saturating_sub(12), false);
                }
                ui.weak(if count == 0 {
                    format!("0 shown of {total}")
                } else {
                    format!("{}–{} of {total}", offset + 1, offset + count)
                });
                if ui
                    .add_enabled(offset + count < total, egui::Button::new("Next"))
                    .clicked()
                {
                    self.lookup(ui, app, anchor, offset + count, false);
                }
            });
        } else if self.detail.is_null() {
            ui.weak("Find receipts to inspect runs from personas and operators.");
        }
        if self.detail.is_null() {
            return;
        }
        ui.separator();
        let r = &self.detail["result"];
        let receipt = s(&self.detail, "receipt");
        ui.horizontal_wrapped(|ui| {
            ui.heading(s(r, "argv0"));
            ui.colored_label(
                if r["success"] == true {
                    TEAL
                } else {
                    Color32::LIGHT_RED
                },
                exit_label(r),
            );
            ui.weak(format!("{} · {}", s(r, "origin"), s(r, "actor")));
        });
        ui.horizontal_wrapped(|ui| {
            ui.monospace(format!("Arguments  {}", r["arguments"]));
            if ui
                .small_button("Load as new run")
                .on_hover_text("Edit these exact inputs and arguments before running again")
                .clicked()
            {
                match serde_json::from_value::<Run>(r["intent"].clone()) {
                    Ok(input) => {
                        self.installation = input.installation;
                        self.arguments = input.arguments;
                        self.files = input.files.into_iter().collect();
                        self.outputs = input.outputs;
                        self.stdin = input.stdin.unwrap_or_default();
                        self.tab = Tab::Run;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
        if let Some(error) = r["error"].as_str() {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        for (name, value) in r["file_errors"].as_object().into_iter().flatten() {
            ui.colored_label(
                Color32::LIGHT_RED,
                format!("{name} · {}", value.as_str().unwrap_or("unavailable")),
            );
        }
        let mut show = None;
        ui.collapsing("Program and input files", |ui| {
            if ui
                .link(format!("Installed program · {}", s(r, "argv0")))
                .on_hover_text(s(r, "installation"))
                .clicked()
            {
                show = Some((s(r, "installation").to_owned(), None));
            }
            for (name, file) in r["files_supplied"].as_object().into_iter().flatten() {
                let reference = s(file, "reference");
                if ui
                    .link(format!("{name}  ·  {} bytes", file["bytes"]))
                    .on_hover_text(reference)
                    .clicked()
                {
                    let binding = reference
                        .starts_with("object:")
                        .then(|| (receipt.to_owned(), name.clone()));
                    show = Some((reference.to_owned(), binding));
                }
            }
            if let Some(reference) = r["stdin"]["reference"].as_str() {
                if ui
                    .link(format!("Standard input  ·  {} bytes", r["stdin"]["bytes"]))
                    .on_hover_text(reference)
                    .clicked()
                {
                    show = Some((reference.to_owned(), None));
                }
            }
        });
        for (name, value) in r["outputs"].as_object().into_iter().flatten() {
            if ui
                .link(format!("{name}  ·  {} bytes", value["bytes"]))
                .clicked()
            {
                show = Some((
                    s(value, "object").into(),
                    Some((receipt.to_owned(), name.clone())),
                ));
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (key, label) in [("stdout", "Standard output"), ("stderr", "Standard error")] {
                if ui
                    .link(format!("{label} · {} bytes", r[key]["bytes"]))
                    .clicked()
                {
                    show = Some((s(&r[key], "object").into(), None));
                }
            }
            if ui.link("Full receipt").clicked() {
                show = Some((receipt.into(), None));
            }
            if ui.small_button("Copy receipt").clicked() {
                ui.ctx().copy_text(receipt.into());
            }
        });
        if let Some((reference, source)) = show {
            self.open = false;
            app.open_named_resource(ui.ctx(), reference, source);
        }
    }
}
fn exit_label(value: &Value) -> String {
    match value["exit_code"].as_i64() {
        Some(code) if value["success"] == false && code == 0 => "Exit 0 · output missing".into(),
        Some(code) => format!("Exit {code}"),
        None => "Execution failed".into(),
    }
}
fn rows(ui: &mut Ui, label: &str, items: &mut Vec<String>, add: &str, hint: &str) {
    ui.push_id(label, |ui| {
        ui.strong(label);
        let mut remove = None;
        for (i, value) in items.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .desired_width((ui.available_width() - 85.).clamp(70., 460.))
                            .hint_text(hint),
                    );
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                })
            });
        }
        if let Some(i) = remove {
            items.remove(i);
        }
        if ui.small_button(add).clicked() {
            items.push(String::new());
        }
    });
}
