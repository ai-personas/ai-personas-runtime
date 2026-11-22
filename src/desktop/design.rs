//! Studio navigation and editorial surfaces shared by the launcher and workspace.
use super::theme::{self, Icon};
use eframe::egui::{self, Align, Color32, FontFamily, FontId, Pos2, Rect, RichText, Stroke, Ui};

pub const RAIL_WIDTH: f32 = 202.;
pub const INK: Color32 = Color32::from_rgb(37, 36, 34);
const RAIL: Color32 = Color32::from_rgb(25, 27, 29);
const RAIL_TEXT: Color32 = Color32::from_rgb(242, 240, 231);
const RAIL_MUTED: Color32 = Color32::from_rgb(181, 182, 172);
const CORAL: Color32 = Color32::from_rgb(245, 149, 113);

pub fn display(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name("Studio display".into())))
}

pub fn caption(text: &str, color: Color32) -> RichText {
    RichText::new(text.to_uppercase())
        .monospace()
        .size(11.)
        .extra_letter_spacing(1.1)
        .color(color)
}

pub fn brand(ui: &mut Ui) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(32., 32.), egui::Sense::hover());
        ui.painter().rect_filled(r, 7, CORAL);
        for i in 0..3 {
            let x = r.left() + 8. + i as f32 * 5.8;
            let y = r.top() + 8. + (2 - i) as f32 * 2.2;
            let points = vec![
                egui::pos2(x, y + 11.),
                egui::pos2(x, y + 2.),
                egui::pos2(x + 3.8, y),
                egui::pos2(x + 3.8, y + 9.),
            ];
            ui.painter()
                .add(egui::Shape::convex_polygon(points, INK, Stroke::NONE));
        }
        ui.add_space(2.);
        ui.label(
            RichText::new("Persona Studio")
                .color(RAIL_TEXT)
                .strong()
                .size(19.),
        );
    });
}

pub fn rail(
    ctx: &egui::Context,
    id: &'static str,
    footer: &str,
    foot_detail: &str,
    contents: impl FnOnce(&mut Ui),
) {
    egui::SidePanel::left(id)
        .exact_width(RAIL_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(RAIL)
                .inner_margin(egui::Margin::symmetric(14, 22)),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6., 3.);
            brand(ui);
            ui.add_space(16.);
            egui::TopBottomPanel::bottom(egui::Id::new((id, "footer")))
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    ui.add_space(10.);
                    ui.painter().hline(
                        ui.max_rect().x_range(),
                        ui.cursor().min.y,
                        Stroke::new(1., Color32::from_gray(68)),
                    );
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(30., 30.), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(r, 7, Color32::from_rgb(54, 57, 50));
                        theme::paint_icon(ui.painter(), r.shrink(7.), Icon::People, RAIL_TEXT);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.;
                            ui.add(
                                egui::Label::new(RichText::new(footer).color(RAIL_TEXT).size(14.))
                                    .truncate(),
                            )
                            .on_hover_text(footer);
                            ui.label(RichText::new(foot_detail).color(RAIL_MUTED).size(11.));
                        });
                    });
                });
            egui::ScrollArea::vertical()
                .id_salt((id, "navigation"))
                .auto_shrink([false, true])
                .show(ui, contents);
        });
}

pub fn rail_group(ui: &mut Ui, name: &str) {
    ui.add_space(14.);
    ui.horizontal(|ui| {
        ui.add_space(7.);
        ui.label(caption(name, RAIL_MUTED));
    });
    ui.add_space(5.);
}

