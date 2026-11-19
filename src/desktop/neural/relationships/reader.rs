//! Show the selected evidence in place using Studio's verified event journal.
use super::*;

pub(super) fn event<'a>(events: &'a [Value], reference: &str) -> Option<&'a Value> {
    let sequence = reference
        .strip_prefix("experience:event-")?
        .strip_suffix("@1")?
        .parse::<i64>()
        .ok()?;
    let position = events
        .binary_search_by_key(&sequence, |e| e["event"].as_i64().unwrap_or(-1))
        .ok()?;
    Some(&events[position])
}

pub(super) fn show(
    ctx: &egui::Context,
    status: &Value,
    events: &[Value],
    stack: &mut Vec<String>,
) -> Option<Action> {
    let reference = stack.last()?;
    let e = event(events, reference)?;
    let body = &e["body"];
    let actor = s(e, "actor");
    let sender = match (e["invocation"].is_null(), s(e, "kind")) {
        (true, "asset_feedback" | "package_review") => format!(
            "{} · operator",
            actor.strip_prefix("operator:").unwrap_or(actor)
        ),
        (true, "invitation" | "user_message" | "user_acceptance" | "requirements_amended") => {
            format!("{actor} · operator")
        }
        (true, _) => actor.to_owned(),
        (false, _) => name(status, actor).to_owned(),
    };
    let target = match s(e, "target") {
        "*" => "Activity",
        "user" => "User inbox",
        id => name(status, id),
    };
    let mut visible = true;
    let mut action = None;
    let mut back = false;
    egui::Window::new("Recorded exchange")
        .open(&mut visible)
        .default_width(560.)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, &title(s(e, "kind")), TEAL);
                ui.small(s(e, "created_utc"));
            });
            ui.add_space(6.);
            ui.heading(format!("{sender}  →  {target}"));
            let work = work_name(status, s(e, "work"));
            ui.label(excerpt(work, 105)).on_hover_text(work);
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(320.)
                .id_salt(("exchange-text", reference))
                .show(ui, |ui| {
                    if let Some(text) = body["text"].as_str() {
                        ui.add(
                            egui::Label::new(RichText::new(text).size(16.))
                                .selectable(true)
                                .wrap(),
                        );
                    } else {
                        let mut rendered = false;
                        for (label, value) in [
                            ("Contribution", &body["contribution"]),
                            ("Reason", &body["reason"]),
                            ("Finding", &body["finding"]),
                            ("Description", &body["description"]),
                            ("Commitment", &body["commitment"]),
                            ("Human review", &body["decision"]["reason"]),
                        ] {
                            if let Some(text) = value.as_str() {
                                eyebrow(ui, label);
                                ui.add(egui::Label::new(text).selectable(true).wrap());
                                rendered = true;
                            }
                        }
                        if !rendered {
                            if let Ok(json) = serde_json::to_string_pretty(body) {
                                ui.add(
                                    egui::Label::new(RichText::new(json).monospace())
                                        .selectable(true)
                                        .wrap(),
                                );
                            }
                        }
                    }
                });
            if let Some(parent) = body["reply_to"].as_str() {
                if ui
                    .small_button("Read the earlier exchange")
                    .on_hover_text(parent)
                    .clicked()
                {
                    action = Some(Action::Inspect(parent.into()));
                }
            }
            for source in items(body, "references").filter_map(Value::as_str) {
                let (kind, detail) = source.split_once(':').unwrap_or(("source", source));
                if ui
                    .small_button(format!("{} · {}", title(kind), excerpt(detail, 32)))
                    .on_hover_text(source)
                    .clicked()
                {
                    action = Some(Action::Inspect(source.into()));
                }
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if stack.len() > 1 {
                    back = ui.small_button("← Back").clicked();
                }
                if ui
                    .small_button("Open full record ↗")
                    .on_hover_text(reference)
                    .clicked()
                {
                    action = Some(Action::Raw(reference.clone()));
                }
            });
        });
    if back {
        stack.pop();
    }
    if !visible {
        stack.clear();
    }
    action
}

pub(super) fn markers(ui: &mut Ui, history: &Value, action: &mut Option<Action>) {
    let Some(entries) = history["encounters"].as_array().filter(|e| !e.is_empty()) else {
        return;
    };
    ui.small(if history["offset"] == 0 {
        "Recent"
    } else {
        "On this page"
    });
    for row in entries.iter().rev() {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(17., 18.), egui::Sense::click());
        let p = rect.center();
        let (color, direction, sign) = match s(row, "direction") {
            "sent" => (TEAL, "Sent", 1.),
            "received" => (PURPLE, "Received", -1.),
            _ => (theme::palette(ui).muted, "Activity-wide", 0.),
        };
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, 3., theme::palette(ui).hover);
        }
        if sign == 0. {
            ui.painter().rect_stroke(
                egui::Rect::from_center_size(p, egui::vec2(8., 8.)),
                2.,
                Stroke::new(1.4, color),
                egui::StrokeKind::Inside,
            );
        } else {
            ui.painter().line_segment(
                [
                    p + egui::vec2(-5. * sign, 0.),
                    p + egui::vec2(4. * sign, 0.),
                ],
                Stroke::new(1.7, color),
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    p + egui::vec2(6. * sign, 0.),
                    p + egui::vec2(0., -4.),
                    p + egui::vec2(0., 4.),
                ],
                color,
                Stroke::NONE,
            ));
        }
        let label = format!(
            "{direction} · {} · {}",
            title(s(row, "kind")),
            s(row, "created_utc")
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
        });
        if response.on_hover_text(label).clicked() {
            *action = Some(Action::Inspect(s(row, "reference").into()));
        }
    }
}
