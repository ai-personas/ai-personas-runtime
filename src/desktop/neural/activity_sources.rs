//! Inspect exact files from another activity while browsing work or a persona.
use super::*;
use ai_personas::neural::activity_sources::{inspect, Query};
use std::path::Path;

const PAGE: usize = 10;

#[derive(Default)]
pub(super) struct Window {
    work: String,
    persona: Option<String>,
    source: Option<String>,
    offset: usize,
    changed: (i64, usize),
    workspace_changed: (i64, usize),
    reader: Option<mpsc::Receiver<Result<Value, String>>>,
    report: Option<Result<Value, String>>,
}

impl App {
    pub(super) fn open_sources(&mut self, ctx: &egui::Context, work: &str, persona: Option<&str>) {
        let mut window = Window {
            work: work.to_owned(),
            persona: persona.map(str::to_owned),
            changed: self.source_stamp(work),
            workspace_changed: (self.cursor, self.character_decisions.len()),
            ..Default::default()
        };
        window.read(ctx, &self.root);
        self.activity_sources = window;
    }

    fn source_stamp(&self, work: &str) -> (i64, usize) {
        let last = self
            .events
            .iter()
            .rev()
            .find(|e| e["work"] == work)
            .and_then(|e| e["event"].as_i64())
            .unwrap_or(0);
        let choices = self
            .character_decisions
            .iter()
            .filter(|d| d["work"] == work)
            .count();
        (last, choices)
    }

    pub(super) fn sources_window(&mut self, ctx: &egui::Context) {
        if self.activity_sources.work.is_empty() {
            return;
        }
        let mut window = std::mem::take(&mut self.activity_sources);
        let preview = self.preview_generation;
        let mut visible = true;
        egui::Window::new("Earlier work")
            .id(egui::Id::new("activity-sources"))
            .open(&mut visible)
            .default_width(740.)
            .default_height(510.)
            .resizable(true)
            .show(ctx, |ui| window.draw(ui, self));
        if visible && self.preview_generation == preview {
            self.activity_sources = window;
        }
    }
}

impl Window {
    fn read(&mut self, ctx: &egui::Context, root: &Path) {
        let (tx, rx) = mpsc::channel();
        self.reader = Some(rx);
        self.report = None;
        let (root, work, actor, ctx) = (
            root.to_owned(),
            self.work.clone(),
            self.persona.clone(),
            ctx.clone(),
        );
        let query = Query {
            source_work: self.source.clone(),
            offset: self.offset,
            page_size: PAGE,
        };
        std::thread::spawn(move || {
            let result =
                data::open(&root).and_then(|db| inspect(&db, &work, actor.as_deref(), &query));
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn draw(&mut self, ui: &mut Ui, app: &mut App) {
        if let Some(reader) = &self.reader {
            match reader.try_recv() {
                Ok(result) => {
                    self.report = Some(result);
                    self.reader = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.report = Some(Err("The file history reader stopped.".to_owned()));
                    self.reader = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let title = app.work_name(&self.work);
        ui.label(RichText::new(excerpt(&title, 115)).size(20.).strong())
            .on_hover_text(title);
        ui.add_space(5.);
        let mut reload = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("Show activity by");
            let before = self.persona.clone();
            egui::ComboBox::from_id_salt("earlier-work-persona")
                .selected_text(
                    self.persona
                        .as_deref()
                        .map_or("All personas", |id| app.label(id)),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.persona, None, "All personas");
                    for member in app
                        .records("participation")
                        .iter()
                        .filter(|r| r["body"]["work"] == self.work)
                    {
                        let id = s(&member["body"], "persona");
                        ui.selectable_value(&mut self.persona, Some(id.to_owned()), app.label(id));
                    }
                });
            if self.persona != before {
                self.offset = 0;
                reload = true;
            }
            if let Some(source) = &self.source {
                let name = app.work_name(source);
                if ui
                    .button(format!("{}  ×", excerpt(&name, 45)))
                    .on_hover_text("Show all origin activities")
                    .clicked()
                {
                    self.source = None;
                    self.offset = 0;
                    reload = true;
                }
            }
        });
        ui.add_space(6.);
        let mut origin = None;
        let mut narrow = None;
        if let Some(Ok(report)) = &self.report {
            let coverage = &report["coverage"];
            let count = |key: &str| coverage[key].as_u64().unwrap_or(0);
            let tiles = [
                ("Source activity", "Source activities", count("source_activities")),
                ("File revision", "File revisions", count("distinct_files")),
                ("Verified decision", "Verified decisions", count("verified_decisions")),
            ];
            ui.columns(tiles.len(), |columns| {
                for (col, (one, many, n)) in columns.iter_mut().zip(tiles) {
                    egui::Frame::new()
                        .fill(theme::palette(col).header)
                        .corner_radius(6)
                        .inner_margin(8)
                        .show(col, |ui| {
                            ui.set_min_width((ui.available_width() - 2.).max(0.));
                            ui.label(RichText::new(number(n)).strong().size(19.));
                            ui.small(if n == 1 { one } else { many });
                        });
                }
            });
            let unverified = count("unverified_decisions");
            let legacy = count("legacy_decisions");
            let incomplete = count("unverified_receipts");
            let unknown = count("unattributed_files");
            if incomplete > 0 || unknown > 0 {
                ui.weak(format!("Evidence gaps · {incomplete} unverified tool or package receipts · {unknown} file revisions with unknown origin"));
            }
            if unverified > 0 {
                ui.label(format!(
                    "Input evidence is unavailable for {unverified} of {} ordinary decisions.",
                    count("ordinary_decisions")
                ));
            }
            if legacy > 0 {
                ui.weak(format!("{legacy} earlier decisions have a matching input receipt and limited output history."));
            }
            let rows = report["items"].as_array().map_or(&[][..], Vec::as_slice);
            ui.separator();
            if rows.is_empty() {
                if self.offset == 0 {
                    ui.strong("No earlier file use found");
                    ui.label("This selection has no recorded catalogue entries, file content, tool inputs or package members from another activity.");
                } else {
                    ui.label("No more file history on this page.");
                }
            }
            egui::ScrollArea::vertical()
                .id_salt(("source-rows", &self.work, self.offset))
                .max_height((ui.available_height() - 40.).max(120.))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            file_row(ui, app, row, &mut origin, &mut narrow);
                            ui.separator();
                        });
                    }
                });
            ui.horizontal_wrapped(|ui| {
                if self.offset > 0 && ui.button("← Previous").clicked() {
                    self.offset = self.offset.saturating_sub(PAGE);
                    reload = true;
                }
                if report["total"] == 1 {
                    ui.weak("1 record");
                } else if !rows.is_empty() {
                    ui.weak(format!(
                        "{}–{} of {} records",
                        report["offset"].as_u64().unwrap_or(0) + 1,
                        report["offset"].as_u64().unwrap_or(0) + rows.len() as u64,
                        report["total"]
                    ));
                }
                if let Some(next) = report["next_offset"].as_u64() {
                    if ui.button("Next →").clicked() {
                        self.offset = next as usize;
                        reload = true;
                    }
                }
            });
        } else if let Some(Err(message)) = &self.report {
            ui.label(message);
            reload |= ui.button("Retry").clicked();
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking the recorded file history…");
            });
        }
        if let Some(id) = narrow {
            self.source = Some(id);
            self.offset = 0;
            reload = true;
        }
        if let Some(id) = origin {
            app.work = id;
            app.conversation_page = 0;
            app.layout.editor = Editor::Studio;
            self.work.clear();
            return;
        }
        let checkpoint = (app.cursor, app.character_decisions.len());
        let stamp = if checkpoint != self.workspace_changed {
            app.source_stamp(&self.work)
        } else {
            self.changed
        };
        if reload || (self.reader.is_none() && stamp != self.changed) {
            self.changed = stamp;
            self.workspace_changed = checkpoint;
            self.read(ui.ctx(), &app.root);
        } else if stamp == self.changed {
            self.workspace_changed = checkpoint;
        }
    }
}

