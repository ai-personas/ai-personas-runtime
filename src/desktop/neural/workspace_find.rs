use super::*;

mod index;
use index::{Entry, Index, Kind};

const SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::K);

#[derive(Default)]
pub(super) struct Finder {
    open: bool,
    focus_input: bool,
    previous_focus: Option<egui::Id>,
    query: String,
    kind: Option<Kind>,
    index: Index,
    hits: Vec<usize>,
    cursor: usize,
}

impl App {
    pub(super) fn find_hint(&self, ctx: &egui::Context) -> String {
        format!("Find in workspace ({})", ctx.format_shortcut(&SHORTCUT))
    }

    pub(super) fn find_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| input.consume_shortcut(&SHORTCUT)) {
            self.open_workspace_find(ctx);
        }
    }

    pub(super) fn open_workspace_find(&mut self, ctx: &egui::Context) {
        let finder = &mut self.workspace_find;
        if !finder.open {
            finder.previous_focus = ctx.memory(|m| m.focused());
            finder.query.clear();
            finder.kind = None;
            finder.cursor = 0;
        }
        finder.open = true;
        finder.focus_input = true;
    }

    pub(super) fn workspace_find_window(&mut self, ctx: &egui::Context) {
        if !self.workspace_find.open {
            return;
        }
        let mut finder = std::mem::take(&mut self.workspace_find);
        let chosen = finder.show(ctx, self);
        self.workspace_find = finder;
        let Some((kind, reference, work)) = chosen else {
            return;
        };
        self.compact_browser = false;
        match kind {
            Kind::Person => {
                self.show_person(&reference);
                self.profile_view = people::ProfileView::Overview;
            }
            Kind::Activity => self.open_home_activity(&reference),
            Kind::Delivery => self.open_home_delivery(&reference, &work),
            Kind::Request => {
                if let Some((id, _)) = reference
                    .strip_prefix("human_request:")
                    .and_then(|r| r.rsplit_once('@'))
                {
                    self.show_human_request(id);
                }
            }
            Kind::File => {
                if !work.is_empty() {
                    self.select_activity(&work);
                }
                self.open_resource(ctx, reference.clone());
                // A global result need not belong to the package visible behind Find.
                self.delivery_desk.set_return(&reference, None);
            }
        }
    }
}

