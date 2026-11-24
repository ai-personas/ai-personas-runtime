use super::*;

fn activity_option(
    ui: &mut Ui,
    a: &home_index::Activity,
    h: &History,
    members: &[usize],
    count: usize,
    selected: bool,
) -> egui::Response {
    let p = theme::palette(ui);
    let (rect, r) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 60.), egui::Sense::click());
    r.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            &a.title,
        )
    });
    if selected || r.hovered() || r.has_focus() {
        ui.painter()
            .rect_filled(rect, 4, if selected { p.selection } else { p.hover });
    } else {
        ui.painter()
            .hline(rect.x_range(), rect.top(), Stroke::new(1., p.border));
    }
    let label = RichText::new(short_text(&a.title, 180))
        .size(14.)
        .color(p.text);
    let mut job = egui::WidgetText::from(label).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        egui::Align::LEFT,
    );
    let wrap = &mut Arc::make_mut(&mut job).wrap;
    wrap.max_width = (rect.width() - 16.).max(8.);
    wrap.max_rows = 2;
    let galley = ui.fonts_mut(|f| f.layout_job((*job).clone()));
    ui.painter()
        .galley(rect.min + egui::vec2(8., 4.), galley, p.text);
    let mut x = rect.left() + 8.;
    let y = rect.bottom() - 17.;
    for &n in members.iter().take(3) {
        map::mark(
            ui,
            egui::Rect::from_min_size(egui::pos2(x, y - 1.), egui::Vec2::splat(15.)),
            h, n,
        );
        x += 12.;
    }
    if !members.is_empty() {
        x += 8.;
    }
    let kind = if a.kind == "social" {
        "Conversation"
    } else {
        "Task"
    };
    let mut detail = format!(
        "{kind} · {}",
        counted(count as u64, "contribution", "contributions")
    );
    if !members.is_empty() {
        detail.push_str(" · ");
        detail.push_str(
            &members
                .iter()
                .take(3)
                .map(|&n| h.name(n))
                .collect::<Vec<_>>()
                .join(", "),
        );
        if members.len() > 3 {
            detail.push_str(&format!(" +{}", members.len() - 3));
        }
    }
    map::text_line(
        ui,
        egui::pos2(x, y - 2.),
        rect.right() - x - 5.,
        RichText::new(detail).size(12.),
        p.muted,
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| {
            ui.label(&a.title);
            ui.small(&a.id);
        })
}

impl View {
    pub(super) fn activity_picker(
        &mut self,
        ui: &mut Ui,
        archive: &Archive,
        index: &home_index::Index,
    ) {
        let selected = self.activity.as_deref().and_then(|id| index.activity(id));
        let title = selected.map_or("All activities", |a| a.title.as_str());
        let room = ui.available_size_before_wrap().x;
        let row = ui.max_rect().width();
        let width = if selected.is_some() {
            if room >= 146. {
                room.min(250.)
            } else {
                row.min(250.)
            }
        } else {
            row.min(126.)
        };
        let button_size = egui::vec2(width, ui.spacing().interact_size.y);
        let layout = egui::Layout::top_down(egui::Align::Min);
        let popup = ui
            .allocate_ui_with_layout(button_size, layout, |ui| {
                ui.set_width(width);
                egui::ComboBox::from_id_salt("society-activity")
                    .width(width)
                    .truncate()
                    .height(340.)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .selected_text(short_text(title, 100))
                    .show_ui(ui, |ui| self.activity_options(ui, archive, index))
            })
            .inner;
        if popup.response.clicked() && popup.inner.is_some() {
            ui.memory_mut(|memory| memory.request_focus(egui::Id::new("society-activity-query")));
        }
        popup.response.on_hover_text(title);
        if let Some(work) = popup.inner.flatten() {
            self.focus_activity(work);
        }
    }

    fn activity_options(
        &mut self,
        ui: &mut Ui,
        archive: &Archive,
        index: &home_index::Index,
    ) -> Option<Option<String>> {
        let w = (ui.ctx().content_rect().width() - 44.).clamp(105., 370.);
        ui.set_width(w);
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .id(egui::Id::new("society-activity-query"))
                .desired_width(f32::INFINITY)
                .hint_text("Find activity or persona…"),
        );
        if ui
            .selectable_label(self.activity.is_none(), "All activities")
            .clicked()
        {
            ui.close();
            return Some(None);
        }
        let query = self.query.trim().to_lowercase();
        let mut found = false;
        for activity in &index.activities {
            let history = archive.history(Some(&activity.id));
            let end = history.time.end(self.held);
            let members: Vec<_> = activity
                .members
                .iter()
                .filter_map(|id| archive.persona_node(id))
                .filter(|&n| self.held.is_none() || history.time.involved(n, end))
                .collect();
            let member_matches = |&n: &usize| {
                history.name(n).to_lowercase().contains(&query)
                    || matches!(&history.nodes[n], Identity::Persona(id) if id.to_lowercase().contains(&query))
            };
            if !query.is_empty()
                && !activity.title.to_lowercase().contains(&query)
                && !activity.id.to_lowercase().contains(&query)
                && !members.iter().any(member_matches)
            {
                continue;
            }
            found = true;
            let selected = self.activity.as_deref() == Some(&activity.id);
            let r = activity_option(ui, activity, &history, &members, end, selected);
            if r.clicked() {
                ui.close();
                return Some(Some(activity.id.clone()));
            }
        }
        if !found {
            ui.weak("No matching activities.");
        }
        None
    }
}
