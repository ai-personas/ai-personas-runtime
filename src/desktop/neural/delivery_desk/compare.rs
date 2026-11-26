//! Compare the actual member bytes of two chosen, issued ZIPs. Opening this
//! window does not run a check on behalf of a persona or record a package review.
mod locations;
use super::*;
use ai_personas::neural::{
    issued_files::Source,
    issued_zip::{self, Comparison, Difference, Locations, MemberAddress, Side},
    tools::DeliveryScope,
};
use std::sync::OnceLock;

const PAGE: usize = 5;
struct Choice {
    edition: String,
    file: String,
    title: String,
    path: String,
}
pub(super) struct Window {
    root: PathBuf,
    options: Vec<Choice>,
    target: Option<usize>,
    baseline: Option<usize>,
    query: String,
    choose_page: usize,
    compare: Option<Arc<OnceLock<Result<Comparison, String>>>>,
    member_query: String,
    locations: Option<Result<Locations, String>>,
    pending_lookup: Option<(MemberAddress, usize)>,
    open_member: Option<(String, String, artifacts::MemberTarget)>,
    scroll_top: bool,
    baseline_root: String,
    selected_root: String,
    differences_only: bool,
    member_page: usize,
    counts: Option<[usize; 4]>,
    visible_members: Vec<usize>,
}
impl Window {
    pub(super) fn new(app: &App, p: &Package) -> Self {
        let assets: BTreeMap<_, _> = app
            .snapshot
            .assets
            .iter()
            .map(|a| (s(a, "reference"), a))
            .collect();
        let mut options = Vec::new();
        for record in app.records("deliveries") {
            let edition = inbox::handle(record);
            for file in strings(&record["body"]["artifacts"]) {
                let Some(asset) = assets.get(file) else {
                    continue;
                };
                let path = s(&asset["body"], "path");
                if Format::identify(path, s(asset, "media")).archive() {
                    options.push(Choice {
                        edition: edition.clone(),
                        file: file.into(),
                        path: path.into(),
                        title: s(&record["body"], "title").into(),
                    });
                }
            }
        }
        options.sort_by(|a, b| {
            (&a.title, &a.path, &a.edition, &a.file).cmp(&(&b.title, &b.path, &b.edition, &b.file))
        });
        let mut here = options
            .iter()
            .enumerate()
            .filter(|(_, c)| c.edition == p.reference);
        let target = match (here.next(), here.next()) {
            (Some((i, _)), None) => Some(i),
            _ => options.iter().position(|c| {
                c.edition == p.reference
                    && Some(c.file.as_str()) == app.delivery_desk.selected_file()
            }),
        };
        Self {
            root: app.root.clone(),
            options,
            target,
            baseline: None,
            query: String::new(),
            choose_page: 0,
            compare: None,
            member_query: String::new(),
            locations: None,
            pending_lookup: None,
            open_member: None,
            scroll_top: false,
            baseline_root: String::new(),
            selected_root: String::new(),
            differences_only: true,
            member_page: 0,
            counts: None,
            visible_members: Vec::new(),
        }
    }

