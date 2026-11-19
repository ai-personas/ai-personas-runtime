//! The issued package, its exact file revisions and the operator's review.
use super::*;

mod preview;
use preview::{Format, Inline};

#[derive(Default)]
pub(super) struct Desk {
    package: String,
    generation: Option<u64>,
    model: Option<Arc<Package>>,
    selected: usize,
    inline: Inline,
    focus_preview: bool,
    show_references: bool,
    file_page: usize,
    file_search: String,
    origin: Option<(String, String, String)>,
}

impl Desk {
    pub(super) fn record_origin(
        &mut self,
        from_delivery: bool,
        package: &str,
        work: &str,
        file: &str,
    ) {
        self.origin = from_delivery.then(|| (package.into(), work.into(), file.into()));
    }
}

struct File {
    reference: String,
    path: String,
    bytes: Option<u64>,
    author: Option<String>,
    qualifications: Vec<String>,
    format: Format,
}

struct Package {
    body: Value,
    reference: String,
    files: Vec<File>,
    issued: Option<String>,
}

impl Package {
    fn load(app: &App, record: &Value) -> Self {
        let reference = inbox::handle(record);
        let body = &record["body"];
        let assets: BTreeMap<_, _> = app
            .snapshot
            .assets
            .iter()
            .map(|a| (s(a, "reference"), a))
            .collect();
        let files = strings(&body["artifacts"])
            .map(|reference| {
                let asset = assets.get(reference).copied().unwrap_or(&Value::Null);
                let metadata = &asset["body"];
                let path = metadata["path"]
                    .as_str()
                    .filter(|p| !p.is_empty())
                    .unwrap_or(reference);
                File {
                    reference: reference.into(),
                    path: path.into(),
                    bytes: asset["size"].as_u64(),
                    author: metadata["last_content_author"]
                        .as_str()
                        .or_else(|| metadata["owner"].as_str())
                        .map(str::to_owned),
                    qualifications: strings(&metadata["qualifications"])
                        .map(str::to_owned)
                        .collect(),
                    format: Format::identify(path, s(asset, "media")),
                }
            })
            .collect();
        let issued = app
            .events
            .iter()
            .rev()
            .find(|e| e["kind"] == "delivery" && e["body"]["reference"] == reference)
            .and_then(|e| e["created_utc"].as_str())
            .map(str::to_owned);
        Self {
            body: body.clone(),
            reference,
            files,
            issued,
        }
    }

    fn default_file(&self) -> usize {
        self.files
            .iter()
            .position(|f| f.format.visual() && f.bytes.is_some_and(|n| n <= preview::SOURCE_LIMIT))
            .or_else(|| {
                self.files.iter().position(|f| {
                    f.format.inline() && f.bytes.is_some_and(|n| n <= preview::SOURCE_LIMIT)
                })
            })
            .unwrap_or(0)
    }
}

fn strings(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(18.).strong());
}

fn panel(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1., p.border))
        .inner_margin(13)
        .corner_radius(8)
}

fn revision(reference: &str) -> Option<&str> {
    reference.rsplit_once('@').map(|(_, n)| n)
}

impl App {
    pub(super) fn delivery_return_link(&mut self, ui: &mut Ui) {
        let Some((package, work, file)) = self
            .delivery_desk
            .origin
            .as_ref()
            .filter(|(_, _, resource)| resource == &self.selected_reference)
        else {
            return;
        };
        if ui
            .link("← Back to delivery")
            .on_hover_text(package)
            .clicked()
        {
            let (package, work, file) = (package.clone(), work.clone(), file.clone());
            self.open_home_delivery(&package, &work);
            self.refresh_delivery_model();
            let desk = &mut self.delivery_desk;
            if let Some(index) = desk
                .model
                .as_ref()
                .and_then(|p| p.files.iter().position(|f| f.reference == file))
            {
                desk.focus_preview |= desk.selected != index;
                desk.selected = index;
            }
        }
    }

