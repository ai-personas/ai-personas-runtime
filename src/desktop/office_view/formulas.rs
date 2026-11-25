//! Read literal worksheet formula records. Do not evaluate expressions, expand shared
//! formulas, apply number formats or treat an absent cached result as zero.
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};
use zip::ZipArchive;

type Zip<'a> = ZipArchive<Cursor<&'a [u8]>>;
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const STRICT: &str = "http://purl.oclc.org/ooxml/spreadsheetml/main";
const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const ROWS: u32 = 1_048_576;
const COLS: u32 = 16_384;

pub(super) struct Source<'a> {
    zip: Zip<'a>,
    paths: HashMap<String, Option<usize>>,
    namespace: &'static str,
    remaining: u64,
}

// An inspection budget for extra XML across this workbook. Do not build a dense
// bounding rectangle between two widely separated formula cells.
fn part(zip: &mut Zip<'_>, index: usize, remaining: &mut u64) -> Result<String, String> {
    let entry = zip.by_index(index).map_err(|e| e.to_string())?;
    let size = entry.size();
    if size > (*remaining).min(16 * 1024 * 1024) {
        return Err("Worksheet XML exceeds the formula inspection limit.".into());
    }
    *remaining -= size;
    let mut text = String::new();
    entry
        .take(size + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 != size {
        return Err("Worksheet XML size differs from the package directory.".into());
    }
    Ok(text)
}

fn xml(text: &str) -> Result<Document<'_>, String> {
    Document::parse_with_options(
        text,
        ParsingOptions {
            nodes_limit: 500_000,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())
}

fn one<'a, 'b>(node: Node<'a, 'b>, name: &str) -> Result<Option<Node<'a, 'b>>, String> {
    let ns = node.tag_name().namespace();
    let mut found = node.children().filter(|n| {
        n.is_element() && n.tag_name().namespace() == ns && n.tag_name().name() == name
    });
    let first = found.next();
    if found.next().is_some() {
        return Err(format!("Ambiguous worksheet element: {name}."));
    }
    Ok(first)
}

impl<'a> Source<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, String> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let mut remaining = 32 * 1024 * 1024;
        // Calamine looks up parts ignoring ASCII case and normalizing slashes.
        // Do not pair its values with formula records from a competing name.
        let mut members = HashMap::new();
        for path in zip.file_names() {
            let key = path.replace('\\', "/").to_ascii_lowercase();
            if key.starts_with("xl/") {
                members
                    .entry(key)
                    .and_modify(|v| *v = None)
                    .or_insert_with(|| zip.index_for_name(path));
            }
        }
        let locate = |path: &str| members.get(&path.to_ascii_lowercase()).copied().flatten();
        let names = part(
            &mut zip,
            locate("xl/workbook.xml").ok_or("Missing or ambiguous workbook part.")?,
            &mut remaining,
        )?;
        let rels = part(
            &mut zip,
            locate("xl/_rels/workbook.xml.rels")
                .ok_or("Missing or ambiguous workbook relationships.")?,
            &mut remaining,
        )?;
        let book = xml(&names)?;
        let root = book.root_element();
        let (namespace, office) = match (root.tag_name().namespace(), root.tag_name().name()) {
            (Some(MAIN), "workbook") => (MAIN, OFFICE),
            (Some(STRICT), "workbook") => (
                STRICT,
                "http://purl.oclc.org/ooxml/officeDocument/relationships",
            ),
            _ => return Err("Unrecognized workbook XML.".into()),
        };
        let doc = xml(&rels)?;
        let relationships = doc.root_element();
        if !relationships.has_tag_name((PACKAGE, "Relationships")) {
            return Err("Unrecognized workbook relationships.".into());
        }
        let worksheet = format!("{office}/worksheet");
        let mut links = HashMap::new();
        for link in relationships
            .children()
            .filter(|n| n.has_tag_name((PACKAGE, "Relationship")))
        {
            if let Some(id) = link.attribute("Id") {
                let target = link.attribute("Target").filter(|_| {
                    link.attribute("Type") == Some(&worksheet)
                        && matches!(link.attribute("TargetMode"), None | Some("Internal"))
                });
                // Only local ZIP members reached from this workbook. Never open a URI
                // or disk file, or guess sheet1.xml from a sheetId or tab position.
                let member = target.and_then(|t| {
                    let path = t
                        .strip_prefix('/')
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("xl/{t}"));
                    (path.starts_with("xl/worksheets/")
                        && !path.contains(['\\', ':', '%', '?', '#'])
                        && path.split('/').all(|s| !matches!(s, "" | "." | "..")))
                    .then(|| locate(&path))
                    .flatten()
                });
                links.entry(id).and_modify(|v| *v = None).or_insert(member);
            }
        }
        let mut paths = HashMap::new();
        let sheets = one(root, "sheets")?.ok_or("No worksheet directory in this file.")?;
        for sheet in sheets
            .children()
            .filter(|n| n.has_tag_name((namespace, "sheet")))
        {
            let Some(name) = sheet.attribute("name") else {
                continue;
            };
            let target = sheet
                .attribute((office, "id"))
                .and_then(|id| links.get(id))
                .copied()
                .flatten();
            paths
                .entry(name.to_owned())
                .and_modify(|p| *p = None)
                .or_insert(target);
        }
        Ok(Self {
            zip,
            paths,
            namespace,
            remaining,
        })
    }

    pub fn member(&self, name: &str) -> Result<usize, String> {
        self.paths
            .get(name)
            .copied()
            .flatten()
            .ok_or_else(|| "No unambiguous local worksheet relationship for this sheet.".into())
    }

    pub fn sheet(&mut self, member: usize) -> Result<Vec<Cell>, String> {
        let text = part(&mut self.zip, member, &mut self.remaining)?;
        let doc = xml(&text)?;
        let root = doc.root_element();
        if !root.has_tag_name((self.namespace, "worksheet")) {
            return Err("The linked part is not a worksheet.".into());
        }
        let data = one(root, "sheetData")?.ok_or("Missing worksheet cells.")?;
        let mut cells = Vec::new();
        let mut next_row = 0;
        for row in data
            .children()
            .filter(|n| n.has_tag_name((self.namespace, "row")))
        {
            let r = match row.attribute("r") {
                Some(number) => index(number, ROWS).ok_or("Invalid worksheet row.")?,
                None => next_row,
            };
            if r < next_row || r >= ROWS {
                return Err("Worksheet row addresses are inconsistent.".into());
            }
            next_row = r + 1;
            let mut next_col = 0;
            for cell in row
                .children()
                .filter(|n| n.has_tag_name((self.namespace, "c")))
            {
                let pos = match cell.attribute("r") {
                    Some(s) => position(s).ok_or("Invalid worksheet cell address.")?,
                    None => (r, next_col),
                };
                if pos.0 != r || pos.1 < next_col || pos.1 >= COLS {
                    return Err("Worksheet cell addresses are inconsistent.".into());
                }
                next_col = pos.1 + 1;
                let Some(f) = one(cell, "f")? else { continue };
                let metadata = [
                    f.attribute("t")
                        .filter(|&t| t != "normal")
                        .map(str::to_owned),
                    f.attribute("si").map(|s| format!("group {s}")),
                    f.attribute("ref").map(str::to_owned),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                cells.push(Cell {
                    pos,
                    metadata,
                    expression: content(f)?,
                    cached: one(cell, "v")?.map(content).transpose()?,
                    value_kind: match cell.attribute("t") {
                        None | Some("n") => "Number",
                        Some("str") => "Text",
                        Some("e") => "Error",
                        Some("b") => "Boolean code",
                        Some("s") => "Shared-string index",
                        Some("d") => "Date",
                        _ => "XML value",
                    },
                });
            }
        }
        Ok(cells)
    }
}

fn content(node: Node<'_, '_>) -> Result<String, String> {
    if node.children().any(|n| n.is_element()) {
        return Err("Unsupported nested formula or value content.".into());
    }
    Ok(node
        .children()
        .filter(Node::is_text)
        .filter_map(|n| n.text())
        .collect())
}
fn index(s: &str, limit: u32) -> Option<u32> {
    if s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u32>()
        .ok()
        .filter(|n| (1..=limit).contains(n))
        .map(|n| n - 1)
}
/// Zero-based row/column, as in the values reader. Accept lowercase for desk entry.
pub(super) fn position(address: &str) -> Option<(u32, u32)> {
    let (letters, digits) =
        address.split_at(address.bytes().take_while(u8::is_ascii_alphabetic).count());
    let mut col = 0;
    for b in letters.bytes() {
        col = col * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1);
        if col > COLS {
            return None;
        }
    }
    Some((index(digits, ROWS)?, col.checked_sub(1)?))
}

pub(super) struct Cell {
    pub pos: (u32, u32),
    pub metadata: String,
    pub expression: String,
    pub cached: Option<String>,
    pub value_kind: &'static str,
}
