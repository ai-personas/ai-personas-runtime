use super::*;
use crate::desktop::{helpers, journal::CodexPaths};
use ai_personas::neural::runner::AuthRoute;

pub(in crate::desktop::neural) struct Form(CodexPaths);

impl Default for Form {
    fn default() -> Self {
        Self(helpers::codex(None).unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            CodexPaths {
                executable: String::new(),
                codex_home: std::env::var("CODEX_HOME").unwrap_or_else(|_| format!("{home}/.codex")),
                host_home: home,
            }
        }))
    }
}

impl Form {
    pub(super) fn show(&mut self, ui: &mut Ui, app: &mut App) -> Option<Command> {
        ui.strong("Model connection");
        for person in app.records("personas") {
            let p = &person["body"];
            let settings = &p["provider"];
            let route = match s(settings, "mode") {
                "chatgpt_oauth" => "ChatGPT OAuth",
                "siwc_oauth" => "OpenAI OAuth",
                "platform_api_key" => "OpenAI API key",
                _ => "Configured provider",
            };
            ui.label(format!("{} · {} · {} effort · {route}",
                s(p, "label"), s(settings, "model"), s(settings, "effort")));
        }
        ui.small("Use your signed-in Codex account for future calls by these personas.");
        let c = &mut self.0;
        ui.collapsing("Codex account location", |ui| {
            for (label, path) in [("Executable", &mut c.executable), ("Account home", &mut c.host_home), ("Codex home", &mut c.codex_home)] {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
            }
        });
        if app.running || matches!(app.runner_observation, ai_personas::neural::live::RunnerObservation::Observed { .. }) {
            ui.horizontal_wrapped(|ui| {
                ui.small("Finish the current run to change connections.");
                app.finish_run_button(ui);
            });
            return None;
        }
        let ready = app.can_start_run() && [&c.executable, &c.host_home, &c.codex_home].iter().all(|s| !s.trim().is_empty());
        ui.add_enabled(ready, egui::Button::new("Use Codex login"))
            .on_hover_text("Keep the workspace, model choices and reasoning efforts. Uses Codex for authentication only.")
            .clicked().then(|| Command::ConnectCodex(AuthRoute {
                codex_executable: c.executable.trim().into(),
                host_home: c.host_home.trim().into(),
                codex_home: c.codex_home.trim().into(),
            }))
    }
}
