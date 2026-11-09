//! The save path is part of the workspace UI so exports also work on desktops
//! without a file-chooser portal. Only the explicit Save action writes files.
use super::*;
use std::io::Write;

#[derive(Clone)]
enum Contents {
    Artifact(String),
    Deliveries(String),
}
pub(super) struct Dialog {
    pub serial: u64,
    contents: Contents,
    directory: String,
    name: String,
    pub busy: bool,
    pub error: Option<String>,
}
impl Dialog {
    fn new(serial: u64, contents: Contents, name: String) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let downloads = home.join("Downloads");
        Self {
            serial,
            contents,
            directory: if downloads.is_dir() { downloads } else { home }
                .display()
                .to_string(),
            name,
            busy: false,
            error: None,
        }
    }
}
impl App {
    pub(super) fn save_artifact(&mut self, _ctx: &egui::Context) {
        if self.selected_reference.is_empty() {
            return;
        }
        let path = s(&self.preview_metadata, "path");
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("resource.json")
            .to_owned();
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Artifact(self.selected_reference.clone()),
            name,
        ));
    }
    pub(super) fn export_deliveries(&mut self, _ctx: &egui::Context) {
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Deliveries(self.work.clone()),
            format!("personas-delivery-{}", ai_personas::neural::now()),
        ));
    }
    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        let artifact = matches!(dialog.contents, Contents::Artifact(_));
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if artifact {
            "Save artifact"
        } else {
            "Export deliveries"
        })
        .id(egui::Id::new("studio-export"))
        .open(&mut open)
        .default_width(560.)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(if artifact {
                "Save the exact retained file to your computer."
            } else {
                "Export this task's delivered files with their delivery records."
            });
            ui.add_space(12.);
            ui.add_enabled_ui(!dialog.busy, |ui| {
                ui.strong("Destination folder");
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.directory).desired_width(f32::INFINITY),
                );
                ui.add_space(8.);
                ui.strong(if artifact {
                    "File name"
                } else {
                    "New folder name"
                });
                ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(f32::INFINITY));
                ui.add_space(8.);
                ui.small("Choose a new name if a file or folder already exists.");
                ui.horizontal(|ui| {
                    save = ui
                        .add_enabled(
                            !dialog.directory.trim().is_empty() && !dialog.name.trim().is_empty(),
                            theme::primary(if artifact {
                                "Save file"
                            } else {
                                "Export deliveries"
                            }),
                        )
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
            if dialog.busy {
                ui.label("Saving…");
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
        if !open || cancel {
            self.export_dialog = None;
            return;
        }
        if !save {
            return;
        }
        let name = std::path::Path::new(&dialog.name);
        if name.file_name() != Some(name.as_os_str()) || matches!(dialog.name.as_str(), "." | "..")
        {
            dialog.error = Some("Enter a file name without folder separators.".into());
            return;
        }
        let path = PathBuf::from(&dialog.directory).join(name);
        dialog.busy = true;
        dialog.error = None;
        let serial = dialog.serial;
        let (root, contents, tx, ctx) = (
            self.root.clone(),
            dialog.contents.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                if !path.parent().is_some_and(|parent| parent.is_dir()) {
                    return Err("The destination folder does not exist.".into());
                }
                match contents {
                    Contents::Artifact(reference) => {
                        let preview =
                            data::preview(&root, &reference).map_err(|e| e.to_string())?;
                        let bytes = std::fs::read(&preview.path).map_err(|e| e.to_string())?;
                        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())
                            .map_err(|e| e.to_string())?;
                        file.write_all(&bytes)
                            .and_then(|_| file.as_file().sync_all())
                            .map_err(|e| e.to_string())?;
                        file.persist_noclobber(&path)
                            .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
                    }
                    Contents::Deliveries(work) => {
                        tokio::runtime::Runtime::new()
                            .map_err(|e| e.to_string())?
                            .block_on(cli::execute(
                                &root,
                                "user",
                                Command::Export {
                                    directory: path.clone(),
                                    work: if work.is_empty() { None } else { Some(work) },
                                },
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                }
                Ok(format!("Saved {}", path.display()))
            })();
            let _ = tx.send(Update::Saved(serial, result));
            ctx.request_repaint();
        });
    }
}
