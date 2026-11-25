use super::*;
use ai_personas::neural::task_roles::Role;

pub(super) struct Draft {
    work: String,
    operator: String,
    brief: String,
    base: Option<String>,
    role: Role,
    note: String,
}

pub(super) fn show(
    ui: &mut Ui,
    report: &Value,
    operator: &str,
    busy: bool,
    draft: &mut Option<Draft>,
) -> Option<Action> {
    let work = s(report, "work");
    let brief = s(report, "brief");
    let ended = report["phase"] == "dissolved";
    let roles = items(report, "human_roles");
    let active = |r: &&Value| r["state"] == "active";
    let owner = roles.iter().filter(active).find(|r| r["role"] == "owner");
    let mine = roles.iter().find(|r| r["operator"] == operator);
    let base = mine.and_then(|r| r["reference"].as_str());
    let busy_owner = owner.is_some_and(|r| r["operator"] != operator);
    let mut action = None;
    if draft
        .as_ref()
        .is_some_and(|d| d.work != work || d.operator != operator)
        || ended
    {
        *draft = None;
    }
    card(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        eyebrow(ui, "Responsibility board  /  Human desk");
        ui.label(RichText::new("People on this task").size(19.).strong());
        if owner.is_none() && !ended { ui.weak("No human owner. Personas can work independently."); }
        for r in owner.into_iter().chain(roles.iter().filter(active).filter(|r| r["role"] != "owner")) {
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, if r["role"] == "owner" {"Owner"} else {"Contributor"}, TEAL);
                ui.strong(s(r,"operator"));
                if ui.small_button("Role record ↗").on_hover_text(s(r,"reference")).clicked() {
                    action = Some(Action::Inspect(s(r,"reference").into()));
                }
            });
            ui.label(s(r,"note"));
        }
        let inactive: Vec<_> = roles.iter().filter(|r| r["state"] != "active").collect();
        if !inactive.is_empty() {
            ui.collapsing(format!("Earlier participants · {}", inactive.len()), |ui| {
                for r in inactive {ui.horizontal_wrapped(|ui| {
                    ui.label(s(r,"operator"));
                    ui.small(if r["state"] == "left" {"Left this task"} else {"Activity ended"});
                    if ui.link("Record ↗").clicked() {action = Some(Action::Inspect(s(r,"reference").into()));}
                });}
            });
        }
        if ended || operator.is_empty() { return; }
        let joined = mine.is_some_and(|r| r["state"] == "active");
        if draft.is_none() {
            if ui.add_enabled(!busy, egui::Button::new(if joined {"Edit my part…"} else {"Choose my part…"})).clicked() {
                *draft = Some(Draft { work:work.into(), operator:operator.into(), brief:brief.into(), base:base.map(str::to_owned),
                    role:if joined && mine.is_some_and(|r| r["role"] == "owner") {Role::Owner} else {Role::Contributor},
                    note:mine.filter(|_| joined).map_or("", |r| s(r,"note")).into() });
            }
        } else if let Some(d) = draft.as_mut() {
            ui.separator();
            ui.strong(format!("My part · {operator}"));
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut d.role, Role::Contributor, "Contribute");
                ui.add_enabled_ui(!busy_owner, |ui| { ui.selectable_value(&mut d.role, Role::Owner, "Own task"); });
                if joined { ui.selectable_value(&mut d.role, Role::Leave, "Leave role"); }
            });
            ui.label(match d.role {
                Role::Owner => "What follow-through are you taking on for the task?",
                Role::Contributor => "What are you contributing? You don’t have to own the task.",
                Role::Leave => "What are you leaving unfinished or handing back?",
            });
            ui.add(egui::TextEdit::multiline(&mut d.note).desired_width(f32::INFINITY).desired_rows(2));
            ui.small("Visible to the personas. This does not change an outside request or accept a delivery.");
            let current = d.brief == brief && d.base.as_deref() == base;
            if !current { ui.colored_label(theme::ACCENT, "The brief or your role changed. Cancel and review the current edition before saving."); }
            let mut dismiss = false;
            ui.horizontal_wrapped(|ui| {
                let valid = current && !busy && !d.note.trim().is_empty() && (d.role != Role::Owner || !busy_owner);
                if ui.add_enabled(valid, theme::primary(if d.role == Role::Leave {"Leave this role"} else {"Save my part"})).clicked() {
                    action = Some(Action::SetRole(Command::TaskRole { work:d.work.clone(), role:d.role, note:d.note.trim().into(), brief:d.brief.clone(), base:d.base.clone() }));
                    dismiss = true;
                }
                dismiss |= ui.small_button("Cancel").clicked();
            });
            if dismiss { *draft = None; }
        }
    });
    ui.add_space(8.);
    action
}
