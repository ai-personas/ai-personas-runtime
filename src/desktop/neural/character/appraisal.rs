use super::{egui, theme, title, RichText, Ui, Value, TEAL};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Basis {
    Contextual,
    Baseline,
    Pending,
    Unknown,
    NoParticipation,
}

impl Basis {
    pub(super) fn observed(part: &Value, origin: Option<&Value>) -> Self {
        if !part.is_object() {
            return Self::NoParticipation;
        }
        match origin.and_then(|v| v["basis"].as_str()) {
            Some("contextual") => Self::Contextual,
            Some("baseline") => Self::Baseline,
            Some("pending") => Self::Pending,
            None if part["appraisal_use"].is_null() => Self::Baseline,
            _ => Self::Unknown,
        }
    }
}

fn statement(ui: &mut Ui, label: &'static str, text: &str, compact: bool) {
    let limit = if compact { 145 } else { 350 };
    // Long authored explanations stay readable in a narrow inspector. The expanded
    // view always retains the exact text, including the original line breaks.
    let cut = text
        .char_indices()
        .nth(limit)
        .map(|(i, _)| i)
        .into_iter()
        .chain(
            text.match_indices('\n')
                .nth(if compact { 2 } else { 5 })
                .map(|(i, _)| i),
        )
        .min();
    let id = ui.make_persistent_id(("authored-appraisal-text", label));
    let expanded = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let p = theme::palette(ui);
    let mut line = egui::text::LayoutJob::default();
    let caption = egui::TextFormat::simple(egui::TextStyle::Small.resolve(ui.style()), p.muted);
    let body = egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), p.text);
    line.append(label, 0., caption);
    if let Some(end) = cut.filter(|_| !expanded) {
        line.append(&format!("  {}…", &text[..end]), 0., body);
    } else {
        line.append("  ", 0., body.clone());
        line.append(text, 0., body);
    }
    ui.add(egui::Label::new(line).wrap());
    if cut.is_some() {
        let toggle = if expanded {
            "Show less"
        } else {
            "Read full text"
        };
        if ui.link(RichText::new(toggle).small()).clicked() {
            ui.data_mut(|data| data.insert_temp(id, !expanded));
        }
    }
}

