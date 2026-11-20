//! Reading a portable report only changes this request's unsent response draft.
use super::*;
use ai_personas::neural::{human_requests::FieldReport, input_files::CapturedFile};
use std::path::Path;

#[derive(Default)]
pub(super) struct Reader {
    pub report: Option<Arc<FieldReport>>,
    choosing: bool,
    path: String,
    pending: Option<mpsc::Receiver<Result<Option<FieldReport>, String>>>,
    error: Option<String>,
}

impl Reader {
    pub fn busy(&self) -> bool {
        self.choosing || self.pending.is_some()
    }

    fn open(&mut self, ctx: &egui::Context, root: &Path, reference: &str, path: Option<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.choosing = false;
        self.error = None;
        let (ctx, root, reference) = (ctx.clone(), root.to_owned(), reference.to_owned());
        std::thread::spawn(move || {
            let path = path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title("Import field report")
                    .add_filter("Field report", &["json"])
                    .pick_file()
            });
            let result = path
                .map(|path| {
                    let report = FieldReport::read(&path)?;
                    report.check_request(&data::open(&root)?, &reference)?;
                    Ok(report)
                })
                .transpose()
                .map_err(|e: ai_personas::neural::Error| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) -> Option<String> {
        let rx = self.pending.as_ref()?;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("The field report reader stopped.".into()),
        };
        self.pending = None;
        match result {
            Ok(Some(report)) => {
                self.path.clear();
                let reference = report.request().to_owned();
                self.report = Some(Arc::new(report));
                Some(reference)
            }
            Ok(None) => None,
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, root: &Path, reference: &str, files: &[CapturedFile]) {
        if let Some(report) = &self.report {
            ui.strong(format!("Field report from {}", report.observer()))
                .on_hover_text("Name entered by the observer on the returned form");
            ui.small(format!(
                "From request edition {}",
                report.request().rsplit_once('@').map_or("?", |v| v.1)
            ))
            .on_hover_text(report.request());
            ui.add(
                egui::Label::new(excerpt(report.observations(), 480))
                    .wrap()
                    .selectable(true),
            );
            if report.observations().chars().nth(480).is_some() {
                ui.collapsing("Read full field report", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("field-observations")
                        .max_height(180.)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(report.observations())
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                });
            }
            let missing = report.missing_evidence(files);
            if !missing.is_empty() {
                ui.label("Attach these original evidence files before sending");
                egui::ScrollArea::vertical()
                    .id_salt("field-evidence")
                    .max_height(75.)
                    .show_rows(ui, 16., missing.len(), |ui, rows| {
                        for index in rows {
                            let file = missing[index];
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{} · {}", file.name, size(file.bytes)))
                                        .small(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(format!("{}\nSHA-256 {}", file.name, file.sha256));
                        }
                    });
            } else {
                ui.small("The original field report will accompany your response.")
                    .on_hover_text(report.source().sha256());
            }
            if ui.small_button("Remove field report").clicked() {
                self.report = None;
            }
        } else if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading field report…");
            });
        } else if self.choosing {
            ui.label("Open the JSON saved from Request.html");
            ui.horizontal_wrapped(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.path)
                        .desired_width((ui.available_width() - 115.).clamp(74., 340.))
                        .hint_text("Local field report path"),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if !self.path.trim().is_empty()
                    && (ui.small_button("Read report").clicked() || enter)
                {
                    self.open(ui.ctx(), root, reference, Some(PathBuf::from(&self.path)));
                }
                if ui.small_button("Cancel").clicked() {
                    self.choosing = false;
                }
            });
        } else {
            ui.menu_button("Import field report…", |ui| {
                if ui.button("Choose report…").clicked() {
                    ui.close();
                    self.open(ui.ctx(), root, reference, None);
                }
                if ui.button("Enter file path…").clicked() {
                    ui.close();
                    self.choosing = true;
                }
            });
        }
        if let Some(reason) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, reason);
        }
    }
}