    fn choose(&mut self, ui: &mut Ui, p: &Package) {
        eyebrow(ui, "Selected edition");
        ui.add(egui::Label::new(RichText::new(s(&p.body, "title")).strong()).wrap());
        let target = self.target;
        egui::ComboBox::from_id_salt("comparison-target-file")
            .selected_text(
                target.map_or("Choose an issued ZIP…", |i| self.options[i].path.as_str()),
            )
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                for (i, c) in self
                    .options
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.edition == p.reference)
                {
                    ui.selectable_value(&mut self.target, Some(i), &c.path)
                        .on_hover_text(&c.file);
                }
            });
        if self.target != target {
            self.baseline = None;
            self.choose_page = 0;
        }
        let Some(t) = self.target else {
            return;
        };
        ui.add_space(8.);
        eyebrow(ui, "Choose a baseline");
        ui.label("Pick the issued ZIP to compare against. Neither package will be changed.");
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Find an edition, ZIP path or reference…")
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            self.choose_page = 0;
            self.baseline = None;
        }
        let query = self.query.trim().to_lowercase();
        let choices = self
            .options
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                *i != t
                    && [&c.title, &c.path, &c.edition, &c.file]
                        .iter()
                        .any(|s| s.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        if choices.is_empty() {
            ui.weak("No other issued ZIP matches this selection.");
        }
        self.choose_page = self.choose_page.min(choices.len().saturating_sub(1) / PAGE);
        for &(i, c) in choices.iter().skip(self.choose_page * PAGE).take(PAGE) {
            ui.push_id((&c.edition, &c.file), |ui| {
                if ui
                    .add(egui::Button::selectable(self.baseline == Some(i), &c.title).wrap())
                    .on_hover_text(&c.edition)
                    .clicked()
                {
                    self.baseline = Some(i);
                }
                ui.add(egui::Label::new(RichText::new(&c.path).small()).wrap())
                    .on_hover_text(&c.file);
            });
        }
        home::pager(ui, &mut self.choose_page, choices.len(), PAGE, "ZIPs");
        ui.add_space(6.);
        if let Some(b) = self.baseline {
            ui.small(format!(
                "Baseline: {} · {}",
                self.options[b].title, self.options[b].path
            ));
        }
        if ui
            .add_enabled(
                self.baseline.is_some(),
                theme::primary("Compare member bytes"),
            )
            .clicked()
        {
            self.begin(ui.ctx(), None);
        }
    }
    fn begin(&mut self, ctx: &egui::Context, address: Option<(MemberAddress, usize)>) {
        let (Some(b), Some(t)) = (self.baseline, self.target) else {
            return;
        };
        let (b, t) = (&self.options[b], &self.options[t]);
        let (be, bf, te, tf) = (
            b.edition.clone(),
            b.file.clone(),
            t.edition.clone(),
            t.file.clone(),
        );
        let (br, tr) = (self.baseline_root.clone(), self.selected_root.clone());
        let (ctx, root) = (ctx.clone(), self.root.clone());
        let state = Arc::new(OnceLock::new());
        self.compare = Some(state.clone());
        self.locations = None;
        self.pending_lookup = address;
        self.scroll_top = true;
        self.counts = None;
        self.member_page = 0;
        self.visible_members.clear();
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| issued_zip::compare(&db, &be, &bf, &te, &tf, &br, &tr));
            let _ = state.set(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }
    fn root_choice(ui: &mut Ui, side: &str, enclosing: &str, choice: &mut String) {
        ui.small(side);
        egui::ComboBox::from_id_salt(("enclosing-folder", side))
            .selected_text(if choice.is_empty() {
                "Original paths"
            } else {
                choice.as_str()
            })
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                for prefix in std::iter::once("")
                    .chain(enclosing.match_indices('/').map(|(n, _)| &enclosing[..=n]))
                {
                    if ui
                        .selectable_label(
                            choice == prefix,
                            if prefix.is_empty() {
                                "Original paths"
                            } else {
                                prefix
                            },
                        )
                        .clicked()
                    {
                        choice.clear();
                        choice.push_str(prefix);
                    }
                }
            });
    }
    fn terms(app: &App, ui: &mut Ui, source: &Source) {
        let issue = &source.at_issue;
        let scope = match issue.scope {
            DeliveryScope::Partial => "Partial edition",
            DeliveryScope::ClaimedComplete => "Author claims complete",
        };
        ui.collapsing(format!("{scope} · Use & limits"), |ui| {
            ui.strong(format!("Issued by {}", app.label(&issue.issuer)));
            ui.add(
                egui::Label::new(&issue.supported_use)
                    .wrap()
                    .selectable(true),
            );
            if !issue.remaining.is_empty() {
                ui.small("Still open at issue");
                for item in &issue.remaining {
                    ui.add(
                        egui::Label::new(format!("• {item}"))
                            .wrap()
                            .selectable(true),
                    );
                }
            }
            let file = &source.registration;
            ui.separator();
            ui.strong(format!("File notes · {}", app.label(&file.owner)));
            ui.small("Recorded on this file revision, not a later review.");
            if file.retired {
                ui.label("Retired in this revision");
            }
            if let (Some(by), Some(reason)) = (&file.organized_by, &file.organization_reason) {
                ui.add(
                    egui::Label::new(format!("Organization note · {}: {reason}", app.label(by)))
                        .wrap()
                        .selectable(true),
                );
            }
            if file.qualifications.is_empty() {
                ui.weak("No qualifications recorded on this revision.");
            }
            for item in &file.qualifications {
                ui.add(
                    egui::Label::new(format!("• {item}"))
                        .wrap()
                        .selectable(true),
                );
            }
        });
    }
    fn result(&mut self, app: &App, ui: &mut Ui, r: &Comparison) {
        for (label, source) in [("Baseline", &r.baseline), ("Selected", &r.selected)] {
            ui.push_id(label, |ui| {
                eyebrow(ui, label);
                ui.add(egui::Label::new(RichText::new(&source.title).strong()).wrap())
                    .on_hover_text(&source.edition);
                ui.add(egui::Label::new(&source.path).wrap())
                    .on_hover_text(&source.file);
                Self::terms(app, ui, source);
            });
        }
        ui.weak(if r.baseline.sha256 == r.selected.sha256 {
            "The ZIP files themselves are byte-identical."
        } else {
            "The ZIP files themselves have different bytes."
        });
        ui.collapsing("Exact sources & scope", |ui| {
            for (side, s) in [("Baseline", &r.baseline), ("Selected", &r.selected)] {
                ui.strong(side);
                for (what, id) in [
                    ("Delivery", &s.edition),
                    ("Issued file", &s.file),
                    ("Content revision", &s.content_revision),
                    ("Content recording decision", &s.content_decision),
                    ("ZIP SHA-256", &s.sha256),
                ] {
                    ui.add(
                        egui::Label::new(format!("{what}: {id}"))
                            .wrap()
                            .selectable(true),
                    );
                }
            }
            ui.label(r.contents.scope);
        });
        let files = &r.contents;
        if !files.baseline_root.is_empty() || !files.selected_root.is_empty() {
            ui.label(format!(
                "Comparing inside: baseline {} · selected {}",
                display_root(&files.baseline_root),
                display_root(&files.selected_root)
            ));
        }
        if !files.baseline_enclosing_folder.is_empty()
            || !files.selected_enclosing_folder.is_empty()
        {
            let mut apply = false;
            ui.collapsing("Align enclosing folders…",|ui|{
                ui.weak("For a changed package wrapper, choose where each path starts. Every file stays in the comparison.");
                Self::root_choice(ui,"Baseline folder",&files.baseline_enclosing_folder,&mut self.baseline_root);
                Self::root_choice(ui,"Selected folder",&files.selected_enclosing_folder,&mut self.selected_root);
                apply=ui.add_enabled(self.baseline_root!=files.baseline_root || self.selected_root!=files.selected_root,theme::primary("Compare at these folders")).clicked();
            });
            if apply {
                self.begin(ui.ctx(), None);
                return;
            }
        }
        ui.separator();
        let first = self.counts.is_none();
        let counts = self.counts.get_or_insert_with(|| {
            let mut counts = [0; 4];
            for m in &files.members {
                counts[m.difference as usize] += 1;
            }
            counts
        });
        ui.horizontal_wrapped(|ui| {
            for (kind, label, color) in [
                (Difference::Changed, "changed", theme::ACCENT),
                (Difference::OnlyBaseline, "baseline only", PURPLE),
                (Difference::OnlySelected, "selected only", TEAL),
                (Difference::Same, "same", theme::palette(ui).muted),
            ] {
                theme::badge(ui, &format!("{} {label}", counts[kind as usize]), color);
            }
        });
        ui.small("Uncompressed file contents by exact path; not a design review.");
        let toggled = ui
            .checkbox(&mut self.differences_only, "Differences only")
            .changed();
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.member_query)
                    .hint_text("Find a member path…")
                    .desired_width(f32::INFINITY),
            )
            .changed()
            || toggled
            || first
        {
            self.member_page = 0;
            let query = self.member_query.trim().to_lowercase();
            self.visible_members = files
                .members
                .iter()
                .enumerate()
                .filter_map(|(i, m)| {
                    ((!self.differences_only || m.difference != Difference::Same)
                        && (query.is_empty() || m.path.to_lowercase().contains(&query)))
                    .then_some(i)
                })
                .collect();
        }
        let rows = &self.visible_members;
        if rows.is_empty() {
            ui.weak(
                if self.member_query.trim().is_empty() && self.differences_only {
                    "No member differences."
                } else {
                    "No matching members."
                },
            );
        }
        for &i in rows.iter().skip(self.member_page * PAGE).take(PAGE) {
            let m = &files.members[i];
            let label = match m.difference {
                Difference::Same => "Same bytes",
                Difference::Changed => "Different bytes",
                Difference::OnlySelected => "Only in selected ZIP",
                Difference::OnlyBaseline => "Only in baseline ZIP",
            };
            ui.push_id(&m.path, |ui| {
                ui.separator();
                ui.add(
                    egui::Label::new(RichText::new(&m.path).strong())
                        .wrap()
                        .selectable(true),
                );
                ui.collapsing(label, |ui| {
                    for (side, label, root, entry) in [
                        (Side::Baseline, "Baseline", &files.baseline_root, &m.baseline),
                        (Side::Selected, "Selected", &files.selected_root, &m.selected),
                    ] {
                        if let Some(entry) = entry {
                            let issued_zip::Fingerprint { bytes, sha256 } = &entry.fingerprint;
                            if !root.is_empty() {
                                ui.add(
                                    egui::Label::new(format!("{label} path: {root}{}", m.path))
                                        .wrap()
                                        .selectable(true),
                                );
                            }
                            ui.small(format!("{label} · {bytes} bytes · SHA-256"));
                            ui.add(egui::Label::new(sha256).wrap().selectable(true));
                            let other = if side == Side::Baseline { "selected" } else { "baseline" };
                            if ui.link(format!("Locate these bytes in {other} ZIP →")).clicked() {
                                let anchor = MemberAddress { side, path: format!("{root}{}", m.path) };
                                self.locations = Some(files.locate(&anchor, 0, PAGE).map_err(|e| e.to_string()));
                                self.scroll_top = true;
                            }
                        }
                    }
                });
            });
        }
        home::pager(ui, &mut self.member_page, rows.len(), PAGE, "members");
    }
    fn show(&mut self, app: &App, ui: &mut Ui, p: &Package) {
        let Some(state) = self.compare.clone() else {
            self.choose(ui, p);
            return;
        };
        if std::mem::take(&mut self.scroll_top) { ui.scroll_to_cursor(Some(egui::Align::Min)); }
        if let Some(Ok(r)) = state.get() {
            if let Some((address, offset)) = self.pending_lookup.take() {
                self.locations = Some(r.contents.locate(&address, offset, PAGE).map_err(|e| e.to_string()));
            }
        }
        match state.get() {
            Some(Ok(r)) if self.locations.is_some() => {
                match self.locations.as_ref().unwrap() {
                    Ok(found) => match locations::show(app, ui, r, found) {
                        Some(locations::Action::Back) => self.locations = None,
                        Some(locations::Action::Page(offset)) => {
                            self.locations = Some(r.contents.locate(&found.source, offset, PAGE).map_err(|e| e.to_string()));
                        }
                        Some(locations::Action::Open(side, path)) => {
                            let zip = locations::source(r, side);
                            if let Some(entry) = r.contents.at_original(side, &path) {
                                let member = artifacts::MemberTarget::from_zip(zip.sha256.clone(), zip.path.clone(), entry.index, path);
                                self.open_member = Some((zip.edition.clone(), zip.file.clone(), member));
                            }
                        }
                        None => {}
                    },
                    Err(reason) => {
                        ui.colored_label(ui.visuals().error_fg_color, reason);
                        if ui.button("Back to differences").clicked() { self.locations = None; }
                    }
                }
                return;
            }
            Some(Ok(r)) => self.result(app, ui, r),
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                if ui.button("Check these ZIPs again").clicked() {
                    let address = self.pending_lookup.take();
                    self.begin(ui.ctx(), address);
                }
            }
            None => {
                ui.spinner();
                ui.label("Verifying editions and reading member bytes…");
                return;
            }
        }
        if ui.button("Choose ZIPs…").clicked() {
            self.compare = None;
            self.member_page = 0;
            self.member_query.clear();
            self.baseline_root.clear();
            self.selected_root.clear();
            self.counts = None;
            self.visible_members.clear();
        }
    }
}
/// Return to the same pair and original path, not to cached proof of issue.
/// Keep the picker settings but release the old fingerprint table while away.
pub(super) struct Return {
    window: Window,
    edition: String,
    address: MemberAddress,
    offset: usize,
}
impl Return {
    pub(super) fn reopen(self, app: &mut App, ctx: &egui::Context) {
        let Self { mut window, edition, address, offset } = self;
        if window.root != app.root { return; }
        app.open_issued_package(ctx, &edition);
        if app.layout.editor != Editor::Delivery || app.layout.delivery != edition { return; }
        app.refresh_delivery_model();
        window.begin(ctx, Some((address, offset)));
        app.delivery_desk.compare = Some(window);
    }
}
fn display_root(prefix: &str) -> &str {
    if prefix.is_empty() {
        "(ZIP root)"
    } else {
        prefix
    }
}
impl App {
    pub(super) fn zip_comparison_window(&mut self, ctx: &egui::Context, p: &Package) {
        let Some(mut w) = self.delivery_desk.compare.take() else {
            return;
        };
        if self.root != w.root {
            return;
        }
        let rect = ctx.content_rect().shrink(6.);
        let bounds = (rect.size() - egui::vec2(32., 76.)).max(egui::vec2(1., 1.));
        let initial = egui::vec2(620., 560.).min(bounds);
        let mut open = true;
        egui::Window::new("Compare issued ZIPs")
            .id(egui::Id::new(("issued-zip-comparison", &p.reference)))
            .open(&mut open)
            .min_size(egui::vec2(180., 120.).min(bounds))
            .max_size(bounds)
            .default_size(initial)
            .constrain_to(rect)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.set_max_width(ui.available_width());
                w.show(self, ui, p);
            });
        if let Some((edition, file, member)) = w.open_member.take() {
            // Re-open the real edition on the chosen side (which can belong to
            // a different activity). The Save and Back actions must follow it.
            self.open_issued_package(ctx, &edition);
            if self.layout.delivery == edition && self.layout.editor == Editor::Delivery {
                self.open_package_file(ctx, &edition, &file);
                self.preview_member = Some(member);
                if let (Some(Ok(found)), Some(state)) = (w.locations.take(), w.compare.take()) {
                    if let Some(Ok(r)) = state.get() {
                        // The enclosing-folder controls may contain an unapplied draft.
                        w.baseline_root.clone_from(&r.contents.baseline_root);
                        w.selected_root.clone_from(&r.contents.selected_root);
                    }
                    self.delivery_desk.lookup_return = Some(Return {
                        window: w, edition: p.reference.clone(), address: found.source, offset: found.offset,
                    });
                }
            }
        } else if open {
            self.delivery_desk.compare = Some(w);
        }
    }
}
