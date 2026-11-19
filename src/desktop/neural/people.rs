use super::*;

impl App {
    pub(super) fn show_person(&mut self, id: &str) {
        self.persona = id.into();
        self.layout.editor = Editor::People;
    }

    pub(super) fn people(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .id_salt(("persona-portrait", self.persona.clone()))
            .show(ui, |ui| {
                let person = self.selected_person();
                if person.is_null() {
                    ui.heading("Personas");
                    ui.label("Create a persona to see their identity and work here.");
                    return;
                }
                let id = s(&person, "id");
                let name = s(&person, "label");
                let color = self.color(id);
                let members: Vec<_> = self
                    .records("participation")
                    .iter()
                    .filter(|r| r["body"]["persona"] == id)
                    .map(|r| r["body"].clone())
                    .collect();
                let active_work = self
                    .snapshot
                    .active
                    .iter()
                    .find(|a| a["actor"] == id)
                    .map(|a| s(a, "work").to_owned());
                ui.horizontal_wrapped(|ui| {
                    avatar(ui, name, color, 56.);
                    ui.vertical(|ui| {
                        eyebrow(ui, "Persona");
                        ui.label(RichText::new(name).strong().size(28.));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::ComboBox::from_id_salt("portrait-person")
                            .selected_text(name)
                            .show_ui(ui, |ui| {
                                for record in self.records("personas").to_vec() {
                                    ui.selectable_value(
                                        &mut self.persona,
                                        s(&record["body"], "id").into(),
                                        s(&record["body"], "label"),
                                    );
                                }
                            });
                    });
                });
                ui.add_space(12.);
                let model = self
                    .snapshot
                    .characters
                    .get(id)
                    .map(|c| c.baseline.inclinations());
                card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let mut identity = |ui: &mut Ui| {
                        ui.strong("Origin");
                        ui.label(format!("Introduced by {}", s(&person, "seed_author")));
                        ui.label(s(&person, "seed"));
                        ui.add_space(8.);
                        if ui.button("Open Character Lab →").clicked() {
                            self.layout.editor = Editor::Character;
                        }
                    };
                    if let Some(values) = model.filter(|v| v.len() >= 3) {
                        if ui.available_width() > 720. {
                            ui.columns(2, |cols| {
                                cols[0].with_layout(
                                    egui::Layout::top_down(egui::Align::Min),
                                    &mut identity,
                                );
                                signature(&mut cols[1], &values, color);
                            });
                        } else {
                            identity(ui);
                            ui.add_space(8.);
                            signature(ui, &values, color);
                        }
                    } else {
                        identity(ui);
                    }
                });
                ui.add_space(14.);
                section(ui, "Activities", &members.len().to_string());
                if members.is_empty() {
                    ui.weak("This persona has no activity invitations yet.");
                }
                for p in members {
                    let w = s(&p, "work");
                    let (state, tint) = if active_work.as_deref() == Some(w) {
                        ("Working now", TEAL)
                    } else if p["closed_by_activity"].is_string() {
                        ("Activity ended", Color32::GRAY)
                    } else if p["departure"].is_string() {
                        ("Left", Color32::GRAY)
                    } else if p["declined"] == true {
                        ("Declined", PURPLE)
                    } else if p["accepted"] != true {
                        ("Invited", theme::ACCENT)
                    } else {
                        match s(&p["continuation"], "kind") {
                            "wait" => ("Waiting", theme::ACCENT),
                            "stop" => ("Stopped", Color32::GRAY),
                            "ready" => ("Ready", TEAL),
                            _ => ("Participating", TEAL),
                        }
                    };
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(ui, state, tint);
                        let need = self.work_name(w);
                        if ui.link(excerpt(&need, 90)).on_hover_text(need).clicked() {
                            self.work = w.into();
                            self.conversation_page = 0;
                            self.layout.editor = Editor::Studio;
                        }
                        if ui.small_button("Earlier work").on_hover_text("Files from other activities recorded for this persona here").clicked() {
                            self.open_sources(ui.ctx(), w, Some(id));
                        }
                    });
                    if let Some(reason) = p["continuation"]["reason"]
                        .as_str()
                        .or(p["continuation"]["purpose"].as_str())
                    {
                        ui.label(excerpt(reason, 225)).on_hover_text(reason);
                    }
                    ui.add_space(6.);
                }
                let requests: Vec<_> = self
                    .records("human_requests")
                    .iter()
                    .filter(|r| r["body"]["requester"] == id)
                    .cloned()
                    .collect();
                if !requests.is_empty() {
                    ui.add_space(8.);
                    section(ui, "Requests for a human", &requests.len().to_string());
                    for r in requests {
                        let b = &r["body"];
                        ui.horizontal_wrapped(|ui| {
                            ui.weak(title(s(b, "state")));
                            if ui.link(s(b, "title")).clicked() {
                                self.show_human_request(s(&r["reference"], "id"));
                            }
                        });
                    }
                }
                let files: Vec<_> = self
                    .snapshot
                    .assets
                    .iter()
                    .filter(|a| {
                        a["current"] == true
                            && a["body"]["retired"] != true
                            && a["body"]
                                .get("last_content_author")
                                .unwrap_or(&a["body"]["owner"])
                                == id
                    })
                    .cloned()
                    .collect();
                ui.add_space(12.);
                section(ui, "Created files", &files.len().to_string());
                if files.is_empty() {
                    ui.weak("No current files authored by this persona.");
                }
                for a in files.iter().take(6) {
                    self.artifact_row(ui, a, true);
                }
                if files.len() > 6 {
                    ui.collapsing(format!("{} more files by {name}", files.len() - 6), |ui| {
                        for file in &files[6..] {
                            self.artifact_row(ui, file, true);
                        }
                    });
                }
            });
    }
}

