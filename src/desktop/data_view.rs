//! Structure and numeric plots without assumptions about a task's domain or units.
use super::theme;
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeSet;

pub struct Dataset {
    /// Exact JSON Pointer; an empty pointer denotes the document root.
    pub pointer: String,
    pub rows: Vec<Vec<String>>,
    pub header: Option<bool>,
}

/// Discover tabular representations by structure, without guessing which data
/// constitute simulation results or assigning meaning or units to columns.
pub fn json_datasets(value: &serde_json::Value) -> Vec<Dataset> {
    let mut datasets = Vec::new();
    let mut pending = vec![(String::new(), value)];
    while let Some((pointer, value)) = pending.pop() {
        if let Some(rows) = json_rows(value) {
            let header = if value.as_array().is_some_and(|items| {
                !items.is_empty() && items.iter().all(serde_json::Value::is_array)
            }) {
                None
            } else {
                Some(true)
            };
            datasets.push(Dataset { pointer: pointer.clone(), rows, header });
            // A column-array object already represents one table. Its scalar
            // columns are not additional datasets. Likewise, scalar matrix rows
            // are rows of the selected table, not hundreds of new choices.
            if value.is_object() || value.as_array().is_some_and(|items| {
                items.iter().all(|row| row.as_array().is_some_and(|cells| {
                    cells.iter().all(|cell| !cell.is_array() && !cell.is_object())
                }))
            }) {
                continue;
            }
        }
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map.iter().rev() {
                    let key = key.replace('~', "~0").replace('/', "~1");
                    pending.push((format!("{pointer}/{key}"), child));
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate().rev() {
                    if child.is_object() || child.is_array() {
                        pending.push((format!("{pointer}/{index}"), child));
                    }
                }
            }
            _ => {}
        }
    }
    datasets
}

/// Project JSON records or column arrays without guessing a domain schema.
pub fn json_rows(value: &serde_json::Value) -> Option<Vec<Vec<String>>> {
    let cell = |v: &serde_json::Value| {
        v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string())
    };
    if let Some(items) = value.as_array() {
        if items.is_empty() {
            return None;
        }
        if items.iter().all(serde_json::Value::is_object) {
            let keys: BTreeSet<_> = items
                .iter()
                .flat_map(|v| v.as_object().unwrap().keys().cloned())
                .collect();
            let keys: Vec<_> = keys.into_iter().collect();
            let mut rows = vec![keys.clone()];
            rows.extend(items.iter().map(|v| {
                keys.iter()
                    .map(|k| v.get(k).map(cell).unwrap_or_default())
                    .collect()
            }));
            return Some(rows);
        }
        if items.iter().all(serde_json::Value::is_array) {
            return Some(
                items
                    .iter()
                    .map(|v| v.as_array().unwrap().iter().map(cell).collect())
                    .collect(),
            );
        }
        if items.iter().all(|v| !v.is_object() && !v.is_array()) {
            let mut rows = vec![vec!["Value".to_owned()]];
            rows.extend(items.iter().map(|v| vec![cell(v)]));
            return Some(rows);
        }
    }
    if let Some(map) = value.as_object() {
        if map.is_empty() || !map.values().all(|column| {
            column.as_array().is_some_and(|items| {
                items.iter().all(|v| !v.is_object() && !v.is_array())
            })
        }) {
            return None;
        }
        let mut rows = vec![map.keys().cloned().collect()];
        let len = map
            .values()
            .map(|v| v.as_array().unwrap().len())
            .max()
            .unwrap_or(0);
        for i in 0..len {
            rows.push(
                map.values()
                    .map(|v| v.get(i).map(cell).unwrap_or_default())
                    .collect(),
            );
        }
        return Some(rows);
    }
    None
}

pub fn parameters(ui: &mut Ui, value: &serde_json::Value) {
    fn entry(ui: &mut Ui, key: &str, value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                egui::CollapsingHeader::new(format!("{key}  ·  {} fields", map.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for (k, v) in map {
                            ui.push_id(k, |ui| entry(ui, k, v));
                        }
                    });
            }
            serde_json::Value::Array(items) => {
                egui::CollapsingHeader::new(format!("{key}  ·  {} items", items.len())).show(
                    ui,
                    |ui| {
                        for (i, v) in items.iter().enumerate() {
                            ui.push_id(i, |ui| entry(ui, &format!("[{i}]"), v));
                        }
                    },
                );
            }
            _ => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(key).color(theme::palette(ui).muted));
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                value
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| value.to_string()),
                            )
                            .monospace(),
                        )
                        .selectable(true),
                    );
                });
            }
        }
    }
    entry(ui, "Parameters", value);
}

