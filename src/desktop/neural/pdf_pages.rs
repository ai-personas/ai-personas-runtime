//! Paging and local zoom for a single rendered PDF page.
use super::*;
use crate::desktop::page_view::{self, Orientation};

pub(super) use crate::desktop::page_view::SAMPLING;

pub(super) fn render(
    bytes: &[u8],
    page: u32,
    max_side: usize,
    cancelled: impl FnMut() -> bool,
) -> ai_personas::neural::Result<(egui::ColorImage, u32)> {
    let raster = ai_personas::pdf_preview::render_with_control(bytes, page, cancelled)?;
    let mut image = image::load_from_memory_with_format(&raster.png, image::ImageFormat::Png)?;
    let side = max_side.clamp(1, 2400) as u32;
    if image.width().max(image.height()) > side {
        image = image.thumbnail(side, side);
    }
    let image = image.into_rgba8();
    let pixels = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    Ok((pixels, raster.page_count))
}

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
    pub height_limit: Option<f32>,
    zoom: Zoom,
    page_jump: String,
    orientation: Orientation,
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            page: 1,
            count: None,
            height_limit: None,
            zoom: Default::default(),
            page_jump: String::new(),
            orientation: Orientation::Original,
        }
    }
}

impl Pages {
    pub fn navigation(&mut self, ui: &mut Ui, reference: &str) -> bool {
        let original = self.page;
        ui.push_id(("pdf-reader", reference), |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut number = self.page as usize;
                page_view::navigation(
                    ui,
                    &mut number,
                    self.count.map(|n| n as usize),
                    &mut self.page_jump,
                );
                self.page = number as u32;
                egui::ComboBox::from_id_salt("delivery-pdf-zoom")
                    .width(90.)
                    .selected_text(self.zoom.label())
                    .show_ui(ui, |ui| {
                        for zoom in [Zoom::Page, Zoom::Width, Zoom::TwiceWidth] {
                            ui.selectable_value(&mut self.zoom, zoom, zoom.label());
                        }
                    })
                    .response
                    .on_hover_text("Page zoom");
                self.orientation.control(ui);
            })
        });
        self.page != original
    }

    pub fn loading(&self, ui: &mut Ui) {
        let frame = page_frame(ui);
        frame.show(ui, |ui| {
            let size = viewport(ui, self.height_limit);
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
            let available = viewport(ui, self.height_limit);
            let pixels = self.orientation.size(texture.size_vec2());
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
                let (rect, response) = ui.allocate_exact_size(area, egui::Sense::hover());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Image,
                        true,
                        format!("{path}, page {}", self.page),
                    )
                });
                let bounds = egui::Rect::from_center_size(rect.center(), image_size);
                self.orientation.paint(ui.painter(), texture.id(), bounds);
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
                    .id_salt((
                        "delivery-pdf-scroll",
                        reference,
                        self.page,
                        self.zoom,
                        self.orientation,
                    ))
                    .max_width(available.x)
                    .max_height(available.y)
                    .min_scrolled_height(available.y)
                    .auto_shrink([false, false])
                    .show(ui, draw);
            }
        });
    }
}

fn viewport(ui: &Ui, height_limit: Option<f32>) -> egui::Vec2 {
    let width = ui.available_width();
    let height = height_limit.unwrap_or_else(|| {
        (width * 0.8)
            .clamp(170., 455.)
            .min(ui.ctx().content_rect().height() * 0.54)
    });
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
