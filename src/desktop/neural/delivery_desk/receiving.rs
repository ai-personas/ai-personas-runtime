//! A receiving run belongs to a particular edition or dispatch slip. A command
//! result and the issuer's supported-use judgment are independently inspectable.
use super::*;

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

impl App {
    pub(super) fn receiving_checks(&mut self, ui: &mut Ui, package: &Package) {
        let events = self.home_index.receiving_checks(&package.reference);
        let mut page = self.delivery_desk.check_page;
        let mut inspect = None;
        ui.add_space(8.);
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Activity, theme::palette(ui).ink(TEAL));
                heading(ui, "Receiving checks");
                if !events.is_empty() {
                    ui.weak(counted(events.len() as u64, "run", "runs"));
                }
            });
            if events.is_empty() {
                ui.weak("No persona has run a check on the files from this edition.");
                return;
            }
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
            });
            if let Some(error) = check["error"].as_str() {
                ui.colored_label(theme::palette(ui).ink(theme::ACCENT), error);
            }
        });
        self.delivery_desk.check_page = page;
        if let Some(reference) = inspect {
            let command = self.command_bench.locate(reference);
            self.command(ui.ctx(), command);
        }
    }
}
