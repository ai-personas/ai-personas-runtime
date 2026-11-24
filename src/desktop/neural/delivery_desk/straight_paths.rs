//! Read resolved stroke centerlines in the same page space as the SVG renderer.
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui};
use resvg::{tiny_skia::PathSegment, usvg};
use sha2::{Digest, Sha256};

pub(super) struct Drawing {
    paths: Vec<Vec<Pos2>>,
    page: egui::Vec2,
    bounds: Rect,
    closed: usize,
    digest: String,
}

impl Drawing {
    pub fn inspect(tree: &usvg::Tree, bytes: &[u8]) -> Option<Self> {
        let mut drawing = Self {
            paths: Vec::new(),
            bounds: Rect::NOTHING,
            closed: 0,
            page: egui::vec2(tree.size().width(), tree.size().height()),
            digest: String::new(),
        };
        let mut remaining = 40_000;
        drawing.visit(tree.root(), &mut remaining)?;
        if drawing.paths.is_empty() {
            return None;
        }
        drawing.digest = format!("{:x}", Sha256::digest(bytes));
        Some(drawing)
    }

    fn visit(&mut self, group: &usvg::Group, remaining: &mut usize) -> Option<()> {
        if group.clip_path().is_some()
            || group.mask().is_some()
            || !group.filters().is_empty()
            || group.opacity().get() != 1.0
        {
            return None;
        }
        for node in group.children() {
            let path = match node {
                usvg::Node::Group(g) => {
                    self.visit(g, remaining)?;
                    continue;
                }
                usvg::Node::Path(p) if !p.is_visible() => continue,
                usvg::Node::Path(p) => p,
                _ => return None,
            };
            let stroke = path.stroke()?;
            if path.fill().is_some() || stroke.dasharray().is_some() || stroke.opacity().get() == 0.
            {
                return None;
            }
            let mapped = |mut p: resvg::tiny_skia::Point| {
                path.abs_transform().map_point(&mut p);
                egui::pos2(p.x, p.y)
            };
            let mut current = Vec::new();
            for segment in path.data().segments() {
                *remaining = remaining.checked_sub(1)?;
                match segment {
                    PathSegment::MoveTo(p) => {
                        self.finish(&mut current)?;
                        current.push(mapped(p));
                    }
                    PathSegment::LineTo(p) if !current.is_empty() => current.push(mapped(p)),
                    PathSegment::Close if !current.is_empty() => {
                        if current.first() != current.last() {
                            current.push(current[0]);
                        }
                        self.finish(&mut current)?;
                    }
                    _ => return None,
                }
            }
            self.finish(&mut current)?;
        }
        Some(())
    }

    fn finish(&mut self, path: &mut Vec<Pos2>) -> Option<()> {
        if path.is_empty() {
            return Some(());
        }
        if path.len() < 2 || self.paths.len() == 2048 {
            return None;
        }
        for p in path.iter() {
            if !p.is_finite() {
                return None;
            }
            self.bounds.extend_with(*p);
        }
        self.closed += usize::from(path.first() == path.last());
        self.paths.push(std::mem::take(path));
        Some(())
    }

    fn dimensions(&self) -> (String, String) {
        let mm = 25.4 / 96.;
        (
            format!(
                "{:.3} × {:.3} mm",
                self.bounds.width() * mm,
                self.bounds.height() * mm
            ),
            format!("{:.3} × {:.3} mm", self.page.x * mm, self.page.y * mm),
        )
    }

    pub fn controls(&self, ui: &mut Ui, selected: &mut bool, name: &str) {
        let (span, page) = self.dimensions();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(selected, false, "As drawn");
            ui.selectable_value(selected, true, "Centerlines");
            if ui.small_button("Copy dimensions").clicked() {
                ui.ctx().copy_text(format!("{name}\nSHA-256: {}\nSVG centerline bounds: {span}\nSVG page: {page}\nPaths: {} closed, {} open\nSVG page units at 96 px/in; no stroke width or tool offset included.\n",
                    self.digest, self.closed, self.paths.len()-self.closed));
            }
        });
        ui.small(format!(
            "Centerline bounds: {span} · {} closed · {} open",
            self.closed,
            self.paths.len() - self.closed
        ));
    }

    pub fn paint(&self, ui: &Ui, rect: Rect) {
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 5., Color32::from_rgb(32, 43, 51));
        let space = rect.shrink(15.).size();
        let scale = (space.x / self.page.x).min(space.y / self.page.y).max(0.);
        if scale == 0. {
            return;
        }
        let origin = rect.center() - self.page * scale * 0.5;
        // Clip to the SVG page, including drawings with paths extending beyond it.
        let ink = p.with_clip_rect(Rect::from_min_size(origin, self.page * scale).expand(1.));
        for contour in &self.paths {
            ink.add(egui::Shape::line(
                contour
                    .iter()
                    .map(|p| origin + p.to_vec2() * scale)
                    .collect(),
                Stroke::new(1.8, Color32::from_rgb(118, 221, 205)),
            ));
        }
    }
}
