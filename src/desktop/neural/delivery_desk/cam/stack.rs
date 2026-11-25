//! Draw the registered film images. Hiding layers never changes the frame.
use super::{
    plot::{self, Archive, Bounds, Info, Plot},
    theme,
};
use eframe::egui::{self, Color32, Rect, RichText, Sense, Ui};
use std::{collections::BTreeSet, fmt::Write};

struct Layer {
    info: Info,
    hash: String,
    texture: egui::TextureHandle,
    color: Color32,
    visible: bool,
}

fn ink(info: &Info, i: usize) -> (u8, Color32) {
    let other = [
        [242, 164, 99],
        [76, 209, 201],
        [178, 148, 239],
        [238, 201, 98],
        [122, 190, 117],
        [112, 170, 235],
    ];
    let top = info.function.split(", ").any(|s| s == "Top");
    let bot = info.function.split(", ").any(|s| s == "Bot");
    let (order, [r, g, b]) = match info.function.split(", ").next() {
        _ if plot::format(&info.path) == Some("Excellon") => (4, [250, 251, 249]),
        Some("Profile") => (3, [195, 211, 227]),
        Some("Legend") => (2, [242, 205, 105]),
        Some("Copper") if top => (1, [245, 153, 101]),
        Some("Copper") if bot => (1, [92, 198, 221]),
        Some("Soldermask") if top => (0, [139, 204, 119]),
        Some("Soldermask") if bot => (0, [180, 149, 223]),
        _ => (1, other[i % other.len()]),
    };
    (order, Color32::from_rgb(r, g, b))
}
impl Info {
    fn description(&self) -> String {
        let role = if self.function.is_empty() {
            plot::format(&self.path).unwrap_or("CAM")
        } else {
            &self.function
        };
        if self.polarity == "Negative" {
            format!("{role} · negative film")
        } else {
            role.into()
        }
    }
    fn round_drills(&self) -> String {
        self.round_hits
            .iter()
            .map(|(diam, n)| format!("{n} × Ø{diam} mm"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}
fn extent([[l, b], [r, t]]: Bounds) -> String {
    format!("X {l:.6} to {r:.6}; Y {b:.6} to {t:.6} mm")
}

pub struct Stack {
    renderer: String,
    bounds: Bounds,
    layers: Vec<Layer>,
    zoom: f32,
    pan: egui::Vec2,
}
impl Stack {
    pub fn new(ctx: &egui::Context, p: Plot) -> Self {
        let mut layers: Vec<_> = p
            .layers
            .into_iter()
            .enumerate()
            .map(|(i, (info, hash, image))| {
                let (order, color) = ink(&info, i);
                let texture =
                    ctx.load_texture(format!("cam-{hash}"), image, egui::TextureOptions::LINEAR);
                (
                    order,
                    Layer {
                        info,
                        hash,
                        texture,
                        color,
                        visible: true,
                    },
                )
            })
            .collect();
        layers.sort_by_key(|(order, _)| *order); // paint order, not a physical stackup
        Self {
            renderer: p.renderer,
            bounds: p.bounds,
            layers: layers.into_iter().map(|(_, l)| l).collect(),
            zoom: 1.,
            pan: egui::Vec2::ZERO,
        }
    }
    pub fn details(&self, archive: &Archive) -> String {
        let src = &archive.source;
        let mut out = format!("CAM overlay · {}\nIssued by {}: {}\n{}\n{} · {}\nZIP SHA-256: {}\nPlot bounds (including stroke widths): {}\nSelected layers in paint order; bottom files are not mirrored.\n", self.renderer, src.at_issue.issuer, src.title, src.edition, src.path, src.file, src.sha256, extent(self.bounds));
        for l in &self.layers {
            let _ = writeln!(
                out,
                "{}  {}  {}",
                if l.visible { "shown" } else { "hidden" },
                l.hash,
                l.info.path
            );
            let _ = writeln!(
                out,
                "  {} · {} objects · {}",
                l.info.description(),
                l.info.objects,
                l.info.bounds.map_or_else(|| "empty".into(), extent)
            );
            if !l.info.round_hits.is_empty() {
                let _ = writeln!(out, "  Round drill hits: {}", l.info.round_drills());
            }
            for note in &l.info.warnings {
                let _ = writeln!(out, "  Parser: {note}");
            }
        }
        let selected: BTreeSet<_> = self.layers.iter().map(|l| &l.info.path).collect();
        for path in &archive.paths {
            if !selected.contains(path) {
                let _ = writeln!(out, "not plotted  {path}");
            }
        }
        out + "Only recognized, selected Gerber/Excellon files are plotted. Local artwork inspection; no registration verdict, order, fabrication or physical receipt."
    }
    fn controls(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Visible layers");
            for (label, on) in [("All", true), ("None", false)] {
                if ui.small_button(label).clicked() {
                    for l in &mut self.layers {
                        l.visible = on;
                    }
                }
            }
        });
        for l in self.layers.iter_mut().rev() {
            let desc = l.info.description();
            let color = theme::palette(ui).ink(l.color);
            ui.checkbox(&mut l.visible, RichText::new(&l.info.path).color(color))
                .on_hover_text(format!(
                    "{desc} · {} objects\n{}\nSHA-256: {}",
                    l.info.objects,
                    l.info.bounds.map_or_else(|| "No geometry".into(), extent),
                    l.hash
                ));
            ui.small(if l.info.round_hits.is_empty() {
                desc
            } else {
                format!("Round hits: {}", l.info.round_drills())
            });
        }
    }
    fn fit(&mut self) {
        self.zoom = 1.;
        self.pan = egui::Vec2::ZERO;
    }
    fn zoom_by(&mut self, by: f32) {
        let zoom = (self.zoom * by).clamp(1., 24.);
        self.pan *= zoom / self.zoom;
        self.zoom = zoom;
    }
    fn canvas(&mut self, ui: &mut Ui, h: f32) {
        let [[x0, y0], [x1, y1]] = self.bounds;
        let (w, ht) = (x1 - x0, y1 - y0);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(1.), h),
            Sense::click_and_drag(),
        );
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.double_clicked() {
            self.fit();
        }
        let size = rect.shrink(12.).size();
        let scale =
            (f64::from(size.x) / w).min(f64::from(size.y) / ht).max(0.) * f64::from(self.zoom);
        let dest = Rect::from_center_size(
            rect.center() + self.pan,
            egui::vec2((w * scale) as f32, (ht * scale) as f32),
        );
        let paint = ui.painter_at(rect);
        paint.rect_filled(rect, 5., Color32::from_rgb(19, 30, 36));
        let uv = Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.));
        for l in self.layers.iter().filter(|l| l.visible) {
            paint.image(l.texture.id(), dest, uv, l.color.gamma_multiply(0.86));
        }
        let visible = self.layers.iter().filter(|l| l.visible).count();
        if visible == 0 {
            paint.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No layers visible",
                egui::FontId::proportional(15.),
                Color32::LIGHT_GRAY,
            );
        }
        let cursor = resp
            .hover_pos()
            .filter(|pos| scale > 0. && dest.contains(*pos))
            .map(|p| {
                let x = x0 + f64::from(p.x - dest.left()) / scale;
                let y = y1 - f64::from(p.y - dest.top()) / scale;
                format!("X {x:.2}   Y {y:.2} mm")
            });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Fit").clicked() {
                self.fit();
            }
            if ui.small_button("−").clicked() {
                self.zoom_by(1. / 1.5);
            }
            if ui.small_button("+").clicked() {
                self.zoom_by(1.5);
            }
            ui.weak(format!(
                "{:.1}× · {visible}/{} shown",
                self.zoom,
                self.layers.len()
            ));
            ui.monospace(cursor.unwrap_or_else(|| "Drag to pan".into()));
        });
        ui.small(format!(
            "Plotted extent {w:.3} × {ht:.3} mm, including stroke widths."
        ));
    }
    pub fn show(&mut self, ui: &mut Ui, height: f32) {
        if ui.available_width() >= 735. {
            let width = ui.available_width();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(width * 0.64);
                    self.canvas(ui, height);
                });
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt("cam-legend")
                        .max_height(height + 60.)
                        .show(ui, |ui| self.controls(ui));
                });
            });
        } else {
            self.canvas(ui, height);
            egui::CollapsingHeader::new(format!("Layers · {}", self.layers.len()))
                .show(ui, |ui| self.controls(ui));
        }
        let notes = self
            .layers
            .iter()
            .map(|l| l.info.warnings.len())
            .sum::<usize>();
        if notes > 0 {
            egui::CollapsingHeader::new(format!("Parser notes · {notes}")).show(ui, |ui| {
                for l in &self.layers {
                    for note in &l.info.warnings {
                        ui.label(format!("{}: {note}", l.info.path));
                    }
                }
            });
        }
        ui.weak(format!("{} · Raster preview in file coordinates, X right and Y up. Colors show plotted artwork, not a physical stackup or fabrication check.",self.renderer));
    }
}
