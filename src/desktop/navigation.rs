//! Shared navigation keeps task scope, people, and conversation filters coherent.
use super::*;
use std::collections::BTreeMap;

impl App {
    pub(super) fn persona_catalog(&self) -> BTreeMap<String, String> {
        let mut people = BTreeMap::new();
        if let Some(labels) = self.view.inspection["settings"]["persona_labels"].as_object() {
            for (id, label) in labels {
                people.insert(id.clone(), label.as_str().unwrap_or(id).to_owned());
            }
        }
        for work in models_at(&self.view.inspection, "works") {
            for person in models_at(work, "participants") {
                let id = s(person, "persona");
                if !id.is_empty() {
                    people
                        .entry(id.to_owned())
                        .or_insert_with(|| person["label"].as_str().unwrap_or(id).to_owned());
                }
            }
        }
        people
    }

    pub(super) fn clear_conversation_filters(&mut self) {
        self.conversation_filter.clear();
        self.conversation_persona = None;
        self.conversation_route = None;
        self.conversation_both_directions = false;
        self.conversation_channel = 0;
        self.conversation_page = 0;
        self.conversation_target = None;
        self.highlight_message = None;
        self.scroll_to_message = false;
    }

    pub(super) fn open_task(&mut self, work: String, tab: usize, persona: Option<String>) {
        if self.busy {
            return;
        }
        self.tab = tab;
        self.inspected_persona = persona;
        self.clear_conversation_filters();
        self.conversation_mode = 0;
        self.conversation_inputs = false;
        if self.view.selected.as_ref() != Some(&work) {
            self.send(Action::Select(work));
        }
    }

    pub(super) fn open_profile(&mut self, persona: String) {
        if self.busy {
            return;
        }
        self.tab = 6;
        self.inspected_persona = Some(persona.clone());
        if self.profile_persona.as_ref() != Some(&persona) {
            self.profile_filter.clear();
            self.profile_history = false;
            self.profile_tab = 0;
        }
        if self.profile_persona.as_ref() != Some(&persona)
            || self.view.development["persona"].as_str() != Some(&persona)
        {
            self.send(Action::Development(persona));
        }
    }

    pub(super) fn open_exchange(&mut self, from: String, to: String, message: Option<String>) {
        self.clear_conversation_filters();
        self.tab = 0;
        self.conversation_mode = 0;
        self.conversation_inputs = false;
        self.conversation_route = Some((from, to));
        self.conversation_target = message;
    }

    pub(super) fn persona_link(&mut self, ui: &mut Ui, id: &str) {
        let label = self.persona_label(id);
        let known = self.persona_catalog().contains_key(id)
            || self.view.conversation["persona_labels"].get(id).is_some();
        if known {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(RichText::new(label).strong()).frame(false),
                )
                .on_hover_text(format!("{id} · open persona profile"))
                .clicked()
            {
                self.open_profile(id.to_owned());
            }
        } else {
            ui.strong(label);
        }
    }

    pub(super) fn workspace_navigation(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [(5, "Overview"), (6, "Personas")] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }
            ui.separator();
            let task_available = self.selected().is_some();
            ui.add_enabled_ui(task_available, |ui| {
                for (mode, label) in [(0, "Conversation"), (1, "Collaboration")] {
                    if ui
                        .selectable_label(self.tab == 0 && self.conversation_mode == mode, label)
                        .clicked()
                    {
                        self.tab = 0;
                        self.conversation_mode = mode;
                    }
                }
                for (tab, label) in [
                    (1, "Files"),
                    (2, "Questions"),
                    (3, "Activity"),
                    (4, "Sessions"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
            });
        });
    }
}
