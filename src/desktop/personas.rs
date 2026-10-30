//! Continuing identities have a workspace independent of the selected task.
use super::*;

impl App {
    pub(super) fn persona_roster(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("persona-roster")
            .default_width(230.0)
            .width_range(180.0..=350.0)
            .frame(theme::panel(ctx))
            .show(ctx, |ui| {
                let people = self.persona_catalog();
                theme::header(ui, Icon::People, "Personas", &people.len().to_string());
                ui.add(
                    egui::TextEdit::singleline(&mut self.persona_filter)
                        .hint_text("Find a persona…")
                        .desired_width(f32::INFINITY),
                );
                let query = self.persona_filter.trim().to_lowercase();
                let mut shown = 0;
                egui::ScrollArea::vertical()
                    .id_salt("persona-roster-list")
                    .show(ui, |ui| {
                        for (id, label) in &people {
                            if !query.is_empty()
                                && !label.to_lowercase().contains(&query)
                                && !id.to_lowercase().contains(&query)
                            {
                                continue;
                            }
                            shown += 1;
                            let tasks = models_at(&self.view.inspection, "works")
                                .iter()
                                .filter(|work| {
                                    models_at(work, "participants")
                                        .iter()
                                        .any(|p| s(p, "persona") == id)
                                })
                                .count();
                            let scope = if tasks == 0 {
                                "No listed task participation".to_owned()
                            } else {
                                format!(
                                    "Participates in {tasks} task{}",
                                    if tasks == 1 { "" } else { "s" }
                                )
                            };
                            if ui
                                .add_enabled(
                                    !self.busy,
                                    egui::Button::new(format!("{label}\n{scope}"))
                                        .selected(self.profile_persona.as_ref() == Some(id))
                                        .wrap()
                                        .min_size(egui::vec2(ui.available_width(), 48.0)),
                                )
                                .on_hover_text(id)
                                .clicked()
                            {
                                self.open_profile(id.clone());
                            }
                        }
                        if shown == 0 {
                            ui.weak(if people.is_empty() {
                                "No persona identities are available in this inspection."
                            } else {
                                "No personas match this search."
                            });
                        }
                    });
            });
    }

    pub(super) fn personas_workspace(&mut self, ui: &mut Ui) {
        if self.profile_persona.is_none() && !self.busy {
            let people = self.persona_catalog();
            let first = self
                .inspected_persona
                .as_ref()
                .filter(|id| people.contains_key(*id))
                .cloned()
                .or_else(|| people.keys().next().cloned());
            if let Some(persona) = first {
                self.open_profile(persona);
            }
        }
        egui::Frame::new().inner_margin(12).show(ui, |ui| {
            self.banner(ui);
            if self.profile_persona.is_none() {
                theme::header(ui, Icon::People, "Personas", "CONTINUING IDENTITIES");
                ui.label("Profiles become available when this workspace has persona identities.");
                return;
            }
            let v = self.view.development.clone();
            if v.is_null() || v["persona"].as_str() != self.profile_persona.as_deref() {
                if let Some(persona) = self.profile_persona.clone() {
                    ui.heading(self.persona_label(&persona));
                    if self.busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Loading retained profile…");
                        });
                    } else {
                        ui.label("This profile is unavailable in the current inspection.");
                        if ui.button("Retry profile").clicked() {
                            self.send(Action::Development(persona));
                        }
                    }
                }
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.heading(s(&v, "label"));
                theme::badge(ui, "AI PERSONA", theme::BLUE);
                if !s(&v["lifecycle"], "status").is_empty() {
                    theme::badge(ui, s(&v["lifecycle"], "status"), theme::palette(ui).muted);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Refresh profile"))
                    .clicked()
                {
                    self.send(Action::Development(s(&v, "persona").to_owned()));
                }
            });
            ui.label(RichText::new(s(&v, "persona")).monospace().small().weak());
            ui.horizontal_wrapped(|ui| {
                for (tab, label) in [
                    (0, "Character"),
                    (1, "Characteristic fragments"),
                    (3, "Fragment relationships"),
                    (2, "Work & supporting records"),
                ] {
                    ui.selectable_value(&mut self.profile_tab, tab, label);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(("persona-workspace", s(&v, "persona"), self.profile_tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match self.profile_tab {
                        0 => self.persona_character(ui, &v),
                        1 => self.persona_fragments(ui, &v),
                        3 => self.persona_relationships(ui, &v),
                        _ => self.persona_work(ui, &v),
                    }
                    if v["truncated"] == true {
                        ui.colored_label(
                            theme::ACCENT,
                            "This inspection contains a subset of retained revisions.",
                        );
                    }
                    ui.add_space(10.0);
                    ui.collapsing("Exact development record", |ui| json_view(ui, &v));
                });
        });
    }

