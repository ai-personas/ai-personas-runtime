//! Character Atlas is a read-only ensemble view. Verification runs off the UI
//! thread and is invalidated by identity changes, not every live stream byte.
use super::*;
use ai_personas::neural::revision_uses::atlas::{self as model, Atlas, Values};
use std::{path::Path, sync::mpsc::TryRecvError};
mod portrait;

#[derive(Clone, PartialEq)]
struct Key {
    identities: Vec<String>,
    inputs: Values,
    decision: Option<String>,
}

#[derive(Default)]
pub(super) struct Panel {
    loaded: Option<Key>,
    result: Option<Result<Atlas, String>>,
    pending: Option<(Key, mpsc::Receiver<Result<Atlas, String>>)>,
    inputs: Values,
    draft: Values,
    decision: Option<String>,
    editing: bool,
    input_axis: usize,
    focus: usize,
    hidden: BTreeSet<String>,
    people_filter: String,
    // Absence follows the active epoch, including after a live switch.
    epochs: BTreeMap<String, u64>,
}

enum Action {
    Lab,
    Profile(String),
    Generation(String, String),
    Inspect(String),
}

impl App {
    pub(super) fn character_atlas(&mut self, ui: &mut Ui) {
        let people = self.snapshot.status["personas"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let action = egui::ScrollArea::vertical()
            .id_salt("character-atlas")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.atlas.show(ui, &self.root, people, &self.home_index)
            })
            .inner;
        match action {
            Some(Action::Lab) => self.layout.editor = Editor::Character,
            Some(Action::Profile(id)) => self.show_person(&id),
            Some(Action::Generation(owner, change)) => {
                self.open_generation(ui.ctx(), owner, change)
            }
            Some(Action::Inspect(reference)) => self.open_resource(ui.ctx(), reference),
            None => {}
        }
    }
}

impl Panel {
    pub(super) fn use_decision(&mut self, decision: String, owner: String, epoch: u64) {
        self.hidden.remove(&owner);
        self.epochs.insert(owner, epoch);
        self.decision = Some(decision);
        self.inputs.clear();
        self.draft.clear();
        self.editing = false;
    }

