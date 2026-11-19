//! Read and answer shared field reports while keeping the source revision and
//! reporting activity attached to the draft.
use super::{activity_title, card, data, excerpt, s, theme, time_label, TEAL};
use ai_personas::neural::{
    self,
    asset_feedback::{self, Feedback, History},
    cli::Command,
    store,
};
use eframe::egui::{self, RichText, Ui};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::mpsc};

const PAGE: usize = 8;
type Response = Result<Value, String>;

#[derive(Default)]
struct Draft {
    work: String,
    text: String,
    evidence: String,
    reply: Option<(String, String)>,
    request: Option<(String, String)>,
}

#[derive(Default)]
pub(super) struct Window {
    visible: bool,
    reference: String,
    drafts: BTreeMap<String, Draft>,
    page: Option<Value>,
    offset: usize,
    pending: Option<mpsc::Receiver<Response>>,
    publishing: bool,
    composing: bool,
    error: Option<String>,
    posted: Option<String>,
}

impl Window {
    pub(super) fn open(&mut self, ctx: &egui::Context, root: &Path, reference: &str, work: &str) {
        self.visible = true;
        if self.publishing {
            return;
        }
        self.composing = false;
        self.reference = reference.to_owned();
        self.drafts
            .entry(self.reference.clone())
            .or_insert_with(|| Draft {
                work: work.to_owned(),
                ..Default::default()
            });
        self.posted = None;
        self.load(ctx, root, 0);
    }

