use super::*;

pub(super) fn show(
    ui: &mut Ui,
    report: &Value,
    operator: &str,
    busy: bool,
    saving: bool,
    drafts: &mut BTreeMap<String, RoleDraft>,
) -> Option<Action> {
    let work = s(report, "work");
    let brief = s(report, "brief");
    let ended = report["phase"] == "dissolved";
    let roles = items(report, "human_roles");
    let active = |r: &&Value| r["state"] == "active";
    let owner = roles.iter().filter(active).find(|r| r["role"] == "owner");
    let now = role_draft::Terms {
        brief, body: &report["brief_terms"], mine: roles.iter().find(|r| r["operator"] == operator),
        other_owner: owner.is_some_and(|r| r["operator"] != operator),
    };
    let mut action = None;
    if drafts.get(work).is_some_and(|d| d.operator != operator) {
        drafts.remove(work);
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
                if ui.small_button("Who received this?").clicked() {
                    action = Some(Action::RoleReceipts(s(r,"reference").into()));
                }
            });
            ui.label(s(r,"note"));
            if let Some(filed) = r["brief"].as_str().filter(|filed| *filed != brief) {
                if ui.link(format!("Declared under brief {} ↗", brief_terms::revision(filed))).on_hover_text(filed).clicked() {
                    action = Some(Action::Inspect(filed.into()));
                }
            }
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
        if operator.is_empty() { return; }
        if let Some(d) = drafts.get_mut(work) {
            if d.show(ui, &now, busy, saving, &mut action) { drafts.remove(work); }
        } else if !ended && ui.add_enabled(!busy, egui::Button::new(if now.joined() {"Edit my part…"} else {"Choose my part…"})).clicked() {
            drafts.insert(work.into(), RoleDraft::open(work, operator, &now));
        }
    });
    ui.add_space(8.);
    action
}
