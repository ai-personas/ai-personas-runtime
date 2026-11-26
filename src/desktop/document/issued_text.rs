//! Inline actions in issued Markdown are ordinary Studio links, not URLs.
//! The parser has already associated individual source offsets with entries in
//! this edition. Never infer a destination from the *displayed* label.
use eframe::egui::{self, Ui, WidgetText};
use pulldown_cmark::{Event, Tag, TagEnd};
use std::collections::BTreeMap;

pub(crate) struct Files<'a> {
    pub names: &'a [(String, String)],
    pub at: &'a BTreeMap<usize, usize>,
}

#[derive(Default)]
pub(super) struct Cursor {
    active: Option<usize>,
    images: usize,
}

#[derive(Default)]
pub(super) struct Mark {
    pub file: Option<usize>,
    pub closed_bound_link: bool,
}

impl Cursor {
    /// Image syntax is passive, including alt text nested inside a bound link.
    /// The label of a bound link inherits its destination, not a second action
    /// from any code or formatting used to write its name.
    pub(super) fn mark(
        &mut self,
        event: &Event<'_>,
        offset: usize,
        files: Option<&Files<'_>>,
    ) -> Option<Mark> {
        let Some(files) = files else {
            return Some(Mark::default());
        };
        match event {
            Event::Start(Tag::Image { .. }) => {
                self.images += 1;
                return None;
            }
            Event::End(TagEnd::Image) => {
                self.images = self.images.saturating_sub(1);
                return None;
            }
            Event::Start(Tag::Link { .. }) | Event::End(TagEnd::Link) if self.images != 0 => {
                return None
            }
            _ if self.images != 0 => return Some(Mark::default()),
            Event::Start(Tag::Link { .. }) => self.active = files.at.get(&offset).copied(),
            Event::End(TagEnd::Link) => {
                return Some(Mark {
                    file: None,
                    closed_bound_link: self.active.take().is_some(),
                })
            }
            _ => (),
        }
        let file = self.active.or_else(|| match event {
            Event::Code(_) => files.at.get(&offset).copied(),
            _ => None,
        });
        Some(Mark {
            file,
            closed_bound_link: false,
        })
    }
}

/// Keep adjacent text with the same target in one native label, including
/// styled text within a link. Ordinary unlinked paragraphs keep their single
/// selectable label. Only mixed paragraphs need a wrapping line of widgets.
#[derive(Default)]
pub(super) struct Runs<T>(Vec<(Option<usize>, T)>);

impl<T: Default> Runs<T> {
    pub(super) fn part(&mut self, target: Option<usize>) -> &mut T {
        if self.0.last().is_none_or(|(last, _)| *last != target) {
            self.0.push((target, T::default()));
        }
        &mut self.0.last_mut().expect("just added or matched").1
    }

    pub(super) fn show_cell(
        mut self,
        ui: &mut Ui,
        width: f32,
        files: &[(String, String)],
        text: impl Fn(T) -> WidgetText,
    ) -> Option<usize> {
        // Keep an empty cell in its column and avoid a new layout for the
        // usual single label. Mixed cells wrap inside one bounded grid item.
        if self.0.is_empty() {
            self.part(None);
        }
        if self.0.len() > 1 {
            ui.vertical(|ui| {
                ui.set_max_width(width);
                self.show(ui, files, text)
            })
            .inner
        } else {
            self.show(ui, files, text)
        }
    }

    pub(super) fn show(
        self,
        ui: &mut Ui,
        files: &[(String, String)],
        text: impl Fn(T) -> WidgetText,
    ) -> Option<usize> {
        let mixed = self.0.len() > 1;
        let mut follow = None;
        let draw = |ui: &mut Ui| {
            for (index, value) in self.0 {
                let label = text(value);
                if let Some((index, (path, reference))) =
                    index.and_then(|i| files.get(i).map(|file| (i, file)))
                {
                    // Link only returns a click. Hyperlink would hand an
                    // untrusted destination to the OS/browser instead.
                    if ui
                        .add(egui::Link::new(label))
                        .on_hover_text(format!("Inspect in this edition\n{path}\n{reference}"))
                        .clicked()
                    {
                        follow = Some(index);
                    }
                } else {
                    ui.add(egui::Label::new(label).wrap().selectable(true));
                }
            }
        };
        if mixed {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                draw(ui);
            });
        } else {
            draw(ui);
        }
        follow
    }
}
