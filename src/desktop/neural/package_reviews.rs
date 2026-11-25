//! Compare a delivered or proposed file set with the actual scope of a finding.
use super::{activity_title, card, data, excerpt, operator_call, s, theme, title, TEAL};
use ai_personas::neural::{
    self,
    cli::Command,
    input_files::CapturedFile,
    package_review::{self, EvidenceFile, Lookup, Review, Subject, Verdict},
};
use eframe::egui::{self, Color32, RichText, Ui};
use serde_json::Value;
use std::{path::Path, sync::mpsc};

const PAGE: usize = 6;

#[derive(Clone, Default, PartialEq)]
struct Selection {
    work: String,
    delivery: String,
    proposed: bool,
    files: String,
    inputs: String,
    specific: String,
}
fn handles(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}
impl Selection {
    fn subject(&self) -> Subject {
        if self.proposed {
            Subject {
                artifacts: handles(&self.files),
                inputs: handles(&self.inputs),
                ..Default::default()
            }
        } else {
            Subject {
                delivery: Some(self.delivery.trim().into()),
                ..Default::default()
            }
        }
    }
    fn can_compare(&self) -> bool {
        if self.proposed {
            !self.files.trim().is_empty()
        } else {
            !self.delivery.trim().is_empty()
        }
    }
}
#[derive(Default)]
struct Draft {
    criterion: String,
    finding: String,
    evidence: String,
    files: Vec<CapturedFile>,
    picker: super::input_files::Picker,
    verdict: Option<Verdict>,
    request: Option<(String, String)>,
}
pub(super) enum Action {
    Inspect(String),
    Save(EvidenceFile),
}

enum Reply {
    Page(Value),
    Published(String),
}

#[derive(Default)]
pub(super) struct Desk {
    visible: bool,
    reading_return: bool,
    selection: Selection,
    offset: usize,
    page: Option<Value>,
    pending: Option<mpsc::Receiver<Result<Reply, String>>>,
    draft: Option<Draft>,
    publishing: bool,
    error: Option<String>,
    posted: Option<String>,
}

