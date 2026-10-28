//! Shared visual language for the desktop editors and their controls.
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, Ui};

pub const ACCENT: Color32 = Color32::from_rgb(232, 145, 63);
pub const BLUE: Color32 = Color32::from_rgb(71, 114, 179);
pub const GREEN: Color32 = Color32::from_rgb(125, 183, 137);

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
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                canvas: Color32::from_rgb(32, 33, 35),
                panel: Color32::from_rgb(43, 44, 47),
                header: Color32::from_rgb(51, 52, 55),
                field: Color32::from_rgb(27, 28, 30),
                border: Color32::from_rgb(23, 24, 26),
                text: Color32::from_rgb(216, 219, 224),
                muted: Color32::from_rgb(151, 156, 165),
                hover: Color32::from_rgb(63, 65, 70),
                selection: Color32::from_rgb(57, 83, 119),
            }
        } else {
            Self {
                canvas: Color32::from_rgb(238, 239, 241),
                panel: Color32::from_rgb(222, 224, 228),
                header: Color32::from_rgb(207, 210, 215),
                field: Color32::from_rgb(247, 248, 250),
                border: Color32::from_rgb(177, 182, 190),
                text: Color32::from_rgb(41, 45, 52),
                muted: Color32::from_rgb(92, 99, 111),
                hover: Color32::from_rgb(194, 202, 215),
                selection: Color32::from_rgb(185, 205, 234),
            }
        }
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
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.interact_size = egui::vec2(24.0, 22.0);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.indent = 16.0;
    style.spacing.scroll.bar_width = 7.0;
    for (text, size) in [
        (egui::TextStyle::Body, 12.0),
        (egui::TextStyle::Button, 11.5),
        (egui::TextStyle::Small, 10.5),
        (egui::TextStyle::Heading, 16.0),
    ] {
        style.text_styles.insert(text, FontId::proportional(size));
    }
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(11.0));
    let v = &mut style.visuals;
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.field;
    v.text_edit_bg_color = Some(p.field);
    v.code_bg_color = p.field;
    v.faint_bg_color = p.header.gamma_multiply(0.5);
    v.weak_text_color = Some(p.muted);
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0_f32, if dark { Color32::WHITE } else { p.text });
    v.hyperlink_color = if dark {
        Color32::from_rgb(133, 178, 238)
    } else {
        BLUE
    };
    v.warn_fg_color = ACCENT;
    v.error_fg_color = Color32::from_rgb(225, 122, 117);
    v.window_corner_radius = egui::CornerRadius::same(5);
    v.menu_corner_radius = egui::CornerRadius::same(3);
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(3);
    v.widgets.inactive.weak_bg_fill = p.header;
    v.widgets.inactive.bg_fill = p.header;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.muted.gamma_multiply(0.6));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.active.weak_bg_fill = if dark { BLUE } else { p.selection };
    v.widgets.active.bg_fill = if dark { BLUE } else { p.selection };
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, if dark { Color32::WHITE } else { p.text });
    for widget in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(3);
        widget.expansion = 0.0;
    }
    v.widgets.open.weak_bg_fill = p.selection;
    v.widgets.open.bg_fill = p.selection;
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.collapsing_header_frame = true;
    v.indent_has_left_vline = true;
    ctx.set_style(style);
    ctx.set_zoom_factor(zoom.clamp(0.75, 1.75));
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
        .inner_margin(egui::Margin::symmetric(7, 5))
        .corner_radius(3)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                glyph(ui, icon, p.muted);
                ui.label(RichText::new(title).size(12.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(detail).size(10.5).color(p.muted));
                });
            });
        });
}
pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let color = if ui.visuals().dark_mode {
        color
    } else {
        let shade = |channel: u8| (u16::from(channel) * 60 / 100) as u8;
        Color32::from_rgb(shade(color.r()), shade(color.g()), shade(color.b()))
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(3)
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.5).color(color));
        });
}
pub fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(Color32::WHITE))
        .fill(BLUE)
        .wrap_mode(egui::TextWrapMode::Extend)
}
pub fn property(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.add_sized(
            [68.0, 18.0],
            egui::Label::new(RichText::new(label).small().weak()),
        );
        ui.add(egui::Label::new(value).wrap());
    });
}

#[derive(Clone, Copy)]
pub enum Icon {
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
    let stroke = Stroke::new(1.2_f32, color);
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
    let (rect, response) = ui.allocate_exact_size(egui::vec2(25.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 3, visuals.weak_bg_fill);
    }
    paint_icon(
        ui.painter(),
        rect.shrink2(egui::vec2(5.0, 4.0)),
        icon,
        visuals.fg_stroke.color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

/// Readable text formatting without HTML, network assets, or executable content.
/// Unsupported Markdown remains visible as text; retained messages are unchanged.
pub fn message_body(ui: &mut Ui, text: &str) {
    let mut paragraph = String::new();
    let mut code = String::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.starts_with("```") {
            if in_code {
                code_block(ui, &code);
                code.clear();
            } else if !paragraph.is_empty() {
                rich_paragraph(ui, paragraph.trim_end(), 13.5);
                paragraph.clear();
            }
            in_code = !in_code;
        } else if in_code {
            code.push_str(line);
            code.push('\n');
        } else if line.trim().is_empty() {
            if !paragraph.is_empty() {
                rich_paragraph(ui, paragraph.trim_end(), 13.5);
                paragraph.clear();
                ui.add_space(3.0);
            }
        } else if line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ") {
            if !paragraph.is_empty() {
                rich_paragraph(ui, paragraph.trim_end(), 13.5);
                paragraph.clear();
            }
            ui.add_space(4.0);
            rich_paragraph(ui, line.trim_start_matches('#').trim_start(), 16.0);
        } else {
            paragraph.push_str(line);
            paragraph.push('\n');
        }
    }
    if !paragraph.is_empty() {
        rich_paragraph(ui, paragraph.trim_end(), 13.5);
    }
    if in_code {
        code_block(ui, &code);
    }
}
fn code_block(ui: &mut Ui, code: &str) {
    egui::Frame::new()
        .fill(palette(ui).field)
        .inner_margin(9)
        .corner_radius(3)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(code.trim_end()).monospace().size(11.5))
                    .wrap()
                    .selectable(true),
            );
        });
}
fn rich_paragraph(ui: &mut Ui, text: &str, size: f32) {
    let p = palette(ui);
    let plain = egui::TextFormat {
        font_id: FontId::proportional(size),
        color: p.text,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let mut rest = text;
    while !rest.is_empty() {
        let next = [(rest.find("**"), "**"), (rest.find('`'), "`")]
            .into_iter()
            .filter_map(|(at, mark)| at.map(|at| (at, mark)))
            .min_by_key(|(at, _)| *at);
        let Some((at, mark)) = next else {
            job.append(rest, 0.0, plain.clone());
            break;
        };
        let after = &rest[at + mark.len()..];
        let Some(end) = after.find(mark) else {
            job.append(rest, 0.0, plain.clone());
            break;
        };
        job.append(&rest[..at], 0.0, plain.clone());
        let mut format = plain.clone();
        if mark == "`" {
            format.font_id = FontId::monospace(size - 1.0);
            format.background = p.field;
        } else {
            format.font_id.size += 0.5;
            format.color = ui.visuals().strong_text_color();
        }
        job.append(&after[..end], 0.0, format);
        rest = &after[end + mark.len()..];
    }
    ui.add(egui::Label::new(job).wrap().selectable(true));
}
