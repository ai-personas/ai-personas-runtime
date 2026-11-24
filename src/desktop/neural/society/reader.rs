use super::*;

const ROWS: usize = 4;

impl RecordKind {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All contributions",
            Self::Addressed => "Addressed",
            Self::Shared => "Shared",
            Self::Individual => "Individual work",
        }
    }

    fn indices<'a>(self, h: &'a History, end: usize) -> Option<&'a [usize]> {
        let lane = match self {
            Self::All => return None,
            Self::Addressed => 0,
            Self::Shared => 1,
            Self::Individual => 2,
        };
        Some(through(&h.time.channels[lane], end))
    }
}

impl View {
    pub(super) fn shows_route(&self, h: &History, from: usize, to: usize) -> bool {
        let direct =
            self.scope.from.is_none_or(|n| n == from) && self.scope.to.is_none_or(|n| n == to);
        if !h.is_pair(self.scope) {
            return direct;
        }
        let reverse = self.scope.from == Some(to) && self.scope.to == Some(from);
        match self.direction {
            Direction::Both => direct || reverse,
            Direction::Forward => direct,
            Direction::Reverse => reverse,
        }
    }

    fn selected_records<'a>(&self, h: &'a History, end: usize) -> Option<&'a [usize]> {
        match (self.scope.from, self.scope.to) {
            (None, None) => self.kind.indices(h, end),
            (Some(a), Some(b)) if h.is_pair(self.scope) => match self.direction {
                Direction::Both => Some(h.time.between(a, b, end)),
                Direction::Forward => h.time.matches(self.scope, end),
                Direction::Reverse => h.time.matches(
                    Scope {
                        from: Some(b),
                        to: Some(a),
                    },
                    end,
                ),
            },
            _ => h.time.matches(self.scope, end),
        }
    }

    fn reader_heading(&mut self, ui: &mut Ui, app: &App, h: &History, action: &mut Option<Action>) {
        let heading = match (self.scope.from, self.scope.to) {
            (Some(a), Some(b)) if h.is_pair(self.scope) => {
                format!("{} & {}", h.name(a), h.name(b))
            }
            (Some(a), Some(b)) => format!("{} → {}", h.name(a), h.name(b)),
            (Some(a), None) => format!("From {}", h.name(a)),
            (None, Some(b)) => format!("To {}", h.name(b)),
            _ if self.activity.is_some() => "Activity record".into(),
            _ => "Society record".into(),
        };
        ui.label(design::display(heading, 28.));
        if self.scope != Scope::default() || self.activity.is_some() {
            ui.horizontal_wrapped(|ui| {
                if self.scope != Scope::default() && ui.small_button("Clear selection").clicked() {
                    self.select(Scope::default());
                }
                if let Some(work) = &self.activity {
                    if ui
                        .small_button("Open activity")
                        .on_hover_text(work_title(app, work))
                        .clicked()
                    {
                        *action = Some(Action::Activity(work.clone()));
                    }
                }
                for (i, node) in [self.scope.from, self.scope.to].into_iter().enumerate() {
                    if i == 1 && node == self.scope.from {
                        continue;
                    }
                    if let Some(Identity::Persona(id)) = node.map(|n| &h.nodes[n]) {
                        if app
                            .records("personas")
                            .iter()
                            .any(|r| r["body"]["id"] == id.as_str())
                            && ui
                                .small_button(format!("{}’s profile", app.label(id)))
                                .clicked()
                        {
                            *action = Some(Action::Profile(id.clone()));
                        }
                    }
                }
            });
        }
        if self.held.is_none() && self.scope.to.is_none() {
            if let Some(Identity::Persona(id)) = self.scope.from.map(|n| &h.nodes[n]) {
                let work = current_work(app, id, self.activity.as_deref());
                if let Some((first, more)) = work.split_first() {
                    participation(ui, app, first, action);
                    if !more.is_empty() {
                        egui::CollapsingHeader::new(format!("More participation ({})", more.len()))
                            .id_salt(("other-activities", id, &self.activity))
                            .show(ui, |ui| {
                                for line in more {
                                    participation(ui, app, line, action);
                                }
                            });
                    }
                }
            }
        }
    }
    fn record_controls(&mut self, ui: &mut Ui, h: &History, end: usize) {
        let p = theme::palette(ui);
        let previous = self.kind;
        ui.horizontal_wrapped(|ui| {
            if self.scope == Scope::default() {
                egui::ComboBox::from_id_salt("society-record-kind")
                    .selected_text(self.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in [
                            RecordKind::All,
                            RecordKind::Addressed,
                            RecordKind::Shared,
                            RecordKind::Individual,
                        ] {
                            let total = kind.indices(h, end).map_or(end, <[usize]>::len);
                            ui.selectable_value(
                                &mut self.kind,
                                kind,
                                format!("{} · {total}", kind.label()),
                            );
                        }
                    });
            }
            if previous != self.kind {
                self.restart_reader();
            }
            let count = self.selected_records(h, end).map_or(end, <[usize]>::len);
            let last_page = count.saturating_sub(1) / ROWS;
            self.record_page = self.record_page.min(last_page);
            let start = self.record_page * ROWS;
            if count > ROWS {
                ui.label(
                    RichText::new(format!(
                        "{}–{} of {count}",
                        start + 1,
                        (start + ROWS).min(count)
                    ))
                    .size(12.)
                    .color(p.muted),
                );
                let page = self.record_page;
                if ui
                    .add_enabled(page > 0, egui::Button::new("Newer").small())
                    .clicked()
                {
                    self.record_page -= 1;
                }
                if ui
                    .add_enabled(page < last_page, egui::Button::new("Older").small())
                    .clicked()
                {
                    self.record_page += 1;
                    if self.held.is_none() {
                        self.held = Some(h.time.entries[end - 1].event);
                    }
                }
                if page != self.record_page {
                    self.reset_record_scroll = true;
                    self.expanded = None;
                }
            } else {
                ui.small(counted(count as u64, "contribution", "contributions"));
            }
        });
    }

    fn direction_picker(&mut self, ui: &mut Ui, h: &History, a: usize, b: usize, end: usize) {
        let p = theme::palette(ui);
        let both = h.time.between(a, b, end).len();
        let from_a = h.time.matches(self.scope, end).unwrap_or_default().len();
        let gap = 4.;
        let all_width = 58.;
        let person_width = (ui.available_width() - all_width - 2. * gap) * 0.5;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (direction, sender, count) in [
                (Direction::Both, None, both),
                (Direction::Forward, Some(a), from_a),
                (Direction::Reverse, Some(b), both - from_a),
            ] {
                let name = sender.map_or("Both", |n| h.name(n));
                let description = sender.map_or_else(
                    || format!("{both} contributions in both directions"),
                    |n| {
                        format!(
                            "{} → {} · {count} contributions",
                            h.name(n),
                            h.name(if n == a { b } else { a })
                        )
                    },
                );
                let detail = if sender.is_some() {
                    format!("{count} sent")
                } else {
                    format!("{count} total")
                };
                let width = if sender.is_some() {
                    person_width.max(24.)
                } else {
                    all_width
                };
                let (r, response) =
                    ui.allocate_exact_size(egui::vec2(width, 41.), egui::Sense::click());
                let selected = direction == self.direction;
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        selected,
                        &description,
                    )
                });
                let ink = sender.map_or(p.accent, |n| h.nodes[n].color(ui));
                let fill = if selected {
                    p.field.lerp_to_gamma(ink, 0.15)
                } else if response.hovered() {
                    p.hover
                } else {
                    p.field
                };
                let edge = if response.has_focus() || selected {
                    ink
                } else {
                    p.border
                };
                ui.painter()
                    .rect(r, 5, fill, Stroke::new(1., edge), egui::StrokeKind::Inside);
                let mut x = r.left() + 7.;
                if let Some(n) = sender.filter(|_| width >= 96.) {
                    map::mark(
                        ui,
                        egui::Rect::from_center_size(
                            egui::pos2(x + 11., r.center().y),
                            egui::Vec2::splat(23.),
                        ),
                        h, n,
                    );
                    x += 29.;
                }
                let room = r.right() - x - 3.;
                map::text_line(
                    ui,
                    egui::pos2(x, r.center().y - 16.),
                    room,
                    RichText::new(name).strong().size(14.),
                    p.text,
                );
                map::text_line(
                    ui,
                    egui::pos2(x, r.center().y + 1.),
                    room,
                    RichText::new(detail).size(12.),
                    p.muted,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(description)
                    .clicked()
                    && !selected
                {
                    self.direction = direction;
                    self.restart_reader();
                }
            }
        });
    }

    pub(super) fn reader(
        &mut self,
        ui: &mut Ui,
        app: &App,
        h: &History,
        end: usize,
        split: bool,
        action: &mut Option<Action>,
    ) {
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.reader_heading(ui, app, h, action);
            if let (Some(a), Some(b)) = (self.scope.from, self.scope.to) {
                if h.is_pair(self.scope) {
                    self.direction_picker(ui, h, a, b, end);
                }
            }
            self.record_controls(ui, h, end);
            if split {
                let height =
                    (ui.clip_rect().bottom() - ui.next_widget_position().y - 14.).max(145.);
                let mut area = egui::ScrollArea::vertical()
                    .id_salt("society-reader-events")
                    .max_height(height)
                    .auto_shrink([false, true]);
                if std::mem::take(&mut self.reset_record_scroll) {
                    area = area.vertical_scroll_offset(0.);
                }
                area.show(ui, |ui| self.entries(ui, app, h, end, action));
            } else {
                self.entries(ui, app, h, end, action);
            }
        });
    }

    fn entries(
        &mut self,
        ui: &mut Ui,
        app: &App,
        h: &History,
        end: usize,
        action: &mut Option<Action>,
    ) {
        let p = theme::palette(ui);
        let mut focus = None;
        let indices = self.selected_records(h, end);
        let count = indices.map_or(end, <[usize]>::len);
        if count == 0 {
            ui.add_space(8.);
            ui.label(if self.held.is_some() {
                "No contributions for this selection by this point in history."
            } else if self.scope != Scope::default() || self.kind != RecordKind::All {
                "No contributions match this selection."
            } else {
                "Messages, shared posts and individual work appear here as people contribute."
            });
            return;
        }
        let mut day = "";
        for offset in (self.record_page * ROWS..count).take(ROWS) {
            let pos = count - 1 - offset;
            let mark = h.time.entries[indices.map_or(pos, |rows| rows[pos])];
            let event = &app.events[mark.event];
            let stamp = s(event, "created_utc");
            let date = stamp.get(..10).unwrap_or(stamp);
            ui.separator();
            if date != day {
                ui.label(design::caption(&format!("{date} · UTC"), p.muted));
                day = date;
            }
            ui.push_id(("society-contribution", mark.event), |ui| {
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(26.), egui::Sense::hover());
                    map::mark(ui, r, h, mark.from);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(h.name(mark.from))
                                .strong()
                                .color(h.nodes[mark.from].color(ui)),
                        );
                        if let Some(to) = mark.to {
                            ui.weak("→");
                            ui.strong(h.name(to));
                        }
                    });
                });
                let time = time_label(event["created"].as_i64().unwrap_or(0));
                ui.small(format!(
                    "{} · {}",
                    record_label(app, event),
                    time.trim_end_matches(" UTC")
                ));
                let text = event_detail(event);
                let limit = if ui.available_width() < 480. {
                    150
                } else {
                    240
                };
                let long = text.chars().nth(limit).is_some() || text.lines().nth(1).is_some();
                let expanded = self.expanded == Some(mark.event);
                if !text.is_empty() {
                    if expanded {
                        document::message_markdown(ui, text);
                    } else {
                        ui.label(reading_preview(text, limit));
                    }
                    if let Some(r) = app.message_editions(ui, text) {
                        *action = Some(if r.starts_with("delivery:") { Action::Package(r) } else { Action::File(r) });
                    }
                }
                if self.activity.is_none() {
                    let work = s(event, "work");
                    let title = work_title(app, work);
                    if ui
                        .scope(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            ui.link(
                                RichText::new(format!("Activity · {}", short_text(title, 100)))
                                    .size(12.)
                                    .color(p.muted),
                            )
                        })
                        .inner
                        .on_hover_text(format!("Show this activity\n{title}"))
                        .clicked()
                    {
                        focus = Some(work.to_owned());
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .small_button(format!("Open #{}", event["event"]))
                        .on_hover_text("Open the exact event in the activity timeline")
                        .clicked()
                    {
                        *action = Some(Action::Event(mark.event));
                    }
                    if long
                        && ui
                            .small_button(if expanded { "Collapse" } else { "Read full" })
                            .clicked()
                    {
                        self.expanded = (!expanded).then_some(mark.event);
                    }
                    let body = &event["body"];
                    if let Some(parent) = body["reply_to"]
                        .as_str()
                        .and_then(|r| home_navigation::event_position(&app.events, r))
                        .filter(|i| *i < mark.event)
                    {
                        if ui
                            .small_button(format!("In reply to #{}", app.events[parent]["event"]))
                            .clicked()
                        {
                            *action = Some(Action::Event(parent));
                        }
                    }
                    if let Some(package) = body["delivery"]
                        .as_str()
                        .or(body["reference"].as_str())
                        .filter(|r| r.starts_with("delivery:"))
                    {
                        if ui.small_button("Open package").clicked() {
                            *action = Some(Action::Package(package.into()));
                        }
                    }
                });
                referenced_files(ui, app, event, action);
            });
        }
        if let Some(work) = focus {
            self.focus_activity(Some(work));
        }
    }
}

