//! Nominal geometry from the DXF, not a measurement of the fitted SVG preview.
use super::theme;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Ui};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Register {
    Drawing {
        sha256: String,
        units: String,
        layers: Vec<Layer>,
    },
    Unavailable {
        error: String,
    },
}

#[derive(Deserialize)]
pub(super) struct Layer {
    name: String,
    state: String,
    kinds: BTreeMap<String, usize>,
    circles: Vec<Circle>,
}

impl Layer {
    fn count(&self) -> usize {
        self.kinds.values().sum()
    }
}

#[derive(Deserialize)]
struct Circle {
    handle: String,
    center: [f64; 3],
    normal: [f64; 3],
    radius: f64,
}

impl Circle {
    fn parallel(&self) -> bool {
        self.normal[0] == 0. && self.normal[1] == 0.
    }
}

#[derive(Default)]
pub(super) struct View {
    pub open: bool,
    layer: Option<usize>,
    circle: usize,
}

impl View {
    pub fn show(&mut self, ui: &mut Ui, register: &Register, source: &str) {
        let (sha256, units, layers) = match register {
            Register::Drawing {
                sha256,
                units,
                layers,
            } => (sha256, units, layers),
            Register::Unavailable { error } => {
                ui.strong("Layer register unavailable");
                ui.label(error);
                return;
            }
        };
        let mut index = self
            .layer
            .unwrap_or_else(|| layers.iter().position(|l| l.count() > 0).unwrap_or(0));
        let Some(current) = layers.get(index) else {
            ui.label("No modelspace layers listed.");
            return;
        };
        let before = index;
        ui.horizontal_wrapped(|ui| {
            ui.strong("Modelspace layers");
            ui.weak(format!("{} layers  ·  {units}", layers.len()));
            ui.small(format!("Source {}", sha256.get(..10).unwrap_or(sha256)))
                .on_hover_text(format!("{source}\nSHA-256 {sha256}"));
        });
        let mut choices = |ui: &mut Ui| {
            for (i, l) in layers.iter().enumerate() {
                ui.selectable_value(&mut index, i, format!("{}  ·  {}", l.name, l.count()));
            }
        };
        if layers.len() <= 8 && ui.available_width() >= 420. {
            ui.horizontal_wrapped(choices);
        } else {
            egui::ComboBox::from_id_salt("drawing-layer")
                .selected_text(&current.name)
                .truncate()
                .width(180.)
                .show_ui(ui, &mut choices);
        }
        if before != index {
            self.circle = 0;
        }
        self.layer = Some(index);
        let active = &layers[index];
        let kinds = active
            .kinds
            .iter()
            .map(|(k, n)| format!("{k} × {n}"))
            .collect::<Vec<_>>()
            .join("  ·  ");
        ui.horizontal_wrapped(|ui| {
            ui.strong(&active.name);
            ui.weak(&active.state);
            if kinds.is_empty() {
                ui.label("No direct modelspace entities on this layer.");
            } else {
                ui.label(kinds);
            }
        });
        ui.small("Direct entities only. Block inserts are not expanded; no depth or manufacturing intent is inferred.");
        ui.separator();
        if ui.available_width() >= 510. {
            ui.columns(2, |cols| {
                plan(&mut cols[0], layers, index, &mut self.circle);
                self.schedule(&mut cols[1], active, units, source, sha256);
            });
        } else {
            plan(ui, layers, index, &mut self.circle);
            self.schedule(ui, active, units, source, sha256);
        }
    }

    fn schedule(&mut self, ui: &mut Ui, layer: &Layer, units: &str, source: &str, sha256: &str) {
        let Some(c) = layer.circles.get(self.circle) else {
            ui.weak("No direct CIRCLE entities on this layer.");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong("Circle").on_hover_text("# is the decoded entity handle. The reader can assign a handle if the source has none.");
            if layer.circles.len() > 64 {
                let mut number = self.circle + 1;
                ui.add(egui::DragValue::new(&mut number).range(1..=layer.circles.len()));
                self.circle = number - 1;
                ui.label(format!(
                    "of {} · #{}",
                    layer.circles.len(),
                    layer.circles[self.circle].handle
                ));
                return;
            }
            egui::ComboBox::from_id_salt("drawing-circle")
                .width(155.)
                .selected_text(format!(
                    "{} of {} · #{}",
                    self.circle + 1,
                    layer.circles.len(),
                    c.handle
                ))
                .show_ui(ui, |ui| {
                    for (i, c) in layer.circles.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.circle,
                            i,
                            format!("{} · #{} · Ø {}", i + 1, c.handle, 2. * c.radius),
                        );
                    }
                });
        });
        let c = &layer.circles[self.circle];
        let [x, y, z] = c.center;
        let [nx, ny, nz] = c.normal;
        egui::Grid::new("drawing-circle-facts")
            .num_columns(2)
            .spacing([10., 4.])
            .show(ui, |ui| {
                for (name, value) in [
                    ("Centre X", x),
                    ("Y", y),
                    ("Z", z),
                    ("Radius", c.radius),
                    ("Diameter", 2. * c.radius),
                ] {
                    ui.weak(name);
                    ui.monospace(value.to_string());
                    ui.end_row();
                }
            });
        ui.small(format!("World coordinates · normal ({nx}, {ny}, {nz})"));
        if !c.parallel() {
            ui.label("Oblique circle: dimensions are in its own plane. Not drawn as an XY ring.");
        }
        if ui.small_button("Copy circle dimensions").clicked() {
            ui.ctx().copy_text(format!("{source}\nSHA-256 {sha256}\n{} · CIRCLE · decoded handle #{} · {units}\nWCS centre ({x}, {y}, {z}) · normal ({nx}, {ny}, {nz})\nRadius {} · diameter {}", layer.name, c.handle, c.radius, 2. * c.radius));
        }
    }
}

