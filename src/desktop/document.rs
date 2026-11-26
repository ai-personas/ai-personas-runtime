//! Shared, selectable Markdown and source presentation for messages and files.
use super::theme;
use eframe::egui::{self, FontId, RichText, Ui};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
pub(crate) mod edition_links;
mod issued_text;
pub(crate) use issued_text::Files;
use issued_text::Runs;

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
    render_markdown(ui, text, Reading::File(base));
}

pub fn message_markdown(ui:&mut Ui,text:&str){
    render_markdown(ui, text, Reading::Message);
}

/// The reading view interprets Markdown. The transcript uses the submitted
/// string directly, including its line breaks and any formatting characters.
pub fn message_edition(ui: &mut Ui, id: egui::Id, text: &str, start_as_sent: bool) {
    let mut literal = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(start_as_sent);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut literal, false, "Reading view");
        ui.selectable_value(&mut literal, true, "As sent");
        if literal {
            ui.small(format!("{} UTF-8 bytes", text.len()));
            if ui.small_button("Copy exact text").clicked() {
                ui.ctx().copy_text(text.to_owned());
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, literal));
    if literal {
        egui::Frame::new().fill(theme::palette(ui).field).inner_margin(8).corner_radius(3)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal().id_salt(id.with("lines")).auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend));
                    });
            });
    } else {
        message_markdown(ui, text);
    }
}

enum Reading<'a> {
    File(Option<&'a std::path::Path>),
    Message,
    Issued(std::ops::Range<usize>, Files<'a>),
}

/// Parse the whole document (reference definitions may be in another section).
/// Issued documents have no local base, remote actions or executable content.
/// Return only an index into this edition's file list; opening requires a
/// fresh read by the delivery desk. No other destination is actionable.
pub(super) fn issued_section(
    ui: &mut Ui, text: &str, section: std::ops::Range<usize>, files: Files<'_>,
) -> Option<usize> {
    render_markdown(ui, text, Reading::Issued(section, files))
}

fn render_markdown(ui: &mut Ui, text: &str, reading: Reading<'_>) -> Option<usize> {
    let preserve_lines = matches!(reading, Reading::Message);
    let base = match &reading { Reading::File(path) => *path, _ => None };
    let p = theme::palette(ui);
    let link_color = ui.visuals().hyperlink_color;
    let issued = match &reading { Reading::Issued(_, files) => Some(files), _ => None };
    let names = issued.map_or(&[][..], |f| f.names);
    let follow = std::cell::Cell::new(None);
    let mut cursor = issued_text::Cursor::default();
    let mut job = Runs::<egui::text::LayoutJob>::default();
    let mut size = theme::BODY_TEXT;
    let mut strong = 0;
    let mut emphasis = 0;
    let mut strike = 0;
    let mut quote = 0;
    let mut lists: Vec<Option<u64>> = vec![];
    let mut links: Vec<(String, String)> = vec![];
    let mut link: Option<(String, String)> = None;
    let mut code_block: Option<(String, String)> = None;
    let mut table: Option<Vec<Vec<Runs<String>>>> = None;
    let flush =
        |ui: &mut Ui, job: &mut Runs<egui::text::LayoutJob>, links: &mut Vec<(String, String)>| {
            if let Some(i) = std::mem::take(job).show(ui, names, Into::into) {
                follow.set(Some(i));
            }
            if !links.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (label, url) in links.drain(..) {
                        if issued.is_none()
                            && (url.starts_with("https://") || url.starts_with("http://")) {
                            ui.hyperlink_to(label, url);
                        } else {
                            ui.label(RichText::new(format!("{label} · {url}")).small().weak());
                        }
                    }
                });
            }
        };
    for (event, span) in Parser::new_ext(
        text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ).into_offset_iter() {
        if matches!(&reading, Reading::Issued(section, _) if !section.contains(&span.start)) {
            continue;
        }
        let Some(mark) = cursor.mark(&event, span.start, issued) else { continue; };
        let bound = mark.file;
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
                        row.push(Runs::default());
                    }
                }
                Event::Text(t) | Event::Code(t) | Event::InlineHtml(t) => {
                    if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut()) {
                        cell.part(bound).push_str(&t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut()) {
                        cell.part(bound).push(' ');
                    }
                }
                Event::End(TagEnd::Table) => {
                    let rows = std::mem::take(rows);
                    let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                    let width = ((ui.available_width() - 16. * (columns-1) as f32) / columns as f32).clamp(82., 300.);
                    let id = ui.next_auto_id();
                    egui::ScrollArea::horizontal()
                        .id_salt(("markdown-table", id))
                        .show(ui, |ui| {
                            egui::Grid::new(("md-grid", id))
                                .min_col_width(width.min(110.))
                                .max_col_width(width)
                                .striped(true)
                                .spacing([16.0, 6.0])
                                .show(ui, |ui| {
                                    for (n, row) in rows.into_iter().enumerate() {
                                        for cell in row {
                                            if let Some(i) = cell.show_cell(ui, width, names, |s| {
                                                let text = RichText::new(s);
                                                (if n == 0 { text.strong() } else { text }).into()
                                            }) { follow.set(Some(i)); }
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
                FontId::monospace(theme::MONO_TEXT)
            } else {
                FontId::proportional(size + if strong > 0 { 0.5 } else { 0.0 })
            },
            color: if bound.is_some() || (issued.is_none() && link.is_some()) {
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
                size = theme::BODY_TEXT + 9.0 - level as u8 as f32;
                ui.add_space(5.0);
            }
            Event::End(TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::Item) => {
                flush(ui, &mut job, &mut links);
                size = theme::BODY_TEXT;
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
                job.part(None).append(
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
                    // Studio draws exact file/package actions from the full message,
                    // even when the reading preview ends before the link.
                    if !mark.closed_bound_link && (!preserve_lines || edition_links::target(&item.1).is_none()) {
                        links.push(item);
                    }
                }
            }
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                job.part(bound).append(&t, 0.0, format(false));
                if let Some((label, _)) = &mut link {
                    label.push_str(&t);
                }
            }
            Event::Code(t) => {
                job.part(bound).append(&t, 0.0, format(true));
            }
            Event::SoftBreak => job.part(bound).append(if preserve_lines {"\n"} else {" "}, 0.0, format(false)),
            Event::HardBreak => job.part(bound).append("\n", 0.0, format(false)),
            Event::TaskListMarker(done) => {
                job.part(None).append(if done { "[x] " } else { "[ ] " }, 0.0, format(true))
            }
            Event::Rule => {
                flush(ui, &mut job, &mut links);
                ui.separator();
            }
            _ => {}
        }
    }
    flush(ui, &mut job, &mut links);
    follow.get()
}
