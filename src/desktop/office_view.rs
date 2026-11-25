//! A reading view for Office containers. Spreadsheet values come from the
//! workbook's stored cells; opening one never recalculates formulas.
use super::{data_view, design, theme};
use calamine::{open_workbook_auto_from_rs, Reader, Sheets};
#[path = "office_view/formulas.rs"]
mod formulas;
#[path = "office_view/values.rs"]
mod values;
use eframe::egui::{self, RichText, Ui};
use std::io::{Cursor, Read};

pub(super) struct Book {
    pages: Vec<Page>,
    pub kind: &'static str,
}
pub(super) struct Page {
    name: String,
    body: PageBody,
}
enum PageBody {
    Sheet {
        values: Result<values::Table, String>,
        formulas: Option<Result<Vec<formulas::Cell>, String>>,
    },
    Document(Vec<(bool, String)>),
    Unavailable(String),
}

#[derive(Default)]
pub(super) struct View {
    page: usize,
    expressions: bool,
    address: Option<String>,
}

impl Book {
    pub fn workbook(bytes: &[u8]) -> Result<Self, String> {
        let mut workbook =
            open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let mut pages = Vec::new();
        if let Sheets::Xlsx(book) = &mut workbook {
            let mut source = formulas::Source::open(bytes);
            let mut budget = values::Budget::default();
            for name in book.sheet_names() {
                let body = match &mut source {
                    Ok(source) => match source.member(&name) {
                        Ok(member) => PageBody::Sheet {
                            values: values::xlsx(book, &name, &mut budget),
                            formulas: Some(source.sheet(member)),
                        },
                        Err(reason) => PageBody::Unavailable(reason),
                    },
                    Err(reason) => PageBody::Unavailable(reason.clone()),
                };
                pages.push(Page { name, body });
            }
        } else {
            for name in workbook.sheet_names() {
                let values = workbook
                    .worksheet_range(&name)
                    .map(|range| {
                        let caption = values::caption(
                            range.height(),
                            range.width(),
                            range.start().unwrap_or((0, 0)),
                        );
                        let rows = range
                            .rows()
                            .map(|row| row.iter().map(ToString::to_string).collect())
                            .collect();
                        (rows, caption)
                    })
                    .map_err(|e| e.to_string());
                pages.push(Page {
                    name,
                    body: PageBody::Sheet {
                        values,
                        formulas: None,
                    },
                });
            }
        }
        Ok(Self {
            pages,
            kind: "Spreadsheet",
        })
    }

