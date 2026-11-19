use super::*;

#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
pub(super) enum ProfileView {
    #[default]
    Overview,
    Packages,
    Files,
    Requests,
}

impl App {
    pub(super) fn show_person(&mut self, id: &str) {
        if self.layout.editor != Editor::People {
            self.profile_view = ProfileView::Overview;
        }
        self.persona = id.into();
        self.layout.editor = Editor::People;
    }

    pub(super) fn people(&mut self, ui: &mut Ui) {
        if self.selected_person().is_null() {
            ui.heading("Personas");
            ui.label("Create a persona to see their identity and work here.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let current = self.label(&self.persona);
            avatar(ui, current, self.color(&self.persona), 46.);
            ui.vertical(|ui| {
                eyebrow(ui, "Persona");
                ui.label(RichText::new(current).strong().size(26.));
            });
            if self.records("personas").len() > 1 {
                ui.menu_button("Switch persona", |ui| {
                    for record in self.records("personas").to_vec() {
                        if ui
                            .selectable_value(
                                &mut self.persona,
                                s(&record["body"], "id").into(),
                                s(&record["body"], "label"),
                            )
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
            }
        });
        let person = self.selected_person();
        let id = s(&person, "id");
        let packages = self
            .records("deliveries")
            .iter()
            .filter(|r| r["body"]["owner"] == id)
            .count();
        let authored_file = |file: &&Value| {
            let b = &file["body"];
            let author = b["last_content_author"].as_str().or(b["owner"].as_str());
            file["current"] == true && b["retired"] != true && author == Some(id)
        };
        let files = self.snapshot.assets.iter().filter(authored_file).count();
        let own_request = |r: &&Value| r["body"]["requester"] == id;
        let is_open = |r: &Value| matches!(s(&r["body"], "state"), "open" | "in_progress");
        let requests = self.records("human_requests").iter().filter(own_request);
        let open = requests.clone().filter(|r| is_open(r)).count();
        let request_label = if open > 0 {
            format!("Human requests  ·  {open} active")
        } else {
            format!("Human requests  {}", requests.count())
        };
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.profile_view, ProfileView::Overview, "Overview");
            ui.selectable_value(
                &mut self.profile_view,
                ProfileView::Packages,
                format!("Delivered packages  {packages}"),
            );
            ui.selectable_value(
                &mut self.profile_view,
                ProfileView::Files,
                format!("Authored files  {files}"),
            );
            ui.selectable_value(&mut self.profile_view, ProfileView::Requests, request_label);
        });
        ui.separator();
        let view = self.profile_view;
        egui::ScrollArea::vertical()
            .id_salt(("persona-portrait", id, view))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.);
                match view {
                    ProfileView::Overview => self.profile_overview(ui, &person),
                    ProfileView::Packages => {
                        if packages == 0 {
                            ui.weak("This persona has not issued a package yet.");
                        }
                        for delivery in self.delivered_packages(|body| body["owner"] == id) {
                            self.delivery_card(ui, &delivery);
                            ui.add_space(8.);
                        }
                    }
                    ProfileView::Files => {
                        if files == 0 {
                            ui.weak("This persona has no current authored files.");
                        }
                        let rows: Vec<_> = self
                            .snapshot
                            .assets
                            .iter()
                            .filter(authored_file)
                            .cloned()
                            .collect();
                        for file in rows.iter().take(12) {
                            self.artifact_row(ui, file, true);
                        }
                        if rows.len() > 12 {
                            ui.collapsing(format!("Show {} more files", rows.len() - 12), |ui| {
                                for file in &rows[12..] {
                                    self.artifact_row(ui, file, true);
                                }
                            });
                        }
                    }
                    ProfileView::Requests => {
                        let mut sorted_requests: Vec<_> = self
                            .records("human_requests")
                            .iter()
                            .filter(own_request)
                            .cloned()
                            .collect();
                        sorted_requests.sort_by_key(|r| {
                            (
                                !is_open(r),
                                std::cmp::Reverse(r["body"]["updated"].as_i64()),
                            )
                        });
                        if sorted_requests.is_empty() {
                            ui.weak("This persona has not requested human help.");
                        }
                        for request in &sorted_requests {
                            self.request_card(ui, request);
                            ui.add_space(8.);
                        }
                    }
                }
            });
    }

    fn profile_overview(&mut self, ui: &mut Ui, person: &Value) {
        let id = s(person, "id");
        let color = self.color(id);
        let model = self
            .snapshot
            .characters
            .get(id)
            .map(|c| c.baseline.inclinations());
        card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let mut identity = |ui: &mut Ui| {
                ui.strong("Origin");
                ui.small(format!(
                    "Introduced by {}",
                    self.label(s(person, "seed_author"))
                ));
                ui.label(s(person, "seed"));
                ui.add_space(8.);
                if ui.button("Open Character Lab →").clicked() {
                    self.layout.editor = Editor::Character;
                }
            };
            if let Some(values) = model.filter(|v| v.len() >= 3) {
                if ui.available_width() > 720. {
                    ui.columns(2, |cols| {
                        cols[0]
                            .with_layout(egui::Layout::top_down(egui::Align::Min), &mut identity);
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
        let mut activities: Vec<_> = self
            .records("participation")
            .iter()
            .filter(|r| r["body"]["persona"] == id)
            .map(|r| {
                let b = &r["body"];
                let work = s(b, "work");
                let occupied = self
                    .snapshot
                    .active
                    .iter()
                    .chain(&self.snapshot.active_effects)
                    .any(|r| r["actor"] == id && r["work"] == work);
                (
                    b.clone(),
                    self.work_name(work),
                    self.persona_state_in(id, work),
                    occupied,
                )
            })
            .collect();
        activities.sort_by_cached_key(|(b, name, _, occupied)| {
            (!*occupied, b["work"] != self.work, name.to_lowercase())
        });
        section(ui, "Activities", &activities.len().to_string());
        if activities.is_empty() {
            ui.weak("This persona has no activity invitations yet.");
        }
        for (member, name, state, occupied) in activities {
            let work = s(&member, "work");
            ui.push_id(work, |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(
                            ui,
                            &state,
                            if occupied {
                                TEAL
                            } else {
                                theme::palette(ui).muted
                            },
                        );
                        if ui
                            .link(RichText::new(excerpt(&name, 90)).strong())
                            .on_hover_text(&name)
                            .clicked()
                        {
                            self.profile_activity(work);
                        }
                    });
                    if let Some(reason) = member["continuation"]["reason"]
                        .as_str()
                        .or(member["continuation"]["purpose"].as_str())
                    {
                        ui.label(excerpt(reason, 225)).on_hover_text(reason);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .small_button(format!("Message {}", self.label(id)))
                            .clicked()
                        {
                            self.profile_activity(work);
                            self.address_in_live(id);
                        }
                        if ui
                            .small_button("Earlier work")
                            .on_hover_text("Files from other activities used here")
                            .clicked()
                        {
                            self.open_sources(ui.ctx(), work, Some(id));
                        }
                    });
                });
            });
            ui.add_space(6.);
        }
    }

    fn profile_activity(&mut self, work: &str) {
        self.work = work.into();
        self.conversation_page = 0;
        self.live_board.show_activity(work);
        self.layout.editor = Editor::Studio;
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
