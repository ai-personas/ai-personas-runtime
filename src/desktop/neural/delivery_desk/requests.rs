use super::*;
use std::cmp::Reverse;

const PER_PAGE: usize = 3;

/// A request can name an entire issued package or pick individual files. Use
/// the original request event and its current record to preserve that distinction.
pub(super) fn using_edition(app: &App, edition: &str) -> Vec<usize> {
    let records = app.records("human_requests");
    let mut matching: Vec<_> = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            let body = &record["body"];
            if !strings(&body["deliveries"]).any(|r| r == edition) {
                return None;
            }
            let origin = format!("human_request:{}@1", record["reference"]["id"].as_str()?);
            let seq = app.home_index.request_origin(&origin)?;
            let pos = app
                .events
                .binary_search_by_key(&seq, |e| e["event"].as_i64().unwrap_or(0))
                .ok()?;
            let issue = &app.events[pos];
            let call = issue["invocation"].as_str()?;
            (issue["kind"] == "human_request"
                && issue["target"] == "user"
                && issue["work"] == body["work"]
                && issue["actor"] == body["requester"]
                && issue["body"]["reference"] == origin
                && issue["body"]["deliveries"] == body["deliveries"]
                && body["decision"] == format!("experience:{call}@1"))
            .then_some(index)
        })
        .collect();
    matching.sort_by_cached_key(|&index| {
        let record = &records[index];
        let body = &record["body"];
        (
            !matches!(s(body, "state"), "open" | "in_progress"),
            Reverse(app.home_index.request_event(&inbox::handle(record))),
            index,
        )
    });
    matching
}

impl App {
    pub(super) fn package_human_requests(&mut self, ui: &mut Ui, package: &Package) {
        let total = package.requests.len();
        if total == 0 {
            return;
        }
        let mut selected = None;
        let mut page = self.delivery_desk.request_page.min((total - 1) / PER_PAGE);
        ui.add_space(10.);
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, "Human requests using this edition");
            let records = self.records("human_requests");
            for (position, &index) in package
                .requests
                .iter()
                .skip(page * PER_PAGE)
                .take(PER_PAGE)
                .enumerate()
            {
                if position > 0 {
                    ui.separator();
                }
                let record = &records[index];
                let body = &record["body"];
                let id = s(&record["reference"], "id");
                ui.push_id(id, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (state, color) = inbox::phase(s(body, "state"));
                        theme::badge(ui, state, color);
                        ui.small(format!("From {}", self.label(s(body, "requester"))));
                        let origin = format!("human_request:{id}@1");
                        if let Some(seq) = self.home_index.request_origin(&origin) {
                            ui.small(format!("Request #{seq}")).on_hover_text(origin);
                        }
                        if let Some(operator) = body["claimed_by"].as_str() {
                            let verb = if body["state"] == "in_progress" {
                                "Taken by"
                            } else {
                                "Reported by"
                            };
                            ui.small(format!("{verb} {operator}"));
                        }
                    });
                    if ui
                        .link(RichText::new(s(body, "title")).strong())
                        .on_hover_text(
                            "Open the requested action, return instructions and conversation",
                        )
                        .clicked()
                    {
                        selected = Some(id.to_owned());
                    }
                    let action = s(body, "action");
                    if !action.is_empty() && action != s(body, "title") {
                        ui.label(excerpt(action, 155)).on_hover_text(action);
                    }
                });
            }
            super::super::home::pager(ui, &mut page, total, PER_PAGE, "requests");
        });
        self.delivery_desk.request_page = page;
        if let Some(id) = selected {
            self.show_human_request(&id);
        }
    }
}
