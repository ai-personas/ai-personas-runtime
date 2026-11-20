use super::*;
use model::{Entry, Epoch, Profile, Reading};

pub(super) fn show(
    ui: &mut Ui,
    entry: &Entry,
    hue: Color32,
    contract: &FeatureContract,
    focus: &mut usize,
    mut pinned: Option<u64>,
) -> (Option<u64>, Option<Action>) {
    let p = theme::palette(ui);
    let ink = p.ink(hue);
    let mut action = None;
    let frame = card(ui).corner_radius(10).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let (profile, epochs) = match &entry.reading {
            Reading::Available { profile, epochs } => (profile, epochs.as_slice()),
            Reading::Pending { profile } => (profile, &[][..]),
            Reading::Unavailable { reason } => {
                eyebrow(ui, "Evidence unavailable");
                ui.strong(&entry.owner);
                ui.label(reason);
                return;
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&profile.label).strong().size(23.).color(ink));
            ui.small(&entry.owner);
        });
        let Some((first, active)) = epochs.first().zip(epochs.last()) else {
            ui.add_space(8.);
            ui.strong("Awaiting first adoption");
            ui.label("A starting profile is not a numerical model.");
            origin(ui, profile, &mut action);
            return;
        };
        if epochs.len() > 1 {
            let caption = |e: &Epoch| {
                format!(
                    "Epoch {} · {}",
                    e.epoch,
                    if e.epoch == active.epoch {
                        "current".into()
                    } else {
                        title(&e.kind)
                    }
                )
            };
            let selected = pinned
                .and_then(|number| epochs.iter().find(|e| e.epoch == number))
                .unwrap_or(active);
            egui::ComboBox::from_id_salt("epoch")
                .selected_text(caption(selected))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut pinned,
                        None,
                        format!("Current · epoch {}", active.epoch),
                    );
                    for e in epochs.iter().rev() {
                        ui.selectable_value(
                            &mut pinned,
                            Some(e.epoch),
                            format!("Epoch {} · {}", e.epoch, title(&e.kind)),
                        );
                    }
                });
        } else {
            theme::badge(ui, "Epoch 1 · first and current", ink);
        }
        let chosen = pinned
            .and_then(|number| epochs.iter().find(|e| e.epoch == number))
            .unwrap_or(active);
        ui.add_space(7.);
        let mut plot = |ui: &mut Ui| {
            chart(ui, contract, first, chosen, ink, focus);
            let axis = &contract.outputs[*focus];
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title(&axis.id)).strong());
                ui.monospace(format!(
                    "{:+.3} → {:+.3}",
                    first.inclinations[&axis.id], chosen.inclinations[&axis.id]
                ));
            });
            ui.small(if chosen.epoch == 1 {
                "First model on this input"
            } else if first.package == chosen.package {
                "Back at the first package"
            } else if first.numerical == chosen.numerical {
                "Different package · same parameters"
            } else if first.inclinations == chosen.inclinations {
                "Changed parameters · same outputs here"
            } else {
                "Changed parameters and outputs here"
            });
        };
        let mut details = |ui: &mut Ui| {
            if ui
                .button("Inspect decisions →")
                .on_hover_text("Open the actual inputs and choices for this exact generation")
                .clicked()
            {
                action = Some(Action::Generation(
                    entry.owner.clone(),
                    chosen.change.clone(),
                ));
            }
            ui.collapsing("Model & origin", |ui| {
                ui.strong(format!("Epoch {} · {}", chosen.epoch, title(&chosen.kind)));
                ui.add(egui::Label::new(&chosen.reason).wrap());
                ui.horizontal_wrapped(|ui| {
                    for (label, exact) in [
                        ("Adoption decision", &chosen.decision),
                        ("Package", &chosen.package),
                    ] {
                        if ui.link(label).on_hover_text(exact).clicked() {
                            action = Some(Action::Inspect(exact.clone()));
                        }
                    }
                });
                origin(ui, profile, &mut action);
                if ui.button("Open persona profile").clicked() {
                    action = Some(Action::Profile(entry.owner.clone()));
                }
            });
        };
        if ui.available_width() >= 630. {
            ui.columns(2, |col| {
                plot(&mut col[0]);
                details(&mut col[1]);
            });
        } else {
            plot(ui);
            ui.add_space(8.);
            details(ui);
        }
        ui.collapsing("All inclinations", |ui| {
            egui::Grid::new("inclinations")
                .min_col_width(0.)
                .spacing([8., 3.])
                .striped(true)
                .show(ui, |ui| {
                    ui.small("Inclination");
                    ui.small("First");
                    ui.small(format!("E{}", chosen.epoch));
                    ui.end_row();
                    for axis in &contract.outputs {
                        ui.small(title(&axis.id)).on_hover_text(&axis.meaning);
                        for value in [first.inclinations[&axis.id], chosen.inclinations[&axis.id]] {
                            ui.label(RichText::new(format!("{value:+.3}")).monospace().small())
                                .on_hover_text(value.to_string());
                        }
                        ui.end_row();
                    }
                });
        });
    });
    let rect = frame.response.rect.shrink(1.);
    ui.painter().line_segment(
        [
            rect.left_top() + egui::vec2(10., 0.),
            rect.right_top() - egui::vec2(10., 0.),
        ],
        Stroke::new(2., ink),
    );
    (pinned, action)
}

