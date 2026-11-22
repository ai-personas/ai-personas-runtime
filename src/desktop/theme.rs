//! Shared visual language for the desktop editors and their controls.
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, Ui};

pub const ACCENT: Color32 = Color32::from_rgb(225, 139, 94);
pub const BLUE: Color32 = Color32::from_rgb(46, 94, 163);
pub const GREEN: Color32 = Color32::from_rgb(120, 191, 153);
pub const BODY_TEXT: f32 = 16.0;
pub const SMALL_TEXT: f32 = 13.0;
pub const MONO_TEXT: f32 = 14.0;
pub const MIN_ZOOM: f32 = 0.75;
pub const MAX_ZOOM: f32 = 3.0;

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: Color32,
    pub panel: Color32,
    pub header: Color32,
    pub field: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub hover: Color32,
    pub selection: Color32,
    pub accent: Color32,
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                canvas: Color32::from_rgb(28, 30, 33),
                panel: Color32::from_rgb(39, 42, 45),
                header: Color32::from_rgb(49, 52, 55),
                field: Color32::from_rgb(31, 33, 36),
                border: Color32::from_rgb(65, 69, 74),
                text: Color32::from_rgb(240, 239, 230),
                muted: Color32::from_rgb(181, 186, 190),
                hover: Color32::from_rgb(57, 61, 65),
                selection: Color32::from_rgb(77, 60, 51),
                accent: Color32::from_rgb(244, 174, 139),
            }
        } else {
            Self {
                canvas: Color32::from_rgb(246, 245, 240),
                panel: Color32::from_rgb(255, 254, 250),
                header: Color32::from_rgb(239, 237, 229),
                field: Color32::from_rgb(248, 247, 243),
                border: Color32::from_rgb(214, 213, 203),
                text: Color32::from_rgb(40, 43, 38),
                muted: Color32::from_rgb(92, 96, 86),
                hover: Color32::from_rgb(231, 230, 221),
                selection: Color32::from_rgb(241, 220, 205),
                accent: Color32::from_rgb(144, 63, 41),
            }
        }
    }

    /// Preserve each status or persona's hue while keeping small labels legible.
    pub fn ink(self, color: Color32) -> Color32 {
        let luminance = |rgb: egui::Rgba| 0.2126 * rgb.r() + 0.7152 * rgb.g() + 0.0722 * rgb.b();
        let foreground = egui::Rgba::from(color);
        let surface = luminance(self.header.into());
        let value = luminance(foreground);
        let contrast = 5.5;
        if (value.max(surface) + 0.05) >= contrast * (value.min(surface) + 0.05) {
            return color;
        }
        let text = egui::Rgba::from(self.text);
        let target = if luminance(text) > surface {
            contrast * (surface + 0.05) - 0.05
        } else {
            (surface + 0.05) / contrast - 0.05
        };
        let mix = ((target - value) / (luminance(text) - value)).clamp(0.0, 1.0);
        (foreground * (1.0 - mix) + text * mix).into()
    }
}
pub fn palette(ui: &Ui) -> Palette {
    Palette::new(ui.visuals().dark_mode)
}
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes, family) in [
        (
            "Inter",
            include_bytes!("fonts/Inter-Regular.ttf").as_slice(),
            egui::FontFamily::Proportional,
        ),
        (
            "DejaVu Sans Mono",
            include_bytes!("fonts/DejaVuSansMono.ttf").as_slice(),
            egui::FontFamily::Monospace,
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(bytes).into());
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, name.into());
    }
    let display = "Instrument Serif";
    fonts.font_data.insert(display.into(), egui::FontData::from_static(include_bytes!("fonts/InstrumentSerif-Regular.ttf")).into());
    let mut faces = fonts.families[&egui::FontFamily::Proportional].clone();
    faces.insert(0, display.into());
    fonts.families.insert(egui::FontFamily::Name("Studio display".into()), faces);
    ctx.set_fonts(fonts);
}
pub fn apply(ctx: &egui::Context, dark: bool, zoom: f32) {
    let p = Palette::new(dark);
    let mut style = egui::Style::default();
    style.visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.spacing.item_spacing = egui::vec2(7.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size = egui::vec2(28.0, 28.0);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.indent = 16.0;
    style.spacing.scroll.bar_width = 7.0;
    for (text, size) in [
        (egui::TextStyle::Body, BODY_TEXT),
        (egui::TextStyle::Button, 15.0),
        (egui::TextStyle::Small, SMALL_TEXT),
        (egui::TextStyle::Heading, 25.0),
    ] {
        style.text_styles.insert(text, FontId::proportional(size));
    }
    style.text_styles.insert(egui::TextStyle::Heading, FontId::new(28., egui::FontFamily::Name("Studio display".into())));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(MONO_TEXT));
    let v = &mut style.visuals;
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.field;
    v.text_edit_bg_color = Some(p.field);
    v.code_bg_color = p.field;
    v.faint_bg_color = p.accent.gamma_multiply(0.05);
    v.weak_text_color = Some(p.muted);
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0_f32, if dark { Color32::WHITE } else { p.text });
    v.hyperlink_color = p.accent;
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.warn_fg_color = p.ink(ACCENT);
    v.error_fg_color = p.ink(Color32::from_rgb(237, 133, 130));
    v.window_corner_radius = egui::CornerRadius::same(9);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.bg_fill = p.panel;
    v.widgets.noninteractive.weak_bg_fill = p.header;
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(5);
    v.widgets.inactive.weak_bg_fill = p.header;
    v.widgets.inactive.bg_fill = p.header;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border.lerp_to_gamma(p.muted, 0.55));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.active.weak_bg_fill = p.selection;
    v.widgets.active.bg_fill = p.selection;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    let strong = if dark {
        Color32::from_rgb(250, 249, 243)
    } else {
        Color32::from_rgb(36, 36, 31)
    };
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, strong);
    for widget in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(5);
        widget.expansion = 0.0;
    }
    v.widgets.open.weak_bg_fill = p.selection;
    v.widgets.open.bg_fill = p.selection;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.accent);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = true;
    ctx.set_style(style);
    ctx.set_zoom_factor(if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 });
}

