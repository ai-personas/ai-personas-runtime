//! Shared reading surface for the operator's pinned review or amendment basis.
use super::*;

pub(super) fn revision(reference: &str) -> &str {
    reference.rsplit_once('@').map_or(reference, |(_, n)| n)
}

pub(super) enum Link {
    Edition(String),
    File {
        reference: String,
        brief: String,
        name: String,
    },
}

impl Link {
    pub(super) fn open(self, app: &mut App, ctx: &egui::Context) {
        match self {
            Self::Edition(reference) => app.open_issued_package(ctx, &reference),
            Self::File {
                reference,
                brief,
                name,
            } => {
                app.open_named_resource(ctx, reference, Some((brief, name)));
            }
        }
    }
}

pub(super) fn show(
    ui: &mut Ui,
    handle: &str,
    body: &Value,
    caption: &str,
    inspect: &mut Option<Link>,
) {
    egui::CollapsingHeader::new(format!("{caption} · brief {}", revision(handle)))
        .id_salt((caption, handle))
        .show(ui, |ui| {
            ui.small(handle);
            ui.strong(format!("Original request · {}", s(body, "requester")));
            ui.label(s(body, "original_need"));
            if let Some(edition) = body["from_edition"].as_str() {
                if ui
                    .link("Starting edition ↗")
                    .on_hover_text(edition)
                    .clicked()
                {
                    *inspect = Some(Link::Edition(edition.to_owned()));
                }
            }
            let mut files = |ui: &mut Ui, list: &Value| {
                if let Some((reference, name)) = overview::source_file_links(ui, list, handle) {
                    *inspect = Some(Link::File {
                        reference,
                        brief: handle.into(),
                        name,
                    });
                }
            };
            files(ui, &body["materials"]);
            for (index, change) in body["amendments"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                ui.push_id(index, |ui| {
                    ui.strong(format!("Amendment {} · {}", index + 1, s(change, "author")));
                    ui.label(s(change, "text"));
                    files(ui, &change["attachments"]);
                });
            }
            ui.small(format!("Activity state: {}", s(body, "phase")));
        });
}
