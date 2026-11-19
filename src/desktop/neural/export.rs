//! The save path is part of the workspace UI so exports also work on desktops
//! without a file-chooser portal. Only the explicit Save action writes files.
use super::*;
use std::io::Write;

#[derive(Clone)]
enum Contents {
    Artifact { reference: String, metadata_view: bool, source: Option<(String, String)> },
    Deliveries(String),
    Package(String),
    Handoff { reference: String, files: usize },
}

fn title_stem(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join("-").chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).take(72).collect()
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
    pub(super) fn export_package(&mut self, reference: &str, title: &str) {
        self.export_generation += 1;
        let name = title_stem(title);
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::Package(reference.into()), format!("{name}.zip")));
    }
    pub(super) fn save_request_handoff(&mut self, reference: &str, title: &str, files: usize) {
        self.save_handoff(reference, title, files, "Request");
    }
    pub(super) fn save_delivery_handoff(&mut self, reference: &str, title: &str, files: usize) {
        self.save_handoff(reference, title, files, "Delivery");
    }
    fn save_handoff(&mut self, reference: &str, title: &str, files: usize, fallback: &str) {
        let stem = title_stem(title);
        let folder = if stem.is_empty() { format!("{fallback}-handoff") } else { format!("{stem}-handoff") };
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Handoff { reference: reference.into(), files },
            folder,
        ));
    }
    pub(super) fn save_artifact(&mut self, _ctx: &egui::Context) {
        if self.selected_reference.is_empty() || self.preview_metadata.is_null() {
            return;
        }
        let path = s(&self.preview_metadata, "path");
        let source = self.preview_metadata["source_record"].as_str()
            .map(|record| (record.to_owned(), path.to_owned()));
        let metadata_view = self.preview_metadata["view"] == "provider_receipt_metadata";
        let name = if metadata_view {
            "provider-receipt-metadata.json".to_owned()
        } else {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("resource.json")
                .to_owned()
        };
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Artifact { reference: self.selected_reference.clone(), metadata_view, source },
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
        let file = matches!(dialog.contents, Contents::Artifact { .. } | Contents::Package(_));
        let (heading, description, action) = match &dialog.contents {
            Contents::Artifact { metadata_view: true, .. } => (
                "Save receipt metadata", "Save the public receipt view, including its audit and usage.", "Save file"),
            Contents::Artifact { .. } => ("Save artifact", "Save the exact retained file to your computer.", "Save file"),
            Contents::Deliveries(_) => ("Export deliveries", "Export this task's delivered files with their delivery records.", "Export deliveries"),
            Contents::Package(_) => ("Save file package", "The ZIP contains the selected file revisions with their original names and folders.", "Save file"),
            Contents::Handoff { reference, .. } if reference.starts_with("delivery:") => (
                "Save delivery handoff",
                "The new folder contains a printable Delivery.html with the author’s scope and remaining work, plus Files.zip with the exact issued files.",
                "Save handoff"),
            Contents::Handoff { files: 0, .. } => ("Save handoff", "Open Request.html in the new folder to read the request offline.", "Save handoff"),
            Contents::Handoff { .. } => ("Save handoff", "The new folder contains Request.html for offline reading and Files.zip with the requester’s attached files.", "Save handoff"),
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(heading)
        .id(egui::Id::new("studio-export"))
        .open(&mut open)
        .default_width(560.)
        .max_width((ctx.content_rect().width() - 32.).max(160.))
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(description);
            ui.add_space(12.);
            ui.add_enabled_ui(!dialog.busy, |ui| {
                ui.strong("Destination folder");
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.directory).desired_width(f32::INFINITY),
                );
                ui.add_space(8.);
                ui.strong(if file {
                    "File name"
                } else {
                    "New folder name"
                });
                ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(f32::INFINITY));
                ui.add_space(8.);
                ui.small("Choose a new name if a file or folder already exists.");
                ui.horizontal_wrapped(|ui| {
                    save = ui
                        .add_enabled(
                            !dialog.directory.trim().is_empty() && !dialog.name.trim().is_empty(),
                            theme::primary(action),
                        )
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
            if dialog.busy {
                ui.label("Saving…");
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
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
            dialog.error = Some(format!("Enter a {} name without folder separators.", if file { "file" } else { "folder" }));
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
                    Contents::Package(reference) => {
                        let db = data::open(&root).map_err(|e|e.to_string())?;
                        ai_personas::neural::human_requests::export_package(&db, &reference, &path).map_err(|e|e.to_string())?;
                    }
                    Contents::Handoff { reference, .. } => {
                        let db = data::open(&root).map_err(|e| e.to_string())?;
                        ai_personas::neural::human_requests::export_handoff(&db, &reference, &path)
                            .map_err(|e| e.to_string())?;
                    }
                    Contents::Artifact { reference, source, .. } => {
                        let preview = data::preview_output(&root, &reference, source.as_ref())
                            .map_err(|e| e.to_string())?;
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
