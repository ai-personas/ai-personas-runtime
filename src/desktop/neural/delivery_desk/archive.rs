//! The ZIP is itself an issued file. Read its directory without unpacking or
//! silently rebuilding the upload a recipient will save.
use super::*;
use std::io::Cursor;

pub(super) struct Listing {
    entries: Vec<(String, u64)>,
    page: usize,
    query: String,
}

impl Listing {
    pub(super) fn read(bytes: &[u8]) -> ai_personas::neural::Result<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut entries = Vec::new();
        for index in 0..zip.len() {
            let member = zip.by_index_raw(index)?;
            if !member.is_dir() {
                entries.push((member.name().to_owned(), member.size()));
            }
        }
        Ok(Self {
            entries,
            page: 0,
            query: String::new(),
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui) {
        const PAGE: usize = 6;
        let palette = theme::palette(ui);
        egui::Frame::new()
            .fill(palette.canvas)
            .stroke(Stroke::new(1., palette.border))
            .corner_radius(6)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                eyebrow(ui, "Inside this archive");
                ui.label(
                    RichText::new(counted(self.entries.len() as u64, "file", "files")).strong(),
                );
                if self.entries.len() > PAGE
                    && ui
                        .add(
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text("Find a member…")
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                {
                    self.page = 0;
                }
                let needle = self.query.trim().to_lowercase();
                let mut found = 0;
                let start = self.page * PAGE;
                for (path, bytes) in &self.entries {
                    if !needle.is_empty() && !path.to_lowercase().contains(&needle) {
                        continue;
                    }
                    if (start..start + PAGE).contains(&found) {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(path).monospace().small()).wrap(),
                            );
                            ui.weak(RichText::new(size(*bytes)).small())
                                .on_hover_text(format!(
                                    "{bytes} bytes uncompressed, as listed in the ZIP"
                                ));
                        });
                    }
                    found += 1;
                }
                if found == 0 {
                    ui.weak("No matching file entries.");
                }
                if found > PAGE {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(self.page > 0, egui::Button::new("Previous").small())
                            .clicked()
                        {
                            self.page -= 1;
                        }
                        ui.small(format!(
                            "{}–{} of {found}",
                            start + 1,
                            (start + PAGE).min(found)
                        ));
                        if ui
                            .add_enabled(start + PAGE < found, egui::Button::new("Next").small())
                            .clicked()
                        {
                            self.page += 1;
                        }
                    });
                }
            });
    }
}
