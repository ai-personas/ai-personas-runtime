use super::*;

mod event;
mod index;
use index::{Index, Kind};

const SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::K);
const QUERY: &str = "workspace-find-query";
const OVERLAY: &str = "workspace-find";

#[derive(Default)]
pub(super) struct Finder {
    pub(super) open: bool,
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
        // egui keeps a modal's focus boundary until the end of its last frame.
        // Restore the prior editor when it can accept keyboard events again.
        if !self.workspace_find.open {
            if let Some(previous) = self.workspace_find.previous_focus {
                if ctx.memory(|m| {
                    m.top_modal_layer()
                        .is_some_and(|l| l.id == egui::Id::new(OVERLAY))
                }) {
                    ctx.request_repaint();
                } else {
                    ctx.memory_mut(|m| m.request_focus(previous));
                    self.workspace_find.previous_focus = None;
                }
            }
        }
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
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(QUERY)));
    }

    pub(super) fn find_activities(&mut self, ctx: &egui::Context) {
        self.open_workspace_find(ctx);
        self.workspace_find.kind = Some(Kind::Activity);
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
            Kind::Event => event::open(self, &reference, &work),
            Kind::TaskRole => self.open_task_board(&work),
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
        let field_id = egui::Id::new(QUERY);
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
        let tight = size.x < 320. || size.y < 295.;
        let margin = if tight { 5 } else { 13 };
        let row_height = if size.x < 270. { 100. } else { 54. };
        let page_size = ((size.y - 248.) / row_height).floor().clamp(1., 7.) as usize;
        let mut chosen = None;
        let id = egui::Id::new(OVERLAY);
        let area = egui::Modal::default_area(id).anchor(
            egui::Align2::CENTER_TOP,
            egui::vec2(
                0.,
                if tight {
                    4.
                } else {
                    (size.y * 0.11).clamp(6., 96.)
                },
            ),
        );
        let frame = egui::Frame::popup(&ctx.style())
            .corner_radius(9)
            .inner_margin(margin);
        let response = egui::Modal::new(id)
            .area(area)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width((size.x - 2. * f32::from(margin) - 12.).clamp(1., 610.));
                ui.spacing_mut().item_spacing.y = if tight { 3. } else { 6. };
                ui.horizontal(|ui| {
                    if tight {
                        ui.strong("Find");
                    } else {
                        ui.heading("Find in workspace");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Close").clicked() {
                            ui.close();
                        }
                    });
                });
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id(field_id)
                        .hint_text("Name, filename, reference or #event number")
                        .desired_width(f32::INFINITY),
                );
                if opening {
                    field.request_focus();
                }
                // Keep Close and the query reachable even when zoom leaves
                // less room than one result row. The body may scroll instead
                // of growing the modal beyond the client viewport.
                let body_height =
                    (ctx.content_rect().bottom() - ui.cursor().top() - f32::from(margin) - 8.)
                        .max(1.);
                let scroll = egui::ScrollArea::vertical()
                    .id_salt((OVERLAY, "results"))
                    .auto_shrink([false, true])
                    // A short previous query is not a cap on the next one's results.
                    .min_scrolled_height(body_height)
                    .max_height(body_height);
                let scroll = if opening || up || down || field.changed() {
                    scroll.vertical_scroll_offset(0.)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    let changed_kind = self.kind_filters(ui);
                    if changed_kind {
                        field.request_focus();
                    }
                    if self.kind.is_none() {
                        if let Some(address) = event::Address::parse(&self.query) {
                            chosen = address.result(ui, app, enter);
                            return;
                        }
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
                        ui.weak(if app.snapshot.status.is_null() && app.error.is_some() {
                            "Workspace data is unavailable."
                        } else if app.snapshot.status.is_null() {
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
                        if result_row(
                            ui,
                            entry.kind,
                            &entry.reference,
                            &entry.title,
                            &entry.subtitle,
                            active,
                        )
                        .clicked()
                            || (active && enter)
                        {
                            chosen =
                                Some((entry.kind, entry.reference.clone(), entry.work.clone()));
                        }
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if self.hits.len() > page_size {
                            ui.small(format!("{}–{} of {}", start + 1, end, self.hits.len()));
                            if ui
                                .add_enabled(
                                    start > 0,
                                    egui::Button::new(if tight { "Prev" } else { "Previous" })
                                        .small(),
                                )
                                .clicked()
                            {
                                self.cursor = start.saturating_sub(page_size);
                                field.request_focus();
                            }
                            if ui
                                .add_enabled(
                                    end < self.hits.len(),
                                    egui::Button::new("Next").small(),
                                )
                                .clicked()
                            {
                                self.cursor = end;
                                field.request_focus();
                            }
                        } else {
                            ui.small(counted(self.hits.len() as u64, "result", "results"));
                        }
                    });
                    if !tight {
                        ui.weak("↑ ↓  Select     Enter  Open");
                    }
                });
            });
        if chosen.is_some() || response.should_close() {
            self.open = false;
            ctx.memory_mut(|m| m.surrender_focus(field_id));
            if chosen.is_some() {
                self.previous_focus = None;
            }
            ctx.request_repaint();
        }
        chosen
    }

    fn selected_key(&self) -> Option<(Kind, String)> {
        let entry = self.index.entries.get(*self.hits.get(self.cursor)?)?;
        Some((entry.kind, entry.reference.clone()))
    }

    fn kind_filters(&mut self, ui: &mut Ui) -> bool {
        let previous = self.kind;
        let mut choices = |ui: &mut Ui| {
            ui.selectable_value(&mut self.kind, None, "All");
            for kind in Kind::FILTERS {
                ui.selectable_value(&mut self.kind, Some(kind), kind.filter());
            }
        };
        if ui.available_width() < 350. {
            egui::ComboBox::from_id_salt("find-type")
                .selected_text(previous.map_or("All types", Kind::filter))
                .width(ui.available_width().min(160.))
                .show_ui(ui, |ui| {
                    choices(ui);
                });
        } else {
            ui.horizontal_wrapped(choices);
        }
        self.kind != previous
    }
}