    pub fn document(bytes: &[u8], ext: &str) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let mut pages = Vec::new();
        let files = match ext {
            "docx" => vec![("Document".to_owned(), "word/document.xml".to_owned())],
            "pptx" => {
                // Relationship order in the presentation is the visible slide order.
                let xml = xml_member(&mut zip, "ppt/presentation.xml")?;
                let rels = xml_member(&mut zip, "ppt/_rels/presentation.xml.rels")?;
                let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
                let links = roxmltree::Document::parse(&rels).map_err(|e| e.to_string())?;
                let mut slides = Vec::new();
                for node in doc.descendants().filter(|n| n.tag_name().name() == "sldId") {
                    let rid = node
                        .attributes()
                        .find(|a| a.name() == "id" && a.namespace().is_some())
                        .map(|a| a.value());
                    let target = links
                        .descendants()
                        .find(|n| n.has_tag_name("Relationship") && n.attribute("Id") == rid)
                        .and_then(|n| n.attribute("Target"))
                        .ok_or("Slide relationship is missing.")?;
                    let name = if let Some(path) = target.strip_prefix('/') {
                        path.to_owned()
                    } else {
                        format!("ppt/{target}")
                    };
                    slides.push((format!("Slide {}", slides.len() + 1), name));
                }
                slides
            }
            _ => vec![("Document".into(), "content.xml".into())],
        };
        for (name, path) in files {
            let xml = xml_member(&mut zip, &path)?;
            let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
            if ext == "odp" {
                for (i, slide) in doc
                    .descendants()
                    .filter(|n| n.tag_name().name() == "page")
                    .enumerate()
                {
                    pages.push(Page {
                        name: format!("Slide {}", i + 1),
                        body: PageBody::Document(paragraphs(slide, true)),
                    });
                }
            } else {
                pages.push(Page {
                    name,
                    body: PageBody::Document(paragraphs(doc.root(), ext == "odt")),
                });
            }
        }
        Ok(Self {
            pages,
            kind: if matches!(ext, "pptx" | "odp") {
                "Slide deck"
            } else {
                "Document"
            },
        })
    }

    pub fn ui(&self, ui: &mut Ui, view: &mut View, table: &mut data_view::TableView) {
        let selected = &mut view.page;
        if self.pages.is_empty() {
            ui.weak("This file contains no visible pages or sheets.");
            return;
        }
        *selected = (*selected).min(self.pages.len() - 1);
        let before = *selected;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("office-page")
                .selected_text(&self.pages[*selected].name)
                .show_ui(ui, |ui| {
                    for (index, page) in self.pages.iter().enumerate() {
                        ui.selectable_value(selected, index, &page.name);
                    }
                });
            ui.weak(format!("{} of {}", *selected + 1, self.pages.len()));
            if matches!(
                &self.pages[*selected].body,
                PageBody::Sheet {
                    formulas: Some(_),
                    ..
                }
            ) {
                ui.separator();
                ui.selectable_value(&mut view.expressions, false, "Values");
                ui.selectable_value(&mut view.expressions, true, "Formulas");
            }
        });
        if *selected != before {
            *table = data_view::TableView::default();
            view.address = None;
        }
        ui.add_space(5.);
        match &self.pages[*selected].body {
            PageBody::Sheet { values, formulas } => {
                if let Some(formulas) = formulas.as_ref().filter(|_| view.expressions) {
                    match formulas {
                        Ok(cells) => {
                            formula_view(ui, &self.pages[*selected].name, cells, &mut view.address)
                        }
                        Err(error) => {
                            ui.label(format!("Formula inspection unavailable: {error}"));
                        }
                    }
                } else {
                    ui.small("Stored values · no display formatting or recalculation");
                    match values {
                        Ok((rows, caption)) => {
                            ui.weak(caption);
                            table.ui(ui, rows);
                        }
                        Err(error) => {
                            ui.label(format!("Values unavailable: {error}"));
                        }
                    }
                }
            }
            PageBody::Unavailable(reason) => {
                ui.label(format!("Worksheet unavailable: {reason}"));
            }
            PageBody::Document(lines) => {
                let p = theme::palette(ui);
                egui::ScrollArea::vertical()
                    .id_salt(("office-paper", *selected))
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .fill(p.panel)
                            .stroke(egui::Stroke::new(1., p.border))
                            .inner_margin(24)
                            .corner_radius(8)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                if lines.is_empty() {
                                    ui.weak("No readable text on this page.");
                                }
                                for (heading, text) in lines {
                                    let style = if *heading {
                                        design::display(text, 30.)
                                    } else {
                                        RichText::new(text).size(16.)
                                    };
                                    ui.add(egui::Label::new(style).wrap().selectable(true));
                                    ui.add_space(4.);
                                }
                            });
                    });
            }
        }
    }
}