pub fn rail_link(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let (r, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            r,
            6,
            if selected {
                Color32::from_rgb(60, 61, 54)
            } else {
                Color32::from_rgb(47, 49, 46)
            },
        );
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(r, 6, Stroke::new(1., CORAL), egui::StrokeKind::Inside);
    }
    if selected {
        ui.painter().rect_filled(
            Rect::from_center_size(
                egui::pos2(r.left() + 1.5, r.center().y),
                egui::vec2(3., 15.),
            ),
            2,
            CORAL,
        );
    }
    let center = egui::pos2(r.left() + 19., r.center().y);
    theme::paint_icon(
        ui.painter(),
        Rect::from_center_size(center, egui::vec2(16., 16.)),
        icon,
        if selected { CORAL } else { RAIL_MUTED },
    );
    ui.painter().text(
        egui::pos2(r.left() + 36., center.y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.),
        if selected {
            RAIL_TEXT
        } else {
            Color32::from_rgb(213, 212, 204)
        },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn breadcrumb(ui: &mut Ui, parent: &str, page: &str) {
    let p = theme::palette(ui);
    ui.horizontal(|ui| {
        theme::glyph(ui, Icon::Workspaces, p.muted);
        ui.label(RichText::new(parent).size(13.).color(p.muted));
        ui.label(RichText::new("/").size(13.).color(p.muted));
        ui.add(egui::Label::new(RichText::new(page).size(13.).strong()).truncate())
            .on_hover_text(page);
    });
}

/// The picture sits behind real, keyboard-accessible widgets. Measure the content
/// first so long workspace names and translated labels can grow the cover.
pub fn cover(ui: &mut Ui, label: &str, title: &str, detail: &str, actions: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let dark = ui.visuals().dark_mode;
    let heading = if dark {
        Color32::from_rgb(247, 240, 225)
    } else {
        INK
    };
    let muted = if dark {
        Color32::from_rgb(194, 192, 185)
    } else {
        Color32::from_rgb(71, 59, 47)
    };
    let small = width < 520.;
    let full = !detail.is_empty() && !small;
    let inset = if small {
        16
    } else if full {
        32
    } else {
        24
    };
    let radius = 12;
    let mut frame = egui::Frame::new()
        .fill(if dark {
            Color32::from_rgb(34, 37, 39)
        } else {
            Color32::from_rgb(250, 226, 199)
        })
        .corner_radius(radius)
        .inner_margin(egui::Margin::symmetric(
            inset,
            if small {
                14
            } else if full {
                32
            } else {
                16
            },
        ))
        .begin(ui);
    let body = &mut frame.content_ui;
    let painter = body.painter().clone();
    let cover = painter.add(egui::Shape::Noop);
    body.set_min_width(body.available_width());
    if full {
        body.set_min_height(190.);
    }
    let text_width = if small {
        body.available_width()
    } else {
        body.available_width() * 0.51
    };
    body.vertical(|ui| {
        ui.set_width(text_width);
        ui.visuals_mut().override_text_color = Some(heading);
        ui.visuals_mut().weak_text_color = Some(muted);
        ui.spacing_mut().item_spacing.y = 3.;
        ui.label(caption(
            label,
            if dark {
                CORAL
            } else {
                Color32::from_rgb(109, 64, 47)
            },
        ));
        ui.add_space(if small { 2. } else { 7. });
        ui.add(
            egui::Label::new(
                display(
                    title,
                    if small {
                        36.
                    } else if full {
                        61.
                    } else {
                        50.
                    },
                )
                .color(heading),
            )
            .wrap(),
        );
        if !detail.is_empty() {
            ui.add_space(3.);
            ui.add(egui::Label::new(RichText::new(detail).size(15.).color(muted)).wrap());
        }
        ui.add_space(if small { 10. } else { 15. });
        ui.scope(|ui| {
            ui.visuals_mut().override_text_color = None;
            let v = &mut ui.visuals_mut().widgets;
            if !dark {
                v.inactive.weak_bg_fill = Color32::from_rgb(255, 242, 224);
                v.inactive.bg_stroke = Stroke::new(1., Color32::from_rgb(180, 146, 118));
                v.inactive.fg_stroke.color = INK;
                v.hovered.weak_bg_fill = Color32::from_rgb(255, 248, 231);
                v.hovered.fg_stroke.color = INK;
                v.noninteractive.fg_stroke.color = INK;
            }
            ui.spacing_mut().button_padding = egui::vec2(13., 8.);
            ui.horizontal_wrapped(actions);
        });
    });
    let rect = frame.end(ui).rect;
    if !small {
        let picture = if dark {
            egui::Image::new(egui::include_image!("art/studio-glass-graphite.jpg"))
        } else {
            egui::Image::new(egui::include_image!("art/studio-glass.jpg"))
        };
        if let Ok(egui::load::TexturePoll::Ready { texture }) =
            picture.load_for_size(ui.ctx(), rect.size())
        {
            // Stretch only the quiet half of the photograph; preserve the actual profiles' proportions.
            let split = 0.46;
            let subject_width = (rect.height() * texture.size.x / texture.size.y * (1. - split))
                .min(rect.width() * 0.52);
            let x = rect.right() - subject_width;
            let left = Rect::from_min_max(rect.min, egui::pos2(x + 0.3, rect.bottom()));
            let right = Rect::from_min_max(egui::pos2(x, rect.top()), rect.max);
            let parts = [
                (
                    left,
                    egui::CornerRadius {
                        nw: radius,
                        sw: radius,
                        ..Default::default()
                    },
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(split, 1.)),
                ),
                (
                    right,
                    egui::CornerRadius {
                        ne: radius,
                        se: radius,
                        ..Default::default()
                    },
                    Rect::from_min_max(egui::pos2(split, 0.), egui::pos2(1., 1.)),
                ),
            ];
            painter.set(
                cover,
                egui::Shape::Vec(
                    parts
                        .into_iter()
                        .map(|(r, corners, uv)| {
                            egui::epaint::RectShape::filled(r, corners, Color32::WHITE)
                                .with_texture(texture.id, uv)
                                .into()
                        })
                        .collect(),
                ),
            );
        }
    }
}

