use super::*;
use ai_personas::persona_model::Evaluation;
mod appraisal;
mod follow_ups;
use appraisal::Basis;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InputView {
    #[default]
    Next,
    Recorded,
    Baseline,
}

impl App {
    fn evaluation(&self, view: InputView) -> Option<(Evaluation, bool)> {
        let character = self.snapshot.characters.get(&self.persona)?;
        let part = self.participation(&self.persona);
        let recorded = part["last_evaluation"]
            .as_str()
            .and_then(|r| self.snapshot.evaluations.get(r));
        let contextual = match view {
            InputView::Baseline => false,
            InputView::Recorded => true,
            InputView::Next => {
                let key = (self.work.clone(), self.persona.clone());
                match Basis::observed(&part, self.snapshot.appraisal_provenance.get(&key)) {
                    Basis::Contextual => true,
                    Basis::Baseline => false,
                    _ => return None,
                }
            }
        };
        if contextual {
            let evaluation = recorded?;
            if view == InputView::Next && evaluation.model != character.baseline.model {
                return None;
            }
            Some((evaluation.clone(), true))
        } else {
            Some((character.baseline.clone(), false))
        }
    }

    fn appraisal_origin(&mut self, ui: &mut Ui, compact: bool) {
        let key = (self.work.clone(), self.persona.clone());
        let part = self.participation(&self.persona);
        let provenance = self.snapshot.appraisal_provenance.get(&key);
        let opened = ui
            .push_id(("appraisal-origin", &key), |ui| {
                appraisal::panel(ui, &part, provenance, compact, |id| {
                    self.label(id).to_owned()
                })
            })
            .inner;
        if let Some(reference) = opened {
            self.open_resource(ui.ctx(), reference);
        }
    }

