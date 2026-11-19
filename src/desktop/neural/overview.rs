use super::*;

impl App {
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
            .filter(|c| c["work"] == self.work)
            .count();
        let ended = record["body"]["phase"] == "dissolved";
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
                ui.add_space(9.);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !self.running && !self.busy && self.snapshot.active.is_empty(),
                            theme::primary("Run activity"),
                        )
                        .on_hover_text(
                            "Continue eligible personas here and in activities they request",
                        )
                        .clicked()
                    {
                        self.command(
                            ui.ctx(),
                            Command::Run {
                                watch: false,
                                work: None,
                                persona: None,
                                from_activity: Some(self.work.clone()),
                            },
                        );
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
                                self.persona = person.into();
                                self.layout.editor = Editor::Character;
                            }
                            ui.vertical(|ui| {
                                if ui.link(RichText::new(&name).strong().size(17.)).clicked() {
                                    self.persona = person.into();
                                    self.layout.editor = Editor::Character;
                                }
                                ui.small(self.persona_state(person));
                            });
                        });
                        if !reason.is_empty() {
                            ui.add_space(5.);
                            ui.label(excerpt(reason, 110)).on_hover_text(reason);
                        }
                    });
                }
            });
            ui.add_space(6.);
        }
        ui.add_space(10.);
        let mut deliveries: Vec<_> = self
            .records("deliveries")
            .iter()
            .filter(|r| r["body"]["work"] == self.work)
            .cloned()
            .collect();
        // Events retain issue order even where several packages share a timestamp.
        let order: BTreeMap<_, _> = self
            .events
            .iter()
            .filter(|e| e["work"] == self.work && e["kind"] == "delivery")
            .filter_map(|e| Some((e["body"]["reference"].as_str()?, e["event"].as_i64()?)))
            .collect();
        deliveries
            .sort_by_key(|r| std::cmp::Reverse(order.get(inbox::handle(r).as_str()).copied()));
        section(
            ui,
            "Delivered work",
            &format!("{} packages", deliveries.len()),
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

    fn delivery_card(&mut self, ui: &mut Ui, d: &Value) {
        let b = &d["body"];
        let handle = inbox::handle(d);
        let accepted = self.snapshot.status["delivery_reviews"][&handle]["accepted"].as_bool();
        let n = b["artifacts"].as_array().map_or(0, Vec::len);
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Files, TEAL);
                ui.strong(s(b, "title"));
                let (label, c) = match accepted {
                    Some(true) => ("Accepted", theme::GREEN),
                    Some(false) => ("Changes requested", theme::ACCENT),
                    None => ("Delivered", TEAL),
                };
                theme::badge(ui, label, c);
                ui.weak(format!("by {}", self.label(s(b, "owner"))));
            });
            ui.label(s(b, "supported_use"));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save package…").clicked() {
                    self.export_package(&handle, s(b, "title"));
                }
                if ui.small_button("Inspect delivery ↗").clicked() {
                    self.open_resource(ui.ctx(), handle.clone());
                }
            });
            ui.push_id(&handle, |ui| {
                ui.collapsing(
                    format!(
                        "Files and remaining work  ·  {n} {}",
                        if n == 1 { "file" } else { "files" }
                    ),
                    |ui| {
                        self.attachment_links(ui, &b["artifacts"]);
                        for gap in b["remaining"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            ui.label(format!("• {gap}"));
                        }
                    },
                )
            });
        });
    }
}
