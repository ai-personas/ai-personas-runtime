use super::{input, theme};
use eframe::egui::{self, RichText, Ui};
use std::{fmt::Write, fs::File, sync::Arc};

const PAGE_BYTES: u64 = 1024;

pub(super) struct Page {
    pub file: Arc<File>,
    pub prefer_text: bool,
    pub length: u64,
    index: u64,
    bytes: Vec<u8>,
    text: String,
}

impl Page {
    pub fn read(file: Arc<File>, index: u64) -> Result<Self, String> {
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        let index = index.min(length.saturating_sub(1) / PAGE_BYTES);
        let offset = index * PAGE_BYTES;
        let mut bytes = input::read_at(&file, offset, PAGE_BYTES as usize + 3)?;
        let end = bytes.len().min(PAGE_BYTES as usize);
        let bom = input::read_at(&file, 0, 3)?;
        let text = page_text(&bytes, end, index, &bom);
        bytes.truncate(end);
        Ok(Self {
            file,
            prefer_text: false,
            length,
            index,
            bytes,
            text,
        })
    }

    pub fn ui(&self, ui: &mut Ui, text: &mut bool) -> Option<u64> {
        let total = self.length.div_ceil(PAGE_BYTES).max(1);
        let mut target = self.index + 1;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(text, false, "Hexadecimal");
            ui.selectable_value(text, true, "Text");
            ui.separator();
            if ui
                .add_enabled(target > 1, egui::Button::new("Previous"))
                .clicked()
            {
                target -= 1;
            }
            ui.label("Page");
            ui.add(egui::DragValue::new(&mut target).range(1..=total));
            ui.weak(format!("of {total}"));
            if ui
                .add_enabled(target < total, egui::Button::new("Next"))
                .clicked()
            {
                target += 1;
            }
            ui.weak(format!("{} bytes", self.length));
        });
        ui.add_space(6.);
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.panel)
            .stroke(egui::Stroke::new(1., p.border))
            .corner_radius(8)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::both()
                    .id_salt(("bytes", self.index, *text))
                    .show(ui, |ui| {
                        if self.bytes.is_empty() {
                            ui.weak("Empty file");
                            return;
                        }
                        if *text {
                            ui.label(RichText::new(&self.text).monospace());
                        } else {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 18.;
                                let mut offsets = String::new();
                                let mut hex = String::new();
                                let mut chars = String::new();
                                for (row, chunk) in self.bytes.chunks(16).enumerate() {
                                    let _ = writeln!(
                                        offsets,
                                        "{:08X}",
                                        self.index * PAGE_BYTES + (row * 16) as u64
                                    );
                                    for (i, &byte) in chunk.iter().enumerate() {
                                        if i == 8 {
                                            hex.push(' ');
                                        }
                                        let _ = write!(hex, "{byte:02X} ");
                                        chars.push(if byte == b' ' || byte.is_ascii_graphic() {
                                            byte as char
                                        } else {
                                            '·'
                                        });
                                    }
                                    hex.push('\n');
                                    chars.push('\n');
                                }
                                for (label, body, color) in [
                                    ("Offset", offsets, p.accent),
                                    ("Bytes", hex, p.text),
                                    ("ASCII", chars, p.muted),
                                ] {
                                    ui.vertical(|ui| {
                                        ui.label(super::super::design::caption(label, p.muted));
                                        ui.label(
                                            RichText::new(body.trim_end_matches('\n'))
                                                .monospace()
                                                .color(color),
                                        );
                                    });
                                }
                            });
                        }
                    });
            });
        (target - 1 != self.index).then_some(target - 1)
    }
}

// A Unicode character crossing a page boundary belongs to the page where it starts.
// The hexadecimal presentation always uses exactly the original page's bytes.
fn page_text(bytes: &[u8], end: usize, page: u64, bom: &[u8]) -> String {
    let little = bom.starts_with(&[0xff, 0xfe]);
    if little || bom.starts_with(&[0xfe, 0xff]) {
        let unit = |b: &[u8]| {
            if little {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            }
        };
        let mut words: Vec<_> = bytes[..end].chunks_exact(2).map(unit).collect();
        if matches!(words.last(), Some(0xd800..=0xdbff)) {
            if let Some(next) = bytes
                .get(end..end + 2)
                .map(unit)
                .filter(|v| (0xdc00..=0xdfff).contains(v))
            {
                words.push(next);
            }
        }
        let skip = usize::from(words.first().is_some_and(|n| {
            (page == 0 && *n == 0xfeff) || (page > 0 && (0xdc00..=0xdfff).contains(n))
        }));
        return String::from_utf16_lossy(&words[skip..]);
    }
    let start = if page == 0 && bom.starts_with(&[0xef, 0xbb, 0xbf]) {
        3
    } else if page > 0 {
        bytes[..end]
            .iter()
            .take(3)
            .take_while(|b| **b & 0xc0 == 0x80)
            .count()
    } else {
        0
    };
    let continuation = bytes[end..]
        .iter()
        .take_while(|b| **b & 0xc0 == 0x80)
        .count();
    String::from_utf8_lossy(&bytes[start.min(end)..end + continuation]).into_owned()
}
