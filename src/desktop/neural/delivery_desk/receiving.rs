//! A receiving run belongs to a particular edition or dispatch slip. A command
//! result and the issuer's supported-use judgment are independently inspectable.
use super::*;
use std::sync::OnceLock;

type Linked = Arc<OnceLock<Result<Value, String>>>;

/// The two ends can belong to the same decision, different people, or even
/// different activities. Only offer decision links after checking their origin.
fn issue_and_check(ui: &mut Ui, app: &App, package: &Package, seq: i64, checker: &str) -> Option<String> {
    let mut open = None;
    let reference = format!("experience:event-{seq}@1");
    egui::CollapsingHeader::new("Issue & check")
        .id_salt(("receiving-decisions", &app.root, &reference))
        .show(ui, |ui| {
            let id = ui.make_persistent_id((&app.root, &reference, app.home_index.generation));
            let result: Linked = ui.data_mut(|d| d.get_temp_mut_or_insert_with(id, || {
                let slot = Arc::new(OnceLock::new());
                let reader = Arc::clone(&slot);
                let root = app.root.clone();
                let reference = reference.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let read = data::open(&root).and_then(|db|
                        ai_personas::neural::context::read(&db, &reference, None, None));
                    let _ = reader.set(read.map_err(|e| e.to_string()));
                    ctx.request_repaint();
                });
                slot
            }).clone());
            match result.get() {
                None => { ui.spinner(); }
                Some(Ok(r)) if r["issuance_checked"]["delivery"] == package.reference => {
                    let link = &r["issuance_checked"];
                    let issuer = s(link, "issuer");
                    for (person, role, address) in [
                        (issuer, "Issued this edition", s(link, "decision").to_owned()),
                        (checker, "Chose the receiving run", format!("experience:{}@1", s(link, "checking_call"))),
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            avatar(ui, app.label(person), persona_color(person), 20.);
                            ui.strong(app.label(person));
                            ui.weak(role);
                            if ui.link("Open choice ↗").on_hover_text(&address).clicked() {
                                open = Some(address);
                            }
                        });
                    }
                }
                Some(_) => { ui.weak("The issuing and checking choices could not be linked."); }
            }
            if ui.small_button("Observation record ↗").on_hover_text(&reference).clicked() {
                open = Some(reference);
            }
        });
    open
}

fn observed(check: &Value) -> (String, Color32) {
    if check["timed_out"] == true {
        return ("Timed out".into(), theme::ACCENT);
    }
    if check["cancelled"] == true {
        return ("Cancelled".into(), theme::ACCENT);
    }
    if check["process_started"] != true {
        return ("Did not start".into(), theme::ACCENT);
    }
    if let Some(code) = check["exit_code"].as_i64() {
        let missing = check["missing_outputs"]
            .as_array()
            .is_some_and(|items| !items.is_empty());
        let status = if missing {
            format!("Exit {code} · missing output")
        } else {
            format!("Exit {code}")
        };
        return (
            status,
            if check["success"] == true {
                TEAL
            } else {
                theme::ACCENT
            },
        );
    }
    ("Interrupted".into(), theme::ACCENT)
}

fn prior_file_checks(ui: &mut Ui, app: &App, package: &Package) -> Option<String> {
    use ai_personas::neural::activity_sources::{prior_checks, PriorChecks};
    let mut open = None;
    egui::CollapsingHeader::new("Earlier receiving runs")
        .id_salt(("prior-file-checks", &package.reference))
        .show(ui, |ui| {
            let subject = ui.make_persistent_id(("prior-check-scope", &app.root, &package.reference));
            let mut file = ui.data(|d| d.get_temp::<String>(subject)).unwrap_or_default();
            let before = file.clone();
            let selected = package.by_reference.get(&file).map(|&i| package.files[i].path.as_str());
            ui.small("What traveled in an earlier handoff?");
            egui::ComboBox::from_id_salt(subject)
                .selected_text(selected.unwrap_or("Complete set of issued files"))
                .width(ui.available_width()).truncate().show_ui(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.selectable_value(&mut file, String::new(), "Complete set of issued files");
                    for f in &package.files {
                        ui.selectable_value(&mut file, f.reference.clone(), &f.path).on_hover_text(&f.reference);
                    }
                }).response.on_hover_text(selected.unwrap_or("Use the complete set of exact issued revisions at their original paths"));
            let page_id = subject.with(("page", &file));
            if before != file {
                ui.data_mut(|d| { d.insert_temp(subject, file.clone()); d.remove::<usize>(page_id); });
            }
            let mut page = ui.data(|d| d.get_temp::<usize>(page_id)).unwrap_or_default();
            let id = page_id.with(page);
            let result: Linked = ui.data_mut(|d| d.get_temp_mut_or_insert_with(id, || {
                let slot = Arc::new(OnceLock::new());
                let reader = Arc::clone(&slot);
                let root = app.root.clone();
                let query = PriorChecks { delivery: package.reference.clone(), file: (!file.is_empty()).then(|| file.clone()), offset: page, page_size: 1 };
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let r = data::open(&root).and_then(|db| prior_checks(&db, None, &query));
                    let _ = reader.set(r.map_err(|e| e.to_string()));
                    ctx.request_repaint();
                });
                slot
            }).clone());
            let Some(result) = result.get() else { ui.spinner(); return; };
            let report = match result {
                Ok(r) => r,
                Err(e) => {
                    ui.weak(e);
                    if ui.small_button("Retry earlier runs").clicked() {
                        ui.data_mut(|d| d.remove::<Linked>(id));
                        ui.ctx().request_repaint();
                    }
                    return;
                }
            };
            let count = report["total"].as_u64().unwrap_or_default() as usize;
            if count == 0 {
                ui.weak(if file.is_empty() { "No earlier full-edition run on this file set." } else { "No earlier receiving run staged this exact file revision." });
                return;
            }
            ui.weak(if file.is_empty() { "These exact files were staged together. The earlier edition had its own instructions." }
                else { "This file revision was staged. Other files and instructions may differ; staging does not prove the file was opened or tested." });
            let previous = page;
            home::pager(ui, &mut page, count, 1, "earlier runs");
            ui.data_mut(|d| d.insert_temp(page_id, page));
            if page != previous { return; }
            let row = &report["items"][0];
            if let Some(path) = row["staged_file"]["path"].as_str() {
                let slip = row["dispatch"].as_u64().map_or_else(|| "Full earlier edition".to_owned(), |n| format!("Earlier dispatch #{n}"));
                ui.small(format!("{slip} · {} staged", counted(row["files_staged"].as_u64().unwrap_or_default(), "file", "files")));
                ui.add(egui::Label::new(format!("Earlier staging path: {path}")).wrap().selectable(true));
            }
            let who = s(row, "checker");
            ui.horizontal_wrapped(|ui| {
                avatar(ui, app.label(who), persona_color(who), 20.);
                ui.strong(app.label(who));
                let (label, tint) = observed(row);
                theme::badge(ui, &label, tint);
                if ui.link("Checking choice ↗").clicked() { open = Some(s(row,"checking_choice").to_owned()); }
                if ui.link("Observation ↗").clicked() { open = Some(s(row,"observation").to_owned()); }
            });
            ui.label(s(row, "purpose"));
            ui.horizontal_wrapped(|ui| {
                ui.weak(format!("Original issue · {}", app.label(s(row,"issuer"))));
                if ui.link(s(row,"title")).on_hover_text(s(row,"earlier_edition")).clicked() {
                    open = Some(s(row,"earlier_edition").to_owned());
                }
            });
        });
    open
}

