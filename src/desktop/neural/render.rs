use super::*;

pub(super) struct RenderDialog {
    path: String,
    reference: String,
    parameters: String,
    companions: String,
    filter: String,
    compiler_source: String,
}

impl RenderDialog {
    pub fn new(path: &str, reference: &str) -> Self {
        Self {
            path: path.into(),
            reference: reference.into(),
            parameters: "{}".into(),
            companions: String::new(),
            filter: String::new(),
            compiler_source: String::new(),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, app: &App) -> Option<Command> {
        ui.label(RichText::new(&self.path).strong())
            .on_hover_text(&self.reference);
        let installed = app
            .records("installations")
            .iter()
            .find(|r| r["body"]["package"] == "openscad-wasm" && r["body"]["release"] == "0.0.4");
        let Some(installed) = installed else {
            ui.label("This workspace needs the OpenSCAD compiler to render this project.");
            ui.collapsing("Install from retained bytes", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.compiler_source)
                        .hint_text("Exact asset or object reference"),
                );
            });
            ui.small("The default installation downloads openscad-wasm 0.0.4 and verifies its digest and version.");
            return ui.button("Install workspace compiler").clicked().then(|| {
                Command::InstallOpenscad {
                    source: (!self.compiler_source.trim().is_empty())
                        .then(|| self.compiler_source.trim().into()),
                }
            });
        };
        ui.small(s(&installed["body"], "compiler_version"));
        ui.separator();
        ui.label("Parameter overrides");
        ui.add(
            egui::TextEdit::multiline(&mut self.parameters)
                .font(egui::TextStyle::Monospace)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        let parameters = serde_json::from_str::<BTreeMap<String, Value>>(&self.parameters);
        if let Err(error) = &parameters {
            ui.colored_label(Color32::LIGHT_RED, error.to_string());
        }
        let mut chosen: BTreeSet<String> = self
            .companions
            .lines()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned)
            .collect();
        ui.collapsing(format!("Companion files · {}", chosen.len()), |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter library paths"));
            let filter = self.filter.to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(145.)
                .show(ui, |ui| {
                    for asset in &app.snapshot.assets {
                        let reference = s(asset, "reference");
                        let path = s(&asset["body"], "path");
                        if reference == self.reference || !path.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let mut checked = chosen.contains(reference);
                        if ui
                            .checkbox(&mut checked, format!("{} · v{}", path, asset["revision"]))
                            .on_hover_text(reference)
                            .changed()
                        {
                            if checked {
                                chosen.insert(reference.to_owned());
                            } else {
                                chosen.remove(reference);
                            }
                            self.companions = chosen.iter().cloned().collect::<Vec<_>>().join("\n");
                        }
                    }
                });
            ui.label("Exact companion revisions");
            ui.add(
                egui::TextEdit::multiline(&mut self.companions)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.separator();
        ui.small(format!(
            "This render will be recorded under operator {}.",
            app.operator
        ));
        ui.add_enabled(parameters.is_ok(), egui::Button::new("Render STL"))
            .clicked()
            .then(|| Command::RenderScad {
                reference: self.reference.clone(),
                parameters: self.parameters.clone(),
                companions: self
                    .companions
                    .lines()
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
    }
}
