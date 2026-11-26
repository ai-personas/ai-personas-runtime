//! In-place reading of a verified Markdown file. Never resolve a document
//! path against a filesystem or against a different edition of the package.
use crate::desktop::{document, theme};
use eframe::egui::{self, RichText, Ui};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
};

pub(super) const LIMIT: usize = 64 * 1024;
type Paths = BTreeMap<PathBuf, String>;
struct Section {
    start: usize,
    title: String,
}
pub(super) struct Document {
    text: String,
    bom: usize, // offset in the original UTF-8 source; never dropped from Copy
    sections: Vec<Section>,
    named: Vec<(String, String)>, // logical path and exact reference, in mention order
}

// For ordinary Markdown links use the containing document's directory. An
// inline code span can also spell a full package path. Never guess between two
// such readings, match by basename, or search a different package or a ZIP.
fn target<'a>(
    paths: &'a Paths,
    document: &str,
    token: &str,
    code: bool,
) -> Option<(&'a PathBuf, &'a String)> {
    if token.is_empty()
        || token.len() > 4096
        || token.contains([':', '\\', '?', '#'])
        || token.chars().any(char::is_control)
        || (!code && token.contains('%'))
    {
        return None;
    }
    if token.ends_with("/.") || token.ends_with("/..") || token == "." || token == ".." {
        return None;
    }
    let literal = code && !token.split('/').any(|s| matches!(s, "." | ".."));
    let root = literal
        .then(|| paths.get_key_value(Path::new(token)))
        .flatten();
    let mut dir = document.split('/').collect::<Vec<_>>();
    dir.pop();
    for part in token.split('/') {
        match part {
            "" => return None,
            "." => (),
            ".." => {
                dir.pop()?;
            }
            _ => dir.push(part),
        }
    }
    let relative = paths.get_key_value(Path::new(&dir.join("/")));
    match (root, relative) {
        (Some(a), Some(b)) if a.0 != b.0 => None,
        (Some(a), _) | (_, Some(a)) if a.0 != Path::new(document) => Some(a),
        _ => None,
    }
}

