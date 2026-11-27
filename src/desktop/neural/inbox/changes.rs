//! Read an outside step's attachments at publication, without entering the response form.
use super::*;
use ai_personas::neural::human_requests::PacketChanges;
use std::path::Path;

type Report = Result<PacketChanges, String>;

pub(super) struct Reader {
    event: i64,
    edition: String,
    work: String,
    pending: Option<mpsc::Receiver<Report>>,
    report: Option<Report>,
    preview: Option<u64>,
    instructions: bool,
}

impl Reader {
    fn load(&mut self, root: &Path, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.report = None;
        self.preview = None;
        let (root, ctx, event, edition, work) = (
            root.to_owned(),
            ctx.clone(),
            self.event,
            self.edition.clone(),
            self.work.clone(),
        );
        std::thread::spawn(move || {
            let result = data::open(&root).and_then(|db| PacketChanges::at(&db, event)).map_err(|e| e.to_string()).and_then(|r| {
                if r.edition == edition && r.work == work { Ok(r) }
                else { Err("This publication no longer matches the selected request edition and activity.".into()) }
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn update(&mut self) {
        if let Some(rx) = &self.pending {
            self.report = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The packet reader stopped.".into()))
                }
            };
            self.pending = None;
        }
    }
}

enum Open {
    File(String, String, String),
    Delivery(String),
    SaveRecord,
}

fn file_link(
    ui: &mut Ui,
    label: &str,
    path: &str,
    edition: &str,
    file: &str,
    open: &mut Option<Open>,
) {
    if ui
        .link(label)
        .on_hover_text(format!("{path}\n{edition}\n{file}"))
        .clicked()
    {
        *open = Some(Open::File(file.into(), edition.into(), path.into()));
    }
}

fn version(reference: &str) -> &str {
    reference.rsplit_once('@').map_or(reference, |(_, v)| v)
}

fn group(ui: &mut Ui, label: &str, count: usize, expanded: bool, draw: impl FnOnce(&mut Ui)) {
    if count > 0 {
        egui::CollapsingHeader::new(format!("{label} · {count}"))
            .default_open(expanded)
            .show(ui, draw);
    }
}

fn files(ui: &mut Ui, r: &PacketChanges, open: &mut Option<Open>) {
    group(ui, "Added", r.added.len(), true, |ui| {
        for (path, file) in &r.added {
            file_link(ui, path, path, &r.edition, file, open);
        }
    });
    group(
        ui,
        "Replaced at the same path",
        r.replaced.len(),
        true,
        |ui| {
            for (path, pair) in &r.replaced {
                ui.push_id(path, |ui| {
                    ui.add(egui::Label::new(path).wrap());
                    ui.horizontal_wrapped(|ui| {
                        file_link(
                            ui,
                            &format!("Before · v{}", version(&pair.before)),
                            path,
                            &r.base,
                            &pair.before,
                            open,
                        );
                        file_link(
                            ui,
                            &format!("At issue · v{}", version(&pair.issued)),
                            path,
                            &r.edition,
                            &pair.issued,
                            open,
                        );
                    });
                });
            }
        },
    );
    for (label, rows, edition) in [
        ("No longer attached", &r.removed, &r.base),
        ("Same file revision", &r.kept, &r.edition),
    ] {
        group(ui, label, rows.len(), false, |ui| {
            for (path, file) in rows {
                file_link(ui, path, path, edition, file, open);
            }
        });
    }
}

fn contents(ui: &mut Ui, r: &PacketChanges, issuer: &str, instructions: &mut bool) -> Option<Open> {
    let p = theme::palette(ui);
    let mut open = None;
    let shared_paths = r.replaced.len() + r.kept.len();
    let before = r.removed.len() + shared_paths;
    let at_issue = r.added.len() + shared_paths;
    ui.horizontal_wrapped(|ui| {
        theme::glyph(ui, Icon::Files, p.accent);
        ui.label(design::caption(
            &format!("Packet issue · #{}", r.event),
            p.accent,
        ));
        ui.small(format!("Issued by {issuer}"));
    });
    ui.add(egui::Label::new(design::display(&r.delivery_title, 22.)).wrap());
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!(
            "Request editions {} → {}",
            version(&r.base),
            version(&r.edition)
        ))
        .on_hover_text(format!("{}\n{}", r.base, r.edition));
        ui.separator();
        ui.small(format!("Attachments: {before} → {at_issue}"));
        if ui
            .link("Inspect issued delivery →")
            .on_hover_text(&r.delivery)
            .clicked()
        {
            open = Some(Open::Delivery(r.delivery.clone()));
        }
    });
    ui.add_space(6.);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(instructions, false, "File changes");
        ui.selectable_value(instructions, true, "Instructions at issue");
    });
    ui.separator();
    if *instructions {
        ui.label(design::caption("With this packet", p.muted));
        ui.add(egui::Label::new(&r.instructions).wrap().selectable(true));
        ui.add_space(8.);
        ui.label(design::caption("Original outside step", p.muted));
        ui.strong(&r.request_title);
        ui.add(egui::Label::new(&r.action).wrap().selectable(true));
        if let Some(text) = &r.return_requested {
            ui.label(design::caption("Requested return", p.muted));
            ui.add(egui::Label::new(text).wrap().selectable(true));
        }
    } else {
        ui.small("Files are compared by path and revision, not by ZIP contents. Removing an attachment does not change the earlier delivery.");
        files(ui, r, &mut open);
    }
    ui.add_space(5.);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Save issue record…").on_hover_text("Save this edition’s request sheet and original files. Do not substitute the current packet.").clicked() {
            open = Some(Open::SaveRecord);
        }
        ui.weak("Historical issue. Use the current request for outside work.");
    });
    open
}

