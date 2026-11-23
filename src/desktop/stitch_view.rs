//! Read actual needle records, not the DST header's advertised size or a companion picture.
use super::{native_preview::Cancellation, theme};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui};
use resvg::tiny_skia as sk;
use sha2::{Digest, Sha256};

// Bound preview work, not the size of a file a recipient can take unchanged.
pub(super) const MAX_BYTES: usize = 512 + 3 * 250_000;
type Point = [i32; 2]; // tenths of a millimetre, +Y up when viewed from the front

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    lo: Point,
    hi: Point,
}
impl Bounds {
    fn at(p: Point) -> Self {
        Self { lo: p, hi: p }
    }
    fn include(&mut self, p: Point) {
        for (axis, v) in p.into_iter().enumerate() {
            self.lo[axis] = self.lo[axis].min(v);
            self.hi[axis] = self.hi[axis].max(v);
        }
    }
    fn span(&self, i: usize) -> i32 {
        self.hi[i] - self.lo[i]
    }
    fn ranges(&self) -> String {
        format!(
            "X {:.1}…{:.1} mm, Y {:.1}…{:.1} mm",
            mm(self.lo[0]),
            mm(self.hi[0]),
            mm(self.lo[1]),
            mm(self.hi[1])
        )
    }
}
fn mm(v: i32) -> f64 {
    f64::from(v) / 10.
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Pierce,
    Jump,
    Change,
}
struct Record {
    kind: Kind,
    at: Point,
}
struct Pattern {
    moves: Vec<Record>,
    travel: Bounds,
    needle: Option<Bounds>,
    stitches: usize,
    jumps: usize,
    changes: usize,
    sewn_blocks: usize,
}

fn decode(bytes: &[u8], cancel: &Cancellation) -> Result<Pattern, String> {
    if bytes.len() > MAX_BYTES {
        return Err("This design exceeds the stitch preview limit.".into());
    }
    if bytes.len() < 515 || !bytes.starts_with(b"LA:") {
        return Err("No 512-byte Tajima DST header and movement stream found.".into());
    }
    let mut p = Pattern {
        moves: Vec::new(),
        travel: Bounds::at([0, 0]),
        needle: None,
        stitches: 0,
        jumps: 0,
        changes: 0,
        sewn_blocks: 0,
    };
    let mut at = [0, 0];
    let mut new_block = true;
    // Each signed ternary digit is stored as two bits; X and Y use different bit orders.
    let digit =
        |byte: u8, plus, minus| i32::from((byte >> plus) & 1u8) - i32::from((byte >> minus) & 1u8);
    for (i, word) in bytes[512..].chunks_exact(3).enumerate() {
        if i % 2048 == 0 && cancel.is_cancelled() {
            return Err("Preview cancelled.".into());
        }
        let (a, b, c) = (word[0], word[1], word[2]);
        if word == [0, 0, 0xf3] {
            if !bytes[515 + 3 * i..]
                .iter()
                .all(|b| matches!(b, 0 | 0x1a | b' '))
            {
                return Err("Additional non-padding data follows the DST end record.".into());
            }
            return Ok(p);
        }
        let kind = match c & 0xc3 {
            0x03 => Kind::Pierce,
            0x83 => Kind::Jump,
            0xc3 if c & 0xf3 != 0xf3 => Kind::Change,
            0x43 => {
                return Err(
                    "This DST uses sequin mode, which the stitch preview cannot display.".into(),
                )
            }
            _ => return Err(format!("Unrecognized DST record at byte {}.", 512 + 3 * i)),
        };
        at[0] += digit(a, 0, 1)
            + 3 * digit(b, 0, 1)
            + 9 * digit(a, 2, 3)
            + 27 * digit(b, 2, 3)
            + 81 * digit(c, 2, 3);
        at[1] += digit(a, 7, 6)
            + 3 * digit(b, 7, 6)
            + 9 * digit(a, 5, 4)
            + 27 * digit(b, 5, 4)
            + 81 * digit(c, 5, 4);
        p.travel.include(at);
        match kind {
            Kind::Pierce => {
                p.needle.get_or_insert(Bounds::at(at)).include(at);
                p.stitches += 1;
                if new_block {
                    p.sewn_blocks += 1;
                    new_block = false;
                }
            }
            Kind::Jump => p.jumps += 1,
            Kind::Change => {
                p.changes += 1;
                new_block = true;
            }
        }
        p.moves.push(Record { kind, at });
    }
    Err("The DST stream has no complete end record.".into())
}