pub fn section_heading(ui: &mut Ui, title: &str, detail: &str) {
    let p = theme::palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.label(display(title, 29.).color(p.text));
        if !detail.is_empty() {
            ui.label(RichText::new(detail).small().color(p.muted));
        }
    });
}

pub fn page_frame(ctx: &egui::Context, margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::Palette::new(ctx.style().visuals.dark_mode).canvas)
        .inner_margin(egui::Margin::same(margin))
}

pub fn action_at_right(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(egui::Layout::right_to_left(Align::Center), add);
}

pub fn file_stamp(ui: &Ui, rect: Rect, extension: &str) {
    let p = theme::palette(ui);
    let hue = match extension {
        "png" | "jpg" | "jpeg" | "svg" | "webp" | "gif" => Color32::from_rgb(115, 105, 164),
        "csv" | "xlsx" | "ods" | "json" | "sqlite" => Color32::from_rgb(49, 135, 110),
        "zip" | "gz" | "tar" | "7z" | "rar" => Color32::from_rgb(177, 124, 54),
        "mid" | "midi" | "mp3" | "wav" | "mp4" => Color32::from_rgb(64, 132, 158),
        _ => Color32::from_rgb(180, 100, 75),
    };
    let color = p.ink(hue);
    ui.painter().rect_filled(
        rect,
        6,
        p.panel
            .lerp_to_gamma(hue, if ui.visuals().dark_mode { 0.30 } else { 0.17 }),
    );
    let center = rect.center();
    let mark: String = extension
        .chars()
        .take(4)
        .flat_map(char::to_uppercase)
        .collect();
    if mark.is_empty() {
        theme::paint_icon(
            ui.painter(),
            rect.shrink(rect.width() * 0.25),
            Icon::Files,
            color,
        );
    } else {
        let size = (rect.width() / (mark.len() as f32 * 0.7 + 1.)).clamp(9., 13.);
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            mark,
            FontId::monospace(size),
            color,
        );
    }
}

