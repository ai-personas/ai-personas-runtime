//! Explicit, activity-local working sources. Only an accepted open operation
//! reads the host. Later requests use the selected edition's immutable bytes.
use super::read_file;
use crate::neural::{context, failure, store, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Open {
    /// Replace your open text sources for this activity. [] closes all.
    pub paths: Vec<Source>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Source {
    File(String),
    /// An exact UTF-8 byte range, end exclusive.
    Range {
        path: String,
        start: usize,
        end: usize,
    },
}

impl Source {
    fn parts(&self) -> (&str, Option<(usize, usize)>) {
        match self {
            Self::File(path) => (path, None),
            Self::Range { path, start, end } => (path, Some((*start, *end))),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Opened {
    path: String,
    reference: String,
    bytes: usize,
    start: usize,
    end: usize,
}

pub(crate) fn capture(
    store: &mut store::Store,
    directory: &Path,
    work: &str,
    actor: &str,
    call: &str,
    operation: &str,
    input: Open,
) -> Result<Value> {
    let mut captured = std::collections::BTreeMap::new();
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    for source in &input.paths {
        let (name, range) = source.parts();
        let path = Path::new(name)
            .strip_prefix(directory)
            .unwrap_or(Path::new(name));
        if !captured.contains_key(path) {
            let (_, data) = read_file(directory, name).map_err(|e| {
                failure(format!(
                    "Cannot open {name:?}: {e}; previous selection retained"
                ))
            })?;
            captured.insert(path, data);
        }
        let data = &captured[path];
        let (start, end) = range.unwrap_or((0, data.len()));
        let text = data
            .get(start..end)
            .and_then(|s| std::str::from_utf8(s).ok());
        if text.is_none() {
            return Err(failure(format!(
                "No UTF-8 byte range {start}..{end} in {name:?}; previous selection retained"
            )));
        }
        if seen.insert((path, start, end)) {
            sources.push((path, start, end));
        }
    }
    store.transaction(|db| {
        let objects = captured
            .iter()
            .map(|(path, data)| {
                store::put_bytes(db, "application/octet-stream", data).map(|digest| (*path, digest))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
        let files = sources
            .iter()
            .map(|&(path, start, end)| Opened {
                path: path.to_string_lossy().into_owned(),
                reference: format!("object:{}", objects[path]),
                bytes: captured[path].len(),
                start,
                end,
            })
            .collect::<Vec<_>>();
        // Check the same view preparation will use (including protected
        // historical provider metadata) before replacing the last good set.
        for f in &files {
            if !context::read_for(db, Some(actor), &f.reference, Some(f.start), Some(f.end))?
                ["text"]
                .is_string()
            {
                return Err(failure(
                    "Select a readable text resource; previous selection retained",
                ));
            }
        }
        let key = format!("{work}/{actor}");
        let last = store::head(db, "project_view", &key)?.map_or(0, |r| r.reference.revision);
        let body = json!({"work":work,"actor":actor,"invocation":call,"operation":operation,
            "directory":directory,"files":files});
        let edition = store::write(db, "project_view", &key, last, &body, Some(call))?;
        Ok(json!({"view":edition.handle(),"files":files,"host_files_written":false}))
    })
}

pub(crate) fn supply(
    db: &Connection,
    actor: &str,
    work: &str,
    recovery: bool,
    selected: &mut Vec<Value>,
) -> Result<Option<Value>> {
    let Some(view) = store::head(db, "project_view", &format!("{work}/{actor}"))? else {
        return Ok(None);
    };
    let edition = view.reference.handle();
    let files: Vec<Opened> = serde_json::from_value(view.body["files"].clone())?;
    for f in &files {
        let mut resource = if recovery {
            let mut metadata = context::reference_metadata(db, &f.reference)?;
            metadata["selected_range"] = json!({"start":f.start,"end":f.end});
            metadata
        } else {
            context::read_for(db, Some(actor), &f.reference, Some(f.start), Some(f.end))?
        };
        resource["workspace_path"] = json!(f.path);
        resource["opened_from"] = json!(edition);
        selected.push(resource);
    }
    Ok(Some(
        json!({"edition":edition,"count":files.len(),"change_with":"workspace.open",
        "meaning":"Your last successful selection here. Frozen bytes, not a live read of changing host files."}),
    ))
}
