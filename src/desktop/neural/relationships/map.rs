//! Each connection represents recorded contact from the selected persona's perspective.
use super::*;

const PAGE_SIZE: usize = 4;

struct MapContact {
    person: Contact,
    label: String,
    sent: u64,
    received: u64,
    shared: u64,
    latest: Option<String>,
}

#[derive(Default)]
pub(super) struct Map {
    filter: String,
    page: usize,
    source: Option<Arc<Vec<Value>>>,
    matches: Vec<MapContact>,
    pub detail: bool,
}

impl Map {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        status: &Value,
        owner: &str,
        contacts: &Arc<Vec<Value>>,
        events: &[Value],
        page_scrolls: bool,
    ) -> Option<Action> {
        people::scroll_section(ui, ("correspondence-map", owner), page_scrolls, |ui| {
            self.contacts(ui, status, owner, contacts, events)
        })
    }

    fn contacts(
        &mut self,
        ui: &mut Ui,
        status: &Value,
        owner: &str,
        contacts: &Arc<Vec<Value>>,
        events: &[Value],
    ) -> Option<Action> {
        if contacts.is_empty() {
            ui.weak("No named contacts have been recorded yet.");
            return None;
        }
        let field = egui::TextEdit::singleline(&mut self.filter)
            .hint_text("Find a person or operator…")
            .desired_width(ui.available_width().min(340.));
        let changed = ui.add(field).changed();
        if changed
            || !self
                .source
                .as_ref()
                .is_some_and(|prior| Arc::ptr_eq(prior, contacts))
        {
            if changed {
                self.page = 0;
            }
            self.source = Some(Arc::clone(contacts));
            let query = self.filter.trim().to_lowercase();
            self.matches = contacts
                .iter()
                .filter_map(|row| {
                    let person: Contact =
                        serde_json::from_value(row["counterpart"].clone()).ok()?;
                    let label = contact_label(status, &person);
                    if !query.is_empty()
                        && !label.to_lowercase().contains(&query)
                        && !person.id.to_lowercase().contains(&query)
                    {
                        return None;
                    }
                    Some(MapContact {
                        person,
                        label,
                        sent: row["sent"].as_u64()?,
                        received: row["received"].as_u64()?,
                        shared: row["shared_in_activity"].as_u64()?,
                        latest: row["latest_encounter"].as_str().map(str::to_owned),
                    })
                })
                .collect();
        }
        if self.matches.is_empty() {
            ui.weak("No matching contacts.");
            return None;
        }
        let total = self.matches.len();
        self.page = self.page.min((total - 1) / PAGE_SIZE);
        let start = self.page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(total);
        let visible = &self.matches[start..end];
        ui.add_space(8.);
        let selected = if ui.available_width() >= 640. {
            graph(ui, visible, owner, name(status, owner))
        } else {
            let mut selected = None;
            for contact in visible {
                let size = egui::vec2(ui.available_width(), 54.);
                if ui.add_sized(size, contact.button(ui)).clicked() {
                    selected = Some(contact.person.clone());
                }
            }
            selected
        };
        ui.add_space(6.);
        if total > PAGE_SIZE {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(start > 0, egui::Button::new("Previous").small())
                    .clicked()
                {
                    self.page -= 1;
                }
                ui.small(format!("{}–{end} of {total}", start + 1));
                if ui
                    .add_enabled(end < total, egui::Button::new("Next").small())
                    .clicked()
                {
                    self.page += 1;
                }
            });
        }
        ui.weak("Sent and received count addressed exchanges. Shared counts contributions visible in a common activity.");
        let encounter = latest_encounters(ui, visible, events, status);
        selected.map(Action::Select).or(encounter)
    }
}