    fn worker(&mut self, ctx: &egui::Context, action: impl FnOnce() -> Response + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.error = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(action());
            ctx.request_repaint();
        });
    }

    fn load(&mut self, ctx: &egui::Context, root: &Path, offset: usize) {
        self.offset = offset;
        self.page = None;
        let query = History {
            reference: self.reference.clone(),
            offset,
            page_size: PAGE,
        };
        let root = root.to_path_buf();
        self.worker(ctx, move || {
            data::open(&root)
                .and_then(|db| feedback_page(&db, &query))
                .map_err(|e| e.to_string())
        });
    }

    fn publish(&mut self, ctx: &egui::Context, root: &Path, operator: &str) -> neural::Result<()> {
        let draft = self
            .drafts
            .get_mut(&self.reference)
            .ok_or_else(|| neural::failure("Choose a revision"))?;
        let command = Command::AssetFeedback {
            work: draft.work.clone(),
            feedback: Feedback {
                reference: self.reference.clone(),
                text: draft.text.trim().into(),
                evidence: draft
                    .evidence
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
                reply_to: draft.reply.as_ref().map(|(handle, _)| handle.clone()),
            },
        };
        let operator = operator.trim().to_owned();
        let intent = serde_json::to_string(&(&operator, &command))?;
        let request = match &draft.request {
            Some((previous, key)) if *previous == intent => key.clone(),
            _ => {
                let key = neural::id("studio-feedback")?;
                draft.request = Some((intent, key.clone()));
                key
            }
        };
        let root = root.to_path_buf();
        self.publishing = true;
        self.posted = None;
        self.worker(ctx, move || {
            super::operator_call(&root,&operator,command,Some(&request))
        });
        Ok(())
    }

    fn receive(&mut self, ctx: &egui::Context, root: &Path) {
        let Some(rx) = &self.pending else { return };
        let answer = match rx.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The workspace worker disconnected. Retry the same request.".into())
            }
        };
        self.pending = None;
        let publishing = std::mem::take(&mut self.publishing);
        match answer {
            Err(error) => self.error = Some(error),
            Ok(answer) if publishing => {
                self.composing = false;
                self.posted = answer["reference"].as_str().map(str::to_owned);
                if let Some(draft) = self.drafts.get_mut(&self.reference) {
                    *draft = Draft {
                        work: std::mem::take(&mut draft.work),
                        ..Default::default()
                    };
                }
                self.load(ctx, root, 0);
            }
            Ok(page) => self.page = Some(page),
        }
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        operator: &str,
        status: &Value,
    ) -> Option<String> {
        self.receive(ctx, root);
        if !self.visible {
            return None;
        }
        let busy = self.pending.is_some();
        let mut visible = true;
        let mut inspect = None;
        let mut next_page = None;
        let mut send = false;
        egui::Window::new("Resource feedback")
            .open(&mut visible)
            .id(egui::Id::new("studio-resource-feedback"))
            .default_width(565.)
            .default_height(548.)
            .vscroll(true)
            .show(ctx, |ui| {
                let page = self.page.as_ref().unwrap_or(&Value::Null);
                if let Some(path) = page["path"].as_str() {
                    ui.heading(path);
                }
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(&self.reference);
                    if ui
                        .small_button("Copy")
                        .on_hover_text("Copy exact asset revision")
                        .clicked()
                    {
                        ctx.copy_text(self.reference.clone());
                    }
                });
                let total = page["total"].as_u64().unwrap_or(0) as usize;
                ui.horizontal(|ui| {
                    let reports = if self.page.is_some() {
                        format!("Reports · {total}")
                    } else {
                        "Reports".into()
                    };
                    ui.selectable_value(&mut self.composing, false, reports);
                    ui.selectable_value(&mut self.composing, true, "Write feedback");
                    if ui
                        .add_enabled(!busy, egui::Button::new("Refresh").small())
                        .clicked()
                    {
                        next_page = Some(0);
                    }
                    if busy {
                        ui.spinner();
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                if let Some(posted) = &self.posted {
                    ui.horizontal(|ui| {
                        ui.colored_label(TEAL, "Report published");
                        if ui.small_button("Open record").clicked() {
                            inspect = Some(posted.clone());
                        }
                    });
                }
                let works = status["work"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let work_title = |id: &str| {
                    works
                        .iter()
                        .find(|w| w["body"]["id"] == id)
                        .map(|w| activity_title(&w["body"]))
                        .unwrap_or("")
                        .to_owned()
                };
                let Some(draft) = self.drafts.get_mut(&self.reference) else {
                    return;
                };
                ui.add_enabled_ui(!self.publishing, |ui| {
                    if !self.composing {
                        if total == 0 && self.page.is_some() {
                            ui.weak("No reports on this revision yet.");
                        }
                        for entry in page["reports"].as_array().into_iter().flatten() {
                            let report = &entry["report"];
                            match report_card(
                                ui,
                                report,
                                entry,
                                &work_title(s(report, "work")),
                                status,
                                &page["evidence_labels"],
                            ) {
                                Some(ReportAction::Inspect(reference)) => inspect = Some(reference),
                                Some(ReportAction::Reply) => {
                                    draft.reply = Some((
                                        s(report, "reference").into(),
                                        person_name(
                                            status,
                                            s(report, "author"),
                                            s(report, "author_kind"),
                                        )
                                        .into(),
                                    ));
                                    self.composing = true;
                                    ui.scroll_to_cursor(Some(egui::Align::TOP));
                                }
                                None => {}
                            }
                        }
                        if self.offset > 0 || self.offset.saturating_add(PAGE) < total {
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        !busy && self.offset > 0,
                                        egui::Button::new("Newer"),
                                    )
                                    .clicked()
                                {
                                    next_page = Some(self.offset.saturating_sub(PAGE));
                                }
                                if ui
                                    .add_enabled(
                                        !busy && self.offset.saturating_add(PAGE) < total,
                                        egui::Button::new("Older"),
                                    )
                                    .clicked()
                                {
                                    next_page = Some(self.offset.saturating_add(PAGE));
                                }
                            });
                        }
                        return;
                    }
                    ui.separator();
                    if let Some((handle, who)) = draft.reply.clone() {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!("Reply to {who}")).on_hover_text(handle);
                            if ui.small_button("Cancel reply").clicked() {
                                draft.reply = None;
                            }
                        });
                    } else {
                        let owner = page["content_author"]
                            .as_str()
                            .unwrap_or("the content author");
                        ui.strong(format!(
                            "Report to {}",
                            person_name(status, owner, "persona")
                        ));
                    }
                    ui.weak(format!(
                        "Published as {operator} · Visible to this workspace"
                    ));
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Reporting activity");
                        let title = work_title(&draft.work);
                        egui::ComboBox::from_id_salt("reporting-activity")
                            .width(272.)
                            .selected_text(if title.is_empty() {
                                "Choose an activity".into()
                            } else {
                                excerpt(&title, 36)
                            })
                            .show_ui(ui, |ui| {
                                for w in works {
                                    let body = &w["body"];
                                    let title = activity_title(body);
                                    ui.selectable_value(
                                        &mut draft.work,
                                        s(body, "id").to_owned(),
                                        excerpt(title, 52),
                                    )
                                    .on_hover_text(format!("{title}\n{}", s(body, "id")));
                                }
                            });
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut draft.text)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .hint_text("What happened when you used this revision?"),
                    );
                    ui.label("Evidence");
                    ui.add(
                        egui::TextEdit::multiline(&mut draft.evidence)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .hint_text(
                                "Exact receipt or resource references, separated by whitespace",
                            ),
                    );
                    send = ui
                        .add_enabled(
                            !busy
                                && self.page.is_some()
                                && !draft.text.trim().is_empty()
                                && !draft.work.is_empty()
                                && !operator.trim().is_empty(),
                            egui::Button::new("Publish feedback"),
                        )
                        .clicked();
                });
            });
        self.visible = visible && inspect.is_none();
        if let Some(offset) = next_page {
            self.load(ctx, root, offset);
        }
        if send {
            if let Err(error) = self.publish(ctx, root, operator) {
                self.error = Some(error.to_string());
            }
        }
        inspect
    }
}