impl Desk {
    pub(super) fn open(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        work: &str,
        delivery: Option<&str>,
    ) {
        self.open_selection(ctx, root, work, delivery, None);
    }
    pub(super) fn write_for(&mut self, ctx: &egui::Context, root: &Path, work: &str, edition: &str) {
        self.open(ctx, root, work, Some(edition));
        // Opening another entry point must never re-address an existing draft.
        if !self.publishing { self.draft.get_or_insert_with(Draft::default); }
    }
    pub(super) fn open_finding(
        &mut self, ctx: &egui::Context, root: &Path, work: &str, delivery: &str, finding: &str,
    ) {
        self.open_selection(ctx, root, work, Some(delivery), Some(finding));
    }
    fn open_selection(
        &mut self, ctx: &egui::Context, root: &Path, work: &str,
        delivery: Option<&str>, finding: Option<&str>,
    ) {
        self.visible = true;
        if self.draft.is_some() || self.publishing {
            return;
        }
        self.reading_return = finding.is_some();
        self.selection = Selection {
            work: work.into(),
            delivery: delivery.unwrap_or_default().into(),
            specific: finding.unwrap_or_default().into(),
            ..Default::default()
        };
        self.reset();
        if self.selection.can_compare() {
            self.load(ctx, root, 0);
        }
    }
    fn reset(&mut self) {
        self.page = None;
        self.pending = None;
        self.error = None;
        self.posted = None;
        self.offset = 0;
    }
    fn worker(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> Result<Reply, String> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.error = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }
    fn load(&mut self, ctx: &egui::Context, root: &Path, offset: usize) {
        self.offset = offset;
        self.page = None;
        let root = root.to_path_buf();
        let selected = &self.selection;
        let query = Lookup {
            subject: selected.subject(),
            review: (!selected.specific.trim().is_empty()).then(|| selected.specific.trim().into()),
            offset,
            page_size: PAGE,
        };
        let work = selected.work.clone();
        self.worker(ctx, move || {
            data::open(&root)
                .and_then(|db| {
                    package_review::lookup(&db, (!work.is_empty()).then_some(work.as_str()), &query)
                })
                .map(Reply::Page)
                .map_err(|e| e.to_string())
        });
    }
    fn publish(&mut self, ctx: &egui::Context, root: &Path, operator: &str) -> neural::Result<()> {
        let draft = self
            .draft
            .as_mut()
            .ok_or_else(|| neural::failure("Open a review draft"))?;
        let command = Command::PackageReview {
            work: self.selection.work.clone(),
            files: draft.files.clone().into(),
            review: Review {
                subject: self.selection.subject(),
                expected_mandate: self
                    .page
                    .as_ref()
                    .and_then(|p| p["subject"]["mandate"].as_str())
                    .map(str::to_owned),
                criterion: draft.criterion.trim().into(),
                finding: draft.finding.trim().into(),
                verdict: draft
                    .verdict
                    .ok_or_else(|| neural::failure("Choose your finding"))?,
                evidence: handles(&draft.evidence),
            },
        };
        let operator = operator.trim().to_owned();
        let intent = serde_json::to_string(&(&operator, &command))?;
        let id = match &draft.request {
            Some((earlier, key)) if *earlier == intent => key.clone(),
            _ => {
                let key = neural::id("studio-package-review")?;
                draft.request = Some((intent, key.clone()));
                key
            }
        };
        self.publishing = true;
        let root = root.to_path_buf();
        self.worker(ctx, move || {
            operator_call(&root, &operator, command, Some(&id)).and_then(|v| {
                v["reference"]
                    .as_str()
                    .map(|r| Reply::Published(r.to_owned()))
                    .ok_or_else(|| "Missing published review reference".into())
            })
        });
        Ok(())
    }
    fn receive(&mut self, ctx: &egui::Context, root: &Path) {
        let Some(rx) = &self.pending else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(x) => x,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The workspace worker disconnected. Retry the request.".into())
            }
        };
        self.pending = None;
        self.publishing = false;
        match result {
            Ok(Reply::Page(page)) => self.page = Some(page),
            Ok(Reply::Published(reference)) => {
                self.posted = Some(reference);
                self.draft = None;
                self.selection.specific.clear();
                self.load(ctx, root, 0);
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        operator: &str,
        status: &Value,
    ) -> Option<Action> {
        self.receive(ctx, root);
        if !self.visible {
            return None;
        }
        let mut visible = true;
        let mut inspect = None;
        let mut save = None;
        let mut load = None;
        let mut publish = false;
        let rect = ctx.content_rect().shrink(6.);
        let bounds = (rect.size() - egui::vec2(32., 76.)).max(egui::vec2(1., 1.));
        let heading = if self.reading_return { "Returned finding" } else { "Package reviews" };
        egui::Window::new(heading)
            .id(egui::Id::new("studio-package-reviews"))
            .open(&mut visible)
            .min_size(egui::vec2(180., 120.).min(bounds))
            .max_size(bounds)
            .default_size(egui::vec2(760., 590.).min(bounds))
            .constrain_to(rect)
            .resizable(true)
            .show(ctx, |ui| {
                if self.reading_return {
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(ui, "Peer review", TEAL);
                        ui.weak("Returned to the issuer · not an acceptance");
                        if ui.small_button("Browse other reviews…").clicked() {
                            self.reading_return = false;
                            self.selection.specific.clear();
                            load = Some(0);
                        }
                        if self.pending.is_some() { ui.spinner(); }
                    });
                } else if self.draft.is_none() {
                    let previous = self.selection.clone();
                    let compare = choose_subject(ui, &mut self.selection, status);
                    if previous != self.selection {
                        self.reset();
                    }
                    ui.horizontal_wrapped(|ui| {
                        if (ui
                            .add_enabled(
                                self.selection.can_compare() && self.pending.is_none(),
                                egui::Button::new("Compare reviews"),
                            )
                            .clicked()
                            || compare)
                            && self.selection.can_compare()
                        {
                            load = Some(0);
                        }
                        if ui
                            .add_enabled(
                                self.page.is_some()
                                    && !self.selection.work.is_empty()
                                    && !operator.trim().is_empty(),
                                egui::Button::new("Write review…"),
                            )
                            .clicked()
                        {
                            self.draft = Some(Draft::default());
                            self.posted = None;
                        }
                        if self.pending.is_some() {
                            ui.spinner();
                        }
                    });
                }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if self.reading_return && self.pending.is_none() && ui.button("Retry finding").clicked() {
                        load = Some(0);
                    } else if self.draft.is_some() && self.pending.is_none() {
                        ui.horizontal(|ui| {
                            if ui.button("Reload reviewed brief").clicked() {
                                load = Some(0);
                            }
                            if self.page.is_none() && ui.button("Cancel draft").clicked() {
                                self.draft = None;
                            }
                        });
                    }
                }
                if let Some(reference) = &self.posted {
                    ui.horizontal(|ui| {
                        ui.colored_label(TEAL, "Review published");
                        if ui.small_button("Open record").clicked() {
                            inspect = Some(reference.clone());
                        }
                    });
                }
                let Some(page) = &self.page else {
                    return;
                };
                let package = &page["subject"];
                ui.separator();
                let title = package["title"].as_str().unwrap_or("Proposed package");
                ui.heading(title);
                ui.horizontal_wrapped(|ui| {
                    let count = package["members"].as_object().map_or(0, |m| m.len());
                    ui.small(format!(
                        "{count} {}",
                        if count == 1 { "file" } else { "files" }
                    ));
                    if let Some(brief) = package["mandate"].as_str() {
                        ref_button(
                            ui,
                            brief,
                            format!("Brief · {}", version(brief)),
                            &mut inspect,
                        );
                    }
                    if let Some(delivery) = package["delivery"].as_str() {
                        ref_button(ui, delivery, "Delivery record", &mut inspect);
                    }
                });
                if let Some(draft) = &mut self.draft {
                    let mut cancel = false;
                    egui::ScrollArea::vertical()
                        .id_salt("package-review-compose")
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!self.publishing, |ui| {
                                ui.strong(format!("Finding by {operator}"));
                                ui.weak("Publish what you checked. This does not accept the edition or complete an outside step.");
                                let activity = status["work"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .find(|w| w["body"]["id"] == self.selection.work);
                                if let Some(activity) = activity {
                                    ui.weak(format!(
                                        "Reporting activity · {}",
                                        excerpt(activity_title(&activity["body"]), 82)
                                    ));
                                }
                                ui.label("Criterion");
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.criterion)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("Which claim or interface did you assess?"),
                                );
                                ui.horizontal_wrapped(|ui| {
                                    for (value, label) in [
                                        (Verdict::Supported, "Supported"),
                                        (Verdict::Concern, "Concern"),
                                        (Verdict::Inconclusive, "Inconclusive"),
                                    ] {
                                        if ui
                                            .selectable_label(
                                                draft.verdict.as_ref().is_some_and(|v| *v == value),
                                                label,
                                            )
                                            .clicked()
                                        {
                                            draft.verdict = Some(value);
                                        }
                                    }
                                });
                                ui.label("Finding");
                                ui.add(
                                    egui::TextEdit::multiline(&mut draft.finding)
                                        .desired_rows(4)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("What did you observe, under which conditions?"),
                                );
                                ui.label("Supporting references");
                                ui.add(
                                    egui::TextEdit::multiline(&mut draft.evidence)
                                        .desired_rows(1)
                                        .desired_width(f32::INFINITY)
                                        .hint_text(
                                            "Exact resources or receipts, separated by whitespace",
                                        ),
                                );
                                if let Some(role) = status["human_task_roles"].as_array().into_iter().flatten().find(|r| {
                                    let b = &r["body"];
                                    b["state"] == "active" && b["operator"] == operator && b["work"] == self.selection.work
                                }) {
                                    let reference = super::inbox::handle(role);
                                    if !draft.evidence.split_whitespace().any(|r| r == reference)
                                        && ui.small_button("Cite my role declaration").on_hover_text(s(&role["body"], "note")).clicked() {
                                        if !draft.evidence.is_empty() { draft.evidence.push('\n'); }
                                        draft.evidence.push_str(&reference);
                                    }
                                }
                                ui.add_space(5.);
                                ui.strong("My evidence files");
                                ui.small("Optional. These files will be available with your published finding, separately from the issuer’s files.");
                                draft.picker.show(ui, &mut draft.files);
                                ui.horizontal_wrapped(|ui| {
                                    publish = ui
                                        .add_enabled(
                                            !draft.picker.busy() && draft.verdict.is_some()
                                                && !draft.criterion.trim().is_empty()
                                                && !draft.finding.trim().is_empty(),
                                            egui::Button::new("Publish review"),
                                        )
                                        .clicked();
                                    cancel = ui.button("Cancel draft").clicked();
                                });
                            });
                        });
                    if cancel {
                        self.draft = None;
                    }
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt(("package-review-results", self.offset))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::CollapsingHeader::new("Selected files and inputs")
                            .id_salt("selected-review-files")
                            .default_open(page["total"] == 0 && page["operator_review_history"].as_array().is_none_or(Vec::is_empty))
                            .show(ui, |ui| {
                                for (path, m) in
                                    package["members"].as_object().into_iter().flatten()
                                {
                                    let r = s(m, "reference");
                                    ref_button(
                                        ui,
                                        r,
                                        format!("{path} · {}", version(r)),
                                        &mut inspect,
                                    );
                                }
                                for r in package["inputs"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_str)
                                {
                                    ref_button(ui, r, r, &mut inspect);
                                }
                            });
                        if let Some(history) = page["operator_review_history"]
                            .as_array()
                            .filter(|h| !h.is_empty())
                        {
                            egui::CollapsingHeader::new(format!(
                                "Operator decisions · {}",
                                history.len()
                            ))
                            .id_salt(("package-disposition", &self.selection.delivery))
                            .default_open(history.len() > 1)
                            .show(ui, |ui| {
                                for review in history.iter().rev() {
                                    ui.horizontal_wrapped(|ui| {
                                        super::edition_review::summary(ui, review);
                                        ref_button(
                                            ui,
                                            s(review, "reference"),
                                            "Decision record",
                                            &mut inspect,
                                        );
                                    });
                                    ui.label(s(review, "reason"));
                                    if let Some(basis) = review["reviewed_brief"].as_str() {
                                        ref_button(ui, basis, format!("Reviewed brief · {}", version(basis)), &mut inspect);
                                    }
                                    if let Some(head) = review["brief_at_review"].as_str().filter(|h| review["reviewed_brief"].as_str() != Some(*h)) {
                                        ref_button(ui, head, format!("Current when recorded · {}", version(head)), &mut inspect);
                                    }
                                }
                            });
                        }
                        let total = page["total"].as_u64().unwrap_or(0) as usize;
                        if total == 0 {
                            ui.weak("No published findings for this selection.");
                        } else {
                            let rows = page["items"].as_array().map(Vec::as_slice).unwrap_or_default();
                            let verified = rows.iter().filter(|r| r["review"].is_object()).count();
                            ui.small(format!("Verified on this page: {verified} · Total candidates: {total}"));
                        }
                        for entry in page["items"].as_array().into_iter().flatten() {
                            if let Some(reason) = entry["unavailable"].as_str() {
                                card(ui).show(ui, |ui| {
                                    ui.strong("Finding unavailable"); ui.label(reason);
                                    if ui.small_button("Check again").clicked() { load = Some(self.offset); }
                                });
                            } else {
                                review_card(ui, entry, package, status, &mut inspect, &mut save);
                            }
                        }
                        if self.offset > 0 || self.offset + PAGE < total {
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(self.offset > 0, egui::Button::new("Newer"))
                                    .clicked()
                                {
                                    load = Some(self.offset.saturating_sub(PAGE));
                                }
                                ui.label(format!(
                                    "{}–{} of {total}",
                                    self.offset + 1,
                                    total.min(self.offset + PAGE)
                                ));
                                if ui
                                    .add_enabled(
                                        self.offset + PAGE < total,
                                        egui::Button::new("Older"),
                                    )
                                    .clicked()
                                {
                                    load = Some(self.offset + PAGE);
                                }
                            });
                        }
                    });
            });
        self.visible = visible && inspect.is_none() && save.is_none();
        if let Some(offset) = load {
            self.load(ctx, root, offset);
        }
        if publish {
            if let Err(e) = self.publish(ctx, root, operator) {
                self.error = Some(e.to_string());
            }
        }
        save.map(Action::Save).or_else(|| inspect.map(Action::Inspect))
    }
}