fn latest_encounters(
    ui: &mut Ui,
    contacts: &[MapContact],
    events: &[Value],
    status: &Value,
) -> Option<Action> {
    let mut entries: Vec<_> = contacts
        .iter()
        .filter_map(|contact| {
            let reference = contact.latest.as_deref()?;
            Some((contact, reference, reader::event(events, reference)))
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    entries.sort_by_key(|(_, _, event)| std::cmp::Reverse(event.and_then(|e| e["event"].as_i64())));
    ui.add_space(17.);
    ui.heading("Latest encounter with each contact");
    let mut action = None;
    for (contact, reference, event) in entries {
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong(&contact.label);
            let kind = event
                .map(|e| format!("{} from {}", title(s(e, "kind")), reader::sender(status, e)))
                .unwrap_or_else(|| "Open exchange".to_owned());
            if ui.link(kind).on_hover_text(reference).clicked() {
                action = Some(Action::Inspect(reference.to_owned()));
            }
            if let Some(event) = event {
                ui.small(s(event, "created_utc"));
            }
        });
        if let Some(event) = event {
            let body = &event["body"];
            if let Some(extract) = [
                "text",
                "reason",
                "title",
                "contribution",
                "finding",
                "description",
            ]
            .iter()
            .find_map(|key| body[*key].as_str().filter(|v| !v.trim().is_empty()))
            {
                ui.add(egui::Label::new(home::short_text(extract, 160)).truncate())
                    .on_hover_text(home::short_text(extract, 480));
            }
        }
    }
    action
}

impl MapContact {
    fn button(&self, ui: &Ui) -> egui::Button<'static> {
        let palette = theme::palette(ui);
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &self.label,
            0.,
            egui::TextFormat::simple(egui::FontId::proportional(16.), palette.text),
        );
        let counts = format!(
            "\n{} sent · {} received · {} shared",
            self.sent, self.received, self.shared
        );
        job.append(
            &counts,
            0.,
            egui::TextFormat::simple(egui::FontId::proportional(13.), palette.muted),
        );
        egui::Button::new(job)
            .fill(palette.panel)
            .stroke(Stroke::new(1., palette.border))
            .corner_radius(7)
            .wrap()
    }
}

fn graph(ui: &mut Ui, contacts: &[MapContact], owner: &str, owner_name: &str) -> Option<Contact> {
    let width = ui.available_width();
    let height = if contacts.len() > 2 { 228. } else { 149. };
    let (bounds, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let palette = theme::palette(ui);
    ui.painter().rect_filled(bounds, 9., palette.field);
    let center = bounds.center();
    let node_width = (width * 0.30).min(275.);
    let mut selected = None;
    for (n, contact) in contacts.iter().enumerate() {
        let column = n % 2;
        let count = (contacts.len() + 1 - column) / 2;
        let y = bounds.top() + height * (n / 2 + 1) as f32 / (count + 1) as f32;
        let x = if column == 0 {
            bounds.left() + 12. + node_width / 2.
        } else {
            bounds.right() - 12. - node_width / 2.
        };
        let node = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(node_width, 65.));
        let edge = if column == 0 {
            node.right_center()
        } else {
            node.left_center()
        };
        connection(ui, center, edge, contact);
        let response = ui.place(node, contact.button(ui));
        if response
            .on_hover_text(format!("Read exchanges with {}", contact.label))
            .clicked()
        {
            selected = Some(contact.person.clone());
        }
    }
    let ink = palette.ink(persona_color(owner));
    ui.painter().circle_filled(center, 28., palette.header);
    ui.painter()
        .circle_stroke(center, 28., Stroke::new(2., ink));
    let initial = owner_name.chars().next().unwrap_or('?').to_uppercase();
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(24.),
        ink,
    );
    let label = egui::Rect::from_center_size(
        center + egui::vec2(0., 43.),
        egui::vec2((width - 2. * node_width - 48.).max(1.), 21.),
    );
    ui.place(
        label,
        egui::Label::new(RichText::new(owner_name).strong()).truncate(),
    )
    .on_hover_text(owner_name);
    selected
}

fn connection(ui: &Ui, from: egui::Pos2, to: egui::Pos2, contact: &MapContact) {
    let palette = theme::palette(ui);
    let dir = (to - from).normalized();
    let start = from + dir * 29.;
    if contact.sent == 0 && contact.received == 0 {
        if contact.shared > 0 {
            ui.painter().extend(egui::Shape::dashed_line(
                &[start, to],
                Stroke::new(1., palette.muted),
                4.,
                4.,
            ));
        }
        return;
    }
    ui.painter()
        .line_segment([start, to], Stroke::new(1.25, palette.muted));
    let middle = start + (to - start) * 0.5;
    for (count, direction, color) in [(contact.sent, dir, TEAL), (contact.received, -dir, PURPLE)] {
        if count == 0 {
            continue;
        }
        let base = middle + direction * 3.;
        let normal = egui::vec2(-direction.y, direction.x) * 3.5;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![base + direction * 9., base + normal, base - normal],
            palette.ink(color),
            Stroke::NONE,
        ));
    }
}
