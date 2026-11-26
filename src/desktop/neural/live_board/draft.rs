//! Display only the public reply text attached to the current admitted call.
use super::*;

struct Draft<'a> {
    id: &'a str,
    actor: &'a str,
    text: &'a str,
    truncated: bool,
}

impl<'a> Draft<'a> {
    fn observed(call: &'a Value, worker: bool) -> Option<Self> {
        if !worker || call["state"] != "dispatched" || call["stage"] != "ordinary" {
            return None;
        }
        let reply = &call["reply_draft"];
        Some(Self {
            id: call["id"].as_str().filter(|id| !id.is_empty())?,
            actor: call["actor"].as_str().filter(|id| !id.is_empty())?,
            text: reply["text"]
                .as_str()
                .filter(|text| !text.trim().is_empty())?,
            truncated: reply["truncated"].as_bool()?,
        })
    }

    fn heading(&self, ui: &mut Ui) {
        let p = theme::palette(ui);
        theme::glyph(ui, Icon::Conversation, p.accent);
        ui.label(
            RichText::new("Draft reply")
                .size(13.)
                .strong()
                .color(p.accent),
        )
        .on_hover_text("Reply text received during this call.");
    }

    fn excerpt_label(&self, ui: &mut Ui) {
        if self.truncated {
            ui.label(
                RichText::new("Beginning only")
                    .small()
                    .color(theme::palette(ui).muted),
            )
            .on_hover_text("The preview shows the beginning of this reply.");
        }
    }

    fn body(&self, ui: &mut Ui, height: f32) {
        let font = egui::FontId::proportional(14.);
        let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
        let visible_height = (height / line_height).floor().max(1.) * line_height;
        let pane = ui.make_persistent_id(("draft-reading", self.id));
        let selection = egui::Id::new("selected-public-reply-text");
        let owns_selection = ui.data(|d| d.get_temp::<egui::Id>(selection)) == Some(pane);
        let reading = owns_selection
            && ui
                .ctx()
                .plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection();
        let output = egui::ScrollArea::vertical()
            .id_salt(("draft-text", self.id))
            .min_scrolled_height(0.)
            .max_height(height.min(visible_height))
            .auto_shrink([false, false])
            .stick_to_bottom(!reading)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(self.text)
                            .font(font)
                            .color(theme::palette(ui).text),
                    )
                    .wrap()
                    .selectable(true),
                )
            });
        // Egui releases the bottom anchor on manual scroll. Hold it during text
        // selection too, so the reader can finish selecting and copying a passage.
        if output.inner.is_pointer_button_down_on() || output.inner.clicked() {
            ui.data_mut(|d| d.insert_temp(selection, pane));
        } else if owns_selection
            && (!reading
                || ui.input(|i| i.pointer.any_pressed())
                || (ui.rect_contains_pointer(output.inner_rect)
                    && ui.input(|i| i.raw_scroll_delta.y != 0.)))
        {
            ui.data_mut(|d| d.remove::<egui::Id>(selection));
        }
    }
}

fn draft_frame(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(p.field.lerp_to_gamma(p.header, 0.45))
        .stroke(Stroke::new(1., p.border))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(9, 6))
}

pub(in super::super) fn home_reply(ui: &mut Ui, call: &Value, worker: bool) {
    if let Some(draft) = Draft::observed(call, worker) {
        draft_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                draft.heading(ui);
                draft.excerpt_label(ui);
            });
            draft.body(ui, 52.);
        });
    }
}

impl App {
    pub(super) fn current_reply_area(&mut self, ui: &mut Ui) {
        if self.live_board.hold.is_some()
            || !matches!(
                self.live_board.filter,
                Filter::Highlights | Filter::All | Filter::Messages
            )
        {
            return;
        }
        let mut drafts: Vec<_> = self
            .snapshot
            .active
            .iter()
            .filter(|call| call["work"] == self.work)
            .filter_map(|call| {
                let actor = s(call, "actor");
                if !self.live_board.person.is_empty() && actor != self.live_board.person {
                    return None;
                }
                Draft::observed(call, self.runner_observation.has_worker(&self.work, actor))
            })
            .collect();
        if drafts.is_empty() {
            return;
        }
        drafts.sort_unstable_by(|a, b| {
            self.label(a.actor)
                .cmp(self.label(b.actor))
                .then(a.id.cmp(b.id))
        });
        let mut selected = drafts
            .iter()
            .position(|d| d.id == self.live_board.draft_call)
            .unwrap_or(0);
        // Keep the saved rows anchored at the top and the composer reachable as the text grows.
        let available = ui.available_height().max(0.);
        let height = (available * 0.38).clamp(86., 116.).min(available * 0.58);
        egui::TopBottomPanel::bottom(egui::Id::new(("current-public-reply", &self.work)))
            .exact_height(height)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                ui.add_space(4.);
                draft_frame(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        drafts[selected].heading(ui);
                        if drafts.len() > 1 {
                            egui::ComboBox::from_id_salt("draft-author")
                                .width(ui.available_width().clamp(70., 124.))
                                .truncate()
                                .selected_text(self.label(drafts[selected].actor))
                                .show_ui(ui, |ui| {
                                    for (i, d) in drafts.iter().enumerate() {
                                        ui.push_id(d.id, |ui| {
                                            ui.selectable_value(
                                                &mut selected,
                                                i,
                                                self.label(d.actor),
                                            )
                                            .on_hover_text(d.actor);
                                        });
                                    }
                                })
                                .response
                                .on_hover_text("Choose a draft reply by author");
                        }
                        drafts[selected].excerpt_label(ui);
                        let name = self.label(drafts[selected].actor);
                        let title = self.task_title();
                        let attribution = if drafts.len() == 1 {
                            format!("{name} · {title}")
                        } else {
                            title.to_owned()
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(attribution)
                                    .small()
                                    .color(theme::palette(ui).muted),
                            )
                            .truncate(),
                        )
                        .on_hover_text(format!("{name} · {title}"));
                    });
                    let remaining = ui.available_height().max(14.);
                    drafts[selected].body(ui, remaining);
                });
            });
        if self.live_board.draft_call != drafts[selected].id {
            self.live_board.draft_call = drafts[selected].id.to_owned();
        }
    }
}
