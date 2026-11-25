//! The source of an invitation is not the author of the resulting commitment.
use super::*;

pub(super) struct Panel {
    actor: String,
    pending: Option<Receiver<Result<Value, String>>>,
    result: Option<Result<Value, String>>,
}

impl Panel {
    fn open(ctx: &egui::Context, root: &Path, work: &str, actor: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        let (ctx, root, work, who) = (
            ctx.clone(),
            root.to_owned(),
            work.to_owned(),
            actor.to_owned(),
        );
        std::thread::spawn(move || {
            let page = data::open(&root)
                .and_then(|db| ai_personas::neural::live::participation_terms(&db, &work, &who));
            let _ = tx.send(page.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        Self {
            actor: actor.into(),
            pending: Some(rx),
            result: None,
        }
    }

    fn receive(&mut self) {
        let Some(rx) = &self.pending else { return };
        self.result = Some(match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Participation reader closed".into()),
        });
        self.pending = None;
    }
}

fn link(ui: &mut Ui, label: &str, address: &Value, action: &mut Option<Action>) {
    if let Some(r) = address.as_str() {
        if ui.small_button(label).on_hover_text(r).clicked() {
            *action = Some(Action::Inspect(r.into()));
        }
    }
}

fn entry(ui: &mut Ui, e: &Value, members: &[Value], subject: &str, action: &mut Option<Action>) {
    let kind = s(e, "kind");
    let b = &e["record"];
    let (label, ink) = match kind {
        "invitation" if e["previous_participation"].is_string() => ("Invited again", PURPLE),
        "invitation" => ("Posted invitation", PURPLE),
        "contribution_proposal" => ("Additional proposal", PURPLE),
        "invitation_response" if b["accept"] == true => ("Accepted contribution", TEAL),
        "invitation_response" => ("Declined invitation", theme::ACCENT),
        "commitment_changed" => ("Revised commitment", TEAL),
        "participation_left" => ("Departure notice", theme::ACCENT),
        _ => ("Terms unavailable", theme::ACCENT),
    };
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        theme::badge(ui, label, ink);
        if let Some(author) = e["author"].as_str() {
            if e["author_kind"] == "operator" {
                ui.label(format!("Operator · {author}"));
            } else {
                ui.label(format!("Persona · {}", person(members, author)))
                    .on_hover_text(author);
            }
        } else if matches!(
            kind,
            "invitation_response" | "commitment_changed" | "participation_left"
        ) {
            ui.label(person(members, subject)).on_hover_text(subject);
        }
        link(ui, &format!("#{} ↗", e["event"]), &e["reference"], action);
        if e["decision"].is_string() {
            let proof = match s(e, "binding") {
                "provider_output" => "Matched the original provider final and ordered input",
                "provider_completion" => "Matched the provider completion and ordered input",
                _ => "Matched the retained accepted choice and ordered input",
            };
            if ui
                .small_button("Choice ↗")
                .on_hover_text(format!("{proof}\n{}", s(e, "decision")))
                .clicked()
            {
                *action = Some(Action::Inspect(s(e, "decision").into()));
            }
            link(ui, "Result ↗", &e["result"], action);
        }
    });
    if let Some(offer) = e["contribution"].as_str() {
        ui.label(offer);
    }
    if e["previous_participation"].is_string() {
        link(
            ui,
            "Before this invitation ↗",
            &e["previous_participation"],
            action,
        );
    }
    if e["existing_participation"].is_string() {
        ui.horizontal_wrapped(|ui| {
            ui.small("No new acceptance. The recorded continuation was unchanged.");
            link(
                ui,
                "Existing participation ↗",
                &e["existing_participation"],
                action,
            );
        });
    }
    if kind == "unavailable" {
        ui.label(s(e, "reason"));
        return;
    }
    if let Some(text) = b["commitment"]
        .as_str()
        .filter(|t| !t.is_empty() && b["accept"] != false)
    {
        ui.label(text);
    }
    if kind == "commitment_changed" {
        link(ui, "Previous wording ↗", &b["previous"], action);
    }
    if let Some(reason) = b["reason"].as_str() {
        ui.small(format!("Reason · {reason}"));
    }
    for seq in items(e, "unmatched_incoming") {
        ui.small(format!(
            "Could not match incoming offer #{seq} to its posted record."
        ));
    }
    let offered = items(e, "incoming_offers");
    if !offered.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.small("Incoming with this choice:");
            for r in offered {
                if let Some(n) = r
                    .as_str()
                    .and_then(|s| s.strip_prefix("experience:event-"))
                    .and_then(|s| s.strip_suffix("@1"))
                {
                    link(ui, &format!("#{n} ↗"), r, action);
                }
            }
        });
    }
}

