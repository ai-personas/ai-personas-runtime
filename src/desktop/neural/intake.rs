//! An operator-facing reading of the opening brief and the local activity journal.
use super::*;
use ai_personas::neural::intake::{inspect, Page};
use std::path::Path;

const ROWS: usize = 8;
type Report = Result<Value, String>;
pub(super) struct Window {
    work: String,
    reader: Option<mpsc::Receiver<Report>>,
    report: Option<Report>,
}

impl Window {
    pub(super) fn open(ctx: &egui::Context, root: &Path, work: &str) -> Self {
        let mut window = Self { work: work.into(), reader: None, report: None };
        window.read(ctx, root, 0, None);
        window
    }
    fn read(&mut self, ctx: &egui::Context, root: &Path, offset: usize, through_event: Option<i64>) {
        let (tx, rx) = mpsc::channel();
        self.reader = Some(rx);
        self.report = None;
        let (ctx, root, work) = (ctx.clone(), root.to_owned(), self.work.clone());
        std::thread::spawn(move || {
            let page = Page { offset, through_event, page_size: ROWS };
            let result = data::open(&root).and_then(|db| inspect(&db, &work, &page));
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }
    fn draw(&mut self, ui: &mut Ui, app: &mut App) -> bool {
        if let Some(rx) = &self.reader {
            match rx.try_recv() {
                Ok(r) => { self.report = Some(r); self.reader = None; }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.report = Some(Err("Intake reader stopped".into())); self.reader = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some(report) = &self.report else { ui.spinner(); return false; };
        let r = match report {
            Ok(r) => r,
            Err(e) => { ui.label(e); return false; }
        };
        let opening = &r["opening"];
        let files = opening["files"].as_array().map_or(0, Vec::len);
        let setup = &r["project_at_workspace_setup"]["body"];
        let through = r["through_event"].as_i64().unwrap_or_default();
        let offset = r["offset"].as_u64().unwrap_or_default() as usize;
        let count = r["event_count"].as_u64().unwrap_or_default() as usize;
        let mut next = None;
        let mut selected = None;
        let mut open_edition = false;
        egui::ScrollArea::vertical().id_salt(("activity-intake", &self.work)).show(ui, |ui| {
            eyebrow(ui, "ACTIVITY ORIGIN  /  OPENING EDITION");
            ui.label(RichText::new(s(opening, "need")).size(17.).strong());
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} · {}", s(opening,"requester"), s(opening,"source")));
                ui.weak(s(opening,"created"));
            });
            ui.label(format!("Opening attachments: {files}"));
            app.source_file_links(ui, &opening["files"], s(opening,"brief"));
            if let Some(reference) = opening["from_edition"]["reference"].as_str() {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Starting edition");
                    open_edition = ui.link(s(&opening["from_edition"], "title")).on_hover_text(reference).clicked();
                });
            }
            ui.add_space(6.);
            ui.strong("Working folder at workspace creation");
            let observation = match s(setup, "entry_state") {
                "empty" => "No entries observed",
                "entries_present" => "At least one entry present",
                "unobserved" => "Folder could not be inspected",
                _ => "No setup observation recorded",
            };
            ui.label(observation).on_hover_text(s(setup,"directory"));
            ui.small("This was not an inventory at invitation time. Later changes and other host files are not measured here.");
            ui.add_space(8.);
            ui.separator();
            ui.strong("Events without a linked persona decision");
            ui.small("Includes operator invitations, messages, amendments and host actions for this activity. It does not show what a program read or whether a persona saw a message.");
            let kinds = &r["event_kinds"];
            ui.horizontal_wrapped(|ui| {
                for (key, name) in [("invitation", "Invitations"), ("user_message", "User messages"), ("requirements_amended", "Amendments")] {
                    ui.label(format!("{name}: {}", kinds[key].as_u64().unwrap_or(0)));
                }
            });
            for event in r["events"].as_array().into_iter().flatten() {
                let seq = event["event"].as_i64().unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    if ui.link(format!("#{seq}")).on_hover_text(s(event,"record")).clicked() { selected = Some(seq); }
                    ui.strong(s(event,"kind").replace('_', " "));
                    ui.label(format!("{} → {}", s(event,"actor"), s(event,"target")));
                    ui.small(s(event,"at"));
                });
            }
            ui.horizontal_wrapped(|ui| {
                if count > 0 { ui.small(format!("{}–{} of {count}", offset + 1, (offset + ROWS).min(count))); }
                if ui.add_enabled(offset > 0, egui::Button::new("Previous")).clicked() { next = Some((offset.saturating_sub(ROWS), Some(through))); }
                if ui.add_enabled(offset + ROWS < count, egui::Button::new("Next")).clicked() { next = Some((offset + ROWS, Some(through))); }
                if ui.button("Refresh").clicked() { next = Some((0, None)); }
                ui.small(format!("Through workspace event #{through}"));
            });
        });
        if let Some(seq) = selected { app.open_home_event(&self.work, seq); return true; }
        if open_edition {
            let edition = &opening["from_edition"];
            app.open_home_delivery(s(edition,"reference"), s(edition,"work"));
            return true;
        }
        if let Some((offset, boundary)) = next { self.read(ui.ctx(), &app.root, offset, boundary); }
        false
    }
}

impl App {
    pub(super) fn intake_window(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.intake.take() else { return; };
        let (mut open, mut jumped) = (true, false);
        let preview = self.preview_generation;
        egui::Window::new("Intake record").id(egui::Id::new("intake-record"))
            .default_width(585.).resizable(true).open(&mut open)
            .show(ctx, |ui| { jumped = window.draw(ui, self); });
        if open && !jumped && self.preview_generation == preview { self.intake = Some(window); }
    }
}