fn signature(ui: &mut Ui, values: &BTreeMap<String, f64>, color: Color32) {
    ui.strong("Character at baseline");
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 154.), egui::Sense::hover());
    let center = rect.center();
    let radius = (rect.width() / 2. - 75.).clamp(22., 53.);
    let p = theme::palette(ui);
    let painter = ui.painter_at(rect);
    let mut points = Vec::new();
    let mut hovered = None;
    for fraction in [0.5, 1.] {
        painter.circle_stroke(center, radius * fraction, Stroke::new(1., p.border));
    }
    for (i, (name, value)) in values.iter().enumerate() {
        let angle =
            std::f32::consts::TAU * i as f32 / values.len() as f32 - std::f32::consts::FRAC_PI_2;
        let direction = egui::vec2(angle.cos(), angle.sin());
        let edge = center + direction * radius;
        painter.line_segment([center, edge], Stroke::new(1., p.hover));
        let endpoint = center + direction * (radius * ((*value as f32 + 1.) * 0.5).clamp(0., 1.));
        points.push(endpoint);
        let align = if direction.x.abs() < 0.2 {
            egui::Align2::CENTER_CENTER
        } else if direction.x < 0. {
            egui::Align2::RIGHT_CENTER
        } else {
            egui::Align2::LEFT_CENTER
        };
        painter.text(
            center + direction * (radius + 11.),
            align,
            title(name),
            FontId::proportional(10.5),
            p.text,
        );
        if response
            .hover_pos()
            .is_some_and(|pos| pos.distance(endpoint) < 12.)
        {
            hovered = Some(format!("{}  {value:+.3}", title(name)));
        }
    }
    for i in 0..points.len() {
        let next = points[(i + 1) % points.len()];
        painter.add(egui::Shape::convex_polygon(
            vec![center, points[i], next],
            color.gamma_multiply(0.18),
            Stroke::NONE,
        ));
        painter.line_segment([points[i], next], Stroke::new(1.8, color));
        painter.circle_filled(points[i], 2.5, color);
    }
    if let Some(text) = hovered {
        response.on_hover_text(text);
    }
    ui.small("Each inclination runs from −1 at the centre to +1 at the edge.");
}