pub fn panel(ctx: &egui::Context) -> egui::Frame {
    let p = Palette::new(ctx.style().visuals.dark_mode);
    egui::Frame::new()
        .fill(p.panel)
        .inner_margin(8)
        .stroke(Stroke::new(1.0_f32, p.border))
}
pub fn header(ui: &mut Ui, icon: Icon, title: &str, detail: &str) {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.header)
        .inner_margin(egui::Margin::symmetric(9, 6))
        .corner_radius(5)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                glyph(ui, icon, p.muted);
                ui.label(RichText::new(title).size(BODY_TEXT).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(detail).size(SMALL_TEXT).color(p.muted));
                });
            });
        });
}
pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let color = palette(ui).ink(color);
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(0.75, color.gamma_multiply(0.28)))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(6)
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(SMALL_TEXT).color(color));
        });
}
pub fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(Color32::from_rgb(35, 29, 26)).strong())
        .fill(Color32::from_rgb(235, 155, 111))
        .stroke(Stroke::NONE)
        .wrap_mode(egui::TextWrapMode::Extend)
}
pub fn property(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.add_sized(
            [100.0, 24.0],
            egui::Label::new(RichText::new(label).small().weak()),
        );
        ui.add(egui::Label::new(value).wrap());
    });
}

#[derive(Clone, Copy)]
pub enum Icon {
    Search,
    Map,
    Workspaces,
    Folder,
    Task,
    People,
    Conversation,
    Files,
    Questions,
    Activity,
    Sessions,
    Refresh,
    Settings,
}
pub fn glyph(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    paint_icon(ui.painter(), rect, icon, color);
}
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let at = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let stroke = Stroke::new(1.4_f32, color);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    let box_at = |x, y, w, h| {
        painter.rect_stroke(
            Rect::from_min_max(at(x, y), at(x + w, y + h)),
            1,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Map => {
            line(&[(0.08,0.22), (0.35,0.12), (0.65,0.26), (0.92,0.15), (0.92,0.8), (0.65,0.91), (0.35,0.77), (0.08,0.88), (0.08,0.22)]);
            line(&[(0.35,0.12), (0.35,0.77)]);
            line(&[(0.65,0.26), (0.65,0.91)]);
        }
        Icon::Search => {
            painter.circle_stroke(at(0.43, 0.42), rect.width() * 0.28, stroke);
            line(&[(0.63, 0.63), (0.91, 0.91)]);
        }
        Icon::Workspaces => {
            for (x, y) in [(0.1, 0.1), (0.57, 0.1), (0.1, 0.57), (0.57, 0.57)] {
                box_at(x, y, 0.33, 0.33);
            }
        }
        Icon::Folder => {
            line(&[
                (0.1, 0.8),
                (0.1, 0.22),
                (0.4, 0.22),
                (0.5, 0.35),
                (0.9, 0.35),
                (0.9, 0.8),
                (0.1, 0.8),
            ]);
        }
        Icon::Task | Icon::Files => {
            line(&[
                (0.22, 0.9),
                (0.22, 0.1),
                (0.65, 0.1),
                (0.82, 0.3),
                (0.82, 0.9),
                (0.22, 0.9),
            ]);
            line(&[(0.36, 0.48), (0.68, 0.48)]);
            line(&[(0.36, 0.66), (0.68, 0.66)]);
        }
        Icon::People => {
            painter.circle_stroke(at(0.5, 0.3), rect.width() * 0.17, stroke);
            line(&[
                (0.16, 0.88),
                (0.21, 0.67),
                (0.38, 0.57),
                (0.62, 0.57),
                (0.79, 0.67),
                (0.84, 0.88),
            ]);
        }
        Icon::Conversation | Icon::Questions => {
            line(&[
                (0.1, 0.17),
                (0.9, 0.17),
                (0.9, 0.7),
                (0.45, 0.7),
                (0.24, 0.9),
                (0.24, 0.7),
                (0.1, 0.7),
                (0.1, 0.17),
            ]);
            line(&[(0.28, 0.38), (0.72, 0.38)]);
            line(&[(0.28, 0.52), (0.56, 0.52)]);
        }
        Icon::Activity => {
            line(&[
                (0.05, 0.55),
                (0.28, 0.55),
                (0.4, 0.16),
                (0.57, 0.88),
                (0.7, 0.4),
                (0.8, 0.55),
                (0.95, 0.55),
            ]);
        }
        Icon::Sessions => {
            painter.circle_stroke(at(0.5, 0.5), rect.width() * 0.37, stroke);
            line(&[(0.5, 0.23), (0.5, 0.5), (0.7, 0.62)]);
        }
        Icon::Refresh => {
            line(&[
                (0.83, 0.38),
                (0.7, 0.2),
                (0.4, 0.14),
                (0.16, 0.33),
                (0.13, 0.64),
                (0.37, 0.85),
                (0.68, 0.8),
                (0.83, 0.64),
            ]);
            line(&[(0.84, 0.13), (0.84, 0.39), (0.58, 0.39)]);
        }
        Icon::Settings => {
            for y in [0.25, 0.5, 0.75] {
                line(&[(0.1, y), (0.9, y)]);
            }
            for (x, y) in [(0.34, 0.25), (0.65, 0.5), (0.43, 0.75)] {
                painter.circle_filled(at(x, y), rect.width() * 0.08, color);
            }
        }
    }
}
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 5, visuals.weak_bg_fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            5,
            Stroke::new(1.0, palette(ui).accent),
            egui::StrokeKind::Inside,
        );
    }
    paint_icon(
        ui.painter(),
        rect.shrink(5.0),
        icon,
        visuals.fg_stroke.color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

/// Shared document formatting preserves the retained source text.
pub fn message_body(ui: &mut Ui, text: &str) {
    super::document::markdown(ui, text);
}

pub fn continuation_label(value: &str) -> &str {
    match value {
        "persona_stop" => "Stopped on this task",
        "persona_pause" => "Paused",
        "immediate" => "Continuing",
        "wait" => "Waiting",
        "reconsider_at" => "Scheduled",
        "absent" => "No choice yet",
        _ => value,
    }
}