    fn refresh(&mut self, ctx: &egui::Context, root: &Path, key: Key) {
        if let Some((loading, rx)) = &self.pending {
            let result = match rx.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Disconnected) => {
                    Err("The character history reader stopped.".into())
                }
                Err(TryRecvError::Empty) => return,
            };
            if *loading == key {
                self.loaded = Some(key.clone());
                self.result = Some(result);
            }
            self.pending = None;
        }
        if self.loaded.as_ref() == Some(&key) {
            return;
        }
        self.result = None;
        self.loaded = None;
        let (tx, rx) = mpsc::channel();
        let (root, ctx, inputs) = (root.to_owned(), ctx.clone(), key.inputs.clone());
        let decision = key.decision.clone();
        self.pending = Some((key, rx));
        std::thread::spawn(move || {
            let result = data::open(&root).and_then(|db| match decision {
                Some(reference) => model::from_decision(&db, &[], &reference),
                None => model::inspect(&db, &[], &inputs),
            });
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    fn show(
        &mut self,
        ui: &mut Ui,
        root: &Path,
        people: &[Value],
        works: &home_index::Index,
    ) -> Option<Action> {
        let mut action = None;
        let contract = FeatureContract::standard();
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("← Character Lab").clicked() {
                action = Some(Action::Lab);
            }
            eyebrow(ui, "Ensemble study");
        });
        ui.add_space(5.);
        ui.label(design::display("Character Atlas", 38.));
        ui.label("Compare each persona with their first model, on the same situation.");
        ui.add_space(12.);
        self.controls(ui, &contract, people);
        let key = Key {
            identities: people
                .iter()
                .map(|p| s(&p["reference"], "digest").to_owned())
                .collect(),
            inputs: self.inputs.clone(),
            decision: self.decision.clone(),
        };
        self.refresh(ui.ctx(), root, key.clone());
        ui.add_space(14.);
        // Never show the old chart under the label of a new situation.
        if self.loaded.as_ref() != Some(&key) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking adopted models…");
            });
            return action;
        }
        let report = match self.result.as_ref()? {
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                if ui.button("Retry").clicked() {
                    self.loaded = None;
                }
                return action;
            }
            Ok(data) => data,
        };
        if let Some(source) = &report.source {
            ui.horizontal_wrapped(|ui| {
                eyebrow(ui, "Received input");
                ui.strong(format!(
                    "{} · epoch {} · {}",
                    source.owner,
                    source.epoch,
                    title(&source.basis)
                ));
                if ui
                    .link("Recorded choice")
                    .on_hover_text(&source.decision)
                    .clicked()
                {
                    action = Some(Action::Inspect(source.decision.clone()));
                }
                if ui
                    .link("Provider receipt")
                    .on_hover_text(&source.provider_receipt)
                    .clicked()
                {
                    action = Some(Action::Inspect(source.provider_receipt.clone()));
                }
            });
            if let Some(work) = works.activity(&source.work) {
                ui.small(excerpt(&work.title, 120))
                    .on_hover_text(&work.title);
            }
            let inputs = if report.inputs.is_empty() {
                "All inputs unknown".into()
            } else {
                report
                    .inputs
                    .iter()
                    .map(|(id, v)| format!("{} {v:+}", title(id)))
                    .collect::<Vec<_>>()
                    .join("  ·  ")
            };
            ui.label(inputs);
            ui.small("Only the source persona at the named epoch received this input in that decision. Other portraits are local comparisons.");
            ui.add_space(8.);
        }
        let visible: Vec<_> = report
            .people
            .iter()
            .filter(|e| !self.hidden.contains(&e.owner))
            .collect();
        if visible.is_empty() {
            ui.label(if report.people.is_empty() {
                "No personas yet. Choose + New → Persona to add a starting profile."
            } else {
                "No portraits selected."
            });
            if !report.people.is_empty() && ui.button("Show everyone").clicked() {
                self.hidden.clear();
            }
            return action;
        }
        ui.horizontal_wrapped(|ui| {
            eyebrow(ui, "Model signatures");
            ui.small(
                "Hollow marks: first model   ·   Solid line: chosen epoch   ·   Scale: −1 to +1",
            );
        });
        ui.add_space(5.);
        let cols = ((ui.available_width() / 268.).floor() as usize).clamp(1, visible.len());
        for group in visible.chunks(cols) {
            ui.columns(cols, |columns| {
                for (entry, column) in group.iter().zip(columns) {
                    column.push_id(&entry.owner, |ui| {
                        let selected = self.epochs.get(&entry.owner).copied();
                        let (choice, result) = portrait::show(
                            ui,
                            entry,
                            persona_color(&entry.owner),
                            &contract,
                            &mut self.focus,
                            selected,
                            report.source.as_ref(),
                        );
                        if let Some(epoch) = choice {
                            self.epochs.insert(entry.owner.clone(), epoch);
                        } else {
                            self.epochs.remove(&entry.owner);
                        }
                        if result.is_some() {
                            action = result;
                        }
                    });
                }
            });
            ui.add_space(9.);
        }
        ui.small("These are inclinations, not measures of skill or improvement. To compare a situation someone actually encountered, choose Inspect decisions → Use situation in Atlas.");
        action
    }

    fn controls(&mut self, ui: &mut Ui, contract: &FeatureContract, people: &[Value]) {
        card(ui).fill(theme::palette(ui).header).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                eyebrow(ui, "Shared situation");
                if self.decision.is_some() {
                    ui.strong("From a recorded decision");
                    if ui.button("Return to all unknown").clicked() {
                        self.decision = None;
                    }
                    return;
                }
                ui.strong(input_label(&self.inputs));
                if ui
                    .button(if self.editing {
                        "Close editor"
                    } else {
                        "Edit inputs…"
                    })
                    .clicked()
                {
                    self.editing = !self.editing;
                    self.draft = self.inputs.clone();
                }
                if !self.inputs.is_empty() && ui.small_button("All unknown").clicked() {
                    self.inputs.clear();
                    self.draft.clear();
                    self.editing = false;
                }
            });
            if self.editing {
                ui.add_space(4.);
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("input-axis")
                        .selected_text(title(&contract.inputs[self.input_axis].id))
                        .show_ui(ui, |ui| {
                            for (index, axis) in contract.inputs.iter().enumerate() {
                                ui.selectable_value(&mut self.input_axis, index, title(&axis.id))
                                    .on_hover_text(&axis.meaning);
                            }
                        });
                    let id = &contract.inputs[self.input_axis].id;
                    let mut known = self.draft.contains_key(id);
                    if ui.checkbox(&mut known, "Known value").changed() {
                        if known {
                            self.draft.insert(id.clone(), 0.);
                        } else {
                            self.draft.remove(id);
                        }
                    }
                    if let Some(value) = self.draft.get_mut(id) {
                        ui.add(
                            egui::Slider::new(value, -1.0..=1.0)
                                .step_by(0.01)
                                .text("Value"),
                        );
                    }
                });
                ui.small(&contract.inputs[self.input_axis].meaning);
                ui.horizontal_wrapped(|ui| {
                    if ui.add(theme::primary("Apply situation")).clicked() {
                        self.inputs = self.draft.clone();
                        self.editing = false;
                    }
                    ui.small(format!("Preview: {}", input_label(&self.draft)));
                });
            }
            ui.add_space(5.);
            ui.horizontal_wrapped(|ui| {
                ui.label("Spotlight");
                egui::ComboBox::from_id_salt("output-axis")
                    .selected_text(title(&contract.outputs[self.focus].id))
                    .show_ui(ui, |ui| {
                        for (index, axis) in contract.outputs.iter().enumerate() {
                            ui.selectable_value(&mut self.focus, index, title(&axis.id))
                                .on_hover_text(&axis.meaning);
                        }
                    });
                egui::containers::menu::MenuButton::new("Choose people…")
                    .config(
                        egui::containers::menu::MenuConfig::new()
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                    )
                    .ui(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.people_filter)
                                .hint_text("Find name or ID")
                                .desired_width(180.),
                        );
                        ui.horizontal(|ui| {
                            if ui.small_button("Show everyone").clicked() {
                                self.hidden.clear();
                            }
                            if ui.small_button("Clear selection").clicked() {
                                self.hidden
                                    .extend(people.iter().map(|p| s(&p["body"], "id").to_owned()));
                            }
                        });
                        let find = self.people_filter.to_lowercase();
                        egui::ScrollArea::vertical()
                            .max_height(240.)
                            .show(ui, |ui| {
                                for person in people {
                                    let b = &person["body"];
                                    let id = s(b, "id");
                                    let name = s(b, "label");
                                    if !find.is_empty()
                                        && !id.to_lowercase().contains(&find)
                                        && !name.to_lowercase().contains(&find)
                                    {
                                        continue;
                                    }
                                    ui.push_id(id, |ui| {
                                        let mut shown = !self.hidden.contains(id);
                                        if ui
                                            .checkbox(&mut shown, s(b, "label"))
                                            .on_hover_text(id)
                                            .changed()
                                        {
                                            if shown {
                                                self.hidden.remove(id);
                                            } else {
                                                self.hidden.insert(id.into());
                                            }
                                        }
                                    });
                                }
                            });
                    });
            });
            ui.small(&contract.outputs[self.focus].meaning);
        });
    }
}

fn input_label(values: &Values) -> String {
    if values.is_empty() {
        return "All inputs unknown".into();
    }
    values
        .iter()
        .map(|(id, value)| format!("{} {value:+.2}", title(id)))
        .collect::<Vec<_>>()
        .join("  ·  ")
}