impl Finder {
    fn show(&mut self, ctx: &egui::Context, app: &App) -> Option<(Kind, String, String)> {
        let field_id = egui::Id::new("workspace-find-query");
        let opening = std::mem::take(&mut self.focus_input);
        let (up, down, enter) = if ctx.memory(|m| m.has_focus(field_id)) {
            ctx.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                )
            })
        } else {
            (false, false, false)
        };
        let size = ctx.content_rect().size();
        let page_size = ((size.y - 248.) / 54.).floor().clamp(1., 7.) as usize;
        let mut chosen = None;
        let response = egui::Modal::new(egui::Id::new("workspace-find")).show(ctx, |ui| {
            ui.set_width((size.x - 56.).clamp(170., 610.));
            ui.spacing_mut().item_spacing.y = 6.;
            ui.horizontal(|ui| {
                ui.heading("Find in workspace");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Close").clicked() {
                        ui.close();
                    }
                });
            });
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(field_id)
                    .hint_text("Name, title, filename or reference")
                    .desired_width(f32::INFINITY),
            );
            if opening {
                field.request_focus();
            }
            let changed_kind = ui
                .horizontal_wrapped(|ui| {
                    let previous = self.kind;
                    ui.selectable_value(&mut self.kind, None, "All");
                    for kind in Kind::ALL {
                        ui.selectable_value(&mut self.kind, Some(kind), kind.filter());
                    }
                    self.kind != previous
                })
                .inner;
            if changed_kind {
                field.request_focus();
            }
            let changed = opening || field.changed() || changed_kind;
            let refreshed = self.index.generation != Some(app.home_index.generation);
            if changed || refreshed {
                let keep = (!changed).then(|| self.selected_key()).flatten();
                if refreshed {
                    self.index.refresh(app);
                }
                self.hits = self.index.search(&self.query, self.kind);
                self.cursor = keep
                    .and_then(|(kind, reference)| {
                        self.hits.iter().position(|i| {
                            let e = &self.index.entries[*i];
                            e.kind == kind && e.reference == reference
                        })
                    })
                    .unwrap_or(0);
            }
            if up {
                self.cursor = self.cursor.saturating_sub(1);
            }
            if down {
                self.cursor = (self.cursor + 1).min(self.hits.len().saturating_sub(1));
            }
            ui.separator();
            if self.hits.is_empty() {
                ui.weak(if app.snapshot.status.is_null() {
                    "Opening workspace…"
                } else if self.query.trim().is_empty() {
                    "No items in this view."
                } else {
                    "No matching items."
                });
                return;
            }
            let start = (self.cursor / page_size) * page_size;
            let end = (start + page_size).min(self.hits.len());
            for position in start..end {
                let entry = &self.index.entries[self.hits[position]];
                let active = position == self.cursor;
                if result_row(ui, entry, active).clicked() || (active && enter) {
                    chosen = Some((entry.kind, entry.reference.clone(), entry.work.clone()));
                }
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if self.hits.len() > page_size {
                    ui.small(format!("{}–{} of {}", start + 1, end, self.hits.len()));
                    if ui
                        .add_enabled(start > 0, egui::Button::new("Previous").small())
                        .clicked()
                    {
                        self.cursor = start.saturating_sub(page_size);
                        field.request_focus();
                    }
                    if ui
                        .add_enabled(end < self.hits.len(), egui::Button::new("Next").small())
                        .clicked()
                    {
                        self.cursor = end;
                        field.request_focus();
                    }
                } else {
                    ui.small(counted(self.hits.len() as u64, "result", "results"));
                }
                if ui.available_width() > 200. {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak("↑ ↓  Select     Enter  Open");
                    });
                }
            });
        });
        if chosen.is_some() || response.should_close() {
            self.open = false;
            ctx.memory_mut(|m| {
                m.surrender_focus(field_id);
                if let (None, Some(previous)) = (&chosen, self.previous_focus.take()) {
                    m.request_focus(previous);
                }
            });
        }
        chosen
    }

    fn selected_key(&self) -> Option<(Kind, String)> {
        let entry = self.index.entries.get(*self.hits.get(self.cursor)?)?;
        Some((entry.kind, entry.reference.clone()))
    }
}

fn result_row(ui: &mut Ui, entry: &Entry, selected: bool) -> egui::Response {
    let p = theme::palette(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 47.), egui::Sense::click());
    if selected || response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, 5, if selected { p.selection } else { p.hover });
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, 5, Stroke::new(1., p.accent), egui::StrokeKind::Inside);
    }
    let icon = egui::Rect::from_center_size(
        rect.left_center() + egui::vec2(16., 0.),
        egui::vec2(17., 17.),
    );
    theme::paint_icon(ui.painter(), icon, entry.kind.icon(), p.accent);
    let left = rect.left() + 33.;
    for (line, top, size, color) in [
        (&entry.title, 3., 16., p.text),
        (&entry.subtitle, 25., 13., p.muted),
    ] {
        let mut job = egui::text::LayoutJob::simple_singleline(
            line.clone(),
            FontId::proportional(size),
            color,
        );
        job.wrap.max_width = (rect.right() - left - 7.).max(1.);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        ui.painter()
            .galley(egui::pos2(left, rect.top() + top), galley, color);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            format!("{} · {}", entry.title, entry.subtitle),
        )
    });
    response.on_hover_ui(|ui| {
        ui.label(&entry.title);
        ui.small(&entry.subtitle);
        ui.small(&entry.reference);
    })
}
