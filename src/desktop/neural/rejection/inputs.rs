use super::*;

fn inbox(ui: &mut Ui, summary: &Value) {
    ui.strong("Inbox entries");
    if let Some(gap) = summary["unavailable"].as_str() {
        ui.weak(gap);
        return;
    }
    for (key, label) in [
        ("repeated", "Repeated unchanged"),
        ("only_later", "Only in the later input"),
        ("changed_entry", "Same number, different entry"),
        ("only_first", "Only in the earlier input"),
    ] {
        let Some(events) = summary[key].as_array() else {
            continue;
        };
        if events.is_empty() && matches!(key, "changed_entry" | "only_first") {
            continue;
        }
        let ids: Vec<_> = events
            .iter()
            .filter_map(Value::as_i64)
            .map(|n| format!("#{n}"))
            .collect();
        let list = if ids.is_empty() {
            "None".into()
        } else {
            ids.join(", ")
        };
        ui.label(format!("{label}: {list}"));
    }
    ui.small("A repeated entry is not another send or execution. This compares the numbered inbox entries, not all files or images.");
}

fn working_note(ui: &mut Ui, comparison: &Value) {
    ui.strong("Working note in the later input");
    ui.small(match s(comparison, "working_note_comparison") {
        "same_text" => "Same text as in the earlier input",
        "different_text" => "Different text from the earlier input",
        "neither_has_previous_choice" => "Neither input names a previous work decision",
        "only_later" => "The earlier input had no previous work decision",
        "only_first" => "The later input had no previous work decision",
        _ => "Working-note comparison unavailable",
    });
    if let Some(text) = comparison["later_working_note"].as_str() {
        if text.is_empty() {
            ui.weak("Empty working note");
        } else {
            ui.add(egui::Label::new(text).wrap().selectable(true));
        }
    }
}

/// Show the later input's ordered selection, not operations proposed or run.
fn selection(ui: &mut Ui, label: &str, choices: &Value) {
    let Some(items) = choices["later"].as_array() else {
        ui.weak(format!("{label}: later selection unavailable"));
        return;
    };
    let change = match choices["same_as_earlier"].as_bool() {
        Some(true) => "unchanged",
        Some(false) => "different from earlier",
        None => "earlier selection unavailable",
    };
    let heading = format!("{label}: {} · {change}", items.len());
    if items.is_empty() {
        ui.label(heading);
    } else {
        egui::CollapsingHeader::new(heading)
            .id_salt(label)
            .show(ui, |ui| {
                for item in items {
                    let text = item
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| item.to_string());
                    ui.add(egui::Label::new(text).wrap().selectable(true));
                }
            });
    }
}

pub(super) fn show(ui: &mut Ui, row: &Value, seq: i64) -> bool {
    let c = &row["compared_inputs"];
    egui::CollapsingHeader::new(format!("Compare inputs across rejection #{seq}"))
        .id_salt(("refusal-inputs", seq, s(row, "invocation")))
        .show(ui, |ui| {
            inbox(ui, &c["inbox"]);
            ui.add_space(5.);
            working_note(ui, c);
            selection(ui, "Selected references", &c["references"]);
            selection(ui, "Selected tools", &c["tools"]);
            ui.link("Open later frozen input ↗")
                .on_hover_text(s(row, "input"))
                .clicked()
        })
        .body_returned
        .unwrap_or(false)
}
