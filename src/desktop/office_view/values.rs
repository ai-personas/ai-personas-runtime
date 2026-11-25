//! A worksheet and its formula records need not have the same extent. Keep the
//! values table bounded independently; a sparse ledger can still be inspected by address.
use calamine::{Data, DataRef, Xlsx};
use std::io::Cursor;

pub(super) type Table = (Vec<Vec<String>>, String);
pub(super) struct Budget {
    slots: u64,
    text: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            slots: 2_000_000,
            text: 32 * 1024 * 1024,
        }
    }
}

pub(super) fn caption(height: usize, width: usize, start: (u32, u32)) -> String {
    format!(
        "{height} rows × {width} columns  ·  Starts at {}",
        super::cell_name(start)
    )
}

pub(super) fn xlsx(
    book: &mut Xlsx<Cursor<&[u8]>>,
    name: &str,
    budget: &mut Budget,
) -> Result<Table, String> {
    let mut reader = book
        .worksheet_cells_reader(name)
        .map_err(|e| e.to_string())?;
    let mut lo = (u32::MAX, u32::MAX);
    let mut hi = (0, 0);
    let mut area = 0;
    let mut text = budget.text;
    let mut cells = Vec::new();
    while let Some(cell) = reader.next_cell().map_err(|e| e.to_string())? {
        let value = cell.get_value();
        if matches!(value, DataRef::Empty) {
            continue;
        }
        let pos = cell.get_position();
        if cells.last().is_some_and(|(prev, _)| pos <= *prev) {
            return Err("Worksheet cell addresses are inconsistent.".into());
        }
        lo = (lo.0.min(pos.0), lo.1.min(pos.1));
        hi = (hi.0.max(pos.0), hi.1.max(pos.1));
        // Include a slot for each row vector, not just its contents. Never
        // reserve from a declared XML dimension (which can include blank space).
        area = (u64::from(hi.0 - lo.0) + 1).saturating_mul(u64::from(hi.1 - lo.1) + 2);
        if area > budget.slots {
            return Err("The stored values span too large a table for this preview.".into());
        }
        let s = match value {
            DataRef::SharedString(s) if s.len() <= text => (*s).to_owned(),
            DataRef::String(s) if s.len() <= text => s.to_owned(),
            DataRef::SharedString(_) | DataRef::String(_) => {
                return Err("Stored text exceeds the values preview limit.".into())
            }
            other => Data::from(other.clone()).to_string(),
        };
        text = text
            .checked_sub(s.len())
            .ok_or("Stored text exceeds the values preview limit.")?;
        cells.push((pos, s));
    }
    if cells.is_empty() {
        return Ok((Vec::new(), caption(0, 0, (0, 0))));
    }
    budget.slots -= area;
    budget.text = text;
    let (height, width) = ((hi.0 - lo.0) as usize + 1, (hi.1 - lo.1) as usize + 1);
    let mut rows = vec![vec![String::new(); width]; height];
    for ((r, c), s) in cells {
        rows[(r - lo.0) as usize][(c - lo.1) as usize] = s;
    }
    Ok((rows, caption(height, width, lo)))
}
