//! The selected activity may change behind this window. Only an explicit
//! choice moves the draft (including its already captured files) to new terms.
use super::*;

pub(in crate::desktop::neural) struct Form {
    basis: Value,
    text: String,
    sources: SourceFiles,
    pub(super) retry: Option<(String, String)>,
}

fn current<'a>(app: &'a App, work: &str) -> Option<&'a Value> {
    app.records("work").iter().find(|r| {
        let reference = &r["reference"];
        reference["kind"] == "work"
            && reference["id"] == work
            && r["body"]["id"] == work
            && reference["revision"].as_i64().is_some_and(|n| n > 0)
            && reference["digest"].is_string()
            && r["body"]["amendments"].is_array()
    })
}

fn latest_change(ui: &mut Ui, change: &Value) {
    ui.strong(format!("Latest amendment · {}", s(change, "author")));
    ui.label(excerpt(s(change, "text"), 240));
}

impl Form {
    pub(super) fn open(app: &App) -> Option<Self> {
        let basis = current(app, &app.work)?;
        (basis["body"]["phase"] == "open").then(|| Self {
            basis: basis.clone(),
            text: String::new(),
            sources: SourceFiles::default(),
            retry: None,
        })
    }

    pub(super) fn work(&self) -> &str {
        s(&self.basis["reference"], "id")
    }

    pub(super) fn show(&mut self, ui: &mut Ui, app: &mut App) -> Option<Command> {
        let brief = inbox::handle(&self.basis);
        let body = &self.basis["body"];
        let n = self.basis["reference"]["revision"].as_i64()?;
        let mut inspect = None;
        eyebrow(ui, "Activity  /  Change of terms");
        let need = s(body, "original_need");
        ui.strong(if need.is_empty() {
            "Open conversation".into()
        } else {
            home::short_text(need, 80)
        });
        ui.small(format!("Writing as {}", app.operator));
        brief_terms::show(ui, &brief, body, "Draft basis", &mut inspect);

        let mut ready = false;
        match current(app, self.work()) {
            Some(now) if now["body"]["phase"] == "dissolved" => {
                ui.colored_label(
                    theme::ACCENT,
                    "This activity has ended. Your draft has not been sent.",
                );
                brief_terms::show(
                    ui,
                    &inbox::handle(now),
                    &now["body"],
                    "Current terms",
                    &mut inspect,
                );
            }
            Some(now) if now["body"]["phase"] == "open" => {
                let latest = &now["reference"];
                if latest == &self.basis["reference"] {
                    if let Some(last) = body["amendments"].as_array().and_then(|a| a.last()) {
                        latest_change(ui, last);
                    }
                    ready = true;
                } else if let Some(next) = latest["revision"].as_i64().filter(|next| *next > n) {
                    ui.separator();
                    ui.colored_label(
                        theme::ACCENT,
                        format!("Brief {next} is current. Your draft is still based on brief {n}."),
                    );
                    let prior = body["amendments"].as_array()?;
                    let changes = now["body"]["amendments"].as_array()?;
                    if let Some(last) = changes
                        .strip_prefix(prior.as_slice())
                        .and_then(|added| added.last())
                    {
                        latest_change(ui, last);
                    }
                    brief_terms::show(
                        ui,
                        &inbox::handle(now),
                        &now["body"],
                        "Current terms",
                        &mut inspect,
                    );
                    if ui
                        .button(format!("Use brief {next} for this draft"))
                        .clicked()
                    {
                        self.basis = now.clone();
                        app.error = None;
                    }
                } else {
                    ui.colored_label(theme::ACCENT, "The stored revision no longer matches the draft basis. Inspect the workspace before starting a new amendment.");
                }
            }
            _ => {
                ui.colored_label(
                    theme::ACCENT,
                    "The current brief is unavailable. Your draft is kept.",
                );
            }
        }
        if let Some(link) = inspect {
            link.open(app, ui.ctx());
        }
        ui.separator();
        ui.strong("Your amendment");
        multiline(ui, &mut self.text, "What should change?");
        self.sources.picker.show(ui, &mut self.sources.files);
        ui.small(
            "Adds to the brief. Does not replace the original request or resume a stopped persona.",
        );
        let enabled = ready && !self.sources.picker.busy() && !self.text.trim().is_empty();
        ui.add_enabled(enabled, theme::primary("Save amendment"))
            .clicked()
            .then(|| Command::Amend {
                brief,
                expect_digest: Some(s(&self.basis["reference"], "digest").to_owned()),
                text: self.text.clone(),
                files: InputFiles::from(self.sources.files.clone()),
            })
    }
}
