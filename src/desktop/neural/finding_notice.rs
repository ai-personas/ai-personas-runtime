//! Read one review where it was published or returned. Independent of the
//! comparison editor: inspecting a notice never re-addresses a review draft.
use super::*;
use ai_personas::neural::package_review::{EvidenceFile, FindingNotice};
use std::path::Path;

type Report = Result<FindingNotice, String>;
pub(super) struct Reader {
    event: i64,
    digest: String,
    work: String,
    pending: Option<mpsc::Receiver<Report>>,
    report: Option<Report>,
    basis: bool,
    follow: Option<Open>,
    back: Option<Place>,
}

enum Place {
    Edition(String),
    Preview(u64),
}
impl Place {
    fn showing(&self, app: &App) -> bool {
        match self {
            Self::Edition(r) => app.layout.editor == Editor::Delivery && &app.layout.delivery == r,
            Self::Preview(serial) => {
                app.layout.editor == Editor::Artifacts
                    && app.artifact_mode == ArtifactView::Retained
                    && app.preview_generation == *serial
            }
        }
    }
}

impl Reader {
    fn load(&mut self, ctx: &egui::Context, root: &Path) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let previous = self
            .report
            .take()
            .and_then(Result::ok)
            .filter(|_| self.follow.is_some());
        let (ctx, root, event, work, digest) = (
            ctx.clone(),
            root.to_owned(),
            self.event,
            self.work.clone(),
            self.digest.clone(),
        );
        std::thread::spawn(move || {
            let r = data::open(&root).and_then(|db| FindingNotice::at(&db, &work, event, &digest));
            let r = r.map_err(|e| e.to_string()).and_then(|r| {
                if previous.is_some_and(|p| !p.same_publication(&r)) {
                    return Err(
                        "The published finding changed. Read it again before opening a link."
                            .into(),
                    );
                }
                Ok(r)
            });
            let _ = tx.send(r);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) {
        if let Some(rx) = &self.pending {
            self.report = Some(match rx.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => Err("The finding reader stopped.".into()),
            });
            self.pending = None;
        }
    }
}

enum Open {
    Edition(String),
    Record(String),
    IssuedFile {
        file: String,
        edition: String,
        path: String,
    },
    File(EvidenceFile),
    Reviews(String),
}

fn link(ui: &mut Ui, reference: &str, label: &str, action: &mut Option<Open>) {
    if ui.link(label).on_hover_text(reference).clicked() {
        *action = Some(Open::Record(reference.into()));
    }
}

fn evidence_label(reference: &str) -> String {
    let Some((kind, address)) = reference.split_once(':') else {
        return reference.into();
    };
    let (id, edition) = address
        .rsplit_once('@')
        .map_or((address, String::new()), |(id, n)| (id, format!(" · v{n}")));
    format!("{} · {}{edition}", title(kind), excerpt(id, 22))
}

