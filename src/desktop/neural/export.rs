//! The save path is part of the workspace UI so exports also work on desktops
//! without a file-chooser portal. Only the explicit Save action writes files.
use super::*;
use ai_personas::neural::human_requests::HandoffFormat;
use std::io::Write;
mod packet;
use packet::Packet;

#[derive(Clone)]
enum Contents {
    Artifact { reference: String, digest: String, metadata_view: bool, source: Option<(String, String)>, entry: Option<artifacts::ViewedMember> },
    PackageMember { reference: String, member: Member },
    Printed { source: print_preview::Source, document: print_preview::Document },
    Deliveries { work: Option<String>, label: String, editions: Vec<ListedEdition> },
    Package(Packet),
    ReviewFile(ai_personas::neural::package_review::EvidenceFile),
    Dispatch { delivery: String, number: usize, recipient: String, files: usize, packet: bool },
    Handoff { packet: Packet, files: usize, format: HandoffFormat },
}

#[derive(Clone)]
struct ListedEdition {
    reference: String,
    digest: String,
    title: String,
}

#[derive(Clone)]
struct Member {
    edition: String,
    path: String,
    digest: String,
    entry: Option<delivery_desk::ZipFile>,
}

impl Member {
    fn save(&self, root: &std::path::Path, reference: &str, destination: &std::path::Path) -> Result<String, String> {
        use ai_personas::neural::human_requests::Materials;
        let db = data::open(root).map_err(|e| e.to_string())?;
        let file = Materials::open(&db, &self.edition).and_then(|edition| edition.read(reference)).map_err(|e| e.to_string())?;
        drop(db);
        if file.path != self.path || file.digest != self.digest {
            return Err("The retained file differs from the selected package member.".into());
        }
        if let Some(entry) = &self.entry {
            save_new_file_with(destination, |temp| entry.copy_to(&file.bytes, temp).map_err(|e| e.to_string()))?;
            Ok(format!("Saved {} from the issued ZIP to {}", entry.path, destination.display()))
        } else {
            save_new_file(destination, &file.bytes)?;
            Ok(format!("Saved {} from the selected edition to {}", file.path, destination.display()))
        }
    }
}

fn basename(path: &str) -> &str {
    std::path::Path::new(path).file_name().and_then(|name| name.to_str())
        .filter(|name| !name.is_empty()).unwrap_or("resource.json")
}

fn save_new_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    save_new_file_with(path, |file| file.write_all(bytes).map_err(|e| e.to_string()))
}

