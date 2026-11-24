//! Compare the actual member bytes of two chosen, issued ZIPs. Opening this
//! window does not run a check on behalf of a persona or record a package review.
use super::*;
use ai_personas::neural::issued_zip::{self, Comparison, Difference, Fingerprint};
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
            if let Some(b) = self.baseline {
                let (b, t) = (&self.options[b], &self.options[t]);
                let (be, bf, te, tf) = (
                    b.edition.clone(),
                    b.file.clone(),
                    t.edition.clone(),
                    t.file.clone(),
                );
                let (ctx, root) = (ui.ctx().clone(), self.root.clone());
                let state = Arc::new(OnceLock::new());
                self.compare = Some(state.clone());
                std::thread::spawn(move || {
                    let result = data::open(&root)
                        .and_then(|db| issued_zip::compare(&db, &be, &bf, &te, &tf));
                    let _ = state.set(result.map_err(|e| e.to_string()));
                    ctx.request_repaint();
                });
            }
        }
    }
    fn result(&mut self, ui: &mut Ui, r: &Comparison) {
        for (label, source) in [("Baseline", &r.baseline), ("Selected", &r.selected)] {
            ui.push_id(label, |ui| {
                eyebrow(ui, label);
                ui.add(egui::Label::new(RichText::new(&source.title).strong()).wrap())
                    .on_hover_text(&source.edition);
                ui.add(egui::Label::new(&source.path).wrap())
                    .on_hover_text(&source.file);
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
            ui.label(r.scope);
        });
        ui.separator();
        let first = self.counts.is_none();
        let counts = self.counts.get_or_insert_with(|| {
            let mut counts = [0; 4];
            for m in &r.members {
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
            self.visible_members = r
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
            let m = &r.members[i];
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
                    for (side, fp) in [("Baseline", &m.baseline), ("Selected", &m.selected)] {
                        if let Some(Fingerprint { bytes, sha256 }) = fp {
                            ui.small(format!("{side} · {bytes} bytes · SHA-256"));
                            ui.add(egui::Label::new(sha256).wrap().selectable(true));
                        }
                    }
                });
            });
        }
        home::pager(ui, &mut self.member_page, rows.len(), PAGE, "members");
    }
    fn show(&mut self, ui: &mut Ui, p: &Package) {
        let Some(state) = self.compare.clone() else {
            self.choose(ui, p);
            return;
        };
        match state.get() {
            Some(Ok(r)) => self.result(ui, r),
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
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
            self.counts = None;
            self.visible_members.clear();
        }
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
                w.show(ui, p);
            });
        if open {
            self.delivery_desk.compare = Some(w);
        }
    }
}
