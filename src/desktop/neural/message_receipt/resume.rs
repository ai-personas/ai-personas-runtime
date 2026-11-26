//! A read-only trail from an operator's request to the participant's own choice.
use super::*;

pub(super) enum Follow {
    Journal(String),
    Rejection(i64),
}

pub(super) struct Request {
    work: String,
    recipient: String,
    operator: String,
    source: String,
    event: i64,
    opening: Option<Value>,
    changed: bool,
}

impl Request {
    pub(super) fn at(e: &Value) -> Option<Self> {
        if e["kind"] != "explicit_resume" || !e["invocation"].is_null() {
            return None;
        }
        Some(Self {
            work: e["work"].as_str()?.into(),
            recipient: e["target"].as_str()?.into(),
            operator: e["actor"].as_str()?.into(),
            source: format!("object:{}", e["digest"].as_str()?),
            event: e["event"].as_i64()?,
            opening: None,
            changed: false,
        })
    }

    /// Navigation re-reads both the selected notice and its attempted/adopted
    /// decision. New, unrelated activity need not invalidate the link.
    pub(super) fn checked(&mut self, result: &mut Result<Value, String>) -> Option<Follow> {
        if result.as_ref().is_ok_and(|r| {
            r["kind"] != "decision_request"
                || r["event"] != self.event
                || r["source"] != self.source
                || r["work"] != self.work
                || r["recipient"] != self.recipient
                || r["operator"] != self.operator
        }) {
            *result = Err("The decision request changed. Reopen it from its activity.".into());
        }
        let expected = self.opening.take()?;
        if !result.as_ref().is_ok_and(|r| {
            r["attempts"]
                .as_array()
                .is_some_and(|a| a.contains(&expected))
        }) {
            self.changed = true;
            return None;
        }
        if let Some(d) = expected["decision"].as_str() {
            Some(Follow::Journal(d.into()))
        } else {
            expected["rejection_event"].as_i64().map(Follow::Rejection)
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, r: &Value, name: &str, loading: bool) -> bool {
        let p = theme::palette(ui);
        eyebrow(ui, &format!("ACTIVITY   /   REQUEST #{}", self.event));
        ui.heading(format!("Decision requested from {name}"));
        ui.small(format!(
            "{} · {} · Brief {}",
            self.operator,
            time_label(r["created"].as_i64().unwrap_or_default()),
            edition(&r["brief"])
        ));
        ui.add_space(6.);
        ui.add(egui::Label::new(s(r, "reason")).selectable(true).wrap());
        let previous = &r["preserved"];
        egui::CollapsingHeader::new(format!(
            "Preserved {} · edition {}",
            s(previous, "kind"),
            edition(&r["previous_participation"])
        ))
        .id_salt((self.event, "preserved-choice"))
        .show(ui, |ui| {
            ui.label(s(previous, "reason"));
            ui.small(format!(
                "Participation edition {} registered the request without changing that choice.",
                edition(&r["registered_participation"])
            ))
            .on_hover_text(s(r, "registered_participation"));
        });
        ui.separator();
        let attempts = r["attempts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        ui.strong("What followed");
        ui.small("Calls on this request, in order. For later choices, see the decision journal.");
        if r["permission_pending"] == true {
            theme::badge(ui, "Request pending", p.ink(PURPLE));
            ui.label("Permission for another decision remains. This does not start a runner or override a hold.");
        }
        if attempts.is_empty() {
            ui.weak(if r["permission_pending"] == true {
                "No attempt found on this request."
            } else {
                "Request is no longer pending. No decision on it was verified."
            });
        }
        let mut refresh = false;
        for (i, a) in attempts.iter().enumerate() {
            ui.push_id(i, |ui| {
                ui.add_space(6.);
                if let Some(error) = a["unavailable"].as_str() {
                    ui.strong(format!("{}. Evidence unavailable", i + 1));
                    ui.label(error);
                    return;
                }
                let c = &a["continuation"];
                let what = match (s(a, "state"), s(c, "kind")) {
                    ("accepted", "ready") => "Chose to continue",
                    ("accepted", "wait") => "Chose to wait",
                    ("accepted", "stop") => "Chose to stop",
                    ("accepted", _) => "Character initialization",
                    ("rejected", _) => "Response rejected",
                    ("responded", _) => "Response awaiting adoption",
                    ("canceled_response", _) => "Response returned · canceled without adoption",
                    ("interrupted", _) if a["binding"].is_string() => {
                        "Response returned · interrupted before adoption"
                    }
                    _ => "Attempt recorded",
                };
                let color = if a["decision"].is_string() {
                    p.ink(TEAL)
                } else {
                    p.muted
                };
                ui.label(
                    RichText::new(format!("{}. {what}", i + 1))
                        .size(16.)
                        .strong()
                        .color(color),
                )
                .on_hover_text(s(a, "invocation"));
                let verified = a["binding"].is_string();
                let input = &a["input"];
                let words = match (s(input, "notice"), verified) {
                    ("reason", true) => "Request text supplied in this call",
                    ("metadata", true) => {
                        "Recovery input carried the request address, not its text"
                    }
                    ("withheld_for_initialization", _) => {
                        "Work request withheld during initialization"
                    }
                    ("metadata", false) => "Request address in prepared input",
                    _ => "Request text in prepared input",
                };
                ui.small(format!("{words} · Brief {}", edition(&input["brief"])))
                    .on_hover_text(s(a, "snapshot"));
                if let Some(note) = c["purpose"].as_str().or(c["reason"].as_str()) {
                    ui.label(note);
                }
                if let Some(tools) = a["tools_chosen"].as_array().filter(|t| !t.is_empty()) {
                    let names = tools
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.small(format!("Tools chosen: {names}")).on_hover_text(
                        "These are choices in the response, not execution receipts.",
                    );
                }
                let label = if let Some(n) = a["rejection_event"].as_i64() {
                    ui.label(s(a, "reason"));
                    Some(format!("Inspect rejection #{n}"))
                } else if a["decision"].is_string() {
                    Some("Open accepted decision".into())
                } else {
                    None
                };
                if let Some(label) = label {
                    if ui
                        .add_enabled(!loading, egui::Button::new(label).small())
                        .clicked()
                    {
                        self.opening = Some(a.clone());
                        self.changed = false;
                        refresh = true;
                    }
                } else if !verified {
                    ui.label(format!(
                        "State: {}. No adopted choice is verified here.",
                        s(a, "state").replace('_', " ")
                    ));
                }
            });
        }
        if let Some(n) = r["unplaced_calls"].as_u64().filter(|n| *n > 0) {
            ui.weak(format!("Calls with unknown input and completion order in this participation: {n}. They could not be assigned to a request."));
        }
        if self.changed {
            ui.label("The decision link could not be verified again. Nothing was opened.");
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("Checked through event #{}", r["through"]));
            refresh |= ui
                .add_enabled(!loading, egui::Button::new("Refresh receipt").small())
                .clicked();
            if loading {
                ui.spinner();
            }
        });
        refresh
    }
}

fn edition(v: &Value) -> &str {
    v.as_str()
        .and_then(|s| s.rsplit_once('@'))
        .map(|(_, n)| n)
        .unwrap_or("?")
}