fn report(
    ui: &mut Ui,
    notice: &FindingNotice,
    basis: &mut bool,
    can_compare: bool,
) -> Option<Open> {
    let r = &notice.record.body;
    let subject = &r["subject"];
    let p = theme::palette(ui);
    let mut open = None;
    let summary = notice.subject();
    let delivery = subject["delivery"].as_str();
    let label = notice
        .author_label
        .as_deref()
        .unwrap_or_else(|| s(r, "author"));
    ui.horizontal_wrapped(|ui| {
        if r["author_kind"] == "persona" {
            avatar(ui, label, persona_color(s(r, "author")), 22.);
        }
        ui.strong(label);
        ui.weak(if r["author_kind"] == "operator" {
            "Operator finding"
        } else {
            "Persona finding"
        });
        let source = if notice.event == notice.publication_event {
            format!("Published · #{}", notice.event)
        } else {
            format!(
                "Published · #{}   /   Returned to {} · #{}",
                notice.publication_event,
                notice.issuer_label().unwrap_or("issuer"),
                notice.event
            )
        };
        ui.small(source);
    });
    ui.add_space(4.);
    ui.label(design::caption(
        if delivery.is_some() {
            "Reviewed edition"
        } else {
            "Proposed files"
        },
        p.muted,
    ));
    ui.add(
        egui::Label::new(design::display(
            subject["title"].as_str().unwrap_or("File set at review"),
            20.,
        ))
        .wrap(),
    );
    ui.horizontal_wrapped(|ui| {
        ui.small(counted(
            summary["file_count"].as_u64().unwrap_or_default(),
            "file",
            "files",
        ));
        if let Some(n) = summary["input_count"].as_u64().filter(|n| *n > 0) {
            ui.small(counted(n, "governing input", "governing inputs"));
        }
        if let (Some(edition), Some(seq)) = (delivery, summary["issue_event"].as_i64()) {
            ui.small(format!(
                "{} · Issue #{seq}",
                notice.issuer_label().unwrap_or("Issuer")
            ));
            if ui.link("Open package ↗").on_hover_text(edition).clicked() {
                open = Some(Open::Edition(edition.into()));
            }
        } else {
            ui.weak("Not a finding on an issued delivery");
        }
    });
    ui.add_space(6.);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(basis, false, "Finding");
        ui.selectable_value(basis, true, "Review basis");
    });
    ui.separator();
    if *basis {
        if let Some(brief) = subject["mandate"].as_str() {
            link(
                ui,
                brief,
                &format!(
                    "Reviewed brief · {}",
                    brief.rsplit('@').next().unwrap_or(brief)
                ),
                &mut open,
            );
        }
        if let Some(reporting) = r["reporting_mandate"]
            .as_str()
            .filter(|b| Some(*b) != subject["mandate"].as_str())
        {
            link(
                ui,
                reporting,
                &format!(
                    "Reporting brief · {}",
                    reporting.rsplit('@').next().unwrap_or(reporting)
                ),
                &mut open,
            );
        }
        ui.label(design::caption("File revisions at review", p.muted));
        for (path, file) in subject["members"].as_object().into_iter().flatten() {
            let reference = s(file, "reference");
            if ui
                .link(format!(
                    "{path} · v{}",
                    reference.rsplit('@').next().unwrap_or(reference)
                ))
                .on_hover_text(reference)
                .clicked()
            {
                open = Some(match delivery {
                    Some(edition) => Open::IssuedFile {
                        file: reference.into(),
                        edition: edition.into(),
                        path: path.clone(),
                    },
                    None => Open::Record(reference.into()),
                });
            }
        }
        for (label, list) in [
            ("Governing inputs", &subject["inputs"]),
            ("Cited evidence", &r["evidence"]),
        ] {
            if let Some(items) = list.as_array().filter(|v| !v.is_empty()) {
                ui.label(design::caption(label, p.muted));
                for item in items.iter().filter_map(Value::as_str) {
                    link(ui, item, &evidence_label(item), &mut open);
                }
            }
        }
        if let Some(files) = r["attachments"].as_array() {
            ui.strong("Reviewer’s files · not part of the package");
            for f in files {
                if let Some(file) = package_reviews::file_button(
                    ui,
                    s(r, "reference"),
                    s(f, "name"),
                    s(f, "reference"),
                    f["bytes"].as_u64().unwrap_or_default(),
                ) {
                    open = Some(Open::File(file));
                }
            }
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            theme::badge(
                ui,
                &title(s(r, "verdict")),
                if r["verdict"] == "supported" {
                    TEAL
                } else {
                    p.accent
                },
            );
            ui.weak("On this criterion · not acceptance");
        });
        ui.add(
            egui::Label::new(RichText::new(s(r, "criterion")).strong())
                .wrap()
                .selectable(true),
        );
        ui.add_space(5.);
        ui.add(egui::Label::new(s(r, "finding")).wrap().selectable(true));
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if let Some(call) = r["decision"].as_str() {
                link(
                    ui,
                    &format!("experience:{call}@1"),
                    "Reviewer's choice ↗",
                    &mut open,
                );
            }
            link(ui, s(r, "reference"), "Review record ↗", &mut open);
            if let Some(edition) = delivery {
                if ui
                    .add_enabled(can_compare, egui::Link::new("Compare reviews…"))
                    .on_disabled_hover_text(
                        "Finish or cancel the open review draft before changing its subject.",
                    )
                    .clicked()
                {
                    open = Some(Open::Reviews(edition.into()));
                }
            }
        });
    }
    open
}