fn referenced_files(ui: &mut Ui, app: &App, event: &Value, action: &mut Option<Action>) {
    let body = &event["body"];
    let mut seen = BTreeSet::new();
    let files: Vec<_> = event["tool_outcome"]["file_reference"]
        .as_str()
        .into_iter()
        .chain(body["asset"].as_str())
        .chain(
            body["references"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .filter(|r| r.starts_with("asset:") && seen.insert(*r))
        .collect();
    let mut link = |ui: &mut Ui, reference: &str| {
        let key = egui::Id::new((
            "society-file-title",
            &app.root,
            reference,
            app.home_index.latest_asset_receipt,
        ));
        let name = ui.ctx().data_mut(|cache| {
            cache
                .get_temp_mut_or_insert_with(key, || {
                    app.snapshot
                        .assets
                        .iter()
                        .find(|a| s(a, "reference") == reference)
                        .and_then(|a| a["body"]["path"].as_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or("Referenced file")
                        .to_owned()
                })
                .clone()
        });
        let label = reference
            .rsplit_once('@')
            .map_or_else(|| name.clone(), |(_, n)| format!("{name} · v{n}"));
        ui.horizontal(|ui| {
            theme::glyph(ui, Icon::Files, theme::palette(ui).accent);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            if ui
                .link(RichText::new(label).size(13.))
                .on_hover_text(format!("Open {name}\n{reference}"))
                .clicked()
            {
                *action = Some(Action::File(reference.into()));
            }
        });
    };
    match files.as_slice() {
        [] => {}
        [only] => link(ui, only),
        many => {
            egui::CollapsingHeader::new(format!("Referenced files ({})", many.len())).show(
                ui,
                |ui| {
                    for reference in many {
                        link(ui, reference);
                    }
                },
            );
        }
    }
}

fn reading_preview(text: &str, limit: usize) -> String {
    // Keep the line breaks the Markdown parser uses for lists, tables and paragraphs.
    let end = text
        .char_indices()
        .nth(limit * 5)
        .map_or(text.len(), |(i, _)| i);
    let sample = &text[..end];
    let prose = sample
        .split_once("\n\n")
        .and_then(|(intro, body)| {
            let block = body.trim_start();
            (!intro.trim().is_empty() && (block.starts_with('|') || block.starts_with("```")))
                .then_some(intro)
        })
        .unwrap_or(sample);
    document::excerpt(prose, limit)
}

fn participation(ui: &mut Ui, app: &App, line: &WorkLine, action: &mut Option<Action>) {
    ui.strong(&line.state);
    let title = work_title(app, &line.work);
    if ui
        .link(short_text(title, 100))
        .on_hover_text(title)
        .clicked()
    {
        *action = Some(Action::Activity(line.work.clone()));
    }
    if !line.note.is_empty() {
        ui.label(&line.note);
    }
}
