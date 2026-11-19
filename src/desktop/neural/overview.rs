use super::*;

pub(super) fn repeated_file_names(files: &[Value]) -> BTreeSet<&str> {
    let mut seen = BTreeMap::new();
    let mut repeated = BTreeSet::new();
    for file in files {
        let (Some(name), Some(reference)) = (file["name"].as_str(), file["reference"].as_str()) else {
            continue;
        };
        if seen.insert(name, reference).is_some_and(|prior| prior != reference) {
            repeated.insert(name);
        }
    }
    repeated
}

pub(super) fn source_file_link(ui: &mut Ui, file: &Value, source: &str, repeated: &BTreeSet<&str>) -> Option<(String, String)> {
    let name = file["name"].as_str()?;
    let reference = file["reference"].as_str()?;
    ui.horizontal_wrapped(|ui| {
        theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
        let link = ui.link(name).on_hover_text(format!("{reference}\n{source}"));
        if let Some(bytes) = file["bytes"].as_u64() {
            ui.small(size(bytes)).on_hover_text(format!("{bytes} bytes"));
        }
        if repeated.contains(name) {
            if let Some(digest) = reference.strip_prefix("object:").and_then(|s| s.get(..10)) {
                ui.weak(digest).on_hover_text(reference);
            }
        }
        link.clicked().then(|| (reference.to_owned(), name.to_owned()))
    }).inner
}

pub(super) fn source_file_links(ui: &mut Ui, files: &Value, source: &str) -> Option<(String, String)> {
    let files = files.as_array()?;
    let repeated = repeated_file_names(files);
    let mut selected = None;
    for (index, file) in files.iter().enumerate() {
        ui.push_id((source, index), |ui| {
            if let Some(file) = source_file_link(ui, file, source, &repeated) {
                selected = Some(file);
            }
        });
    }
    selected
}

impl App {
    pub(super) fn source_file_links(&mut self, ui: &mut Ui, files: &Value, source: &str) {
        if let Some((reference, name)) = source_file_links(ui, files, source) {
            self.open_named_resource(ui.ctx(), reference, Some((source.to_owned(), name)));
        }
    }

