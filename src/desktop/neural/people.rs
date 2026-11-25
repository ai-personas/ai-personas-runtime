use super::*;
mod briefs;
mod files;
mod images;
pub(super) use briefs::ReceivedBriefs;
pub(super) use images::VisualHistory;
mod work;
pub(super) use files::AuthoredFiles;

#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
pub(super) enum ProfileView {
    #[default]
    Overview,
    Packages,
    Briefs,
    Images,
    Files,
    Connections,
    Requests,
    Origin,
}

pub(super) fn starting_profile_credit(person: &Value) -> String {
    match s(person, "seed_author").trim() {
        "" => "Starting profile".into(),
        author => format!("Starting profile by {author}"),
    }
}

pub(super) fn starting_description(ui: &mut Ui, person: &Value) {
    let seed = s(person, "seed").trim();
    if !seed.is_empty() {
        let copy = RichText::new(home::short_text(seed, 320)).size(15.);
        let mut job = egui::WidgetText::from(copy).into_layout_job(
            ui.style(),
            egui::FontSelection::Default,
            egui::Align::Center,
        );
        Arc::make_mut(&mut job).wrap.max_rows = 2;
        ui.add(egui::Label::new(job).wrap()).on_hover_ui(|ui| {
            ui.label(seed);
        });
    }
}

pub(super) fn scroll_section<R>(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    page_scrolls: bool,
    draw: impl FnOnce(&mut Ui) -> R,
) -> R {
    if page_scrolls {
        ui.push_id(id, draw).inner
    } else {
        egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .show(ui, draw)
            .inner
    }
}

impl App {
    pub(super) fn show_person(&mut self, id: &str) {
        self.compact_browser = false;
        if self.layout.editor != Editor::People {
            self.profile_view = ProfileView::Overview;
        }
        self.persona = id.into();
        self.layout.editor = Editor::People;
        self.align_profile_activity();
    }

    pub(super) fn align_profile_activity(&mut self) {
        if self
            .home_index
            .activity(&self.work)
            .is_some_and(|a| a.members.contains(&self.persona))
        {
            return;
        }
        let latest = self
            .home_index
            .participations
            .get(&self.persona)
            .and_then(|parts| parts.first())
            .and_then(|index| self.records("participation").get(*index))
            .and_then(|r| r["body"]["work"].as_str())
            .unwrap_or_default();
        if self.work != latest {
            self.work = latest.to_owned();
            self.conversation_page = 0;
        }
    }

    pub(super) fn people(&mut self, ui: &mut Ui) {
        if self.selected_person().is_null() {
            ui.heading("Personas");
            ui.label("Create a persona to see their identity and work here.");
            return;
        }
        self.align_profile_activity();
        let compact = Self::constrained_activity_layout(ui.ctx());
        if compact {
            egui::ScrollArea::vertical()
                .id_salt(("whole-persona-profile", &self.persona, self.profile_view))
                .min_scrolled_height(1.)
                .show(ui, |ui| self.profile_page(ui, true));
        } else {
            self.profile_page(ui, false);
        }
    }

