//! Interactive view of measured crossings in original STL coordinates.
use super::{operator_call, s, theme, TEAL};
use ai_personas::neural::{
    cli::Command,
    geometry::profile::{Axis, Profile},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui};
use serde_json::Value;
use std::{path::Path, sync::mpsc};

enum Answer {
    Bounds(Value),
    Profile(Value),
}
#[derive(Default)]
pub(super) struct Window {
    visible: bool,
    reference: String,
    member: Option<String>,
    edition: Option<String>,
    work: String,
    axis: Option<Axis>,
    coordinates: [String; 3],
    bounds: Option<Value>,
    result: Option<Value>,
    selected: Option<usize>,
    error: Option<String>,
    pending: Option<mpsc::Receiver<Result<Answer, String>>>,
}
impl Window {
    pub(super) fn open(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        operator: &str,
        reference: &str,
        work: &str,
        member: Option<String>,
        edition: Option<String>,
    ) {
        *self = Self {
            visible: true,
            reference: reference.into(),
            member: member.clone(),
            edition: edition.clone(),
            work: work.into(),
            axis: Some(Axis::X),
            ..Default::default()
        };
        let command = Command::InspectStl {
            reference: reference.into(),
            member, edition,
        };
        self.start(ctx, root, operator, command, Answer::Bounds);
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        operator: &str,
        command: Command,
        wrap: fn(Value) -> Answer,
    ) {
        let root = root.to_owned();
        let operator = operator.to_owned();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.error = None;
        std::thread::spawn(move || {
            let _ = tx.send(operator_call(&root, &operator, command, None).map(wrap));
            ctx.request_repaint();
        });
    }
    fn receive(&mut self) {
        let Some(rx) = &self.pending else {
            return;
        };
        let a = match rx.try_recv() {
            Ok(a) => a,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The mesh reader disconnected.".into()),
        };
        self.pending = None;
        match a {
            Err(error) => self.error = Some(error),
            Ok(Answer::Profile(result)) => {
                self.result = Some(result);
                self.selected = None;
            }
            Ok(Answer::Bounds(value)) => {
                let g = &value["geometry"];
                for i in 0..3 {
                    if let (Some(a), Some(b)) = (g["minimum"][i].as_f64(), g["maximum"][i].as_f64())
                    {
                        self.coordinates[i] = ((a + b) / 2.).to_string();
                    }
                }
                self.bounds = Some(value);
            }
        }
    }
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        operator: &str,
    ) -> Option<String> {
        self.receive();
        if !self.visible {
            return None;
        }
        let mut visible = true;
        let mut command = None;
        let mut open = None;
        egui::Window::new("Mesh line profile")
            .id(egui::Id::new("studio-mesh-line"))
            .open(&mut visible)
            .default_width(660.)
            .default_height(520.)
            .show(ctx, |ui| {
                if let Some(path) = &self.member {
                    ui.strong(path);
                    ui.small("Member of this exact ZIP");
                }
                ui.monospace(&self.reference);
                if let Some(edition) = &self.edition { ui.small(edition); }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                if self.pending.is_some() {
                    ui.spinner();
                }
                let Some(bounds) = &self.bounds else { return };
                let geometry = &bounds["geometry"];
                if let Some(extent) = geometry["span"].as_array().filter(|v| v.len() == 3) {
                    ui.horizontal_wrapped(|ui| {
                        ui.small("Model envelope");
                        for (axis, span) in ["X", "Y", "Z"].iter().zip(extent) {
                            if let Some(value) = span.as_f64() { ui.strong(format!("{axis}  {}", number(value))); }
                        }
                        ui.small("model units");
                    });
                }
                let mut changed = false;
                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("Measure along");
                        for (axis, label) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
                            changed |= ui.selectable_value(&mut self.axis, Some(axis), label).changed();
                        }
                    });
                    let axis = self.axis.unwrap_or(Axis::X);
                    let [_, u, v] = axis.indices();
                    ui.horizontal_wrapped(|ui| {
                        for index in [u, v] {
                            ui.label(["X", "Y", "Z"][index]);
                            changed |= ui.add(egui::TextEdit::singleline(&mut self.coordinates[index])
                                .desired_width(92.)).changed();
                            let lo = geometry["minimum"][index].as_f64().unwrap_or_default();
                            let hi = geometry["maximum"][index].as_f64().unwrap_or_default();
                            ui.small(format!("{} … {}", number(lo), number(hi)));
                        }
                    });
                    let coordinate = |index: usize| self.coordinates[index].trim().parse::<f64>()
                        .ok().filter(|value| value.is_finite());
                    let line = coordinate(u).zip(coordinate(v));
                    if ui.add_enabled(line.is_some(), egui::Button::new("Measure line")).clicked() {
                        if let Some((u, v)) = line {
                            command = Some(Command::MeshProfile {
                                work: (!self.work.is_empty()).then(|| self.work.clone()),
                                 profile: Profile { reference: self.reference.clone(), member: self.member.clone(), edition: self.edition.clone(), axis, u, v },
                            });
                        }
                    }
                });
                if changed {
                    self.result = None;
                    self.selected = None;
                }
                ui.separator();
                let Some(result) = &self.result else {
                    ui.weak("Distances use the original mesh coordinates. STL files do not encode a unit.");
                    return;
                };
                let profile = &result["profile"];
                let spans = profile["intervals"].as_array().map(Vec::as_slice).unwrap_or_default();
                let crossings = profile["crossings"].as_array().map(Vec::as_slice).unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("{} surface crossings", crossings.len()));
                    if ui.small_button("Open receipt").clicked() {
                        open = Some(s(result, "receipt").to_owned());
                    }
                    if ui.small_button("Copy receipt").clicked() {
                        ctx.copy_text(s(result, "receipt").to_owned());
                    }
                });
                if profile["classified"] != true {
                    ui.colored_label(theme::ACCENT, "Surface positions only");
                    let validation = if profile["mesh_validation"]["passed"] == true { "passed" } else { "failed" };
                    ui.label(format!("Mesh edge validation {validation} · {} coplanar faces · {} mixed-direction contacts",
                        profile["coplanar_faces"], profile["mixed_direction_contacts"]));
                    if profile["ambiguous_crossing_order"] == true {
                        ui.weak("The surface crossings have an ambiguous order.");
                    }
                }
                let lo = profile["axis_bounds"][0].as_f64().unwrap_or_default();
                let hi = profile["axis_bounds"][1].as_f64().unwrap_or(lo);
                profile_strip(ui, lo, hi, spans, &mut self.selected);
                if let Some(span) = self.selected.and_then(|index| spans.get(index)) {
                    let region = super::title(s(span, "region"));
                    let length = number(span["length"].as_f64().unwrap_or_default());
                    ui.strong(format!("{region} · {length} model units"));
                }
                egui::ScrollArea::vertical().id_salt("mesh-profile-rows").auto_shrink([false,false]).show(ui, |ui| {
                    if spans.is_empty() {
                        match crossings.first().and_then(Value::as_f64) {
                            Some(position) => { ui.label(format!("Isolated surface crossing at {}", number(position))); }
                            None => { ui.weak("No isolated crossings on this line."); }
                        }
                        return;
                    }
                    egui::Grid::new("mesh-profile-table").striped(true).spacing([22.,5.]).show(ui, |ui| {
                        for heading in ["Region", "From", "To", "Span"] {
                            ui.strong(heading);
                        }
                        ui.end_row();
                        for (i, span) in spans.iter().enumerate() {
                            if ui.selectable_label(self.selected == Some(i), super::title(s(span, "region"))).clicked() {
                                self.selected = Some(i);
                            }
                            for key in ["from", "to", "length"] {
                                let value = span[key].as_f64().unwrap_or_default();
                                ui.monospace(number(value)).on_hover_text(value.to_string());
                            }
                            ui.end_row();
                        }
                    });
                });
            });
        self.visible = visible && open.is_none();
        if let Some(c) = command {
            self.start(ctx, root, operator, c, Answer::Profile);
        }
        open
    }
}
fn number(v: f64) -> String {
    if v != 0. && (v.abs() < 0.0001 || v.abs() >= 1e8) {
        format!("{v:.5e}")
    } else {
        format!("{v:.5}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}
fn profile_strip(ui: &mut Ui, lo: f64, hi: f64, spans: &[Value], selected: &mut Option<usize>) {
    if hi <= lo {
        return;
    }
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 35.), Sense::click());
    let band = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 6.),
        Pos2::new(rect.right(), rect.bottom() - 4.),
    );
    let painter = ui.painter();
    painter.rect_filled(band, 2., theme::palette(ui).field);
    let at = |x: f64| rect.left() + (((x - lo) / (hi - lo)) as f32) * rect.width();
    let pointer = response.hover_pos();
    let mut hover = None;
    for (i, span) in spans.iter().enumerate() {
        let (Some(a), Some(b)) = (span["from"].as_f64(), span["to"].as_f64()) else {
            continue;
        };
        let r = Rect::from_min_max(
            Pos2::new(at(a), band.top()),
            Pos2::new(at(b), band.bottom()),
        );
        let fill = match span["region"].as_str() {
            Some("inside") => TEAL,
            Some("outside") => theme::palette(ui).field,
            _ => theme::ACCENT,
        };
        painter.rect_filled(r, 0., fill);
        painter.vline(
            r.left(),
            band.y_range(),
            Stroke::new(1., theme::palette(ui).muted),
        );
        if let Some(pos) = pointer.filter(|pos| r.contains(*pos)) {
            hover = Some((i, pos.x));
        }
        if *selected == Some(i) {
            painter.hline(r.x_range(), band.bottom(), Stroke::new(3., Color32::WHITE));
        }
    }
    if let Some((i, x)) = hover {
        painter.vline(
            x,
            band.y_range(),
            Stroke::new(1.5, ui.visuals().text_color()),
        );
        if response.clicked() {
            *selected = Some(i);
        }
        response.on_hover_text(format!(
            "{} · {}",
            super::title(s(&spans[i], "region")),
            number(spans[i]["length"].as_f64().unwrap_or_default())
        ));
    }
    ui.horizontal(|ui| {
        ui.small(number(lo));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.small(number(hi));
        });
    });
}
