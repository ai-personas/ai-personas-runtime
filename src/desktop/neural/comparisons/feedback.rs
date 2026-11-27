//! Show only the views in the shared situation. Opening a fold reads no file.
use super::*;

fn event(reference: &Value) -> String {
    reference
        .as_str()
        .and_then(|r| r.strip_prefix("experience:event-"))
        .and_then(|r| r.strip_suffix("@1"))
        .map_or_else(|| "Observation".into(), |n| format!("#{n}"))
}

pub(super) fn streams(ui: &mut Ui, result: &Value) {
    for name in ["stdout", "stderr"] {
        let view = &result[name];
        let bytes = view["bytes"].as_u64();
        let body = view["text"].as_str().or(view["excerpt"].as_str());
        let complete = body.is_some_and(|s| bytes == Some(s.len() as u64))
            && (view["excerpt_complete"] == true
                || (view["text"].is_string()
                    && view["range"][0].as_u64() == Some(0)
                    && view["range"][1].as_u64() == bytes));
        let scope = match (body, bytes) {
            (Some(_), Some(0)) if complete => "empty stream · 0 bytes".to_owned(),
            (_, Some(n)) if complete => format!("complete text · {} bytes", number(n)),
            (Some(s), Some(n)) => format!(
                "excerpt · {} of {} bytes",
                number(s.len() as u64),
                number(n)
            ),
            (Some(_), None) => "supplied text · total length unknown".into(),
            (None, Some(n)) => format!("{} bytes · no text supplied", number(n)),
            (None, None) if view["object"].is_string() => "reference only".into(),
            _ => "not supplied".into(),
        };
        let heading = format!("{name} · {scope}");
        if let Some(text) = body.filter(|s| !s.is_empty()) {
            ui.collapsing(heading, |ui| {
                ui.add(egui::Label::new(text).wrap().selectable(true));
                if let Some(r) = view["object"].as_str() {
                    ui.add(egui::Label::new(RichText::new(r).small().weak()).wrap());
                }
            });
        } else {
            let response = ui.small(heading);
            if let Some(r) = view["object"].as_str() {
                response.on_hover_text(r);
            }
            if let Some(reason) = view["text_unavailable"].as_str() {
                ui.small(reason);
            }
        }
    }
}

fn run(ui: &mut Ui, subject: &Value, status: &Value, program: &str) {
    let scope = match subject.get("dispatch") {
        Some(n) if n.is_number() => format!("Dispatch slip {}", n),
        _ => "Whole edition".to_owned(),
    };
    let mut details = vec![scope];
    if let Some(n) = status["files_staged"].as_u64() {
        details.push(format!("{} staged", counted(n, "file", "files")));
    }
    if !program.is_empty() {
        details.push(program.to_owned());
    }
    if status["process_started"] == false {
        details.push("Did not start".into());
    }
    if let Some(code) = status["exit_code"].as_i64() {
        details.push(format!("Exit {code}"));
    }
    for (field, label) in [("timed_out", "Timed out"), ("cancelled", "Cancelled")] {
        if status[field] == true {
            details.push(label.into());
        }
    }
    if status["success"] == false {
        details.push("Run unsuccessful".into());
    }
    ui.small(details.join(" · "));
    if let Some(error) = status["error"].as_str() {
        ui.label(error);
    }
    if let Some(purpose) = subject["purpose"].as_str() {
        ui.label(format!("Purpose: {purpose}"));
    }
    ui.collapsing("Edition & run details", |ui| {
        for (label, value) in [
            ("Edition", &subject["delivery"]), ("Checking activity", &status["checking_work"]),
            ("Environment", &status["receiving_environment"]), ("Receipt", &status["receipt"]),
        ] {
            if let Some(text) = value.as_str() {
                ui.add(egui::Label::new(format!("{label}: {text}")).wrap().selectable(true));
            }
        }
    });
}

pub(super) fn show(ui: &mut Ui, status: &Value, situation: &Value) {
    let commands = situation["local_command_results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let notices = situation["edition_feedback"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if notices.is_empty() && !commands.iter().any(|r| r["receiving"].is_object()) {
        return;
    }
    eyebrow(ui, "Package checks and findings");
    ui.weak("A run reports what the process did. A finding is a separate judgment about a stated criterion.");
    for result in commands.iter().filter(|r| {
        r["receiving"].is_object()
            && !notices
                .iter()
                .any(|n| n["same_run_as"].is_string() && n["same_run_as"] == r["experience"])
    }) {
        ui.push_id(&result["experience"].as_str(), |ui| {
            ui.strong(format!(
                "{} · {}’s command return",
                event(&result["experience"]),
                persona_label(status, s(result, "actor"))
            ));
            run(ui, &result["receiving"], result, s(result, "argv0"));
            streams(ui, result);
            ui.separator();
        });
    }
    for item in notices {
        ui.push_id(&item["experience"].as_str(), |ui| {
            let notice = &item["notice"];
            let author = persona_label(status, s(item, "from"));
            let destination = if item["to"] == "*" {
                "activity"
            } else {
                persona_label(status, s(item, "to"))
            };
            if item["kind"] == "delivery_check" {
                ui.strong(format!(
                    "{} · Receiving check · {author} → {destination}",
                    event(&item["experience"])
                ));
                run(ui, notice, notice, s(notice, "program"));
                if let Some(own) = commands.iter().find(|c| {
                    item["same_run_as"].is_string() && c["experience"] == item["same_run_as"]
                }) {
                    ui.label(format!(
                        "One run, two observations. Command return {}:",
                        event(&own["experience"])
                    ));
                    ui.push_id("own", |ui| streams(ui, own));
                }
                ui.small(format!("Output supplied with the notice to {destination}"));
                streams(ui, &item["streams"]);
            } else {
                ui.strong(format!(
                    "{} · Finding · {author} → {destination}",
                    event(&item["experience"])
                ));
                ui.label(s(notice, "criterion"));
                ui.small(format!("Verdict: {}", s(notice, "verdict")));
                if let Some(edition) = notice["delivery"].as_str() {
                    ui.label(format!("Edition named in the notice: {edition}"));
                }
                let subject = &item["reviewed_subject"];
                if let Some(edition) = subject["delivery"].as_str() {
                    ui.label(edition);
                } else if subject["kind"] == "proposed_files" {
                    ui.small("Subject: proposed files");
                } else if subject["status"] == "unavailable" {
                    ui.weak("The subject was marked unavailable in this input.");
                } else {
                    ui.weak("Subject details were not supplied with this notice.");
                }
            }
            ui.separator();
        });
    }
}