pub(super) struct Plan {
    sewn: egui::ColorImage,
    with_jumps: egui::ColorImage,
    pixels_per_unit: f32,
    origin: Pos2,
    size: String,
    counts: String,
    report: String,
}

impl Plan {
    pub fn read(bytes: &[u8], name: &str, cancel: &Cancellation) -> Result<Self, String> {
        let p = decode(bytes, cancel)?;
        let size = p.needle.map_or_else(
            || "No needle penetrations".into(),
            |n| format!("{:.1} × {:.1} mm sewn", mm(n.span(0)), mm(n.span(1))),
        );
        let counts = format!(
            "{} needle records · {} jumps · {} changes",
            p.stitches, p.jumps, p.changes
        );
        let needle = p.needle.map_or_else(
            || "none".into(),
            |n| {
                format!(
                    "{}; center ({:.2}, {:.2}) mm",
                    n.ranges(),
                    mm(n.lo[0] + n.hi[0]) / 2.,
                    mm(n.lo[1] + n.hi[1]) / 2.
                )
            },
        );
        let report = format!("{name}\nSHA-256: {:x}\nTajima DST · front view (+X right, +Y up)\n{size}\nNeedle positions: {needle}\nAll movement including origin: {}\n{counts} · {} sewn blocks\n",
            Sha256::digest(bytes), p.travel.ranges(), p.sewn_blocks);

        // Rasterize once on the file worker. Zoom-independent stats use the full integer records.
        let dx = p.travel.span(0) as f32;
        let dy = p.travel.span(1) as f32;
        let scale = (900. / dx.max(1.)).min(540. / dy.max(1.)).min(15.);
        let (w, h) = (
            (dx * scale).ceil() as u32 + 32,
            (dy * scale).ceil() as u32 + 32,
        );
        let origin = egui::pos2(
            16. - p.travel.lo[0] as f32 * scale,
            16. + p.travel.hi[1] as f32 * scale,
        );
        let pos = |[x, y]: Point| origin + egui::vec2(x as f32 * scale, -y as f32 * scale);
        let mut sewn = sk::Pixmap::new(w, h).ok_or("Cannot allocate DST preview.")?;
        let mut jumps = sewn.clone();
        let colors = [
            (251, 195, 144),
            (103, 210, 195),
            (186, 165, 226),
            (135, 188, 237),
        ];
        let mut paint = sk::Paint::default();
        let sew_stroke = sk::Stroke {
            width: 2.6,
            line_cap: sk::LineCap::Round,
            line_join: sk::LineJoin::Round,
            ..Default::default()
        };
        let jump_stroke = sk::Stroke {
            width: 1.8,
            dash: sk::StrokeDash::new(vec![6., 4.], 0.),
            ..Default::default()
        };
        let mut block = 0;
        let mut prev = origin;
        let mut previous_kind = None;
        let mut path = sk::PathBuilder::new();
        for (i, r) in p.moves.iter().enumerate() {
            if i % 2048 == 0 && cancel.is_cancelled() {
                return Err("Preview cancelled.".into());
            }
            let to = pos(r.at);
            if r.kind == Kind::Pierce {
                let (red, green, blue) = colors[block % colors.len()];
                paint.set_color_rgba8(red, green, blue, 255);
                if previous_kind != Some(Kind::Pierce) {
                    // A jump is not a sewn connector. Show the first puncture even if it stands alone.
                    path.move_to(to.x, to.y);
                    if let Some(dot) = sk::Rect::from_xywh(to.x - 1., to.y - 1., 2., 2.) {
                        sewn.fill_rect(dot, &paint, sk::Transform::identity(), None);
                    }
                } else {
                    path.line_to(to.x, to.y);
                }
                if i % 2048 == 2047
                    || p.moves
                        .get(i + 1)
                        .is_none_or(|next| next.kind != Kind::Pierce)
                {
                    if let Some(segment) =
                        std::mem::replace(&mut path, sk::PathBuilder::new()).finish()
                    {
                        sewn.stroke_path(
                            &segment,
                            &paint,
                            &sew_stroke,
                            sk::Transform::identity(),
                            None,
                        );
                    }
                    path.move_to(to.x, to.y);
                }
            } else {
                if prev != to {
                    let mut line = sk::PathBuilder::new();
                    line.move_to(prev.x, prev.y);
                    line.line_to(to.x, to.y);
                    paint.set_color_rgba8(135, 186, 221, 230);
                    if let Some(line) = line.finish() {
                        jumps.stroke_path(
                            &line,
                            &paint,
                            &jump_stroke,
                            sk::Transform::identity(),
                            None,
                        );
                    }
                }
                if r.kind == Kind::Change {
                    block += 1;
                }
            }
            prev = to;
            previous_kind = Some(r.kind);
        }
        jumps.draw_pixmap(
            0,
            0,
            sewn.as_ref(),
            &Default::default(),
            sk::Transform::identity(),
            None,
        );
        let image = |map: sk::Pixmap| {
            egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], map.data())
        };
        Ok(Self {
            sewn: image(sewn),
            with_jumps: image(jumps),
            pixels_per_unit: scale,
            origin,
            size,
            counts,
            report,
        })
    }

    pub fn ui(&self, ui: &mut Ui, travel: &mut bool, texture: &mut Option<egui::TextureHandle>) {
        let p = theme::palette(ui);
        ui.horizontal_wrapped(|ui| {
            ui.strong(&self.size);
            ui.separator();
            ui.weak(&self.counts);
        });
        let changed = ui
            .horizontal_wrapped(|ui| {
                let toggled = ui.checkbox(travel, "Show travel").changed();
                if ui.small_button("Copy measurements").clicked() {
                    ui.ctx().copy_text(self.report.clone());
                }
                toggled
            })
            .inner;
        if texture.is_none() || changed {
            let image = if *travel {
                &self.with_jumps
            } else {
                &self.sewn
            };
            *texture = Some(ui.ctx().load_texture(
                "dst-records",
                image.clone(),
                egui::TextureOptions::LINEAR,
            ));
        }
        let t = texture.as_ref().unwrap();
        let (r, response) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                (ui.available_height() - 26.).clamp(80., 450.),
            ),
            egui::Sense::hover(),
        );
        let paint = ui.painter_at(r);
        paint.rect_filled(r, 6., Color32::from_rgb(35, 42, 50));
        let a = r.shrink(6.);
        let scale = (a.width() / t.size_vec2().x)
            .min(a.height() / t.size_vec2().y)
            .max(0.001);
        let target = Rect::from_center_size(r.center(), t.size_vec2() * scale);
        let origin = target.min + self.origin.to_vec2() * scale;
        let axis = Stroke::new(1., Color32::from_rgb(77, 89, 100));
        paint.hline(a.x_range(), origin.y, axis);
        paint.vline(origin.x, a.y_range(), axis);
        paint.image(
            t.id(),
            target,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        paint.circle_stroke(
            origin,
            3.5,
            Stroke::new(1., Color32::from_rgb(177, 191, 200)),
        );
        if let Some(cursor) = response.hover_pos() {
            let offset = (cursor - origin) / (scale * self.pixels_per_unit * 10.);
            response.on_hover_text(format!("X {:+.1} mm · Y {:+.1} mm", offset.x, -offset.y));
        }
        ui.colored_label(
            p.muted,
            "Front · +X right, +Y up · Colors identify thread blocks, not spool colors.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dst(words: &[[u8; 3]]) -> Vec<u8> {
        let mut bytes = vec![b' '; 512];
        bytes[..16].copy_from_slice(b"LA:TEST\r+X:9999\r"); // deliberately untrue header extents
        bytes.extend(words.iter().flatten());
        bytes
    }
    #[test]
    fn travel_is_not_sewn_and_y_is_up() {
        let b = dst(&[
            [0, 0, 0x87],
            [0x81, 0, 3],
            [0x14, 0, 3],
            [0, 0, 0xc3],
            [0, 0x40, 0x83],
            [2, 0, 3],
            [0, 0, 0xf3],
        ]);
        let p = decode(&b, &Default::default()).unwrap();
        assert_eq!(
            p.travel,
            Bounds {
                lo: [0, -11],
                hi: [91, 1]
            }
        );
        assert_eq!(
            p.needle,
            Some(Bounds {
                lo: [82, -11],
                hi: [91, 1]
            })
        );
        assert_eq!(
            (p.stitches, p.jumps, p.changes, p.sewn_blocks),
            (3, 2, 1, 2)
        );
    }
    #[test]
    fn partial_or_unsupported_stream_is_not_a_finished_design() {
        let c = Cancellation::default();
        assert!(decode(&dst(&[[0, 0, 3]]), &c).is_err());
        assert!(decode(&dst(&[[0, 0, 0x43], [0, 0, 0xf3]]), &c).is_err());
        assert!(decode(&dst(&[[0, 0, 0xf3], [0, 0, 3]]), &c).is_err());
        assert!(decode(&dst(&[[0, 0, 0xf3]]), &c).unwrap().needle.is_none());
    }
}