/// Overlapping glass profiles form a consistent, recognizable mark at both roster
/// and portrait sizes. These variations identify a person without implying traits.
pub fn persona_mark(ui: &Ui, rect: Rect, label: &str, color: Color32) {
    let palette = theme::palette(ui);
    let ink = palette.ink(color);
    let field = palette
        .panel
        .lerp_to_gamma(color, if ui.visuals().dark_mode { 0.18 } else { 0.13 });
    let radius = (rect.height() * 0.17).round() as u8;
    ui.painter().rect_filled(rect, radius, field);
    let seed = label.bytes().fold(0u32, |sum, b| {
        sum.wrapping_mul(31).wrapping_add(u32::from(b))
    });
    let variation = ((seed % 13) as f32 - 6.) * 0.003;
    // Each row gives the back and front edge of the profile at a given height.
    let silhouette = [
        (0.10, 0.41, 0.47),
        (0.12, 0.34, 0.55),
        (0.17, 0.29, 0.60),
        (0.24, 0.25, 0.64),
        (0.32, 0.23, 0.65),
        (0.38, 0.23, 0.65 + variation),
        (0.42, 0.24, 0.63),
        (0.49, 0.26, 0.69),
        (0.51, 0.27, 0.69 + variation),
        (0.53, 0.28, 0.64),
        (0.57, 0.31, 0.64),
        (0.59, 0.32, 0.62),
        (0.61, 0.33, 0.64),
        (0.65, 0.34, 0.63),
        (0.68, 0.35, 0.60),
        (0.69, 0.35, 0.53),
        (0.77, 0.34, 0.52),
        (0.84, 0.28, 0.55),
        (0.89, 0.20, 0.60),
    ];
    let painter = ui.painter().with_clip_rect(rect);
    for (offset, tint) in [(0.14, 0.29), (0.07, 0.48), (0., 0.72)] {
        let mut mesh = egui::Mesh::default();
        let mut rim = Vec::with_capacity(silhouette.len());
        for (i, &(y, back, face)) in silhouette.iter().enumerate() {
            let at = |x| rect.min + egui::vec2((x + offset) * rect.width(), y * rect.height());
            mesh.colored_vertex(at(back), field.lerp_to_gamma(color, tint * 0.66));
            mesh.colored_vertex(at(face), field.lerp_to_gamma(ink, tint));
            rim.push(at(face));
            if i > 0 {
                let k = (i * 2) as u32;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k, k - 1, k + 1);
            }
        }
        painter.add(mesh);
        painter.add(egui::Shape::line(
            rim,
            Stroke::new(
                (rect.height() / 95.).clamp(0.65, 1.4),
                field.lerp_to_gamma(ink, tint),
            ),
        ));
    }
    if rect.width() >= 38. {
        let initial = label
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        painter.text(
            rect.left_bottom() + egui::vec2(rect.width() * 0.07, -rect.height() * 0.02),
            egui::Align2::LEFT_BOTTOM,
            initial,
            FontId::new(
                rect.height() * 0.39,
                FontFamily::Name("Studio display".into()),
            ),
            palette.text,
        );
    }
}

pub fn persona_tab(
    ui: &mut Ui,
    name: &str,
    detail: &str,
    color: Color32,
    selected: bool,
) -> egui::Response {
    let p = theme::palette(ui);
    let name = egui::WidgetText::from(RichText::new(name).strong().size(15.)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        104.,
        egui::TextStyle::Button,
    );
    let detail = egui::WidgetText::from(RichText::new(detail).size(12.)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        104.,
        egui::TextStyle::Small,
    );
    let width = (name.size().x.max(detail.size().x) + 54.).min(ui.available_width().max(90.));
    let (r, response) = ui.allocate_exact_size(egui::vec2(width, 47.), egui::Sense::click());
    let label = &name.job.text;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let active = response.hovered() || response.has_focus() || selected;
    let fill = if active {
        p.panel.lerp_to_gamma(color, 0.18)
    } else {
        p.panel
    };
    let edge = if active { p.ink(color) } else { p.border };
    ui.painter()
        .rect(r, 7, fill, Stroke::new(1., edge), egui::StrokeKind::Inside);
    let mark = Rect::from_min_size(r.min + egui::vec2(7., 8.), egui::Vec2::splat(30.));
    persona_mark(ui, mark, label, color);
    let x = r.left() + 43.;
    ui.painter().galley(
        egui::pos2(x, r.center().y - name.size().y + 1.),
        name,
        p.text,
    );
    ui.painter()
        .galley(egui::pos2(x, r.center().y + 2.), detail, p.muted);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn workspace_heading(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui);
    let wide = ui.available_width() >= 700.;
    let title = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 1.;
        ui.label(caption("Workspace", p.accent));
        ui.add(egui::Label::new(display(title, if wide { 38. } else { 32. })).wrap());
    };
    let picture = if ui.visuals().dark_mode {
        egui::Image::new(egui::include_image!("art/studio-glass-graphite.jpg"))
    } else {
        egui::Image::new(egui::include_image!("art/studio-glass.jpg"))
    };
    let width = ui.available_width();
    if wide {
        ui.horizontal(|ui| {
            let uv = Rect::from_min_max(egui::pos2(0.54, 0.), egui::pos2(0.99, 1.));
            ui.add(
                picture
                    .uv(uv)
                    .maintain_aspect_ratio(false)
                    .fit_to_exact_size(egui::vec2(82., 56.))
                    .corner_radius(8),
            );
            ui.add_space(6.);
            ui.vertical(|ui| {
                ui.set_width((width - 390.).max(140.));
                title(ui);
            });
            action_at_right(ui, |ui| {
                ui.horizontal_wrapped(actions);
            });
        });
    } else {
        title(ui);
        ui.horizontal_wrapped(actions);
    }
}
