use super::*;

impl App {
    pub(super) fn comparisons(&mut self, ui: &mut Ui) {
        let records: Vec<_> = self
            .snapshot
            .assessments
            .iter()
            .filter(|r| r["owner"] == self.persona)
            .cloned()
            .collect();
        section(
            ui,
            "Behavioral comparisons",
            &format!("{} recorded", records.len()),
        );
        if records.is_empty() {
            ui.weak("This persona has no recorded comparison.");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("comparisons", &self.persona))
            .show(ui, |ui| {
                for record in records {
                    let name = self
                        .snapshot
                        .candidates
                        .iter()
                        .find(|c| c["reference"] == record["candidate"])
                        .and_then(|c| c["body"]["intent"].as_str())
                        .unwrap_or("Character proposal");
                    let heading = excerpt(name, 94);
                    ui.push_id(s(&record, "reference"), |ui| {
                        egui::CollapsingHeader::new(heading)
                            .default_open(true)
                            .show(ui, |ui| {
                                if let Some(reason) = record["interruption"].as_str() {
                                    ui.label(format!("Incomplete comparison · {reason}"));
                                }
                                for (i, case) in
                                    record["cases"].as_array().into_iter().flatten().enumerate()
                                {
                                    ui.push_id(i, |ui| {
                                        ui.add_space(6.);
                                        if let Some(decision) = case["origin"]["decision"].as_str()
                                        {
                                            theme::badge(ui, "Prior encounter", theme::ACCENT);
                                            if ui.small_button("Original decision ↗").clicked() {
                                                self.open_resource(ui.ctx(), decision.into());
                                            }
                                        } else {
                                            ui.strong("Hypothetical situation");
                                            if let Some(text) = case["situation"].as_str() {
                                                ui.label(text);
                                            }
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            if let Some(n) =
                                                case["image_count"].as_u64().filter(|n| *n > 0)
                                            {
                                                ui.small(format!("{n} attached image(s) per arm"));
                                            }
                                            if let Some(r) = case["matched_input"].as_str() {
                                                if ui
                                                    .small_button("Inspect shared inputs ↗")
                                                    .clicked()
                                                {
                                                    self.open_resource(ui.ctx(), r.into());
                                                }
                                            }
                                        });
                                        let arms = case["arms"]
                                            .as_array()
                                            .map(Vec::as_slice)
                                            .unwrap_or(&[]);
                                        if let (Some(left), Some(right)) = (
                                            arms.first().and_then(|a| a["outputs"].as_object()),
                                            arms.get(1).and_then(|a| a["outputs"].as_object()),
                                        ) {
                                            let changes = left
                                                .iter()
                                                .filter_map(|(axis, a)| {
                                                    let a = a.as_f64()?;
                                                    let b = right.get(axis)?.as_f64()?;
                                                    (a != b).then(|| {
                                                        format!("{} {a:+.3} → {b:+.3}", title(axis))
                                                    })
                                                })
                                                .collect::<Vec<_>>();
                                            ui.small(if changes.is_empty() {
                                                "Equal numerical outputs for this situation".into()
                                            } else {
                                                changes.join("   ·   ")
                                            });
                                        }
                                        ui.add_space(5.);
                                        if !arms.is_empty()
                                            && ui.available_width() >= 240. * arms.len() as f32
                                        {
                                            ui.columns(arms.len(), |columns| {
                                                for (column, arm) in columns.iter_mut().zip(arms) {
                                                    self.comparison_arm(column, arm);
                                                }
                                            });
                                        } else {
                                            for arm in arms {
                                                self.comparison_arm(ui, arm);
                                            }
                                        }
                                    });
                                }
                                if ui.small_button("Open complete assessment ↗").clicked() {
                                    self.open_resource(ui.ctx(), s(&record, "reference").into());
                                }
                            });
                    });
                    ui.separator();
                }
            });
    }

    fn comparison_arm(&mut self, ui: &mut Ui, arm: &Value) {
        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (label, color) = match arm["arm"].as_str() {
                Some("active") => ("Active at comparison", theme::ACCENT),
                Some("candidate") => ("Proposed character", PURPLE),
                _ => ("Without character", theme::palette(ui).muted),
            };
            theme::badge(ui, label, color);
            if let Some(error) = arm["error"].as_str() {
                ui.label(error);
            }
            for (field, label) in [
                ("judgment", "Judgment"),
                ("proposed_action", "Proposed action"),
                ("communication", "Communication"),
                ("remaining_uncertainty", "Remaining uncertainty"),
            ] {
                if let Some(text) = arm["response"][field].as_str() {
                    ui.add_space(6.);
                    ui.strong(label);
                    ui.add(egui::Label::new(text).wrap());
                }
            }
            if let Some(r) = arm["receipt"].as_str() {
                if ui.small_button("Provider receipt ↗").clicked() {
                    self.open_resource(ui.ctx(), r.into());
                }
            }
        });
    }
}
