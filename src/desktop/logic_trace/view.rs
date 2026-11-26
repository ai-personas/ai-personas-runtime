use super::{
    super::{design, document, source_page, theme},
    Trace,
};
use eframe::egui::{self, RichText, Ui};

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Assignments,
    Signals,
    Source,
}
#[derive(Default)]
pub(in super::super) struct View {
    tab: Tab,
    cursor: usize,
    reference: Option<usize>,
    page: usize,
}
fn short(v: &str) -> String {
    // Only called with ASCII bit strings (or “not recorded”).
    if v.len() > 20 {
        format!("{}…{}", &v[..8], &v[v.len() - 8..])
    } else {
        v.into()
    }
}
impl View {
    pub(in super::super) fn show(&mut self, ui: &mut Ui, trace: &Trace, name: &str) {
        // When the file window is short, let the reader scroll past the
        // cursor controls too. Bound the inner lists in that layout.
        if ui.available_height() < 320. {
            egui::ScrollArea::vertical()
                .id_salt("vcd-short")
                .show(ui, |ui| {
                    self.body(ui, trace, name);
                });
        } else {
            self.body(ui, trace, name);
        }
    }
    fn body(&mut self, ui: &mut Ui, trace: &Trace, name: &str) {
        let p = theme::palette(ui);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Assignments, "Timing register");
            ui.selectable_value(&mut self.tab, Tab::Signals, "Signals at cursor");
            ui.selectable_value(&mut self.tab, Tab::Source, "Source");
        });
        if self.tab == Tab::Source {
            if ui.small_button("Copy source").clicked() {
                ui.ctx().copy_text(trace.source.clone());
            }
            let text = if trace.source.len() > 64 * 1024 {
                source_page(ui, &trace.source, &mut self.page)
            } else {
                &trace.source
            };
            egui::ScrollArea::both()
                .id_salt(("vcd-source", self.page))
                .show(ui, |ui| document::code(ui, text, "vcd"));
            return;
        }
        let total = trace.records.len();
        let clock = if trace.scale.is_some() {
            format!("1 tick = {}", trace.time(1))
        } else {
            "Timescale not declared".into()
        };
        ui.small(format!(
            "{} identifiers · {} declarations · {total} assignments · {clock}",
            trace.signals.len(),
            trace.declarations.len()
        ));
        if total == 0 {
            ui.label(
                "No values recorded. The declarations below do not establish an initial state.",
            );
            self.signals(ui, trace);
            return;
        }
        let initial = self.cursor;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.cursor > 0, egui::Button::new("Previous"))
                .clicked()
            {
                self.cursor -= 1;
            }
            ui.label("Record");
            let mut n = self.cursor + 1;
            if ui
                .add(egui::DragValue::new(&mut n).range(1..=total))
                .changed()
            {
                self.cursor = n - 1;
            }
            ui.weak(format!("of {total}"));
            if ui
                .add_enabled(self.cursor + 1 < total, egui::Button::new("Next"))
                .clicked()
            {
                self.cursor += 1;
            }
            if ui
                .button("Set reference")
                .on_hover_text("Pin this record’s time, then select another record to compare.")
                .clicked()
            {
                self.reference = Some(self.cursor);
            }
            if self.reference.is_some() && ui.small_button("Clear reference").clicked() {
                self.reference = None;
            }
        });
        let r = &trace.records[self.cursor];
        let sig = &trace.signals[r.signal];
        egui::Frame::new().fill(p.panel).stroke(egui::Stroke::new(1., p.border)).inner_margin(10).corner_radius(7).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(design::caption("Cursor",p.muted));
                ui.label(design::display(&trace.time(r.tick.into()), 25.));
                ui.label(RichText::new(format!("#{}", self.cursor+1)).color(p.muted));
            });
            let value = format!("{}  ·  {} → {}", trace.declarations[sig.primary].path, trace.state(r.signal, self.cursor), trace.value(self.cursor));
            ui.add(egui::Label::new(RichText::new(&value).monospace()).truncate()).on_hover_text(value);
            if let Some(a) = self.reference {
                ui.horizontal_wrapped(|ui| {
                    ui.label(design::caption("Cursor − reference", p.muted));
                    ui.colored_label(p.accent, RichText::new(trace.interval(a, self.cursor)).strong());
                    let first = &trace.records[a];
                    let path = &trace.declarations[trace.signals[first.signal].primary].path;
                    ui.add(egui::Label::new(RichText::new(format!("from #{} at {} · {path}", a+1, trace.time(first.tick.into()))).weak()).truncate()).on_hover_text(trace.assignment(a));
                });
            }
            if ui.small_button("Copy timing").on_hover_text("Copy file name, SHA-256, time basis, selected assignment and reference interval.").clicked() { ui.ctx().copy_text(trace.report(name, self.cursor, self.reference)); }
        });
        if self.tab == Tab::Signals {
            ui.small(format!(
                "After record #{} in file order. Names with the same ID share recorded values.",
                self.cursor + 1
            ));
            self.signals(ui, trace);
        } else {
            ui.small(format!(
                "Assignments in file order · last timestamp {}",
                trace.time(trace.last_tick.into())
            ));
            let height = ui.spacing().interact_size.y;
            let mut area = egui::ScrollArea::vertical()
                .id_salt("vcd-rows")
                .max_height(ui.available_height().clamp(150., 1600.))
                .auto_shrink([false, false]);
            if self.cursor != initial {
                area = area.vertical_scroll_offset(
                    self.cursor.saturating_sub(2) as f32 * (height + ui.spacing().item_spacing.y),
                );
            }
            area.show_rows(ui, height, total, |ui, rows| {
                for i in rows {
                    let r = &trace.records[i];
                    let sig = &trace.signals[r.signal];
                    let mark = if self.reference == Some(i) { "R" } else { " " };
                    let label = format!(
                        "{mark} #{:<5} {:<14} {}",
                        i + 1,
                        trace.time(r.tick.into()),
                        trace.declarations[sig.primary].path
                    );
                    let button = egui::Button::selectable(
                        self.cursor == i,
                        RichText::new(label).monospace(),
                    )
                    .right_text(short(&trace.value(i)))
                    .truncate();
                    if ui
                        .add_sized([(ui.available_width() - 10.).max(1.), height], button)
                        .on_hover_text(trace.assignment(i))
                        .clicked()
                    {
                        self.cursor = i;
                    }
                }
            });
        }
    }
    fn signals(&self, ui: &mut Ui, trace: &Trace) {
        let height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt("vcd-signals")
            .max_height(ui.available_height().clamp(150., 1600.))
            .show_rows(ui, height, trace.declarations.len(), |ui, rows| {
                for i in rows {
                    let d = &trace.declarations[i];
                    let sig = &trace.signals[d.signal];
                    let value = trace.state(d.signal, self.cursor + 1);
                    let label = format!("{}  ·  [{}]  {}", short(&value), sig.code, d.path);
                    let details = format!(
                        "{} · {} bits · identifier {}\n{} = {value}",
                        d.kind, sig.width, sig.code, d.path
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(egui::Label::new(RichText::new(label).monospace()).truncate())
                                .on_hover_text(details);
                        },
                    );
                }
            });
    }
}
