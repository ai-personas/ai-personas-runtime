use super::*;
use ai_personas::neural::live::RunnerObservation;

enum Action {
    Timeline,
    Message,
    EarlierWork,
    Resource(String),
}

impl App {
    pub(super) fn profile_overview(&mut self, ui: &mut Ui, person: &Value) {
        let id = s(person, "id");
        let projects = self.observed_projects(id);
        let records = self.records("participation");
        let other: Vec<_> = self
            .home_index
            .participations
            .get(id)
            .into_iter()
            .flatten()
            .map(|i| &records[*i]["body"])
            .filter(|part| !projects.contains_key(s(part, "work")))
            .collect();
        let mut projects: Vec<_> = projects.into_iter().collect();
        projects.sort_by_key(|(work, _)| !self.runner_observation.has_worker(work, id));
        let mut action = None;
        let now = ai_personas::neural::now();
        section(ui, "Current work", "");
        if projects.is_empty() {
            let message = if matches!(
                self.runner_observation,
                RunnerObservation::Unavailable { .. }
            ) {
                "Runner status unavailable."
            } else if other.is_empty() {
                "No activity invitations yet."
            } else {
                "No live work observed."
            };
            ui.weak(message);
        }
        for (work, observations) in &projects {
            ui.push_id(("profile-current", work), |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.profile_activity_header(ui, work, id, &mut action);
                    ui.add_space(5.);
                    if let Some(reference) = self.work_observations(ui, id, work, observations, now)
                    {
                        action = Some(((*work).to_owned(), Action::Resource(reference)));
                    }
                });
                ui.add_space(5.);
            });
        }
        if !other.is_empty() {
            ui.add_space(10.);
            let title = if projects.is_empty() {
                "Activities"
            } else {
                "Other activities"
            };
            section(ui, title, &other.len().to_string());
            let mut participation = |ui: &mut Ui, part: &Value| {
                let work = s(part, "work");
                ui.push_id(("profile-participation", work), |ui| {
                    card(ui).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        self.profile_activity_header(ui, work, id, &mut action);
                        ui.add_space(3.);
                        ui.horizontal_wrapped(|ui| {
                            theme::badge(
                                ui,
                                &self.membership_summary(part),
                                theme::palette(ui).muted,
                            );
                        });
                        if let Some(reason) = part["continuation"]["reason"]
                            .as_str()
                            .or(part["continuation"]["purpose"].as_str())
                            .filter(|s| !s.is_empty())
                        {
                            ui.label(excerpt(reason, 225)).on_hover_text(reason);
                        }
                    });
                    ui.add_space(5.);
                });
            };
            let shown = other.len().min(6);
            for part in &other[..shown] {
                participation(ui, part);
            }
            if shown < other.len() {
                let more = counted(
                    (other.len() - shown) as u64,
                    "more activity",
                    "more activities",
                );
                ui.collapsing(more, |ui| {
                    for part in &other[shown..] {
                        participation(ui, part);
                    }
                });
            }
        }
        if let Some((work, target)) = action {
            match target {
                Action::Timeline => {
                    self.open_home_activity(&work);
                    self.live_board.show_person_timeline(&work, id);
                }
                Action::Message => self.open_home_message(&work, id),
                Action::EarlierWork => self.open_sources(ui.ctx(), &work, Some(id)),
                Action::Resource(reference) => {
                    self.open_home_activity(&work);
                    self.open_resource(ui.ctx(), reference);
                }
            }
        }
    }

    fn profile_activity_header(
        &self,
        ui: &mut Ui,
        work: &str,
        id: &str,
        action: &mut Option<(String, Action)>,
    ) {
        let activity = self.home_index.activity(work);
        let name = activity
            .map(|a| a.title.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or(work);
        let title = |ui: &mut Ui| {
            // The activity title can be the full original brief. Keep actions and
            // the live reply in reach for activities with a multi-paragraph brief.
            let mut job = egui::WidgetText::from(RichText::new(name).strong().size(16.))
                .into_layout_job(
                    ui.style(),
                    egui::FontSelection::Default,
                    egui::Align::Center,
                );
            Arc::make_mut(&mut job).wrap.max_rows = 2;
            ui.add(egui::Label::new(job).wrap())
                .on_hover_text(format!("{name}\n{work}"));
        };
        let Some(activity) = activity else {
            title(ui);
            return;
        };
        let member = activity.members.iter().any(|person| person == id);
        let mut actions = |ui: &mut Ui| {
            ui.horizontal_wrapped(|ui| {
                let person = self.label(id);
                let mut button = |caption: &str, description: String| {
                    let response = ui.button(caption);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            &description,
                        )
                    });
                    response.on_hover_text(description).clicked()
                };
                if button("Timeline", format!("Open {person}’s timeline in {name}")) {
                    *action = Some((work.into(), Action::Timeline));
                }
                if member && button("Message", format!("Message {person} in {name}")) {
                    *action = Some((work.into(), Action::Message));
                }
                if button(
                    "Earlier work",
                    format!("Files from earlier activities used by {person} in {name}"),
                ) {
                    *action = Some((work.into(), Action::EarlierWork));
                }
            });
        };
        if ui.available_width() >= 650. {
            ui.horizontal_top(|ui| {
                let spacing = ui.spacing();
                let per_button = 2. * spacing.button_padding.x + spacing.item_spacing.x;
                let mut actions_width = spacing.item_spacing.x + 4.;
                for caption in ["Timeline", "Message", "Earlier work"] {
                    if member || caption != "Message" {
                        let text = egui::WidgetText::from(caption).into_galley(
                            ui,
                            Some(egui::TextWrapMode::Extend),
                            f32::INFINITY,
                            egui::TextStyle::Button,
                        );
                        actions_width += text.size().x + per_button;
                    }
                }
                let title_width = ui.available_width() - actions_width;
                ui.allocate_ui_with_layout(
                    egui::vec2(title_width, 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(title_width);
                        title(ui);
                    },
                );
                actions(ui);
            });
        } else {
            title(ui);
            actions(ui);
        }
    }
}