impl App {
    pub(super) fn receiving_checks(&mut self, ui: &mut Ui, package: &Package) {
        let events = self.home_index.receiving_checks(&package.reference);
        let mut page = self.delivery_desk.check_page;
        let mut inspect = None;
        let mut record = None;
        let mut activity = None;
        ui.add_space(8.);
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Activity, theme::palette(ui).ink(TEAL));
                heading(ui, "Receiving checks");
                if !events.is_empty() {
                    ui.weak(format!("{} on this edition", counted(events.len() as u64, "run", "runs")));
                }
            });
            record = prior_file_checks(ui, self, package);
            if events.is_empty() {
                ui.weak("No receiving run was addressed to this edition.");
                return;
            }
            ui.separator();
            home::pager(ui, &mut page, events.len(), 1, "checks");
            let seq = events[events.len() - 1 - page.min(events.len() - 1)];
            let Ok(index) = self
                .events
                .binary_search_by_key(&seq, |e| e["event"].as_i64().unwrap_or(0))
            else {
                return;
            };
            let event = &self.events[index];
            let check = &event["body"];
            let who = s(event, "actor");
            let name = self.label(who);
            let slip = check["dispatch"]
                .as_u64()
                .and_then(|n| n.checked_sub(1))
                .and_then(|i| package.body["dispatches"].get(i as usize));
            ui.add_space(4.);
            ui.horizontal_wrapped(|ui| {
                avatar(ui, name, persona_color(who), 23.);
                ui.strong(name);
                let scope = slip.map_or("Full delivery", |s| {
                    s["recipient"].as_str().unwrap_or("Dispatch")
                });
                ui.label(format!("→ {scope}"));
                let (status, tint) = observed(check);
                theme::badge(ui, &status, tint);
            });
            ui.add(egui::Label::new(s(check, "purpose")).wrap());
            ui.horizontal_wrapped(|ui| {
                let count = check["files_staged"].as_u64().unwrap_or(0);
                ui.small(format!(
                    "{} staged · {}",
                    counted(count, "file", "files"),
                    s(check, "program")
                ));
                match s(check, "receiving_environment") {
                    "receiving_launcher" => { ui.small("· Receiving toolchain").on_hover_text("A receiving launcher was selected. Inspect the run for its identity and result."); }
                    "local_toolchain" => { ui.small("· Local toolchain").on_hover_text("Fresh files with the ordinary local toolchain."); }
                    _ => {}
                }
                ui.weak(RichText::new(s(event, "created_utc")).small());
                if ui
                    .add_enabled(!self.busy, egui::Link::new("Inspect run"))
                    .on_hover_text("Open the command, staged files, output and diagnostics")
                    .clicked()
                {
                    inspect = check["receipt"].as_str().map(str::to_owned);
                }
                if let Some(work) = check["checking_work"].as_str().filter(|w| *w != s(&package.body, "work")) {
                    if ui.link("Receiving activity ↗").on_hover_text(self.work_name(work)).clicked() {
                        activity = Some(work.to_owned());
                    }
                }
            });
            record = issue_and_check(ui, self, package, seq, who).or(record.take());
            if let Some(error) = check["error"].as_str() {
                ui.colored_label(theme::palette(ui).ink(theme::ACCENT), error);
            }
        });
        self.delivery_desk.check_page = page;
        if let Some(reference) = inspect {
            let command = self.command_bench.locate(reference);
            self.command(ui.ctx(), command);
        } else if let Some(reference) = record {
            if reference.starts_with("delivery:") { self.open_issued_package(ui.ctx(), &reference); }
            else { self.open_choice_or_record(ui.ctx(), reference); }
        } else if let Some(work) = activity {
            self.open_home_activity(&work);
        }
    }
}
