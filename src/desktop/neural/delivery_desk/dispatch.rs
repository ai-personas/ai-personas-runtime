//! Recipient-facing slips. Only selections made by the issuing persona appear
//! here; a preview or a local export is not a human response or an order.
use super::*;

impl App {
    pub(super) fn dispatch_slips(&mut self, ui: &mut Ui, p: &Package) {
        let Some(slips) = p.body["dispatches"].as_array().filter(|s| !s.is_empty()) else {
            return;
        };
        let mut index = self.delivery_desk.slip.min(slips.len() - 1);
        let colors = theme::palette(ui);
        let issuer = s(&p.body, "owner");
        let ink = colors.ink(persona_color(issuer));
        let mut save = false;
        let mut preview = None;
        ui.add_space(8.);
        panel(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Files, ink);
                eyebrow(ui, "Dispatch slips");
                ui.small(format!("{} / {}", index + 1, slips.len()));
                if slips.len() > 1 {
                    egui::ComboBox::from_id_salt(("dispatch-selector", &p.reference))
                        .selected_text("Choose recipient")
                        .width(164.)
                        .show_ui(ui, |ui| {
                            ui.set_max_width(
                                (ui.ctx().content_rect().width() - 40.).clamp(160., 480.),
                            );
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                            for (i, item) in slips.iter().enumerate() {
                                ui.selectable_value(
                                    &mut index,
                                    i,
                                    format!("{:02}  {}", i + 1, s(item, "recipient")),
                                );
                            }
                        });
                }
            });
            ui.separator();
            let slip = &slips[index];
            let addresses: Vec<_> = strings(&slip["artifacts"]).collect();
            let direction = |ui: &mut Ui| {
                eyebrow(ui, "Prepared for");
                ui.add(
                    egui::Label::new(RichText::new(s(slip, "recipient")).size(21.).strong()).wrap(),
                );
                ui.label(s(slip, "instructions"));
            };
            let mut contents = |ui: &mut Ui| {
                ui.horizontal_wrapped(|ui| {
                    eyebrow(ui, "Take unchanged");
                    ui.small(counted(addresses.len() as u64, "file", "files"));
                });
                let mut ready = !addresses.is_empty();
                for reference in &addresses {
                    match p
                        .files
                        .iter()
                        .enumerate()
                        .find(|(_, f)| &f.reference == reference)
                    {
                        Some((i, f)) => {
                            ready &= f.digest.is_some();
                            if ui
                                .link(&f.path)
                                .on_hover_text(format!("Preview {}\n{}", f.path, reference))
                                .clicked()
                            {
                                preview = Some(i);
                            }
                        }
                        None => {
                            ready = false;
                            ui.weak(format!("File edition unavailable: {reference}"));
                        }
                    }
                }
                if ui
                    .add_enabled(
                        ready,
                        theme::primary(if addresses.len() == 1 {
                            "Save original file…"
                        } else {
                            "Save selected files ZIP…"
                        }),
                    )
                    .on_hover_text(
                        "Save this slip's file selection locally. Nothing is transmitted.",
                    )
                    .clicked()
                {
                    save = true;
                }
            };
            if ui.available_width() >= 680. {
                ui.columns(2, |c| {
                    direction(&mut c[0]);
                    contents(&mut c[1]);
                });
            } else {
                direction(ui);
                ui.add_space(8.);
                contents(ui);
            }
        });
        self.delivery_desk.slip = index;
        if let Some(i) = preview {
            self.delivery_desk.selected = i;
            self.delivery_desk.focus_preview = true;
        }
        if save {
            let slip = &slips[index];
            let refs: Vec<_> = strings(&slip["artifacts"]).collect();
            let path = (refs.len() == 1)
                .then(|| p.files.iter().find(|f| f.reference == refs[0]))
                .flatten()
                .map(|f| f.path.as_str());
            self.save_dispatch(
                &p.reference,
                index + 1,
                s(slip, "recipient"),
                refs.len(),
                path,
            );
        }
    }
}
