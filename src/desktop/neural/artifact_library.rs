//! Navigate actual retained paths. The UI never moves files or assigns categories.
use super::*;
use ai_personas::library_folders::{self, Entry};

impl App {
    pub(super) fn clear_file_preview(&mut self) {
        self.preview_generation += 1;
        self.preview_pending = false;
        self.preview_package = None;
        self.preview_zip_member = None;
        self.selected_reference.clear();
        self.preview_metadata = Value::Null;
        self.viewer.clear_selection();
    }

    pub(super) fn reset_library_view(&mut self) {
        self.artifact_folders.directory.clear();
        self.asset_filter.clear();
        self.asset_owner.clear();
        self.clear_file_preview();
    }

    fn library_filters(&mut self, ui: &mut Ui) {
        let scopes = [
            "Current artifacts",
            "Delivered for this task",
            "Retired artifacts",
        ];
        egui::ComboBox::from_id_salt("artifact-scope")
            .selected_text(scopes[self.artifact_scope])
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (i, text) in scopes.iter().enumerate() {
                    ui.selectable_value(&mut self.artifact_scope, i, *text);
                }
            });
        let owner = if self.asset_owner.is_empty() {
            "All personas"
        } else {
            self.label(&self.asset_owner)
        }
        .to_owned();
        egui::ComboBox::from_id_salt("artifact-owner")
            .selected_text(owner)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.asset_owner, String::new(), "All personas");
                for p in self.records("personas").to_vec() {
                    ui.selectable_value(
                        &mut self.asset_owner,
                        s(&p["body"], "id").into(),
                        s(&p["body"], "label"),
                    );
                }
            });
    }

    pub(super) fn retained_library(&mut self, ui: &mut Ui) {
        let before = (self.artifact_scope, self.asset_owner.clone());
        let field = |ui: &mut Ui, filter: &mut String| {
            ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text("Search all folders…")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("Search across folders by path, filename or discovery cue")
            .changed()
        };
        let edited = if ui.available_height() < 280. {
            ui.horizontal(|ui| {
                ui.menu_button("Filters", |ui| self.library_filters(ui));
                field(ui, &mut self.asset_filter)
            })
            .inner
        } else {
            let edited = field(ui, &mut self.asset_filter);
            self.library_filters(ui);
            edited
        };
        let scope_changed = before != (self.artifact_scope, self.asset_owner.clone());
        if scope_changed {
            self.artifact_folders.directory.clear();
        }
        let query = self.asset_filter.trim().to_lowercase();
        let search = !query.is_empty();
        let mut navigated = self
            .artifact_folders
            .location(ui, "Library folders", search);
        let mut delivered = self.delivered_refs();
        let mut paths = Vec::new();
        for (i, asset) in self.snapshot.assets.iter().enumerate() {
            let b = &asset["body"];
            let member = delivered.remove(s(asset, "reference"));
            let scope = match self.artifact_scope {
                1 => member,
                2 => b["retired"] == true,
                _ => b["retired"] != true && asset["current"] != false,
            };
            let path = s(b, "path");
            if scope
                && (self.asset_owner.is_empty() || b["owner"] == self.asset_owner)
                && (!search
                    || path.to_lowercase().contains(&query)
                    || b["cues"].to_string().to_lowercase().contains(&query))
            {
                paths.push((i, path));
            }
        }
        // Keep exact delivered references reachable even if their paths were
        // not projected. Do not invent a filename, parent, size or author.
        let absent: Vec<_> = if self.artifact_scope == 1 && !search && self.asset_owner.is_empty() {
            delivered.into_iter().collect()
        } else {
            Vec::new()
        };
        let known = self.snapshot.assets.len();
        paths.extend(
            absent
                .iter()
                .enumerate()
                .map(|(i, r)| (known + i, r.as_str())),
        );
        let total = paths.len();
        let rows = library_folders::entries(
            (!search).then_some(&self.artifact_folders.directory[..]),
            paths,
        );
        let folders = rows
            .iter()
            .filter(|r| matches!(r, Entry::Folder { .. }))
            .count();
        if search {
            ui.small(counted(total as u64, "matching file", "matching files"));
        } else {
            ui.small(format!(
                "{} · {} here",
                counted(folders as u64, "folder", "folders"),
                counted((rows.len() - folders) as u64, "file", "files")
            ));
        }
        if rows.is_empty() {
            ui.weak(if search {
                "No artifacts match these filters."
            } else {
                "No files in this folder under these filters."
            });
        }
        let height = self.compact_artifact_height(ui, search);
        let scroll = egui::Id::new((
            "library-folder-rows",
            &self.artifact_folders.directory,
            &query,
            self.artifact_scope,
            &self.asset_owner,
        ));
        egui::ScrollArea::vertical()
            .id_salt(scroll)
            .auto_shrink([false, false])
            .show_rows(ui, height, rows.len(), |ui, range| {
                for i in range {
                    match &rows[i] {
                        Entry::Folder { name, files } => {
                            ui.push_id(("folder", name), |ui| {
                                ui.vertical(|ui| {
                                    ui.set_min_height(height);
                                    navigated |=
                                        self.artifact_folders.open_button(ui, name, *files);
                                    ui.small(counted(
                                        *files as u64,
                                        "file in folder",
                                        "files in folder",
                                    ));
                                });
                            });
                        }
                        Entry::File(index) if *index < known => {
                            let asset = self.snapshot.assets[*index].clone();
                            ui.push_id(s(&asset, "reference"), |ui| {
                                self.artifact_row(ui, &asset, true)
                            });
                            ui.add_space(4.);
                        }
                        Entry::File(index) => {
                            let reference = &absent[index - known];
                            if ui
                                .link(reference)
                                .on_hover_text(
                                    "Open this exact issued reference; library path unavailable",
                                )
                                .clicked()
                            {
                                self.open_resource(ui.ctx(), reference.clone());
                            }
                        }
                    }
                }
            });
        if navigated || edited || scope_changed {
            self.clear_file_preview();
        }
    }
}
