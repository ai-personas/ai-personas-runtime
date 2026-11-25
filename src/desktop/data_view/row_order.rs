//! An index into the loaded rows, never a rewrite of the file. Keep equal
//! keys in source order and unranked cells last in either direction.
use eframe::egui::{ComboBox, Ui};
use std::cmp::Reverse;

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    Largest,
    Smallest,
    TextAscending,
    TextDescending,
}
impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Largest => "Largest first",
            Self::Smallest => "Smallest first",
            Self::TextAscending => "Text · A to Z",
            Self::TextDescending => "Text · Z to A",
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct RowOrder {
    pub(super) column: Option<usize>,
    mode: Mode,
}
impl RowOrder {
    pub(super) fn controls(
        &mut self,
        ui: &mut Ui,
        columns: usize,
        name: &impl Fn(usize) -> String,
    ) -> bool {
        let before = *self;
        ui.horizontal_wrapped(|ui| {
            ui.label("Sort rows");
            ComboBox::from_id_salt("order-column").width(155.).truncate()
                .selected_text(self.column.map(name).unwrap_or_else(|| "File order".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.column, None, "File order");
                    for c in 0..columns {
                        let full = name(c);
                        let mut short: String = full.chars().take(34).collect();
                        if short.len() < full.len() { short.push('…'); }
                        ui.selectable_value(&mut self.column, Some(c), format!("{} · {short}", c + 1)).on_hover_text(full);
                    }
                });
            if self.column.is_some() {
                ComboBox::from_id_salt("order-mode").width(105.).truncate().selected_text(self.mode.label()).show_ui(ui, |ui| {
                    for m in [Mode::Largest, Mode::Smallest, Mode::TextAscending, Mode::TextDescending] {
                        ui.selectable_value(&mut self.mode, m, m.label());
                    }
                }).response.on_hover_text("Largest and smallest compare decimal numbers. Text order compares the original cell strings.");
            }
        }).response.on_hover_text("Order the loaded table or matching rows. The original rows, plot and saved file do not change.");
        before != *self
    }

    pub(super) fn apply(&self, indices: &mut [usize], rows: &[Vec<String>]) -> usize {
        let Some(c) = self.column else { return 0 };
        let cell = |i: usize| {
            rows[i]
                .get(c)
                .map(String::as_str)
                .filter(|s| !s.trim().is_empty())
        };
        match self.mode {
            Mode::Largest | Mode::Smallest => {
                order(indices, self.mode == Mode::Largest, |i| decimal(cell(i)?))
            }
            Mode::TextAscending | Mode::TextDescending => {
                order(indices, self.mode == Mode::TextDescending, cell)
            }
        }
    }
}

fn order<T: Ord>(
    indices: &mut [usize],
    descending: bool,
    key: impl Fn(usize) -> Option<T>,
) -> usize {
    // Parse once per row, not once per comparison or repaint.
    let mut ranked: Vec<_> = indices.iter().map(|&i| (key(i), i)).collect();
    ranked.sort_unstable_by(|(a, i), (b, j)| {
        match (a, b) {
            (Some(a), Some(b)) if descending => b.cmp(a),
            (Some(a), Some(b)) => a.cmp(b),
            _ => a.is_none().cmp(&b.is_none()),
        }
        .then(i.cmp(j))
    });
    let missing = ranked.iter().rev().take_while(|(k, _)| k.is_none()).count();
    for (index, (_, i)) in indices.iter_mut().zip(ranked) {
        *index = i;
    }
    missing
}

// Decimal place of the first significant digit, then its significant digits
// (no trailing zeroes). String and integer fields avoid f64 rounding a pair
// of long IDs, near-equal measurements or very small readings to the same key.
type Magnitude = (i128, String);
#[derive(Eq, PartialEq, Ord, PartialOrd)]
enum Decimal {
    Negative(Reverse<Magnitude>),
    Zero,
    Positive(Magnitude),
}
fn decimal(cell: &str) -> Option<Decimal> {
    let s = cell.trim();
    let negative = s.starts_with('-');
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((base, exp)) => (base, exp.parse::<i64>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = [whole, fraction].concat();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Some(Decimal::Zero);
    }
    let place =
        i128::from(exponent) + whole.len() as i128 - (digits.len() - significant.len()) as i128;
    let magnitude = (place, significant.trim_end_matches('0').to_owned());
    Some(if negative {
        Decimal::Negative(Reverse(magnitude))
    } else {
        Decimal::Positive(magnitude)
    })
}
