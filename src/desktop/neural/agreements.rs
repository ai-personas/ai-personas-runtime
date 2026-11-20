//! An activity's terms, editions and individually attributed responses.
mod form;
use super::*;
use ai_personas::neural::{agreements as model, relationships::Contact};
pub(super) use form::Form;
use std::sync::mpsc::{Receiver, TryRecvError};

const PAGE: usize = 6;

#[derive(Clone, PartialEq)]
struct Selection {
    work: String,
    operator: String,
    edition: Option<String>,
    index_offset: usize,
    response_offset: usize,
    history: bool,
}

struct Report {
    index: Value,
    detail: Option<Value>,
}

fn load(root: &std::path::Path, selection: &Selection) -> ai_personas::neural::Result<Report> {
    let db = data::open(root)?;
    let reader = Contact::operator(&selection.operator);
    let read = |edition: Option<String>, offset| {
        model::inspect(
            &db,
            &selection.work,
            &reader,
            &model::Lookup {
                latest_only: edition.is_some() && !selection.history,
                edition,
                offset,
                page_size: PAGE,
            },
        )
    };
    let index = read(None, selection.index_offset)?;
    let selected = selection
        .edition
        .as_deref()
        .or_else(|| index["items"].get(0)?["reference"].as_str());
    let detail = selected
        .map(|edition| read(Some(edition.to_owned()), selection.response_offset))
        .transpose()?;
    Ok(Report { index, detail })
}

#[derive(Default)]
pub(super) struct Book {
    work: String,
    edition: Option<String>,
    index_offset: usize,
    response_offset: usize,
    history: bool,
    reading: bool,
    requested: Option<(Selection, i64)>,
    pending: Option<Receiver<Result<Report, String>>>,
    report: Option<Report>,
    error: Option<String>,
}

enum Action {
    Edition(String),
    IndexPage(usize),
    ResponsePage(usize),
    History(bool),
    Compose(Form),
    Activity,
    Index,
    Inspect(String),
}

impl Book {
    pub(super) fn request_refresh(&mut self) {
        self.requested = None;
    }
    fn open(&mut self, work: &str, edition: Option<String>) {
        if self.work != work {
            *self = Self::default();
            self.work = work.into();
        }
        self.reading = edition.is_some();
        self.edition = edition;
        self.response_offset = 0;
        self.history = false;
    }

    fn refresh(
        &mut self,
        ctx: &egui::Context,
        root: &std::path::Path,
        operator: &str,
        cursor: i64,
    ) {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(report) => {
                            self.report = Some(report);
                            self.error = None;
                        }
                        Err(error) => {
                            self.error = Some(error);
                            self.report = None;
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.report = None;
                    self.error = Some("The agreement reader closed.".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let selection = Selection {
            work: self.work.clone(),
            operator: operator.to_owned(),
            edition: self.edition.clone(),
            index_offset: self.index_offset,
            response_offset: self.response_offset,
            history: self.history,
        };
        let same = self
            .requested
            .as_ref()
            .is_some_and(|(s, _)| s == &selection);
        if same
            && (self.pending.is_some()
                || self
                    .requested
                    .as_ref()
                    .is_some_and(|(_, seen)| *seen == cursor))
        {
            return;
        }
        if !same {
            self.report = None;
            self.error = None;
        }
        self.requested = Some((selection.clone(), cursor));
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let (ctx, root) = (ctx.clone(), root.to_owned());
        std::thread::spawn(move || {
            let _ = tx.send(load(&root, &selection).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn page(&mut self, ui: &mut Ui, operator: &str, people: &[Value]) -> Option<Action> {
        let mut action = None;
        let wide = ui.available_width() >= 730.;
        egui::ScrollArea::vertical()
            .id_salt((
                "agreement-book",
                &self.work,
                &self.edition,
                self.reading,
                self.index_offset,
                self.response_offset,
            ))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.link("← Activity").clicked() {
                        action = Some(Action::Activity);
                    }
                    if self.reading && !wide && ui.link("All editions").clicked() {
                        action = Some(Action::Index);
                    }
                });
                ui.add_space(6.);
                ui.label(RichText::new("Agreement book").size(28.).strong());
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if ui.button("Retry").clicked() {
                        self.requested = None;
                    }
                }
                let Some(report) = &self.report else {
                    if self.pending.is_some() {
                        ui.spinner();
                    }
                    return;
                };
                let selected = report.detail.as_ref().and_then(|v| v["reference"].as_str());
                let total = report.index["total"].as_u64().unwrap_or(0);
                ui.horizontal_wrapped(|ui| {
                    ui.weak(counted(total, "edition", "editions"));
                    if report.index["ended"] == true {
                        ui.weak("Activity ended");
                    } else if ui.small_button("Propose terms…").clicked() {
                        action = Some(Action::Compose(Form::proposal(
                            &self.work,
                            operator,
                            &report.index,
                            None,
                        )));
                    }
                });
                ui.add_space(10.);
                if total == 0 {
                    ui.label(
                        "People in this activity can propose terms and record their own responses.",
                    );
                    return;
                }
                if wide {
                    let gap = 18.;
                    let width = ui.available_width();
                    let left = (width * 0.30).clamp(195., 330.);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.vertical(|ui| {
                            ui.set_width(left);
                            ui.spacing_mut().item_spacing.x = 6.;
                            index(ui, &report.index, selected, people, &mut action);
                        });
                        ui.vertical(|ui| {
                            ui.set_width(width - left - gap);
                            ui.spacing_mut().item_spacing.x = 6.;
                            if let Some(detail) = &report.detail {
                                reader(ui, detail, operator, people, self.history, &mut action);
                            }
                        });
                    });
                } else if self.reading {
                    if let Some(detail) = &report.detail {
                        reader(ui, detail, operator, people, self.history, &mut action);
                    }
                } else {
                    index(ui, &report.index, selected, people, &mut action);
                }
            });
        action
    }
}

