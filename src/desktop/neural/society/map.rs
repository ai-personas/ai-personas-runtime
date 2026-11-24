use super::*;
use egui::{pos2, vec2, Pos2, Rect, Sense};

const PAGE: usize = 6;

struct Sender {
    node: usize,
    state: String,
    work: String,
    note: String,
    detail: String,
    live_worker: bool,
}

impl Sender {
    fn read(
        app: &App,
        history: &History,
        node: usize,
        end: usize,
        live: bool,
        activity: Option<&str>,
    ) -> Self {
        let identity = &history.nodes[node];
        let last = history
            .time
            .last_by(node, end)
            .map(|entry| &app.events[entry.event]);
        let mut row = Self {
            node,
            state: String::new(),
            work: String::new(),
            note: String::new(),
            detail: String::new(),
            live_worker: false,
        };
        if live {
            if let Identity::Persona(id) = identity {
                let lines = current_work(app, id, activity);
                if let Some(first) = lines.first() {
                    row.live_worker = app.runner_observation.has_worker(&first.work, id);
                    row.state = first.state.clone();
                    row.note = short_text(&first.note, 140);
                    row.work = work_title(app, &first.work).into();
                    let extra = lines
                        .iter()
                        .map(|line| &line.work)
                        .collect::<BTreeSet<_>>()
                        .len()
                        .saturating_sub(1);
                    if extra > 0 {
                        row.state.push_str(&format!(
                            " · +{}",
                            counted(extra as u64, "activity", "activities")
                        ));
                    }
                    row.detail = lines
                        .iter()
                        .take(3)
                        .map(|line| {
                            let note = if line.note.is_empty() {
                                String::new()
                            } else {
                                format!(" · {}", short_text(&line.note, 160))
                            };
                            format!(
                                "{} · {}{note}",
                                line.state,
                                short_text(work_title(app, &line.work), 100)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if lines.len() > 3 {
                        row.detail
                            .push_str("\nSelect this sender for all participation records.");
                    }
                }
            }
        }
        if let Some(event) = last {
            let action = app.event_heading(event);
            if row.state.is_empty() {
                row.state = format!("Last · {action}");
                row.work = work_title(app, s(event, "work")).into();
                row.note = document::excerpt(&short_text(event_detail(event), 360), 140);
            }
            if !row.detail.is_empty() {
                row.detail.push('\n');
            }
            if activity.is_some() && row.note.is_empty() {
                row.note = format!("Last · {action}");
            }
            row.detail.push_str(&format!(
                "#{} · {action} · {}",
                event["event"],
                short_text(event_detail(event), 180)
            ));
        } else if row.state.is_empty() {
            row.state = if live {
                "No recorded work yet"
            } else {
                "No action by this point"
            }
            .into();
        }
        row
    }
}

impl View {
    pub(super) fn map(
        &mut self,
        ui: &mut Ui,
        app: &App,
        history: &History,
        end: usize,
        split: bool,
    ) -> bool {
        let mut authors = Vec::new();
        let mut targets = Vec::new();
        let mut shared = None;
        let activity = self
            .activity
            .as_deref()
            .and_then(|id| app.home_index.activity(id));
        for (node, identity) in history.nodes.iter().enumerate() {
            let sent = history
                .time
                .sent
                .get(&node)
                .is_some_and(|v| v.first().is_some_and(|i| *i < end));
            let received = history
                .time
                .received
                .get(&node)
                .is_some_and(|v| v.first().is_some_and(|i| *i < end));
            if matches!(identity, Identity::Persona(id) if self.person_visible(history, node, id, activity, end))
                || sent
            {
                authors.push(node);
            }
            if received {
                if matches!(identity, Identity::Shared) {
                    shared = Some(node);
                } else {
                    targets.push(node);
                }
            }
        }
        let order = |&a: &usize, &b: &usize| {
            let (first, second) = (&history.nodes[a], &history.nodes[b]);
            let rank = |n: &Identity| match n {
                Identity::Persona(_) => 0,
                Identity::Operator(_) => 1,
                Identity::Inbox => 2,
                Identity::Shared => 3,
            };
            rank(first)
                .cmp(&rank(second))
                .then_with(|| history.name(a).cmp(history.name(b)))
                .then_with(|| first.cmp(second))
        };
        authors.sort_by(order);
        if self.held.is_none() {
            if let ai_personas::neural::live::RunnerObservation::Observed { workers, .. } =
                &app.runner_observation
            {
                authors.sort_by_key(|&node| !matches!(&history.nodes[node], Identity::Persona(id) if workers.get(id).is_some_and(|work| self.activity.as_ref().is_none_or(|a| a == work))));
            }
        }
        targets.sort_by(order);
        if authors.is_empty() {
            ui.weak(if self.activity.is_some() && self.held.is_some() {
                "This activity has no contributions by this point."
            } else if self.activity.is_some() {
                "No participants or contributions are recorded in this activity."
            } else {
                "Add a persona or invite someone to an activity to start the map."
            });
            return false;
        }
        let compact = ui.available_width() < 525.;
        let size = if compact { 3 } else { PAGE };
        self.sender_page = self.sender_page.min((authors.len() - 1) / size);
        self.recipient_page = self
            .recipient_page
            .min(targets.len().saturating_sub(1) / PAGE);
        let sources: Vec<_> = authors
            .iter()
            .skip(self.sender_page * size)
            .take(size)
            .map(|node| {
                Sender::read(
                    app,
                    history,
                    *node,
                    end,
                    self.held.is_none(),
                    self.activity.as_deref(),
                )
            })
            .collect();
        let destinations: Vec<_> = if compact {
            targets.iter().copied().chain(shared).collect()
        } else {
            targets
                .iter()
                .skip(self.recipient_page * PAGE)
                .take(PAGE)
                .copied()
                .chain(shared)
                .collect()
        };
        let previous_scope = self.scope;
        let picked_route = if compact {
            self.list_map(ui, history, end, &sources, &destinations)
        } else {
            self.draw_map(ui, app, history, end, &sources, &destinations)
        };
        if authors.len() > size || !compact && targets.len() > PAGE {
            ui.horizontal_wrapped(|ui| {
                page(ui, "Senders", &mut self.sender_page, authors.len(), size);
                if !compact {
                    page(
                        ui,
                        "Recipients",
                        &mut self.recipient_page,
                        targets.len(),
                        PAGE,
                    );
                }
            });
        }
        let mut read = picked_route
            || self.scope != previous_scope
                && self.scope != Scope::default()
                && (compact || self.scope.from.is_some() && self.scope.to.is_some());
        ui.horizontal_wrapped(|ui| {
            if destinations.is_empty() {
                ui.small("Select a persona to inspect their work.");
            } else if compact {
                ui.small("Select a name or an address to read its history.");
            } else {
                ui.small("Line width shows volume. Dashed lines are shared posts.");
            }
            if !split && self.scope != Scope::default() {
                if ui.small_button("Read selection").clicked() {
                    read = true;
                }
                if ui.small_button("Clear selection").clicked() {
                    self.select(Scope::default());
                }
            }
        });
        read
    }

    fn toggle_from(&mut self, node: usize) {
        self.select(Scope {
            from: (self.scope.from != Some(node)).then_some(node),
            ..self.scope
        });
    }

    fn toggle_to(&mut self, node: usize) {
        self.select(Scope {
            to: (self.scope.to != Some(node)).then_some(node),
            ..self.scope
        });
    }

    fn selected_endpoint(&self, h: &History, node: usize, sender: bool) -> bool {
        let (primary, other) = if sender {
            (self.scope.from, self.scope.to)
        } else {
            (self.scope.to, self.scope.from)
        };
        if !h.is_pair(self.scope) {
            return primary == Some(node);
        }
        match self.direction {
            Direction::Both => primary == Some(node) || other == Some(node),
            Direction::Forward => primary == Some(node),
            Direction::Reverse => other == Some(node),
        }
    }

    fn list_map(
        &mut self,
        ui: &mut Ui,
        h: &History,
        end: usize,
        sources: &[Sender],
        targets: &[usize],
    ) -> bool {
        let mut picked = false;
        let p = theme::palette(ui);
        let label = self.scope.to.map_or("All recipients", |n| h.name(n));
        egui::ComboBox::from_id_salt("society-list-recipients")
            .selected_text(short_text(label, 30))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.scope.to.is_none(), "All recipients")
                    .clicked()
                {
                    self.select(Scope {
                        to: None,
                        ..self.scope
                    });
                }
                for &to in targets {
                    let who = &h.nodes[to];
                    if ui
                        .selectable_label(
                            self.scope.to == Some(to),
                            format!("{} · {}", h.name(to), who.role()),
                        )
                        .clicked()
                    {
                        self.select(Scope {
                            to: Some(to),
                            ..self.scope
                        });
                    }
                }
            });
        for row in sources {
            ui.separator();
            let who = &h.nodes[row.node];
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(29., 29.), Sense::hover());
                mark(ui, r, h, row.node);
                if ui
                    .selectable_label(
                        self.selected_endpoint(h, row.node, true),
                        RichText::new(h.name(row.node)).strong(),
                    )
                    .clicked()
                {
                    self.toggle_from(row.node);
                }
                if matches!(who, Identity::Operator(_)) {
                    ui.small("Human");
                }
            });
            ui.label(RichText::new(&row.state).small().color(if row.live_worker {
                p.ink(TEAL)
            } else {
                p.text
            }))
            .on_hover_text(&row.detail);
            if !row.note.is_empty() {
                ui.label(short_text(&row.note, 90))
                    .on_hover_text(&row.detail);
            }
            if self.activity.is_none() && !row.work.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(&row.work).small().color(p.muted)).truncate(),
                )
                .on_hover_text(&row.work);
            }
            ui.horizontal_wrapped(|ui| {
                for &to in targets {
                    let Some(indices) = h.time.routes.get(&(row.node, to)) else {
                        continue;
                    };
                    let count = through(indices, end).len();
                    if count == 0 {
                        continue;
                    }

                    let pair = Scope {
                        from: Some(row.node),
                        to: Some(to),
                    };
                    if ui
                        .add(
                            egui::Button::selectable(
                                self.scope != Scope::default() && self.shows_route(h, row.node, to),
                                format!("→ {} · {count}", h.name(to)),
                            )
                            .small(),
                        )
                        .on_hover_text(format!(
                            "{} to {} · {} records",
                            h.name(row.node),
                            h.name(to),
                            count
                        ))
                        .clicked()
                    {
                        self.select(pair);
                        picked = true;
                    }
                }
            });
        }
        picked
    }

    fn draw_map(
        &mut self,
        ui: &mut Ui,
        app: &App,
        h: &History,
        end: usize,
        sources: &[Sender],
        targets: &[usize],
    ) -> bool {
        let mut picked = false;
        let p = theme::palette(ui);
        let width = ui.available_width();
        let left = (width * if width < 780. { 0.39 } else { 0.35 }).clamp(180., 430.);
        let right = (width * 0.21).clamp(118., 240.);
        let lean = width < 780.;
        let rows = sources.len().max(targets.len()).max(1);
        let row_height = if lean { 52. } else { 61. };
        let height = rows as f32 * row_height;
        let (area, _) = ui.allocate_exact_size(vec2(width, height + 27.), Sense::hover());
        let body = Rect::from_min_max(area.min + vec2(0., 24.), area.max);
        let x1 = body.left() + left;
        let x2 = body.right() - right;
        let draw = ui.painter().clone();
        let heading = if self.held.is_some() {
            "SENDER  /  LAST ACTION"
        } else {
            "SENDER  /  CURRENT WORK"
        };
        draw.text(
            area.min + vec2(3., 3.),
            egui::Align2::LEFT_TOP,
            heading,
            FontId::monospace(11.),
            p.muted,
        );
        draw.text(
            pos2(x2 + 12., area.top() + 3.),
            egui::Align2::LEFT_TOP,
            "RECIPIENT",
            FontId::monospace(11.),
            p.muted,
        );
        let mut center = body;
        center.set_left(x1);
        center.set_right(x2);
        draw.rect_filled(center, 3, p.field);
        for t in [0.25, 0.5, 0.75] {
            draw.vline(
                egui::lerp(x1..=x2, t),
                center.y_range(),
                Stroke::new(0.6, p.field.lerp_to_gamma(p.border, 0.7)),
            );
        }
        let y = |i: usize, count: usize| body.top() + (i as f32 + 0.5) * height / count as f32;
        let from_y = |i| y(i, sources.len());
        let to_y = |i| y(i, targets.len());
        // Spread the endpoints of a busy persona's lines so nearby paths remain traceable.
        let port = |slot: usize, slots: usize| {
            if slots <= 1 {
                0.
            } else {
                (slot as f32 / (slots - 1) as f32 - 0.5) * 22.
            }
        };
        let mut paths = Vec::new();
        for (i, row) in sources.iter().enumerate() {
            for (j, &to) in targets.iter().enumerate() {
                let Some(events) = h.time.routes.get(&(row.node, to)) else {
                    continue;
                };
                let visible = through(events, end);
                if let Some(&last) = visible.last() {
                    paths.push(Path {
                        from: row.node,
                        to,
                        count: visible.len(),
                        last,
                        points: curve(
                            pos2(x1, from_y(i) + port(j, targets.len())),
                            pos2(x2, to_y(j) + port(i, sources.len())),
                        ),
                    });
                }
            }
        }
        let surface = ui.interact(center, ui.id().with("society-connections"), Sense::click());
        let hovered = surface.hover_pos().and_then(|pos| {
            paths
                .iter()
                .enumerate()
                .map(|(i, path)| (i, distance(&path.points, pos)))
                .filter(|(_, d)| *d <= 7.)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
        let matches = |path: &Path| self.shows_route(h, path.from, path.to);
        for highlight in [false, true] {
            for (i, path) in paths.iter().enumerate() {
                let lit = hovered.map_or_else(
                    || self.scope != Scope::default() && matches(path),
                    |n| n == i,
                );
                if lit != highlight {
                    continue;
                }
                let faded = !lit && (hovered.is_some() || self.scope != Scope::default());
                let ink = h.nodes[path.from].color(ui);
                let ink = p.field.lerp_to_gamma(
                    ink,
                    if faded {
                        0.19
                    } else if lit {
                        1.
                    } else {
                        0.74
                    },
                );
                let weight =
                    (1.0 + (path.count as f32).log2() * 0.64).min(5.) + if lit { 1.2 } else { 0. };
                let stroke = Stroke::new(weight, ink);
                if matches!(h.nodes[path.to], Identity::Shared) {
                    draw.extend(egui::Shape::dashed_line(&path.points, stroke, 6., 4.));
                } else {
                    draw.add(egui::Shape::line(path.points.clone(), stroke));
                }
                let tip = path.points[path.points.len() - 1];
                draw.add(egui::Shape::convex_polygon(
                    vec![tip, tip + vec2(-7., -4.), tip + vec2(-7., 4.)],
                    ink,
                    Stroke::NONE,
                ));
            }
        }
        if let Some(i) = hovered {
            let path = &paths[i];
            let event = &app.events[h.time.entries[path.last].event];
            if surface.clicked() {
                picked = true;
                self.select(Scope {
                    from: Some(path.from),
                    to: Some(path.to),
                });
            }
            surface
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_ui(|ui| {
                    ui.strong(format!("{} → {}", h.name(path.from), h.name(path.to)));
                    ui.label(counted(path.count as u64, "contribution", "contributions"));
                    ui.small(format!(
                        "Latest #{} · {}",
                        event["event"],
                        short_text(event_detail(event), 120)
                    ));
                });
        }
        if paths.is_empty() {
            let message = if targets.is_empty() {
                "No exchanges yet"
            } else {
                "No connections on this page"
            };
            draw.text(
                center.center(),
                egui::Align2::CENTER_CENTER,
                message,
                FontId::proportional(12.),
                p.muted,
            );
        }
        for (i, row) in sources.iter().enumerate() {
            let node = &h.nodes[row.node];
            let mid = from_y(i);
            let half = (row_height - 2.) * 0.5;
            let r = Rect::from_min_max(pos2(body.left(), mid - half), pos2(x1 - 5., mid + half));
            let selected = self.selected_endpoint(h, row.node, true);
            let response = endpoint(
                ui,
                r,
                ("from", row.node),
                selected,
                &format!("From {} · {}", h.name(row.node), row.state),
            );
            if response.clicked() {
                self.toggle_from(row.node);
            }
            response.on_hover_ui(|ui| {
                ui.strong(format!("{} · {}", h.name(row.node), node.role()));
                if !row.detail.is_empty() {
                    ui.label(&row.detail);
                } else {
                    ui.label(&row.state);
                }
            });
            mark(
                ui,
                Rect::from_center_size(pos2(r.left() + 22., mid), vec2(38., 42.)),
                h, row.node,
            );
            let x = r.left() + 49.;
            let room = r.right() - x;
            if lean {
                text_line(
                    ui,
                    pos2(x, mid - 23.),
                    room,
                    RichText::new(h.name(row.node)).size(15.).strong(),
                    p.text,
                );
                text_line(
                    ui,
                    pos2(x, mid - 5.),
                    room,
                    RichText::new(&row.state).size(12.),
                    node.color(ui),
                );
                let detail = if self.activity.is_some() {
                    &row.note
                } else {
                    &row.work
                };
                if !detail.is_empty() {
                    text_line(
                        ui,
                        pos2(x, mid + 10.),
                        room,
                        RichText::new(short_text(detail, 100)).size(11.),
                        p.muted,
                    );
                }
                draw.vline(x1, mid - 11. ..=mid + 11., Stroke::new(2., node.color(ui)));
                continue;
            }
            let name_width = text_line(
                ui,
                pos2(x, mid - 27.),
                if row.note.is_empty() {
                    room
                } else {
                    room * 0.56
                },
                RichText::new(h.name(row.node)).size(16.).strong(),
                p.text,
            );
            let state = if row.note.is_empty() {
                pos2(x, mid - 5.)
            } else {
                pos2(x + name_width + 9., mid - 24.)
            };
            text_line(
                ui,
                state,
                r.right() - state.x,
                RichText::new(&row.state).size(12.),
                if row.live_worker {
                    p.ink(TEAL)
                } else {
                    node.color(ui)
                },
            );
            text_line(
                ui,
                pos2(x, mid - 5.),
                r.right() - x,
                RichText::new(&row.note).size(13.),
                p.text,
            );
            text_line(
                ui,
                pos2(x, mid + 12.),
                r.right() - x,
                RichText::new(short_text(&row.work, 140)).size(12.),
                p.muted,
            );
            draw.vline(x1, mid - 11. ..=mid + 11., Stroke::new(2., node.color(ui)));
        }
        for (i, &node) in targets.iter().enumerate() {
            let identity = &h.nodes[node];
            let mid = to_y(i);
            let r = Rect::from_min_max(pos2(x2 + 4., mid - 21.), pos2(body.right(), mid + 21.));
            let count = through(&h.time.received[&node], end).len();
            let name = h.name(node);
            let response = endpoint(
                ui,
                r,
                ("to", node),
                self.selected_endpoint(h, node, false),
                &format!("To {name} · {count} contributions"),
            );
            if response.clicked() {
                self.toggle_to(node);
            }
            response.on_hover_text(format!(
                "{name} · {}\n{count} contributions received by this point",
                identity.role()
            ));
            let x = r.left() + 10.;
            text_line(
                ui,
                pos2(x, mid - 16.),
                r.right() - x,
                RichText::new(name).strong().size(15.),
                p.text,
            );
            let pair = h.is_pair(self.scope);
            let peer = if pair && self.scope.from == Some(node) {
                self.scope.to
            } else if !pair || self.scope.to == Some(node) {
                self.scope.from
            } else {
                None
            };
            let summary = match peer {
                Some(from) => {
                    let sent = h
                        .time
                        .routes
                        .get(&(from, node))
                        .map_or(0, |v| through(v, end).len());
                    if pair {
                        format!("{sent} from {}", h.name(from))
                    } else {
                        format!("{sent} from {} · {count} total", h.name(from))
                    }
                }
                None => match identity {
                    Identity::Shared => format!("{count} shared posts"),
                    Identity::Operator(_) => format!("Human · {count} addressed"),
                    _ => format!("{count} addressed"),
                },
            };
            text_line(
                ui,
                pos2(x, mid + 3.),
                r.right() - x,
                RichText::new(summary).size(12.),
                p.muted,
            );
        }
        picked
    }
}

struct Path {
    from: usize,
    to: usize,
    count: usize,
    last: usize,
    points: Vec<Pos2>,
}

fn curve(start: Pos2, end: Pos2) -> Vec<Pos2> {
    let middle = (start.x + end.x) * 0.5;
    let curve = egui::epaint::CubicBezierShape::from_points_stroke(
        [start, pos2(middle, start.y), pos2(middle, end.y), end],
        false,
        Color32::TRANSPARENT,
        Stroke::NONE,
    );
    (0..=36).map(|n| curve.sample(n as f32 / 36.)).collect()
}

fn distance(points: &[Pos2], pos: Pos2) -> f32 {
    points
        .windows(2)
        .map(|line| {
            let delta = line[1] - line[0];
            let t = ((pos - line[0]).dot(delta) / delta.length_sq().max(0.0001)).clamp(0., 1.);
            pos.distance(line[0] + delta * t)
        })
        .fold(f32::INFINITY, f32::min)
}

fn page(ui: &mut Ui, name: &str, page: &mut usize, total: usize, size: usize) {
    if total <= size {
        return;
    }
    let start = *page * size;
    ui.push_id(name, |ui| {
        ui.horizontal(|ui| {
            ui.small(format!(
                "{name} {}–{} of {total}",
                start + 1,
                (start + size).min(total)
            ));
            if ui
                .add_enabled(*page > 0, egui::Button::new("‹").small())
                .on_hover_text(format!("Previous {name}"))
                .clicked()
            {
                *page -= 1;
            }
            if ui
                .add_enabled(start + size < total, egui::Button::new("›").small())
                .on_hover_text(format!("Next {name}"))
                .clicked()
            {
                *page += 1;
            }
        })
    });
}

fn endpoint(
    ui: &mut Ui,
    rect: Rect,
    id: (&str, usize),
    selected: bool,
    label: &str,
) -> egui::Response {
    let r = ui.interact(rect, ui.id().with(id), Sense::click());
    r.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let p = theme::palette(ui);
    if selected || r.has_focus() || r.hovered() {
        ui.painter()
            .rect_filled(rect, 4, if selected { p.selection } else { p.hover });
    }
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn text_line(ui: &Ui, pos: Pos2, width: f32, text: RichText, ink: Color32) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        width.max(1.),
        egui::TextStyle::Body,
    );
    let width = galley.size().x;
    ui.painter().galley(pos, galley, ink);
    width
}

pub(super) fn mark(ui: &Ui, rect: Rect, history: &History<'_>, index: usize) {
    let node = &history.nodes[index];
    if let Identity::Persona(id) = node {
        design::persona_mark(ui, rect, history.name(index), persona_color(id));
    } else {
        let p = theme::palette(ui);
        ui.painter().rect_filled(rect, 5, p.field);
        theme::paint_icon(ui.painter(), rect.shrink(6.), Icon::People, node.color(ui));
    }
}
