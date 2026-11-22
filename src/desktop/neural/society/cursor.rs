use super::*;

impl View {
    pub(super) fn time_cursor(&mut self, ui: &mut Ui, app: &App, history: &History) -> usize {
        let timeline = history.time;
        let total = timeline.entries.len();
        if total == 0 && self.held.is_none() {
            return 0;
        }
        let mut end = timeline.end(self.held);
        let p = theme::palette(ui);
        let inks = [p.ink(TEAL), p.ink(PURPLE), p.muted];
        let wide = ui.available_width() >= 920.;
        let narrow = ui.available_width() < 620.;
        let stamp = self
            .held
            .or_else(|| timeline.entries.last().map(|e| e.event));
        let held = self.held.is_some();
        let previous = |at: Option<usize>| {
            let at = at.or_else(|| timeline.entries.last().map(|e| e.event))?;
            let count = timeline.entries.partition_point(|e| e.event < at);
            (count > 0).then_some(count)
        };
        let mut controls = |ui: &mut Ui| {
            let earlier = previous(self.held);
            if ui
                .add_enabled(earlier.is_some(), egui::Button::new("Earlier").small())
                .clicked()
            {
                if let Some(n) = earlier {
                    self.held = Some(timeline.entries[n - 1].event);
                }
            }
            if ui
                .add_enabled(end < total, egui::Button::new("Later").small())
                .clicked()
            {
                self.held = Some(timeline.entries[end].event);
            }
            if ui
                .small_button(if held { "Follow latest" } else { "Hold here" })
                .clicked()
            {
                self.held = if held { None } else { stamp };
            }
        };
        let legend = |ui: &mut Ui| {
            for (label, ink) in ["Addressed", "Shared", "Individual work"]
                .into_iter()
                .zip(inks)
            {
                let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(7.), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 2.8, ink);
                ui.label(RichText::new(label).size(12.).color(p.muted));
            }
        };
        ui.horizontal_wrapped(|ui| {
            let label = if self.activity.is_some() {
                "Activity history"
            } else {
                "Workspace history"
            };
            ui.label(design::caption(label, p.muted));
            if let Some(i) = stamp {
                let e = &app.events[i];
                let at = if held { "Held at" } else { "Latest" };
                ui.label(
                    RichText::new(format!("{at} #{} · {}", e["event"], s(e, "created_utc")))
                        .size(12.)
                        .color(if held { p.accent } else { p.muted }),
                );
            }
            if !narrow {
                controls(ui);
            }
            if wide && total > 0 {
                ui.add_space(6.);
                legend(ui);
            }
        });
        if narrow {
            ui.horizontal_wrapped(|ui| controls(ui));
        }
        if total == 0 {
            return 0;
        }
        end = timeline.end(self.held);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 21.),
            egui::Sense::click_and_drag(),
        );
        response.widget_info(|| egui::WidgetInfo::slider(true, end as f64, "History cursor"));
        let strip = rect.shrink2(egui::vec2(4., 4.));
        let at_x = |x: f32| {
            let fraction = ((x - strip.left()) / strip.width().max(1.)).clamp(0., 1.);
            1 + (fraction * (total - 1) as f32).round() as usize
        };
        if response.clicked() || response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                end = at_x(pos.x);
                self.held = Some(timeline.entries[end - 1].event);
                response.request_focus();
            }
        }
        if response.has_focus() {
            ui.input_mut(|input| {
                for (key, dest) in [
                    (egui::Key::ArrowLeft, previous(self.held)),
                    (egui::Key::ArrowRight, (end < total).then_some(end + 1)),
                    (egui::Key::Home, Some(1)),
                    (egui::Key::End, Some(total)),
                ] {
                    if input.consume_key(egui::Modifiers::NONE, key) {
                        let Some(dest) = dest else {
                            continue;
                        };
                        end = dest;
                        self.held = Some(timeline.entries[dest - 1].event);
                    }
                }
            });
        }
        let paint = ui.painter();
        paint.rect_filled(rect, 4, p.field);
        let bins = total.min((strip.width() / 5.).clamp(1., 180.) as usize);
        let step = strip.width() / bins as f32;
        for bin in 0..bins {
            let lo = total * bin / bins;
            let hi = total * (bin + 1) / bins;
            let left = strip.left() + step * bin as f32;
            let mut bottom = strip.bottom();
            for (channel, ink) in timeline.channels.iter().zip(inks) {
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
        let fraction = if end == 0 {
            0.
        } else if total == 1 {
            1.
        } else {
            end.saturating_sub(1) as f32 / (total - 1) as f32
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
                let entry = timeline.entries[point - 1];
                let event = &app.events[entry.event];
                ui.label(format!("#{} · {}", event["event"], s(event, "created_utc")));
                ui.label(format!(
                    "{} {}",
                    history.name(entry.from),
                    app.event_heading(event)
                ));
            }
            ui.small("Click or drag to inspect this point in history.");
        });
        if !wide {
            ui.horizontal_wrapped(legend);
        }
        end
    }
}