    fn profile_page(&mut self, ui: &mut Ui, page_scrolls: bool) {
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
        let tabs = [
            (ProfileView::Overview, "Overview".to_owned()),
            (ProfileView::Briefs, "Received briefs".to_owned()),
            (ProfileView::Images, "Visual history".to_owned()),
            (
                ProfileView::Packages,
                format!("Delivered packages  {packages}"),
            ),
            (ProfileView::Connections, "Correspondence".to_owned()),
            (ProfileView::Files, files_label),
            (ProfileView::Requests, request_label),
            (ProfileView::Origin, "Origin".to_owned()),
        ];
        let current = tabs
            .iter()
            .find(|(view, _)| *view == self.profile_view)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let mut choose = |ui: &mut Ui| {
            for (view, label) in &tabs {
                ui.selectable_value(&mut self.profile_view, *view, label.as_str());
            }
        };
        if Self::constrained_activity_layout(ui.ctx()) || ui.available_width() < 370. {
            egui::ComboBox::from_id_salt("profile-section")
                .selected_text(current)
                .width(ui.available_width().min(230.))
                .show_ui(ui, &mut choose);
        } else {
            ui.horizontal_wrapped(&mut choose);
        }
        ui.separator();
        let view = self.profile_view;
        let mut contents = |ui: &mut Ui| {
            ui.add_space(6.);
            match view {
                ProfileView::Overview => {
                    if (Self::constrained_activity_layout(ui.ctx()) || ui.available_width() < 370.) && !s(&person, "seed").trim().is_empty() {
                        if ui
                            .link(RichText::new(starting_profile_credit(&person)).small())
                            .clicked()
                        {
                            self.profile_view = ProfileView::Origin;
                        }
                        starting_description(ui, &person);
                        ui.add_space(10.);
                    }
                    self.profile_overview(ui, &person);
                }
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
                ProfileView::Briefs => {
                    let name = self.label(id).to_owned();
                    if let Some(decision) = self.profile_briefs.show(ui, &self.root, id, &name) {
                        self.open_choice_or_record(ui.ctx(), decision);
                    }
                }
                ProfileView::Images => {
                    let name = self.label(id).to_owned();
                    if let Some(reference) = self.profile_images.show(ui, &self.root, id, &name, &self.home_index) {
                        self.open_choice_or_record(ui.ctx(), reference);
                    }
                }
                ProfileView::Connections => self.show_relationships(ui),
                ProfileView::Files => self.profile_authored_files(ui, id, page_scrolls),
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
        if matches!(view, ProfileView::Files | ProfileView::Connections) {
            contents(ui);
        } else {
            scroll_section(ui, ("persona-portrait", id, view), page_scrolls, contents);
        }
    }

    fn profile_heading(&mut self, ui: &mut Ui) {
        let current = self.label(&self.persona);
        let color = self.color(&self.persona);
        let mut selected = None;
        if Self::constrained_activity_layout(ui.ctx()) || ui.available_width() < 370. {
            let width = ui.available_width().max(1.);
            egui::ComboBox::from_id_salt("profile-persona")
                .width(width.min(230.))
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for record in self.records("personas") {
                        let person = &record["body"];
                        let id = s(person, "id");
                        if ui
                            .selectable_label(id == self.persona, s(person, "label"))
                            .on_hover_text(id)
                            .clicked()
                        {
                            selected = Some(id.to_owned());
                        }
                    }
                });
            if let Some(id) = selected {
                self.show_person(&id);
            }
            return;
        }
        let p = theme::palette(ui);
        let person = self.selected_person();
        let mut show_origin = false;
        egui::Frame::new().fill(p.panel.lerp_to_gamma(color, 0.10))
            .stroke(egui::Stroke::new(1., p.border.lerp_to_gamma(color, 0.38)))
            .inner_margin(16).corner_radius(10).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    avatar(ui, current, color, 62.);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(design::caption("Persona profile", p.ink(color)));
                            if self.records("personas").len() > 1 {
                                design::action_at_right(ui, |ui| {
                                    ui.menu_button("Switch persona", |ui| {
                                        ui.set_max_width(360.);
                                        for record in self.records("personas") {
                                            let body = &record["body"];
                                            let id = s(body, "id");
                                            let name = body["label"].as_str().unwrap_or(id);
                                            if ui.selectable_label(id == self.persona, name).on_hover_text(id).clicked() {
                                                selected = Some(id.to_owned());
                                                ui.close();
                                            }
                                        }
                                    });
                                });
                            }
                        });
                        ui.add(egui::Label::new(design::display(current, 42.)).truncate()).on_hover_text(&self.persona);
                        if !s(&person, "seed").trim().is_empty() {
                            show_origin = ui.link(RichText::new(starting_profile_credit(&person)).small()).clicked();
                            starting_description(ui, &person);
                        }
                    });
                });
            });
        ui.add_space(6.);
        if show_origin { self.profile_view = ProfileView::Origin; }
        if let Some(id) = selected {
            self.show_person(&id);
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
                ui.small(starting_profile_credit(person));
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
