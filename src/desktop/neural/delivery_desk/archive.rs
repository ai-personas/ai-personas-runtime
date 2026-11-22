//! The ZIP is itself an issued file. Read its directory without unpacking or
//! silently rebuilding the upload a recipient will save.
use super::*;
use std::io::{self, Cursor, Read, Write};

#[derive(Clone)]
pub(in crate::desktop::neural) struct Member {
    pub index: usize,
    pub path: String,
    bytes: u64,
    crc: u32,
    pub(super) kind: Kind,
}

impl Member {
    pub fn key(&self, parent: &str) -> String {
        format!("{parent}#zip-entry-{}", self.index)
    }

    pub fn contents(&self, bytes: &[u8]) -> ai_personas::neural::Result<Vec<u8>> {
        if self.bytes > SOURCE_LIMIT {
            return Err(ai_personas::neural::failure(
                "This member is too large for an inline preview.",
            ));
        }
        let mut result = Vec::new();
        self.copy_to(bytes, &mut result)?;
        Ok(result)
    }

    /// Export streams to an unpublished file, independent of preview limits.
    pub fn copy_to(&self, bytes: &[u8], output: &mut impl Write) -> ai_personas::neural::Result<()> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut entry = zip.by_index(self.index)?;
        if entry.name() != self.path
            || entry.size() != self.bytes
            || entry.crc32() != self.crc
            || entry.is_dir()
        {
            return Err(ai_personas::neural::failure(
                "The ZIP entry differs from the selected member.",
            ));
        }
        let copied = io::copy(&mut (&mut entry).take(self.bytes), output)?;
        // Reading once past the expected end also runs the ZIP CRC check.
        if copied != self.bytes || entry.read(&mut [0])? != 0 {
            return Err(ai_personas::neural::failure(
                "The ZIP member size does not match its directory entry.",
            ));
        }
        Ok(())
    }
}

pub(super) struct Listing {
    entries: Vec<Member>,
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
                entries.push(Member {
                    index,
                    path: member.name().into(),
                    bytes: member.size(),
                    crc: member.crc32(),
                    kind: Format::identify(member.name(), "").kind,
                });
            }
        }
        Ok(Self {
            entries,
            page: 0,
            query: String::new(),
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui) -> Option<Member> {
        const PAGE: usize = 6;
        let palette = theme::palette(ui);
        let mut selected = None;
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
                for member in &self.entries {
                    let (path, bytes) = (&member.path, member.bytes);
                    if !needle.is_empty() && !path.to_lowercase().contains(&needle) {
                        continue;
                    }
                    if (start..start + PAGE).contains(&found) {
                        ui.push_id(member.index, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let text = RichText::new(path).monospace().small();
                                if ui.link(text)
                                    .on_hover_text("Inspect or save this file from the issued ZIP")
                                    .clicked() {
                                    selected = Some(member.clone());
                                }
                                ui.weak(RichText::new(size(bytes)).small())
                                    .on_hover_text(format!(
                                        "{bytes} bytes uncompressed, as listed in the ZIP"
                                    ));
                            })
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
        selected
    }
}
