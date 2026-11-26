//! Show what the author declared, never today's classification of a historical tool.
use super::*;

pub(super) fn show(ui: &mut Ui, row: &Value, invocation: &str) {
    let declaration = &row["asset_maintenance"];
    let Some(kind) = declaration["kind"].as_str() else {
        ui.weak("Resource maintenance declaration unavailable");
        return;
    };
    egui::CollapsingHeader::new(format!("Resource maintenance · {}", title(kind)))
        .id_salt(("declared-resources", invocation))
        .show(ui, |ui| {
            ui.label(
                declaration["reason"]
                    .as_str()
                    .unwrap_or("No readable reason in this response."),
            );
            let Some(ids) = declaration["operations"].as_array() else {
                if kind == "change" || declaration.get("operations").is_some() {
                    ui.weak("Local operation ID list unavailable");
                }
                return;
            };
            if ids.is_empty() {
                ui.weak("No local operation IDs listed");
            }
            for id in ids {
                let Some(id) = id.as_str() else {
                    ui.weak("Not a text operation ID");
                    continue;
                };
                let mut named = row["operations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|op| op["id"].as_str() == Some(id));
                let description = match (named.next(), named.next()) {
                    (Some(op), None) => op["tool"].as_str().unwrap_or("Tool name unavailable"),
                    (None, _) if !row["operations"].is_array() => "Operation list unavailable",
                    (None, _) => "No operation with this ID",
                    _ => "More than one operation with this ID",
                };
                ui.horizontal_wrapped(|ui| {
                    ui.code(id);
                    ui.label("→");
                    ui.label(description);
                });
            }
        });
}
