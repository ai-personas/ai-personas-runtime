use super::*;
use ai_personas::neural::activity_sources;
use std::sync::OnceLock;

#[derive(Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    ThisWork,
    OtherWork,
    Unknown,
    Shared(String),
}

#[derive(Default)]
pub(super) struct Window {
    result: Option<Arc<OnceLock<Result<Value, String>>>>,
    filter: Filter,
    page: usize,
}

enum Open {
    Record(String),
    Choice(String),
    Activity(String),
    Edition(String),
}

impl App {
    pub(super) fn delivery_origins_window(&mut self, ctx: &egui::Context, package: &Package) {
        let Some(mut window) = self.delivery_desk.origins.take() else {
            return;
        };
        let mut visible = true;
        let mut open = None;
        let root = self.root.clone();
        egui::Window::new("File origins")
            .id(egui::Id::new(("delivery-file-origins", &package.reference)))
            .open(&mut visible)
            .resizable(true)
            .default_width(650.)
            .min_width(230.)
            .max_width((ctx.content_rect().width() - 30.).max(230.))
            .show(ctx, |ui| {
                ui.set_max_width(ui.available_width());
                ui.add(
                    egui::Label::new(RichText::new(s(&package.body, "title")).strong().size(18.))
                        .wrap(),
                );
                let result = window
                    .result
                    .get_or_insert_with(|| {
                        let slot = Arc::new(OnceLock::new());
                        let send = Arc::clone(&slot);
                        let reference = package.reference.clone();
                        let ctx = ctx.clone();
                        std::thread::spawn(move || {
                            let read = data::open(&root)
                                .and_then(|db| activity_sources::delivery_files(&db, &reference));
                            let _ = send.set(read.map_err(|e| e.to_string()));
                            ctx.request_repaint();
                        });
                        slot
                    })
                    .clone();
                match result.get() {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(issue)) => {
                        ui.colored_label(ui.visuals().error_fg_color, issue);
                        if ui.button("Retry file origins").clicked() {
                            window.result = None;
                        }
                    }
                    Some(Ok(report)) => {
                        egui::ScrollArea::vertical()
                            .id_salt(("file-origin-rows", &package.reference))
                            .max_height((ctx.content_rect().height() - 160.).clamp(120., 630.))
                            .show(ui, |ui| {
                                open = window.draw(ui, self, report);
                            });
                    }
                }
            });
        match open {
            Some(Open::Record(r)) => self.open_resource(ctx, r),
            Some(Open::Choice(r)) => self.open_choice_or_record(ctx, r),
            Some(Open::Activity(w)) => self.open_home_activity(&w),
            Some(Open::Edition(r)) => self.open_issued_package(ctx, &r),
            None if visible => self.delivery_desk.origins = Some(window),
            None => {}
        }
    }
}

impl Window {
    fn draw(&mut self, ui: &mut Ui, app: &App, report: &Value) -> Option<Open> {
        const PAGE: usize = 6;
        let files = report["files"].as_array()?;
        let earlier = report["earlier"].as_array()?;
        let mut open = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Issued by {}", app.label(s(report, "issuer"))));
            if let Some(decision) = report["issuing_decision"].as_str() {
                if ui.link("Issuing decision").on_hover_text(decision).clicked() {
                    open = Some(Open::Choice(decision.into()));
                }
            }
        });
        if !earlier.is_empty() {
            ui.add_space(6.);
            egui::CollapsingHeader::new(format!(
                "Earlier deliveries with shared file revisions · {}",
                earlier.len()
            ))
            .default_open(earlier.len() == 1)
            .show(ui, |ui| {
                for row in earlier {
                    ui.push_id(s(row, "reference"), |ui| {
                        if ui
                            .link(s(row, "title"))
                            .on_hover_text("Open this earlier delivery")
                            .clicked()
                        {
                            open = Some(Open::Edition(s(row, "reference").into()));
                        }
                        let total = row["shared"].as_array().map_or(0, Vec::len);
                        ui.horizontal_wrapped(|ui| {
                            ui.small(format!("Issued by {}", app.label(s(row, "owner"))));
                            if ui.small_button(format!("Show {} shared", total)).clicked() {
                                self.filter = Filter::Shared(s(row, "reference").into());
                                self.page = 0;
                            }
                        });
                    });
                }
            });
        }
        if report["unverified_earlier"].as_u64().unwrap_or(0) > 0 {
            ui.weak("Some earlier delivery records could not be verified.");
        }
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            for (filter, label, count) in [
                (Filter::All, "All files", files.len() as u64),
                (
                    Filter::ThisWork,
                    "This activity",
                    report["here"].as_u64().unwrap_or(0),
                ),
                (
                    Filter::OtherWork,
                    "Other activities",
                    report["elsewhere"].as_u64().unwrap_or(0),
                ),
                (
                    Filter::Unknown,
                    "Unknown origin",
                    report["unknown"].as_u64().unwrap_or(0),
                ),
            ] {
                if (count > 0 || filter == Filter::All)
                    && ui
                        .selectable_label(self.filter == filter, format!("{label} {count}"))
                        .clicked()
                {
                    self.filter = filter;
                    self.page = 0;
                }
            }
        });
        let shared = if let Filter::Shared(reference) = &self.filter {
            earlier
                .iter()
                .find(|r| r["reference"] == reference.as_str())
        } else {
            None
        };
        if let Some(previous) = shared {
            ui.small(format!("Shared with {}", s(previous, "title")));
        }
        let shared_refs: BTreeSet<_> = shared
            .into_iter()
            .flat_map(|r| strings(&r["shared"]))
            .collect();
        let matching: Vec<_> = files
            .iter()
            .filter(|file| {
                let source = &file["origin"];
                match self.filter {
                    Filter::All => true,
                    Filter::ThisWork => source["work"] == report["work"],
                    Filter::OtherWork => {
                        source["work"].is_string() && source["work"] != report["work"]
                    }
                    Filter::Unknown => source.is_null(),
                    Filter::Shared(_) => shared_refs.contains(s(file, "reference")),
                }
            })
            .collect();
        self.page = self.page.min(matching.len().saturating_sub(1) / PAGE);
        for file in matching.iter().skip(self.page * PAGE).take(PAGE) {
            ui.separator();
            ui.push_id(s(file, "reference"), |ui| {
                if ui
                    .link(RichText::new(s(file, "path")).strong())
                    .on_hover_text(s(file, "reference"))
                    .clicked()
                {
                    open = Some(Open::Record(s(file, "reference").into()));
                }
                let source = &file["origin"];
                if source.is_null() {
                    ui.weak("Content origin unavailable");
                    return;
                }
                let verb = match source["tool"].as_str() {
                    Some("asset.save") => "Saved by",
                    Some("asset.register") => "Registered by",
                    Some("asset.import") => "Imported by",
                    _ => "Recorded under",
                };
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!("{verb} {}", app.label(s(source, "person"))));
                    let work = s(source, "work");
                    let name = app.work_name(work);
                    let label = if source["work"] == report["work"] {
                        "This activity".to_owned()
                    } else {
                        excerpt(&name, 65)
                    };
                    if ui.link(label).on_hover_text(&name).clicked() {
                        open = Some(Open::Activity(work.into()));
                    }
                    if ui
                        .small_button("Source decision")
                        .on_hover_text(s(source, "decision"))
                        .clicked()
                    {
                        open = Some(Open::Choice(s(source, "decision").into()));
                    }
                });
            });
        }
        super::super::home::pager(ui, &mut self.page, matching.len(), PAGE, "files");
        open
    }
}