fn origin(ui: &mut Ui, profile: &Profile, action: &mut Option<Action>) {
    ui.small(format!("Starting profile by {}", profile.seed_author));
    ui.add(egui::Label::new(&profile.seed).wrap());
    if ui
        .link("Original identity record")
        .on_hover_text(&profile.origin)
        .clicked()
    {
        *action = Some(Action::Inspect(profile.origin.clone()));
    }
}

/// One vertical axis per named output, in contract order (not elapsed time).
/// Shapes, text equivalents and a keyboard-operable selector accompany colour.
fn chart(
    ui: &mut Ui,
    contract: &FeatureContract,
    first: &Epoch,
    chosen: &Epoch,
    ink: Color32,
    focus: &mut usize,
) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 95.), egui::Sense::click());
    let p = theme::palette(ui);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6., p.field);
    let left = rect.left() + 22.;
    let right = (rect.right() - 9.).max(left + 1.);
    let x = |i: usize| egui::lerp(left..=right, i as f32 / (contract.outputs.len() - 1) as f32);
    let y = |v: f64| egui::lerp(rect.bottom() - 8. ..=rect.top() + 8., (v as f32 + 1.) / 2.);
    let nearest = |pos: egui::Pos2| {
        (((pos.x - left) / (right - left)).clamp(0., 1.) * (contract.outputs.len() - 1) as f32)
            .round() as usize
    };
    if let Some(pos) = response
        .interact_pointer_pos()
        .filter(|_| response.clicked())
    {
        *focus = nearest(pos);
    }
    for (value, sign) in [(-1., "−1"), (0., "0"), (1., "+1")] {
        painter.text(
            egui::pos2(left - 4., y(value)),
            egui::Align2::RIGHT_CENTER,
            sign,
            FontId::monospace(9.),
            p.muted,
        );
        painter.line_segment(
            [egui::pos2(left, y(value)), egui::pos2(right, y(value))],
            Stroke::new(1., if value == 0. { p.border } else { p.hover }),
        );
    }
    painter.line_segment(
        [
            egui::pos2(x(*focus), rect.top() + 4.),
            egui::pos2(x(*focus), rect.bottom() - 4.),
        ],
        Stroke::new(1., p.muted),
    );
    let series = |e: &Epoch| {
        contract
            .outputs
            .iter()
            .enumerate()
            .map(|(i, a)| egui::pos2(x(i), y(e.inclinations[&a.id])))
            .collect::<Vec<_>>()
    };
    let active = series(chosen);
    let base = series(first);
    // A faint line joins the original hollow points. The smaller solid points
    // remain visible even when the two projections exactly coincide.
    for segment in base.windows(2) {
        painter.line_segment([segment[0], segment[1]], Stroke::new(1., p.muted));
    }
    for segment in active.windows(2) {
        painter.line_segment([segment[0], segment[1]], Stroke::new(2., ink));
    }
    for (i, (a, b)) in active.iter().zip(base).enumerate() {
        painter.circle_stroke(b, 4., Stroke::new(1., p.muted));
        painter.circle_filled(*a, if i == *focus { 4. } else { 2.2 }, ink);
    }
    if let Some(pos) = response.hover_pos() {
        let axis = &contract.outputs[nearest(pos)].id;
        response
            .on_hover_cursor(egui::CursorIcon::Crosshair)
            .on_hover_text(format!(
                "{}\nFirst {:+.6}   Epoch {} {:+.6}\nClick to spotlight across portraits",
                title(axis),
                first.inclinations[axis],
                chosen.epoch,
                chosen.inclinations[axis]
            ));
    }
}
