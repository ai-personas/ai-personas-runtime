//! What a recipient can learn *now*, distinct from the author's frozen release.
use super::*;
use ai_personas::neural::package_review::{
    after_issue::{self, Entry, Page},
    Verdict,
};
use std::sync::OnceLock;

const PER_PAGE: usize = 2;
type Read = Arc<OnceLock<Result<Page, String>>>;

#[derive(Default)]
pub(super) struct Updates {
    page: usize,
    read: Option<(u64, usize, Read)>,
    expanded: Option<String>,
}

impl Updates {
    /// Put a route beside the original claim, including on a small screen.
    /// Only name a concern we have actually verified on the displayed page.
    pub(super) fn link_text(&self, generation: u64) -> Option<&'static str> {
        let report = self
            .read
            .as_ref()
            .filter(|(at, p, _)| *at == generation && *p == self.page)
            .and_then(|(_, _, r)| r.get())
            .and_then(|r| r.as_ref().ok());
        match report {
            Some(p) if p.total_candidates == 0 => None,
            Some(p)
                if p.items.iter().any(|i| {
                    matches!(
                        i,
                        Entry::Finding {
                            verdict: Verdict::Concern,
                            ..
                        }
                    )
                }) =>
            {
                Some("Later concern ↓")
            }
            _ => Some("After issue ↓"),
        }
    }

    // The desk resets this state on navigation. Keep at most one worker for
    // this edition, and never paint a page from a previous workspace snapshot.
    fn load(
        &mut self,
        ctx: &egui::Context,
        root: &std::path::Path,
        edition: &str,
        generation: u64,
    ) -> Option<Read> {
        if let Some((at, page, read)) = &self.read {
            if *at == generation && *page == self.page {
                return Some(read.clone());
            }
            if read.get().is_none() {
                return None;
            }
        }
        let edition = edition.to_owned();
        let offset = self.page * PER_PAGE;
        let root = root.to_owned();
        let ctx = ctx.clone();
        let read = Arc::new(OnceLock::new());
        let output = read.clone();
        std::thread::spawn(move || {
            let page =
                data::open(&root).and_then(|db| after_issue::page(&db, &edition, offset, PER_PAGE));
            let _ = output.set(page.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.read = Some((generation, self.page, read.clone()));
        Some(read)
    }
}

impl App {
    pub(super) fn edition_updates(&mut self, ui: &mut Ui, p: &Package) {
        let read = self.delivery_desk.updates.load(
            ui.ctx(),
            &self.root,
            &p.reference,
            self.home_index.generation,
        );
        let mut edition = None;
        let mut evidence = None;
        let mut retry = false;
        ui.add_space(8.);
        let Some(result) = read.as_ref().and_then(|r| r.get()) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Looking for records after issue…");
            });
            return;
        };
        if result.as_ref().is_ok_and(|r| r.total_candidates == 0) {
            ui.weak("After issue · No findings on this edition or later citations found.");
            return;
        }
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Conversation, theme::palette(ui).muted);
                heading(ui, "After this edition was issued");
            });
            let report = match result {
                Ok(report) => report,
                Err(e) => {
                    ui.label(e);
                    retry = ui.small_button("Retry lookup").clicked();
                    return;
                }
            };
            ui.weak("These records do not replace this edition or record acceptance.");
            for item in &report.items {
                ui.separator();
                match item {
                    Entry::Finding {
                        reference,
                        author,
                        author_kind,
                        criterion,
                        verdict,
                        finding,
                        evidence: sources,
                        ..
                    } => {
                        let expanded =
                            self.delivery_desk.updates.expanded.as_ref() == Some(reference);
                        let toggle = ui
                            .horizontal_wrapped(|ui| {
                                let (label, tint) = match verdict {
                                    Verdict::Concern => ("Concern", theme::ACCENT),
                                    Verdict::Supported => ("Supported", TEAL),
                                    Verdict::Inconclusive => {
                                        ("Inconclusive", theme::palette(ui).muted)
                                    }
                                };
                                theme::badge(ui, label, tint);
                                if author_kind == "operator" {
                                    ui.strong(format!("{author} · operator finding"));
                                } else {
                                    ui.strong(self.label(author)).on_hover_text(author);
                                    ui.weak("· finding on this criterion");
                                }
                                ui.small_button(if expanded {
                                    "Hide finding"
                                } else {
                                    "Read finding"
                                })
                                .on_hover_text(reference)
                                .clicked()
                            })
                            .inner;
                        if toggle {
                            self.delivery_desk.updates.expanded =
                                (!expanded).then(|| reference.clone());
                        }
                        ui.add(egui::Label::new(criterion).wrap());
                        if self.delivery_desk.updates.expanded.as_ref() == Some(reference) {
                            ui.add_space(4.);
                            ui.add(egui::Label::new(finding).wrap().selectable(true));
                            if !sources.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.small("Cited evidence");
                                    for (i, r) in sources.iter().enumerate() {
                                        if ui
                                            .link(format!(
                                                "{} {} ↗",
                                                match r.split_once(':').map(|(k, _)| k) {
                                                    Some("command_run") => "Command receipt",
                                                    Some("asset") => "File",
                                                    Some("object") => "Object",
                                                    Some("delivery") => "Edition",
                                                    _ => "Record",
                                                },
                                                i + 1
                                            ))
                                            .on_hover_text(r)
                                            .clicked()
                                        {
                                            evidence = Some(r.clone());
                                        }
                                    }
                                });
                            }
                            exact_ref(ui, reference);
                        }
                    }
                    Entry::Citation {
                        reference,
                        issuer,
                        title,
                        in_evidence,
                        in_review_inputs,
                        ..
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(self.label(issuer)).on_hover_text(issuer);
                            ui.label(match (in_evidence, in_review_inputs) {
                                (true, true) => "cited this edition as evidence and a review input",
                                (true, _) => "cited this edition as evidence",
                                _ => "named this edition as a review input",
                            });
                        });
                        if ui
                            .link(title)
                            .on_hover_text(format!("Open this later edition\n{reference}"))
                            .clicked()
                        {
                            edition = Some(reference.clone());
                        }
                    }
                    Entry::Unavailable { reason } => {
                        ui.strong("A later record could not be verified");
                        ui.label(reason);
                        retry |= ui.small_button("Retry lookup").clicked();
                    }
                }
            }
            let page = &mut self.delivery_desk.updates.page;
            *page = (*page).min(report.total_candidates.saturating_sub(1) / PER_PAGE);
            home::pager(ui, page, report.total_candidates, PER_PAGE, "later records");
        });
        if retry {
            self.delivery_desk.updates.read = None;
            ui.ctx().request_repaint();
        }
        if let Some(r) = edition {
            self.open_issued_package(ui.ctx(), &r);
        }
        if let Some(r) = evidence {
            self.open_choice_or_record(ui.ctx(), r);
        }
    }
}