pub(super) fn show(
    ui: &mut Ui,
    root: &Path,
    work: &str,
    cursor: i64,
    members: &[Value],
    panel: &mut Option<Panel>,
) -> Option<Action> {
    let mut action = None;
    let mut select = None;
    ui.small("Persona participation · Select a name to inspect terms");
    ui.horizontal_wrapped(|ui| {
        for m in members {
            let actor = s(m, "actor");
            let status = if m["pending_resume"].is_object() {
                "resume requested"
            } else if m["state"] == "accepted" {
                match s(&m["continuation"], "kind") {
                    "stop" => "accepted · stopped",
                    "wait" => "accepted · waiting",
                    _ => "accepted",
                }
            } else {
                s(m, "state")
            };
            let on = panel.as_ref().is_some_and(|p| p.actor == actor);
            if ui
                .add(
                    egui::Button::new(format!("{} · {status}", person(members, actor)))
                        .selected(on),
                )
                .on_hover_text(
                    "Read posted offers, the persona’s choices and its recorded continuation",
                )
                .clicked()
            {
                select = Some(if on { None } else { Some(actor) });
            }
        }
    });
    if let Some(person) = select {
        *panel = person.map(|actor| Panel::open(ui.ctx(), root, work, actor));
    }
    let p = panel.as_mut()?;
    let member = members.iter().find(|m| m["actor"] == p.actor)?;
    p.receive();
    let mut refresh = false;
    let mut close = false;
    card(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        eyebrow(ui, "Participation  /  Posted terms");
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(person(members, &p.actor)).size(20.).strong())
                .on_hover_text(&p.actor);
            link(
                ui,
                "Current participation ↗",
                &member["participation"],
                &mut action,
            );
            close = ui.small_button("Close terms").clicked();
        });
        if member["state"] == "accepted" {
            let c = &member["continuation"];
            if let Some(reason) = c["purpose"].as_str().or(c["reason"].as_str()) {
                ui.collapsing(format!("Recorded continuation · {}", s(c, "kind")), |ui| {
                    ui.label(reason);
                    if c["kind"] == "wait" {
                        ui.horizontal_wrapped(|ui| {
                            ui.small("Wake on:");
                            let triggers = items(c, "on_events");
                            if triggers.is_empty() {
                                ui.label("Any new arrival");
                            }
                            for (index, trigger) in
                                triggers.iter().filter_map(Value::as_str).enumerate()
                            {
                                if index != 0 {
                                    ui.weak("or");
                                }
                                if trigger.starts_with("human_request:") {
                                    if ui
                                        .small_button("Outside step changes ↗")
                                        .on_hover_text(trigger)
                                        .clicked()
                                    {
                                        action = Some(Action::Inspect(trigger.into()));
                                    }
                                } else {
                                    ui.label(trigger.replace('_', " "));
                                }
                            }
                            if let Some(time) = c["wake_at_unix"].as_i64() {
                                ui.label(format!("or {}", time_label(time)));
                            }
                        });
                    }
                });
            }
        }
        ui.small("An offered part is not a commitment. Neither establishes that work was done.");
        match &p.result {
            None => {
                ui.spinner();
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                refresh = ui.small_button("Retry terms").clicked();
            }
            Some(Ok(page)) => {
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!("Journal through #{}", page["through_event"]));
                    let newer = page["through_event"].as_i64().is_some_and(|n| n < cursor)
                        || member["participation"] != page["participation"];
                    if newer {
                        ui.small("Activity has changed");
                        if member["participation"] != page["participation"] {
                            link(ui, "At snapshot ↗", &page["participation"], &mut action);
                        }
                    }
                    refresh = ui.small_button("Refresh terms").clicked();
                });
                if items(page, "entries").is_empty() {
                    ui.weak("No scope notices or results indexed for this participation.");
                }
                for e in items(page, "entries") {
                    ui.push_id(e["event"].as_i64(), |ui| {
                        entry(ui, e, members, &p.actor, &mut action)
                    });
                }
            }
        }
    });
    if close {
        *panel = None;
    } else if refresh {
        *panel = Some(Panel::open(ui.ctx(), root, work, &p.actor));
    }
    action
}
