use super::*;
use ai_personas::persona_model::Evaluation;
impl App {
    fn evaluation(&self) -> Option<(Evaluation, bool)> {
        let character = self.snapshot.characters.get(&self.persona)?;
        if self.contextual {
            let p = self.participation(&self.persona);
            if let Some(evaluation) = p["last_evaluation"]
                .as_str()
                .and_then(|r| self.snapshot.evaluations.get(r))
                .filter(|e| e.model == character.baseline.model)
            {
                return Some((evaluation.clone(), true));
            }
        }
        Some((character.baseline.clone(), false))
    }
    fn appraisal_origin(&mut self, ui: &mut Ui, inspect: bool) {
        let key = (self.work.clone(), self.persona.clone());
        let Some(provenance) = self.snapshot.appraisal_provenance.get(&key).cloned() else {
            ui.weak("Appraisal origin unavailable; current applicability is unknown.");
            return;
        };
        if let Some(recorded) = provenance["recorded_at_utc"].as_str() {
            ui.small(format!("Appraised {recorded}"));
        }
        if let Some(subsequent) = provenance["accepted_decisions_since"].as_u64() {
            ui.small(format!("{subsequent} accepted decisions since"));
        }
        ui.weak("Recorded observations; subsequent work does not refresh them.");
        if inspect {
            if let Some(reference) = provenance["response"].as_str() {
                if ui.small_button("Inspect authored appraisal →").clicked() {
                    self.open_resource(ui.ctx(), reference.into());
                }
            }
        }
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
        ui.add_space(14.);
        let part = self.participation(&self.persona);
        ui.horizontal(|ui| {
            theme::badge(
                ui,
                &format!("Epoch {}", person["epoch"].as_u64().unwrap_or(0)),
                color,
            );
            ui.small(format!("{} decisions", part["turn"].as_u64().unwrap_or(0)));
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
        if let Some((evaluation, contextual)) = self.evaluation() {
            ui.label(
                RichText::new(if contextual {
                    "Latest contextual evaluation"
                } else {
                    "Baseline · all inputs unknown"
                })
                .small()
                .color(color),
            );
            if contextual {
                self.appraisal_origin(ui, false);
            }
            ui.add_space(8.);
            self.output_bars(ui, &evaluation, true);
            ui.add_space(10.);
            ui.label(
                RichText::new(
                    "Calculated inclinations, not measures of ability or design quality.",
                )
                .size(11.)
                .weak(),
            );
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
            match metadata["kind"].as_str() { Some("job") => "Background command", Some("control") => "Execution control", _ => "Exact resource" }
        } else {
            path.rsplit('/').next().unwrap_or(path)
        }).size(18.).strong()).wrap());
        if !path.is_empty() {
            ui.add(egui::Label::new(RichText::new(path).small().weak()).wrap());
        }
        ui.add_space(12.);
        if !s(&metadata, "owner").is_empty() {
            theme::property(ui, "Owner", self.label(s(&metadata, "owner")));
        }
        if let Some(asset) = &asset {
            theme::property(ui, "Version", &asset["revision"].to_string());
            theme::property(ui, "Size", &size(asset["size"].as_u64().unwrap_or(0)));
            theme::property(
                ui,
                "Retained",
                &time_label(asset["created"].as_i64().unwrap_or(0)),
            );
        }
        ui.add_space(12.);
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
            ui.add_space(15.);
            section(ui, "Retained record", "");
            ui.weak("The preview shows this exact stored record. Later observations remain separate.");
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
            ui.small(if b["user_accepted"] == true {
                "Accepted by you"
            } else {
                "Not accepted by you"
            });
            ui.label(s(b, "supported_use"));
            ui.add_space(8.);
        }
    }
    pub(super) fn character(&mut self, ui: &mut Ui) {
        eyebrow(ui, "Character Lab");
        ui.horizontal_wrapped(|ui| {
            ui.heading("A model behind each persona");
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
                "{} inputs · {} hidden units · {} outputs · {} stored coefficients",
                FeatureContract::standard().inputs.len(),
                character.model.hidden_units,
                FeatureContract::standard().outputs.len(),
                character.model.parameters.len()
            ));
        });
        ui.add_space(12.);
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["Characteristics", "Network", "Parameters", "Revisions", "Development"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.character_tab, i, *label);
            }
        });
        ui.separator();
        match self.character_tab {
            0 => {
                egui::ScrollArea::vertical().id_salt("character-profile").show(ui,|ui|{
                ui.horizontal_wrapped(|ui|{ui.selectable_value(&mut self.contextual,true,"Latest context");ui.selectable_value(&mut self.contextual,false,"Baseline");});
                if let Some((evaluation,contextual))=self.evaluation(){
                    ui.small(if contextual{"Recorded contextual evaluation for this task, bound to the active model."}else{"Forward evaluation with all appraisal inputs unknown. This is not a current emotional or task assessment."});
                    if contextual{self.appraisal_origin(ui,true);}ui.add_space(14.);
                    let column_gap=ui.spacing().item_spacing.x;ui.spacing_mut().item_spacing.x=18.;
            ui.columns(2,|cols|{
                        section(&mut cols[0],"Forward inclinations", "−1 … +1");self.output_bars(&mut cols[0],&evaluation,false);
                        section(&mut cols[1],"Authored intent", "Active model");cols[1].add(egui::Label::new(&character.model.intent).wrap().halign(egui::Align::Min));cols[1].add_space(16.);
                        cols[1].collapsing("Starting profile & attribution",|ui|{ui.small(format!("From {}",s(&person,"seed_author")));ui.label(s(&person,"seed"));});
                        cols[1].add_space(12.);cols[1].small("The persona directly authors numerical edits. Activation and rollback are recorded separately; forward evaluation does not change weights.");
                    });ui.spacing_mut().item_spacing.x=column_gap;
                    ui.add_space(20.);section(ui,"Appraisal inputs",if contextual{"Recorded values & evidence"}else{"Unknown at baseline"});
                    for axis in FeatureContract::standard().inputs{
                        let feature=evaluation.appraisal.features.get(&axis.id);
                        ui.collapsing(format!("{}   {}",title(&axis.id),feature.and_then(|f|f.value).map(|v|format!("{v:+.3}")).unwrap_or_else(||"unknown".into())),|ui|{ui.weak(&axis.meaning);if let Some(feature)=feature{ui.label(&feature.interpretation);for reference in &feature.evidence{if ui.small_button(excerpt(reference,55)).on_hover_text(reference).clicked(){self.open_resource(ui.ctx(),reference.clone());}}}});
                    }
                }
            });
            }
            1 => {
                ui.small("Connections show stored weights; biases are listed in Parameters. Teal is positive, orange negative.");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.contextual, true, "Latest context");
                    ui.selectable_value(&mut self.contextual, false, "Baseline");
                });
                if self.evaluation().is_some_and(|(_, contextual)| contextual) {
                    self.appraisal_origin(ui, true);
                }
                ui.add_space(8.);
                egui::ScrollArea::both()
                    .id_salt("character-network")
                    .show(ui, |ui| self.network(ui, &character));
            }
            2 => self.parameter_table(ui, &character),
            4 => self.character_development(ui),
            _ => {
                egui::ScrollArea::vertical().id_salt("character-revisions").show(ui,|ui|{
                section(ui,"Active model", "Exact immutable version");ui.add(egui::Label::new(RichText::new(&character.baseline.model).monospace().size(12.)).wrap());
                if ui.button("Inspect model JSON").clicked(){self.open_resource(ui.ctx(),format!("object:{}",character.baseline.model));}
                ui.add_space(16.);section(ui,"Authored candidates", "Candidate ≠ activation");
                let candidates:Vec<Value>=self.snapshot.candidates.iter().filter(|c|c["body"]["owner"]==self.persona).cloned().collect();
                for candidate in candidates{let b=&candidate["body"];card(ui).show(ui,|ui|{let active=b["model"]==character.baseline.model;theme::badge(ui,if active{"Active"}else{"Retained candidate"},if active{TEAL}else{theme::ACCENT});ui.label(s(b,"intent"));ui.add_space(7.);if ui.small_button("Open candidate record").clicked(){self.open_resource(ui.ctx(),s(&candidate,"reference").into());}if let Some(parent)=b["predecessor"].as_str(){if ui.small_button("Open predecessor").clicked(){self.open_resource(ui.ctx(),format!("object:{parent}"));}}});ui.add_space(8.);}
                ui.add_space(16.);section(ui,"Activation history", "Saved events");
                let events:Vec<Value>=self.events.iter().rev().filter(|e|e["kind"]=="model_changed"&&e["actor"]==self.persona).cloned().collect();
                for event in events{ui.collapsing(format!("{} · model changed",time_label(event["created"].as_i64().unwrap_or(0))),|ui|{super::super::data_view::parameters(ui,&event["body"]);});}
                ui.add_space(12.);ui.weak("A revision records a change. Useful learning requires evidence from subsequent behavior and outcomes.");
            });
            }
        }
    }
    fn character_development(&mut self, ui: &mut Ui) {
        let scope = (self.persona.clone(), self.work.clone());
        if self.development_scope != scope {
            self.development_scope = scope;
            self.development_anchor.clear();
        }
        egui::ScrollArea::vertical()
            .id_salt(("character-development", &self.persona, &self.work, &self.development_anchor,
                self.development_filter, self.development_all_work))
            .show(ui, |ui| self.character_development_contents(ui));
    }
    fn character_development_contents(&mut self, ui: &mut Ui) {
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
        let edits = decisions.iter().flat_map(|d| d["operations"].as_array().into_iter().flatten())
            .filter(|op| op["tool"] == "persona.patch").count();
        let bindings = decisions.iter().filter(|d| matches!(d["next_model"]["kind"].as_str(), Some("activate" | "rollback"))).count();
        ui.add_space(12.);
        card(ui).show(ui, |ui| {
            let labels = [format!("{} accepted decisions", decisions.len()), format!("{no_change} no-change choices"),
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
            for (index, label) in ["All choices", "No change", "Declared change", "Deferred"].iter().enumerate() {
                if ui.selectable_value(&mut self.development_filter, index, *label).changed() {
                    self.development_anchor.clear();
                }
            }
        });
        let visible: Vec<&Value> = decisions.into_iter().filter(|d| match self.development_filter {
            1 => d["maintenance"]["kind"] == "no_change",
            2 => d["maintenance"]["kind"] == "change",
            3 => d["maintenance"]["kind"] == "defer",
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
                            for operation in decision["operations"].as_array().into_iter().flatten() {
                                let label = match s(operation, "tool") {
                                    "persona.initialize" => "Initial parameters",
                                    "persona.evaluate" => "Contextual appraisal",
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
                                    .map(|w| s(&w["body"], "original_need")).unwrap_or(work);
                                ui.add(egui::Label::new(RichText::new(format!("Task: {}", excerpt(need, 100))).small().weak()).wrap())
                                    .on_hover_text(need);
                            }
                            ui.add_space(8.);
                            ui.horizontal_wrapped(|ui| {
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
    fn network(&self, ui: &mut Ui, character: &data::Character) {
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
        let evaluation = self
            .evaluation()
            .map(|e| e.0)
            .unwrap_or_else(|| character.baseline.clone());
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
            "{} of {} stored coefficients",
            rows.len(),
            character.model.parameters.len()
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
