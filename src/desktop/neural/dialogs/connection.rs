use super::*;
use crate::desktop::{helpers, journal::{self, CodexPaths}};
use ai_personas::neural::runner::AuthRoute;

#[derive(PartialEq)]
enum Choice { Codex, Platform }

pub(in crate::desktop::neural) struct Form {
    choice: Choice,
    codex: CodexPaths,
    key_file: String,
}

impl Form {
    pub(super) fn new(app: &App) -> Self {
        let people = app.records("personas");
        let platform = !people.is_empty() && people.iter().all(|p| p["body"]["provider"]["mode"] == "platform_api_key");
        let codex = helpers::codex(None).unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            CodexPaths {
                executable: String::new(),
                codex_home: std::env::var("CODEX_HOME").unwrap_or_else(|_| format!("{home}/.codex")),
                host_home: home,
            }
        });
        // Only recall the path explicitly selected in this workspace; do not
        // open the key, inspect the environment or discover other accounts.
        let key_file = ai_personas::neural::store::open_inspection(&app.root).ok().and_then(|db|
            db.query_row("SELECT json_extract(CAST(o.bytes AS TEXT),'$.platform_key_file')
                FROM versions v JOIN objects o ON o.digest=v.digest WHERE v.kind='config' AND v.id='runtime'
                AND json_type(CAST(o.bytes AS TEXT),'$.platform_key_file')='text' ORDER BY v.revision DESC LIMIT 1",
                [], |r| r.get::<_,String>(0)).ok()
        ).unwrap_or_default();
        Self { choice: if platform { Choice::Platform } else { Choice::Codex }, codex, key_file }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, app: &mut App) -> Option<Command> {
        let palette = theme::palette(ui);
        ui.strong("Model connection");
        egui::Frame::new().fill(palette.field).corner_radius(6).inner_margin(9).show(ui, |ui| {
            ui.label(design::caption("Current ensemble", palette.muted));
            for person in app.records("personas") {
                let p = &person["body"];
                let settings = &p["provider"];
                let route = match s(settings, "mode") {
                    "chatgpt_oauth" => "ChatGPT OAuth",
                    "siwc_oauth" => "OpenAI OAuth",
                    "platform_api_key" => "OpenAI API key",
                    _ => "Configured provider",
                };
                ui.horizontal_wrapped(|ui| {
                    ui.strong(s(p, "label"));
                    ui.label(format!("{} · {} effort · {route}", s(settings, "model"), s(settings, "effort")));
                });
            }
        });
        if app.running || matches!(app.runner_observation, ai_personas::neural::live::RunnerObservation::Observed { .. }) {
            ui.horizontal_wrapped(|ui| {
                ui.small("Finish the current run to change connections.");
                app.finish_run_button(ui);
            });
            return None;
        }
        ui.add_space(4.);
        ui.label(design::caption("Connection for future calls", palette.muted));
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.choice, Choice::Codex, "Codex login");
            ui.selectable_value(&mut self.choice, Choice::Platform, "OpenAI API key");
        });
        let ready = app.can_start_run();
        match self.choice {
            Choice::Platform => {
                ui.small("Choose a private key file or a saved Studio credential file.");
                let label = ui.label("Private key file");
                ui.add(egui::TextEdit::singleline(&mut self.key_file).hint_text("Path to key file").desired_width(f32::INFINITY))
                    .labelled_by(label.id);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Choose file…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().set_title("Choose OpenAI key file").pick_file() {
                            self.key_file = path.display().to_string();
                        }
                    }
                    ui.add_enabled(ready && !self.key_file.trim().is_empty(), theme::primary("Use key file"))
                        .on_hover_text("Save this connection for the ensemble. Each persona keeps its model and effort.")
                        .clicked().then(|| Command::ConnectPlatform {
                            platform_key_file: journal::expand(std::path::Path::new(self.key_file.trim())),
                        })
                }).inner
            }
            Choice::Codex => {
                ui.small("Use your signed-in Codex account for authentication.");
                let c = &mut self.codex;
                ui.collapsing("Codex account location", |ui| {
                    for (label, path) in [("Executable", &mut c.executable), ("Account home", &mut c.host_home), ("Codex home", &mut c.codex_home)] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
                    }
                });
                let complete = [&c.executable, &c.host_home, &c.codex_home].iter().all(|s| !s.trim().is_empty());
                ui.add_enabled(ready && complete, theme::primary("Use Codex login"))
                    .on_hover_text("Save this connection for the ensemble. Each persona keeps its model and effort.")
                    .clicked().then(|| Command::ConnectCodex(AuthRoute {
                        codex_executable: c.executable.trim().into(),
                        host_home: c.host_home.trim().into(),
                        codex_home: c.codex_home.trim().into(),
                    }))
            }
        }
    }
}