impl App {
    pub(super) fn show_agreements(&mut self, edition: Option<String>) {
        self.compact_browser = false;
        self.agreement_book.open(&self.work, edition);
        self.layout.editor = Editor::Agreements;
    }
    pub(super) fn agreements(&mut self, ui: &mut Ui) {
        if self.work.is_empty() {
            ui.heading("Agreement book");
            ui.label("Open an activity to see its agreements.");
            return;
        }
        if self.agreement_book.work != self.work {
            self.agreement_book.open(&self.work, None);
        }
        self.agreement_book
            .refresh(ui.ctx(), &self.root, self.operator.trim(), self.cursor);
        let people = self.snapshot.status["personas"]
            .as_array()
            .map_or(&[][..], |p| p.as_slice());
        match self.agreement_book.page(ui, self.operator.trim(), people) {
            Some(Action::Edition(reference)) => {
                self.agreement_book.open(&self.work, Some(reference))
            }
            Some(Action::IndexPage(offset)) => {
                self.agreement_book.index_offset = offset;
                self.agreement_book.open(&self.work, None);
            }
            Some(Action::History(history)) => {
                self.agreement_book.history = history;
                self.agreement_book.response_offset = 0;
            }
            Some(Action::ResponsePage(offset)) => self.agreement_book.response_offset = offset,
            Some(Action::Index) => self.agreement_book.reading = false,
            Some(Action::Activity) => self.layout.editor = Editor::Studio,
            Some(Action::Inspect(reference)) => self.open_resource(ui.ctx(), reference),
            Some(Action::Compose(form)) => {
                self.error = None;
                self.dialog = Some(Dialog::Agreement(form));
            }
            None => {}
        }
    }
}

fn name(author: &Value, people: &[Value]) -> String {
    let id = s(author, "id");
    if author["kind"] == "operator" {
        return format!("{id} · Operator");
    }
    people
        .iter()
        .find(|person| person["body"]["id"] == id)
        .and_then(|p| p["body"]["label"].as_str())
        .unwrap_or(id)
        .to_owned()
}
fn version(reference: &str) -> &str {
    reference.rsplit_once('@').map_or("?", |(_, v)| v)
}

fn pager(ui: &mut Ui, report: &Value) -> Option<usize> {
    let total = report["total"].as_u64().unwrap_or(0) as usize;
    if total <= PAGE {
        return None;
    }
    let start = report["offset"].as_u64().unwrap_or(0) as usize;
    let mut next = None;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(start > 0, egui::Button::new("Newer").small())
            .clicked()
        {
            next = Some(start.saturating_sub(PAGE));
        }
        ui.small(format!(
            "{}–{} of {total}",
            start + 1,
            (start + PAGE).min(total)
        ));
        if ui
            .add_enabled(start + PAGE < total, egui::Button::new("Older").small())
            .clicked()
        {
            next = Some(start + PAGE);
        }
    });
    next
}

fn index(
    ui: &mut Ui,
    report: &Value,
    selected: Option<&str>,
    people: &[Value],
    action: &mut Option<Action>,
) {
    eyebrow(ui, "Editions");
    let palette = theme::palette(ui);
    for row in report["items"].as_array().into_iter().flatten() {
        let reference = s(row, "reference");
        let selected = Some(reference) == selected;
        ui.push_id(reference, |ui| {
            let label = s(row, "title");
            let frame = egui::Frame::new()
                .fill(if selected {
                    palette.selection
                } else {
                    palette.panel
                })
                .stroke(Stroke::new(
                    1.,
                    if selected {
                        palette.accent
                    } else {
                        palette.border
                    },
                ))
                .corner_radius(6)
                .inner_margin(10);
            let response = frame
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let revision = format!(
                        "Edition {}{}",
                        row["revision"],
                        if row["latest"] == false {
                            " · Earlier"
                        } else {
                            ""
                        }
                    );
                    ui.small(revision);
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).strong().size(16.))
                                .frame(false)
                                .wrap(),
                        )
                        .clicked()
                    {
                        *action = Some(Action::Edition(reference.into()));
                    }
                    ui.weak(name(&row["author"], people));
                })
                .response;
            if response
                .interact(egui::Sense::click())
                .on_hover_text(label)
                .clicked()
            {
                *action = Some(Action::Edition(reference.into()));
            }
            ui.add_space(3.);
        });
    }
    if let Some(offset) = pager(ui, report) {
        *action = Some(Action::IndexPage(offset));
    }
}

