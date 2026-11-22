use super::*;

impl App {
    pub(super) fn ensemble_shortcuts(&mut self, ui: &mut Ui, in_menu: bool) {
        let people = self.records("personas");
        if people.is_empty() {
            return;
        }
        let palette = theme::Palette::new(!in_menu || ui.visuals().dark_mode);
        let mut open = None;
        let mut portrait = |ui: &mut Ui, person: &Value, width: f32| {
            let body = &person["body"];
            let id = s(body, "id");
            let name = body["label"]
                .as_str()
                .filter(|label| !label.is_empty())
                .unwrap_or(id);
            let active = match &self.runner_observation {
                ai_personas::neural::live::RunnerObservation::Observed { workers, .. } => {
                    workers.get(id)
                }
                _ => None,
            };
            let selected = self.layout.editor == Editor::People && self.persona == id;
            let (r, button) = ui.allocate_exact_size(egui::vec2(width, 42.), egui::Sense::click());
            button.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    ui.is_enabled(),
                    selected,
                    format!("Open {name}’s profile ({id})"),
                )
            });
            if selected || button.hovered() || button.has_focus() {
                ui.painter().rect(
                    r,
                    5,
                    palette.hover,
                    Stroke::new(1., palette.ink(persona_color(id))),
                    egui::StrokeKind::Inside,
                );
            }
            let mark = egui::Rect::from_center_size(
                egui::pos2(r.center().x, r.top() + 13.),
                egui::Vec2::splat(23.),
            );
            design::persona_mark(ui, mark, id, persona_color(id));
            if active.is_some() {
                ui.painter().circle(
                    mark.right_top() + egui::vec2(-1., 2.),
                    3.5,
                    palette.ink(TEAL),
                    Stroke::new(1., palette.field),
                );
            }
            let label = egui::WidgetText::from(RichText::new(name).size(12.)).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                (width - 2.).max(1.),
                egui::TextStyle::Small,
            );
            ui.painter().galley(
                egui::pos2(r.center().x - label.size().x * 0.5, r.bottom() - 16.),
                label,
                palette.text,
            );
            let response = button
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_ui(|ui| {
                    ui.strong(format!("{name} · {id}"));
                    if let Some(work) = active {
                        let title = self
                            .home_index
                            .activity(work)
                            .map_or(work.as_str(), |a| a.title.as_str());
                        ui.label(format!("Working in {title}"));
                    } else {
                        ui.label("Open persona profile");
                    }
                });
            if response.clicked() {
                open = Some(id.to_owned());
            }
        };
        ui.add_space(if in_menu { 4. } else { 12. });
        ui.label(design::caption("Ensemble", palette.muted));
        // A small roster stays directly accessible. Larger rosters expand inside the navigation scroll area.
        let cols = if ui.available_width() < 116. {
            2
        } else if matches!(people.len(), 3 | 5 | 6) {
            3
        } else {
            4
        };
        let gap = 2.;
        let width = ((ui.available_width() - gap * (cols - 1) as f32) / cols as f32)
            .max(16.)
            .floor();
        let draw = |ui: &mut Ui, rows: &[Value], portrait: &mut dyn FnMut(&mut Ui, &Value, f32)| {
            for row in rows.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for person in row {
                        portrait(ui, person, width);
                    }
                });
            }
        };
        let shown = people.len().min(cols * 2);
        draw(ui, &people[..shown], &mut portrait);
        if shown < people.len() {
            egui::CollapsingHeader::new(format!("{} more personas", people.len() - shown))
                .id_salt("ensemble-overflow")
                .show(ui, |ui| draw(ui, &people[shown..], &mut portrait));
        }
        if let Some(id) = open {
            self.open_roster_profile(&id);
            if in_menu {
                ui.close();
            }
        }
    }
}