enum ReportAction {
    Inspect(String),
    Reply,
}

fn person_name<'a>(status: &'a Value, name: &'a str, kind: &str) -> &'a str {
    if kind == "persona" {
        if let Some(label) = status["personas"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["body"]["id"] == name)
            .and_then(|p| p["body"]["label"].as_str())
        {
            return label;
        }
    }
    name
}

fn feedback_page(db: &rusqlite::Connection, query: &History) -> neural::Result<Value> {
    let mut page = asset_feedback::history(db, query)?;
    let mut labels = BTreeMap::new();
    for entry in page["reports"].as_array().into_iter().flatten() {
        for reference in entry["report"]["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            labels
                .entry(reference)
                .or_insert_with(|| evidence_label(db, reference));
        }
    }
    page["evidence_labels"] = serde_json::to_value(labels)?;
    Ok(page)
}

fn evidence_label(db: &rusqlite::Connection, reference: &str) -> String {
    let (kind, identity) = reference.split_once(':').unwrap_or(("resource", reference));
    match kind {
        "asset" => {
            if let Ok(asset) = store::exact(db, reference) {
                let bytes = db
                    .query_row(
                        "SELECT length(bytes) FROM objects WHERE digest=?1",
                        [s(&asset.body, "blob")],
                        |row| row.get::<_, i64>(0),
                    )
                    .ok();
                let size = bytes
                    .map(|b| format!(" · {}", super::size(b as u64)))
                    .unwrap_or_default();
                return format!(
                    "{} · v{}{size}",
                    s(&asset.body, "path"),
                    asset.reference.revision
                );
            }
        }
        "command_run" => {
            if let Ok(run) = store::exact(db, reference) {
                let exit = run.body["exit_code"]
                    .as_i64()
                    .map(|v| format!("exit {v}"))
                    .unwrap_or_else(|| "no exit code".into());
                let input = run.body["stdin"]["reference"]
                    .as_str()
                    .filter(|r| r.starts_with("asset:"))
                    .and_then(|r| store::exact(db, r).ok())
                    .and_then(|a| {
                        Path::new(s(&a.body, "path"))
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                    });
                let name = input.as_deref().unwrap_or_else(|| s(&run.body, "argv0"));
                return format!("Command receipt · {name} · {exit}");
            }
        }
        _ => {}
    }
    let n = identity.chars().count();
    let display = if n > 24 {
        format!("…{}", identity.chars().skip(n - 12).collect::<String>())
    } else {
        identity.to_owned()
    };
    format!("{} · {display}", super::title(kind))
}

fn report_card(
    ui: &mut Ui,
    report: &Value,
    entry: &Value,
    activity: &str,
    status: &Value,
    evidence_labels: &Value,
) -> Option<ReportAction> {
    let mut inspect = None;
    card(ui).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(person_name(
                    status,
                    s(report, "author"),
                    s(report, "author_kind"),
                ))
                .strong()
                .color(theme::palette(ui).text),
            );
            theme::badge(ui, s(report, "author_kind"), TEAL);
            ui.weak(
                entry["time_utc"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| time_label(entry["time"].as_i64().unwrap_or(0))),
            );
        });
        ui.weak(excerpt(activity, 72))
            .on_hover_text(format!("{activity}\n{}", s(report, "work")));
        if let Some(parent) = report["reply_to"].as_str() {
            if ui
                .small_button(format!(
                    "In reply to {}",
                    person_name(status, s(report, "recipient"), s(report, "recipient_kind"))
                ))
                .on_hover_text(parent)
                .clicked()
            {
                inspect = Some(ReportAction::Inspect(parent.to_owned()));
            }
        }
        let text = s(report, "text");
        if text.chars().count() > 320 {
            let id = ui.id().with(("full-report", s(report, "reference")));
            let expanded = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
            if expanded {
                ui.label(text);
            } else {
                ui.label(excerpt(text, 320));
            }
            if ui
                .small_button(if expanded {
                    "Show less"
                } else {
                    "Read full report"
                })
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(id, !expanded));
            }
        } else {
            ui.label(text);
        }
        for reference in report["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let label = evidence_labels[reference].as_str().unwrap_or(reference);
            if ui.link(label).on_hover_text(reference).clicked() {
                inspect = Some(ReportAction::Inspect(reference.into()));
            }
        }
        ui.horizontal(|ui| {
            if ui.small_button("Reply").clicked() {
                inspect = Some(ReportAction::Reply);
            }
            if ui.small_button("Record").clicked() {
                inspect = Some(ReportAction::Inspect(s(report, "reference").into()));
            }
        });
    });
    ui.add_space(4.);
    inspect
}