fn reader(
    ui: &mut Ui,
    detail: &Value,
    operator: &str,
    people: &[Value],
    history: bool,
    action: &mut Option<Action>,
) {
    let reference = s(detail, "reference");
    let work = s(detail, "work");
    let record = &detail["record"];
    let current = detail["latest"] == reference;
    let current_brief = detail["brief"] == record["brief"];
    let open = detail["ended"] != true;
    let standing = detail["your_position"]["body"]["position"]
        .as_str()
        .is_some_and(|v| v != "withdraw");
    let palette = theme::palette(ui);
    let sheet = egui::Frame::new()
        .fill(palette.header)
        .stroke(Stroke::new(1., palette.border))
        .corner_radius(8)
        .inner_margin(15);
    let surface = sheet
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::badge(
                    ui,
                    &format!("Edition {}", version(reference)),
                    theme::ACCENT,
                );
                if !current && ui.link("Read latest edition").clicked() {
                    *action = Some(Action::Edition(s(detail, "latest").into()));
                }
                if !current_brief {
                    ui.weak("Earlier activity brief");
                }
            });
            ui.add_space(5.);
            ui.label(RichText::new(s(record, "title")).strong().size(23.));
            ui.small(format!("Proposed by {}", name(&record["author"], people)));
            ui.add_space(10.);
            eyebrow(ui, "Scope");
            ui.label(s(record, "scope"));
            ui.add_space(7.);
            eyebrow(ui, "Terms");
            ui.label(RichText::new(s(record, "terms")).size(17.));
            ui.add_space(9.);
            ui.horizontal_wrapped(|ui| {
                if open
                    && ((current && current_brief) || standing)
                    && ui
                        .button(if detail["your_position"].is_object() {
                            "Change my position…"
                        } else {
                            "Record my position…"
                        })
                        .clicked()
                {
                    *action = Some(Action::Compose(Form::response(
                        work,
                        operator,
                        detail,
                        !current || !current_brief,
                    )));
                }
                if open && current && ui.small_button("Revise terms…").clicked() {
                    *action = Some(Action::Compose(Form::proposal(
                        work,
                        operator,
                        detail,
                        Some(record),
                    )));
                }
                if let Some(prior) = record["previous"].as_str() {
                    if ui.small_button("Previous edition").clicked() {
                        *action = Some(Action::Edition(prior.into()));
                    }
                }
            });
        })
        .response
        .rect;
    ui.painter().line_segment(
        [
            surface.left_top() + egui::vec2(2., 9.),
            surface.left_bottom() + egui::vec2(2., -9.),
        ],
        Stroke::new(2., palette.ink(theme::ACCENT)),
    );
    ui.add_space(13.);
    ui.heading("Signed responses");
    let counts = &detail["positions"];
    let counts = [
        (
            counts["endorse"].as_u64().unwrap_or(0),
            "endorsed",
            theme::GREEN,
        ),
        (counts["dissent"].as_u64().unwrap_or(0), "dissented", PURPLE),
        (
            counts["withdraw"].as_u64().unwrap_or(0),
            "withdrew",
            palette.muted,
        ),
    ];
    let total: u64 = counts.iter().map(|(n, _, _)| *n).sum();
    if total > 0 {
        ui.horizontal_wrapped(|ui| {
            for (n, label, color) in counts {
                if n > 0 {
                    ui.colored_label(palette.ink(color), format!("{n} {label}"));
                }
            }
        });
        let (bar, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.), egui::Sense::hover());
        let mut x = bar.left();
        for (n, _, color) in counts {
            let end = x + bar.width() * n as f32 / total as f32;
            if n > 0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x, bar.top()),
                        egui::pos2(end, bar.bottom()),
                    ),
                    1.,
                    palette.ink(color),
                );
            }
            x = end;
        }
        ui.add_space(4.);
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!history, "Current positions").clicked() && history {
                *action = Some(Action::History(false));
            }
            if ui.selectable_label(history, "Response history").clicked() && !history {
                *action = Some(Action::History(true));
            }
        });
    }
    if detail["total"] == 0 {
        ui.weak("No one has recorded a position on this edition.");
        return;
    }
    for item in detail["items"].as_array().into_iter().flatten() {
        let (label, color) = match s(item, "position") {
            "endorse" => ("Endorsed", theme::GREEN),
            "dissent" => ("Dissented", PURPLE),
            _ => ("Withdrew", palette.muted),
        };
        let author = name(&item["author"], people);
        ui.push_id(s(item, "reference"), |ui| {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, label, color);
                ui.strong(author);
                if item["latest"] == false {
                    ui.weak("Earlier position");
                }
            });
            ui.label(s(item, "reason"));
            if ui.small_button("View signed record").clicked() {
                *action = Some(Action::Inspect(s(item, "reference").into()));
            }
        });
    }
    if let Some(offset) = pager(ui, detail) {
        *action = Some(Action::ResponsePage(offset));
    }
}
