use super::*;

const PACKAGES: usize = 4;

#[derive(Default)]
pub(super) struct Uses {
    subject: String,
    issue: (usize, String),
    offset: usize,
    page: Read<Value>,
}

impl Uses {
    fn load(&mut self, ctx: &egui::Context, root: &Path, reference: &str, offset: usize) {
        self.subject = reference.into();
        self.offset = offset;
        let root = root.to_owned();
        let reference = reference.to_owned();
        self.page.start(ctx, move || {
            data::open(&root)
                .and_then(|db| {
                    ai_personas::neural::asset_history::deliveries(
                        &db, &reference, offset, PACKAGES,
                    )
                })
                .map_err(|e| e.to_string())
        });
    }
}

impl App {
    pub(super) fn file_deliveries(
        &mut self,
        ui: &mut Ui,
        reference: &str,
        action: &mut Option<Action>,
    ) {
        egui::CollapsingHeader::new("Deliveries using this revision")
            .id_salt(("file-edition-deliveries", reference))
            .show(ui, |ui| {
                let issues = &self.home_index.deliveries;
                let newest = issues
                    .first()
                    .and_then(|i| self.records("deliveries").get(*i))
                    .map(inbox::handle)
                    .unwrap_or_default();
                let stamp = (issues.len(), newest);
                let uses = &mut self.file_history.uses;
                if uses.subject != reference {
                    *uses = Default::default();
                    uses.issue = stamp;
                    uses.load(ui.ctx(), &self.root, reference, 0);
                } else if uses.issue != stamp && uses.page.pending.is_none() {
                    uses.issue = stamp;
                    uses.load(ui.ctx(), &self.root, reference, uses.offset);
                }
                uses.page.poll();
                let Some(result) = uses.page.result.clone() else {
                    ui.weak("Reading issued packages…");
                    return;
                };
                let page = match result {
                    Ok(page) => page,
                    Err(reason) => {
                        ui.strong("Package evidence unavailable");
                        ui.add(egui::Label::new(reason).wrap());
                        if ui.small_button("Retry packages").clicked() {
                            uses.load(ui.ctx(), &self.root, reference, uses.offset);
                        }
                        return;
                    }
                };
                let total = page["total"].as_u64().unwrap_or(0) as usize;
                if total == 0 {
                    ui.weak("No verified delivery includes this exact revision.");
                }
                for package in rows(&page) {
                    let handle = s(package, "reference");
                    if ui.link(s(package, "title")).on_hover_text(handle).clicked() {
                        *action = Some(Action::Open(Target::Delivery(handle.into())));
                    }
                    let mut detail = format!("Issued by {}", self.label(s(package, "owner")));
                    if let Some(n) = package["files"].as_u64() {
                        detail.push_str(&format!("  ·  {}", counted(n, "file", "files")));
                    }
                    ui.small(detail)
                        .on_hover_text(self.work_name(s(package, "work")));
                }
                if page["unverified"].as_u64().unwrap_or(0) > 0 {
                    ui.small("Some package evidence could not be verified.");
                }
                if total > PACKAGES {
                    let offset = page["offset"].as_u64().unwrap_or(0) as usize;
                    let ready = self.file_history.uses.page.pending.is_none();
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                ready && offset > 0,
                                egui::Button::new("Newer packages").small(),
                            )
                            .clicked()
                        {
                            self.file_history.uses.load(
                                ui.ctx(),
                                &self.root,
                                reference,
                                offset.saturating_sub(PACKAGES),
                            );
                        }
                        ui.small(format!(
                            "{}–{} of {total}",
                            offset + 1,
                            (offset + PACKAGES).min(total)
                        ));
                        if ui
                            .add_enabled(
                                ready && offset + PACKAGES < total,
                                egui::Button::new("Older packages").small(),
                            )
                            .clicked()
                        {
                            self.file_history.uses.load(
                                ui.ctx(),
                                &self.root,
                                reference,
                                offset + PACKAGES,
                            );
                        }
                    });
                }
            });
    }
}
