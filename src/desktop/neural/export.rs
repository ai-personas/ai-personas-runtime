//! The save path is part of the workspace UI so exports also work on desktops
//! without a file-chooser portal. Only the explicit Save action writes files.
use super::*;
use ai_personas::neural::human_requests::HandoffFormat;
use std::io::Write;

#[derive(Clone)]
enum Contents {
    Artifact { reference: String, metadata_view: bool, source: Option<(String, String)>, member: Option<Member> },
    Printed { source: print_preview::Source, document: print_preview::Document },
    Deliveries(String),
    Package(String),
    Handoff { reference: String, files: usize, format: HandoffFormat },
}

#[derive(Clone)]
struct Member {
    delivery: String,
    path: String,
    digest: String,
}

fn basename(path: &str) -> &str {
    std::path::Path::new(path).file_name().and_then(|name| name.to_str())
        .filter(|name| !name.is_empty()).unwrap_or("resource.json")
}

fn save_new_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist_noclobber(path)
        .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
    Ok(())
}

fn title_stem(title: &str, fallback: &str) -> String {
    let words: Vec<_> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return fallback.to_owned();
    }
    let stem: String = words.join("-").chars().take(72).collect();
    stem.trim_end_matches('-').to_owned()
}
pub(super) struct Dialog {
    pub serial: u64,
    contents: Contents,
    directory: String,
    name: String,
    focus_on_open: bool,
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
            focus_on_open: true,
            busy: false,
            error: None,
        }
    }
}
impl App {
    pub(super) fn save_print_pdf(&mut self, source: &print_preview::Source, document: &print_preview::Document) {
        let name = std::path::Path::new(basename(&source.path)).with_extension("pdf");
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Printed { source: source.clone(), document: document.clone() },
            name.to_string_lossy().into_owned(),
        ));
    }
    pub(super) fn export_package(&mut self, reference: &str, title: &str) {
        self.export_generation += 1;
        let name = title_stem(title, "Delivery");
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::Package(reference.into()), format!("{name}.zip")));
    }
    pub(super) fn save_request_handoff(&mut self, reference: &str, title: &str, files: usize) {
        self.save_handoff(reference, title, files, "Request");
    }
    pub(super) fn save_delivery_handoff(&mut self, reference: &str, title: &str, files: usize) {
        self.save_handoff(reference, title, files, "Delivery");
    }
    fn save_handoff(&mut self, reference: &str, title: &str, files: usize, fallback: &str) {
        let stem = title_stem(title, fallback);
        let folder = format!("{stem}-handoff");
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Handoff { reference: reference.into(), files, format: HandoffFormat::Zip },
            format!("{folder}.zip"),
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
            basename(path).to_owned()
        };
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Artifact { reference: self.selected_reference.clone(), metadata_view, source, member: None },
            name,
        ));
    }
    pub(super) fn save_package_member(&mut self, delivery: &str, reference: &str, path: &str, digest: &str) {
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Artifact {
                reference: reference.into(), source: None, metadata_view: false,
                member: Some(Member { delivery: delivery.into(), path: path.into(), digest: digest.into() }),
            },
            basename(path).into(),
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
        let (heading, description, action) = match &dialog.contents {
            Contents::Artifact { metadata_view: true, .. } => (
                "Save receipt metadata", "Save the public receipt view, including its audit and usage.", "Save file"),
            Contents::Artifact { member: Some(_), .. } => ("Save package file", "Save this exact issued file to your computer.", "Save file"),
            Contents::Artifact { .. } => ("Save artifact", "Save the exact retained file to your computer.", "Save file"),
            Contents::Printed { .. } => ("Save print PDF", "Save the PDF generated for this preview.", "Save PDF"),
            Contents::Deliveries(_) => ("Export deliveries", "Export this task's delivered files with their delivery records.", "Export deliveries"),
            Contents::Package(_) => ("Save file package", "The ZIP contains the selected file revisions with their original names and folders.", "Save file"),
            Contents::Handoff { reference, .. } if reference.starts_with("delivery:") =>
                ("Save delivery handoff", "The recipient gets the issuer’s instructions and the exact files in this edition.", "Save handoff"),
            Contents::Handoff { .. } =>
                ("Save request handoff", "The recipient can read the request offline and bring back a field report for an active request.", "Save handoff"),
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(heading)
        .id(egui::Id::new("studio-export"))
        .order(egui::Order::Foreground)
        .open(&mut open)
        .default_width(560.)
        .max_width((ctx.content_rect().width() - 32.).max(160.))
        .max_height((ctx.content_rect().height() - 48.).max(120.))
        .vscroll(true)
        .collapsible(false)
        .show(ctx, |ui| {
            if let Contents::Handoff { reference, files, format } = &mut dialog.contents {
                let delivery = reference.starts_with("delivery:");
                egui::Frame::new().fill(theme::palette(ui).header).corner_radius(7).inner_margin(12).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    eyebrow(ui, "Persona Studio  /  Outgoing handoff");
                    ui.label(RichText::new(if *format == HandoffFormat::Zip { "One file to send" } else { "A folder to open locally" }).size(21.).strong());
                    ui.label(description);
                    ui.add_space(6.);
                    ui.horizontal_wrapped(|ui| {
                        theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                        ui.strong(if delivery { "Delivery.html" } else { "Request.html" });
                        ui.small(if delivery { "Supported use and remaining work" } else { "Requested action and what to return" });
                    });
                    if *files > 0 {
                        ui.horizontal_wrapped(|ui| {
                            theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                            ui.strong("Files.zip");
                            ui.small(format!("{} with original paths and bytes", counted(*files as u64, "file", "files")));
                        });
                    }
                });
                let previous = *format;
                ui.add_enabled_ui(!dialog.busy, |ui| ui.horizontal_wrapped(|ui| {
                    ui.strong("Save as");
                    ui.selectable_value(format, HandoffFormat::Zip, "Shareable ZIP");
                    ui.selectable_value(format, HandoffFormat::Folder, "Folder");
                }));
                if *format != previous {
                    if *format == HandoffFormat::Folder {
                        if let Some(stem) = dialog.name.strip_suffix(".zip") {
                            dialog.name = stem.to_owned();
                        }
                    } else if !dialog.name.ends_with(".zip") {
                        dialog.name.push_str(".zip");
                    }
                }
            } else {
                ui.label(description);
            }
            let file = !matches!(dialog.contents, Contents::Handoff { format: HandoffFormat::Folder, .. } | Contents::Deliveries(_));
            if let Contents::Artifact { reference, member: Some(member), .. } = &dialog.contents {
                let edition = reference.rsplit_once('@').map(|(_, n)| n).unwrap_or(reference);
                ui.small(format!("{}  ·  Revision {edition}", member.path)).on_hover_text(reference);
            }
            if let Contents::Printed { source, document } = &dialog.contents {
                ui.strong(&source.path);
                ui.small(format!("{}  ·  {}", counted(document.pages.into(), "page", "pages"), size(document.pdf.len() as u64)));
                ui.small(&source.reference);
                if let Some(package) = &source.package { ui.small(package); }
            }
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
                let filename = ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(f32::INFINITY));
                if std::mem::take(&mut dialog.focus_on_open) {
                    ctx.move_to_top(ui.layer_id());
                    filename.request_focus();
                }
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
            if let Some(preview) = &mut self.print_preview {
                preview.finish_export(dialog.serial, None);
            }
            self.export_dialog = None;
            return;
        }
        if !save {
            return;
        }
        let name = std::path::Path::new(&dialog.name);
        if name.file_name() != Some(name.as_os_str()) || matches!(dialog.name.as_str(), "." | "..")
        {
            dialog.error = Some("Enter a name without folder separators.".into());
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
                    Contents::Printed { document, .. } => {
                        save_new_file(&path, &document.pdf)?;
                        return Ok(format!("Saved print PDF to {}", path.display()));
                    }
                    Contents::Package(reference) => {
                        let db = data::open(&root).map_err(|e|e.to_string())?;
                        ai_personas::neural::human_requests::export_package(&db, &reference, &path).map_err(|e|e.to_string())?;
                    }
                    Contents::Handoff { reference, format, .. } => {
                        let db = data::open(&root).map_err(|e| e.to_string())?;
                        ai_personas::neural::human_requests::export_handoff(&db, &reference, &path, format)
                            .map_err(|e| e.to_string())?;
                    }
                    Contents::Artifact { reference, source, member, .. } => {
                        if let Some(selected) = &member {
                            let db = data::open(&root).map_err(|e| e.to_string())?;
                            ai_personas::neural::asset_history::verify_delivery_member(&db, &selected.delivery, &reference)
                                .map_err(|e| e.to_string())?;
                        }
                        let preview = data::preview_output(&root, &reference, source.as_ref())
                            .map_err(|e| e.to_string())?;
                        let bytes = std::fs::read(&preview.path).map_err(|e| e.to_string())?;
                        if let Some(selected) = &member {
                            if preview.metadata["path"] != selected.path || preview.metadata["blob"] != selected.digest
                                || preview.metadata["view"] == "provider_receipt_metadata"
                                || ai_personas::store::sha256_bytes(&bytes) != selected.digest {
                                return Err("The retained file differs from the selected package member.".into());
                            }
                        }
                        save_new_file(&path, &bytes)?;
                        if member.is_some() {
                            let edition = reference.rsplit_once('@').map(|(_, n)| n).unwrap_or(&reference);
                            return Ok(format!("Saved file revision {edition} to {}", path.display()));
                        }
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