impl App {
    pub(in crate::desktop::neural) fn open_packet_changes(
        &mut self,
        ctx: &egui::Context,
        event: i64,
        edition: &str,
        work: &str,
    ) {
        let mut r = Reader {
            event,
            edition: edition.into(),
            work: work.into(),
            report: None,
            pending: None,
            preview: None,
            instructions: false,
        };
        r.load(&self.root, ctx);
        self.inbox.changes = Some(r);
    }

    pub(in crate::desktop::neural) fn packet_return_link(&mut self, ui: &mut Ui) {
        if let Some(r) = self
            .inbox
            .changes
            .as_mut()
            .filter(|r| r.preview == Some(self.preview_generation))
        {
            if ui
                .link(format!("← Packet changes at #{}", r.event))
                .clicked()
            {
                r.load(&self.root, ui.ctx());
            }
        }
    }

    pub(in crate::desktop::neural) fn packet_changes_window(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.inbox.changes.take() else {
            return;
        };
        if let Some(generation) = r.preview {
            if self.layout.editor == Editor::Artifacts
                && self.artifact_mode == ArtifactView::Retained
                && generation == self.preview_generation
            {
                self.inbox.changes = Some(r);
            }
            return;
        }
        r.update();
        let mut visible = true;
        let mut retry = false;
        let mut open = None;
        let viewport = ctx.content_rect().shrink(6.);
        let chrome = theme::titled_window_chrome(ctx);
        let available = (viewport.size() - chrome).max(egui::vec2(1., 1.));
        let size = egui::vec2(570., 495.).min(available);
        egui::Window::new("Field packet changes")
            .id(egui::Id::new(("field-packet-changes", r.event)))
            .open(&mut visible)
            .default_size(size)
            .default_pos(viewport.center() - (size + chrome) / 2.)
            .min_size(egui::vec2(200., 100.).min(available))
            .max_size(available)
            .constrain_to(viewport)
            .vscroll(true)
            .show(ctx, |ui| match &r.report {
                Some(Ok(report)) => {
                    open = contents(
                        ui,
                        report,
                        self.label(&report.requester),
                        &mut r.instructions,
                    );
                }
                Some(Err(e)) => {
                    ui.strong("Packet comparison unavailable");
                    ui.add(egui::Label::new(e).wrap());
                    retry = ui.button("Check again").clicked();
                }
                None => {
                    ui.spinner();
                    ui.label("Checking the two request editions…");
                }
            });
        if !visible {
            return;
        }
        match open {
            Some(Open::File(reference, edition, path)) => {
                self.open_named_resource(ctx, reference, Some((edition, path)));
                r.preview = Some(self.preview_generation);
            }
            Some(Open::Delivery(reference)) => {
                self.open_issued_package(ctx, &reference);
                return;
            }
            Some(Open::SaveRecord) => {
                if let Some(Ok(report)) = &r.report {
                    let files = report.added.len() + report.replaced.len() + report.kept.len();
                    self.save_packet_record(&report.edition, &report.edition_digest, &report.request_title, files);
                }
            }
            None if retry => r.load(&self.root, ctx),
            None => (),
        }
        self.inbox.changes = Some(r);
    }
}
