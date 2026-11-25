//! Contact sheet for one person's recorded visual inputs. Thumbnails are
//! display derivatives; opening a picture resolves the original byte object.
use super::*;
use ai_personas::neural::{store, visual_history::{self, Image, Page, Query, Source}};
use std::path::Path;

const PER_PAGE: usize = 3;
type Pictures = BTreeMap<String, Result<egui::TextureHandle, String>>;
type Sheet = Result<(Page, Pictures), String>;

#[derive(Default)]
pub(in super::super) struct VisualHistory {
    owner: String,
    root: PathBuf,
    term: String,
    regex: bool,
    pending: Option<mpsc::Receiver<Sheet>>,
    result: Option<Sheet>,
}

fn thumbnail(db: &rusqlite::Connection, ctx: &egui::Context, item: &Image) -> Result<egui::TextureHandle, String> {
    let digest = item.reference.strip_prefix("object:").ok_or("Missing image object")?;
    let bytes = store::bytes(db, digest).map_err(|e| e.to_string())?;
    let image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let pixels = image.thumbnail(300, 180).to_rgba8();
    let size = [pixels.width() as usize, pixels.height() as usize];
    let preview = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
    Ok(ctx.load_texture(format!("visual-history-{digest}"), preview, egui::TextureOptions::LINEAR))
}

fn picture(ui: &mut Ui, tex: &Result<egui::TextureHandle, String>) -> bool {
    match tex {
        Ok(tex) => {
            let v = tex.size_vec2();
            let size = v * (ui.available_width().min(150.) / v.x).min(90. / v.y);
            ui.add(egui::Image::new(tex).fit_to_exact_size(size).sense(egui::Sense::click()))
                .on_hover_text("Open original image").clicked()
        }
        Err(reason) => {
            ui.weak("No thumbnail").on_hover_text(reason);
            false
        }
    }
}

fn source_label(ui: &mut Ui, source: &Source) {
    let via = if source.via == "selection" { "Selected resource" } else { &source.via };
    let mut label = via.to_owned();
    if let Some(path) = &source.path {
        let name = Path::new(path).file_name().and_then(|s| s.to_str()).unwrap_or(path);
        label.push_str(&format!(" · {name}"));
    } else if let Some(n) = source.page {
        label.push_str(&format!(" · Page {n}"));
    } else if let Some(operation) = &source.operation {
        label.push_str(&format!(" · {operation}"));
    }
    ui.small(home::short_text(&label, 80)).on_hover_ui(|ui| {
        for field in [Some(&source.reference), source.path.as_ref(), source.rendered_from.as_ref(),
            source.operation.as_ref(), source.event.as_ref()].into_iter().flatten() {
            ui.small(field);
        }
    });
}

fn appearance(ui: &mut Ui, item: &Image, pictures: &Pictures, activities: &home_index::Index) -> Option<String> {
    let heading = |ui: &mut Ui| {
        theme::badge(ui, &format!("Image {} of {}", item.position, item.image_count), TEAL);
        let activity = activities.activity(&item.work).map_or(item.work.as_str(), |a| &a.title);
        ui.strong(home::short_text(activity, 76)).on_hover_text(activity);
        let mut caption = format!("{} · {} bytes", item.mime, number(item.bytes));
        if let Some(n) = item.observation_through { caption.push_str(&format!(" · input through event {n}")); }
        ui.small(caption);
    };
    let tex = &pictures[&item.reference];
    let mut image = if ui.available_width() >= 370. {
        ui.horizontal_top(|ui| {
            let clicked = picture(ui, tex);
            ui.vertical(heading);
            clicked
        }).inner
    } else {
        let clicked = picture(ui, tex);
        heading(ui);
        clicked
    };
    for source in &item.sources_in_input { source_label(ui, source); }
    let mut decision = false;
    ui.horizontal_wrapped(|ui| {
        image |= ui.link("Open image ↗").on_hover_text(&item.reference).clicked();
        decision = ui.link("Receiving decision ↗").on_hover_text(&item.decision).clicked();
        if ui.small_button("Copy image reference").clicked() { ui.ctx().copy_text(item.reference.clone()); }
    });
    if decision { Some(item.decision.clone()) } else if image { Some(item.reference.clone()) } else { None }
}