fn formula_view(ui: &mut Ui, sheet: &str, cells: &[formulas::Cell], entry: &mut Option<String>) {
    let Some(first) = cells.first() else {
        ui.weak("No worksheet formula cells on this sheet.");
        return;
    };
    let address = entry.get_or_insert_with(|| cell_name(first.pos));
    let find = |s: &str| {
        formulas::position(s.trim()).map(|pos| cells.binary_search_by_key(&pos, |c| c.pos))
    };
    ui.horizontal_wrapped(|ui| {
        ui.label("Cell");
        ui.add(
            egui::TextEdit::singleline(address)
                .desired_width(74.)
                .hint_text("A1"),
        );
        let at = find(address);
        let (prev, next) = match at {
            Some(Ok(i)) => (i.checked_sub(1), i + 1),
            Some(Err(i)) => (i.checked_sub(1), i),
            None => (None, 0),
        };
        if ui
            .add_enabled(prev.is_some(), egui::Button::new("Previous"))
            .clicked()
        {
            if let Some(i) = prev {
                *address = cell_name(cells[i].pos);
                ui.ctx().request_repaint();
            }
        }
        if ui
            .add_enabled(next < cells.len(), egui::Button::new("Next"))
            .clicked()
        {
            *address = cell_name(cells[next].pos);
            ui.ctx().request_repaint();
        }
        if let Some(Ok(i)) = find(address) {
            ui.weak(format!("Formula {} of {}", i + 1, cells.len()));
        }
    });
    let Some(Ok(i)) = find(address) else {
        if find(address).is_some() {
            ui.weak("No stored formula at this address on this sheet.");
        } else {
            ui.weak("Enter a worksheet cell address, such as A1, or choose Next.");
        }
        return;
    };
    let c = &cells[i];
    let p = theme::palette(ui);
    egui::ScrollArea::vertical()
        .id_salt(("formula-source", sheet, i))
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(p.panel)
                .stroke(egui::Stroke::new(1., p.border))
                .inner_margin(10)
                .corner_radius(6)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!("{sheet}  ·  {}", cell_name(c.pos)))
                            .strong()
                            .size(18.),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Stored expression");
                        if !c.expression.is_empty() && ui.small_button("Copy expression").clicked()
                        {
                            ui.ctx().copy_text(c.expression.clone());
                        }
                    });
                    if !c.metadata.is_empty() {
                        ui.small(&c.metadata);
                    }
                    if c.expression.is_empty() {
                        ui.weak("No expression text stored at this cell.");
                    } else {
                        ui.add(
                            egui::Label::new(RichText::new(&c.expression).monospace())
                                .wrap()
                                .selectable(true),
                        );
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.strong("Saved result");
                        ui.weak("Not recalculated");
                    });
                    match c.cached.as_deref() {
                        None => {
                            ui.weak("No cached value element in this cell.");
                        }
                        Some("") => {
                            ui.weak(format!("{} · empty value element", c.value_kind));
                        }
                        Some(value) => {
                            ui.small(c.value_kind);
                            ui.add(
                                egui::Label::new(RichText::new(value).monospace())
                                    .wrap()
                                    .selectable(true),
                            );
                        }
                    }
                });
        });
}

fn xml_member(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, String> {
    let mut entry = zip.by_name(name).map_err(|e| e.to_string())?;
    if entry.size() > 32 * 1024 * 1024 {
        return Err("This XML part is too large for the reading view.".into());
    }
    let mut xml = String::new();
    entry.read_to_string(&mut xml).map_err(|e| e.to_string())?;
    Ok(xml)
}

fn paragraphs(root: roxmltree::Node<'_, '_>, odf: bool) -> Vec<(bool, String)> {
    let mut paragraphs = Vec::new();
    for p in root
        .descendants()
        .filter(|n| matches!(n.tag_name().name(), "p" | "h"))
    {
        let mut text = String::new();
        for node in p.descendants() {
            if node.is_text() {
                let parent = node.parent_element().map(|n| n.tag_name().name());
                if odf || parent == Some("t") {
                    text.push_str(node.text().unwrap_or_default());
                }
            } else {
                match node.tag_name().name() {
                    "tab" => text.push('\t'),
                    "br" | "line-break" => text.push('\n'),
                    "s" if odf => text.push(' '),
                    _ => {}
                }
            }
        }
        let heading = p.tag_name().name() == "h"
            || p.descendants().any(|n| {
                n.tag_name().name() == "pStyle"
                    && n.attributes().any(|a| {
                        a.name() == "val"
                            && (a.value().starts_with("Heading") || a.value() == "Title")
                    })
            });
        if !text.trim().is_empty() {
            paragraphs.push((heading, text));
        }
    }
    paragraphs
}

fn cell_name((row, column): (u32, u32)) -> String {
    let mut n = column + 1;
    let mut chars = Vec::new();
    while n > 0 {
        n -= 1;
        chars.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    format!("{}{}", chars.into_iter().rev().collect::<String>(), row + 1)
}
