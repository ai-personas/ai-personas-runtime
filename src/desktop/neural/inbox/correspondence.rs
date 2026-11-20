//! The recorded human return and the requester’s responses to that exact return.
use super::*;
use std::path::Path;

const PAGE: usize = 10;
type Page = Result<Arc<Value>, String>;

#[derive(Default)]
pub(super) struct Panel {
    request: String,
    offset: usize,
    observed: Option<(String, Option<i64>)>,
    reader: Option<mpsc::Receiver<Page>>,
    page: Option<Page>,
}

impl Panel {
    fn update(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        reference: &str,
        last_event: Option<i64>,
    ) {
        let (request, _) = reference.rsplit_once('@').unwrap();
        if self.request != request {
            *self = Self {
                request: request.into(),
                ..Default::default()
            };
        }
        if let Some(receiver) = &self.reader {
            let result = match receiver.try_recv() {
                Ok(page) => page,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The correspondence reader stopped.".into())
                }
                Err(mpsc::TryRecvError::Empty) => return,
            };
            if let Ok(page) = &result {
                self.offset = page["offset"].as_u64().unwrap_or_default() as usize;
            }
            self.reader = None;
            self.page = Some(result);
        }
        if self
            .observed
            .as_ref()
            .is_some_and(|(r, e)| r == reference && *e == last_event)
        {
            return;
        }
        let previous_total = self
            .page
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|page| page["total"].as_u64())
            .map(|n| n as usize);
        self.observed = Some((reference.into(), last_event));
        let (tx, rx) = mpsc::channel();
        self.reader = Some(rx);
        let (ctx, root, reference, offset) = (
            ctx.clone(),
            root.to_owned(),
            reference.to_owned(),
            self.offset,
        );
        std::thread::spawn(move || {
            let result = data::open(&root)
                .and_then(|db| {
                    use ai_personas::neural::human_requests::correspondence;
                    let page = correspondence(&db, &reference, offset, PAGE)?;
                    let total = page["total"].as_u64().unwrap_or_default() as usize;
                    // Newer arrivals must not move a reader looking at an older page.
                    if let Some(old) = previous_total.filter(|old| offset > 0 && *old != total) {
                        let anchored = offset
                            .saturating_add(total)
                            .saturating_sub(old)
                            .min(total.saturating_sub(1));
                        if anchored != offset {
                            return correspondence(&db, &reference, anchored, PAGE);
                        }
                    }
                    Ok(page)
                })
                .map(Arc::new)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn go(&mut self, offset: usize) {
        self.offset = offset;
        self.observed = None;
        self.page = None;
        self.reader = None;
    }
}

impl App {
    pub(super) fn request_correspondence(
        &mut self,
        ui: &mut Ui,
        reference: &str,
        work: &str,
        requester: &str,
        show_heading: bool,
    ) {
        let latest = self.home_index.latest_event(work);
        self.inbox
            .correspondence
            .update(ui.ctx(), &self.root, reference, latest);
        let panel = &self.inbox.correspondence;
        if show_heading {
            ui.horizontal_wrapped(|ui| {
                theme::glyph(ui, Icon::Conversation, theme::palette(ui).accent);
                ui.label(RichText::new("Correspondence").size(20.).strong());
                if panel.reader.is_some() {
                    ui.add(egui::Spinner::new().size(13.));
                }
            });
        }
        let Some(page) = panel.page.as_ref() else {
            ui.weak("Reading this request’s correspondence…");
            return;
        };
        let page = match page {
            Ok(page) => Arc::clone(page),
            Err(reason) => {
                ui.strong("Correspondence unavailable");
                ui.add(egui::Label::new(reason).wrap());
                if ui.button("Retry correspondence").clicked() {
                    self.inbox.correspondence.observed = None;
                }
                return;
            }
        };
        let total = page["total"].as_u64().unwrap_or(0) as usize;
        let reports = page["report_count"].as_u64().unwrap_or(0);
        let progress = page["progress_count"].as_u64().unwrap_or(0);
        let replies = page["reply_count"].as_u64().unwrap_or(0);
        ui.horizontal_wrapped(|ui| {
            if total > 0 {
                let mut counts = vec![counted(reports, "human report", "human reports")];
                if progress > 0 {
                    counts.push(counted(progress, "ownership change", "ownership changes"));
                }
                counts.push(counted(
                    replies,
                    "requester response",
                    "requester responses",
                ));
                ui.small(counts.join("  ·  "));
            } else {
                ui.weak("No verified human returns or requester responses yet.");
            }
            if let Some(origin) = page["origin"]["experience"].as_str() {
                if ui
                    .link(RichText::new("Original request").small())
                    .on_hover_text(origin)
                    .clicked()
                {
                    self.open_resource(ui.ctx(), origin.into());
                }
            }
        });
        let missing_reports = page["unverified_report_count"].as_u64().unwrap_or(0);
        let missing_replies = page["unverified_reply_count"].as_u64().unwrap_or(0);
        if missing_reports + missing_replies > 0 {
            let mut missing = Vec::new();
            if missing_reports > 0 {
                missing.push(counted(missing_reports, "human return", "human returns"));
            }
            if missing_replies > 0 {
                missing.push(counted(
                    missing_replies,
                    "requester response",
                    "requester responses",
                ));
            }
            ui.small(format!("Incomplete evidence for {}", missing.join(" and ")));
        }
        let offset = page["offset"].as_u64().unwrap_or(0) as usize;
        if total > PAGE {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(offset > 0, egui::Button::new("Newer"))
                    .clicked()
                {
                    self.inbox.correspondence.go(offset.saturating_sub(PAGE));
                }
                ui.small(format!(
                    "{}–{} of {total}",
                    offset + 1,
                    (offset + PAGE).min(total)
                ));
                if ui
                    .add_enabled(offset + PAGE < total, egui::Button::new("Older"))
                    .clicked()
                {
                    self.inbox.correspondence.go(offset + PAGE);
                }
            });
        }
        // A reply carries the authenticated exact revision of its root report,
        // including when an intervening requester message is its direct parent.
        let mut groups: Vec<(&str, Vec<&Value>)> = Vec::new();
        for row in page["rows"].as_array().into_iter().flatten() {
            let revision = s(row, "request");
            if let Some((_, members)) = groups.iter_mut().find(|(key, _)| *key == revision) {
                members.push(row);
            } else {
                groups.push((revision, vec![row]));
            }
        }
        for (revision, mut members) in groups {
            members.sort_by_key(|row| row["sequence"].as_i64());
            ui.add_space(7.);
            ui.push_id(("request-returns", revision), |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let has_report = members
                        .iter()
                        .any(|row| matches!(s(row, "kind"), "human_report" | "human_progress"));
                    if !has_report {
                        if let Some((_, n)) = revision.rsplit_once('@') {
                            if ui
                                .link(format!("Report revision {n}"))
                                .on_hover_text(revision)
                                .clicked()
                            {
                                self.open_resource(ui.ctx(), revision.to_owned());
                            }
                        }
                    }
                    for (i, row) in members.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(5.);
                            ui.separator();
                            ui.add_space(3.);
                        }
                        ui.push_id(s(row, "experience"), |ui| {
                            self.correspondence_row(ui, row, requester)
                        });
                    }
                });
            });
        }
    }

    fn correspondence_row(&mut self, ui: &mut Ui, row: &Value, requester: &str) {
        let p = theme::palette(ui);
        let kind = s(row, "kind");
        let human = matches!(kind, "human_report" | "human_progress");
        let (label, color) = match (kind, s(row, "action")) {
            ("human_progress" | "human_report", "take") => ("Request taken", p.muted),
            ("human_progress" | "human_report", "release") => ("Request released", p.muted),
            ("human_report", "complete") => ("Reported complete", theme::GREEN),
            ("human_report", "unable") => ("Reported unable", theme::ACCENT),
            ("human_report", _) => ("Progress report", PURPLE),
            ("requester_message", _) => ("Requester message", TEAL),
            ("requester_reply", _) => ("Requester reply", TEAL),
            _ => return,
        };
        let author = s(row, "author");
        let seq = row["sequence"].as_i64();
        let stamp = seq
            .and_then(|seq| {
                self.events
                    .binary_search_by_key(&seq, |e| e["event"].as_i64().unwrap_or(0))
                    .ok()
            })
            .and_then(|i| self.events[i]["created_utc"].as_str())
            .map(str::to_owned)
            .or_else(|| row["created"].as_i64().map(time_label));
        ui.horizontal_wrapped(|ui| {
            theme::badge(ui, label, color);
            let who = if human {
                format!("Operator {author}")
            } else {
                self.label(author).to_owned()
            };
            ui.label(RichText::new(who).strong()).on_hover_text(author);
            if let Some(time) = stamp {
                ui.small(RichText::new(time).color(p.muted));
            }
        });
        if let Some(reference) = row["field_report"]["request"].as_str() {
            let edition = reference.rsplit_once('@').map_or(reference, |v| v.1);
            ui.small(format!("Offline field report · Request edition {edition}"))
                .on_hover_text(reference);
        }
        let text = s(row, "text");
        if !text.trim().is_empty() {
            let id = ui.make_persistent_id("full-correspondence-text");
            let long = text.chars().nth(360).is_some() || text.lines().nth(4).is_some();
            let full = !long || ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
            if full && row["field_report"].is_object() {
                ui.add(egui::Label::new(text).wrap().selectable(true));
            } else if full {
                document::message_markdown(ui, text);
            } else {
                ui.add(
                    egui::Label::new(document::excerpt(text, 360))
                        .wrap()
                        .selectable(true),
                );
            }
            if long
                && ui
                    .link(if full { "Show less" } else { "Read full text" })
                    .clicked()
            {
                ui.data_mut(|d| d.insert_temp(id, !full));
            }
        }
        let source = s(row, "experience");
        self.source_file_links(ui, &row["attachments"], source);
        ui.horizontal_wrapped(|ui| {
            let title = if human {
                "Open report"
            } else {
                "Open response"
            };
            if ui.link(title).on_hover_text(source).clicked() {
                self.open_resource(ui.ctx(), source.to_owned());
            }
            if human {
                if let Some(seq) = seq {
                    if ui
                        .link("Open receipt")
                        .on_hover_text(format!(
                            "Check when {} received this return and which files were supplied",
                            self.label(requester)
                        ))
                        .clicked()
                    {
                        self.open_message_receipt(ui.ctx(), seq);
                    }
                }
            } else {
                if let Some(parent) = row["reply_to"].as_str() {
                    if ui.link("In reply to").on_hover_text(parent).clicked() {
                        self.open_resource(ui.ctx(), parent.to_owned());
                    }
                }
                if let Some(decision) = row["decision"].as_str() {
                    if ui
                        .link("Authored decision")
                        .on_hover_text(decision)
                        .clicked()
                    {
                        self.open_resource(ui.ctx(), decision.to_owned());
                    }
                }
            }
        });
    }
}
