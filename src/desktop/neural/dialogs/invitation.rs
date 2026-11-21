use super::*;
use ai_personas::neural::live::{RunScope, RunnerObservation};

pub(super) fn request_key(
    command: &Command,
    prior: &mut Option<(String, String)>,
) -> ai_personas::neural::Result<String> {
    let hash = ai_personas::store::sha256_bytes(&serde_json::to_vec(command)?);
    if let Some((_, key)) = prior.as_ref().filter(|(old, _)| old == &hash) {
        return Ok(key.clone());
    }
    let key = ai_personas::neural::id("invitation")?;
    *prior = Some((hash, key.clone()));
    Ok(key)
}

/// Some actions submit to a listener already in place. Saving an invitation
/// never creates another executor or clears a pause.
pub(super) fn form(
    app: &App,
    ui: &mut Ui,
    social: bool,
    brief: &mut String,
    invitees: &mut BTreeSet<String>,
    sources: &mut SourceFiles,
) -> Option<bool> {
    let body = |ui: &mut Ui| {
        let p = theme::palette(ui);
        egui::Frame::new().fill(p.header).corner_radius(7).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        eyebrow(ui, "Persona Studio  /  Invitation");
        ui.label(RichText::new(if social { "Start a conversation" } else { "Give the society a brief" }).size(22.).strong());
        ui.label(if social { "An opening topic is optional. Each persona decides whether and how to join." }
            else { "Describe what you want to receive. Each persona chooses its contribution, tools and collaborators." });
    });
        ui.add_space(6.);
        ui.strong(if social {
            "Opening topic"
        } else {
            "Requested result"
        });
        multiline(
            ui,
            brief,
            if social {
                "Leave open for participants to choose…"
            } else {
                "What should you be able to use when the work is done?"
            },
        );
        sources.picker.show(ui, &mut sources.files);
        ui.add_space(7.);
        ui.horizontal_wrapped(|ui| {
            ui.strong("Invite personas");
            ui.weak(format!("{} selected", invitees.len()));
        });
        for person in app.records("personas") {
            let body = &person["body"];
            let id = s(body, "id");
            let label = s(body, "label");
            let mut chosen = invitees.contains(id);
            ui.push_id(id, |ui| {
                let tint = p.ink(app.color(id));
                ui.horizontal_wrapped(|ui| {
                    let portrait = avatar(ui, label, tint, 24.);
                    if portrait.on_hover_text(label).clicked() {
                        chosen = !chosen;
                    }
                    ui.checkbox(&mut chosen, RichText::new(label).strong());
                    let epoch = body["epoch"].as_u64().unwrap_or(0);
                    ui.weak(
                        RichText::new(if epoch == 0 {
                            "Starting profile".into()
                        } else {
                            format!("Character epoch {epoch}")
                        })
                        .small(),
                    );
                });
            });
            if chosen {
                invitees.insert(id.into());
            } else {
                invitees.remove(id);
            }
        }
    };
    if ui.ctx().content_rect().height() >= 370. {
        let height = (ui.available_height() - 82.)
            .clamp(82., (ui.ctx().content_rect().height() - 185.).max(82.));
        egui::ScrollArea::vertical()
            .id_salt("invitation-body")
            .max_height(height)
            .auto_shrink([false, true])
            .show(ui, body);
    } else {
        let mut body = body;
        body(ui);
    }
    ui.separator();
    let held = app
        .records("controls")
        .iter()
        .any(|c| matches!(s(c, "scope"), "global" | "admission") && c["state"] != "running");
    let listening = matches!(&app.runner_observation, RunnerObservation::Observed {session, finish_requested_by:None, ..}
        if session.watch && session.scope == RunScope::Workspace)
        && !app
            .following
            .as_ref()
            .is_some_and(|(_, f)| f.load(std::sync::atomic::Ordering::Acquire));
    let can_start = !held
        && app.can_start_run()
        && matches!(app.runner_observation, RunnerObservation::Unobserved);
    let (action, detail) = if held {
        (
            "Queue invitations",
            "Scheduling is on hold. Saving the invitations keeps that control in effect.",
        )
    } else if listening {
        (
            "Send invitations",
            "The workspace is listening. Eligible invitees can respond as soon as they are free.",
        )
    } else if can_start {
        ("Invite & run workspace", "The workspace will follow eligible activity and incoming replies while Studio is open.")
    } else {
        (
            "Save invitations",
            "Invitations are saved to the activity. Run the workspace to follow all eligible work.",
        )
    };
    ui.label(
        RichText::new(detail)
            .small()
            .color(theme::palette(ui).muted),
    );
    let eligible =
        !sources.picker.busy() && !invitees.is_empty() && (social || !brief.trim().is_empty());
    let mut submit = None;
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(eligible, theme::primary(action)).clicked() {
            submit = Some(can_start);
        }
        if can_start
            && ui
                .add_enabled(eligible, egui::Button::new("Save without starting"))
                .clicked()
        {
            submit = Some(false);
        }
    });
    submit
}
