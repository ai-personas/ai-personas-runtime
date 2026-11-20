use super::*;
use ai_personas::neural::asset_history::{author_catalog, AuthoredCatalog};
use std::path::Path;

const PAGE_SIZE: usize = 8;

#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
enum Filter {
    #[default]
    All,
    Current,
    Superseded,
    Retired,
    Unknown,
}

impl Filter {
    const OPTIONS: [(Self, &'static str, &'static str); 5] = [
        (Self::All, "All", "All verified file contributions"),
        (
            Self::Current,
            "Current",
            "The current file still uses this person's content",
        ),
        (
            Self::Superseded,
            "Superseded",
            "A later write supplies the current file",
        ),
        (
            Self::Retired,
            "Retired",
            "The current library record is retired",
        ),
        (
            Self::Unknown,
            "Status unknown",
            "The current file or its content origin could not be verified",
        ),
    ];

    fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Current => "current",
            Self::Superseded => "superseded",
            Self::Retired => "retired",
            Self::Unknown => "unknown",
        }
    }
}

type CatalogueResult = Result<AuthoredCatalog, String>;

#[derive(Default)]
pub(in super::super) struct AuthoredFiles {
    root: PathBuf,
    person: String,
    generation: Option<u64>,
    // Head revision totals include retired files. A later import receipt can
    // establish authorship after the version itself has become visible.
    stamp: (u64, i64),
    requested: Option<(u64, i64)>,
    reader: Option<mpsc::Receiver<CatalogueResult>>,
    catalog: Option<AuthoredCatalog>,
    query: String,
    filter: Filter,
    offset: usize,
    page: Option<Arc<Value>>,
    error: Option<String>,
}

impl AuthoredFiles {
    pub(super) fn total(&self) -> Option<usize> {
        self.catalog.as_ref().map(AuthoredCatalog::total)
    }

    pub(super) fn observe(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        person: &str,
        index: &home_index::Index,
        assets: &[Value],
    ) {
        if self.person != person || self.root != root {
            *self = Self {
                root: root.into(),
                person: person.into(),
                ..Default::default()
            };
        }
        if self.generation != Some(index.generation) {
            let versions = assets
                .iter()
                .filter(|a| a["current"] == true)
                .filter_map(|a| a["revision"].as_u64())
                .fold(0u64, u64::saturating_add);
            self.stamp = (versions, index.latest_asset_receipt);
            self.generation = Some(index.generation);
        }
        if let Some(reader) = &self.reader {
            let result = match reader.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The file catalogue reader stopped.".into())
                }
            };
            self.reader = None;
            match result {
                Ok(catalog) => {
                    self.catalog = Some(catalog);
                    self.repage();
                }
                Err(error) => {
                    self.catalog = None;
                    self.page = None;
                    self.error = Some(error);
                }
            }
        }
        if self.requested == Some(self.stamp) {
            return;
        }
        self.requested = Some(self.stamp);
        self.error = None;
        let (tx, rx) = mpsc::channel();
        let (ctx, root, person) = (ctx.clone(), self.root.clone(), self.person.clone());
        self.reader = Some(rx);
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| author_catalog(&db, &person))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn repage(&mut self) {
        let Some(catalog) = &self.catalog else { return };
        let fetch = |offset| catalog.page(&self.query, self.filter.key(), offset, PAGE_SIZE);
        let result = fetch(self.offset).and_then(|page| {
            let total = page["total"].as_u64().unwrap_or_default() as usize;
            let last = total.saturating_sub(1) / PAGE_SIZE * PAGE_SIZE;
            if self.offset > last {
                self.offset = last;
                fetch(last)
            } else {
                Ok(page)
            }
        });
        match result {
            Ok(page) => {
                self.page = Some(Arc::new(page));
                self.error = None;
            }
            Err(error) => {
                self.page = None;
                self.error = Some(error.to_string());
            }
        }
    }

    fn controls(&mut self, ui: &mut Ui) {
        if self.total().unwrap_or(0) == 0 {
            return;
        }
        ui.add_space(4.);
        let mut changed = false;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 80.).clamp(48., 580.);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id_salt(("profile-file-search", &self.person))
                    .desired_width(width)
                    .hint_text("Search file names or paths…"),
            );
            edit.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::TextEdit,
                    ui.is_enabled(),
                    "Search authored file names or paths",
                )
            });
            changed |= edit.changed();
            if !self.query.is_empty()
                && ui
                    .button("Clear")
                    .on_hover_text("Clear file search")
                    .clicked()
            {
                self.query.clear();
                changed = true;
                edit.request_focus();
            }
        });
        if changed {
            self.offset = 0;
            self.repage();
        }
        let Some(page) = self.page.clone() else {
            return;
        };
        let before = self.filter;
        ui.horizontal_wrapped(|ui| {
            for (filter, title, detail) in Filter::OPTIONS {
                let count = page["counts"][filter.key()].as_u64().unwrap_or_default();
                if filter != Filter::Unknown || count > 0 || self.filter == filter {
                    ui.selectable_value(&mut self.filter, filter, format!("{title}  {count}"))
                        .on_hover_text(detail);
                }
            }
        });
        if self.filter != before {
            self.offset = 0;
            self.repage();
        }
        let Some(page) = &self.page else { return };
        let total = page["total"].as_u64().unwrap_or_default() as usize;
        let mut position = self.offset / PAGE_SIZE;
        home::pager(ui, &mut position, total, PAGE_SIZE, "authored files");
        if position != self.offset / PAGE_SIZE {
            self.offset = position * PAGE_SIZE;
            self.repage();
        }
        ui.separator();
    }
}