    fn persona_character(&mut self, ui: &mut Ui, v: &Value) {
        if v["lifecycle"]["quarantined"] == true {
            ui.colored_label(theme::ACCENT, "This identity is quarantined.");
        }
        if v["character"].is_null() {
            ui.label("No accepted character account is available in this inspection.");
            ui.weak("This does not establish whether a profile has never been authored or is unavailable here.");
        } else {
            theme::header(
                ui,
                Icon::People,
                "Accepted character",
                "AUTHORED DESCRIPTIONS",
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Character revision {}", v["character"]["revision"]));
                if ui.button("Read characteristic fragments").clicked() {
                    self.profile_tab = 1;
                }
            });
            ui.weak("These descriptions express character and modeled state. They are not measured personality scores or evidence of capability.");
            for (key, title, vocabulary) in [
                ("ocean", "Stable tendencies", "OCEAN"),
                ("vad", "Baseline affect", "MODELED VAD"),
            ] {
                ui.add_space(10.0);
                theme::header(ui, Icon::People, title, vocabulary);
                if let Some(traits) = v["character"][key].as_object() {
                    for (name, value) in traits {
                        egui::Frame::new()
                            .fill(theme::palette(ui).field)
                            .inner_margin(9)
                            .corner_radius(3)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.strong(name.replace('_', " "));
                                if let Some(text) = value.as_str() {
                                    theme::message_body(ui, text);
                                } else {
                                    super::super::data_view::parameters(ui, value);
                                }
                            });
                    }
                }
            }
            ui.collapsing("Current character references & attribution", |ui| {
                json_view(ui, &v["character"]);
            });
        }
        if !v["transient_affect"]["context"].is_null()
            && v["transient_affect"]["context"]["status"] != "none_retained"
        {
            ui.add_space(10.0);
            ui.collapsing(
                "Transient modeled affect · separate from baseline character",
                |ui| {
                    super::super::data_view::parameters(ui, &v["transient_affect"]);
                },
            );
        }
        let selection = &self.view.inspection["settings"]["persona_models"][s(v, "persona")];
        ui.add_space(10.0);
        theme::header(
            ui,
            Icon::Settings,
            "Inference configuration",
            "SAVED SETTINGS",
        );
        theme::property(ui, "Model", s(selection, "model"));
        theme::property(ui, "Reasoning", s(selection, "effort"));
        if !v["creation_origin"].is_null() {
            ui.collapsing("Creation origin", |ui| json_view(ui, &v["creation_origin"]));
        }
    }

    fn persona_fragments(&mut self, ui: &mut Ui, v: &Value) {
        ui.weak("Characteristic fragments express voice, behavior, dispositions, and standards. Knowledge, tools, skills, data, and factual history are supporting assets.");
        if ui
            .button("Explore recorded paths & relationships")
            .clicked()
        {
            self.profile_tab = 3;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.profile_filter)
                .hint_text("Find a fragment, author, or phrase…")
                .desired_width(f32::INFINITY),
        );
        ui.checkbox(&mut self.profile_history, "Include historical revisions");
        let query = self.profile_filter.trim().to_lowercase();
        let revisions = models_at(v, "characteristic_revisions");
        let mut shown = 0;
        for revision in revisions.iter().filter(|r| {
            (self.profile_history || r["current"] == true)
                && (query.is_empty()
                    || s(r, "source").to_lowercase().contains(&query)
                    || s(r, "recorded_author").to_lowercase().contains(&query)
                    || models_at(&r["context"], "entries").iter().any(|entry| {
                        s(entry, "text").to_lowercase().contains(&query)
                            || s(entry, "path").to_lowercase().contains(&query)
                    }))
        }) {
            shown += 1;
            let path = models_at(&revision["context"], "entries")
                .first()
                .map(|entry| s(entry, "path"))
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| s(revision, "source"));
            egui::CollapsingHeader::new(format!(
                "{} · revision {} · {}",
                path,
                revision["revision"],
                if revision["current"] == true {
                    "file head"
                } else {
                    "historical"
                },
            ))
            .id_salt((s(revision, "source"), revision["revision"].to_string()))
            .default_open(revision["current"] == true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "Recorded author: {}",
                        self.persona_label(s(revision, "recorded_author"))
                    ));
                    if revision["source_retired"] == true {
                        theme::badge(ui, "RETIRED SOURCE", theme::ACCENT);
                    }
                });
                // A source head and a current-character designation are distinct.
                // Show the exact references, including any range, without claiming
                // that the entire revision is designated or was supplied later.
                let references: Vec<_> = models_at(&v["character"], "own_voice")
                    .iter()
                    .filter(|reference| {
                        reference["source"] == revision["source"]
                            && reference["revision"] == revision["revision"]
                    })
                    .cloned()
                    .collect();
                if !references.is_empty() {
                    ui.collapsing("Current character references into this revision", |ui| {
                        json_view(ui, &json!(references));
                    });
                }
                for entry in models_at(&revision["context"], "entries") {
                    match fragment_map::entry_scope(entry, revision) {
                        "fragments" => {
                            ui.strong(s(entry, "path"));
                            theme::message_body(ui, s(entry, "text"));
                        }
                        scope => {
                            let label = if scope == "assets" {
                                "Supporting asset qualification"
                            } else {
                                "Qualification · scope unavailable"
                            };
                            ui.collapsing(format!("{label}: {}", s(entry, "path")), |ui| {
                                theme::message_body(ui, s(entry, "text"));
                                json_view(ui, &entry["reference"]);
                            });
                        }
                    }
                }
                ui.collapsing("Authorship and full qualifications", |ui| {
                    json_view(ui, revision)
                });
            });
        }
        if shown == 0 {
            ui.label(if revisions.is_empty() {
                "No characteristic revisions are available in this inspection."
            } else {
                "No fragments match this search and revision filter."
            });
        }
    }

    fn persona_work(&mut self, ui: &mut Ui, v: &Value) {
        let persona = s(v, "persona");
        let tasks: Vec<_> = models_at(&self.view.inspection, "works")
            .iter()
            .filter(|work| {
                models_at(work, "participants")
                    .iter()
                    .any(|p| p["persona"] == persona)
            })
            .cloned()
            .collect();
        theme::header(
            ui,
            Icon::Task,
            "Participation across tasks",
            &tasks.len().to_string(),
        );
        ui.weak("Follow task conversations, recorded files, and activity as supporting records. Participation alone does not establish an accepted responsibility or a successful outcome.");
        if tasks.is_empty() {
            ui.label("This persona has no participation in the listed tasks. Their identity and available character remain inspectable.");
        }
        for task in tasks {
            let id = s(&task, "id");
            ui.push_id(id, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(s(&task, "need")).strong()).wrap());
                    ui.label(
                        RichText::new(format!("{id} · {}", s(&task, "phase")))
                            .small()
                            .weak(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for (tab, label) in [
                            (0, "Conversation"),
                            (1, "Files & citations"),
                            (3, "Activity"),
                        ] {
                            if ui
                                .add_enabled(!self.busy, egui::Button::new(label))
                                .clicked()
                            {
                                self.open_task(id.to_owned(), tab, Some(persona.to_owned()));
                            }
                        }
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Responsibilities"))
                            .clicked()
                        {
                            self.open_task(id.to_owned(), 0, Some(persona.to_owned()));
                            self.conversation_mode = 1;
                        }
                    });
                });
            });
        }
    }
}
