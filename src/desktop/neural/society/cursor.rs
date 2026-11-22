use super::*;

impl View {
    pub(super) fn time_cursor(&mut self, ui: &mut Ui, app: &App, history: &History) -> usize {
        let total = history.entries.len();
        if total == 0 {
            return 0;
        }
        let mut end = self.held.unwrap_or(total).min(total);
        let p = theme::palette(ui);
        let narrow = ui.available_width() < 640.;
        let stamp = |ui: &mut Ui, end: usize| {
            if let Some(entry) = end.checked_sub(1).and_then(|i| history.entries.get(i)) {
                let event = &app.events[entry.event];
                ui.label(
                    RichText::new(format!("#{}   {}", event["event"], s(event, "created_utc")))
                        .size(12.)
                        .color(p.muted),
                );
            }
        };
        let mut controls = |ui: &mut Ui| {
            if ui
                .add_enabled(end > 1, egui::Button::new("Earlier").small())
                .clicked()
            {
                self.held = Some(end - 1);
            }
            if ui
                .add_enabled(end < total, egui::Button::new("Later").small())
                .clicked()
            {
                self.held = Some(end + 1);
            }
            let follow = if self.held.is_some() {
                "Follow latest"
            } else {
                "Hold here"
            };
            if ui
                .add_enabled(total > 0, egui::Button::new(follow).small())
                .on_hover_text("Follow arrivals, or hold the workspace at this contribution")
                .clicked()
            {
                self.held = if self.held.is_some() { None } else { Some(end) };
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(design::caption("Workspace history", p.muted));
            stamp(ui, end);
            if !narrow {
                ui.add_space(12.);
                controls(ui);
            }
        });
        if narrow {
            ui.horizontal_wrapped(|ui| controls(ui));
        }
        end = self.held.unwrap_or(total);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 26.),
            egui::Sense::click_and_drag(),
        );
        response
            .widget_info(|| egui::WidgetInfo::slider(true, end as f64, "Workspace history cursor"));
        let strip = rect.shrink2(egui::vec2(5., 5.));
        let at_x = |x: f32| {
            let fraction = ((x - strip.left()) / strip.width().max(1.)).clamp(0., 1.);
            1 + (fraction * (total - 1) as f32).round() as usize
        };
        if response.clicked() || response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                end = at_x(pos.x);
                self.held = Some(end);
                response.request_focus();
            }
        }
        if response.has_focus() {
            ui.input_mut(|input| {
                for (key, dest) in [
                    (egui::Key::ArrowLeft, end.saturating_sub(1).max(1)),
                    (egui::Key::ArrowRight, (end + 1).min(total)),
                    (egui::Key::Home, 1),
                    (egui::Key::End, total),
                ] {
                    if input.consume_key(egui::Modifiers::NONE, key) {
                        end = dest;
                        self.held = Some(end);
                    }
                }
            });
        }
        let paint = ui.painter();
        paint.rect_filled(rect, 4, p.field);
        let bins = total.min((strip.width() / 5.).clamp(1., 180.) as usize);
        let step = strip.width() / bins as f32;
        let inks = [p.ink(TEAL), p.ink(PURPLE), p.muted];
        for bin in 0..bins {
            let lo = total * bin / bins;
            let hi = total * (bin + 1) / bins;
            let left = strip.left() + step * bin as f32;
            let mut bottom = strip.bottom();
            for (channel, ink) in history.channels.iter().zip(inks) {
                let count =
                    channel.partition_point(|i| *i < hi) - channel.partition_point(|i| *i < lo);
                if count == 0 {
                    continue;
                }
                let top = bottom - strip.height() * count as f32 / (hi - lo) as f32;
                let color = p
                    .field
                    .lerp_to_gamma(ink, if lo < end { 0.84 } else { 0.27 });
                paint.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(left, top),
                        egui::pos2(left + (step - 1.).max(1.), bottom),
                    ),
                    0,
                    color,
                );
                bottom = top;
            }
        }
        let fraction = if total == 1 {
            1.
        } else {
            (end - 1) as f32 / (total - 1) as f32
        };
        let x = strip.left() + fraction * strip.width();
        paint.vline(x, rect.y_range(), Stroke::new(2., p.text));
        paint.circle_filled(egui::pos2(x, rect.top() + 3.), 3., p.text);
        if response.has_focus() {
            paint.rect_stroke(rect, 4, Stroke::new(1., p.accent), egui::StrokeKind::Inside);
        }
        let hover = response.hover_pos().map(|pos| at_x(pos.x));
        response.on_hover_ui(|ui| {
            if let Some(point) = hover {
                let entry = history.entries[point - 1];
                let event = &app.events[entry.event];
                ui.label(format!("#{} · {}", event["event"], s(event, "created_utc")));
                ui.label(format!(
                    "{} {}",
                    history.name(entry.from),
                    app.event_heading(event)
                ));
            }
            ui.small("Click or drag to inspect this point in the workspace.");
        });
        ui.horizontal_wrapped(|ui| {
            for (label, ink) in ["Addressed", "Shared", "Individual work"]
                .into_iter()
                .zip(inks)
            {
                let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(8.), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 3., ink);
                ui.small(label);
                ui.add_space(3.);
            }
            if self.held.is_some() {
                ui.label(
                    RichText::new(format!("Held · {end} of {total}"))
                        .small()
                        .color(p.accent),
                );
            }
        });
        end
    }
}