impl VisualHistory {
    pub(in super::super) fn open_decision(&mut self, ctx: &egui::Context, root: &Path, owner: &str, call: &str) {
        let decision = format!("experience:{call}@1");
        *self = Self { root: root.into(), owner: owner.into(), term: decision.clone(), ..Default::default() };
        self.load(ctx, 0, Some(decision));
    }

    fn load(&mut self, ctx: &egui::Context, offset: usize, through_decision: Option<String>) {
        let q = Query { query: self.term.clone(), regex: self.regex, work: None, offset, through_decision, page_size: PER_PAGE };
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.result = None;
        let (ctx, root, owner) = (ctx.clone(), self.root.clone(), self.owner.clone());
        std::thread::spawn(move || {
            let result = data::open(&root).and_then(|db| {
                let page = visual_history::search(&db, &owner, &q)?;
                let mut pictures = Pictures::new();
                for item in &page.items {
                    pictures.entry(item.reference.clone()).or_insert_with(|| thumbnail(&db, &ctx, item));
                }
                Ok((page, pictures))
            });
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    pub(super) fn show(&mut self, ui: &mut Ui, root: &Path, owner: &str, label: &str, activities: &home_index::Index) -> Option<String> {
        if self.owner != owner || self.root != root {
            *self = Self { owner: owner.into(), root: root.into(), ..Default::default() };
            self.load(ui.ctx(), 0, None);
        }
        if let Some(rx) = &self.pending {
            let answer = match rx.try_recv() {
                Ok(page) => Some(page),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The image reader stopped.".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(answer) = answer { self.result = Some(answer); self.pending = None; }
        }
        section(ui, "Visual history", label);
        ui.label(format!("Images supplied to {label}’s accepted decisions, in their original input order. Open a decision to read the judgment separately."));
        let field = ui.add(egui::TextEdit::singleline(&mut self.term).desired_width(f32::INFINITY)
            .hint_text("Filename, page, tool or exact reference…"));
        let mut submit = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut self.regex, "Regular expression").changed() || field.changed() {
                self.result = None;
                self.pending = None;
            }
            let action = if self.result.is_some() { "Refresh" } else { "Search" };
            submit |= ui.add_enabled(self.pending.is_none(), egui::Button::new(action)).clicked();
        });
        if submit { self.load(ui.ctx(), 0, None); }
        if self.pending.is_some() {
            ui.horizontal(|ui| { ui.spinner(); ui.weak("Checking image inputs…"); });
        }
        let (page, pictures) = match self.result.as_ref()? {
            Ok(result) => result,
            Err(error) => {
                ui.strong("Visual history unavailable");
                ui.colored_label(theme::ACCENT, error);
                ui.weak("Edit the search or use Refresh to check again.");
                return None;
            }
        };
        if page.items.is_empty() {
            ui.weak("No matching images in this person’s accepted decisions.");
            return None;
        }
        let mut offset = None;
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("Appearances {}–{} · newest decision first", page.offset + 1, page.offset + page.items.len()));
            if ui.add_enabled(page.offset > 0, egui::Button::new("← Previous")).clicked() {
                offset = Some(page.offset.saturating_sub(PER_PAGE));
            }
            if ui.add_enabled(page.next_offset.is_some(), egui::Button::new("Next →")).clicked() { offset = page.next_offset; }
        });
        let mut open = None;
        for item in &page.items {
            ui.push_id((&item.decision, item.position), |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(link) = appearance(ui, item, pictures, activities) { open = Some(link); }
                });
            });
            ui.add_space(4.);
        }
        if let Some(n) = offset { self.load(ui.ctx(), n, page.through_decision.clone()); }
        open
    }
}
