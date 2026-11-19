//! A local what-if appraisal. Its values belong only to this view.
use super::{data::Character, section, theme, title, Color32, RichText, Ui, PURPLE, TEAL};
use ai_personas::persona_model::{Appraisal, FeatureContract, FeatureValue};
use eframe::egui;
use std::collections::BTreeMap;

pub(super) struct Explorer {
    owner: String,
    axis: String,
    values: BTreeMap<String, f64>,
}
impl Default for Explorer {
    fn default() -> Self {
        Self {
            owner: String::new(),
            axis: "uncertainty".into(),
            values: BTreeMap::new(),
        }
    }
}
impl Explorer {
    pub(super) fn show(&mut self, ui: &mut Ui, character: &Character) {
        if self.owner != character.model.persona {
            self.values.clear();
            self.owner = character.model.persona.clone();
        }
        let contract = FeatureContract::standard();
        ui.heading("Try an appraisal");
        ui.weak("Change a situational input to preview the response locally. Your preview stays out of the persona’s history.");
        ui.add_space(12.);
        egui::Frame::new()
            .fill(theme::palette(ui).panel)
            .corner_radius(9.)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("preview-axis")
                        .selected_text(title(&self.axis))
                        .show_ui(ui, |ui| {
                            for axis in &contract.inputs {
                                ui.selectable_value(
                                    &mut self.axis,
                                    axis.id.clone(),
                                    title(&axis.id),
                                );
                            }
                        });
                    let mut known = self.values.contains_key(&self.axis);
                    if ui.checkbox(&mut known, "Known input").changed() {
                        if known {
                            self.values.insert(self.axis.clone(), 0.0);
                        } else {
                            self.values.remove(&self.axis);
                        }
                    }
                    if ui
                        .add_enabled(!self.values.is_empty(), egui::Button::new("Reset inputs"))
                        .clicked()
                    {
                        self.values.clear();
                    }
                });
                if let Some(axis) = contract.inputs.iter().find(|axis| axis.id == self.axis) {
                    ui.small(&axis.meaning);
                }
                if let Some(value) = self.values.get_mut(&self.axis) {
                    ui.add(
                        egui::Slider::new(value, -1.0..=1.0)
                            .step_by(0.01)
                            .text("Value"),
                    );
                }
                let known = self
                    .values
                    .iter()
                    .map(|(k, v)| format!("{} {v:+.2}", title(k)))
                    .collect::<Vec<_>>();
                ui.weak(if known.is_empty() {
                    "All inputs unknown".to_owned()
                } else {
                    known.join("  ·  ")
                });
            });
        ui.add_space(16.);
        let appraisal = Appraisal {
            features: self
                .values
                .iter()
                .map(|(axis, v)| {
                    (
                        axis.clone(),
                        FeatureValue {
                            value: Some(*v),
                            evidence: Vec::new(),
                            interpretation: "Operator what-if preview".into(),
                        },
                    )
                })
                .collect(),
            reconsideration: None,
        };
        let current = match character.model.evaluate(&contract, &appraisal) {
            Ok(v) => v,
            Err(e) => {
                ui.colored_label(Color32::LIGHT_RED, e);
                return;
            }
        };
        let previous = match &character.previous {
            Some(model) => match model.evaluate(&contract, &appraisal) {
                Ok(v) => v,
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, e);
                    return;
                }
            },
            None => character.baseline.clone(),
        };
        section(ui, "Forward response", "−1 to +1");
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(TEAL, "● Active model");
            ui.colored_label(
                PURPLE,
                if character.previous.is_some() {
                    "○ Previous model, same inputs"
                } else {
                    "○ All-unknown baseline"
                },
            );
        });
        ui.add_space(8.);
        let muted = theme::palette(ui).muted;
        for axis in &contract.outputs {
            let value = current.outputs[&axis.id].value as f32;
            let old = previous.outputs[&axis.id].value as f32;
            ui.horizontal(|ui| {
                ui.add_sized(
                    [100., 22.],
                    egui::Label::new(RichText::new(title(&axis.id)).size(14.)),
                )
                .on_hover_text(&axis.meaning);
                let width = (ui.available_width() - 72.).clamp(44., 540.);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, 22.), egui::Sense::hover());
                let y = rect.center().y;
                let x = |v: f32| egui::lerp(rect.left() + 4.0..=rect.right() - 4.0, 0.5 + v * 0.5);
                let painter = ui.painter();
                painter.line_segment(
                    [egui::pos2(x(-1.), y), egui::pos2(x(1.), y)],
                    egui::Stroke::new(1., muted.gamma_multiply(0.45)),
                );
                painter.line_segment(
                    [egui::pos2(x(0.), y - 3.), egui::pos2(x(0.), y + 3.)],
                    egui::Stroke::new(1., muted),
                );
                painter.line_segment(
                    [egui::pos2(x(old), y), egui::pos2(x(value), y)],
                    egui::Stroke::new(2., TEAL),
                );
                painter.circle_stroke(egui::pos2(x(old), y), 5., egui::Stroke::new(1.5, PURPLE));
                painter.circle_filled(egui::pos2(x(value), y), 3., TEAL);
                response.on_hover_text(format!("Comparison {old:+.4}    Active {value:+.4}"));
                ui.monospace(format!("{value:+.3}"));
            });
        }
    }
}