pub struct TableView {
    plot: bool,
    header: bool,
    page: usize,
    x: Option<usize>,
    y: BTreeSet<usize>,
    initialized: bool,
    header_hint: Option<bool>,
}
impl Default for TableView {
    fn default() -> Self {
        Self {
            plot: false,
            header: true,
            page: 0,
            x: None,
            y: BTreeSet::new(),
            initialized: false,
            header_hint: None,
        }
    }
}
fn number(cell: &str) -> Option<f64> {
    cell.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}
impl TableView {
    pub fn with_header(header: Option<bool>) -> Self {
        Self { header_hint: header, ..Self::default() }
    }

    pub fn ui(&mut self, ui: &mut Ui, rows: &[Vec<String>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if !self.initialized {
            self.header = self.header_hint.unwrap_or_else(|| rows
                .first()
                .is_some_and(|r| r.iter().any(|v| number(v).is_none())));
            let numeric: Vec<_> = (0..columns)
                .filter(|&c| {
                    rows.iter()
                        .skip(usize::from(self.header))
                        .any(|r| r.get(c).and_then(|v| number(v)).is_some())
                })
                .collect();
            self.x = numeric.first().copied();
            self.y.extend(numeric.iter().skip(1).copied());
            if self.y.is_empty() {
                self.y.extend(numeric);
                self.x = None;
            }
            self.initialized = true;
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.plot, false, "Table");
            ui.selectable_value(&mut self.plot, true, "Plot");
            ui.checkbox(&mut self.header, "First row is column names");
            ui.weak(format!(
                "{} rows · {columns} columns",
                rows.len().saturating_sub(usize::from(self.header))
            ));
        });
        let label = |c: usize| {
            if self.header {
                rows.first()
                    .and_then(|r| r.get(c))
                    .cloned()
                    .unwrap_or_else(|| format!("Column {}", c + 1))
            } else {
                format!("Column {}", c + 1)
            }
        };
        let data = &rows[usize::from(self.header).min(rows.len())..];
        if self.plot {
            ui.horizontal_wrapped(|ui| {
                ui.label("Horizontal axis");
                egui::ComboBox::from_id_salt("plot-x")
                    .selected_text(self.x.map(label).unwrap_or_else(|| "Row number".into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.x, None, "Row number");
                        for c in 0..columns {
                            ui.selectable_value(&mut self.x, Some(c), label(c));
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Series");
                for c in 0..columns {
                    let mut enabled = self.y.contains(&c);
                    if ui.checkbox(&mut enabled, label(c)).changed() {
                        if enabled {
                            self.y.insert(c);
                        } else {
                            self.y.remove(&c);
                        }
                    }
                }
            });
            let series: Vec<_> = self
                .y
                .iter()
                .map(|&c| {
                    let points: Vec<_> = data
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            let x = match self.x {
                                Some(x) => r.get(x).and_then(|v| number(v)),
                                None => Some((i + 1) as f64),
                            }?;
                            let y = r.get(c).and_then(|v| number(v))?;
                            Some([x, y])
                        })
                        .collect();
                    (label(c), points)
                })
                .collect();
            let height = (ui.available_height() - 2.0 * ui.spacing().interact_size.y)
                .clamp(220.0, 500.0);
            egui::ScrollArea::vertical()
                .id_salt("plot-viewport")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    plot(ui, &series, &self.x.map(label).unwrap_or_else(|| "Row number".into()), height);
                });
        } else {
            const PAGE: usize = 200;
            let pages = data.len().div_ceil(PAGE).max(1);
            self.page = self.page.min(pages - 1);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.page > 0, egui::Button::new("Previous rows"))
                    .clicked()
                {
                    self.page -= 1;
                }
                ui.label(format!("Page {} / {pages}", self.page + 1));
                if ui
                    .add_enabled(self.page + 1 < pages, egui::Button::new("Next rows"))
                    .clicked()
                {
                    self.page += 1;
                }
            });
            egui::ScrollArea::both()
                .id_salt("data-table-scroll")
                .show(ui, |ui| {
                    egui::Grid::new("data-table")
                        .striped(true)
                        .spacing([18.0, 8.0])
                        .show(ui, |ui| {
                            ui.weak("Row");
                            for c in 0..columns {
                                ui.strong(label(c));
                            }
                            ui.end_row();
                            for (i, row) in
                                data.iter().enumerate().skip(self.page * PAGE).take(PAGE)
                            {
                                ui.weak((i + 1).to_string());
                                for c in 0..columns {
                                    ui.add(
                                        egui::Label::new(
                                            row.get(c).map(String::as_str).unwrap_or(""),
                                        )
                                        .selectable(true)
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
        }
    }
}
fn plot(ui: &mut Ui, series: &[(String, Vec<Option<[f64; 2]>>)], x_label: &str, height: f32) {
    let p = theme::palette(ui);
    let colors = [
        theme::BLUE,
        theme::ACCENT,
        theme::GREEN,
        egui::Color32::from_rgb(186, 148, 220),
        egui::Color32::from_rgb(235, 198, 112),
    ];
    let points: Vec<_> = series
        .iter()
        .flat_map(|(_, points)| points.iter().flatten())
        .collect();
    if points.is_empty() {
        ui.weak(
            "Choose columns containing finite numbers to plot. Units come from your column names.",
        );
        return;
    }
    // Normalize before subtracting so finite extreme values cannot overflow.
    let scale = [
        points.iter().map(|p| p[0].abs()).fold(1.0, f64::max),
        points.iter().map(|p| p[1].abs()).fold(1.0, f64::max),
    ];
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in &points {
        for axis in 0..2 {
            let v = point[axis] / scale[axis];
            min[axis] = min[axis].min(v);
            max[axis] = max[axis].max(v);
        }
    }
    for axis in 0..2 {
        if min[axis] == max[axis] {
            min[axis] -= 0.5;
            max[axis] += 0.5;
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (i, (label, _)) in series.iter().enumerate() {
            ui.colored_label(colors[i % colors.len()], label);
        }
    });
    let (outer, response) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width().max(180.0),
            height,
        ),
        egui::Sense::hover(),
    );
    let rect = egui::Rect::from_min_max(
        outer.min + egui::vec2(65.0, 15.0),
        outer.max - egui::vec2(20.0, 45.0),
    );
    let painter = ui.painter().with_clip_rect(outer);
    painter.rect_filled(rect, 0, p.field);
    for i in 0..=4 {
        let t = i as f32 / 4.0;
        let y = rect.bottom() - rect.height() * t;
        let x = rect.left() + rect.width() * t;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, p.border),
        );
        painter.text(
            egui::pos2(rect.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{:.3}", (min[1] + (max[1] - min[1]) * t as f64) * scale[1]),
            egui::FontId::monospace(10.0),
            p.muted,
        );
        painter.text(
            egui::pos2(x, rect.bottom() + 8.0),
            egui::Align2::CENTER_TOP,
            format!("{:.3}", (min[0] + (max[0] - min[0]) * t as f64) * scale[0]),
            egui::FontId::monospace(10.0),
            p.muted,
        );
    }
    painter.text(
        egui::pos2(rect.center().x, outer.bottom() - 3.0),
        egui::Align2::CENTER_BOTTOM,
        x_label,
        egui::FontId::proportional(11.0),
        p.text,
    );
    let position = |point: [f64; 2]| {
        egui::pos2(
            rect.left()
                + ((point[0] / scale[0] - min[0]) / (max[0] - min[0])) as f32 * rect.width(),
            rect.bottom()
                - ((point[1] / scale[1] - min[1]) / (max[1] - min[1])) as f32 * rect.height(),
        )
    };
    let mut nearest: Option<(f32, String, [f64; 2], egui::Pos2)> = None;
    for (n, (label, values)) in series.iter().enumerate() {
        let mut previous = None;
        for value in values {
            let Some(value) = value else {
                previous = None;
                continue;
            };
            let point = position(*value);
            let color = colors[n % colors.len()];
            if let Some(last) = previous {
                painter.line_segment([last, point], egui::Stroke::new(1.5_f32, color));
            }
            painter.circle_filled(point, 2.0, color);
            previous = Some(point);
            if let Some(cursor) = response.hover_pos() {
                let d = cursor.distance_sq(point);
                if nearest.as_ref().is_none_or(|p| d < p.0) {
                    nearest = Some((d, label.clone(), *value, point));
                }
            }
        }
    }
    if let Some((_, label, value, point)) = nearest {
        painter.circle_stroke(point, 5.0, egui::Stroke::new(1.0_f32, p.text));
        response.on_hover_text(format!(
            "{label}\n{x_label}: {}\nValue: {}",
            value[0], value[1]
        ));
    }
    let missing: usize = series
        .iter()
        .map(|(_, v)| v.iter().filter(|p| p.is_none()).count())
        .sum();
    if missing > 0 {
        ui.weak(format!(
            "{missing} missing or nonnumeric values are shown as gaps. Points follow file order."
        ));
    }
}
