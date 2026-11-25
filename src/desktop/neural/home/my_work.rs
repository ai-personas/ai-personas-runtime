//! Read-only entry points for the named human at this desk. A task role and
//! a claim on an outside step are independent, even within the same task.
use super::*;
use crate::desktop::neural::home_index::{Activity, HumanRole};

#[derive(Clone, Copy, Default)]
pub(super) struct Pages {
    tasks: usize,
    steps: usize,
}

const PER_PAGE: usize = 5;

enum Open {
    Board(String),
    Receipts(String),
    Edition(String, String),
    Request(String),
    FieldDesk,
    FindActivity,
}

impl App {
    pub(super) fn my_work(&mut self, ui: &mut Ui) {
        let operator = self.operator.trim();
        eyebrow(ui, "My work  /  Local operator");
        ui.label(design::display(operator, 26.));
        if operator.is_empty() {
            ui.weak("This window has no operator name.");
            return;
        }
        if self.snapshot.status.is_null() {
            ui.weak(if self.error.is_some() {
                "Workspace data unavailable."
            } else {
                "Opening workspace…"
            });
            return;
        }
        ui.label("Your declared roles and the requests you’re following through on. Taking a role does not accept a delivery.");
        ui.add_space(9.);

        let tasks = self.home_index.roles_for(operator);
        let steps = self.home_index.steps_for(operator);
        let mut pages = self.home.my_work;
        let mut open = None;
        let show_tasks = |ui: &mut Ui, page: &mut usize, open: &mut Option<Open>| {
            panel(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                panel_heading(ui, Icon::Task, TEAL, "My task roles", tasks.len());
                *page = (*page).min(last_page(tasks.len(), PER_PAGE));
                for (i, &(a, j)) in tasks.iter().skip(*page * PER_PAGE).take(PER_PAGE).enumerate() {
                    if i > 0 { divider(ui); }
                    let activity = &self.home_index.activities[a];
                    let role = &activity.roles[j];
                    ui.push_id(&role.reference, |ui| self.my_task(ui, activity, role, open));
                }
                if tasks.is_empty() {
                    ui.label("No current task role under this name.");
                    ui.weak("On a task, open Participate → Responsibility board → Choose my part… to own the follow-through or contribute.");
                    if ui.small_button("Find an activity…").clicked() { *open = Some(Open::FindActivity); }
                }
                pager(ui, page, tasks.len(), PER_PAGE, "task roles");
            });
        };
        let show_steps = |ui: &mut Ui, page: &mut usize, open: &mut Option<Open>| {
            panel(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                panel_heading(ui, Icon::Questions, PURPLE, "Outside steps I hold", steps.len());
                *page = (*page).min(last_page(steps.len(), PER_PAGE));
                for (i, &idx) in steps.iter().skip(*page * PER_PAGE).take(PER_PAGE).enumerate() {
                    if i > 0 { divider(ui); }
                    let r = &self.records("human_requests")[idx];
                    let b = &r["body"];
                    ui.push_id(inbox::handle(r), |ui| {
                        ui.horizontal(|ui| theme::badge(ui, "In progress", PURPLE));
                        if title_link(ui, s(b,"title"), &inbox::handle(r)).clicked() {
                            *open = Some(Open::Request(s(&r["reference"],"id").into()));
                        }
                        provenance(ui, "From", self.label(s(b,"requester")), self.home_activity_title(s(b,"work")));
                        let last = &b["last_report"];
                        if let Some(text) = last["text"].as_str().filter(|t| !t.is_empty()) {
                            ui.small(format!("Last note from {} · {}", s(last,"human"), short_text(text, 140))).on_hover_text(text);
                        }
                    });
                }
                if steps.is_empty() { ui.weak("You haven’t taken an active outside step. A task role doesn’t put requests in your name."); }
                pager(ui, page, steps.len(), PER_PAGE, "outside steps");
                ui.add_space(6.);
                if ui.link("Browse the field desk →").clicked() { *open = Some(Open::FieldDesk); }
            });
        };
        if ui.available_width() >= 810. {
            ui.columns(2, |cols| {
                show_tasks(&mut cols[0], &mut pages.tasks, &mut open);
                show_steps(&mut cols[1], &mut pages.steps, &mut open);
            });
        } else {
            show_tasks(ui, &mut pages.tasks, &mut open);
            ui.add_space(8.);
            show_steps(ui, &mut pages.steps, &mut open);
        }
        self.home.my_work = pages;
        match open {
            Some(Open::Board(work)) => self.open_task_board(&work),
            Some(Open::Receipts(r)) => self.open_role_receipts(ui.ctx(), r),
            Some(Open::Edition(r, work)) => self.open_home_delivery(&r, &work),
            Some(Open::Request(id)) => self.show_human_request(&id),
            Some(Open::FieldDesk) => {
                self.inbox.active_list();
                self.layout.editor = Editor::Inbox;
            }
            Some(Open::FindActivity) => self.find_activities(ui.ctx()),
            None => {}
        }
    }

    fn my_task(&self, ui: &mut Ui, activity: &Activity, role: &HumanRole, open: &mut Option<Open>) {
        ui.horizontal(|ui| theme::badge(ui, role.label, TEAL));
        if title_link(ui, &activity.title, "Open responsibility board").clicked() {
            *open = Some(Open::Board(activity.id.clone()));
        }
        ui.label(short_text(&role.note, 240))
            .on_hover_text(&role.note);
        if role.brief != activity.brief {
            ui.weak("The brief has changed since this role was recorded.");
        }
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Responsibility board →").clicked() {
                *open = Some(Open::Board(activity.id.clone()));
            }
            if ui
                .small_button("Who received this?")
                .on_hover_text(&role.reference)
                .clicked()
            {
                *open = Some(Open::Receipts(role.reference.clone()));
            }
        });
        if activity.deliveries.is_empty() {
            return;
        }
        ui.add_space(3.);
        egui::CollapsingHeader::new(format!(
            "Deliveries on this task · {}",
            activity.deliveries.len()
        ))
        .id_salt("task-deliveries")
        .default_open(activity.deliveries.len() == 1)
        .show(ui, |ui| {
            // These are this activity’s editions, not assignments inferred from
            // recipient prose. Opening a title never substitutes a later issue.
            let key = ui.make_persistent_id("edition-page");
            let total = activity.deliveries.len();
            let mut page = ui
                .data(|d| d.get_temp::<usize>(key))
                .unwrap_or(0)
                .min(last_page(total, PER_PAGE));
            for &i in activity
                .deliveries
                .iter()
                .skip(page * PER_PAGE)
                .take(PER_PAGE)
            {
                let r = &self.records("deliveries")[i];
                let reference = inbox::handle(r);
                ui.push_id(&reference, |ui| {
                    let b = &r["body"];
                    if ui
                        .link(short_text(s(b, "title"), row_limit(ui, 1.8)))
                        .on_hover_text(format!("{}\n{reference}", s(b, "title")))
                        .clicked()
                    {
                        *open = Some(Open::Edition(reference.clone(), activity.id.clone()));
                    }
                    ui.small(format!(
                        "By {} · {}",
                        self.label(s(b, "owner")),
                        edition_scope(s(b, "scope"))
                    ));
                });
            }
            pager(ui, &mut page, total, PER_PAGE, "deliveries");
            ui.data_mut(|d| d.insert_temp(key, page));
        });
    }
}

fn edition_scope(scope: &str) -> &'static str {
    match scope {
        "partial" => "Partial delivery",
        "claimed_complete" => "Author claims complete",
        _ => "Delivery",
    }
}
