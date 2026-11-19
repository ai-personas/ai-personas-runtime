use super::*;
mod files;
mod work;
pub(super) use files::AuthoredFiles;

#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
pub(super) enum ProfileView {
    #[default]
    Overview,
    Packages,
    Files,
    Requests,
    Origin,
}

impl App {
    pub(super) fn show_person(&mut self, id: &str) {
        self.compact_browser = false;
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
        self.profile_heading(ui);
        let person = self.selected_person();
        let id = s(&person, "id");
        self.profile_files.observe(
            ui.ctx(),
            &self.root,
            id,
            &self.home_index,
            &self.snapshot.assets,
        );
        let packages = self
            .records("deliveries")
            .iter()
            .filter(|r| r["body"]["owner"] == id)
            .count();
        let files_label = self.profile_files.total().map_or_else(
            || "Authored files".to_owned(),
            |total| format!("Authored files  {total}"),
        );
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
            ui.selectable_value(&mut self.profile_view, ProfileView::Files, files_label);
            ui.selectable_value(&mut self.profile_view, ProfileView::Requests, request_label);
            ui.selectable_value(&mut self.profile_view, ProfileView::Origin, "Origin");
        });
        ui.separator();
        let view = self.profile_view;
        let mut contents = |ui: &mut Ui| {
            ui.add_space(6.);
            match view {
                ProfileView::Overview => self.profile_overview(ui, &person),
                ProfileView::Origin => self.profile_origin(ui, &person),
                ProfileView::Packages => {
                    if packages == 0 {
                        ui.weak("This persona has not issued a package yet.");
                    }
                    for delivery in self.delivered_packages(|body| body["owner"] == id) {
                        self.delivery_card(ui, &delivery);
                        ui.add_space(8.);
                    }
                }
                ProfileView::Files => self.profile_authored_files(ui, id),
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
        };
        if view == ProfileView::Files {
            contents(ui);
        } else {
            egui::ScrollArea::vertical()
                .id_salt(("persona-portrait", id, view))
                .auto_shrink([false, false])
                .show(ui, contents);
        }
    }

    fn profile_heading(&mut self, ui: &mut Ui) {
        let current = self.label(&self.persona);
        let color = theme::palette(ui).ink(self.color(&self.persona));
        let mut selected = None;
        ui.horizontal(|ui| {
            avatar(ui, current, color, 44.);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 44.),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if self.records("personas").len() > 1 {
                        ui.menu_button("Switch persona", |ui| {
                            ui.set_max_width(
                                (ui.ctx().content_rect().width() - 40.).clamp(180., 420.),
                            );
                            for record in self.records("personas") {
                                let body = &record["body"];
                                let id = s(body, "id");
                                let name = body["label"].as_str().unwrap_or(id);
                                let row = egui::Button::selectable(id == self.persona, name).wrap();
                                if ui.add(row).on_hover_text(id).clicked() {
                                    selected = Some(id.to_owned());
                                    ui.close();
                                }
                            }
                        });
                    }
                    let width = ui.available_width().max(1.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 44.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(width);
                            eyebrow(ui, "Persona");
                            ui.add(
                                egui::Label::new(RichText::new(current).strong().size(26.))
                                    .truncate(),
                            )
                            .on_hover_text(format!("{current}\n{}", self.persona));
                        },
                    );
                },
            );
        });
        if let Some(id) = selected {
            self.persona = id;
        }
    }

    fn profile_origin(&mut self, ui: &mut Ui, person: &Value) {
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