fn boundaries(ui: &mut Ui, review: &Value) {
    let stamp = review["at_unix"].as_i64().map(|at| {
        review["at_utc"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Unix timestamp {at}"))
    });
    let kind = review["event_kind"].as_str();
    let reached = review["time_reached"] == true;
    match (stamp, kind) {
        (Some(stamp), Some(kind)) => {
            ui.small(format!("Chosen boundaries · {stamp} or {}", title(kind)))
                .on_hover_text(kind);
            if reached {
                ui.small("Time reached");
            }
        }
        (Some(stamp), None) => {
            let label = if reached {
                "Time reached"
            } else {
                "Chosen time"
            };
            ui.small(format!("{label} · {stamp}"));
        }
        (None, Some(kind)) => {
            let label = if review["due"] == false {
                "Awaiting event"
            } else {
                "Chosen event"
            };
            ui.small(format!("{label} · {}", title(kind)))
                .on_hover_text(kind);
        }
        (None, None) => {}
    }
}

fn resource(ui: &mut Ui, name: &str, reference: &str, opened: &mut Option<String>) {
    if ui
        .link(RichText::new(name).small())
        .on_hover_text(reference)
        .clicked()
    {
        *opened = Some(reference.to_owned());
    }
}

fn decision(exact: Option<&str>, invocation: Option<&str>) -> Option<String> {
    exact
        .map(str::to_owned)
        .or_else(|| invocation.map(|id| format!("experience:{id}@1")))
}

pub(super) fn panel(
    ui: &mut Ui,
    part: &Value,
    provenance: Option<&Value>,
    compact: bool,
    person_name: impl Fn(&str) -> String,
) -> Option<String> {
    let basis = Basis::observed(part, provenance);
    let origin = provenance.unwrap_or(&Value::Null);
    let reconsider = &origin["reconsideration"];
    let used = &part["appraisal_use"];
    let mut opened = None;
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(if compact { p.canvas } else { p.panel })
        .stroke(egui::Stroke::new(1., p.border))
        .corner_radius(6)
        .inner_margin(if compact { 9 } else { 12 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let (text, color) = match basis {
                Basis::Contextual => ("Selected appraisal", TEAL),
                Basis::Baseline => ("Baseline", p.accent),
                Basis::Pending => ("Model pending", p.muted),
                Basis::NoParticipation => ("No participation", p.muted),
                Basis::Unknown => ("Basis unavailable", p.muted),
            };
            let narrow = ui.available_width() < 425.;
            if narrow {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Next input · {text}"))
                            .strong()
                            .color(p.ink(color)),
                    )
                    .wrap(),
                );
                if reconsider["due"] == true {
                    ui.label(
                        RichText::new("Reconsideration due")
                            .small()
                            .color(p.ink(theme::ACCENT)),
                    );
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Next decision input");
                    theme::badge(ui, text, color);
                    if reconsider["due"] == true {
                        theme::badge(ui, "Reconsideration due", theme::ACCENT);
                    }
                });
            }
            if basis == Basis::Baseline && reconsider["due"] != true {
                ui.small("All appraisal inputs are unknown at baseline.");
            }
            if let Some(reason) = used["reason"].as_str() {
                statement(ui, "Selection reason", reason, compact);
            }
            if let Some(condition) = reconsider["condition"].as_str() {
                statement(ui, "Reconsider when", condition, compact);
                boundaries(ui, reconsider);
                let event = &reconsider["matched_event"];
                if let Some(reference) = event["experience"].as_str() {
                    let mut name = event["seq"].as_i64().map_or_else(
                        || "Matching event".to_owned(),
                        |n| format!("Matching event {n}"),
                    );
                    if let Some(kind) = event["kind"].as_str() {
                        name.push_str(&format!(" · {}", title(kind)));
                    }
                    if let Some(author) = event["author"].as_str() {
                        name.push_str(&format!(" from {}", person_name(author)));
                    }
                    if let Some(target) = event["target"].as_str() {
                        let recipient = if target == "*" {
                            "the activity".to_owned()
                        } else {
                            person_name(target)
                        };
                        name.push_str(&format!(" to {recipient}"));
                    }
                    resource(ui, &name, reference, &mut opened);
                }
            } else if let Some(condition) = origin["recorded_reconsideration"]["condition"].as_str()
            {
                egui::CollapsingHeader::new("Historical condition")
                    .id_salt("recorded-appraisal-condition")
                    .show(ui, |ui| {
                        statement(ui, "Recorded condition", condition, compact);
                        boundaries(ui, &origin["recorded_reconsideration"]);
                    });
            }
            let correspondence = &origin["correspondence"];
            let authored = decision(
                reconsider["origin_decision"]
                    .as_str()
                    .or_else(|| origin["origin_decision"].as_str()),
                origin["invocation"].as_str(),
            )
            .or_else(|| origin["response"].as_str().map(str::to_owned));
            let selected = decision(
                correspondence["selection_decision"].as_str(),
                used["decision"].as_str(),
            );
            if authored.is_some() || selected.is_some() {
                let show = |ui: &mut Ui| {
                    if let Some(reference) = &authored {
                        resource(ui, "Appraisal decision", reference, &mut opened);
                    }
                    if let Some(reference) =
                        selected.as_ref().filter(|r| Some(*r) != authored.as_ref())
                    {
                        resource(ui, "Selection decision", reference, &mut opened);
                    }
                };
                if narrow {
                    ui.vertical(show);
                } else {
                    ui.horizontal_wrapped(show);
                }
            }
            let changed = origin["changed_dependencies"]
                .as_array()
                .filter(|v| !v.is_empty());
            if let Some(changed) = changed {
                egui::CollapsingHeader::new(format!("Changed dependencies ({})", changed.len()))
                    .id_salt("appraisal-changed")
                    .show(ui, |ui| {
                        for reference in changed.iter().filter_map(Value::as_str) {
                            resource(ui, reference, reference, &mut opened);
                        }
                    });
            }
            if let Some(reference) = part["last_evaluation"].as_str() {
                egui::CollapsingHeader::new("Appraisal history")
                    .id_salt("appraisal-history")
                    .show(ui, |ui| {
                        resource(ui, "Recorded evaluation", reference, &mut opened);
                        if let Some(when) = origin["recorded_at_utc"].as_str() {
                            ui.small(when);
                        }
                        if let Some(n) = origin["accepted_decisions_since"].as_u64() {
                            ui.small(format!(
                                "{n} later {}",
                                if n == 1 { "decision" } else { "decisions" }
                            ));
                        }
                        if let Some(exchanges) = correspondence
                            ["latest_later_exchanges_by_correspondent"]
                            .as_array()
                            .filter(|v| !v.is_empty())
                        {
                            ui.small("Received after the selection");
                            for exchange in exchanges {
                                if let Some(reference) = exchange["experience"].as_str() {
                                    let name = format!(
                                        "{} · {}",
                                        person_name(exchange["author"].as_str().unwrap_or("")),
                                        title(exchange["kind"].as_str().unwrap_or("Exchange"))
                                    );
                                    resource(ui, &name, reference, &mut opened);
                                }
                            }
                        }
                    });
            }
        });
    opened
}