/// Save one explicitly selected attachment. The worker rechecks publication,
/// filename and bytes; this button does not open or execute a submitted file.
pub(super) fn file_button(ui: &mut Ui, finding: &str, name: &str, object: &str, bytes: u64) -> Option<EvidenceFile> {
    ui.push_id((finding, name, object), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(excerpt(name, 54)).on_hover_text(name);
            ui.small(super::size(bytes)).on_hover_text(format!("{bytes} bytes"));
            // Two publications, or even two rows of one report, may use the
            // same filename and length for different observations.
            if let Some(digest) = object.strip_prefix("object:") {
                ui.small(digest.chars().take(12).collect::<String>()).on_hover_text(object);
            }
            ui.small_button("Save original…").on_hover_text(format!("Save {name}\n{object}"))
                .clicked().then(|| EvidenceFile {
                    finding: finding.into(), object: object.into(), name: name.into(),
                })
        }).inner
    }).inner
}

fn choose_subject(ui: &mut Ui, selection: &mut Selection, status: &Value) -> bool {
    let mut compare = false;
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut selection.proposed, false, "Issued delivery");
        ui.selectable_value(&mut selection.proposed, true, "Proposed files");
    });
    if selection.proposed {
        ui.add(
            egui::TextEdit::multiline(&mut selection.files)
                .desired_width(f32::INFINITY)
                .desired_rows(2)
                .hint_text("Exact asset references forming the proposed package"),
        );
        ui.add(
            egui::TextEdit::singleline(&mut selection.inputs)
                .desired_width(f32::INFINITY)
                .hint_text("Additional governing input references, if any"),
        );
    } else {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("review-delivery-list")
                .width(175.)
                .selected_text("Choose a delivery…")
                .show_ui(ui, |ui| {
                    for d in status["deliveries"].as_array().into_iter().flatten() {
                        let r = &d["reference"];
                        let handle = format!("delivery:{}@{}", s(r, "id"), r["revision"]);
                        if ui
                            .selectable_label(
                                selection.delivery == handle,
                                excerpt(s(&d["body"], "title"), 66),
                            )
                            .on_hover_text(&handle)
                            .clicked()
                        {
                            selection.delivery = handle;
                            compare = true;
                        }
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut selection.delivery)
                    .desired_width(280.)
                    .hint_text("Exact delivery reference"),
            );
        });
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Reporting activity");
        let works = status["work"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let selected = works
            .iter()
            .find(|w| w["body"]["id"] == selection.work)
            .map(|w| activity_title(&w["body"]))
            .unwrap_or("Choose an activity");
        egui::ComboBox::from_id_salt("package-review-work")
            .selected_text(excerpt(selected, 46))
            .show_ui(ui, |ui| {
                for w in works {
                    let b = &w["body"];
                    ui.selectable_value(
                        &mut selection.work,
                        s(b, "id").to_owned(),
                        excerpt(activity_title(b), 65),
                    )
                    .on_hover_text(activity_title(b));
                }
            });
    });
    ui.collapsing("Compare a specific review", |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut selection.specific)
                .desired_width(f32::INFINITY)
                .hint_text("Exact package_review reference"),
        );
    });
    compare
}

