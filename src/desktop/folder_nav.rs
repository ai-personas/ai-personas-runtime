//! Read-only navigation of authored library paths and original archive members.
use super::theme;
use ai_personas::library_folders;
use eframe::egui::{self, RichText, Ui};

#[derive(Default, Clone)]
pub(super) struct Folders {
    pub directory: String,
}
impl Folders {
    /// Returns true when the user changes the location (reset paging).
    pub fn location(&mut self, ui: &mut Ui, root: &str, searching: bool) -> bool {
        if searching {
            ui.small("Search results · all folders");
            return false;
        }
        let mut go = None;
        ui.horizontal_wrapped(|ui| {
            if self.directory.is_empty() {
                ui.strong(root);
            } else {
                if ui
                    .small_button("↑ Up")
                    .on_hover_text("Parent folder")
                    .clicked()
                {
                    go = Some(
                        self.directory
                            .rsplit_once('/')
                            .map_or("", |p| p.0)
                            .to_owned(),
                    );
                }
                if ui.link(root).on_hover_text("Return to top level").clicked() {
                    go = Some(String::new());
                }
            }
        });
        if !self.directory.is_empty() {
            ui.add(egui::Label::new(RichText::new(&self.directory).small()).truncate())
                .on_hover_text(&self.directory);
        }
        if let Some(dir) = go {
            self.directory = dir;
            true
        } else {
            false
        }
    }
    pub fn open_button(&mut self, ui: &mut Ui, name: &str, files: usize) -> bool {
        let path = library_folders::child(&self.directory, name);
        let hint = format!(
            "{path}/ · {files} {}",
            if files == 1 { "file" } else { "files" }
        );
        let mut enter = false;
        ui.horizontal(|ui| {
            theme::glyph(ui, theme::Icon::Folder, theme::ACCENT);
            let label = RichText::new(format!("{name}  ›")).strong();
            enter = ui
                .add(egui::Button::new(label).truncate())
                .on_hover_text(&hint)
                .clicked();
        });
        if enter {
            self.directory = path;
        }
        enter
    }
}
