//! Read-only history of an exact retained file, with independent drill-downs.
use super::*;
use std::path::Path;

mod compare;
mod deliveries;
mod view;

const PAGE: usize = 7;

struct Read<T> {
    pending: Option<mpsc::Receiver<Result<T, String>>>,
    result: Option<Result<Arc<T>, String>>,
}
impl<T> Default for Read<T> {
    fn default() -> Self {
        Self {
            pending: None,
            result: None,
        }
    }
}
impl<T: Send + 'static> Read<T> {
    fn start(
        &mut self,
        ctx: &egui::Context,
        read: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read());
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) -> bool {
        let Some(rx) = &self.pending else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("The file history reader stopped.".into()),
        };
        self.pending = None;
        self.result = Some(result.map(Arc::new));
        true
    }
}

#[derive(Clone)]
enum Target {
    File(String),
    Activity(String),
    Delivery(String),
}

#[derive(Clone)]
struct Origin {
    editor: Editor,
    asset: String,
    person: String,
    work: String,
    profile: people::ProfileView,
    edition: Option<String>,
    package: Option<delivery_desk::ReturnTo>,
}

#[derive(Default)]
pub(super) struct History {
    visible: bool,
    opened: String,
    opened_path: String,
    selected: String,
    origin: Option<Origin>,
    target: Option<Target>,
    page: Read<Value>,
    generation: u64,
    comparing: Option<(String, String)>,
    comparison: Read<compare::Comparison>,
    diff_page: usize,
    show_list: bool,
    uses: deliveries::Uses,
}

enum Action {
    Select(String),
    Page(usize),
    Open(Target),
    Print { reference: String, path: String },
    RetryHistory,
    RetryComparison,
}

impl History {
    fn read_page(&mut self, ctx: &egui::Context, root: &Path, offset: Option<usize>) {
        let focus = if offset.is_some() {
            &self.opened
        } else {
            &self.selected
        }
        .clone();
        let root = root.to_owned();
        self.page.start(ctx, move || {
            data::open(&root)
                .and_then(|db| {
                    use ai_personas::neural::asset_history::history;
                    let mut page = history(&db, &focus, offset.unwrap_or(0), PAGE)?;
                    if offset.is_none() {
                        let start =
                            page["selected_offset"].as_u64().unwrap_or(0) as usize / PAGE * PAGE;
                        if start > 0 {
                            page = history(&db, &focus, start, PAGE)?;
                        }
                    }
                    for row in page["rows"].as_array_mut().into_iter().flatten() {
                        if let Some(t) = row["created"].as_i64() {
                            let (utc, media): (Option<String>, Option<String>) = db.query_row(
                                "SELECT strftime('%Y-%m-%d %H:%M:%S UTC',?1,'unixepoch'),
                                    (SELECT media FROM objects WHERE digest=?2)",
                                rusqlite::params![t, row["sha256"].as_str()],
                                |r| Ok((r.get(0)?, r.get(1)?)),
                            )?;
                            row["created_utc"] = utc.into();
                            row["media"] = media.into();
                        }
                    }
                    Ok(page)
                })
                .map_err(|e| e.to_string())
        });
    }

    fn observe(&mut self, ctx: &egui::Context, root: &Path, assets: &[Value], generation: u64) {
        if self.page.poll() {
            if let Some(Ok(page)) = &self.page.result {
                let rows = page["rows"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                if let Some(path) = rows
                    .iter()
                    .find(|r| r["reference"] == self.opened)
                    .and_then(|r| r["path"].as_str())
                {
                    self.opened_path = path.into();
                }
                if !rows.iter().any(|r| r["reference"] == self.selected) {
                    if let Some(first) = rows.first() {
                        self.selected = s(first, "reference").into();
                    }
                }
            }
        }
        if self.generation != generation && self.page.pending.is_none() {
            self.generation = generation;
            if let Some(Ok(page)) = &self.page.result {
                let current = s(page, "current_reference");
                // An unrelated activity update must not interrupt reading a difference.
                let logical = self.opened.rsplit_once('@').map(|(id, _)| id);
                if assets.iter().any(|a| {
                    a["current"] == true
                        && a["reference"] != current
                        && s(a, "reference").rsplit_once('@').map(|(id, _)| id) == logical
                }) {
                    self.read_page(ctx, root, None);
                }
            }
        }
        self.comparison.poll();
        let row = self
            .page
            .result
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|p| rows(p).iter().find(|r| r["reference"] == self.selected));
        let pair = row.and_then(|r| {
            Some((
                s(r, "reference").to_owned(),
                r["previous"].as_str()?.to_owned(),
            ))
        });
        if self.comparing != pair && self.comparison.pending.is_none() {
            self.comparing = pair.clone();
            self.comparison.result = None;
            self.diff_page = 0;
            if let Some((selected, previous)) = pair {
                let change = s(&row.unwrap()["changes"], "content").to_owned();
                let root = root.to_owned();
                self.comparison.start(ctx, move || {
                    data::open(&root)
                        .and_then(|db| compare::read(&db, &previous, &selected, &change))
                        .map_err(|e| e.to_string())
                });
            }
        }
    }
}