    fn character_view_options(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.character_input, InputView::Next, "Next input");
            ui.selectable_value(
                &mut self.character_input,
                InputView::Recorded,
                "Recorded appraisal",
            );
            ui.selectable_value(&mut self.character_input, InputView::Baseline, "Baseline");
        });
    }

    pub(super) fn show_inspector_window(&mut self, ctx: &egui::Context) {
        if !self.inspector_window { return; }
        let mut open = true;
        let bounds = ctx.content_rect();
        // The compact workspace uses a floating inspector. Its previous size
        // cannot be trusted after zoom or viewport changes.
        let width = (bounds.width()-40.).max(1.);
        egui::Window::new("Inspector")
            .open(&mut open)
            .min_width(1.)
            .max_width(width)
            .default_width(330.0_f32.min(width))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((bounds.height()-100.).max(80.))
                    .auto_shrink([false,true])
                    .show(ui, |ui| self.inspector(ui));
            });
        self.inspector_window = open;
    }

    pub(super) fn inspector_panel(&mut self, ctx: &egui::Context) {
        let p = theme::Palette::new(self.layout.dark);
        let panel = egui::SidePanel::right("studio-inspector")
            .resizable(true)
            .default_width(self.layout.right_width)
            .width_range(260.0..=460.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(14))
            .show(ctx, |ui| {
                eyebrow(ui, "Inspector");
                ui.add_space(12.);
                egui::ScrollArea::vertical()
                    .id_salt("inspector-body")
                    .show(ui, |ui| self.inspector(ui));
            });
        self.layout.right_width = panel.response.rect.width();
    }
    pub(super) fn inspector(&mut self, ui: &mut Ui) {
        if self.layout.editor == Editor::Artifacts && !self.selected_reference.is_empty() {
            self.artifact_inspector(ui);
            return;
        }
        let person = self.selected_person();
        if person.is_null() {
            ui.weak("Select a persona in the outliner.");
            return;
        }
        let label = s(&person, "label");
        let color = self.color(&self.persona);
        ui.horizontal(|ui| {
            avatar(ui, label, color, 46.);
            ui.vertical(|ui| {
                ui.heading(label);
                ui.label(
                    RichText::new(self.persona_state(&self.persona))
                        .small()
                        .color(color),
                );
            });
        });
        ui.add_space(15.);
        eyebrow(ui, "Reasoning model");
        ui.add_space(4.);
        ui.strong(s(&person["provider"], "model"));
        ui.small(format!(
            "{} reasoning · {}",
            s(&person["provider"], "effort"),
            s(&person, "lifecycle")
        ));
        if ui.small_button("Change reasoning settings…").clicked() {
            if let Some(record) = self.records("personas").iter().find(|p| p["body"]["id"] == self.persona) {
                self.dialog = Some(Dialog::Reasoning { persona:self.persona.clone(),
                    expected:format!("persona:{}@{}",self.persona,record["reference"]["revision"]),
                    model:s(&person["provider"],"model").into(),effort:s(&person["provider"],"effort").into(),
                    reason:String::new() });
            }
        }
        ui.add_space(14.);
        let part = self.participation(&self.persona);
        ui.horizontal(|ui| {
            theme::badge(
                ui,
                &format!("Epoch {}", person["epoch"].as_u64().unwrap_or(0)),
                color,
            );
            ui.small(counted(part["turn"].as_u64().unwrap_or(0), "decision", "decisions"));
        });
        if !s(&part["last_judgment"], "reason").is_empty() {
            ui.collapsing("Current work & intention", |ui| {
                ui.label(s(&part["last_judgment"], "reason"));
                ui.add_space(5.);
                ui.weak(s(&part["continuation"], "purpose"));
            });
        }
        ui.add_space(16.);
        section(ui, "Character inclinations", "−1 … +1");
        if self.snapshot.characters.contains_key(&self.persona) {
            self.appraisal_origin(ui, true);
            if let Some((evaluation, _)) = self.evaluation(InputView::Next) {
                ui.add_space(8.);
                self.output_bars(ui, &evaluation, true);
            }
        } else {
            ui.weak("The persona has not activated a character model yet.");
        }
        ui.add_space(12.);
        if ui
            .add_sized(
                [ui.available_width(), 32.],
                theme::primary("Open Character Lab →"),
            )
            .clicked()
        {
            self.layout.editor = Editor::Character;
        }
        ui.add_space(12.);
        ui.collapsing("Starting profile", |ui| {
            ui.small(format!("Attributed to {}", s(&person, "seed_author")));
            ui.label(s(&person, "seed"));
        });
    }
    fn render_history(&self, ui:&mut Ui, metadata:&Value) -> Option<String> {
        let content=metadata["blob"].as_str().map(|digest|format!("object:{digest}"));
        let runs=self.model_renders.get(&self.selected_reference)
            .or_else(||content.as_ref().and_then(|reference|self.model_renders.get(reference)))?;
        let mut open=None;
        ui.add_space(10.);
        for &index in runs.iter().rev() {
            let event=&self.events[index];
            let result=&event["tool_outcome"];
            let newest=Some(&index)==runs.last();
            let by=if event["kind"]=="operator_render" {format!("Operator · {}",s(event,"actor"))} else {self.label(s(event,"actor")).to_owned()};
            egui::CollapsingHeader::new(format!("{} · {by}",s(result,"label")))
                .id_salt(("cad-render",&self.selected_reference,index)).default_open(newest).show(ui,|ui|{
                    ui.small(s(event,"created_utc"));
                    if let Some(version)=result["compiler_version"].as_str() { ui.small(version); }
                    let geometry=&result["geometry"];
                    if let Some(triangles)=geometry["triangles"].as_u64() {
                        ui.strong(format!("{triangles} triangles"));
                        egui::Grid::new(("mesh-bounds",index)).spacing([9.,2.]).show(ui,|ui|{
                            for label in ["Axis","Min","Max","Span"] { ui.small(label); }
                            ui.end_row();
                            for (axis,name) in ["X","Y","Z"].iter().enumerate() {
                                ui.strong(*name);
                                for key in ["minimum","maximum","span"] {
                                    if let Some(v)=geometry[key][axis].as_f64() {ui.monospace(format!("{v:.2}")).on_hover_text(v.to_string());}
                                }
                                ui.end_row();
                            }
                        });
                        ui.small("Model coordinates · STL stores no unit");
                    }
                    if let Some(error)=result["error"].as_str() { ui.label(error); }
                    for key in ["warnings","errors"] {
                        for line in result[key].as_array().into_iter().flatten().filter_map(Value::as_str) {
                            ui.small(line);
                        }
                    }
                    ui.horizontal_wrapped(|ui|{
                        for (key,label) in [("mesh","Open mesh"),("source","Editable source"),("result","Render receipt"),("diagnostics","Compiler log"),("installation","Compiler")] {
                            if let Some(reference)=result[key].as_str() {
                                if reference!=self.selected_reference && Some(reference)!=content.as_deref() && ui.small_button(label).clicked() {open=Some(reference.to_owned());}
                            }
                        }
                    });
                });
        }
        open
    }
    fn artifact_inspector(&mut self, ui: &mut Ui) {
        let asset = self
            .snapshot
            .assets
            .iter()
            .find(|a| a["reference"] == self.selected_reference)
            .cloned();
        let metadata = asset
            .as_ref()
            .map(|a| a["body"].clone())
            .filter(|_| self.preview_metadata.is_null())
            .unwrap_or_else(|| self.preview_metadata.clone());
        theme::glyph(ui, Icon::Files, theme::ACCENT);
        ui.add_space(8.);
        let path = s(&metadata, "path");
        ui.add(egui::Label::new(RichText::new(if path.is_empty() {
            match metadata["kind"].as_str() { _ if metadata["view"]=="provider_receipt_metadata" => "Provider receipt metadata", Some("job") => "Background command", Some("control") => "Execution control", Some("installation")=>"Installed program", Some("command_run")=>"Execution receipt", _ if metadata["format"]=="stl"=>"STL mesh", _ if self.model_renders.contains_key(&self.selected_reference)=>"OpenSCAD render receipt", _ => "Exact resource" }
        } else {
            path.rsplit('/').next().unwrap_or(path)
        }).size(18.).strong()).wrap());
        if !path.is_empty() && metadata["source_record"].is_null() {
            ui.add(egui::Label::new(RichText::new(path).small().weak()).wrap());
        }
        ui.add_space(12.);
        if !s(&metadata, "owner").is_empty() {
            theme::property(ui, "Owner", self.label(s(&metadata, "owner")));
        }
        if let Some(asset) = &asset {
            theme::property(ui, "Version", &asset["revision"].to_string());
            theme::property(ui, "Size", &asset_size(asset));
            theme::property(
                ui,
                "Retained",
                &time_label(asset["created"].as_i64().unwrap_or(0)),
            );
        }
        ui.add_space(12.);
        if let Some(reference)=self.render_history(ui,&metadata) { self.open_resource(ui.ctx(),reference); return; }
        eyebrow(ui, "Exact reference");
        ui.add(
            egui::Label::new(
                RichText::new(&self.selected_reference)
                    .monospace()
                    .size(11.),
            )
            .wrap(),
        );
        if ui.small_button("Copy reference").clicked() {
            ui.ctx().copy_text(self.selected_reference.clone());
        }
        if let Some(digest) = metadata["blob"].as_str().or_else(|| metadata["record_digest"].as_str()) {
            ui.add_space(12.);
            eyebrow(ui, if metadata["blob"].is_string() { "File SHA-256" } else { "Record SHA-256" });
            ui.add(egui::Label::new(RichText::new(digest).monospace().size(11.).weak()).wrap());
        }
        if metadata["kind"] == "control" {
            let control = &metadata["control"];
            ui.add_space(15.);
            ui.label(RichText::new("Control state").strong());
            ui.weak("At this recorded revision");
            for (label, value) in [
                ("Scope", s(control, "scope").to_owned()),
                ("State", title(s(control, "state"))),
                ("Control revision", control["revision"].to_string()),
                ("Changed by", s(&control["origin"], "actor").to_owned()),
                ("Origin", s(&control["origin"], "kind").to_owned()),
                ("Recorded", control["updated"].as_i64().map(time_label).unwrap_or_default()),
            ] {
                ui.add_space(5.);
                ui.label(RichText::new(label).small().weak());
                ui.add(egui::Label::new(value).wrap());
            }
            ui.add_space(8.);
            ui.label(s(control, "reason"));
            ui.weak("A control change is separate from process termination and participation resumption.");
            if let Some(reference) = control["previous"]["record"].as_str() {
                if ui.button("Open previous control record").clicked() { self.open_resource(ui.ctx(), reference.into()); }
            }
            return;
        }
        if let Some(receipt) = metadata["command_run"].as_str() {
            ui.add_space(12.);
            section(ui, if metadata["command_file_role"]=="outputs" {"Produced by"} else {"Supplied to"}, "");
            ui.strong(s(&metadata,"command_name"));
            ui.label(format!("{} · {}",s(&metadata,"command_origin"),self.label(s(&metadata,"command_actor"))));
            if let Some(code)=metadata["command_exit"].as_i64(){ui.small(format!("Exit {code}"));}
            if ui.add_enabled(!self.busy,egui::Button::new("Open execution receipt")).clicked() {
                let command=self.command_bench.locate(receipt.into());
                self.command(ui.ctx(),command);
            }
            return;
        }
        if let Some(source) = metadata["source_record"].as_str() {
            ui.add_space(12.);
            section(ui, "Attached to", "");
            let label = if source.starts_with("work:") {
                format!("Brief revision {}", source.rsplit_once('@').map_or("", |(_, rev)| rev))
            } else if source.starts_with("human_request:") {
                "Human request report".to_owned()
            } else {
                "Recorded exchange".to_owned()
            };
            if ui.link(label).on_hover_text(source).clicked() {
                self.open_resource(ui.ctx(), source.to_owned());
            }
            if let Some(bytes) = metadata["bytes"].as_u64() {
                ui.small(format!("{} bytes", number(bytes)));
            }
            return;
        }
        if metadata["kind"] == "job" {
            let job = &metadata["job"];
            ui.add_space(15.);
            section(ui, "Execution", "At this recorded revision");
            theme::property(ui, "Version", &metadata["version"].to_string());
            ui.strong(match s(job, "state") {
                "submitted" => "Submitted; launch not yet recorded",
                "running" => "Process running",
                "completed" => "Terminal result recorded",
                "not_started" => "Process not started",
                "uncertain" => "Outcome uncertain",
                state => state,
            });
            for (field, label) in [("submitted", "Submitted"), ("started", "Launched"), ("finished", "Finished")] {
                if let Some(time) = job[field].as_i64() {
                    theme::property(ui, label, &time_label(time));
                }
            }
            if let Some(reason) = job["cancellation"]["reason"].as_str() {
                ui.label(format!("Cancellation requested: {reason}"));
            }
            for (field, label) in [("input", "Open command input"), ("result", "Open terminal receipt")] {
                if let Some(reference) = job[field].as_str() {
                    if ui.button(label).clicked() { self.open_resource(ui.ctx(), reference.into()); }
                }
            }
            let identity = self.selected_reference.rsplit_once('@').map(|(id, _)| id);
            let latest = self.snapshot.status["jobs"].as_array().into_iter().flatten()
                .filter_map(|job| job["reference"].as_str())
                .find(|reference| reference.rsplit_once('@').map(|(id, _)| id) == identity)
                .map(str::to_owned);
            if let Some(reference) = latest.filter(|reference| reference != &self.selected_reference) {
                if ui.button("Open latest job revision").clicked() { self.open_resource(ui.ctx(), reference); }
            }
            return;
        }
        if !self.selected_reference.starts_with("asset:") {
            if metadata["format"]!="stl" {
                ui.add_space(15.);
                section(ui, "Retained record", "");
                if metadata["view"]=="provider_receipt_metadata" {
                    ui.weak("This view shows the provider audit, usage and output-item counts. The reference identifies the original receipt.");
                } else {
                    ui.weak("The preview shows this exact stored record. Later observations remain separate.");
                }
            }
            return;
        }
        ui.add_space(15.);
        section(ui, "Qualifications", "From the author");
        let qualifications = metadata["qualifications"].as_array();
        if qualifications.is_none_or(|v| v.is_empty()) {
            ui.weak("No qualifications are recorded on this resource.");
        }
        for q in qualifications.into_iter().flatten() {
            ui.label(format!("• {}", q.as_str().unwrap_or("")));
            ui.add_space(4.);
        }
        let mut selected_result = None;
        if let Some(runs) = self.program_runs.get(&self.selected_reference) {
            ui.add_space(10.);
            egui::CollapsingHeader::new(format!("Recorded runs · {}",runs.len()))
                .id_salt(("program-runs",&self.selected_reference)).default_open(runs.len()==1)
                .show(ui,|ui| {
                    for &index in runs.iter().rev() {
                        let event = &self.events[index];
                        let result=&event["tool_outcome"];
                        ui.small(format!("{} · {}",self.label(s(event,"actor")),s(result,"label")));
                        if ui.small_button(s(event,"created_utc")).on_hover_text("Open the exact execution result").clicked() {
                            selected_result=Some(s(result,"result").to_owned());
                        }
                    }
                });
        }
        if let Some(reference)=selected_result { self.open_resource(ui.ctx(),reference); return; }
        if let Some(cues) = metadata["cues"].as_array() {
            ui.add_space(12.);
            eyebrow(ui, "Discovery cues");
            for cue in cues {
                ui.add(egui::Label::new(RichText::new(cue.as_str().unwrap_or("")).small().color(theme::ACCENT)).wrap());
            }
        }
        let deliveries: Vec<Value> = self
            .records("deliveries")
            .iter()
            .filter(|d| {
                d["body"]["artifacts"].as_array().is_some_and(|a| {
                    a.iter()
                        .any(|r| r == &Value::String(self.selected_reference.clone()))
                })
            })
            .cloned()
            .collect();
        ui.add_space(15.);
        section(ui, "Delivery status", "");
        if deliveries.is_empty() {
            ui.weak("Retained working artifact. No delivery includes this exact revision.");
        }
        for d in deliveries {
            let b = &d["body"];
            ui.strong(s(b, "title"));
            theme::badge(ui, s(b, "scope"), theme::ACCENT);
            self.delivery_review(ui, &d);
            ui.label(s(b, "supported_use"));
            ui.add_space(8.);
        }
    }
    pub(super) fn character(&mut self, ui: &mut Ui) {
        eyebrow(ui, "Character Lab");
        ui.horizontal_wrapped(|ui| {
            ui.heading("Character models");
            if ui.button("Character Atlas →").on_hover_text("Compare people and generations on the same input").clicked() {
                self.layout.editor = Editor::Atlas;
            }
        });
        ui.add_space(10.);
        ui.horizontal_wrapped(|ui| {
            for p in self.records("personas").to_vec() {
                let b = &p["body"];
                let id = s(b, "id");
                if ui
                    .add(
                        egui::Button::new(RichText::new(s(b, "label")).color(self.color(id)))
                            .selected(self.persona == id),
                    )
                    .clicked()
                {
                    self.persona = id.into();
                }
            }
        });
        let person = self.selected_person();
        let Some(character) = self.snapshot.characters.get(&self.persona).cloned() else {
            ui.add_space(20.);
            ui.weak("The selected persona has no active numerical character model yet.");
            return;
        };
        ui.add_space(12.);
        ui.horizontal_wrapped(|ui| {
            theme::badge(
                ui,
                &format!("Epoch {}", person["epoch"].as_u64().unwrap_or(0)),
                self.color(&self.persona),
            );
            ui.small(format!(
                "{} inputs · {} · {} outputs · {}",
                FeatureContract::standard().inputs.len(),
                counted(character.model.hidden_units as u64, "hidden unit", "hidden units"),
                FeatureContract::standard().outputs.len(),
                counted(character.model.parameters.len() as u64, "stored coefficient", "stored coefficients")
            ));
        });
        ui.add_space(12.);
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["Characteristics", "Network", "Parameters", "Revisions", "Development", "What if…", "Life history", "Comparisons", "Relationships"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.character_tab, i, *label);
            }
        });
        ui.separator();
        match self.character_tab {
            0 => self.character_profile(ui, &person, &character),
            1 => self.character_network(ui, &character),
            2 => self.parameter_table(ui, &character),
            4 => self.character_development(ui),
            6 => self.life_history(ui),
            7 => self.comparisons(ui),
            8 => self.show_relationships(ui),
            5 => egui::ScrollArea::vertical().id_salt("character-explorer").show(ui,|ui| {
                self.explorer.show(ui,&character);
            }).inner,
            _ => self.character_revisions(ui, &character),
        }
    }
    fn character_profile(&mut self, ui: &mut Ui, person: &Value, character: &data::Character) {
        egui::ScrollArea::vertical().id_salt("character-profile").show(ui, |ui| {
            self.appraisal_origin(ui, false);
            ui.add_space(10.);
            self.character_view_options(ui);
            let Some((evaluation, contextual)) = self.evaluation(self.character_input) else {
                ui.weak(if self.character_input == InputView::Recorded {
                    "A recorded evaluation is unavailable for this persona in this activity."
                } else {
                    "The next numerical input is unavailable for this activity."
                });
                return;
            };
            if self.character_input == InputView::Recorded {
                ui.small(if evaluation.model == character.baseline.model {
                    "Showing the saved numerical evaluation for this activity."
                } else {
                    "Showing the saved numerical evaluation from an earlier model."
                });
            } else if self.character_input == InputView::Baseline {
                ui.small("Showing the active model with all appraisal inputs unknown.");
            }
            ui.add_space(14.);
            let bars = |ui: &mut Ui| {
                section(ui, "Forward inclinations", "−1 … +1");
                self.output_bars(ui, &evaluation, false);
            };
            let intent = |ui: &mut Ui| {
                section(ui, "Authored intent", "Active model");
                ui.add(egui::Label::new(&character.model.intent).wrap());
                ui.add_space(16.);
                ui.collapsing("Starting profile & attribution", |ui| {
                    ui.small(format!("From {}", s(person, "seed_author")));
                    ui.label(s(person, "seed"));
                });
                ui.add_space(12.);
                ui.small("The persona authors numerical edits. Activation and rollback have separate records.");
            };
            if ui.available_width() >= 600. {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.;
                    ui.columns(2, |cols| { bars(&mut cols[0]); intent(&mut cols[1]); });
                });
            } else {
                bars(ui);
                ui.add_space(14.);
                intent(ui);
            }
            ui.add_space(20.);
            ui.horizontal_wrapped(|ui| {
                ui.strong("Appraisal inputs");
                ui.small(if contextual { "Recorded values & evidence" } else { "Unknown at baseline" });
            });
            ui.add_space(7.);
            for axis in FeatureContract::standard().inputs {
                let feature = evaluation.appraisal.features.get(&axis.id);
                let value = feature.and_then(|f| f.value).map_or_else(|| "unknown".into(), |v| format!("{v:+.3}"));
                ui.collapsing(format!("{}   {value}", title(&axis.id)), |ui| {
                    ui.weak(&axis.meaning);
                    if let Some(feature) = feature {
                        ui.label(&feature.interpretation);
                        for reference in &feature.evidence {
                            if ui.small_button(excerpt(reference, 55)).on_hover_text(reference).clicked() {
                                self.open_resource(ui.ctx(), reference.clone());
                            }
                        }
                    }
                });
            }
        });
    }

    fn character_network(&mut self, ui: &mut Ui, character: &data::Character) {
        egui::ScrollArea::vertical().id_salt("character-network-tab").show(ui, |ui| {
            if let Some(decision) = self.character_decisions.iter().rev().find(|d|
                d["actor"] == self.persona && d["work"] == self.work && d["character_use"].is_string()) {
                if ui.button("Trace the latest decision…")
                    .on_hover_text("Inspect the numerical input supplied to the latest accepted choice in this activity").clicked() {
                    self.character_paths.open(ui.ctx(), &self.root, decision);
                }
            }
            self.appraisal_origin(ui, false);
            ui.add_space(10.);
            self.character_view_options(ui);
            let Some((evaluation, _)) = self.evaluation(self.character_input) else {
                ui.weak("The selected numerical input is unavailable for this activity.");
                return;
            };
            if evaluation.model != character.baseline.model {
                if ui.link("Open the earlier model used by this appraisal").clicked() {
                    self.open_resource(ui.ctx(), format!("object:{}", evaluation.model));
                }
                return;
            }
            ui.small("Teal shows positive weights and orange shows negative weights. Biases appear in Parameters.");
            ui.add_space(8.);
            egui::ScrollArea::horizontal().id_salt("character-network")
                .show(ui, |ui| self.network(ui, character, &evaluation));
        });
    }

    fn character_development(&mut self, ui: &mut Ui) {
        let scope = (self.persona.clone(), self.work.clone());
        if self.development_scope != scope {
            self.development_scope = scope;
            self.development_anchor.clear();
        }
        let width = ui.available_width();
        egui::ScrollArea::vertical()
            .id_salt(("character-development", &self.persona, &self.work, &self.development_anchor,
                self.development_filter, self.development_all_work))
            .show(ui, |ui| {
                ui.set_max_width(width.min(ui.available_width()));
                self.character_development_contents(ui);
            });
    }
    fn character_development_contents(&mut self, ui: &mut Ui) {
        let questions:Vec<_>=self.records("development").iter()
            .filter(|q|q["body"]["owner"]==self.persona).cloned().collect();
        section(ui,"Development questions",&format!("{}",questions.len()));
        if questions.is_empty() { ui.weak("This persona has not opened a development question."); }
        for question in questions {
            let body=&question["body"];
            let detail=&body["interpretation"];
            let id=s(&question["reference"],"id");
            let reference=format!("development:{id}@{}",question["reference"]["revision"]);
            let applications:Vec<_>=self.records("question_applications").iter().filter(|r|r["question_id"]==id).cloned().collect();
            ui.push_id(&reference,|ui|{
                let text=s(detail,"question");
                let pending = applications.iter().flat_map(|a| a["linked_responses_now"].as_array().into_iter().flatten())
                    .filter(|r| r["interpretations"].as_array().is_some_and(Vec::is_empty)).count();
                let mut heading = excerpt(text,if ui.available_width() < 510. {44} else {68});
                let closed = detail["disposition"] == "closed";
                let follow_ups = body["follow_ups"].as_array().map_or(0, Vec::len);
                if follow_ups > 0 { heading.push_str(&format!(" · {}", counted(follow_ups as u64, "follow-up", "follow-ups"))); }
                let practice = body["disposition_subject"]["kind"] == "practice_application";
                let disposition = detail["disposition"].as_str().map(|s| match (practice, s) {
                    (true, "supported") => "Practice supported".to_owned(),
                    (true, _) => "Practice observed".to_owned(),
                    _ => title(s),
                });
                if let Some(status) = &disposition { heading.push_str(&format!(" · {status}")); }
                if pending > 0 { heading.push_str(&format!(" · {pending} returns without a recorded finding")); }
                // CollapsingHeader's default single-line text otherwise widens the entire scroll area.
                let wrap = (ui.available_width() - ui.spacing().indent - 2. * ui.spacing().button_padding.x).max(1.);
                let heading = egui::WidgetText::from(heading).into_galley(ui, Some(egui::TextWrapMode::Wrap), wrap, egui::TextStyle::Button);
                egui::CollapsingHeader::new(heading).id_salt("question").show(ui,|ui|{
                    ui.add(egui::Label::new(RichText::new(text).strong()).wrap());
                    if let Some(status) = &disposition {
                        ui.horizontal_wrapped(|ui| {
                            ui.small("Recorded disposition");
                            theme::badge(ui, status, theme::palette(ui).muted);
                            if let Some(call) = body["origin_decision"].as_str() {
                                if ui.link("Opening decision").clicked() {
                                    self.open_resource(ui.ctx(), format!("experience:{call}@1"));
                                }
                            }
                        });
                    }
                    if closed {
                        if let Some(reason) = body["closure"]["reason"].as_str() {
                            ui.label(format!("Closed · {reason}"));
                            if let Some(decision) = body["closure"]["decision"].as_str() {
                                if ui.small_button("Closure decision ↗").clicked() { self.open_resource(ui.ctx(),format!("experience:{decision}@1")); }
                            }
                        }
                    }
                    if practice { ui.weak("This verdict is about one use of a practice, not changed weights. Trace the question history to check its evidence."); }
                    self.development_follow_ups(ui, &reference, body);
                    if let Some(text)=detail["interpretation"].as_str(){ui.label(text);}
                    if let Some(text)=detail["prediction"].as_str(){ui.small(format!("Prediction · {text}"));}
                    if let Some(text)=detail["alternatives"].as_str(){ui.small(format!("Alternative considered · {text}"));}
                    if let Some(condition)=detail["revisit"]["condition"].as_str(){
                        let label=if closed {"Previous revisit condition"} else {"Revisit when"};
                        ui.small(format!("{label} · {condition}"));
                    }
                    for r in detail["evidence"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                        if ui.small_button(r).clicked(){self.open_resource(ui.ctx(),r.into());}
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("Open full question").clicked(){self.open_resource(ui.ctx(),reference.clone());}
                        if ui.small_button("Trace question history…").clicked(){
                            self.question_history.open(ui.ctx(),&self.root,reference.clone(),text.to_owned());
                        }
                    });
                    for application in &applications {
                        ui.separator();
                        let work=self.records("work").iter().find(|w|w["body"]["id"]==application["work"])
                            .map(|w|activity_title(&w["body"])).unwrap_or("Activity");
                        let role = if application["action"].is_object() { "Tried an action" } else { "Examined this question" };
                        ui.strong(format!("{role} in {}",excerpt(work,88)));
                        ui.label(s(application,"watch_for"));
                        if ui.small_button(format!("Application record · epoch {}",application["epoch"])).clicked(){
                            self.open_resource(ui.ctx(),s(application,"reference").into());
                        }
                        for feedback in application["linked_responses_now"].as_array().into_iter().flatten(){
                            let relation=title(s(feedback,"relation"));
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button(format!("{relation} #{} from {} ↗",feedback["sequence"],self.label(s(feedback,"author"))))
                                    .on_hover_text(s(feedback,"experience")).clicked(){
                                    self.open_resource(ui.ctx(),s(feedback,"experience").into());
                                }
                                if let Some(interpretations) = feedback["interpretations"].as_array() {
                                    if interpretations.is_empty() { ui.weak("No recorded question finding"); }
                                    for decision in interpretations.iter().filter_map(Value::as_str) {
                                        if ui.small_button("Interpreted ↗").on_hover_text(decision).clicked() {
                                            self.open_resource(ui.ctx(),decision.into());
                                        }
                                    }
                                }
                            });
                        }
                        for item in body["question_observations"].as_array().into_iter().flatten().filter(|o|o["observation"]["application"]==application["reference"]){
                            let note=&item["observation"];
                            ui.strong(title(s(note,"finding")));
                            ui.label(s(note,"interpretation"));
                            if ui.small_button("Observed outcome ↗").clicked(){self.open_resource(ui.ctx(),s(note,"outcome").into());}
                        }
                    }
                });
            });
        }
        ui.add_space(14.);

        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut self.development_all_work, "Across all tasks").changed() {
                self.development_anchor.clear();
            }
            ui.weak(if self.development_all_work { "Continuing persona history" } else { "Selected task only" });
        });
        let decisions: Vec<&Value> = self.character_decisions.iter().rev()
            .filter(|d| d["actor"] == self.persona && (self.development_all_work || d["work"] == self.work))
            .collect();
        if decisions.is_empty() {
            ui.add_space(15.);
            ui.weak("No accepted character-maintenance decisions in this scope.");
            return;
        }
        let no_change = decisions.iter().filter(|d| d["maintenance"]["kind"] == "no_change").count();
        let trials = decisions.iter().filter(|d| d["maintenance"]["kind"] == "trial").count();
        let edits = trials + decisions.iter().flat_map(|d| d["operations"].as_array().into_iter().flatten())
            .filter(|op| op["tool"] == "persona.patch").count();
        let bindings = decisions.iter().filter(|d| matches!(d["next_model"]["kind"].as_str(), Some("activate" | "rollback"))).count();
        ui.add_space(12.);
        card(ui).show(ui, |ui| {
            let labels = [counted(decisions.len() as u64, "accepted decision", "accepted decisions"),
                    counted(no_change as u64, "no-change choice", "no-change choices"),
                    format!("{edits} parameter {}", if edits == 1 { "proposal" } else { "proposals" }),
                    format!("{bindings} model {}", if bindings == 1 { "switch" } else { "switches" })];
            let columns = if ui.available_width() >= 820. { 4 } else { 2 };
            ui.columns(columns, |columns| {
                let count = columns.len();
                for (index, label) in labels.iter().enumerate() {
                    theme::badge(&mut columns[index % count], label, self.color(&self.persona));
                    columns[index % count].add_space(6.);
                }
            });
            ui.add_space(8.);
            ui.weak("Includes initialization. Model switches are accepted activation or rollback choices. Counts describe decisions, not useful learning; appraisals, parameter proposals and comparisons remain distinct.");
        });
        ui.add_space(12.);
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["All choices", "No change", "Declared change", "Deferred", "Personal trials", "Behavior after a result", "Before acting"].iter().enumerate() {
                if ui.selectable_value(&mut self.development_filter, index, *label).changed() {
                    self.development_anchor.clear();
                }
            }
        });
        let visible: Vec<&Value> = decisions.into_iter().filter(|d| match self.development_filter {
            1 => d["maintenance"]["kind"] == "no_change",
            2 => d["maintenance"]["kind"] == "change",
            3 => d["maintenance"]["kind"] == "defer",
            4 => d["maintenance"]["kind"] == "trial",
            5 => d["behavior"].is_object(),
            6 => d["next_appraisal"]["kind"] == "before_action" || d["pre_action"].is_object(),
            _ => true,
        }).collect();
        const PAGE: usize = 25;
        let mut start = visible.iter().position(|d| d["invocation"] == self.development_anchor).unwrap_or(0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(start > 0, egui::Button::new("← Newer")).clicked() {
                start = start.saturating_sub(PAGE);
                self.development_anchor = if start == 0 { String::new() } else { s(visible[start], "invocation").into() };
            }
            ui.small(format!("{}–{} of {} matching decisions", if visible.is_empty() { 0 } else { start + 1 }, (start + PAGE).min(visible.len()), visible.len()));
            if ui.add_enabled(start + PAGE < visible.len(), egui::Button::new("Older →")).clicked() {
                start += PAGE;
                self.development_anchor = s(visible[start], "invocation").into();
            }
        });
        // Only the visible page is copied before opening a resource can mutate
        // the editor; retaining history does not require laying it all out.
        let page: Vec<Value> = visible.iter().skip(start).take(PAGE)
            .map(|d| (*d).clone()).collect();
        ui.add_space(8.);
        ui.vertical(|ui| {
                if page.is_empty() { ui.weak("No accepted choices match this filter."); }
                for decision in &page {
                    ui.push_id(s(decision, "invocation"), |ui| {
                        card(ui).show(ui, |ui| {
                            let maintenance = &decision["maintenance"];
                            ui.horizontal_wrapped(|ui| {
                                let kind = s(maintenance, "kind");
                                let label = if kind.is_empty() { "Disposition unavailable".into() } else { title(kind) };
                                theme::badge(ui, &label, self.color(&self.persona));
                                ui.small(decision["accepted_utc"].as_str().map(str::to_owned)
                                    .unwrap_or_else(|| format!("Unix {}", decision["accepted"])));
                                let next = s(&decision["next_model"], "kind");
                                ui.weak(if next.is_empty() { "Next model unavailable".into() } else { format!("Next model: {}", title(next)) });
                            });
                            ui.add_space(7.);
                            let reason = s(maintenance, "reason");
                            ui.add(egui::Label::new(if reason.is_empty() { "No reason is present in this retained field; inspect the exact decision." } else { reason }).wrap());
                            let behavior=&decision["behavior"];
                            if let Some(scope)=behavior["kind"].as_str() {
                                ui.strong(match scope {"local"=>"Kept this result local", "undecided"=>"Reserved judgment", "next_time"=>"Experiment for a later occasion", "practice"=>"Tried an approach in this decision", _=>"Behavioral choice"});
                                if let Some(reason)=behavior["reason"].as_str(){ui.label(reason);}
                                let plan=&behavior["plan"];
                                if plan.is_object(){
                                    for (label,key) in [("When","when"),("Approach","approach"),("Alternative","alternative"),("Prediction","watch_for"),("Worth preserving","preserve")] {
                                        if let Some(text)=plan[key].as_str(){ui.label(format!("{label}: {text}"));}
                                    }
                                    if let Some(r)=plan["outcome"].as_str(){if ui.small_button("Observed consequence →").on_hover_text(r).clicked(){self.open_resource(ui.ctx(),r.into());}}
                                    if behavior["action"].is_object(){ui.label(format!("Action tried: {}",behavior["action"]));}
                                    ui.weak("A forecast, not a finding. Look for a later observation and the author’s interpretation.");
                                }
                            }
                            if maintenance["kind"] == "trial" {
                                ui.strong(s(maintenance, "question"));
                                for (label, key) in [("Prediction", "prediction"), ("Worth preserving", "preserve"), ("Other possible cause", "alternative_explanation")] {
                                    ui.label(format!("{label}: {}", s(maintenance, key)));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    for (label, key) in [("Earlier choice", "decision"), ("Observed outcome", "outcome")] {
                                        if let Some(r) = maintenance[key].as_str() {
                                            if ui.small_button(label).on_hover_text(r).clicked() { self.open_resource(ui.ctx(), r.into()); }
                                        }
                                    }
                                });
                                ui.horizontal_wrapped(|ui| {
                                    for (label, key) in [("Candidate as proposed", "candidate"), ("Personal question", "development"), ("Before & after on recorded inputs", "result")] {
                                        if let Some(r) = decision["trial"][key].as_str() {
                                            if ui.small_button(label).on_hover_text(r).clicked() { self.open_resource(ui.ctx(), r.into()); }
                                        }
                                    }
                                });
                                ui.weak("Proposed numerical experiment. Comparison, adoption and benefit are separate observations.");
                            }
                            for version in decision["deferred_questions"].as_array().into_iter().flatten() {
                                if let Some(reference) = version["written"].as_str() {
                                    if ui.small_button("Open the question this choice retained →").on_hover_text(reference).clicked() {
                                        self.open_resource(ui.ctx(), reference.into());
                                    }
                                }
                            }
                            if let Some(source) = decision["pre_action"]["source"].as_str() {
                                ui.strong("Received the pre-action appraisal");
                                ui.label(s(&decision["judgment"], "reason"));
                                if ui.small_button("Open earlier pause →").on_hover_text(source).clicked() {
                                    self.open_resource(ui.ctx(), source.to_owned());
                                }
                                ui.weak("The alternatives and numerical result reached this choice. That does not show what the person would have chosen without them.");
                            } else if let Some(error) = decision["pre_action"]["error"].as_str() {
                                ui.label(format!("Pre-action link not verified: {error}"));
                            }
                            if decision["next_appraisal"]["kind"] == "before_action" {
                                ui.strong("Paused before choosing");
                                for (index, option) in decision["next_appraisal"]["alternatives"].as_array().into_iter().flatten().enumerate() {
                                    if let Some(text) = option.as_str() { ui.label(format!("{}. {text}", index + 1)); }
                                }
                                ui.weak("These are the person's alternatives, not actions performed by the pause.");
                            }
                            if let Some(choice) = decision["next_appraisal"]["kind"].as_str() {
                                ui.small(format!("Next appraisal · {}", title(choice)));
                                if let Some(reason) = decision["next_appraisal"]["reason"].as_str() {
                                    ui.label(reason);
                                }
                            }
                            for operation in decision["operations"].as_array().into_iter().flatten() {
                                let label = match s(operation, "tool") {
                                    "persona.initialize" => "Initial parameters",
                                    "persona.evaluate" => "Contextual appraisal",
                                    "persona.paths" => "Recorded numerical paths",
                                    "persona.replay_input" => "Preview of a prior encounter",
                                    "persona.patch" => "Parameter candidate",
                                    "persona.assess" => "Candidate comparison",
                                    other => other,
                                };
                                ui.small(format!("Requested: {label}"));
                            }
                            if decision["next_model"]["kind"] != "preserve" {
                                let reason = s(&decision["next_model"], "reason");
                                if !reason.is_empty() { ui.add(egui::Label::new(reason).wrap()); }
                                if let Some(candidate) = decision["next_model"]["candidate"].as_str() {
                                    if ui.small_button("Inspect chosen candidate →").clicked() { self.open_resource(ui.ctx(), candidate.into()); }
                                }
                            }
                            if self.development_all_work {
                                let work = s(decision, "work");
                                let need = self.records("work").iter().find(|w| w["body"]["id"] == work)
                                    .map(|w| activity_title(&w["body"])).unwrap_or(work);
                                ui.add(egui::Label::new(RichText::new(format!("Task: {}", excerpt(need, 100))).small().weak()).wrap())
                                    .on_hover_text(need);
                            }
                            ui.add_space(8.);
                            ui.horizontal_wrapped(|ui| {
                                if let Some(r)=decision["character_use"].as_str() {
                                    if ui.small_button("Numerical paths…").clicked(){self.character_paths.open(ui.ctx(), &self.root, decision);}
                                    if ui.small_button("Character supplied →").clicked(){self.open_resource(ui.ctx(),r.into());}
                                }
                                if ui.small_button("Inspect decision →").clicked() { self.open_resource(ui.ctx(), s(decision, "response").into()); }
                                if ui.small_button("Supplied context →").clicked() { self.open_resource(ui.ctx(), s(decision, "request").into()); }
                            });
                        });
                    });
                    ui.add_space(9.);
                }
            });
    }
    fn output_bars(&self, ui: &mut Ui, evaluation: &Evaluation, compact: bool) {
        let color = self.color(&self.persona);
        for axis in FeatureContract::standard().outputs {
            let Some(output) = evaluation.outputs.get(&axis.id) else {
                continue;
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(title(&axis.id)).size(if compact { 12. } else { 15. }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{:+.3}", output.value))
                            .monospace()
                            .size(if compact { 11. } else { 13. })
                            .color(color),
                    );
                });
            });
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), if compact { 5. } else { 8. }),
                egui::Sense::hover(),
            );
            let midpoint = rect.center().x;
            let end = midpoint + output.value as f32 * rect.width() / 2.;
            ui.painter().rect_filled(rect, 2, theme::palette(ui).field);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(midpoint.min(end), rect.top()),
                    egui::pos2(midpoint.max(end), rect.bottom()),
                ),
                2,
                if output.value >= 0. {
                    color
                } else {
                    theme::ACCENT
                },
            );
            ui.painter().line_segment(
                [
                    egui::pos2(midpoint, rect.top() - 1.),
                    egui::pos2(midpoint, rect.bottom() + 1.),
                ],
                Stroke::new(1., theme::palette(ui).muted),
            );
            response.on_hover_text(format!("{}\nOutput = tanh(bias + direct + interaction)\nBias {:+.4} · direct {:+.4} · interaction {:+.4}",axis.meaning,output.bias,output.direct,output.interaction));
            ui.add_space(if compact { 4. } else { 10. });
        }
    }
    fn network(&self, ui: &mut Ui, character: &data::Character, evaluation: &Evaluation) {
        let contract = FeatureContract::standard();
        let n = contract
            .inputs
            .len()
            .max(character.model.hidden_units)
            .max(contract.outputs.len());
        let height = (n as f32 * 26. + 100.).max(560.);
        let width = ui.available_width().max(720.);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let palette = theme::palette(ui);
        p.rect_filled(rect, 6, palette.field);
        let left = rect.left() + 175.;
        let mid = rect.center().x;
        let right = rect.right() - 170.;
        let top = rect.top() + 70.;
        let span = height - 110.;
        let input: Vec<_> = (0..contract.inputs.len())
            .map(|i| {
                egui::pos2(
                    left,
                    top + span * i as f32 / (contract.inputs.len() - 1).max(1) as f32,
                )
            })
            .collect();
        let hidden: Vec<_> = (0..character.model.hidden_units)
            .map(|i| {
                egui::pos2(
                    mid,
                    top + span * (i + 1) as f32 / (character.model.hidden_units + 1) as f32,
                )
            })
            .collect();
        let output: Vec<_> = (0..contract.outputs.len())
            .map(|i| {
                egui::pos2(
                    right,
                    top + span * i as f32 / (contract.outputs.len() - 1).max(1) as f32,
                )
            })
            .collect();
        for (x, label) in [
            (left, "APPRAISAL"),
            (mid, "HIDDEN · ReLU"),
            (right, "INCLINATIONS · tanh"),
        ] {
            p.text(
                egui::pos2(x, rect.top() + 22.),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(12.),
                palette.muted,
            );
        }
        p.text(
            egui::pos2(left, rect.top() + 42.),
            egui::Align2::CENTER_CENTER,
            "value / known mask",
            FontId::proportional(10.),
            palette.muted,
        );
        for weight in &character.model.parameters {
            let ends = match weight.tensor {
                Tensor::Hidden => Some((
                    input[weight.column / 2]
                        + egui::vec2(0., if weight.column % 2 == 0 { -3. } else { 3. }),
                    hidden[weight.row],
                )),
                Tensor::Output => Some((hidden[weight.column], output[weight.row])),
                Tensor::Direct => Some((input[weight.column / 2], output[weight.row])),
                _ => None,
            };
            if let Some((a, b)) = ends {
                let color = if weight.value >= 0. {
                    TEAL
                } else {
                    theme::ACCENT
                }
                .gamma_multiply(if weight.tensor == Tensor::Direct {
                    0.15
                } else {
                    0.4
                });
                p.line_segment(
                    [a, b],
                    Stroke::new(0.5 + (weight.value.abs() as f32).min(2.), color),
                );
            }
        }
        for (i, axis) in contract.inputs.iter().enumerate() {
            let pos = input[i];
            let f = evaluation.appraisal.features.get(&axis.id);
            let value = f.and_then(|f| f.value);
            let color = if value.is_some() { TEAL } else { palette.muted };
            p.circle_filled(pos - egui::vec2(0., 3.), 3., color);
            p.circle_stroke(pos + egui::vec2(0., 3.), 3., Stroke::new(1., color));
            p.text(
                pos - egui::vec2(12., 0.),
                egui::Align2::RIGHT_CENTER,
                title(&axis.id),
                FontId::proportional(12.),
                palette.text,
            );
            ui.interact(
                egui::Rect::from_center_size(pos - egui::vec2(70., 0.), egui::vec2(160., 23.)),
                ui.id().with(("input", i)),
                egui::Sense::hover(),
            )
            .on_hover_text(format!(
                "{}\n{}",
                axis.meaning,
                value
                    .map(|v| format!("Recorded value: {v:+.4}"))
                    .unwrap_or_else(|| "Unknown input; value and known mask are both zero.".into())
            ));
        }
        for (i, pos) in hidden.iter().enumerate() {
            let value = evaluation.hidden.get(i).copied().unwrap_or(0.);
            p.circle_filled(*pos, 18., palette.panel);
            p.circle_stroke(*pos, 18., Stroke::new(1.5, PURPLE));
            p.text(
                *pos,
                egui::Align2::CENTER_CENTER,
                format!("h{}", i + 1),
                FontId::monospace(12.),
                palette.text,
            );
            p.text(
                *pos + egui::vec2(0., 27.),
                egui::Align2::CENTER_CENTER,
                format!("{value:.3}"),
                FontId::monospace(10.),
                PURPLE,
            );
        }
        for (i, axis) in contract.outputs.iter().enumerate() {
            let pos = output[i];
            let value = evaluation
                .outputs
                .get(&axis.id)
                .map(|o| o.value)
                .unwrap_or(0.);
            p.circle_filled(pos, 5., self.color(&self.persona));
            p.text(
                pos + egui::vec2(12., -5.),
                egui::Align2::LEFT_CENTER,
                title(&axis.id),
                FontId::proportional(12.),
                palette.text,
            );
            p.text(
                pos + egui::vec2(12., 9.),
                egui::Align2::LEFT_CENTER,
                format!("{value:+.3}"),
                FontId::monospace(10.),
                self.color(&self.persona),
            );
            ui.interact(
                egui::Rect::from_center_size(pos + egui::vec2(70., 0.), egui::vec2(150., 24.)),
                ui.id().with(("output", i)),
                egui::Sense::hover(),
            )
            .on_hover_text(&axis.meaning);
        }
    }
    fn parameter_table(&mut self, ui: &mut Ui, character: &data::Character) {
        ui.label("Literal numerical coefficients authored by the persona. Unspecified coordinates are exactly zero.");
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.parameter_filter)
                    .hint_text("Filter axis or coordinate…")
                    .desired_width(250.),
            );
            egui::ComboBox::from_id_salt("tensor-filter")
                .selected_text(if self.tensor_filter.is_empty() {
                    "All tensors"
                } else {
                    &self.tensor_filter
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.tensor_filter, String::new(), "All tensors");
                    for name in ["bias", "direct", "hidden", "hidden_bias", "output"] {
                        ui.selectable_value(&mut self.tensor_filter, name.into(), name);
                    }
                });
        });
        let contract = FeatureContract::standard();
        let query = self.parameter_filter.to_lowercase();
        let rows: Vec<(String, String, String, f64)> = character
            .model
            .parameters
            .iter()
            .filter_map(|p| {
                let tensor = match p.tensor {
                    Tensor::Bias => "bias",
                    Tensor::Direct => "direct",
                    Tensor::Hidden => "hidden",
                    Tensor::HiddenBias => "hidden_bias",
                    Tensor::Output => "output",
                };
                let target = if matches!(p.tensor, Tensor::Hidden | Tensor::HiddenBias) {
                    format!("h{}", p.row + 1)
                } else {
                    contract.outputs[p.row].id.clone()
                };
                let from = match p.tensor {
                    Tensor::Bias | Tensor::HiddenBias => "constant".into(),
                    Tensor::Output => format!("h{}", p.column + 1),
                    _ => format!(
                        "{} / {}",
                        contract.inputs[p.column / 2].id,
                        if p.column % 2 == 0 { "value" } else { "known" }
                    ),
                };
                let coordinate = format!("{tensor}[{},{}]", p.row, p.column);
                if (!self.tensor_filter.is_empty() && self.tensor_filter != tensor)
                    || (!query.is_empty()
                        && !format!("{coordinate} {target} {from}")
                            .to_lowercase()
                            .contains(&query))
                {
                    None
                } else {
                    Some((coordinate, from, target, p.value))
                }
            })
            .collect();
        ui.small(format!(
            "{} of {}",
            rows.len(),
            counted(character.model.parameters.len() as u64, "stored coefficient", "stored coefficients")
        ));
        ui.add_space(8.);
        let available = ui.available_width();
        let coordinate_width = (available * 0.24).max(130.);
        let input_width = (available * 0.34).max(160.);
        let target_width = (available * 0.24).max(130.);
        ui.horizontal(|ui| {
            for (label, width) in [
                ("Coordinate", coordinate_width),
                ("Source", input_width),
                ("Target", target_width),
                ("Value", 75.),
            ] {
                ui.add_sized(
                    [width, 24.],
                    egui::Label::new(RichText::new(label).strong()),
                );
            }
        });
        ui.separator();
        egui::ScrollArea::both()
            .id_salt("parameter-rows")
            .show_rows(ui, 28., rows.len(), |ui, range| {
                for i in range {
                    let (a, b, c, value) = &rows[i];
                    let background = if i % 2 == 0 {
                        theme::palette(ui).field
                    } else {
                        Color32::TRANSPARENT
                    };
                    egui::Frame::new().fill(background).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (text, width) in
                                [(a, coordinate_width), (b, input_width), (c, target_width)]
                            {
                                ui.add_sized(
                                    [width, 23.],
                                    egui::Label::new(RichText::new(text).monospace().size(12.))
                                        .truncate(),
                                )
                                .on_hover_text(text);
                            }
                            ui.label(
                                RichText::new(format!("{value:+.5}"))
                                    .monospace()
                                    .size(12.)
                                    .color(if *value >= 0. { TEAL } else { theme::ACCENT }),
                            );
                        });
                    });
                }
            });
    }
}
