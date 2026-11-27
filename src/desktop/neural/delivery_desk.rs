//! The issued package, its exact file revisions and the operator's review.
use super::*;

mod cam;
mod compare;
mod dispatch;
mod issued_document;
mod origins;
mod preview;
mod receiving;
mod requests;
mod updates;
use preview::{Format, Inline};
pub(super) use preview::ZipFile;

#[derive(Default)]
pub(super) struct Desk {
    package: String,
    generation: Option<u64>,
    model: Option<Arc<Package>>,
    selected: usize,
    inline: Inline,
    focus_preview: bool,
    document_trail: Vec<(String, String)>, // bounded path through this edition's documents
    slip: usize,
    slip_page: usize,
    show_references: bool,
    file_page: usize,
    file_folders: crate::desktop::folder_nav::Folders,
    request_page: usize,
    brief_page: usize,
    check_page: usize,
    updates: updates::Updates,
    origins: Option<origins::Window>,
    compare: Option<compare::Window>,
    lookup_return: Option<compare::Return>,
    cam: Option<cam::Window>,
    file_search: String,
    origin: Option<(String, ReturnTo)>,
}

#[derive(Clone)]
pub(super) struct ReturnTo {
    pub package: String,
    pub work: String,
    pub file: String,
}

impl Desk {
    pub(super) fn new(gl: Option<&Arc<eframe::glow::Context>>) -> Self {
        Self { inline: Inline::new(gl), ..Self::default() }
    }
    pub(super) fn close_overlay(&mut self) { self.cam = None; }
    pub(super) fn release(&mut self) { self.close_overlay(); self.inline.release(); }

    pub(super) fn selected_file(&self) -> Option<&str> {
        self.model.as_ref()?.files.get(self.selected).map(|f| f.reference.as_str())
    }

    pub(super) fn record_origin(
        &mut self,
        from_delivery: bool,
        package: &str,
        work: &str,
        file: &str,
    ) {
        self.set_return(
            file,
            from_delivery.then(|| ReturnTo {
                package: package.into(),
                work: work.into(),
                file: file.into(),
            }),
        );
    }

    pub(super) fn return_for(&self, inspected: &str) -> Option<ReturnTo> {
        self.origin
            .as_ref()
            .filter(|(file, _)| file == inspected)
            .map(|(_, target)| target.clone())
    }

    pub(super) fn set_return(&mut self, inspected: &str, target: Option<ReturnTo>) {
        self.lookup_return = None;
        self.origin = target.map(|target| (inspected.into(), target));
    }
}

struct File {
    reference: String,
    path: String,
    digest: Option<String>,
    bytes: Option<u64>,
    author: Option<String>,
    qualifications: Vec<String>,
    format: Format,
}

