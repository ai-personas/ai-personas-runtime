//! Paging and local zoom for a single rendered PDF page.
use super::*;

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash)]
enum Zoom {
    #[default]
    Page,
    Width,
    TwiceWidth,
}

impl Zoom {
    fn label(self) -> &'static str {
        match self {
            Self::Page => "Fit page",
            Self::Width => "Fit width",
            Self::TwiceWidth => "2× width",
        }
    }
}

pub(super) struct Pages {
    pub page: u32,
    pub count: Option<u32>,
    zoom: Zoom,
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            page: 1,
            count: None,
            zoom: Default::default(),
        }
    }
}

impl Pages {
    pub fn navigation(&mut self, ui: &mut Ui, reference: &str) -> bool {
        let original = self.page;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.page > 1, egui::Button::new("Previous").small())
                .on_hover_text("Previous PDF page")
                .clicked()
            {
                self.page -= 1;
            }
            let position = self.count.map_or_else(
                || format!("Page {}", self.page),
                |n| format!("Page {} of {n}", self.page),
            );
            ui.label(RichText::new(position).small().strong());
            if ui
                .add_enabled(
                    self.count.is_some_and(|n| self.page < n),
                    egui::Button::new("Next").small(),
                )
                .on_hover_text("Next PDF page")
                .clicked()
            {
                self.page += 1;
            }
            egui::ComboBox::from_id_salt(("delivery-pdf-zoom", reference))
                .width(90.)
                .selected_text(self.zoom.label())
                .show_ui(ui, |ui| {
                    for zoom in [Zoom::Page, Zoom::Width, Zoom::TwiceWidth] {
                        ui.selectable_value(&mut self.zoom, zoom, zoom.label());
                    }
                })
                .response
                .on_hover_text("Page zoom");
        });
        self.page != original
    }

    pub fn loading(&self, ui: &mut Ui) {
        let frame = page_frame(ui);
        frame.show(ui, |ui| {
            let size = viewport(ui);
            ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.set_min_size(size);
                ui.add_space((size.y * 0.40).max(0.));
                ui.spinner();
                ui.weak(format!("Loading page {}…", self.page));
            });
        });
    }

    pub fn show(&self, ui: &mut Ui, texture: &egui::TextureHandle, path: &str, reference: &str) {
        page_frame(ui).show(ui, |ui| {
            let available = viewport(ui);
            let pixels = texture.size_vec2();
            let width_scale = (available.x - 14.).max(1.) / pixels.x;
            let scale = match self.zoom {
                Zoom::Page => width_scale.min((available.y - 14.).max(1.) / pixels.y),
                Zoom::Width => width_scale.min(2.),
                Zoom::TwiceWidth => (width_scale * 2.).min(2.),
            };
            let image_size = pixels * scale;
            let draw = |ui: &mut Ui| {
                let area = egui::vec2(
                    available.x.max(image_size.x + 12.),
                    available.y.max(image_size.y + 12.),
                );
                let (rect, _) = ui.allocate_exact_size(area, egui::Sense::hover());
                let bounds = egui::Rect::from_center_size(rect.center(), image_size);
                ui.place(
                    bounds,
                    egui::Image::new((texture.id(), pixels))
                        .fit_to_exact_size(image_size)
                        .alt_text(format!("{path}, page {}", self.page)),
                );
                ui.painter().rect_stroke(
                    bounds,
                    0.,
                    Stroke::new(1., theme::palette(ui).border),
                    egui::StrokeKind::Outside,
                );
            };
            if self.zoom == Zoom::Page {
                draw(ui);
            } else {
                let scroll = if self.zoom == Zoom::Width {
                    egui::ScrollArea::vertical()
                } else {
                    egui::ScrollArea::both()
                };
                scroll
                    .id_salt(("delivery-pdf-scroll", reference, self.page, self.zoom))
                    .max_width(available.x)
                    .max_height(available.y)
                    .min_scrolled_height(available.y)
                    .auto_shrink([false, false])
                    .show(ui, draw);
            }
        });
    }
}

fn viewport(ui: &Ui) -> egui::Vec2 {
    let width = ui.available_width();
    let height = (width * 0.8)
        .clamp(170., 455.)
        .min(ui.ctx().content_rect().height() * 0.54);
    egui::vec2(width, height)
}

fn page_frame(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(p.header)
        .stroke(Stroke::new(1., p.border))
        .corner_radius(5)
        .inner_margin(2)
}