fn rows(page: &Value) -> &[Value] {
    page["rows"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

impl App {
    pub(super) fn open_file_history(&mut self, ctx: &egui::Context, reference: &str) {
        self.print_preview = None;
        let package = match self.layout.editor {
            Editor::Delivery => Some(delivery_desk::ReturnTo {
                package: self.layout.delivery.clone(),
                work: self.work.clone(),
                file: reference.into(),
            }),
            Editor::Artifacts => self.delivery_desk.return_for(&self.selected_reference),
            _ => None,
        };
        let origin = Origin {
            editor: self.layout.editor,
            asset: self.selected_reference.clone(),
            person: self.persona.clone(),
            work: self.work.clone(),
            profile: self.profile_view,
            edition: match self.layout.editor {
                Editor::Delivery => Some(self.layout.delivery.clone()),
                Editor::Artifacts if self.selected_reference == reference => self.preview_package.clone(),
                _ => None,
            },
            package,
        };
        self.file_history = History {
            visible: true,
            opened: reference.into(),
            selected: reference.into(),
            origin: Some(origin),
            generation: self.home_index.generation,
            ..Default::default()
        };
        self.file_history.read_page(ctx, &self.root, None);
    }

    pub(super) fn file_history_return(&mut self, ui: &mut Ui) {
        if self.file_history.visible {
            return;
        }
        let matches = match &self.file_history.target {
            Some(Target::File(r)) => {
                self.layout.editor == Editor::Artifacts && self.selected_reference == *r
            }
            Some(Target::Activity(w)) => self.layout.editor == Editor::Studio && self.work == *w,
            Some(Target::Delivery(r)) => {
                (self.layout.editor == Editor::Delivery && self.layout.delivery == *r)
                    || (self.layout.editor == Editor::Artifacts && self.selected_reference == *r)
            }
            None => false,
        };
        if !matches || !ui.link("← Back to file history").clicked() {
            return;
        }
        self.print_preview = None;
        let Some(origin) = self.file_history.origin.clone() else {
            return;
        };
        self.persona = origin.person;
        self.work = origin.work;
        self.layout.editor = origin.editor;
        match origin.editor {
            Editor::Delivery => {
                if let Some(package) = &origin.package {
                    self.return_to_issued_file(package);
                }
            }
            Editor::Artifacts => {
                if self.selected_reference != origin.asset || self.preview_package != origin.edition {
                    let selected = origin.edition.map(|edition| self.issued_selection(edition, &origin.asset));
                    self.open_named_resource(ui.ctx(), origin.asset.clone(), selected);
                }
                self.delivery_desk.set_return(&origin.asset, origin.package);
            }
            Editor::People => self.profile_view = origin.profile,
            _ => {}
        }
        self.file_history.visible = true;
        self.file_history.target = None;
    }

    pub(super) fn file_history_window(&mut self, ctx: &egui::Context) {
        if !self.file_history.visible {
            return;
        }
        self.file_history.observe(
            ctx,
            &self.root,
            &self.snapshot.assets,
            self.home_index.generation,
        );
        let mut action = None;
        let id = egui::Id::new("retained-file-history");
        let size = ctx.content_rect().size();
        let tight = size.x < 390. || size.y < 340.;
        let margin: i8 = if tight { 6 } else { 16 };
        let width = (size.x - 2. * f32::from(margin) - 14.).clamp(1., 1080.);
        // A centered modal can keep its previous frame's height after a resize.
        // Anchor the controls; only the reading panes should need scrolling.
        let area = egui::Modal::default_area(id).anchor(
            egui::Align2::CENTER_TOP,
            egui::vec2(0., if tight { 4. } else { (size.y * 0.06).clamp(6., 48.) }),
        );
        let response = egui::Modal::new(id)
            .area(area)
            .frame(
                egui::Frame::new()
                    .fill(theme::Palette::new(self.layout.dark).canvas)
                    .inner_margin(margin)
                    .corner_radius(10)
                    .stroke(Stroke::new(
                        1.,
                        theme::Palette::new(self.layout.dark).border,
                    )),
            )
            .show(ctx, |ui| {
                self.file_history_contents(ui, width, margin, &mut action);
            });
        if response.should_close() {
            self.file_history = Default::default();
            return;
        }
        match action {
            Some(Action::Select(r)) => {
                let here = self
                    .file_history
                    .page
                    .result
                    .as_ref()
                    .and_then(|p| p.as_ref().ok())
                    .is_some_and(|page| rows(page).iter().any(|row| row["reference"] == r));
                self.file_history.selected = r;
                self.file_history.show_list = false;
                if !here {
                    self.file_history.read_page(ctx, &self.root, None);
                }
            }
            Some(Action::Page(offset)) => {
                self.file_history.read_page(ctx, &self.root, Some(offset))
            }
            Some(Action::RetryHistory) => self.file_history.read_page(ctx, &self.root, None),
            Some(Action::RetryComparison) => self.file_history.comparing = None,
            Some(Action::Print { reference, path }) => {
                self.file_history.visible = false;
                self.open_history_file(ctx, &reference);
                let package = self.preview_package.clone();
                self.open_print_preview(ctx, &reference, &path, package, None);
                self.file_history.target = Some(Target::File(reference));
            }
            Some(Action::Open(target)) => {
                self.file_history.visible = false;
                match &target {
                    Target::File(r) => self.open_history_file(ctx, r),
                    Target::Activity(work) => self.open_home_activity(work),
                    Target::Delivery(reference) => self.open_issued_package(ctx, reference),
                }
                self.file_history.target = Some(target);
            }
            None => {}
        }
    }

    fn open_history_file(&mut self, ctx: &egui::Context, reference: &str) {
        let target = self.file_history.origin.as_ref().and_then(|o| o.package.clone());
        let selected = self.file_history.origin.as_ref().filter(|_| self.file_history.opened == reference)
            .and_then(|o| o.edition.clone()).map(|edition| self.issued_selection(edition, reference));
        self.open_named_resource(ctx, reference.into(), selected);
        self.delivery_desk.set_return(reference, target);
    }
}