enum Action {
    Open(String),
    History(String),
    Activity(String),
}

impl App {
    pub(super) fn profile_authored_files(&mut self, ui: &mut Ui, person: &str, page_scrolls: bool) {
        self.profile_files.controls(ui);
        if let Some(error) = &self.profile_files.error {
            ui.strong("Couldn’t load the file catalogue");
            ui.label(error);
            if ui.button("Retry").clicked() {
                self.profile_files.requested = None;
            }
            return;
        }
        if self.profile_files.reader.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(if self.profile_files.catalog.is_some() {
                    "Checking newer file changes…"
                } else {
                    "Checking file contributions…"
                });
            });
        }
        let Some(page) = self.profile_files.page.clone() else {
            return;
        };
        let key = (
            "profile-authored-files",
            person,
            self.profile_files.filter,
            self.profile_files.query.as_str(),
            self.profile_files.offset,
        );
        let mut action = None;
        scroll_section(ui, key, page_scrolls, |ui| {
            ui.add_space(4.);
            if let Some(unverified) = page["unverified_editions"].as_u64().filter(|n| *n > 0) {
                ui.weak(format!(
                    "{} in {}’s catalogue could not be verified.",
                    counted(unverified, "edition", "editions"),
                    self.label(person)
                ));
            }
            let rows = page["rows"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            if rows.is_empty() {
                ui.weak(if self.profile_files.total() == Some(0) {
                    format!("No verified file contributions by {}.", self.label(person))
                } else if !self.profile_files.query.trim().is_empty() {
                    "No files match this search and filter.".into()
                } else {
                    "No files match this filter.".into()
                });
            }
            for row in rows {
                ui.push_id(s(row, "id"), |ui| {
                    card(ui).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.profile_file_row(ui, person, row, &mut action);
                    });
                    ui.add_space(6.);
                });
            }
        });
        match action {
            Some(Action::Open(reference)) => self.open_resource(ui.ctx(), reference),
            Some(Action::History(reference)) => self.open_file_history(ui.ctx(), &reference),
            Some(Action::Activity(work)) => {
                self.open_home_activity(&work);
                self.live_board.show_person_timeline(&work, person);
            }
            None => {}
        }
    }

    fn profile_file_row(
        &self,
        ui: &mut Ui,
        person: &str,
        row: &Value,
        action: &mut Option<Action>,
    ) {
        let authored = &row["authored"];
        let current = &row["current"];
        let path = s(authored, "path");
        let reference = s(authored, "reference");
        let head = current["reference"].as_str();
        let revision = authored["revision"].as_u64().unwrap_or_default();
        let p = theme::palette(ui);
        let (state, tint) = match s(row, "state") {
            "current" => ("Current content", TEAL),
            "superseded" => ("Superseded", theme::ACCENT),
            "retired" => ("Retired", p.muted),
            _ => ("Current status unknown", p.muted),
        };
        ui.horizontal(|ui| {
            theme::glyph(ui, Icon::Files, p.accent);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 22.),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    theme::badge(ui, state, tint);
                    let width = ui.available_width().max(1.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 22.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(path).size(16.).strong()).truncate(),
                            )
                            .on_hover_text(path);
                        },
                    );
                },
            );
        });
        let writes = row["contribution_count"].as_u64().unwrap_or(1);
        let bytes = authored["bytes"].as_u64();
        let mut details = format!(
            "Latest verified write by {} · revision {revision}",
            self.label(person)
        );
        if let Some(bytes) = bytes {
            details.push_str(&format!(" · {}", size(bytes)));
        }
        if writes > 1 {
            details.push_str(&format!(" · {writes} authored revisions"));
        }
        ui.label(RichText::new(details).small().color(p.muted))
            .on_hover_text(reference);

        if let Some(head) = head.filter(|h| *h != reference) {
            let head_revision = current["revision"].as_u64().unwrap_or_default();
            let origin = &current["content_origin"];
            let detail = if row["state"] == "retired" {
                match origin["person"].as_str() {
                    Some(writer) => format!(
                        "Revision {head_revision} is retired · last content by {}",
                        self.label(writer)
                    ),
                    None => format!(
                        "Revision {head_revision} is retired · last content author unverified"
                    ),
                }
            } else if row["current_uses_authored"] == true {
                match current["editor"]["person"]
                    .as_str()
                    .filter(|_| current["tool"] == "asset.organize")
                {
                    Some(organizer) => format!(
                        "{} organized revision {head_revision} · content from revision {revision}",
                        self.label(organizer)
                    ),
                    None => format!(
                        "Revision {head_revision} retains the content from revision {revision}."
                    ),
                }
            } else if let Some(writer) = origin["person"].as_str() {
                format!(
                    "Current content by {} · library revision {head_revision}",
                    self.label(writer)
                )
            } else {
                format!("Library revision {head_revision} · current content author unverified")
            };
            ui.label(detail).on_hover_text(head);
            if let Some(now) = current["path"].as_str().filter(|now| *now != path) {
                let label = if row["state"] == "unknown" {
                    "Recorded path"
                } else {
                    "Latest path"
                };
                ui.add(egui::Label::new(format!("{label}  {now}")).wrap())
                    .on_hover_text(now);
            }
        }
        let matched = &row["match"];
        if let Some(earlier) = matched["reference"].as_str().filter(|r| *r != reference) {
            let version = earlier.rsplit('@').next().unwrap_or(earlier);
            let label = format!("Matching revision {version} · {}", s(matched, "path"));
            if ui.link(label).on_hover_text(earlier).clicked() {
                *action = Some(Action::Open(earlier.into()));
            }
        }
        ui.add_space(3.);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("Open revision {revision}"))
                .on_hover_text(reference)
                .clicked()
            {
                *action = Some(Action::Open(reference.into()));
            }
            if let Some(head) = head.filter(|h| *h != reference) {
                let label = match row["state"].as_str() {
                    Some("retired") => "Retired file",
                    Some("current" | "superseded") => "Current file",
                    _ => "Library record",
                };
                if ui.button(label).on_hover_text(head).clicked() {
                    *action = Some(Action::Open(head.into()));
                }
            }
            if ui
                .button("History")
                .on_hover_text("View the retained revisions of this file")
                .clicked()
            {
                *action = Some(Action::History(reference.into()));
            }
        });
        if let Some(work) = authored["editor"]["work"].as_str() {
            if let Some(activity) = self.home_index.activity(work) {
                ui.horizontal(|ui| {
                    ui.weak("Activity");
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    let link = ui.link(&activity.title);
                    if link
                        .on_hover_text(format!("Author’s activity\n{}\n{work}", activity.title))
                        .clicked()
                    {
                        *action = Some(Action::Activity(work.into()));
                    }
                });
            }
        }
    }
}