fn result_row(
    ui: &mut Ui,
    kind: Kind,
    reference: &str,
    heading: &str,
    detail: &str,
    selected: bool,
) -> egui::Response {
    let p = theme::palette(ui);
    let width = ui.available_width();
    let narrow = width < 235.;
    let inset = if narrow { 5. } else { 33. };
    let line = |text: &str, font_size, color, rows| {
        let mut job = egui::text::LayoutJob::simple_singleline(
            text.to_owned(),
            FontId::proportional(font_size),
            color,
        );
        job.wrap.max_width = (width - inset - 5.).max(1.);
        job.wrap.max_rows = rows;
        job.wrap.break_anywhere = rows == 1;
        ui.painter().layout_job(job)
    };
    let title = line(heading, 16., p.text, if narrow { 3 } else { 1 });
    let subtitle = line(detail, 13., p.muted, if narrow { 2 } else { 1 });
    let clipped = title.elided || subtitle.elided;
    let height = (title.size().y + subtitle.size().y + 10.).max(47.);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if selected || response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, 5, if selected { p.selection } else { p.hover });
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, 5, Stroke::new(1., p.accent), egui::StrokeKind::Inside);
    }
    if !narrow {
        let icon = egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(16., 0.),
            egui::vec2(17., 17.),
        );
        theme::paint_icon(ui.painter(), icon, kind.icon(), p.accent);
    }
    let top = rect.left_top() + egui::vec2(inset, 4.);
    let second = top + egui::vec2(0., title.size().y + 2.);
    ui.painter().galley(top, title, p.text);
    ui.painter().galley(second, subtitle, p.muted);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            format!("{heading} · {detail}"),
        )
    });
    response.context_menu(|ui| {
        ui.small(reference);
        if ui.button("Copy reference").clicked() {
            ui.ctx().copy_text(reference.to_owned());
            ui.close();
        }
    });
    if clipped {
        response.on_hover_ui(|ui| {
            let viewport = ui.ctx().content_rect();
            let width = (viewport.width() - 24.).clamp(1., 460.);
            ui.set_max_width(width);
            // Hover popups close on wheel input. Offer an honest bounded
            // preview rather than a scrollbar that cannot remain usable.
            let preview = |text: &str, font_size, color, rows| {
                let mut job = egui::text::LayoutJob::simple(
                    text.to_owned(),
                    FontId::proportional(font_size),
                    color,
                    width,
                );
                job.wrap.max_rows = rows;
                job
            };
            let rows = ((viewport.height() * 0.35) / 20.).floor().clamp(1., 6.) as usize;
            ui.add(
                egui::Label::new(preview(heading, 16., ui.visuals().text_color(), rows))
                    .wrap()
                    .show_tooltip_when_elided(false),
            );
            ui.add(
                egui::Label::new(preview(detail, 13., ui.visuals().weak_text_color(), 2))
                    .wrap()
                    .show_tooltip_when_elided(false),
            );
            ui.weak("Open result for full details");
        })
    } else {
        response
    }
}