    fn refresh_delivery_model(&mut self) {
        if self.delivery_desk.package != self.layout.delivery
            || self.delivery_desk.generation != Some(self.home_index.generation)
        {
            let model = self
                .selected_home_package()
                .map(|p| Arc::new(Package::load(self, p)));
            let desk = &mut self.delivery_desk;
            if desk.package != self.layout.delivery || desk.model.is_none() {
                desk.selected = model.as_ref().map_or(0, |p| p.default_file());
                desk.file_page = 0;
                desk.file_search.clear();
                desk.focus_preview = false;
                desk.inline.clear();
            }
            desk.package.clone_from(&self.layout.delivery);
            desk.generation = Some(self.home_index.generation);
            desk.model = model;
        }
    }

    pub(super) fn home_package_page(&mut self, ui: &mut Ui) {
        self.refresh_delivery_model();
        let package = self.delivery_desk.model.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.link("← Workspace overview").clicked() {
                self.layout.editor = Editor::Home;
            }
            if let Some(p) = &package {
                let work = s(&p.body, "work");
                ui.weak("/");
                if ui.link(self.work_name(work)).on_hover_text(work).clicked() {
                    self.open_home_activity(work);
                }
            }
        });
        ui.add_space(6.);
        let Some(p) = package else {
            if self.snapshot.status.is_null() {
                if self.error.is_none() {
                    ui.spinner();
                }
                ui.label(if self.error.is_some() {
                    "Workspace data is unavailable."
                } else {
                    "Opening delivery…"
                });
            } else {
                ui.label("This delivery revision is unavailable in the workspace.");
            }
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt(("issued-delivery", &p.reference))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let files_jump = self.package_heading(ui, &p);
                ui.add_space(12.);
                if ui.available_width() >= 820. && !p.files.is_empty() {
                    ui.columns(2, |columns| {
                        columns[0].vertical(|ui| self.package_preview(ui, &p));
                        columns[1].vertical(|ui| {
                            panel(ui).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                supported_use(ui, &p);
                                ui.add_space(12.);
                                remaining_work(ui, &p);
                            });
                        });
                    });
                } else {
                    if !p.files.is_empty() {
                        self.package_preview(ui, &p);
                        ui.add_space(12.);
                    }
                    supported_use(ui, &p);
                    ui.add_space(10.);
                    remaining_work(ui, &p);
                }
                ui.add_space(14.);
                if files_jump {
                    ui.scroll_to_cursor(Some(egui::Align::Min));
                }
                self.package_manifest(ui, &p);
                ui.add_space(14.);
                panel(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    heading(ui, "Feedback on this edition");
                    self.delivery_followup_summary(ui, &p.reference);
                });
                ui.add_space(8.);
                panel(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    heading(ui, "Operator review");
                    self.review_controls(ui, &p.reference, s(&p.body, "title"));
                });
                ui.add_space(10.);
                self.package_sources(ui, &p);
                ui.add_space(12.);
            });
    }

    fn package_heading(&mut self, ui: &mut Ui, p: &Package) -> bool {
        let b = &p.body;
        let mut files_jump = false;
        let colors = theme::palette(ui);
        egui::Frame::new()
            .fill(colors.header)
            .stroke(Stroke::new(1., colors.border))
            .corner_radius(9)
            .inner_margin(15)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    eyebrow(ui, "Delivery package");
                    let (scope, tint) = match s(b, "scope") {
                        "partial" => ("Partial delivery", theme::ACCENT),
                        "claimed_complete" => ("Author claims complete", TEAL),
                        other if !other.is_empty() => (other, TEAL),
                        _ => ("Issued", TEAL),
                    };
                    theme::badge(ui, scope, tint);
                });
                let title = s(b, "title");
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(if ui.available_width() < 460. {
                                23.
                            } else {
                                27.
                            })
                            .strong(),
                    )
                    .wrap(),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Issued by {}", self.label(s(b, "owner"))))
                        .on_hover_text(s(b, "owner"));
                    if let Some(rev) = revision(&p.reference) {
                        ui.weak(format!("·  Revision {rev}"))
                            .on_hover_text(&p.reference);
                    }
                    if let Some(utc) = &p.issued {
                        ui.weak(format!("·  {utc}"));
                    }
                    if ui
                        .link(counted(p.files.len() as u64, "file", "files"))
                        .clicked()
                    {
                        files_jump = true;
                    }
                });
                ui.add_space(5.);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(theme::primary("Save handoff…"))
                        .on_hover_text("Save the printable delivery sheet and its files together.")
                        .clicked()
                    {
                        self.save_delivery_handoff(&p.reference, title, p.files.len());
                    }
                    if ui
                        .button("Save files ZIP…")
                        .on_hover_text("Save the files in this exact package.")
                        .clicked()
                    {
                        self.export_package(&p.reference, title);
                    }
                    if ui
                        .button("Send feedback…")
                        .on_hover_text(format!(
                            "Write to {} about this delivery revision.",
                            self.label(s(b, "owner"))
                        ))
                        .clicked()
                    {
                        self.open_delivery_feedback(
                            &p.reference,
                            s(b, "work"),
                            s(b, "owner"),
                            title,
                        );
                    }
                });
            });
        files_jump
    }

    fn package_preview(&mut self, ui: &mut Ui, p: &Package) {
        let desk = &mut self.delivery_desk;
        let mut open = None;
        let response = panel(ui)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                heading(ui, "File preview");
                desk.selected = desk.selected.min(p.files.len() - 1);
                egui::ComboBox::from_id_salt(("issued-file-select", &p.reference))
                    .selected_text(&p.files[desk.selected].path)
                    .width(ui.available_width())
                    .truncate()
                    .show_ui(ui, |ui| {
                        for (i, f) in p.files.iter().enumerate() {
                            ui.selectable_value(&mut desk.selected, i, &f.path)
                                .on_hover_text(&f.reference);
                        }
                    });
                let file = &p.files[desk.selected];
                ui.horizontal_wrapped(|ui| {
                    ui.small(if desk.inline.detected_pdf(&file.reference) {
                        "PDF document"
                    } else {
                        &file.format.label
                    });
                    if let Some(rev) = revision(&file.reference) {
                        ui.small(format!("·  Revision {rev}"));
                    }
                    if let Some(n) = file.bytes {
                        ui.small(format!("·  {}", size(n)))
                            .on_hover_text(counted(n, "byte", "bytes"));
                    }
                });
                ui.add_space(3.);
                desk.inline.show(ui, &self.root, file);
                ui.add_space(4.);
                if ui
                    .link("Open full file ↗")
                    .on_hover_text(format!("{}\n{}", file.path, file.reference))
                    .clicked()
                {
                    open = Some(file.reference.clone());
                }
            })
            .response;
        if std::mem::take(&mut desk.focus_preview) {
            response.scroll_to_me(Some(egui::Align::Min));
        }
        if let Some(reference) = open {
            self.open_resource(ui.ctx(), reference);
        }
    }

    fn package_manifest(&mut self, ui: &mut Ui, p: &Package) {
        const PAGE: usize = 8;
        let mut open = None;
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                heading(ui, "Package files");
                ui.weak(number(p.files.len() as u64));
            });
            if p.files.is_empty() {
                ui.weak("No files are listed in this delivery.");
                return;
            }
            ui.horizontal_wrapped(|ui| {
                if p.files.len() > PAGE
                    && ui
                        .add(
                            egui::TextEdit::singleline(&mut self.delivery_desk.file_search)
                                .hint_text("Find a file in this package…")
                                .desired_width(ui.available_width().min(220.)),
                        )
                        .changed()
                {
                    self.delivery_desk.file_page = 0;
                }
                ui.checkbox(
                    &mut self.delivery_desk.show_references,
                    "Show exact references",
                );
            });
            let query = self.delivery_desk.file_search.trim().to_lowercase();
            let matching: Vec<_> = p
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    query.is_empty()
                        || f.path.to_lowercase().contains(&query)
                        || f.reference.to_lowercase().contains(&query)
                })
                .collect();
            let last = matching.len().saturating_sub(1) / PAGE;
            self.delivery_desk.file_page = self.delivery_desk.file_page.min(last);
            let start = self.delivery_desk.file_page * PAGE;
            let end = (start + PAGE).min(matching.len());
            for &(index, file) in &matching[start..end] {
                ui.add_space(4.);
                ui.separator();
                ui.add_space(3.);
                ui.push_id(("package-file", &file.reference), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                        if ui
                            .link(
                                RichText::new(&file.path)
                                    .color(theme::palette(ui).accent)
                                    .strong(),
                            )
                            .on_hover_text(&file.reference)
                            .clicked()
                        {
                            open = Some(file.reference.clone());
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if let Some(n) = file.bytes {
                            ui.small(counted(n, "byte", "bytes"));
                        } else {
                            ui.small("Size unavailable");
                        }
                        if let Some(rev) = revision(&file.reference) {
                            ui.small(format!("·  Revision {rev}"));
                        }
                        if let Some(author) = &file.author {
                            ui.small(format!("·  Content by {}", self.label(author)))
                                .on_hover_text(author);
                        } else {
                            ui.small("·  Content author unrecorded");
                        }
                        if (file.format.inline()
                            || self.delivery_desk.inline.detected_pdf(&file.reference))
                            && ui.small_button("Preview here").clicked()
                        {
                            self.delivery_desk.selected = index;
                            self.delivery_desk.focus_preview = true;
                            ui.ctx().request_repaint();
                        }
                    });
                    if file.qualifications.is_empty() {
                        ui.weak("No qualifications recorded.");
                    } else {
                        ui.small(if file.qualifications.len() == 1 {
                            "Qualification"
                        } else {
                            "Qualifications"
                        });
                        for item in &file.qualifications {
                            ui.add(egui::Label::new(format!("•  {item}")).wrap());
                        }
                    }
                    if self.delivery_desk.show_references {
                        exact_ref(ui, &file.reference);
                    }
                });
            }
            if matching.is_empty() {
                ui.weak("No files match this search.");
            }
            if last > 0 {
                ui.add_space(7.);
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!("{}–{} of {}", start + 1, end, matching.len()));
                    if ui
                        .add_enabled(
                            self.delivery_desk.file_page > 0,
                            egui::Button::new("Previous files").small(),
                        )
                        .clicked()
                    {
                        self.delivery_desk.file_page -= 1;
                    }
                    if ui
                        .add_enabled(
                            self.delivery_desk.file_page < last,
                            egui::Button::new("Next files").small(),
                        )
                        .clicked()
                    {
                        self.delivery_desk.file_page += 1;
                    }
                });
            }
        });
        if let Some(r) = open {
            self.open_resource(ui.ctx(), r);
        }
    }

    fn package_sources(&mut self, ui: &mut Ui, p: &Package) {
        let mut open = None;
        egui::CollapsingHeader::new("Evidence and package record")
            .id_salt(("delivery-sources", &p.reference))
            .show(ui, |ui| {
                ui.strong("Issued package");
                exact_ref(ui, &p.reference);
                if ui.link("Open package record ↗").clicked() {
                    open = Some(p.reference.clone());
                }
                for (field, label) in [
                    ("mandate", "Brief at issue"),
                    ("evidence", "Declared evidence"),
                    ("review_inputs", "Review inputs"),
                    ("reviews", "Cited reviews"),
                ] {
                    let value = &p.body[field];
                    let refs: Vec<_> = value.as_str().into_iter().chain(strings(value)).collect();
                    if refs.is_empty() {
                        continue;
                    }
                    ui.add_space(4.);
                    ui.strong(label);
                    for r in refs {
                        let text = self
                            .snapshot
                            .assets
                            .iter()
                            .find(|f| f["reference"] == r)
                            .and_then(|f| f["body"]["path"].as_str())
                            .unwrap_or(r);
                        if ui.link(text).on_hover_text(r).clicked() {
                            open = Some(r.to_owned());
                        }
                    }
                }
            });
        if let Some(r) = open {
            self.open_resource(ui.ctx(), r);
        }
    }
}

fn supported_use(ui: &mut Ui, p: &Package) {
    heading(ui, "Supported use");
    let use_text = s(&p.body, "supported_use");
    if use_text.is_empty() {
        ui.weak("No supported use was recorded.");
    } else {
        ui.add(egui::Label::new(use_text).wrap());
    }
}

fn remaining_work(ui: &mut Ui, p: &Package) {
    let items = strings(&p.body["remaining"]).collect::<Vec<_>>();
    ui.horizontal_wrapped(|ui| {
        heading(ui, "Remaining work");
        ui.weak(number(items.len() as u64));
    });
    if items.is_empty() {
        ui.weak("The author listed no remaining work.");
    }
    for item in items {
        ui.add(egui::Label::new(format!("•  {item}")).wrap());
    }
}

fn exact_ref(ui: &mut Ui, reference: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Label::new(RichText::new(reference).monospace().small()).wrap());
        if ui.small_button("Copy reference").clicked() {
            ui.ctx().copy_text(reference.into());
        }
    });
}
