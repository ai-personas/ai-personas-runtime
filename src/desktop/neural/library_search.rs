use super::*;
use ai_personas::neural::{
    self,
    discovery::{self, Catalog, FindText},
};
use serde_json::json;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

enum Reply {
    Page(Value),
    Excerpt(Value, [usize; 2]),
}

#[derive(Default)]
pub(super) struct Window {
    open: bool,
    document: Option<String>,
    term: String,
    regex: bool,
    history: bool,
    nearby: usize,
    offset: usize,
    pending: Option<Receiver<Result<Reply, String>>>,
    report: Option<Value>,
    excerpt: Option<(Value, [usize; 2])>,
    error: Option<String>,
}

impl Window {
    pub(super) fn catalogue(&mut self, term: String) {
        *self = Self {
            open: true,
            term,
            ..Default::default()
        };
    }
    pub(super) fn document(&mut self, reference: String) {
        *self = Self {
            open: true,
            document: Some(reference),
            ..Default::default()
        };
    }
    fn clear(&mut self) {
        self.offset = 0;
        self.report = None;
        self.excerpt = None;
        self.error = None;
        self.pending = None;
    }
    fn worker(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        action: impl FnOnce(&rusqlite::Connection) -> neural::Result<Reply> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        self.error = None;
        let root = root.to_path_buf();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let answer = data::open(&root)
                .and_then(|db| action(&db))
                .map_err(|e| e.to_string());
            let _ = sender.send(answer);
            ctx.request_repaint();
        });
    }
    fn search(&mut self, ctx: &egui::Context, root: &Path) {
        let query = self.term.clone();
        let regex = self.regex;
        let offset = self.offset;
        self.excerpt = None;
        self.report = None;
        if let Some(reference) = self.document.clone() {
            let find = FindText {
                reference,
                query,
                regex,
                offset,
                page_size: 20,
                case_sensitive: false,
                excerpts: false,
                before: self.nearby,
                after: self.nearby,
            };
            self.worker(ctx, root, move |db| {
                Ok(Reply::Page(discovery::find(db, &find)?))
            });
        } else {
            let catalog = Catalog {
                query,
                regex,
                offset,
                page_size: 20,
                history: self.history,
                author: None,
                work: None,
            };
            self.worker(ctx, root, move |db| {
                Ok(Reply::Page(discovery::catalog(db, &catalog)?))
            });
        }
    }
    fn poll(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        let reply = match receiver.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Library search worker closed".to_owned()),
        };
        self.pending = None;
        match reply {
            Ok(Reply::Page(report)) => self.report = Some(report),
            Ok(Reply::Excerpt(text, range)) => self.excerpt = Some((text, range)),
            Err(reason) => self.error = Some(reason),
        }
    }
    pub(super) fn show(&mut self, ctx: &egui::Context, root: &Path) -> Option<String> {
        if !self.open {
            return None;
        }
        self.poll();
        let mut open = true;
        let mut open_reference = None;
        let title = if self.document.is_some() {
            "Find in this version"
        } else {
            "Search retained library"
        };
        egui::Window::new(title).id(egui::Id::new("neural-library-search")).open(&mut open)
            .default_width(620.).resizable(true).show(ctx,|ui|{
                if let Some(reference)=&self.document{ui.small(reference);}
                let edit=ui.horizontal(|ui|{
                    let edit=ui.add(egui::TextEdit::singleline(&mut self.term).hint_text(if self.document.is_some(){"Text or pattern…"}else{"Name, cue or qualification…"}).desired_width((ui.available_width()-83.).max(100.)));
                    let submit=ui.add_enabled(self.document.is_none() || !self.term.is_empty(),theme::primary("Search")).clicked()
                        || (edit.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter)));
                    (edit.changed(),submit)
                }).inner;
                let changed=ui.horizontal_wrapped(|ui|{
                    let pattern=ui.checkbox(&mut self.regex,"Regular expression").changed();
                    let scope=if self.document.is_some(){
                        ui.label("Surrounding lines");ui.add(egui::DragValue::new(&mut self.nearby)).changed()
                    }else{ui.checkbox(&mut self.history,"Include earlier revisions").changed()};
                    pattern || scope
                }).inner;
                if edit.0 || changed{self.clear();}
                if edit.1{self.offset=0;self.search(ctx,root);}
                if self.pending.is_some(){ui.spinner();}
                if let Some(reason)=&self.error{ui.colored_label(theme::ACCENT,reason);}
                if let Some((text,range))=&self.excerpt{
                    let base=text["range"][0].as_u64().unwrap_or(0) as usize;
                    if let Some(raw)=text["text"].as_str(){
                        ui.separator();
                        ui.horizontal_wrapped(|ui|{
                            ui.strong(format!("Bytes {}–{}",base,text["range"][1]));
                            if ui.small_button("Copy selection").clicked(){
                                ui.ctx().copy_text(json!({"reference":text["reference"],"start":base,"end":text["range"][1]}).to_string());
                            }
                        });
                        let (a,b)=(range[0].saturating_sub(base),range[1].saturating_sub(base));
                        let mut job=egui::text::LayoutJob::default();
                        let format=egui::TextFormat{font_id:egui::FontId::monospace(13.),color:ui.visuals().text_color(),..Default::default()};
                        if let (Some(before),Some(hit),Some(after))=(raw.get(..a),raw.get(a..b),raw.get(b..)){
                            job.append(before,0.,format.clone());
                            job.append(hit,0.,egui::TextFormat{background:ui.visuals().selection.bg_fill,..format.clone()});
                            job.append(after,0.,format);
                        }else{job.append(raw,0.,format);}
                        egui::ScrollArea::vertical().id_salt("found-text").max_height(132.).show(ui,|ui|{ui.add(egui::Label::new(job).wrap().selectable(true));});
                    }
                }
                let Some(report)=&self.report else{return;};
                if report["searchable"]==false{ui.label(s(report,"reason"));return;}
                let total=report["total"].as_u64().unwrap_or(0) as usize;
                let items=report["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                ui.separator();
                ui.small(if items.is_empty(){format!("{total} matches")}else{format!("{}–{} of {total} matches",self.offset+1,self.offset+items.len())});
                let mut next=None;let mut read=None;let mut find=None;
                egui::ScrollArea::vertical().id_salt("search-hits").max_height(270.).show(ui,|ui|{
                    for (i,item) in items.iter().enumerate(){
                        ui.push_id(i,|ui|{
                            if self.document.is_some(){
                                let line=&item["match_lines"];let range=&item["match_bytes"];let selection=&item["selection"];
                                ui.horizontal_wrapped(|ui|{
                                    ui.strong(if line[0]==line[1]{format!("Line {}",line[0])}else{format!("Lines {}–{}",line[0],line[1])});
                                    ui.small(format!("bytes {}–{}",range[0],range[1]));
                                    if ui.small_button("Read excerpt ↗").clicked(){read=Some((selection.clone(),range.clone()));}
                                });
                            }else{
                                ui.strong(s(item,"path"));
                                ui.horizontal_wrapped(|ui|{
                                    let reference=s(item,"reference");
                                    if item["retired"]==true{ui.weak("Retired");}
                                    if ui.small_button("Open version ↗").on_hover_text(reference).clicked(){open_reference=Some(reference.into());}
                                    if ui.small_button("Find in version…").clicked(){find=Some(reference.to_owned());}
                                    if item["current_reference"]!=reference && ui.small_button("Open current ↗").on_hover_text(s(item,"current_path")).clicked(){open_reference=Some(s(item,"current_reference").into());}
                                });
                            }
                            ui.separator();
                        });
                    }
                });
                ui.horizontal(|ui|{
                    if self.offset>0 && ui.small_button("← Previous").clicked(){next=Some(self.offset.saturating_sub(20));}
                    if let Some(offset)=report["next_offset"].as_u64(){if ui.small_button("Next →").clicked(){next=Some(offset as usize);}}
                });
                if let Some(offset)=next{self.offset=offset;self.search(ctx,root);}
                if let Some(reference)=find{self.document(reference);}
                if let Some((selection,range))=read{
                    self.worker(ctx,root,move|db|{
                        let ref_=selection["reference"].as_str().ok_or_else(||neural::failure("Missing selected reference"))?;
                        let at=|n:&Value|n.as_u64().and_then(|v|usize::try_from(v).ok()).ok_or_else(||neural::failure("Invalid selected byte offset"));
                        Ok(Reply::Excerpt(neural::context::read(db,ref_,Some(at(&selection["start"])?),Some(at(&selection["end"])?))?,[at(&range[0])?,at(&range[1])?]))
                    });
                }
            });
        self.open = open && open_reference.is_none();
        open_reference
    }
}