fn version(reference: &str) -> String {
    reference
        .rsplit_once('@')
        .map(|(_, v)| format!("v{v}"))
        .unwrap_or_else(|| reference.to_owned())
}
fn ref_button(
    ui: &mut Ui,
    reference: &str,
    label: impl Into<egui::WidgetText>,
    open: &mut Option<String>,
) {
    if ui.link(label).on_hover_text(reference).clicked() {
        *open = Some(reference.to_owned());
    }
}
fn review_card(ui: &mut Ui, item: &Value, selected: &Value, status: &Value, open: &mut Option<String>, save: &mut Option<EvidenceFile>) {
    let review = &item["review"];
    let reviewed = &review["subject"];
    let reference = s(review, "reference");
    card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let color = match s(review, "verdict") {
                "supported" => TEAL,
                "concern" => Color32::from_rgb(231, 165, 111),
                _ => theme::palette(ui).muted,
            };
            theme::badge(ui, &title(s(review, "verdict")), color);
            ui.strong(s(review, "criterion"));
        });
        let id = s(review, "author");
        let label = item["author_label_at_review"].as_str().unwrap_or(id);
        ui.horizontal_wrapped(|ui| {
            ui.strong(label).on_hover_text(format!("{} · {id}", s(review, "author_kind")));
            if let Some(brief) = review["reporting_mandate"].as_str() {
                ui.weak("published in");
                let activity = status["work"].as_array().into_iter().flatten()
                    .find(|w| w["body"]["id"] == review["work"])
                    .map(|w| activity_title(&w["body"])).unwrap_or("Reporting activity");
                ref_button(ui, brief, format!("{} · {}", excerpt(activity, 52), version(brief)), open);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if item["same_basis"] == true {
                ui.colored_label(TEAL, "Exact files, inputs and brief");
            } else {
                let mut differences = Vec::new();
                if item["same_files"] != true {
                    differences.push("files");
                }
                if item["same_inputs"] == false {
                    differences.push("governing inputs");
                }
                if item["same_mandate"] == false {
                    differences.push("brief");
                }
                if !differences.is_empty() {
                    ui.strong(format!("Changed {}", differences.join(" · ")));
                }
                if item["same_mandate"].is_null() {
                    ui.weak("Brief unrecorded");
                }
            }
            if item["available_at_delivery"] == false {
                ui.weak("Reviewed after delivery");
            }
            if item["listed_in_reviews"] == true {
                ui.weak("Named in the delivery");
            } else if item["cited_as_evidence"] == true {
                ui.weak("Cited as evidence");
            }
        });
        ui.label(s(review, "finding"));
        if let Some(files) = review["attachments"].as_array().filter(|v| !v.is_empty()) {
            ui.strong("Reviewer’s files · not part of the issued package");
            for f in files {
                if let Some(file) = file_button(ui, reference, s(f,"name"), s(f,"reference"), f["bytes"].as_u64().unwrap_or(0)) { *save = Some(file); }
            }
        }
        egui::CollapsingHeader::new("Compare constituent versions")
            .id_salt(("review-files", reference))
            .default_open(item["same_files"] == false || item["same_inputs"] == false)
            .show(ui, |ui| {
                egui::Grid::new(("review-file-grid", reference))
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("File");
                        ui.strong("Reviewed");
                        ui.strong("Selected");
                        ui.end_row();
                        for file in item["files"].as_array().into_iter().flatten() {
                            ui.label(RichText::new(excerpt(s(file, "path"), 53)).color(
                                if file["change"] == "same" {
                                    theme::palette(ui).text
                                } else {
                                    theme::ACCENT
                                },
                            ))
                            .on_hover_text(s(file, "path"));
                            for field in ["reviewed", "selected"] {
                                if let Some(r) = file[field]["reference"].as_str() {
                                    ref_button(ui, r, version(r), open);
                                } else {
                                    ui.weak("—");
                                }
                            }
                            ui.end_row();
                        }
                    });
                if item["same_inputs"] == false {
                    for (label, inputs) in [
                        ("Reviewed inputs", &reviewed["inputs"]),
                        ("Selected inputs", &selected["inputs"]),
                    ] {
                        ui.strong(label);
                        if inputs.as_array().is_none_or(Vec::is_empty) {
                            ui.weak("None declared");
                        }
                        for r in inputs
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            ref_button(ui, r, r, open);
                        }
                    }
                }
            });
        ui.horizontal_wrapped(|ui| {
            ref_button(ui, reference, "Review record", open);
            if let Some(b) = reviewed["mandate"].as_str() {
                ref_button(ui, b, format!("Reviewed brief · {}", version(b)), open);
            }
            if let Some(call) = review["decision"].as_str() {
                ref_button(
                    ui,
                    &format!("experience:{call}@1"),
                    "Reviewer’s decision",
                    open,
                );
            }
        });
        if review["evidence"].as_array().is_some_and(|a| !a.is_empty()) {
            egui::CollapsingHeader::new("Supporting evidence")
                .id_salt(("review-evidence", reference))
                .show(ui, |ui| {
                    for r in review["evidence"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        ref_button(ui, r, r, open);
                    }
                });
        }
    });
    ui.add_space(6.);
}
