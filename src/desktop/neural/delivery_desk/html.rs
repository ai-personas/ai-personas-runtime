//! Passive text from an HTML document. The desktop never resolves URLs or instantiates controls.
use super::*;
use scraper::{node::Element, Html, Node};

const PARSE_BYTES: usize = 256 * 1024;
const EXCERPT_BYTES: usize = 8 * 1024;
const EXCERPT_BLOCKS: usize = 80;

pub(super) struct Preview {
    document: Option<Reader>,
    source: String,
    more_source: bool,
}

impl Preview {
    pub(super) fn read(bytes: &[u8]) -> ai_personas::neural::Result<Self> {
        let source = std::str::from_utf8(bytes)
            .ok()
            .filter(|s| !s.contains('\0'))
            .ok_or_else(|| {
                ai_personas::neural::failure(
                    "The HTML preview requires UTF-8 text. The exact file is available to save.",
                )
            })?;
        let prefix = prefix(source, EXCERPT_BYTES);
        Ok(Self {
            document: (bytes.len() <= PARSE_BYTES).then(|| Reader::parse(source)),
            source: prefix.to_owned(),
            more_source: prefix.len() < source.len(),
        })
    }

    pub(super) fn show(&self, ui: &mut Ui, reference: &str, source: &mut bool) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(source, false, "Document text");
            ui.selectable_value(source, true, "HTML source");
        });
        ui.add_space(3.);
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.canvas)
            .stroke(Stroke::new(1., p.border))
            .corner_radius(5)
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let height = (ui.ctx().content_rect().height() * 0.34).clamp(150., 300.);
                egui::ScrollArea::vertical()
                    .id_salt(("issued-html-text", reference, *source))
                    .auto_shrink([false, true])
                    .min_scrolled_height(height)
                    .max_height(height)
                    .show(ui, |ui| {
                        if *source {
                            if self.source.is_empty() {
                                ui.weak("Empty file");
                            } else {
                                ui.add(egui::Label::new(RichText::new(&self.source).monospace().size(12.)).selectable(true).wrap());
                            }
                        } else if let Some(doc) = &self.document {
                            if doc.blocks.is_empty() {
                                ui.weak("No readable document text was found.");
                            }
                            for (i, (role, text)) in doc.blocks.iter().enumerate() {
                                if i > 0 && matches!(role, Role::Title | Role::Heading) {
                                    ui.add_space(5.);
                                }
                                let text = RichText::new(text);
                                let text = match role {
                                    Role::Title => text.size(18.).strong(),
                                    Role::Heading => text.size(15.).strong(),
                                    Role::TableHeading => text.strong(),
                                    Role::Body => text,
                                };
                                ui.add(egui::Label::new(text).selectable(true).wrap());
                            }
                        } else {
                            ui.weak("This document exceeds the 256 KiB reading preview. Its source excerpt and exact file are available.");
                        }
                    });
            });
        if (*source && self.more_source)
            || (!*source && self.document.as_ref().is_some_and(|d| d.truncated))
        {
            ui.small("Showing an excerpt. Open the full file for the complete source.");
        }
    }
}