fn save_new_file_with(
    path: &std::path::Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<(), String>,
) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|e| e.to_string())?;
    write(file.as_file_mut())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
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
    busy: bool,
    error: Option<String>,
    reveal_error: bool,
}
impl Dialog {
    pub(super) fn failed(&mut self, error: String) {
        self.busy = false;
        self.error = Some(error);
        self.reveal_error = true;
    }

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
            reveal_error: false,
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
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::Package(Packet::Delivery(reference.into())), format!("{name}.zip")));
    }
    pub(super) fn export_closure_files(&mut self, reference: String, source: String, title: &str) {
        self.export_generation += 1;
        let revision = reference.rsplit_once('@').map_or("", |(_, n)| n);
        let name = format!("{}-edition-{revision}.zip", title_stem(title, "Outside-step"));
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::Package(Packet::Record { reference, digest: source.strip_prefix("object:").unwrap_or("").into() }), name));
    }
    pub(super) fn save_dispatch(&mut self, delivery: &str, number: usize, recipient: &str, files: usize, original: Option<&str>, packet: bool) {
        self.export_generation += 1;
        let name = if packet {
            format!("{number:02}-{}-packet.zip", title_stem(recipient, "Dispatch"))
        } else {
            original.map(basename).map(str::to_owned).unwrap_or_else(|| format!("{number:02}-{}.zip", title_stem(recipient, "Dispatch")))
        };
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::Dispatch {
            delivery: delivery.into(), number, recipient: recipient.into(), files, packet,
        }, name));
    }
    pub(super) fn save_review_file(&mut self, file: ai_personas::neural::package_review::EvidenceFile) {
        self.export_generation += 1;
        let name = file.name.clone();
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::ReviewFile(file), name));
    }
    pub(super) fn save_delivery_handoff(&mut self, reference: &str, title: &str, files: usize) {
        self.save_handoff(Packet::Delivery(reference.into()), title, files);
    }
    fn save_handoff(&mut self, packet: Packet, title: &str, files: usize) {
        let folder = match &packet {
            Packet::Delivery(_) => format!("{}-handoff", title_stem(title, "Delivery")),
            _ => format!("{}-edition-{}-handoff", title_stem(title, "Request"), packet.version()),
        };
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Handoff { packet, files, format: HandoffFormat::Zip },
            format!("{folder}.zip"),
        ));
    }
    pub(super) fn save_artifact(&mut self, entry: Option<artifacts::ViewedMember>) {
        if self.selected_reference.is_empty() || self.preview_metadata.is_null() {
            return;
        }
        if let Some(delivery) = self.preview_metadata["package_edition"].as_str().filter(|_| entry.is_none()) {
            let (delivery, file, path, digest) = (delivery.to_owned(), self.selected_reference.clone(),
                s(&self.preview_metadata, "path").to_owned(), s(&self.preview_metadata, "blob").to_owned());
            self.save_package_member(&delivery, &file, &path, &digest, None);
            return;
        }
        let path = s(&self.preview_metadata, "path");
        let source = self.preview_metadata["package_edition"].as_str()
            .or_else(|| self.preview_metadata["source_record"].as_str())
            .or_else(|| self.preview_metadata["command_run"].as_str())
            .map(|record| (record.to_owned(), path.to_owned()));
        let file = &self.preview_metadata["preview_file"];
        let metadata_view = self.preview_metadata["view"] == "provider_receipt_metadata";
        if let Some(entry) = entry {
            self.save_viewed_member(self.selected_reference.clone(), source, s(file, "sha256").to_owned(), entry);
            return;
        }
        let name = if metadata_view {
            "provider-receipt-metadata.json".to_owned()
        } else {
            basename(s(file, "name")).to_owned()
        };
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Artifact { reference: self.selected_reference.clone(), digest: s(file, "sha256").to_owned(), metadata_view, source, entry: None },
            name,
        ));
    }
    pub(super) fn save_viewed_member(&mut self, reference: String, source: Option<(String, String)>, digest: String, entry: artifacts::ViewedMember) {
        let name = basename(entry.name()).to_owned();
        self.export_generation += 1;
        self.export_dialog = Some(Dialog::new(self.export_generation, Contents::Artifact {
            reference, digest, metadata_view: false, source, entry: Some(entry),
        }, name));
    }
    pub(super) fn save_package_member(&mut self, edition: &str, reference: &str, path: &str, digest: &str, entry: Option<delivery_desk::ZipFile>) {
        self.export_generation += 1;
        let name = basename(entry.as_ref().map_or(path, |e| &e.path)).to_owned();
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::PackageMember {
                reference: reference.into(),
                member: Member { edition: edition.into(), path: path.into(), digest: digest.into(), entry },
            },
            name,
        ));
    }
    pub(super) fn export_deliveries(&mut self) {
        self.export_generation += 1;
        let work = (!self.work.is_empty()).then(|| self.work.clone());
        let label = work.as_deref().map_or_else(|| "All activities in this workspace".into(), |id| self.work_name(id));
        let editions = self.records("deliveries").iter().filter(|d| work.as_deref().is_none_or(|w| d["body"]["work"] == w))
            .map(|d| ListedEdition { reference: inbox::handle(d), digest: s(&d["reference"], "digest").into(), title: s(&d["body"], "title").into() }).collect();
        self.export_dialog = Some(Dialog::new(
            self.export_generation,
            Contents::Deliveries { work, label, editions },
            format!("personas-delivery-{}", ai_personas::neural::now()),
        ));
    }
    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        let (heading, description, action) = match &dialog.contents {
            Contents::Artifact { entry: Some(_), .. } => ("Save archive member", "Save the original member bytes, not the enclosing archive or a rendered preview.", "Save member"),
            Contents::Artifact { metadata_view: true, .. } => (
                "Save receipt metadata", "Save the public receipt view, including its audit and usage.", "Save file"),
            Contents::PackageMember { member, .. } if member.entry.is_some() =>
                ("Save file from issued ZIP", "Save the original bytes of this entry from the selected delivery.", "Save file"),
            Contents::PackageMember { .. } => ("Save package file", "Save this exact issued file to your computer.", "Save file"),
            Contents::Artifact { .. } => ("Save artifact", "Save the exact retained file to your computer.", "Save file"),
            Contents::Printed { .. } => ("Save print PDF", "Save the PDF generated for this preview.", "Save PDF"),
            Contents::Deliveries { .. } => ("Export deliveries", "Each edition gets its own folder. The manifest records its files and reviews. This is not a combined build package.", "Export deliveries"),
            Contents::Dispatch { packet: true, .. } => ("Save recipient packet", "Dispatch.html contains this recipient’s instructions and flags later edition notes without sharing the reports. Files/ holds only the original files on this slip.", "Save packet"),
            Contents::Dispatch { files: 1, .. } => ("Save dispatch file", "This is the original file, not a new ZIP around it.", "Save original"),
            Contents::Dispatch { .. } => ("Save dispatch files", "Only the files on this slip are included, at their issued paths.", "Save ZIP"),
            Contents::ReviewFile(_) => ("Save reviewer’s file", "Save the named evidence from this published finding. It is not a file issued by the delivery owner.", "Save original"),
            Contents::Package(Packet::Record { .. }) => ("Save files at closure", "Save a local ZIP of this historical request’s files. This does not take the step, confirm receipt or send a current instruction.", "Save ZIP"),
            Contents::Package(Packet::Current { .. }) => ("Save current request files", "Save only the displayed edition’s attachments, without a request sheet. If the request has changed when you save, no ZIP is written.", "Save files"),
            Contents::Package(Packet::Delivery(_)) => ("Save file package", "The ZIP contains the selected file revisions with their original names and folders.", "Save file"),
            Contents::Handoff { packet: Packet::Delivery(_), .. } =>
                ("Save delivery handoff", "Send this edition as issued. The sheet separates the issuer’s instructions from reviews and citations verified when you save.", "Save handoff"),
            Contents::Handoff { packet: Packet::Current { .. }, .. } =>
                ("Save request handoff", "Take this edition’s instructions and files together. If the current request changes before you save, reopen it to see what changed.", "Save handoff"),
            Contents::Handoff { packet: Packet::Record { .. }, .. } =>
                ("Save packet for the record", "Keep the instructions and files from this issue. The sheet records what could be verified about the current request at export.", "Save edition"),
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let mut reopen = None;
        let viewport = ctx.content_rect().shrink(6.);
        let available = (viewport.size() - theme::titled_window_chrome(ctx)).max(egui::vec2(1., 1.));
        egui::Window::new(heading)
        .id(egui::Id::new("studio-export"))
        .order(egui::Order::Foreground)
        .open(&mut open)
        .default_size(egui::vec2(560., 420.).min(available))
        .min_size(egui::vec2(96., 32.).min(available))
        .max_size(available)
        .constrain_to(viewport)
        .vscroll(true)
        .collapsible(false)
        .show(ctx, |ui| {
            if let Contents::Handoff { packet, files, format } = &mut dialog.contents {
                let delivery = matches!(packet, Packet::Delivery(_));
                egui::Frame::new().fill(theme::palette(ui).header).corner_radius(7).inner_margin(12).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    eyebrow(ui, "Persona Studio  /  Outgoing handoff");
                    let title = match packet {
                        Packet::Current { .. } => format!("Field desk · edition {}", packet.version()),
                        Packet::Record { .. } => format!("Issue record · edition {}", packet.version()),
                        _ if *format == HandoffFormat::Zip => "One file to send".into(),
                        _ => "A folder to open locally".into(),
                    };
                    ui.label(RichText::new(title).size(21.).strong()).on_hover_text(packet.reference());
                    ui.label(description);
                    ui.add_space(6.);
                    ui.horizontal_wrapped(|ui| {
                        theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                        ui.strong(if delivery { "Delivery.html" } else { "Request.html" });
                        ui.small(if delivery { "Issued terms, findings and later editions to look up" } else { "Requested action and what to return" });
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
            } else if let Contents::Dispatch { delivery, number, recipient, files, .. } = &dialog.contents {
                eyebrow(ui, &format!("Persona Studio  /  Dispatch {number:02}"));
                ui.add(egui::Label::new(RichText::new(recipient).size(21.).strong()).wrap());
                ui.label(description);
                ui.small(format!("{} · Local copy only", counted(*files as u64, "file", "files"))).on_hover_text(delivery);
            } else {
                if let Contents::Deliveries { work, label, editions } = &dialog.contents {
                    eyebrow(ui, if work.is_some() { "Issued editions  /  Selected activity" } else { "Issued editions  /  Whole workspace" });
                    ui.add(egui::Label::new(RichText::new(home::short_text(label, 110)).strong()).wrap());
                    ui.push_id(("listed-editions", dialog.serial), |ui| ui.collapsing(counted(editions.len() as u64, "issued edition", "issued editions"), |ui| {
                        if let Some(work) = work { ui.small(work); }
                        for edition in editions {
                            ui.add(egui::Label::new(&edition.title).wrap());
                            ui.add(egui::Label::new(RichText::new(&edition.reference).small().weak()).wrap().selectable(true));
                        }
                    }));
                }
                ui.label(description);
            }
            let file = !matches!(dialog.contents, Contents::Handoff { format: HandoffFormat::Folder, .. } | Contents::Deliveries { .. });
            if let Contents::Package(packet @ (Packet::Record { .. } | Packet::Current { .. })) = &dialog.contents {
                ui.strong(format!("Request edition {}", packet.version())).on_hover_text(packet.reference());
            }
            if let Contents::PackageMember { reference, member } = &dialog.contents {
                let edition = reference.rsplit_once('@').map(|(_, n)| n).unwrap_or(reference);
                if let Some(entry) = &member.entry {
                    ui.add(egui::Label::new(RichText::new(&entry.path).strong()).wrap());
                }
                ui.small(format!("{}  ·  Revision {edition}", member.path)).on_hover_text(reference);
                ui.small(&member.edition);
            }
            if let Contents::Artifact { entry: Some(entry), reference, source, .. } = &dialog.contents {
                let original = source.as_ref().map_or(entry.original(), |(_, path)| path.as_str());
                ui.small(format!("Original archive · {original}")).on_hover_text(reference);
                for name in entry.paths() {
                    ui.add(egui::Label::new(RichText::new(name).strong()).wrap());
                }
                if let Some((origin, _)) = source { ui.small(origin); }
            }
            if let Contents::ReviewFile(selected) = &dialog.contents {
                ui.strong(&selected.name);
                ui.small(&selected.finding);
                ui.small(&selected.object);
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
                let mut last = ui.colored_label(ui.visuals().error_fg_color, error);
                if let Contents::Package(Packet::Current { reference, .. }) | Contents::Handoff { packet: Packet::Current { reference, .. }, .. } = &dialog.contents {
                    last = ui.link("Back to the request →");
                    if last.clicked() {
                        reopen = reference.strip_prefix("human_request:").and_then(|r| r.rsplit_once('@')).map(|(id, _)| id.to_owned());
                    }
                }
                if std::mem::take(&mut dialog.reveal_error) {
                    last.scroll_to_me(Some(egui::Align::Max));
                }
            }
        });
        if let Some(id) = reopen {
            self.export_dialog = None;
            self.show_human_request(&id);
            self.last_poll = Instant::now() - Duration::from_secs(3);
            return;
        }
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
            dialog.failed("Enter a name without folder separators.".into());
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
                    Contents::Dispatch { delivery, number, packet, .. } => {
                        let db = data::open(&root).map_err(|e| e.to_string())?;
                        use ai_personas::neural::human_requests::{export_dispatch,export_dispatch_packet};
                        let export = if packet { export_dispatch_packet } else { export_dispatch };
                        export(&db, &delivery, number, &path).map_err(|e| e.to_string())?;
                        return Ok(format!("Saved dispatch {number} to {}", path.display()));
                    }
                    Contents::ReviewFile(file) => {
                        let db = data::open(&root).map_err(|e| e.to_string())?;
                        file.save(&db, &path).map_err(|e| e.to_string())?;
                    }
                    Contents::Package(packet) => {
                        let db = data::open(&root).map_err(|e| e.to_string())?;
                        packet.check(&db)?;
                        ai_personas::neural::human_requests::export_package(&db, packet.reference(), &path).map_err(|e|e.to_string())?;
                    }
                    Contents::Handoff { packet, format, .. } => {
                        let db = data::open(&root).map_err(|e| e.to_string())?;
                        packet.check(&db)?;
                        ai_personas::neural::human_requests::export_handoff(&db, packet.reference(), &path, format)
                            .map_err(|e| e.to_string())?;
                    }
                    Contents::PackageMember { reference, member } => return member.save(&root, &reference, &path),
                    Contents::Artifact { reference, source, digest, metadata_view, entry } => {
                        let preview = data::preview_output(&root, &reference, source.as_ref()).map_err(|e| e.to_string())?;
                        if preview.digest != digest || (preview.metadata["view"] == "provider_receipt_metadata") != metadata_view {
                            return Err("The retained contents differ from the preview. Reopen the file before saving.".into());
                        }
                        if let Some(entry) = entry {
                            save_new_file_with(&path, |file| entry.copy_from(preview.directory, preview.path, preview.digest, file))?;
                        } else {
                            let bytes = std::fs::read(&preview.path).map_err(|e| e.to_string())?;
                            save_new_file(&path, &bytes)?;
                        }
                    }
                    Contents::Deliveries { work, editions, .. } => {
                        let listed = editions.into_iter().map(|e| (e.reference, e.digest)).collect();
                        let receipt = ai_personas::neural::delivery_export::collection(&root, &path, work.as_deref(), Some(&listed))
                            .map_err(|e| e.to_string())?;
                        return Ok(format!("Saved {} to {}", counted(receipt.delivery_count as u64, "issued edition", "issued editions"), path.display()));
                    }
                }
                Ok(format!("Saved {}", path.display()))
            })();
            let _ = tx.send(Update::Saved(serial, result));
            ctx.request_repaint();
        });
    }
}
