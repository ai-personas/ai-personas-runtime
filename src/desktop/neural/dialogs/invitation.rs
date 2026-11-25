use super::*;
use ai_personas::neural::live::{RunScope, RunnerObservation};

#[derive(Default)]
pub(in crate::desktop::neural) struct Roster {
    pub(super) selected: BTreeSet<String>,
    query: String,
    focus_search: bool,
}

impl Roster {
    fn show(&mut self, ui: &mut Ui, app: &App) {
        ui.strong("Invite personas");
        let search = ui.add(egui::TextEdit::singleline(&mut self.query)
            .hint_text("Find a name or starting profile…")
            .desired_width(f32::INFINITY));
        if std::mem::take(&mut self.focus_search) {
            search.request_focus();
            search.scroll_to_me(Some(egui::Align::Min));
        }
        let query = self.query.trim().to_lowercase();
        let people: Vec<_> = app.records("personas").iter().map(|r| &r["body"])
            .filter(|p| query.is_empty() || ["id", "label", "seed"].iter()
                .any(|k| s(p, k).to_lowercase().contains(&query)))
            .collect();
        ui.horizontal_wrapped(|ui| {
            let add = if query.is_empty() { "Select all" } else { "Select matches" };
            if ui.add_enabled(people.iter().any(|p| !self.selected.contains(s(p, "id"))),
                egui::Button::new(add).small()).clicked() {
                // A concrete set of IDs at click time, never a live invitation rule.
                self.selected.extend(people.iter().map(|p| s(p, "id").to_owned()));
            }
            if ui.add_enabled(!self.selected.is_empty(), egui::Button::new("Clear selection").small()).clicked() {
                self.selected.clear();
            }
            if !query.is_empty() && ui.small_button("Show everyone").clicked() {
                self.query.clear();
            }
        });
        if people.is_empty() {
            ui.weak("No matching personas.");
        }
        for person in people {
            let id = s(person, "id");
            let label = s(person, "label");
            let mut chosen = self.selected.contains(id);
            ui.push_id(id, |ui| {
                let tint = theme::palette(ui).ink(app.color(id));
                ui.horizontal_wrapped(|ui| {
                    if avatar(ui, label, tint, 24.).on_hover_text(label).clicked() {
                        chosen = !chosen;
                    }
                    ui.checkbox(&mut chosen, RichText::new(label).strong());
                    let epoch = person["epoch"].as_u64().unwrap_or(0);
                    let status = if epoch == 0 { "Starting profile".into() } else { format!("Character epoch {epoch}") };
                    ui.weak(RichText::new(status).small()).on_hover_ui(|ui| {
                        ui.strong(people::starting_profile_credit(person));
                        ui.label(s(person, "seed"));
                    });
                });
            });
            if chosen {
                self.selected.insert(id.to_owned());
            } else {
                self.selected.remove(id);
            }
        }
    }

    fn address_line(&mut self, ui: &mut Ui, app: &App) {
        let names: Vec<_> = self.selected.iter().map(|id| app.label(id)).collect();
        let summary = match names.as_slice() {
            [] => "No invitees yet".to_owned(),
            [one] => one.to_string(),
            _ => format!("{} invitees · {}", names.len(), names.join(", ")),
        };
        let detail = if names.is_empty() {
            "Choose who receives this invitation.".to_owned()
        } else {
            self.selected.iter().map(|id| format!("{} · {id}", app.label(id)))
                .collect::<Vec<_>>().join("\n")
        };
        ui.horizontal(|ui| {
            eyebrow(ui, "To");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let edit = if names.is_empty() { "Choose invitees…" } else { "Edit invitees…" };
                if ui.small_button(edit).clicked() {
                    self.focus_search = true;
                    ui.ctx().request_repaint();
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(summary).strong()).truncate())
                        .on_hover_text(detail);
                });
            });
        });
    }
}

/// Some actions submit to a listener already in place. Saving an invitation
/// never creates another executor or clears a pause.
pub(super) fn form(
    app: &App,
    ui: &mut Ui,
    social: bool,
    brief: &mut String,
    roster: &mut Roster,
    sources: &mut SourceFiles,
    edition: Option<&SourceEdition>,
) -> Option<bool> {
    let body = |ui: &mut Ui| {
        let p = theme::palette(ui);
        egui::Frame::new().fill(p.header).corner_radius(7).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
            if let Some(source) = edition {
                eyebrow(ui, "Starting edition");
                ui.add(egui::Label::new(RichText::new(&source.title).size(20.).strong()).wrap());
                ui.horizontal_wrapped(|ui| {
                    avatar(ui, app.label(&source.issuer), app.color(&source.issuer), 20.);
                    ui.label(format!("Issued by {}", app.label(&source.issuer)));
                    if let Some((_, rev)) = source.reference.rsplit_once('@') {
                        ui.weak(format!("· Revision {rev}")).on_hover_text(&source.reference);
                    }
                });
                ui.small("Invite someone to check or revise this edition in a new activity.");
            } else {
                eyebrow(ui, "Persona Studio  /  Invitation");
                ui.label(RichText::new(if social { "Start a conversation" } else { "Give the society a brief" }).size(22.).strong());
                ui.label(if social { "An opening topic is optional. Each persona decides whether and how to join." }
                    else { "Describe what you want to receive. Each persona chooses its contribution, tools and collaborators." });
            }
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
        roster.show(ui, app);
    };
    if ui.ctx().content_rect().height() >= 370. {
        let address_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let height = (ui.available_height() - 82. - address_height)
            .clamp(82., (ui.ctx().content_rect().height() - 185. - address_height).max(82.));
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
    roster.address_line(ui, app);
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
        !sources.picker.busy() && !roster.selected.is_empty() && (social || !brief.trim().is_empty());
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