fn prefix(s: &str, limit: usize) -> &str {
    let mut end = s.len().min(limit);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Role {
    Title,
    Heading,
    TableHeading,
    #[default]
    Body,
}

#[derive(Default)]
struct Reader {
    blocks: Vec<(Role, String)>,
    role: Role,
    line: String,
    space: bool,
    cell_gap: bool,
    cell_depth: usize,
    bytes: usize,
    truncated: bool,
}

impl Reader {
    fn parse(source: &str) -> Self {
        let dom = Html::parse_document(source.strip_prefix('\u{feff}').unwrap_or(source));
        let root = dom.root_element();
        let mut out = Self::default();
        if let Some(title) = root
            .child_elements()
            .find(|e| e.value().name() == "head")
            .and_then(|head| head.child_elements().find(|e| e.value().name() == "title"))
        {
            out.role = Role::Title;
            for text in title.text() {
                out.text(text);
            }
            out.flush();
        }
        let Some(body) = root.child_elements().find(|e| e.value().name() == "body") else {
            return out;
        };
        // Walk the parsed tree iteratively: deeply nested markup never becomes a Rust call stack.
        let mut pending = vec![(*body, false)];
        while let Some((node, closing)) = pending.pop() {
            if out.truncated {
                break;
            }
            match node.value() {
                Node::Text(text) => out.text(text),
                Node::Element(tag) if closing => out.end(tag.name()),
                Node::Element(tag) if !omitted(tag) => {
                    out.start(tag);
                    pending.push((node, true));
                    pending.extend(node.children().rev().map(|child| (child, false)));
                }
                _ => {}
            }
        }
        out.flush();
        out
    }

    fn text(&mut self, s: &str) {
        for ch in s.chars() {
            if ch.is_whitespace() {
                self.space = true;
            } else {
                if !self.line.is_empty() {
                    if self.cell_gap {
                        self.line.push_str("  ·  ");
                    } else if self.space {
                        self.line.push(' ');
                    }
                }
                self.line.push(ch);
                self.cell_gap = false;
                self.space = false;
            }
        }
    }

    fn flush(&mut self) {
        let line = std::mem::take(&mut self.line);
        let role = std::mem::take(&mut self.role);
        self.space = false;
        self.cell_gap = false;
        if line.is_empty() || self.truncated {
            return;
        }
        if role == Role::Title
            && self.blocks.len() == 1
            && self.blocks[0].0 == Role::Title
            && self.blocks[0].1 == line
        {
            return;
        }
        let part = prefix(&line, EXCERPT_BYTES - self.bytes);
        self.truncated = part.len() < line.len() || self.blocks.len() >= EXCERPT_BLOCKS;
        if !part.is_empty() && self.blocks.len() < EXCERPT_BLOCKS {
            self.bytes += part.len();
            self.blocks.push((
                role,
                if part.len() == line.len() {
                    line
                } else {
                    part.to_owned()
                },
            ));
        }
    }

    fn start(&mut self, tag: &Element) {
        let name = tag.name();
        if matches!(name, "td" | "th") {
            self.cell_depth += 1;
            self.cell_gap = true;
            if name == "th" {
                self.role = Role::TableHeading;
            }
        } else if let Some(role) = boundary(name) {
            if self.cell_depth == 0 {
                self.flush();
                self.role = role;
            } else {
                self.space = true;
            }
        }
        let label = match name {
            "img" => tag.attr("alt"),
            "input"
                if tag.attr("type").is_some_and(|t| {
                    ["button", "submit", "reset"]
                        .iter()
                        .any(|v| t.eq_ignore_ascii_case(v))
                }) =>
            {
                tag.attr("value").or_else(|| tag.attr("aria-label"))
            }
            "input" | "select" | "textarea" => {
                tag.attr("aria-label").or_else(|| tag.attr("placeholder"))
            }
            _ => None,
        };
        if let Some(label) = label {
            self.space = true;
            self.text(label);
            self.space = true;
        }
    }

    fn end(&mut self, name: &str) {
        if matches!(name, "td" | "th") {
            self.cell_depth = self.cell_depth.saturating_sub(1);
        } else if boundary(name).is_some() {
            if self.cell_depth == 0 {
                self.flush();
            } else {
                self.space = true;
            }
        }
    }
}

fn boundary(tag: &str) -> Option<Role> {
    Some(match tag {
        "h1" => Role::Title,
        "h2" | "h3" | "h4" | "h5" | "h6" | "legend" | "caption" => Role::Heading,
        "address" | "article" | "aside" | "blockquote" | "br" | "dd" | "div" | "dl" | "dt"
        | "fieldset" | "figcaption" | "figure" | "footer" | "form" | "header" | "hr" | "label"
        | "li" | "main" | "nav" | "ol" | "option" | "p" | "pre" | "section" | "summary"
        | "table" | "tr" | "ul" => Role::Body,
        _ => return None,
    })
}

fn omitted(tag: &Element) -> bool {
    matches!(
        tag.name(),
        "head"
            | "style"
            | "script"
            | "template"
            | "noscript"
            | "iframe"
            | "object"
            | "embed"
            | "svg"
            | "math"
            | "canvas"
            | "audio"
            | "video"
            | "datalist"
    ) || tag.attr("hidden").is_some()
        || tag
            .attr("aria-hidden")
            .is_some_and(|s| s.eq_ignore_ascii_case("true"))
        || (tag.name() == "input"
            && tag
                .attr("type")
                .is_some_and(|s| s.eq_ignore_ascii_case("hidden")))
}
