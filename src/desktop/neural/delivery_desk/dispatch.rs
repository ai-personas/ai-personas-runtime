//! Recipient-facing slips. Only selections made by the issuing persona appear
//! here; a preview or a local export is not a human response or an order.
use super::*;

const FILES_PER_PAGE: usize = 6;

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
        let mut page = self.delivery_desk.slip_page;
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
            if index != self.delivery_desk.slip {
                page = 0;
            }
            page = page.min(addresses.len().saturating_sub(1) / FILES_PER_PAGE);
            let direction = |ui: &mut Ui| {
                eyebrow(ui, "Prepared for");
                ui.add(
                    egui::Label::new(RichText::new(s(slip, "recipient")).size(21.).strong()).wrap(),
                );
                let instructions = s(slip, "instructions");
                ui.label(instructions);
                if ui
                    .small_button("Copy instructions")
                    .on_hover_text("Copy the issuer’s wording. Nothing is sent.")
                    .clicked()
                {
                    ui.ctx().copy_text(instructions.to_owned());
                }
            };
            let mut contents = |ui: &mut Ui| {
                ui.horizontal_wrapped(|ui| {
                    eyebrow(ui, "Take unchanged");
                    ui.small(counted(addresses.len() as u64, "file", "files"));
                });
                let ready = !addresses.is_empty()
                    && addresses.iter().all(|r| {
                        p.by_reference
                            .get(*r)
                            .is_some_and(|&i| p.files[i].digest.is_some())
                    });
                for &reference in addresses
                    .iter()
                    .skip(page * FILES_PER_PAGE)
                    .take(FILES_PER_PAGE)
                {
                    match p.by_reference.get(reference) {
                        Some(&i) => {
                            let f = &p.files[i];
                            if ui
                                .link(&f.path)
                                .on_hover_text(format!("Preview {}\n{}", f.path, reference))
                                .clicked()
                            {
                                preview = Some(i);
                            }
                        }
                        None => {
                            ui.weak(format!("File edition unavailable: {reference}"));
                        }
                    }
                }
                home::pager(ui, &mut page, addresses.len(), FILES_PER_PAGE, "slip files");
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
                    // A justified column can stretch a short word across the
                    // whole line before a long filename or checksum.
                    c[0].with_layout(egui::Layout::top_down(egui::Align::Min), direction);
                    contents(&mut c[1]);
                });
            } else {
                direction(ui);
                ui.add_space(8.);
                contents(ui);
            }
            let outstanding: Vec<_> = strings(&p.body["remaining"]).collect();
            if !outstanding.is_empty() {
                ui.add_space(6.);
                egui::CollapsingHeader::new(format!(
                    "Before forwarding · {}",
                    counted(
                        outstanding.len() as u64,
                        "remaining item",
                        "remaining items"
                    )
                ))
                .id_salt(("dispatch-conditions", &p.reference))
                .show(ui, |ui| {
                    ui.weak("The issuer recorded these open items for this edition:");
                    for condition in outstanding {
                        ui.label(format!("• {condition}"));
                    }
                });
            }
        });
        self.delivery_desk.slip = index;
        self.delivery_desk.slip_page = page;
        if let Some(i) = preview {
            self.delivery_desk.selected = i;
            self.delivery_desk.focus_preview = true;
        }
        if save {
            let slip = &slips[index];
            let refs: Vec<_> = strings(&slip["artifacts"]).collect();
            let path = (refs.len() == 1)
                .then(|| p.by_reference.get(refs[0]))
                .flatten()
                .map(|&i| p.files[i].path.as_str());
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
