//! Browse handoffs by edition, not by the union of the asset revisions they list.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArtifactView {
    Retained,
    Editions,
    Workspace,
}

#[derive(Default)]
pub(super) struct List {
    work: String,
    query: String,
    generation: Option<u64>,
    matches: Vec<(usize, Option<String>)>,
    page: usize,
}

impl App {
    pub(super) fn browse_editions(&mut self) {
        self.clear_file_preview();
        self.artifact_mode = ArtifactView::Editions;
        self.layout.editor = Editor::Artifacts;
    }

    /// A projected notice for navigation, not the verification done at read/save.
    pub(super) fn indexed_issue(&self, reference: &str, body: &Value) -> Option<&Value> {
        let seq = self.home_index.delivery_issue(reference)?;
        let i = self
            .events
            .binary_search_by_key(&seq, |e| e["event"].as_i64().unwrap_or(0))
            .ok()?;
        let e = &self.events[i];
        (e["actor"] == body["owner"]
            && e["work"] == body["work"]
            && e["body"]["title"] == body["title"]
            && e["body"]["scope"] == body["scope"])
            .then_some(e)
    }

    fn matching_editions(&self, needle: &str) -> Vec<(usize, Option<String>)> {
        let Some(activity) = self.home_index.activity(&self.work) else {
            return Vec::new();
        };
        // Projected paths are for discovery only. Do not open a retained file
        // as a stand-in for this edition, even when its digest is identical.
        let paths: BTreeMap<_, _> = self
            .snapshot
            .assets
            .iter()
            .filter(|_| !needle.is_empty())
            .map(|a| (s(a, "reference"), s(&a["body"], "path")))
            .collect();
        activity
            .deliveries
            .iter()
            .copied()
            .filter_map(|index| {
                let d = self.records("deliveries").get(index)?;
                let body = &d["body"];
                if needle.is_empty()
                    || [
                        s(body, "title"),
                        self.label(s(body, "owner")),
                        &inbox::handle(d),
                    ]
                    .iter()
                    .any(|t| t.to_lowercase().contains(needle))
                {
                    return Some((index, None));
                }
                body["artifacts"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(|r| {
                        let path = paths.get(r).copied().filter(|s| !s.is_empty()).unwrap_or(r);
                        (path.to_lowercase().contains(needle) || r.to_lowercase().contains(needle))
                            .then(|| (index, Some(path.to_owned())))
                    })
            })
            .collect()
    }

    pub(super) fn edition_library(&mut self, ui: &mut Ui) {
        const PAGE: usize = 5;
        if self.edition_list.work != self.work {
            self.edition_list = List {
                work: self.work.clone(),
                ..Default::default()
            };
        }
        eyebrow(ui, "DELIVERY DESK  /  SELECTED ACTIVITY");
        ui.heading("Issued editions");
        if self.work.is_empty() {
            ui.weak("Select an activity to see its handoffs.");
            return;
        }
        ui.label(excerpt(self.task_title(), 150))
            .on_hover_text(self.task_title());
        ui.weak("Choose a handoff before opening its files. The same file can belong to more than one edition.");
        let changed = ui
            .add(
                egui::TextEdit::singleline(&mut self.edition_list.query)
                    .hint_text("Find an edition, issuer, file or exact reference…")
                    .desired_width(ui.available_width().min(540.)),
            )
            .changed();
        if changed {
            self.edition_list.page = 0;
        }
        if changed || self.edition_list.generation != Some(self.home_index.generation) {
            self.edition_list.matches =
                self.matching_editions(&self.edition_list.query.trim().to_lowercase());
            self.edition_list.generation = Some(self.home_index.generation);
        }
        let count = self.edition_list.matches.len();
        if count == 0 {
            ui.label(if self.edition_list.query.trim().is_empty() {
                "No edition recorded for this activity."
            } else {
                "No matching edition in this activity."
            });
            return;
        }
        let last = (count - 1) / PAGE;
        self.edition_list.page = self.edition_list.page.min(last);
        ui.horizontal_wrapped(|ui| {
            ui.small(counted(count as u64, "edition", "editions"));
            if last > 0 {
                let page = &mut self.edition_list.page;
                if ui
                    .add_enabled(*page > 0, egui::Button::new("Previous").small())
                    .clicked()
                {
                    *page -= 1;
                }
                ui.small(format!("Page {} of {}", *page + 1, last + 1));
                if ui
                    .add_enabled(*page < last, egui::Button::new("Next").small())
                    .clicked()
                {
                    *page += 1;
                }
            }
        });
        let page = self.edition_list.page;
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt(("edition-list", &self.work, page))
            .show(ui, |ui| {
                for (index, found) in self
                    .edition_list
                    .matches
                    .iter()
                    .skip(page * PAGE)
                    .take(PAGE)
                {
                    let record = &self.records("deliveries")[*index];
                    let b = &record["body"];
                    let reference = inbox::handle(record);
                    ui.push_id(&reference, |ui| {
                        card(ui).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                theme::glyph(ui, Icon::Files, TEAL);
                                if ui
                                    .link(RichText::new(s(b, "title")).strong())
                                    .on_hover_text(&reference)
                                    .clicked()
                                {
                                    open = Some(reference.clone());
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(self.label(s(b, "owner")));
                                let (label, tint) = delivery_desk::scope(b);
                                theme::badge(ui, label, tint);
                                ui.small(counted(
                                    b["artifacts"].as_array().map_or(0, Vec::len) as u64,
                                    "listed file",
                                    "listed files",
                                ));
                                if let Some(seq) = self
                                    .indexed_issue(&reference, b)
                                    .and_then(|e| e["event"].as_i64())
                                {
                                    ui.small(format!("Notice #{seq}"));
                                } else {
                                    ui.colored_label(
                                        theme::ACCENT,
                                        "No matching public notice indexed",
                                    );
                                }
                            });
                            if let Some(path) = found {
                                ui.label(format!("Matching file · {path}"));
                            }
                            ui.label(excerpt(s(b, "supported_use"), 180));
                            if !edition_review::summary(
                                ui,
                                &self.snapshot.status["delivery_reviews"][&reference],
                            ) {
                                ui.weak("No operator decision recorded");
                            }
                            if ui
                                .button("Open edition ↗")
                                .on_hover_text(&reference)
                                .clicked()
                            {
                                open = Some(reference.clone());
                            }
                        })
                    });
                    ui.add_space(5.);
                }
            });
        if let Some(reference) = open {
            self.open_home_delivery(&reference, &self.work.clone());
        }
    }
}
