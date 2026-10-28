//! Shared, selectable Markdown and source presentation for messages and files.
use super::theme;
use eframe::egui::{self, FontId, RichText, Ui};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// A short reading preview, with Markdown formatting removed before truncation.
/// The full source and formatted message remain available in the conversation.
pub fn excerpt(text: &str, limit: usize) -> String {
    let mut plain = String::new();
    for event in Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Text(text) | Event::Code(text) => plain.push_str(&text),
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::TableCell
                | TagEnd::CodeBlock,
            ) => plain.push(' '),
            _ => {}
        }
    }
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = plain.chars();
    let mut preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

pub fn code(ui: &mut Ui, text: &str, language: &str) {
    let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());
    let job =
        egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, text, language);
    ui.add(
        egui::Label::new(job)
            .selectable(true)
            .wrap_mode(egui::TextWrapMode::Extend),
    );
}

pub fn markdown(ui: &mut Ui, text: &str) {
    markdown_at(ui, text, None);
}

pub fn markdown_at(ui: &mut Ui, text: &str, base: Option<&std::path::Path>) {
    let p = theme::palette(ui);
    let link_color = ui.visuals().hyperlink_color;
    let mut job = egui::text::LayoutJob::default();
    let mut size = 13.0;
    let mut strong = 0;
    let mut emphasis = 0;
    let mut strike = 0;
    let mut quote = 0;
    let mut lists: Vec<Option<u64>> = vec![];
    let mut links: Vec<(String, String)> = vec![];
    let mut link: Option<(String, String)> = None;
    let mut code_block: Option<(String, String)> = None;
    let mut table: Option<Vec<Vec<String>>> = None;
    let flush =
        |ui: &mut Ui, job: &mut egui::text::LayoutJob, links: &mut Vec<(String, String)>| {
            if !job.text.is_empty() {
                ui.add(
                    egui::Label::new(std::mem::take(job))
                        .wrap()
                        .selectable(true),
                );
            }
            if !links.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (label, url) in links.drain(..) {
                        if url.starts_with("https://") || url.starts_with("http://") {
                            ui.hyperlink_to(label, url);
                        } else {
                            ui.label(RichText::new(format!("{label} · {url}")).small().weak());
                        }
                    }
                });
            }
        };
    for event in Parser::new_ext(
        text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        if let Some((language, source)) = &mut code_block {
            match event {
                Event::End(TagEnd::CodeBlock) => {
                    egui::Frame::new()
                        .fill(p.field)
                        .inner_margin(9)
                        .corner_radius(3)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.weak(if language.is_empty() {
                                    "Code"
                                } else {
                                    language.as_str()
                                });
                                if ui.small_button("Copy code").clicked() {
                                    ui.ctx().copy_text(source.clone());
                                }
                            });
                            egui::ScrollArea::horizontal()
                                .id_salt(("code", source.as_str()))
                                .show(ui, |ui| code(ui, source, language));
                        });
                    code_block = None;
                }
                Event::Text(t) => source.push_str(&t),
                _ => {}
            }
            continue;
        }
        if let Some(rows) = &mut table {
            match event {
                Event::Start(Tag::TableHead | Tag::TableRow) => rows.push(vec![]),
                Event::Start(Tag::TableCell) => {
                    if let Some(row) = rows.last_mut() {
                        row.push(String::new());
                    }
                }
                Event::Text(t) | Event::Code(t) | Event::InlineHtml(t) => {
                    if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut()) {
                        cell.push_str(&t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut()) {
                        cell.push(' ');
                    }
                }
                Event::End(TagEnd::Table) => {
                    egui::ScrollArea::horizontal()
                        .id_salt(("markdown-table", rows.clone()))
                        .show(ui, |ui| {
                            egui::Grid::new(("md-grid", rows.clone()))
                                .striped(true)
                                .spacing([16.0, 6.0])
                                .show(ui, |ui| {
                                    for (n, row) in rows.iter().enumerate() {
                                        for cell in row {
                                            ui.add(
                                                egui::Label::new(if n == 0 {
                                                    RichText::new(cell).strong()
                                                } else {
                                                    RichText::new(cell)
                                                })
                                                .wrap()
                                                .selectable(true),
                                            );
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                    table = None;
                }
                _ => {}
            }
            continue;
        }
        let format = |mono: bool| egui::TextFormat {
            font_id: if mono {
                FontId::monospace(11.5)
            } else {
                FontId::proportional(size + if strong > 0 { 0.5 } else { 0.0 })
            },
            color: if link.is_some() {
                link_color
            } else if quote > 0 {
                p.muted
            } else {
                p.text
            },
            italics: emphasis > 0,
            strikethrough: if strike > 0 {
                egui::Stroke::new(1.0_f32, p.muted)
            } else {
                egui::Stroke::NONE
            },
            background: if mono {
                p.field
            } else {
                egui::Color32::TRANSPARENT
            },
            ..Default::default()
        };
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(ui, &mut job, &mut links);
                let language = match kind {
                    CodeBlockKind::Fenced(l) => {
                        l.split_whitespace().next().unwrap_or("").to_string()
                    }
                    _ => String::new(),
                };
                code_block = Some((language, String::new()));
            }
            Event::Start(Tag::Table(_)) => {
                flush(ui, &mut job, &mut links);
                table = Some(vec![]);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(ui, &mut job, &mut links);
                size = 21.0 - level as u8 as f32;
                ui.add_space(5.0);
            }
            Event::End(TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::Item) => {
                flush(ui, &mut job, &mut links);
                size = 13.0;
                ui.add_space(3.0);
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong -= 1,
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis -= 1,
            Event::Start(Tag::Strikethrough) => strike += 1,
            Event::End(TagEnd::Strikethrough) => strike -= 1,
            Event::Start(Tag::BlockQuote(_)) => {
                flush(ui, &mut job, &mut links);
                quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(ui, &mut job, &mut links);
                quote -= 1;
            }
            Event::Start(Tag::List(start)) => {
                flush(ui, &mut job, &mut links);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush(ui, &mut job, &mut links);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(ui, &mut job, &mut links);
                let depth = lists.len().saturating_sub(1);
                let prefix = if let Some(Some(n)) = lists.last_mut() {
                    let s = format!("{n}. ");
                    *n += 1;
                    s
                } else {
                    "• ".into()
                };
                job.append(
                    &format!("{}{prefix}", "    ".repeat(depth)),
                    0.0,
                    format(false),
                );
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                link = Some((String::new(), dest_url.into_string()));
            }
            Event::End(TagEnd::Image) => {
                if let Some((label, target)) = link.take() {
                    let local = base
                        .and_then(|b| url::Url::from_directory_path(b).ok())
                        .and_then(|b| b.join(&target).ok())
                        .and_then(|u| u.to_file_path().ok());
                    if let Some(path) = local {
                        flush(ui, &mut job, &mut links);
                        ui.add(
                            egui::Image::new(format!("file://{}", path.display()))
                                .max_width(ui.available_width())
                                .alt_text(label),
                        );
                    } else {
                        links.push((label, target));
                    }
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(item) = link.take() {
                    links.push(item);
                }
            }
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                job.append(&t, 0.0, format(false));
                if let Some((label, _)) = &mut link {
                    label.push_str(&t);
                }
            }
            Event::Code(t) => {
                job.append(&t, 0.0, format(true));
            }
            Event::SoftBreak => job.append(" ", 0.0, format(false)),
            Event::HardBreak => job.append("\n", 0.0, format(false)),
            Event::TaskListMarker(done) => {
                job.append(if done { "[x] " } else { "[ ] " }, 0.0, format(true))
            }
            Event::Rule => {
                flush(ui, &mut job, &mut links);
                ui.separator();
            }
            _ => {}
        }
    }
    flush(ui, &mut job, &mut links);
}
