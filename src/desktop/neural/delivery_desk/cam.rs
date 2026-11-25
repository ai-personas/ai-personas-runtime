//! Local receiving inspection of one exact issue. Never a supplier submission.
mod plot;
mod stack;
use super::*;
use crate::desktop::artifacts::PreviewCancellation;
use plot::{Archive, Plot};
use stack::Stack;
use std::path::Path;

enum Reply {
    Directory(Archive),
    Layers(Plot),
}
type Answer = Result<Reply, String>;
pub(super) struct Window {
    root: PathBuf,
    edition: String,
    file: String,
    name: String,
    archive: Option<Arc<Archive>>,
    chosen: Vec<bool>,
    pending: Option<mpsc::Receiver<Answer>>,
    cancel: Arc<PreviewCancellation>,
    stack: Option<Stack>,
    error: Option<String>,
}
impl Drop for Window {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Window {
    pub(super) fn new(ctx: &egui::Context, root: &Path, p: &Package, f: &File) -> Self {
        let mut w = Self {
            root: root.into(),
            edition: p.reference.clone(),
            file: f.reference.clone(),
            name: f.path.clone(),
            archive: None,
            chosen: Vec::new(),
            pending: None,
            cancel: Default::default(),
            stack: None,
            error: None,
        };
        w.open_archive(ctx);
        w
    }
    fn run(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(&PreviewCancellation) -> ai_personas::neural::Result<Reply> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        self.cancel.cancel();
        self.cancel = Arc::default();
        let cancel = self.cancel.clone();
        self.pending = Some(rx);
        self.error = None;
        self.stack = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work(&cancel).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }
    fn open_archive(&mut self, ctx: &egui::Context) {
        let (root, edition, file) = (self.root.clone(), self.edition.clone(), self.file.clone());
        self.run(ctx, move |cancel| {
            Archive::open(&root, &edition, &file, cancel).map(Reply::Directory)
        });
    }
    fn plot(&mut self, ctx: &egui::Context) {
        let Some(a) = self.archive.clone() else {
            return;
        };
        let paths = a
            .paths
            .iter()
            .zip(&self.chosen)
            .filter(|(_, on)| **on)
            .map(|(p, _)| p.clone())
            .collect::<Vec<_>>();
        self.run(ctx, move |cancel| a.plot(&paths, cancel).map(Reply::Layers));
    }
    fn receive(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending else { return };
        let answer = match rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The local CAM reader disconnected.".into())
            }
            Ok(a) => a,
        };
        self.pending = None;
        match answer {
            Ok(Reply::Directory(a)) => {
                // Several folders may be different board revisions. Ask rather
                // than silently mixing them; allow cross-folder selection.
                fn folder(p: &str) -> Option<&str> {
                    p.rsplit_once('/').map(|(dir, _)| dir)
                }
                let one = !a.paths.is_empty()
                    && a.paths.windows(2).all(|w| folder(&w[0]) == folder(&w[1]));
                self.chosen = vec![one; a.paths.len()];
                self.archive = Some(Arc::new(a));
                if one {
                    self.plot(ctx);
                }
            }
            Ok(Reply::Layers(p)) => self.stack = Some(Stack::new(ctx, p)),
            Err(e) => self.error = Some(e),
        }
    }
    fn picker(&mut self, ui: &mut Ui) {
        let Some(a) = &self.archive else {
            if ui.button("Retry reading this issue").clicked() {
                self.open_archive(ui.ctx());
            }
            return;
        };
        if a.paths.is_empty() {
            ui.label("No regular files with recognized Gerber or Excellon extensions in this ZIP. Its original files remain available in the delivery.");
            return;
        }
        ui.label("Select files for one board. Artwork and drills keep their original coordinates. Job files and nested archives are not plotted.");
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Select all").clicked() {
                self.chosen.fill(true);
            }
            if ui.small_button("Clear").clicked() {
                self.chosen.fill(false);
            }
        });
        for (path, on) in a.paths.iter().zip(&mut self.chosen) {
            ui.checkbox(on, path);
        }
        let n = self.chosen.iter().filter(|on| **on).count();
        if ui
            .add_enabled(n > 0, theme::primary(&format!("Plot selected files · {n}")))
            .clicked()
        {
            self.plot(ui.ctx());
        }
    }
    fn body(&mut self, ui: &mut Ui, issuer: &str, title: &str, height: f32) -> bool {
        ui.horizontal_wrapped(|ui| {
            theme::badge(ui, "Local inspection", TEAL);
            ui.strong(&self.name).on_hover_text(&self.file);
        });
        ui.label(format!("Issued by {issuer} · {title}"))
            .on_hover_text(&self.edition);
        ui.separator();
        if self.pending.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(if self.archive.is_some() {
                    "Plotting selected artwork and drills…"
                } else {
                    "Verifying issued ZIP…"
                });
                if ui.button("Cancel").clicked() {
                    self.cancel.cancel();
                    self.pending = None;
                }
            });
            return false;
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let (Some(a), Some(stack)) = (&self.archive, &mut self.stack) else {
            self.picker(ui);
            return false;
        };
        let mut choose = false;
        let mut feedback = false;
        ui.horizontal_wrapped(|ui| {
            choose = ui
                .button("Choose files…")
                .on_hover_text("Select the layers for one board within this ZIP")
                .clicked();
            if ui.button("Copy view details").clicked() {
                ui.ctx().copy_text(stack.details(a));
            }
            feedback = ui
                .button(format!("Feedback to {issuer}…"))
                .on_hover_text("Open an unsent message about this exact edition")
                .clicked();
            let n = self.chosen.iter().filter(|on| **on).count();
            ui.weak(format!("{n} of {} CAM files plotted", a.paths.len()));
        });
        if choose {
            self.stack = None;
        } else {
            stack.show(ui, height);
        }
        feedback
    }
}

impl App {
    pub(super) fn cam_overlay_window(&mut self, ctx: &egui::Context, p: &Package) {
        let Some(mut w) = self.delivery_desk.cam.take() else {
            return;
        };
        if self.root != w.root
            || w.edition != p.reference
            || self.delivery_desk.selected_file() != Some(&w.file)
        {
            return;
        }
        w.receive(ctx);
        let mut open = true;
        let mut feedback = false;
        let rect = ctx.content_rect().shrink(6.);
        let bounds = (rect.size() - egui::vec2(32., 76.)).max(egui::vec2(1., 1.));
        let owner = s(&p.body, "owner");
        let title = s(&p.body, "title");
        egui::Window::new("CAM layer overlay")
            .id(egui::Id::new(("issued-cam", &w.edition, &w.file)))
            .open(&mut open)
            .min_size(egui::vec2(200., 160.).min(bounds))
            .max_size(bounds)
            .default_size(egui::vec2(940., 590.).min(bounds))
            .constrain_to(rect)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.set_max_width(ui.available_width());
                feedback = w.body(
                    ui,
                    &self.label(owner),
                    title,
                    (bounds.y * 0.40).clamp(120., 335.),
                );
            });
        if feedback {
            self.open_delivery_feedback(&p.reference, s(&p.body, "work"), owner, title);
        } else if open {
            self.delivery_desk.cam = Some(w);
        }
    }
}
