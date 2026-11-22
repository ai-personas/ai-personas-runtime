//! Follow an issued brief through its own activity and back to the requester.
use super::*;
use ai_personas::neural::activity_requests::{history, Query};
use std::path::Path;

pub(super) enum Action {
    Inspect(String),
    OpenWork(String),
}

#[derive(Default)]
pub(super) struct Window {
    work: Option<String>,
    selected: Option<String>,
    list_offset: usize,
    exchange_offset: usize,
    cursor: i64,
    reader: Option<mpsc::Receiver<Result<(Value, Option<Value>), String>>>,
    report: Option<Result<(Value, Option<Value>), String>>,
}

const PAGE: usize = 6;

impl Window {
    pub(super) fn open(&mut self, ctx: &egui::Context, root: &Path, work: &str, cursor: i64) {
        self.open_request(ctx, root, work, None, cursor);
    }

    pub(super) fn open_request(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        work: &str,
        request: Option<&str>,
        cursor: i64,
    ) {
        *self = Self {
            work: Some(work.into()),
            selected: request.map(str::to_owned),
            ..Self::default()
        };
        self.read(ctx, root, cursor);
    }

    fn read(&mut self, ctx: &egui::Context, root: &Path, cursor: i64) {
        let Some(work) = self.work.clone() else {
            return;
        };
        let (list_offset, exchange_offset) = (self.list_offset, self.exchange_offset);
        let requested = self.selected.clone();
        self.cursor = cursor;
        let (tx, rx) = mpsc::channel();
        self.reader = Some(rx);
        let (root, ctx) = (root.to_owned(), ctx.clone());
        std::thread::spawn(move || {
            let result = data::open(&root).and_then(|db| {
                let mut list = history(
                    &db,
                    &work,
                    None,
                    &Query {
                        request: None,
                        offset: list_offset,
                        page_size: PAGE,
                    },
                )?;
                list["tree"] = ai_personas::neural::activity_tree::describe(&db,&work)?;
                let selected = requested
                    .as_deref()
                    .or_else(|| list["items"][0]["request"].as_str());
                let detail = selected
                    .map(|request| {
                        history(
                            &db,
                            &work,
                            None,
                            &Query {
                                request: Some(request.into()),
                                offset: exchange_offset,
                                page_size: PAGE,
                            },
                        )
                    })
                    .transpose()?
                    .and_then(|r| r["items"].as_array()?.first().cloned());
                Ok((list, detail))
            });
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        status: &Value,
        cursor: i64,
    ) -> Option<Action> {
        let work = self.work.clone()?;
        if let Some(rx) = &self.reader {
            match rx.try_recv() {
                Ok(result) => {
                    self.report = Some(result);
                    self.reader = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.report = Some(Err("The activity reader stopped".into()));
                    self.reader = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut visible = true;
        let mut reload = self.reader.is_none() && self.cursor != cursor;
        let mut action = None;
        egui::Window::new("Requested activities").id(egui::Id::new("requested-activities"))
            .open(&mut visible).default_width(690.).default_height(550.).resizable(true)
            .show(ctx,|ui|{
                let Some(result)=&self.report else {ui.spinner();return;};
                let (list,selected)=match result {
                    Ok(pair)=>pair,
                    Err(problem)=>{ui.colored_label(theme::ACCENT,problem);reload|=ui.button("Try again").clicked();return;}
                };
                let count=list["total"].as_u64().unwrap_or(0) as usize;
                if let Some(activities)=list["tree"]["activities"].as_array() {
                    ui.collapsing(format!("Run scope  ·  {} activities",activities.len()),|ui|{
                        egui::ScrollArea::vertical().id_salt("run-scope").max_height(205.).show(ui,|ui|{
                            for node in activities {
                                ui.push_id(s(node,"work"),|ui|{
                                    ui.horizontal_wrapped(|ui|{
                                        ui.add_space((node["depth"].as_u64().unwrap_or(0) as f32*12.).min(60.));
                                        theme::glyph(ui,Icon::Task,TEAL);
                                        if ui.link(excerpt(s(node,"need"),70)).on_hover_text(s(node,"need")).clicked(){action=Some(Action::OpenWork(s(node,"work").into()));}
                                        for p in node["participants"].as_array().into_iter().flatten(){
                                            let state=if p["pending_resume"].is_object(){"resume requested"}else{s(&p["continuation"],"kind")};
                                            ui.weak(format!("{} · {}",person(status,s(p,"persona")),state));
                                        }
                                    });
                                });
                            }
                        });
                    });
                    ui.separator();
                }
                if count==0 {
                    ui.heading("No requests in this activity");
                    ui.weak("A separate activity opened by a persona will appear here with its replies and deliveries.");
                    return;
                }
                if count>1 {
                    ui.horizontal_wrapped(|ui|{
                        if let Some(next)=pages(ui,"activities",self.list_offset,count){
                            self.list_offset=next;self.exchange_offset=0;self.selected=None;reload=true;
                        }
                        for item in list["items"].as_array().into_iter().flatten() {
                            let req=s(item,"request");
                            if ui.selectable_label(selected.as_ref().is_some_and(|r|r["request"]==req),excerpt(s(item,"need"),34))
                                .on_hover_text(s(item,"need")).clicked() {
                                self.selected=Some(req.into());self.exchange_offset=0;reload=true;
                            }
                        }
                    });
                    ui.separator();
                }
                let Some(route)=selected else{return};
                let parent=s(route,"parent");
                let child=s(route,"child");
                ui.horizontal_wrapped(|ui|{
                    eyebrow(ui,"Activity correspondence");
                    ui.separator();
                    ui.weak(format!("Requested by {}",person(status,s(route,"requester"))));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        reload|=ui.small_button("Refresh").clicked();
                    });
                });
                ui.add_space(6.);
                let col_width=(ui.available_width()-ui.spacing().item_spacing.x)/2.;
                let endpoints=[("Originating activity",parent,TEAL,s(route,"parent_brief")),
                    ("Requested activity",child,PURPLE,s(route,"request"))];
                ui.columns(2,|cols|{
                    for (col,(name,id,color,brief)) in cols.iter_mut().zip(endpoints) {
                        egui::Frame::new().fill(theme::palette(col).header).inner_margin(10).corner_radius(6)
                            .stroke(Stroke::new(1.,color.gamma_multiply(0.45))).show(col,|ui|{
                                ui.set_min_width((col_width-26.).max(50.));
                                ui.set_max_width((col_width-22.).max(60.));
                                ui.label(RichText::new(name).strong().color(color));
                                let title=work_name(status,id);
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min),|ui|{
                                    ui.add(egui::Label::new(excerpt(title,92)).wrap()).on_hover_text(title);
                                });
                                ui.horizontal_wrapped(|ui|{
                                    if id==work {ui.weak("Current workspace");}
                                    else if ui.small_button("Open activity →").clicked(){action=Some(Action::OpenWork(id.into()));}
                                    resource(ui,"Brief ↗",brief,&mut action);
                                });
                            });
                    }
                });
                ui.horizontal_wrapped(|ui|{
                    let names=route["invited"].as_array().into_iter().flatten().filter_map(Value::as_str)
                        .map(|id|person(status,id)).collect::<Vec<_>>();
                    ui.small(if names.is_empty(){"No founding invitees".to_owned()}else{format!("Originally invited · {}",names.join(", "))});
                    resource(ui,"Requesting decision ↗",s(route,"decision"),&mut action);
                });
                ui.add_space(8.);
                let correspondence=&route["correspondence"];
                let total=correspondence["total"].as_u64().unwrap_or(0) as usize;
                ui.horizontal_wrapped(|ui|{
                    ui.strong("Correspondence");
                    if let Some(next)=pages(ui,"exchanges",self.exchange_offset,total){
                        self.exchange_offset=next;reload=true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt(("requested-correspondence",child,self.exchange_offset))
                    .show(ui,|ui|{
                        if total==0 {ui.weak("No correspondence has arrived on this route.");}
                        for exchange in correspondence["items"].as_array().into_iter().flatten(){
                            ui.push_id(s(exchange,"experience"),|ui|draw_exchange(ui,status,exchange,&mut action));
                            ui.add_space(6.);
                        }
                    });
            });
        if !visible || action.is_some() {
            *self = Self::default();
        } else if reload {
            self.read(ctx, root, cursor);
        }
        action
    }
}

fn work_name<'a>(status: &'a Value, id: &'a str) -> &'a str {
    status["work"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["body"]["id"] == id)
        .map(|r| activity_title(&r["body"]))
        .unwrap_or(id)
}
fn person<'a>(status: &'a Value, id: &'a str) -> &'a str {
    status["personas"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["body"]["id"] == id)
        .and_then(|r| r["body"]["label"].as_str())
        .unwrap_or(id)
}
fn pages(ui: &mut Ui, label: &str, offset: usize, total: usize) -> Option<usize> {
    if total <= PAGE {
        ui.small(format!("{total} {label}"));
        return None;
    }
    let next = ui
        .add_enabled(offset > 0, egui::Button::new("← Newer"))
        .clicked();
    ui.small(format!(
        "{}–{} of {total}",
        offset + 1,
        (offset + PAGE).min(total)
    ));
    let prev = ui
        .add_enabled(offset + PAGE < total, egui::Button::new("Older →"))
        .clicked();
    if next {
        Some(offset.saturating_sub(PAGE))
    } else if prev {
        Some(offset + PAGE)
    } else {
        None
    }
}
fn resource(ui: &mut Ui, label: &str, r: &str, action: &mut Option<Action>) {
    if !r.is_empty() && ui.small_button(label).on_hover_text(r).clicked() {
        *action = Some(Action::Inspect(r.to_owned()));
    }
}
pub(super) fn label(kind: &str) -> Option<(&'static str, Color32)> {
    match kind {
        "request_delivery" => Some(("Delivery returned", TEAL)),
        "request_reply" => Some(("Reply returned", TEAL)),
        "request_joined" => Some(("Invitation accepted", TEAL)),
        "request_declined" => Some(("Invitation declined", theme::ACCENT)),
        "request_left" => Some(("Participant left", theme::ACCENT)),
        "requester_message" | "request_message_sent" => Some(("Requester message", PURPLE)),
        "requester_review" | "request_review_sent" => Some(("Requester review", PURPLE)),
        _ => None,
    }
}
fn draw_exchange(ui: &mut Ui, status: &Value, item: &Value, action: &mut Option<Action>) {
    let (name, color) = label(s(item, "kind")).unwrap_or(("Exchange", TEAL));
    let body = &item["body"];
    card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            theme::badge(ui, name, color);
            ui.strong(person(status, s(item, "author")));
            ui.weak("→");
            ui.label(person(status, s(item, "recipient")));
            ui.weak(time_label(item["time"].as_i64().unwrap_or(0)));
        });
        if let Some(accepted) = body["review"]["accepted"].as_bool() {
            ui.label(
                RichText::new(if accepted {
                    "Edition accepted"
                } else {
                    "Edition declined"
                })
                .strong()
                .color(color),
            );
        }
        if let Some(title) = body["title"].as_str() {
            ui.label(RichText::new(title).strong());
        }
        if let Some(text) = body["text"]
            .as_str()
            .or_else(|| body["supported_use"].as_str())
            .or_else(|| body["reason"].as_str())
        {
            document::message_markdown(ui, text);
        }
        for gap in body["remaining"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            ui.label(format!("Remaining · {gap}"));
        }
        ui.horizontal_wrapped(|ui| {
            let delivery = body["delivery"]
                .as_str()
                .or_else(|| body["review"]["delivery"].as_str())
                .unwrap_or("");
            resource(ui, "Open exact delivery ↗", delivery, action);
            for file in item["resources"].as_array().into_iter().flatten() {
                let r = s(file, "reference");
                let name = file["path"]
                    .as_str()
                    .and_then(|path| path.rsplit('/').next())
                    .unwrap_or(r);
                resource(ui, &format!("{} ↗", excerpt(name, 40)), r, action);
            }
            let source = body["source"]
                .as_str()
                .or_else(|| body["sent"].as_str())
                .unwrap_or("");
            resource(ui, "Sent exchange ↗", source, action);
            resource(ui, "Inbox event ↗", s(item, "experience"), action);
            if let Some(r) = item["first_supplied"].as_str() {
                resource(ui, "First decision with this exchange ↗", r, action);
            } else {
                ui.weak("In inbox only");
            }
        });
    });
}