impl Document {
    pub(super) fn read(bytes: &[u8], path: &str, paths: Option<&Paths>) -> Option<Self> {
        if bytes.len() > LIMIT || bytes.contains(&0) {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        let markdown = text.strip_prefix('\u{feff}').unwrap_or(text);
        let bom = text.len() - markdown.len();
        let mut sections = vec![Section {
            start: 0,
            title: "Opening text".into(),
        }];
        let mut depth = 0;
        let mut heading: Option<(usize, String)> = None;
        let mut inside_link = 0;
        let mut named = Vec::new();
        let mut seen = BTreeSet::new();
        let mut mention = |word: &str, code| {
            if let Some((p, r)) = paths.and_then(|m| target(m, path, word, code)) {
                if seen.insert(r) {
                    named.push((p.to_string_lossy().into_owned(), r.clone()));
                }
            }
        };
        for (i, (event, span)) in Parser::new_ext(
            markdown,
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
        )
        .into_offset_iter()
        .enumerate()
        {
            if i >= 16_384 {
                return None;
            }
            match event {
                Event::Start(tag) => {
                    match tag {
                        Tag::Heading { .. } if depth == 0 => {
                            heading = Some((span.start + bom, String::new()))
                        }
                        Tag::Link { dest_url, .. } => {
                            if inside_link == 0 {
                                mention(&dest_url, false);
                            }
                            inside_link += 1;
                        }
                        Tag::Image { .. } => inside_link += 1,
                        _ => (),
                    }
                    depth += 1;
                }
                Event::End(tag) => {
                    depth -= 1;
                    if matches!(tag, TagEnd::Link | TagEnd::Image) {
                        inside_link -= 1;
                    }
                    if depth == 0 && matches!(tag, TagEnd::Heading(_)) {
                        if let Some((at, title)) = heading.take() {
                            let title = if title.trim().is_empty() {
                                "Untitled section".into()
                            } else {
                                title
                            };
                            if sections.len() == 1
                                && text[..at].trim_start_matches('\u{feff}').trim().is_empty()
                            {
                                sections[0].title = title;
                            } else {
                                if sections.len() == 256 {
                                    return None;
                                }
                                sections.push(Section { start: at, title });
                            }
                        }
                    }
                }
                Event::Code(word) => {
                    if inside_link == 0 {
                        mention(&word, true);
                    }
                    if let Some((_, name)) = &mut heading {
                        name.push_str(&word);
                    }
                }
                Event::Text(word) => {
                    if let Some((_, name)) = &mut heading {
                        name.push_str(&word);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some((_, name)) = &mut heading {
                        name.push(' ');
                    }
                }
                _ => (),
            }
        }
        Some(Self {
            text: text.into(),
            bom,
            sections,
            named,
        })
    }
    fn range(&self, n: usize) -> Range<usize> {
        self.sections[n].start
            ..self
                .sections
                .get(n + 1)
                .map_or(self.text.len(), |s| s.start)
    }

    pub(super) fn show(&self, ui: &mut Ui, key: (&str, &str, &str)) -> Option<(String, String)> {
        // Remember the reading position on this exact file, including when
        // another revision has the same path. It is not a permission to read.
        let id = egui::Id::new(("issued-markdown", ui.ctx().viewport_id(), key));
        let (mut n, mut source) = ui
            .data(|d| d.get_temp::<(usize, bool)>(id))
            .unwrap_or_default();
        let count = self.sections.len();
        n = n.min(count - 1);
        let mut follow = None;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut source, false, "Reading view");
            ui.selectable_value(&mut source, true, "Section source");
            if !self.named.is_empty() {
                egui::ComboBox::from_id_salt(id.with("files"))
                    .selected_text(format!("Files named here · {}", self.named.len()))
                    .height((ui.ctx().content_rect().height() * 0.5).clamp(140., 360.))
                    .width(ui.available_width().clamp(110., 185.))
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.set_max_width((ui.ctx().content_rect().width() - 42.).clamp(160., 600.));
                        ui.small(
                            "Open a listed path in this edition. Its bytes are checked on open.",
                        );
                        for (path, file) in &self.named {
                            if ui
                                .add(egui::Button::selectable(false, path).wrap())
                                .on_hover_text(file)
                                .clicked()
                            {
                                follow = Some((path.clone(), file.clone()));
                                ui.close();
                            }
                        }
                    });
            }
        });
        if count > 1 {
            ui.small(format!("Section {} of {count}", n + 1));
            egui::ComboBox::from_id_salt(id.with("sections"))
                .selected_text(&self.sections[n].title)
                .height((ui.ctx().content_rect().height() * 0.5).clamp(140., 360.))
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    ui.set_max_width((ui.ctx().content_rect().width() - 42.).clamp(160., 600.));
                    for (i, section) in self.sections.iter().enumerate() {
                        if ui
                            .add(
                                egui::Button::selectable(
                                    n == i,
                                    format!("{} · {}", i + 1, section.title),
                                )
                                .wrap(),
                            )
                            .clicked()
                        {
                            n = i;
                            ui.close();
                        }
                    }
                });
        }
        let section = self.range(n);
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.canvas)
            .inner_margin(10)
            .corner_radius(6)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // The page around us also scrolls, and its columns may report
                // no remaining height. Give long sections a usable viewport;
                // auto_shrink still fits a short paragraph to its content.
                let height = (ui.ctx().content_rect().height() * 0.44).clamp(155., 450.);
                egui::ScrollArea::vertical()
                    .id_salt(id.with(("body", n, source)))
                    .min_scrolled_height(height)
                    .max_height(height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if self.text.is_empty() {
                            ui.weak("Empty document");
                        } else if source {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.text[section.clone()]).monospace(),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        } else {
                            document::issued_section(
                                ui,
                                &self.text[self.bom..],
                                section.start.saturating_sub(self.bom)..section.end - self.bom,
                            );
                        }
                    });
            });
        ui.horizontal_wrapped(|ui| {
            if count > 1 {
                if ui
                    .add_enabled(n > 0, egui::Button::new("Previous section").small())
                    .clicked()
                {
                    n -= 1;
                }
                if ui
                    .add_enabled(n + 1 < count, egui::Button::new("Next section").small())
                    .clicked()
                {
                    n += 1;
                }
            }
            if ui.small_button("Copy section source").clicked() {
                ui.ctx().copy_text(self.text[section].into());
            }
        });
        ui.data_mut(|d| d.insert_temp(id, (n, source)));
        follow
    }
}
