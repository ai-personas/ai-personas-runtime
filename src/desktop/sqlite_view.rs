//! Inspect ordinary tables through a read-only connection, one bounded page at a time.
use super::{data_view, input, native_preview::Cancellation, Source};
use eframe::egui::{self, Ui};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use std::{
    fs::File,
    io::Seek,
    path::{Path, PathBuf},
    sync::Arc,
};

const ROWS: usize = 150;

pub(super) struct Database {
    path: PathBuf,
    _staging: Option<tempfile::TempDir>,
    tables: Vec<String>,
}

pub(super) struct Page {
    pub db: Arc<Database>,
    table: usize,
    index: usize,
    rows: Vec<Vec<String>>,
    more: bool,
}

fn connect(path: &Path) -> Result<Connection, String> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|e| e.to_string())?;
    Ok(db)
}

impl Page {
    pub fn open(source: &Source, file: &mut File, cancel: &Cancellation) -> Result<Self, String> {
        let (path, staging) = if let Source::File(path) = source {
            (path.clone(), None)
        } else {
            let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
            let path = dir.path().join("snapshot.sqlite");
            file.rewind().map_err(|e| e.to_string())?;
            input::copy(
                file,
                &mut File::create(&path).map_err(|e| e.to_string())?,
                cancel,
            )?;
            (path, Some(dir))
        };
        let connection = connect(&path)?;
        let mut query=connection.prepare("SELECT name FROM pragma_table_list WHERE schema='main' AND type='table' AND name NOT GLOB 'sqlite_*' ORDER BY name").map_err(|e|e.to_string())?;
        let tables = query
            .query_map([], |row| row.get(0))
            .and_then(|items| items.collect())
            .map_err(|e| e.to_string())?;
        Self::read(
            Arc::new(Database {
                path,
                _staging: staging,
                tables,
            }),
            0,
            0,
        )
    }

    pub fn read(db: Arc<Database>, table: usize, index: usize) -> Result<Self, String> {
        let mut rows = Vec::new();
        let mut more = false;
        if let Some(name) = db.tables.get(table) {
            let connection = connect(&db.path)?;
            let name = name.replace('"', "\"\"");
            let mut query = connection
                .prepare(&format!("SELECT * FROM main.\"{name}\" LIMIT ? OFFSET ?"))
                .map_err(|e| e.to_string())?;
            rows.push(
                query
                    .column_names()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            );
            let columns = query.column_count();
            let mut result = query
                .query([(ROWS + 1) as i64, (index * ROWS) as i64])
                .map_err(|e| e.to_string())?;
            while let Some(row) = result.next().map_err(|e| e.to_string())? {
                if rows.len() > ROWS {
                    more = true;
                    break;
                }
                let mut values = Vec::with_capacity(columns);
                for column in 0..columns {
                    let cell = match row.get_ref(column).map_err(|e| e.to_string())? {
                        ValueRef::Null => "NULL".into(),
                        ValueRef::Integer(n) => n.to_string(),
                        ValueRef::Real(n) => n.to_string(),
                        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                        ValueRef::Blob(bytes) => format!("BLOB · {} bytes", bytes.len()),
                    };
                    values.push(cell);
                }
                rows.push(values);
            }
        }
        Ok(Self {
            db,
            table,
            index,
            rows,
            more,
        })
    }

    pub fn ui(&self, ui: &mut Ui, table_view: &mut data_view::TableView) -> Option<(usize, usize)> {
        if self.db.tables.is_empty() {
            ui.weak("This database has no ordinary tables.");
            return None;
        }
        let mut table = self.table;
        let mut index = self.index;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("sqlite-table")
                .selected_text(&self.db.tables[table])
                .show_ui(ui, |ui| {
                    for (i, name) in self.db.tables.iter().enumerate() {
                        ui.selectable_value(&mut table, i, name);
                    }
                });
            ui.weak(format!("{} tables", self.db.tables.len()));
            if ui
                .add_enabled(index > 0, egui::Button::new("Previous block"))
                .clicked()
            {
                index -= 1;
            }
            if ui
                .add_enabled(self.more, egui::Button::new("Next block"))
                .clicked()
            {
                index += 1;
            }
            if self.rows.len() > 1 {
                ui.small(format!(
                    "Rows {}–{}",
                    self.index * ROWS + 1,
                    self.index * ROWS + self.rows.len() - 1
                ));
            }
        });
        ui.add_space(6.);
        table_view.ui(ui, &self.rows);
        if table != self.table {
            Some((table, 0))
        } else if index != self.index {
            Some((table, index))
        } else {
            None
        }
    }
}