struct Package {
    body: Value,
    reference: String,
    files: Vec<File>,
    by_reference: BTreeMap<String, usize>,
    issued: Option<String>,
    requests: Vec<usize>,
    briefs: Vec<usize>,
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
        let files: Vec<_> = strings(&body["artifacts"])
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
                    digest: metadata["blob"].as_str().map(str::to_owned),
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
        let issued = app.indexed_issue(&reference, body)
            .and_then(|e| e["created_utc"].as_str())
            .map(str::to_owned);
        Self {
            body: body.clone(),
            requests: requests::using_edition(app, &reference),
            briefs: app.records("work").iter().enumerate()
                .filter_map(|(i, w)| (w["body"]["from_edition"] == reference).then_some(i)).collect(),
            reference,
            by_reference: files.iter().enumerate().map(|(i, f)| (f.reference.clone(), i)).collect(),
            files,
            issued,
        }
    }

    fn default_file(&self) -> usize {
        self.files
            .iter()
            .position(|f| f.format.visual() && f.bytes.is_some_and(|n| n <= preview::SOURCE_LIMIT))
            .or_else(|| self.files.iter().position(|f| f.format.archive() && f.bytes.is_some_and(|n| n <= preview::SOURCE_LIMIT)))
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

pub(super) fn scope(body: &Value) -> (&str, egui::Color32) {
    match s(body, "scope") {
        "partial" => ("Partial delivery", theme::ACCENT),
        "claimed_complete" => ("Author claims complete", TEAL),
        other if !other.is_empty() => (other, TEAL),
        _ => ("Scope not recorded", theme::ACCENT),
    }
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

#[derive(PartialEq)]
enum Jump { Files, Updates }

fn revision(reference: &str) -> Option<&str> {
    reference.rsplit_once('@').map(|(_, n)| n)
}

impl App {
    pub(super) fn issued_selection(&self, edition: String, file: &str) -> (String, String) {
        let path = self.snapshot.assets.iter().find(|a| a["reference"] == file)
            .and_then(|a| a["body"]["path"].as_str()).unwrap_or_default();
        (edition, path.into())
    }

    pub(super) fn open_package_file(&mut self, ctx: &egui::Context, edition: &str, file: &str) {
        if !file.starts_with("asset:") {
            self.error = Some(format!("Not a registered file revision of {edition}: {file}"));
            return;
        }
        let selection = self.issued_selection(edition.into(), file);
        self.open_named_resource(ctx, file.into(), Some(selection));
    }

    pub(super) fn delivery_return_link(&mut self, ui: &mut Ui) {
        let Some(target) = self.delivery_desk.return_for(&self.selected_reference) else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            let from_lookup = self.delivery_desk.lookup_return.is_some();
            if from_lookup && ui.link("← Back to member lookup")
                .on_hover_text("Check both issued ZIPs again and return to the same lookup").clicked() {
                if let Some(saved) = self.delivery_desk.lookup_return.take() {
                    saved.reopen(self, ui.ctx());
                }
            }
            let label = if from_lookup { "This member’s delivery ↗" } else { "← Back to delivery" };
            if ui.link(label).on_hover_text(&target.package).clicked() {
                self.delivery_desk.lookup_return = None;
                self.return_to_issued_file(&target);
            }
        });
    }

    pub(super) fn return_to_issued_file(&mut self, target: &ReturnTo) {
        self.open_home_delivery(&target.package, &target.work);
        self.refresh_delivery_model();
        let desk = &mut self.delivery_desk;
        if let Some(index) = desk
            .model
            .as_ref()
            .and_then(|p| p.files.iter().position(|f| f.reference == target.file))
        {
            desk.focus_preview |= desk.selected != index;
            desk.selected = index;
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
                desk.request_page = 0;
                desk.brief_page = 0;
                desk.check_page = 0;
                desk.updates = updates::Updates::default();
                desk.slip = 0;
                desk.slip_page = 0;
                desk.origins = None;
                desk.compare = None;
                desk.cam = None;
                desk.file_search.clear();
                desk.file_folders.directory.clear();
                desk.focus_preview = false;
                desk.document_trail.clear();
                desk.inline.clear();
            }
            desk.package.clone_from(&self.layout.delivery);
            desk.generation = Some(self.home_index.generation);
            desk.model = model;
        }
    }

    fn delivery_breadcrumb(&mut self, ui: &mut Ui, package: Option<&Package>, compact: bool) {
        ui.horizontal_wrapped(|ui| {
            let overview = if compact {
                "← Overview"
            } else {
                "← Workspace overview"
            };
            if ui.link(overview).clicked() {
                self.layout.editor = Editor::Home;
            }
            if let Some(p) = package {
                let work = s(&p.body, "work");
                let title = self.work_name(work);
                ui.weak("/");
                let label = if compact { "Activity".to_owned() } else {
                    home::short_text(&title, (ui.available_width() / 9.).clamp(14., 96.) as usize)
                };
                if ui.link(label).on_hover_text(format!("{title}\n{work}")).clicked() {
                    self.open_home_activity(work);
                }
                ui.weak("/");
                if ui.link("Deliveries").on_hover_text("Browse the editions in this activity").clicked() {
                    self.browse_editions();
                }
            }
        });
        ui.add_space(6.);
    }

    pub(super) fn home_package_page(&mut self, ui: &mut Ui) {
        self.finding_return_link(ui);
        self.refresh_delivery_model();
        let package = self.delivery_desk.model.clone();
        let compact = Self::constrained_activity_layout(ui.ctx());
        if !compact || package.is_none() {
            self.delivery_breadcrumb(ui, package.as_deref(), compact);
        }
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
                let jump = self.package_heading(ui, &p);
                if compact {
                    self.delivery_breadcrumb(ui, Some(&p), true);
                }
                if jump == Some(Jump::Updates) { ui.scroll_to_cursor(Some(egui::Align::Min)); }
                self.edition_updates(ui, &p);
                self.receiving_checks(ui, &p);
                self.dispatch_slips(ui, &p);
                self.package_human_requests(ui, &p);
                self.package_briefs(ui, &p);
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
                if jump == Some(Jump::Files) {
                    ui.scroll_to_cursor(Some(egui::Align::Min));
                }
                self.package_manifest(ui, &p);
                ui.add_space(14.);
                panel(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    heading(ui, "Messages about this edition");
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
        self.delivery_origins_window(ui.ctx(), &p);
        self.zip_comparison_window(ui.ctx(), &p);
        self.cam_overlay_window(ui.ctx(), &p);
    }

    fn package_heading(&mut self, ui: &mut Ui, p: &Package) -> Option<Jump> {
        let b = &p.body;
        let mut jump = None;
        let later = self.delivery_desk.updates.link_text(self.home_index.generation);
        let updates_link = |ui: &mut Ui| later.is_some_and(|label| ui.link(label)
            .on_hover_text("Inspect published findings and later editions that cite this issue").clicked());
        let colors = theme::palette(ui);
        let compact = Self::constrained_activity_layout(ui.ctx());
        let (scope, tint) = scope(b);
        egui::Frame::new()
            .fill(colors.header)
            .stroke(Stroke::new(1., colors.border))
            .corner_radius(9)
            .inner_margin(if compact { 5 } else { 15 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !compact {
                    ui.horizontal_wrapped(|ui| {
                        theme::glyph(ui, Icon::Files, colors.ink(persona_color(s(b, "owner"))));
                        eyebrow(ui, "Persona Studio  /  Delivery record");
                        theme::badge(ui, scope, tint);
                        if updates_link(ui) { jump = Some(Jump::Updates); }
                    });
                }
                let title = s(b, "title");
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(if compact {
                                17.
                            } else if ui.available_width() < 460. {
                                23.
                            } else {
                                27.
                            })
                            .strong(),
                    )
                    .wrap(),
                );
                if compact {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(tint, scope);
                        if updates_link(ui) { jump = Some(Jump::Updates); }
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Issuer · {}", self.label(s(b, "owner"))))
                        .on_hover_text(s(b, "owner"));
                    if let Some(rev) = revision(&p.reference) {
                        ui.weak(format!("·  Revision {rev}"))
                            .on_hover_text(&p.reference);
                    }
                    if let Some(utc) = &p.issued {
                        ui.weak(format!("·  {utc}"));
                    } else {
                        ui.colored_label(theme::ACCENT, "No matching public notice indexed");
                    }
                    if ui
                        .link(counted(p.files.len() as u64, "file", "files"))
                        .clicked()
                    {
                        jump = Some(Jump::Files);
                    }
                    if p.files.iter().any(|f| f.format.markdown()) {
                        ui.menu_button("Read document…", |ui| {
                            ui.set_max_width((ui.ctx().content_rect().width()-40.).clamp(170., 600.));
                            ui.small("Markdown files in this edition");
                            egui::ScrollArea::vertical().max_height(270.).show(ui, |ui| {
                                for (i, f) in p.files.iter().enumerate().filter(|(_, f)| f.format.markdown()) {
                                    if ui.add(egui::Button::selectable(false, &f.path).wrap()).on_hover_text(&f.reference).clicked() {
                                        self.delivery_desk.selected = i;
                                        self.delivery_desk.focus_preview = true;
                                        self.delivery_desk.document_trail.clear();
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                    if !p.files.is_empty()
                        && ui
                            .link("File origins…")
                            .on_hover_text("See where the files came from and which earlier deliveries contain the same revisions")
                            .clicked()
                    {
                        self.delivery_desk.origins = Some(origins::Window::default());
                    }
                    if p.files.iter().any(|f| f.format.archive()) && ui.link("Compare ZIPs…")
                        .on_hover_text("Compare the file bytes inside two ZIPs from issued editions").clicked() {
                        self.delivery_desk.compare = Some(compare::Window::new(self, p));
                    }
                });
                ui.add_space(5.);
                ui.horizontal_wrapped(|ui| {
                    // A persona may have packed the entire edition into one file.
                    // Let the recipient take those bytes directly from the heading.
                    let single = match p.files.as_slice() {
                        [file] => Some(file),
                        _ => None,
                    };
                    if let Some(file) = single {
                        let label = if file.format.archive() { "Save original ZIP…" } else { "Save original file…" };
                        if ui
                            .add_enabled(file.digest.is_some(), theme::primary(label))
                            .on_hover_text(format!("Save {} exactly as issued.", file.path))
                            .on_disabled_hover_text("This edition’s byte reference is unavailable.")
                            .clicked()
                        {
                            if let Some(digest) = &file.digest {
                                self.save_package_member(&p.reference, &file.reference, &file.path, digest, None);
                            }
                        }
                    }
                    if ui
                        .add(if single.is_some() {
                            egui::Button::new("Save handoff…")
                        } else {
                            theme::primary("Save handoff…")
                        })
                        .on_hover_text("Save the printable delivery sheet and its files together.")
                        .clicked()
                    {
                        self.save_delivery_handoff(&p.reference, title, p.files.len());
                    }
                    if p.files.len() > 1 && ui
                        .button("Save files ZIP…")
                        .on_hover_text("Collect all the issued files into a ZIP.")
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
                    if ui.button("New brief from edition…")
                        .on_hover_text("Start a separate activity with this exact edition as its source. Choose who to invite.")
                        .clicked()
                    {
                        self.dialog = Some(Dialog::from_edition(&p.reference, title, s(b, "owner")));
                    }
                });
            });
        jump
    }

    fn package_preview(&mut self, ui: &mut Ui, p: &Package) {
        enum Action {
            Open,
            History,
            Print(egui::Id),
            Save,
            SaveEntry(ZipFile),
            SaveViewed(artifacts::ViewedMember),
            InspectEntry(artifacts::MemberTarget),
            FollowDocumentPath(String, String),
            BackToDocument(usize),
            Measure(Option<String>),
            Overlay,
        }
        let desk = &mut self.delivery_desk;
        let mut action = None;
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
                        if Self::constrained_activity_layout(ui.ctx()) {
                            ui.set_max_width((ui.ctx().content_rect().width() - 18.).max(1.));
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        }
                        for (i, f) in p.files.iter().enumerate() {
                            ui.selectable_value(&mut desk.selected, i, &f.path)
                                .on_hover_text(&f.reference);
                        }
                    })
                    .response
                    .on_hover_text(&p.files[desk.selected].path);
                let file = &p.files[desk.selected];
                match desk.document_trail.last() {
                    Some((from, to)) if *to == file.reference => {
                        if let Some(&i) = p.by_reference.get(from) {
                            let link = ui.link("← Back to document").on_hover_text(&p.files[i].path);
                            if link.clicked() { action = Some(Action::BackToDocument(i)); }
                        }
                    }
                    _ => desk.document_trail.clear(),
                }
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
                    if file.format.archive() && ui.button("Overlay CAM layers…")
                        .on_hover_text("Look for Gerber and drill files in this issued ZIP. Nothing is uploaded.").clicked() {
                        action = Some(Action::Overlay);
                    }
                });
                ui.add_space(3.);
                if file.format.html() && !desk.inline.detected_pdf(&file.reference) {
                    let print = ui.add_enabled(file.digest.is_some(), egui::Button::new("Print preview"))
                        .on_hover_text("Lay out this issued HTML and its package files for printing");
                    if print.clicked() {
                        action = Some(Action::Print(print.id));
                    }
                    ui.add_space(3.);
                }
                if let Some((path, reference)) = desk.inline.show(ui, &self.root, &p.reference, file) {
                    action = Some(Action::FollowDocumentPath(path, reference));
                }
                ui.add_space(4.);
                ui.horizontal_wrapped(|ui| {
                    let inner = desk.inline.viewed_member();
                    let inside = !matches!(&inner, Ok(None));
                    if let Ok(Some(entry)) = inner {
                        if ui.link("Inspect member ↗").on_hover_text(entry.name()).clicked() {
                            action = Some(Action::InspectEntry(artifacts::MemberTarget::Viewed(entry.clone())));
                        }
                        if ui.link("Save member…").on_hover_text(entry.name()).clicked() {
                            action = Some(Action::SaveViewed(entry));
                        }
                    } else if let Err(reason) = inner {
                        ui.colored_label(ui.visuals().error_fg_color, reason);
                    } else if let Some(entry) = desk.inline.selected_member() {
                        if entry.path.to_ascii_lowercase().ends_with(".stl") && ui.link("Measure mesh…")
                            .on_hover_text("Measure this STL at its original path in the issued ZIP").clicked() {
                            action = Some(Action::Measure(Some(entry.path.clone())));
                        }
                        if ui.link("Inspect selected file ↗").clicked() {
                            action = Some(Action::InspectEntry(artifacts::MemberTarget::Zip(entry.index, entry.path.clone())));
                        }
                        if ui.add_enabled(file.digest.is_some(), egui::Link::new("Save selected file…"))
                            .on_hover_text(format!("Save {} from the issued ZIP", entry.path)).clicked() {
                            action = Some(Action::SaveEntry(entry.clone()));
                        }
                    } else if file.path.to_ascii_lowercase().ends_with(".stl") && ui.link("Measure mesh…").clicked() {
                        action = Some(Action::Measure(None));
                    }
                    if ui
                        .link(if inside { "Open original archive ↗" } else if desk.inline.selected_member().is_some() { "Open full ZIP ↗" } else { "Open full file ↗" })
                        .on_hover_text(format!("{}\n{}", file.path, file.reference))
                        .clicked()
                    {
                        action = Some(Action::Open);
                    }
                    if ui
                        .link("File history…")
                        .on_hover_text(&file.reference)
                        .clicked()
                    {
                        action = Some(Action::History);
                    }
                    if ui
                        .add_enabled(file.digest.is_some(), egui::Link::new(if file.format.archive() { "Save this ZIP unchanged…" } else if inside { "Save original archive…" } else { "Save this file…" }))
                        .on_hover_text(format!("Save {} from this delivery", file.path))
                        .on_disabled_hover_text("This edition’s byte reference is unavailable.")
                        .clicked()
                    {
                        action = Some(Action::Save);
                    }
                });
            })
            .response;
        if std::mem::take(&mut desk.focus_preview) {
            response.scroll_to_me(Some(egui::Align::Min));
        }
        let file = &p.files[desk.selected];
        match action {
            Some(Action::BackToDocument(index)) => {
                desk.document_trail.pop();
                desk.selected = index;
                desk.focus_preview = true;
                ui.ctx().request_repaint();
            }
            Some(Action::FollowDocumentPath(path, target)) => {
                // Both the path and revision have to match the verified list.
                // The preview worker verifies the target itself before display.
                if let Some(&i) = p
                    .by_reference
                    .get(&target)
                    .filter(|&&i| p.files[i].path == path)
                {
                    if desk.document_trail.len() == 32 {
                        desk.document_trail.remove(0);
                    }
                    desk.document_trail.push((file.reference.clone(), target));
                    desk.selected = i;
                    desk.focus_preview = true;
                    ui.ctx().request_repaint();
                } else {
                    self.error = Some(
                        "This file no longer matches the selected edition. Reopen the delivery."
                            .into(),
                    );
                }
            }
            Some(Action::Overlay) => {
                desk.cam = Some(cam::Window::new(ui.ctx(), &self.root, p, file));
            }
            Some(Action::Measure(member)) => self.mesh_profile.open(
                ui.ctx(), &self.root, &self.operator, &file.reference, s(&p.body, "work"), member, Some(p.reference.clone()),
            ),
            Some(Action::InspectEntry(entry)) => {
                self.open_package_file(ui.ctx(), &p.reference, &file.reference);
                self.preview_member = Some(entry);
            }
            Some(Action::Open) => self.open_package_file(ui.ctx(), &p.reference, &file.reference),
            Some(Action::History) => self.open_file_history(ui.ctx(), &file.reference),
            Some(Action::Print(id)) => self.open_print_preview(
                ui.ctx(), &file.reference, &file.path, Some(p.reference.clone()), Some(id),
            ),
            Some(Action::Save) => {
                if let Some(digest) = &file.digest {
                    self.save_package_member(&p.reference, &file.reference, &file.path, digest, None);
                }
            }
            Some(Action::SaveViewed(entry)) => {
                if let Some(digest) = &file.digest {
                    self.save_viewed_member(file.reference.clone(), Some((p.reference.clone(), file.path.clone())), digest.clone(), entry);
                }
            }
            Some(Action::SaveEntry(entry)) => {
                if let Some(digest) = &file.digest {
                    self.save_package_member(&p.reference, &file.reference, &file.path, digest, Some(entry));
                }
            }
            None => {}
        }
    }

    fn package_manifest(&mut self, ui: &mut Ui, p: &Package) {
        const PAGE: usize = 8;
        let mut open = None;
        let mut print = None;
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
                if ui
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
            use ai_personas::library_folders::{self, Entry};
            let folders = &mut self.delivery_desk.file_folders;
            if folders.location(ui, "Issued folders", !query.is_empty()) {
                self.delivery_desk.file_page = 0;
            }
            let matching = library_folders::entries(query.is_empty().then_some(&folders.directory),
                p.files.iter().enumerate().filter(|(_, f)| query.is_empty()
                    || f.path.to_lowercase().contains(&query) || f.reference.to_lowercase().contains(&query))
                    .map(|(i, f)| (i, f.path.as_str())));
            let last = matching.len().saturating_sub(1) / PAGE;
            self.delivery_desk.file_page = self.delivery_desk.file_page.min(last);
            let start = self.delivery_desk.file_page * PAGE;
            let end = (start + PAGE).min(matching.len());
            for row in &matching[start..end] {
                let index = match row {
                    Entry::Folder { name, files } => {
                        if self.delivery_desk.file_folders.open_button(ui, name, *files) {
                            self.delivery_desk.file_page = 0;
                        }
                        continue;
                    }
                    Entry::File(i) => *i,
                };
                let file = &p.files[index];
                let name = if query.is_empty() { library_folders::split(&file.path).1 } else { &file.path };
                ui.add_space(4.);
                ui.separator();
                ui.add_space(3.);
                ui.push_id(("package-file", &file.reference), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        theme::glyph(ui, Icon::Files, theme::palette(ui).muted);
                        if ui
                            .link(
                                RichText::new(name)
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
                        if file.bytes.is_some_and(|n| n <= preview::SOURCE_LIMIT)
                            && ui.small_button("Preview here").clicked()
                        {
                            self.delivery_desk.selected = index;
                            self.delivery_desk.focus_preview = true;
                            ui.ctx().request_repaint();
                        }
                        if file.format.html() && file.digest.is_some() {
                            let button = ui.small_button("Print preview").on_hover_text(&file.path);
                            if button.clicked() {
                                self.delivery_desk.selected = index;
                                self.delivery_desk.focus_preview = true;
                                print = Some((file, button.id));
                            }
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
                            egui::Button::new("Previous").small(),
                        )
                        .clicked()
                    {
                        self.delivery_desk.file_page -= 1;
                    }
                    if ui
                        .add_enabled(
                            self.delivery_desk.file_page < last,
                            egui::Button::new("Next").small(),
                        )
                        .clicked()
                    {
                        self.delivery_desk.file_page += 1;
                    }
                });
            }
        });
        if let Some(r) = open {
            self.open_package_file(ui.ctx(), &p.reference, &r);
        } else if let Some((file, id)) = print {
            self.open_print_preview(ui.ctx(), &file.reference, &file.path, Some(p.reference.clone()), Some(id));
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
        heading(ui, "Open at issue");
        ui.weak(number(items.len() as u64));
    });
    if items.is_empty() {
        ui.weak("The issuer listed no open items.");
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
