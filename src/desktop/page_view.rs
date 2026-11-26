//! Controls for reading a rendered page. The source and renderer never see the view transform.
use eframe::egui::{self, Color32, Painter, Rect, TextureId, Ui, Vec2};

// Keep fine printed lines visible when a large raster shrinks to fit the desk.
pub(super) const SAMPLING: egui::TextureOptions = egui::TextureOptions {
    mipmap_mode: Some(egui::TextureFilter::Linear),
    ..egui::TextureOptions::LINEAR
};

/// One-based PDF page index (not the page label printed on the sheet).
pub(super) fn navigation(ui: &mut Ui, page: &mut usize, count: Option<usize>, jump: &mut String) {
    let Some(count) = count else {
        ui.weak(format!("Page {page}"));
        return;
    };
    if count == 1 && *page == 1 {
        ui.weak("Page 1 of 1");
        return;
    }
    if ui
        .add_enabled(*page > 1, egui::Button::new("Previous").small())
        .clicked()
    {
        *page -= 1;
    }
    let choose = ui
        .small_button(format!("Page {page} of {count}…"))
        .on_hover_text("Go to a PDF page. Count the first sheet as page 1.");
    if choose.clicked() {
        jump.clear();
    }
    egui::Popup::from_toggle_button_response(&choose)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(155.)
        .show(|ui| {
            ui.vertical(|ui| {
                let label = ui.strong("Go to PDF page");
                let entry = ui
                    .add(
                        egui::TextEdit::singleline(jump)
                            .hint_text(format!("1–{count}"))
                            .char_limit(24)
                            .desired_width(ui.available_width()),
                    )
                    .labelled_by(label.id);
                if choose.clicked() {
                    entry.request_focus();
                }
                let number = jump
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=count).contains(n));
                if number.is_none() && !jump.is_empty() {
                    ui.label(format!("Enter a whole page number from 1 to {count}."));
                }
                let enter = entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .add_enabled(number.is_some(), egui::Button::new("Open page"))
                    .clicked()
                    || enter
                {
                    if let Some(target) = number {
                        *page = target;
                        ui.close();
                    } else {
                        entry.request_focus();
                    }
                }
            })
        });
    if ui
        .add_enabled(*page < count, egui::Button::new("Next").small())
        .clicked()
    {
        *page += 1;
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(super) enum Orientation {
    #[default]
    Original,
    Right,
    Half,
    Left,
}

impl Orientation {
    fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Right => "90° right",
            Self::Half => "180°",
            Self::Left => "90° left",
        }
    }

    pub fn control(&mut self, ui: &mut Ui) -> bool {
        let before = *self;
        egui::ComboBox::from_id_salt("page-orientation")
            .selected_text(format!("View: {}", self.label()))
            .show_ui(ui, |ui| {
                for turn in [Self::Original, Self::Right, Self::Half, Self::Left] {
                    ui.selectable_value(self, turn, turn.label());
                }
            })
            .response
            .on_hover_text("Turn the view, not the file. Applies while reading this document.");
        *self != before
    }

    pub fn size(self, pixels: Vec2) -> Vec2 {
        match self {
            Self::Right | Self::Left => egui::vec2(pixels.y, pixels.x),
            Self::Original | Self::Half => pixels,
        }
    }

    /// `bounds` is already fitted to the turned size. Map its four corners back
    /// to the source texture without resampling, copying pixels, or mirroring.
    pub fn paint(self, painter: &Painter, texture: TextureId, bounds: Rect) {
        let uv = Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.));
        let mut mesh = egui::Mesh::with_texture(texture);
        mesh.add_rect_with_uv(bounds, uv, Color32::WHITE);
        for v in &mut mesh.vertices {
            let (x, y) = (v.uv.x, v.uv.y);
            v.uv = match self {
                Self::Original => v.uv,
                Self::Right => egui::pos2(y, 1. - x),
                Self::Half => egui::pos2(1. - x, 1. - y),
                Self::Left => egui::pos2(1. - y, x),
            };
        }
        painter.add(mesh);
    }
}
