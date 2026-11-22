//! A chronological score of actual work. Each column is one event, not a time
//! interval or a measure of progress. Only an addressed exchange joins lanes.
use super::*;

const COLUMNS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Tool,
    File,
    Message,
    Request,
    Return,
    Issue,
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Tool => "Tool",
            Self::File => "File",
            Self::Message => "Message",
            Self::Request => "Request",
            Self::Return => "Update",
            Self::Issue => "Issue",
        }
    }
    fn classify(e: &Value) -> Option<Self> {
        let (k, body) = (s(e, "kind"), &e["body"]);
        if e["invocation"].is_null() {
            return match k {
                "user_message" if body["human_request"].is_string() => Some(Self::Return),
                "human_request_progress" => Some(Self::Return),
                "user_message" => Some(Self::Message),
                _ => None,
            };
        }
        match k {
            "message" | "reply" => Some(Self::Message),
            "human_request" if e["target"] == "user" => Some(Self::Request),
            // Ignore the second, broadcast summary of the very same issue.
            "delivery" if body["record"]["owner"] == e["actor"] && e["target"] != "*" => {
                Some(Self::Issue)
            }
            "tool_result" if body["context"] == "complete" && e["target"] == e["actor"] => {
                if e["tool_outcome"]["file_reference"].is_string() {
                    Some(Self::File)
                } else if body["effect"].is_string() {
                    Some(Self::Tool)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct Mark {
    index: usize,
    kind: Kind,
}
#[derive(Default)]
struct Tracks {
    all: Vec<Mark>,
    exchanges: Vec<Mark>,
    handoffs: Vec<Mark>,
}
#[derive(Default)]
pub(in super::super) struct Loom {
    tracks: BTreeMap<String, Tracks>,
    filter: usize,
    selected: Option<String>,
    // Holding a page pins both its activity and its end while events arrive.
    held: Option<(String, usize)>,
}
impl Loom {
    pub(in super::super) fn observe(&mut self, rows: &[Value], start: usize) {
        for (i, event) in rows.iter().enumerate() {
            let Some(kind) = Kind::classify(event) else {
                continue;
            };
            let Some(work) = event["work"].as_str() else {
                continue;
            };
            let tracks = self.tracks.entry(work.into()).or_default();
            let mark = Mark {
                index: start + i,
                kind,
            };
            tracks.all.push(mark);
            match kind {
                Kind::Message => tracks.exchanges.push(mark),
                Kind::File | Kind::Issue | Kind::Request | Kind::Return => {
                    tracks.handoffs.push(mark)
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lane<'a> {
    Persona(&'a str),
    Human(&'a str),
}
impl Lane<'_> {
    fn name(self, app: &App) -> String {
        match self {
            Self::Persona(p) => app.label(p).into(),
            Self::Human("user") => "Requester".into(),
            Self::Human(h) => format!("Human · {h}"),
        }
    }
    fn tint(self, ui: &Ui) -> Color32 {
        theme::palette(ui).ink(match self {
            Self::Persona(p) => persona_color(p),
            Self::Human(_) => theme::palette(ui).muted,
        })
    }
}
struct Beat<'a> {
    event: &'a Value,
    kind: Kind,
    from: Lane<'a>,
    to: Option<Lane<'a>>,
}
impl<'a> Beat<'a> {
    fn new(event: &'a Value, kind: Kind) -> Self {
        let (actor, target) = (s(event, "actor"), s(event, "target"));
        let human = event["invocation"].is_null();
        let from = if human {
            Lane::Human(actor)
        } else {
            Lane::Persona(actor)
        };
        let to = (!target.is_empty()
            && target != "*"
            && matches!(
                kind,
                Kind::Message | Kind::Request | Kind::Return | Kind::Issue
            ))
        .then(|| {
            if !human && target == "user" {
                Lane::Human(target)
            } else {
                Lane::Persona(target)
            }
        })
        .filter(|lane| *lane != from);
        Self {
            event,
            kind,
            from,
            to,
        }
    }
    fn seq(&self) -> i64 {
        self.event["event"].as_i64().unwrap_or_default()
    }
    fn caption(&self, app: &App) -> String {
        let e = self.event;
        let body = &e["body"];
        let detail = match self.kind {
            Kind::Tool => format!("{} · {}", s(body, "tool"), s(&e["tool_outcome"], "label")),
            Kind::File => s(&e["tool_outcome"], "file_path").into(),
            Kind::Issue => s(&body["record"], "title").into(),
            Kind::Request => s(body, "title").into(),
            Kind::Message | Kind::Return => short_text(s(body, "text"), 160),
        };
        let mut who = self.from.name(app);
        if let Some(to) = self.to {
            who.push_str(" → ");
            who.push_str(&to.name(app));
        }
        format!(
            "#{seq}  {who} · {label}: {detail}",
            seq = self.seq(),
            label = self.kind.label()
        )
    }
}

impl App {
    pub(super) fn activity_loom(&mut self, ui: &mut Ui) -> Option<(String, Option<i64>)> {
        let latest = self.home_index.activities.first()?;
        let latest = latest.id.clone();
        let mut open = None;
        ui.horizontal_wrapped(|ui| {
            theme::glyph(ui, Icon::Activity, theme::palette(ui).accent);
            eyebrow(ui, "Activity loom");
            theme::badge(
                ui,
                if self.home.loom.held.is_some() {
                    "Held"
                } else {
                    "Latest"
                },
                TEAL,
            );
            if self.home_index.activities.len() > 1 {
                let loom = &mut self.home.loom;
                egui::ComboBox::from_id_salt("loom-activity")
                    .selected_text("Choose activity")
                    .width(138.)
                    .show_ui(ui, |ui| {
                        ui.set_max_width((ui.ctx().content_rect().width() - 32.).clamp(120., 440.));
                        if ui
                            .selectable_value(&mut loom.selected, None, "Follow workspace")
                            .clicked()
                        {
                            loom.held = None;
                        }
                        for a in &self.home_index.activities {
                            if ui
                                .selectable_value(
                                    &mut loom.selected,
                                    Some(a.id.clone()),
                                    short_text(&a.title, 60),
                                )
                                .on_hover_text(&a.title)
                                .clicked()
                            {
                                loom.held = None;
                            }
                        }
                    });
            }
        });
        let work = self
            .home
            .loom
            .held
            .as_ref()
            .map(|(w, _)| w)
            .or(self.home.loom.selected.as_ref())
            .unwrap_or(&latest)
            .clone();
        let activity = self.home_index.activity(&work)?;
        if title_link(ui, &activity.title, &activity.id).clicked() {
            open = Some(None);
        }
        let loom = &mut self.home.loom;
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["All marks", "Messages", "Files & handoffs"]
                .into_iter()
                .enumerate()
            {
                if ui.selectable_value(&mut loom.filter, i, label).changed() {
                    // Keep the selected activity if its reader had held it.
                    if let Some((_, end)) = &mut loom.held {
                        *end = usize::MAX;
                    }
                }
            }
        });
        let marks: &[Mark] = loom
            .tracks
            .get(&work)
            .map(|t| match loom.filter {
                1 => &t.exchanges,
                2 => &t.handoffs,
                _ => &t.all,
            })
            .map(Vec::as_slice)
            .unwrap_or_default();
        let total = marks.len();
        let end = loom.held.as_ref().map_or(total, |(_, n)| (*n).min(total));
        if let Some((_, n)) = &mut loom.held {
            *n = end;
        }
        let start = end.saturating_sub(COLUMNS);
        let beats: Vec<_> = marks[start..end]
            .iter()
            .map(|m| Beat::new(&self.events[m.index], m.kind))
            .collect();
        if beats.is_empty() {
            ui.small("No matching records in this activity yet.");
        } else {
            let lanes: BTreeSet<_> = beats
                .iter()
                .flat_map(|b| std::iter::once(b.from).chain(b.to))
                .collect();
            if ui.available_width() >= 395. && lanes.len() <= 5 {
                if let Some(event) = score(ui, self, &beats, &lanes.into_iter().collect::<Vec<_>>())
                {
                    open = Some(Some(event));
                }
                ui.small("Read left to right. Solid mark: author. Ring: addressee. Select a column to read the record.");
            } else {
                for b in &beats {
                    let detail = b.caption(self);
                    if ui
                        .link(short_text(&detail, row_limit(ui, 1.1)))
                        .on_hover_text(detail)
                        .clicked()
                    {
                        open = Some(Some(b.seq()));
                    }
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            if total > 0 {
                ui.small(format!("{}–{end} of {total}", start + 1));
            }
            if ui
                .add_enabled(start > 0, egui::Button::new("Older").small())
                .clicked()
            {
                self.home.loom.held = Some((work.clone(), start));
            }
            if self.home.loom.held.is_some() {
                if ui
                    .add_enabled(end < total, egui::Button::new("Newer").small())
                    .clicked()
                {
                    self.home.loom.held = Some((work.clone(), (end + COLUMNS).min(total)));
                }
                if ui.small_button("Follow latest").clicked() {
                    self.home.loom.held = None;
                }
            } else if total > 0 && ui.small_button("Hold").clicked() {
                self.home.loom.held = Some((work.clone(), end));
            }
        });
        open.map(|event| (work, event))
    }
}

fn score(ui: &mut Ui, app: &App, beats: &[Beat<'_>], lanes: &[Lane<'_>]) -> Option<i64> {
    let p = theme::palette(ui);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 19. + 23. * lanes.len() as f32 + 14.),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6., p.canvas);
    let axis = rect.left() + (rect.width() * 0.24).min(128.);
    let step = (rect.right() - 6. - axis) / beats.len() as f32;
    let y = |i: usize| rect.top() + 29. + 23. * i as f32;
    for (i, lane) in lanes.iter().enumerate() {
        let at = y(i);
        let label_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 9., at - 10.),
            egui::pos2(axis - 5., at + 10.),
        );
        ui.painter_at(label_rect).text(
            label_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            lane.name(app),
            FontId::proportional(13.),
            lane.tint(ui),
        );
        painter.line_segment(
            [egui::pos2(axis, at), egui::pos2(rect.right() - 7., at)],
            Stroke::new(1., p.border),
        );
    }
    let mut open = None;
    for (i, b) in beats.iter().enumerate() {
        let left = axis + step * i as f32;
        let col = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(left + step, rect.bottom()),
        );
        let caption = b.caption(app);
        let hit = ui.interact(
            col,
            ui.make_persistent_id(("loom-mark", b.seq())),
            egui::Sense::click(),
        );
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &caption));
        if hit.has_focus() || hit.hovered() {
            painter.rect_stroke(
                col.shrink(1.),
                4.,
                Stroke::new(1., p.accent),
                egui::StrokeKind::Inside,
            );
        }
        if hit
            .on_hover_ui(|ui| {
                ui.label(&caption);
                ui.small(s(b.event, "created_utc"));
                ui.small("Open timeline record");
            })
            .clicked()
        {
            open = Some(b.seq());
        }
        let x = col.center().x;
        let lane_y = |lane| {
            y(lanes
                .binary_search(&lane)
                .expect("lane gathered from these beats"))
        };
        let point = egui::pos2(x, lane_y(b.from));
        let color = b.from.tint(ui);
        painter.text(
            egui::pos2(x, rect.top() + 9.),
            egui::Align2::CENTER_CENTER,
            b.kind.label(),
            FontId::proportional(11.),
            p.muted,
        );
        if let Some(to) = b.to {
            let target = egui::pos2(x, lane_y(to));
            painter.line_segment([point, target], Stroke::new(1.8, color));
            painter.circle_filled(target, 4.3, p.canvas);
            painter.circle_stroke(target, 4.3, Stroke::new(1.6, to.tint(ui)));
        }
        if matches!(b.kind, Kind::Tool | Kind::File | Kind::Issue) {
            painter.rect_filled(
                egui::Rect::from_center_size(point, egui::vec2(10., 10.)),
                2.,
                color,
            );
        } else {
            painter.circle_filled(point, 5., color);
        }
    }
    open
}
