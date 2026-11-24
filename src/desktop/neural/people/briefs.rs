//! Browse a person's received request editions without changing their work.
use super::*;
use ai_personas::neural::briefs::{self, Hit, Page, Query};
use std::path::Path;

const PER_PAGE: usize = 3;

#[derive(Default)]
pub(in super::super) struct ReceivedBriefs {
    owner: String,
    root: PathBuf,
    term: String,
    regex: bool,
    pending: Option<mpsc::Receiver<Result<Page, String>>>,
    result: Option<Result<Page, String>>,
}
impl ReceivedBriefs {
    fn read(&mut self, ctx: &egui::Context, offset: usize) {
        let query = Query {
            query: self.term.clone(),
            regex: self.regex,
            offset,
            page_size: PER_PAGE,
        };
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.result = None;
        let (ctx, root, owner) = (ctx.clone(), self.root.clone(), self.owner.clone());
        std::thread::spawn(move || {
            let page = data::open(&root)
                .and_then(|db| briefs::search(&db, &owner, &query))
                .map_err(|e| e.to_string());
            let _ = tx.send(page);
            ctx.request_repaint();
        });
    }
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        root: &Path,
        owner: &str,
        name: &str,
    ) -> Option<String> {
        if self.owner != owner || self.root != root {
            *self = Self {
                owner: owner.into(),
                root: root.into(),
                ..Self::default()
            };
            self.read(ui.ctx(), 0);
        }
        if let Some(rx) = &self.pending {
            let reply = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The brief reader stopped.".into()))
                }
            };
            if let Some(r) = reply {
                self.result = Some(r);
                self.pending = None;
            }
        }
        section(ui, "Received briefs", name);
        ui.label("Search the requests and amendments that reached this person’s decisions. Receiving a brief doesn’t mean agreeing to do the work.");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.term)
                .desired_width(f32::INFINITY)
                .hint_text("Words from a request or amendment…"),
        );
        let mut submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal_wrapped(|ui| {
            let changed = ui.checkbox(&mut self.regex, "Regular expression").changed();
            if changed || edit.changed() {
                self.result = None;
                self.pending = None;
            }
            let caption = if self.result.is_some() {
                "Refresh"
            } else {
                "Search"
            };
            submit |= ui
                .add_enabled(self.pending.is_none(), egui::Button::new(caption))
                .clicked();
        });
        if submit {
            self.read(ui.ctx(), 0);
        }
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Checking received editions…");
            });
        }
        let page = match self.result.as_ref()? {
            Ok(page) => page,
            Err(error) => {
                ui.colored_label(theme::ACCENT, error);
                return None;
            }
        };
        if page.total == 0 {
            ui.weak(if self.term.is_empty() {
                "No brief edition found in this person’s accepted decisions."
            } else {
                "No received brief matches these words."
            });
            return None;
        }
        let mut next = None;
        ui.horizontal_wrapped(|ui| {
            ui.weak(format!(
                "{}–{} of {} · newest editions first",
                page.offset + 1,
                page.offset + page.items.len(),
                page.total
            ));
            if page.offset > 0 && ui.small_button("← Previous").clicked() {
                next = Some(page.offset.saturating_sub(PER_PAGE));
            }
            if let Some(offset) = page.next_offset {
                if ui.small_button("Next →").clicked() {
                    next = Some(offset);
                }
            }
        });
        let mut open = None;
        for hit in &page.items {
            ui.push_id((&self.owner, &hit.reference), |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let badge = if hit.kind == "social" {
                        "Conversation"
                    } else {
                        "Task"
                    };
                    ui.horizontal_wrapped(|ui| {
                        theme::badge(ui, badge, PURPLE);
                        ui.strong(&hit.requester);
                        ui.weak(format!("Revision {}", hit.revision))
                            .on_hover_text(&hit.reference);
                    });
                    if !self.term.is_empty()
                        && (hit.preview_bytes[0] > 0 || hit.amendment_number.is_some())
                    {
                        ui.strong(home::short_text(s(&hit.record, "original_need"), 96));
                    }
                    if let Some(n) = hit.amendment_number {
                        ui.small(format!("Match in amendment {n}"));
                    }
                    preview(ui, hit);
                    if ui
                        .link("Receiving decision ↗")
                        .on_hover_text(&hit.receiving_decision)
                        .clicked()
                    {
                        open = Some(hit.receiving_decision.clone());
                    }
                    ui.collapsing("Read this edition", |ui| full_brief(ui, hit));
                });
                ui.add_space(5.);
            });
        }
        if let Some(offset) = next {
            self.read(ui.ctx(), offset);
        }
        open
    }
}
fn preview(ui: &mut Ui, hit: &Hit) {
    let [left, right] = hit.preview_bytes;
    let start = hit.match_bytes[0] - left;
    let end = hit.match_bytes[1].min(right) - left;
    let normal = egui::TextFormat::simple(
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    let mut mark = normal.clone();
    mark.underline = egui::Stroke::new(1., theme::ACCENT);
    let mut text = egui::text::LayoutJob::default();
    if left > 0 {
        text.append("…", 0., normal.clone());
    }
    text.append(&hit.preview[..start], 0., normal.clone());
    text.append(&hit.preview[start..end], 0., mark);
    text.append(&hit.preview[end..], 0., normal.clone());
    if right < hit.field_bytes {
        text.append("…", 0., normal);
    }
    ui.add(egui::Label::new(text).wrap());
}

fn full_brief(ui: &mut Ui, hit: &Hit) {
    ui.strong("Original request");
    let need = s(&hit.record, "original_need");
    ui.add(
        egui::Label::new(if need.is_empty() {
            "No opening topic."
        } else {
            need
        })
        .wrap()
        .selectable(true),
    );
    for (index, item) in hit.record["amendments"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        ui.strong(format!("Amendment {} · {}", index + 1, s(item, "author")));
        ui.add(egui::Label::new(s(item, "text")).wrap().selectable(true));
    }
    if ui
        .small_button("Copy edition reference")
        .on_hover_text(&hit.reference)
        .clicked()
    {
        ui.ctx().copy_text(hit.reference.clone());
    }
}