impl App {
    pub(super) fn open_finding_notice(&mut self, ctx: &egui::Context, event: i64, digest: &str) {
        let mut reader = Reader {
            event,
            digest: digest.into(),
            work: self.work.clone(),
            pending: None,
            report: None,
            basis: false,
            follow: None,
            back: None,
        };
        reader.load(ctx, &self.root);
        self.finding_notice = Some(reader);
    }
    pub(super) fn finding_return_link(&mut self, ui: &mut Ui) {
        let Some(r) = self
            .finding_notice
            .as_ref()
            .filter(|r| r.back.as_ref().is_some_and(|b| b.showing(self)))
        else {
            return;
        };
        if ui.link(format!("← Finding at #{}", r.event)).clicked() {
            let (work, event, digest, basis) = (r.work.clone(), r.event, r.digest.clone(), r.basis);
            // Returning to a read is not a request to address its author.
            self.open_home_activity(&work);
            self.live_board.show_event(&work, event, self.cursor);
            self.open_finding_notice(ui.ctx(), event, &digest);
            if let Some(r) = self.finding_notice.as_mut() {
                r.basis = basis;
            }
        }
    }
    pub(super) fn finding_notice_window(&mut self, ctx: &egui::Context) {
        let Some(mut reader) = self.finding_notice.take() else {
            return;
        };
        if let Some(back) = &reader.back {
            if back.showing(self) {
                self.finding_notice = Some(reader);
            }
            return;
        }
        reader.poll();
        // The report can remain on screen while the workspace changes. Recheck
        // the exact notice before following any of its addresses.
        if reader.report.is_some() {
            if let Some(open) = reader
                .follow
                .take()
                .filter(|_| matches!(&reader.report, Some(Ok(_))))
            {
                match open {
                    Open::Edition(r) => {
                        self.open_issued_package(ctx, &r);
                        reader.back = Some(Place::Edition(r));
                    }
                    Open::IssuedFile {
                        file,
                        edition,
                        path,
                    } => {
                        self.open_named_resource(ctx, file, Some((edition, path)));
                        reader.back = Some(Place::Preview(self.preview_generation));
                    }
                    Open::Record(r) => {
                        let prior = self.preview_generation;
                        self.open_choice_or_record(ctx, r);
                        if self.preview_generation != prior {
                            reader.back = Some(Place::Preview(self.preview_generation));
                        }
                    }
                    Open::File(file) => {
                        self.save_review_file(file);
                        self.finding_notice = Some(reader);
                        return;
                    }
                    Open::Reviews(r) => {
                        self.package_reviews
                            .open(ctx, &self.root, &reader.work, Some(&r))
                    }
                }
                if reader.back.is_some() {
                    self.finding_notice = Some(reader);
                }
                return;
            }
        }
        let viewport = ctx.content_rect().shrink(6.);
        let chrome = theme::titled_window_chrome(ctx);
        let bounds = (viewport.size() - chrome).max(egui::vec2(1., 1.));
        let size = egui::vec2(640., 530.).min(bounds);
        let mut visible = true;
        let mut again = false;
        let mut open = None;
        egui::Window::new("Published finding")
            .id(egui::Id::new(("finding-notice", &self.root, reader.event)))
            .default_size(size)
            .default_pos(viewport.center() - (size + chrome) / 2.)
            .min_size(egui::vec2(200., 100.).min(bounds))
            .max_size(bounds)
            .constrain_to(viewport)
            .open(&mut visible)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| match &reader.report {
                Some(Ok(f)) => {
                    open = report(ui, f, &mut reader.basis, self.package_reviews.can_browse())
                }
                Some(Err(e)) => {
                    ui.strong(format!("Finding unavailable · #{}", reader.event));
                    ui.label("Studio could not verify this notice and its reviewed subject.");
                    again = ui.button("Check again").clicked();
                    ui.collapsing("Verification details", |ui| {
                        ui.add(egui::Label::new(e).wrap().selectable(true));
                    });
                }
                None => {
                    ui.spinner();
                    ui.weak("Checking the notice and reviewed subject…");
                }
            });
        if !visible {
            return;
        }
        if open.is_some() || again {
            reader.follow = open;
            reader.load(ctx, &self.root);
        }
        self.finding_notice = Some(reader);
    }
}
