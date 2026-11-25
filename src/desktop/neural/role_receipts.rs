//! On-demand, pinned reading of a named human's role as supplied to personas.
use super::*;
use std::path::Path;

const PAGE: usize = 6;
pub(super) struct Window {
    role: String,
    cursors: Vec<i64>,
    pending: Option<mpsc::Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
}

impl Window {
    fn read(&mut self, ctx: &egui::Context, root: &Path) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.result = None;
        let (ctx, root, role, before) = (
            ctx.clone(),
            root.to_owned(),
            self.role.clone(),
            self.cursors.last().copied(),
        );
        std::thread::spawn(move || {
            let r = data::open(&root)
                .and_then(|db| ai_personas::neural::task_roles::received(&db, &role, before, PAGE));
            let _ = tx.send(r.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }
    fn draw(&mut self, ui: &mut Ui, app: &mut App) -> bool {
        if let Some(rx) = &self.pending {
            self.result = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The role receipt reader stopped.".into()))
                }
            };
            if self.result.is_some() {
                self.pending = None;
            }
        }
        let Some(result) = &self.result else {
            ui.spinner();
            return false;
        };
        let r = match result {
            Ok(r) => r,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                if ui.button("Try again").clicked() {
                    self.read(ui.ctx(), &app.root);
                }
                return false;
            }
        };
        let role = &r["role"];
        let work = s(r, "work");
        let mut selected = None;
        eyebrow(ui, "Responsibility board  /  Role receipts");
        ui.horizontal_wrapped(|ui| {
            ui.strong(s(role, "operator"));
            theme::badge(
                ui,
                if role["role"] == "owner" {
                    "Task owner"
                } else {
                    "Contributor"
                },
                TEAL,
            );
            if ui
                .small_button("Role declaration ↗")
                .on_hover_text(s(r, "declaration"))
                .clicked()
            {
                selected = Some(s(r, "declaration").into());
            }
        });
        ui.label(s(role, "note"));
        let name = app.work_name(work);
        ui.small(excerpt(&name, 96)).on_hover_text(name);
        ui.separator();
        let items = r["items"].as_array().map(Vec::as_slice).unwrap_or_default();
        let verified = items.iter().filter(|v| v["kind"] == "supplied").count();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Accepted decisions");
            theme::badge(ui, &format!("{verified} verified on this page"), TEAL);
        });
        ui.weak("A receipt means the exact role was supplied, not that the persona agreed or the human did the work.");
        if items.is_empty() {
            ui.label("No matching accepted inputs on this page. A name in a message or an invitation alone does not count.");
        }
        for (i, row) in items.iter().enumerate() {
            ui.push_id(i, |ui| {
                if row["kind"] == "unavailable" {
                    ui.colored_label(theme::ACCENT, "Input unavailable · not counted as received");
                    ui.small(s(row, "reason"));
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(app.label(s(row, "persona")));
                        ui.small(s(row, "accepted_utc"));
                        match row["continuation"].as_str() {
                            Some("stop") => { ui.small("Chose to stop"); }
                            Some("wait") => { ui.small("Chose to wait"); }
                            _ => {}
                        }
                        if ui
                            .small_button("Supplied input ↗")
                            .on_hover_text(s(row, "input"))
                            .clicked()
                        {
                            selected = Some(s(row, "input").into());
                        }
                        if ui
                            .small_button("Decision ↗")
                            .on_hover_text(s(row, "decision"))
                            .clicked()
                        {
                            selected = Some(s(row, "decision").into());
                        }
                    });
                }
            });
        }
        let mut read = false;
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.small("Newest input first");
            if !self.cursors.is_empty() && ui.small_button("← Newer").clicked() {
                self.cursors.pop();
                read = true;
            }
            if let Some(before) = r["next_before"].as_i64() {
                if ui.small_button("Older →").clicked() {
                    self.cursors.push(before);
                    read = true;
                }
            }
            if ui.small_button("Refresh").clicked() {
                self.cursors.clear();
                read = true;
            }
        });
        if let Some(reference) = selected {
            app.select_activity(work);
            app.open_choice_or_record(ui.ctx(), reference);
            return true;
        }
        if read {
            self.read(ui.ctx(), &app.root);
        }
        false
    }
}

impl App {
    pub(super) fn open_role_receipts(&mut self, ctx: &egui::Context, role: String) {
        let mut w = Window {
            role,
            cursors: vec![],
            pending: None,
            result: None,
        };
        w.read(ctx, &self.root);
        self.role_receipts = Some(w);
    }
    pub(super) fn role_receipts_window(&mut self, ctx: &egui::Context) {
        let Some(mut w) = self.role_receipts.take() else {
            return;
        };
        let (mut open, mut jumped) = (true, false);
        let bounds = (ctx.content_rect().size() - egui::vec2(38., 84.)).max(egui::vec2(1., 1.));
        egui::Window::new("Who received this role?")
            .id(egui::Id::new("task-role-receipts"))
            .open(&mut open)
            .default_size(egui::vec2(630., 435.).min(bounds))
            .min_size(egui::vec2(1., 1.))
            .max_size(bounds)
            .vscroll(true)
            .show(ctx, |ui| {
                jumped = w.draw(ui, self);
            });
        if open && !jumped {
            self.role_receipts = Some(w);
        }
    }
}
