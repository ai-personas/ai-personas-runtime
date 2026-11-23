//! The author's prose stays intact. File and package actions resolve in this workspace.
use super::*;
use document::edition_links::{self, Kind};

pub(super) fn show(
    ui: &mut Ui,
    text: &str,
    assets: &[Value],
    deliveries: &[Value],
) -> Option<String> {
    if !text.contains("asset:") && !text.contains("delivery:") {
        return None;
    }
    // Only parsed destinations and labels are cached. A link may be an older edition
    // absent from the current catalogue; opening it still looks up its exact handle.
    let key = egui::Id::new(("studio-message-editions", text));
    let links = ui.ctx().data_mut(|d| {
        Arc::clone(d.get_temp_mut_or_insert_with(key, || Arc::new(edition_links::collect(text))))
    });
    let mut chosen = None;
    for link in links.iter() {
        let reference = link.reference.as_str();
        let (kind, icon, name) = match link.kind {
            Kind::File => (
                "File",
                Icon::Files,
                assets
                    .iter()
                    .find(|r| r["reference"] == reference)
                    .and_then(|r| r["body"]["path"].as_str()),
            ),
            Kind::Package => (
                "Package",
                Icon::Folder,
                deliveries
                    .iter()
                    .find(|r| inbox::handle(r) == reference)
                    .and_then(|r| r["body"]["title"].as_str()),
            ),
        };
        let name = name
            .or_else(|| (!link.label.trim().is_empty()).then(|| link.label.trim()))
            .unwrap_or("Open edition");
        ui.push_id(reference, |ui| {
            ui.horizontal(|ui| {
                let p = theme::palette(ui);
                theme::glyph(ui, icon, p.accent);
                ui.label(design::caption(
                    &format!("{kind} · v{}", link.revision),
                    p.muted,
                ));
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                if ui
                    .link(name)
                    .on_hover_text(format!(
                        "Open {kind} revision {} in Studio\n{reference}",
                        link.revision
                    ))
                    .clicked()
                {
                    chosen = Some(reference.to_owned());
                }
            });
        });
    }
    chosen
}

impl App {
    pub(super) fn message_editions(&self, ui: &mut Ui, text: &str) -> Option<String> {
        show(ui, text, &self.snapshot.assets, self.records("deliveries"))
    }

    pub(super) fn open_message_edition(&mut self, ctx: &egui::Context, reference: String) {
        if reference.starts_with("delivery:") {
            self.open_issued_package(ctx, &reference);
        } else {
            self.open_resource(ctx, reference);
        }
    }
}