    pub(super) fn activity_overview(&mut self, ui: &mut Ui) {
        if self.work.is_empty() {
            ui.heading("Your workspace");
            ui.label("Start an activity with a need and choose the first persona to contact.");
            if ui.add(theme::primary("Create an activity…")).clicked() {
                self.dialog = Some(Dialog::activity(false));
            }
            return;
        }
        let p = theme::palette(ui);
        let brief = self.task_title().to_owned();
        let record = self
            .records("work")
            .iter()
            .find(|r| r["body"]["id"] == self.work)
            .cloned()
            .unwrap_or_default();
        let active = self
            .snapshot
            .active
            .iter()
            .filter(|c| c["work"] == self.work && self.runner_observation.has_worker(&self.work,s(c,"actor")))
            .count();
        let ended = record["body"]["phase"] == "dissolved";
        let focus_files = self.live_board.take_files_focus();
        let original_files = record["body"]["materials"].as_array().is_some_and(|files| !files.is_empty());
        let hero = egui::Frame::new()
            .fill(p.header)
            .stroke(Stroke::new(1., p.border))
            .corner_radius(9)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    theme::glyph(ui, Icon::Task, TEAL);
                    eyebrow(
                        ui,
                        if record["body"]["kind"] == "social" {
                            "Social activity"
                        } else {
                            "Current activity"
                        },
                    );
                    if ended {
                        theme::badge(ui, "Ended", p.muted);
                    } else if active > 0 {
                        theme::badge(ui, &format!("{active} working now"), TEAL);
                    }
                });
                ui.add_space(6.);
                ui.label(RichText::new(excerpt(&brief, 210)).size(24.).strong());
                if brief.chars().count() > 210 {
                    ui.collapsing("Read full brief", |ui| {
                        ui.label(&brief);
                    });
                }
                if original_files {
                    ui.add_space(8.);
                    if focus_files {
                        ui.scroll_to_cursor(Some(egui::Align::Min));
                    }
                    ui.strong("Files in the original brief");
                    let source = format!("work:{}@1", s(&record["reference"], "id"));
                    self.source_file_links(ui, &record["body"]["materials"], &source);
                }
                ui.add_space(9.);
                ui.horizontal_wrapped(|ui| {
                    if self.running || matches!(self.runner_observation,ai_personas::neural::live::RunnerObservation::Observed { .. }) {
                        ui.label(self.runner_status_text());
                        self.finish_run_button(ui);
                    } else if ui.add_enabled(!ended && self.can_start_run(),theme::primary("Run activity"))
                        .on_hover_text("Run this activity and its requests. Keep listening for the events each persona chose while Studio is open.").clicked(){
                        self.follow_activity(ui.ctx());
                    }
                    if ui.button("Activity thread").clicked() {
                        self.requested_activities.open(
                            ui.ctx(),
                            &self.root,
                            &self.work,
                            self.cursor,
                        );
                    }
                    if ui.button("Responsibilities").clicked() {
                        self.layout.editor = Editor::Responsibilities;
                    }
                });
            });
        let rect = hero.response.rect.shrink(1.);
        ui.painter().line_segment(
            [
                rect.left_top() + egui::vec2(1., 9.),
                rect.left_bottom() + egui::vec2(1., -9.),
            ],
            Stroke::new(3., TEAL),
        );
        ui.add_space(14.);
        self.brief_amendments(ui, &record, focus_files, original_files);
        let requests: Vec<_> = self
            .records("human_requests")
            .iter()
            .filter(|r| {
                r["body"]["work"] == self.work
                    && matches!(s(&r["body"], "state"), "open" | "in_progress")
            })
            .cloned()
            .collect();
        if !requests.is_empty() {
            card(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                section(ui, "Needs you", &format!("{} active", requests.len()));
                for r in requests {
                    let b = &r["body"];
                    ui.horizontal_wrapped(|ui| {
                        theme::glyph(ui, Icon::Questions, theme::ACCENT);
                        ui.strong(self.label(s(b, "requester")));
                        if ui.link(s(b, "title")).clicked() {
                            self.show_human_request(s(&r["reference"], "id"));
                        }
                        if let Some(who) = b["claimed_by"].as_str() {
                            ui.weak(format!("Taken by {who}"));
                        }
                    });
                }
            });
            ui.add_space(14.);
        }
        let members: Vec<_> = self
            .records("participation")
            .iter()
            .filter(|r| r["body"]["work"] == self.work)
            .map(|r| r["body"].clone())
            .collect();
        section(ui, "People on this activity", &members.len().to_string());
        if members.is_empty() {
            ui.weak("No persona has been invited to this activity.");
        }
        let columns = if ui.available_width() >= 845. {
            3
        } else if ui.available_width() >= 540. {
            2
        } else {
            1
        };
        let columns = columns.min(members.len().max(1));
        for row in members.chunks(columns) {
            ui.columns(columns, |cols| {
                for (col, member) in cols.iter_mut().zip(row) {
                    let person = s(member, "persona");
                    let name = self.label(person).to_owned();
                    let color = self.color(person);
                    let reason = member["continuation"]["purpose"]
                        .as_str()
                        .or(member["continuation"]["reason"].as_str())
                        .unwrap_or("");
                    card(col).show(col, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            if avatar(ui, &name, color, 32.).clicked() {
                                self.show_person(person);
                            }
                            ui.vertical(|ui| {
                                if ui.link(RichText::new(&name).strong().size(17.)).clicked() {
                                    self.show_person(person);
                                }
                                ui.small(self.persona_state(person));
                            });
                        });
                        if !reason.is_empty() {
                            ui.add_space(5.);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.label(excerpt(reason, if columns == 1 { 260 } else { 130 }))
                                    .on_hover_text(reason)
                            });
                        }
                    });
                }
            });
            ui.add_space(6.);
        }
        ui.add_space(10.);
        let deliveries = self.delivered_packages(|body| body["work"] == self.work);
        if self.live_board.take_delivery_focus() {
            ui.scroll_to_cursor(Some(egui::Align::Min));
        }
        section(
            ui,
            "Delivered work",
            &format!(
                "{} {}",
                deliveries.len(),
                if deliveries.len() == 1 {
                    "package"
                } else {
                    "packages"
                }
            ),
        );
        if deliveries.is_empty() {
            ui.weak("No package has been delivered in this activity yet.");
        }
        for d in deliveries.iter().take(3) {
            self.delivery_card(ui, d);
            ui.add_space(6.);
        }
        if deliveries.len() > 3 {
            ui.collapsing(
                format!("{} earlier deliveries", deliveries.len() - 3),
                |ui| {
                    for d in &deliveries[3..] {
                        self.delivery_card(ui, d);
                    }
                },
            );
        }
    }

    fn brief_amendments(&mut self, ui: &mut Ui, record: &Value, focus_files: bool, original_files: bool) {
        let Some(amendments) = record["body"]["amendments"].as_array().filter(|rows| !rows.is_empty()) else {
            return;
        };
        let mut selected = None;
        egui::CollapsingHeader::new(format!("Brief amendments · {}", amendments.len()))
            .id_salt(("brief-amendments", &self.work))
            .default_open(true)
            .open(focus_files.then_some(true))
            .show(ui, |ui| {
                let events: Vec<_> = self.events.iter()
                    .filter(|event| event["work"] == self.work && event["kind"] == "requirements_amended")
                    .collect();
                let current = inbox::handle(record);
                let mut focus_amendment = focus_files && !original_files;
                for (index, amendment) in amendments.iter().enumerate().rev() {
                    if index + 1 < amendments.len() {
                        ui.separator();
                    }
                    if focus_amendment && amendment["attachments"].as_array().is_some_and(|files| !files.is_empty()) {
                        ui.scroll_to_cursor(Some(egui::Align::Min));
                        focus_amendment = false;
                    }
                    let event = events.get(index).copied().filter(|event|
                        event["actor"] == amendment["author"] && event["body"]["text"] == amendment["text"]
                            && event["body"]["attachments"] == amendment["attachments"]
                    );
                    let revision = event.and_then(|e| e["body"]["reference"].as_str());
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(s(amendment, "author"));
                        ui.small("Operator");
                        if let Some(when) = event.and_then(|e| e["created_utc"].as_str()) {
                            ui.small(when);
                        }
                        if let Some(reference) = revision {
                            if let Some((_, number)) = reference.rsplit_once('@') {
                                if ui.link(format!("Brief revision {number}")).on_hover_text(reference).clicked() {
                                    selected = Some((reference.to_owned(), None));
                                }
                            }
                        }
                    });
                    ui.label(s(amendment, "text"));
                    let source = revision.unwrap_or(&current);
                    if let Some((reference, name)) = source_file_links(ui, &amendment["attachments"], source) {
                        selected = Some((reference, Some((source.to_owned(), name))));
                    }
                }
            });
        if let Some((reference, named)) = selected {
            self.open_named_resource(ui.ctx(), reference, named);
        }
        ui.add_space(12.);
    }

    pub(super) fn delivered_packages(&self, include: impl Fn(&Value) -> bool) -> Vec<Value> {
        let packages = self.records("deliveries");
        self.home_index.deliveries.iter().filter_map(|i| packages.get(*i))
            .filter(|d| include(&d["body"])).cloned().collect()
    }

    pub(super) fn delivery_card(&mut self, ui: &mut Ui, d: &Value) {
        let b = &d["body"];
        let from_profile = self.layout.editor == Editor::People;
        let handle = inbox::handle(d);
        let review = &self.snapshot.status["delivery_reviews"][&handle];
        let accepted = review["accepted"].as_bool();
        let reviewer = s(review, "reviewer").to_owned();
        let n = b["artifacts"].as_array().map_or(0, Vec::len);
        let gaps = b["remaining"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Files, TEAL);
                ui.strong(s(b, "title"));
                let (label, c) = match s(b, "scope") {
                    "partial" => ("Partial delivery", theme::ACCENT),
                    "claimed_complete" => ("Author claims complete", TEAL),
                    _ => ("Delivered", TEAL),
                };
                theme::badge(ui, label, c);
                ui.weak(format!("by {}", self.label(s(b, "owner"))));
            });
            if let Some(accepted) = accepted {
                let verdict = if accepted { "Accepted" } else { "Changes requested" };
                ui.small(if reviewer.is_empty() { verdict.to_owned() } else { format!("{verdict} · {reviewer}") });
            }
            if from_profile {
                let activity = self.work_name(s(b, "work"));
                ui.small(excerpt(&activity, 100)).on_hover_text(activity);
            }
            ui.label(s(b, "supported_use"));
            if let Some(first) = gaps.first().and_then(Value::as_str) {
                ui.weak(format!("Still needed · {}", excerpt(first, 190))).on_hover_text(first);
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save package…").clicked() {
                    self.export_package(&handle, s(b, "title"));
                }
                if ui.small_button("Inspect delivery ↗").clicked() {
                    self.open_resource(ui.ctx(), handle.clone());
                }
            });
            ui.push_id(&handle, |ui| {
                let mut contents = format!("{n} {}", if n == 1 { "file" } else { "files" });
                if !gaps.is_empty() {
                    contents.push_str(&format!("  ·  {} remaining {}", gaps.len(), if gaps.len() == 1 { "item" } else { "items" }));
                }
                egui::CollapsingHeader::new(contents)
                    .default_open(self.layout.editor == Editor::Delivery).show(ui,
                    |ui| {
                        self.attachment_links(ui, &b["artifacts"]);
                        for gap in gaps.iter().filter_map(Value::as_str) {
                            ui.label(format!("• {gap}"));
                        }
                    },
                )
            });
        });
        if from_profile && self.layout.editor == Editor::Artifacts {
            self.work = s(b, "work").into();
            self.conversation_page = 0;
        }
    }
}