fn plan(ui: &mut Ui, layers: &[Layer], active: usize, selected: &mut usize) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut count = 0;
    for c in layers
        .iter()
        .flat_map(|l| &l.circles)
        .filter(|c| c.parallel())
    {
        for d in 0..2 {
            lo[d] = lo[d].min(c.center[d] - c.radius);
            hi[d] = hi[d].max(c.center[d] + c.radius);
        }
        count += 1;
    }
    ui.small("Circle plan · +X right, +Y up");
    if count == 0 {
        ui.weak("No XY-parallel circles to plot.");
        return;
    }
    if count > 1024 {
        ui.weak("Plan hidden above 1,024 rings. Use the circle selector for the dimensions.");
        return;
    }
    let width = ui.available_width().max(40.);
    let height = (ui.available_height() - 30.).clamp(96., 245.);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let p = theme::palette(ui);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 5., p.canvas);
    let field = rect.shrink(10.);
    let scale = (f64::from(field.width()) / (hi[0] - lo[0]))
        .min(f64::from(field.height()) / (hi[1] - lo[1]));
    if !scale.is_finite() || scale <= 0. {
        ui.weak("Circle extents cannot be shown at this scale.");
        return;
    }
    let project = |c: &Circle| -> (Pos2, f32) {
        let dx = ((c.center[0] - lo[0]) - (hi[0] - lo[0]) / 2.) * scale;
        let dy = ((hi[1] - lo[1]) / 2. - (c.center[1] - lo[1])) * scale;
        (
            field.center() + egui::vec2(dx as f32, dy as f32),
            (c.radius * scale) as f32,
        )
    };
    let pointer = response
        .interact_pointer_pos()
        .filter(|_| response.clicked());
    let mut nearest: Option<(f32, usize)> = None;
    let accent = p.ink(Color32::from_rgb(67, 191, 168));
    // Paint context first so coincident entities never hide the selected layer.
    for l in (0..layers.len())
        .filter(|&i| i != active)
        .chain(std::iter::once(active))
    {
        let color = if l == active {
            accent
        } else {
            p.muted.gamma_multiply(0.55)
        };
        for (i, c) in layers[l]
            .circles
            .iter()
            .enumerate()
            .filter(|(_, c)| c.parallel())
        {
            let (center, r) = project(c);
            painter.circle_stroke(center, r, Stroke::new(1.5, color));
            if let Some(pos) = pointer.filter(|_| l == active) {
                let distance = (pos.distance(center) - r).abs().min(pos.distance(center));
                if distance <= 7. && nearest.is_none_or(|(d, _)| distance < d) {
                    nearest = Some((distance, i));
                }
            }
        }
    }
    if let Some((_, i)) = nearest {
        *selected = i;
    }
    if let Some(c) = layers[active]
        .circles
        .get(*selected)
        .filter(|c| c.parallel())
    {
        let (center, r) = project(c);
        painter.circle_stroke(center, r, Stroke::new(2., p.accent));
        let mark = Rect::from_center_size(center, egui::vec2(9., 9.));
        painter.line_segment(
            [mark.center_top(), mark.center_bottom()],
            Stroke::new(1., p.accent),
        );
        painter.line_segment(
            [mark.left_center(), mark.right_center()],
            Stroke::new(1., p.accent),
        );
    }
    response
        .on_hover_text("Select a circle on the active layer. Dim rings belong to other layers.");
    ui.small(
        RichText::new("XY-parallel rings only · other layers dimmed · not print scale").weak(),
    );
}