fn file_row(
    ui: &mut Ui,
    app: &mut App,
    row: &Value,
    origin: &mut Option<String>,
    narrow: &mut Option<String>,
) {
    let reference = s(row, "reference");
    let source = s(row, "source_work");
    let path = s(row, "path");
    let (label, color) = match s(row, "kind") {
        "catalog" => ("Listed in search", PURPLE),
        "text" => ("Text received", TEAL),
        "image" => ("Image received", TEAL),
        "execution" if row["status"] == "failed" => ("Tool failed", theme::ACCENT),
        "execution" if row["role"] == "program" => ("Program executed", theme::GREEN),
        "execution" => ("Used as tool input", theme::GREEN),
        "delivery" => ("In delivery", theme::GREEN),
        "human_request" => ("Attached to human request", theme::ACCENT),
        _ => ("Recorded", theme::palette(ui).muted),
    };
    ui.add_space(3.);
    ui.horizontal_wrapped(|ui| {
        theme::badge(ui, label, color);
        let actor = app.label(s(row, "actor"));
        ui.strong(actor);
        if let Some(pair) = row["range"].as_array().filter(|pair| pair.len() == 2) {
            if let (Some(start), Some(end)) = (pair[0].as_u64(), pair[1].as_u64()) {
                ui.small(format!(
                    "{} bytes · {}..{}",
                    number(end.saturating_sub(start)),
                    number(start),
                    number(end)
                ));
            }
        }
        if let Some(stamp) = row["created_unix"].as_i64() {
            ui.weak(time_label(stamp));
        }
    });
    ui.horizontal_wrapped(|ui| {
        theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
        if ui
            .link(if path.is_empty() { reference } else { path })
            .on_hover_text(reference)
            .clicked()
        {
            app.open_resource(ui.ctx(), reference.into());
        }
        if let Some((_, rev)) = reference.rsplit_once('@') {
            ui.weak(format!("rev. {rev}"));
        }
    });
    let source_name = app.work_name(source);
    let author = app.label(s(row, "author")).to_owned();
    ui.horizontal_wrapped(|ui| {
        ui.small(format!("Source activity · {}", excerpt(&source_name, 72)))
            .on_hover_text(&source_name);
        if !author.is_empty() {
            ui.weak(format!("· Saved by {author}"));
        }
    });
    ui.collapsing("Details", |ui| {
        let detail = &row["detail"];
        if row["tool"] == "document.render_pdf" {
            if let (Some(page), Some(count), Some(image)) =
                (detail["page"].as_u64(), detail["page_count"].as_u64(), detail["image"].as_str())
            {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("PDF page {page} of {count}"));
                    if ui.link("Open rendered page").on_hover_text(image).clicked() {
                        app.open_resource(ui.ctx(), image.into());
                    }
                });
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("{} · {}", s(row, "tool"), title(s(row, "role"))));
            if ui.small_button("Open origin activity").clicked() {
                *origin = Some(source.into());
            }
            if ui.small_button("Filter to this origin").clicked() {
                *narrow = Some(source.into());
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (field, label) in [
                ("decision", "Consuming decision"),
                ("source_decision", "File author’s decision"),
                ("evidence", "Exact record"),
            ] {
                if let Some(handle) = row[field].as_str() {
                    if ui.small_button(label).on_hover_text(handle).clicked() {
                        app.open_resource(ui.ctx(), handle.into());
                    }
                }
            }
        });
    });
}
