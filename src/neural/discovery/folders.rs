//! Browse the organization actually chosen by asset authors. Directory rows
//! are metadata; no scan of the host worktree or unpacking of ZIPs is implied.
use super::*;
use crate::library_folders::{self, Entry};

fn root() -> String {
    ".".into()
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderQuery {
    /// Exact library folder path, or "." for the root.
    #[serde(default = "root")]
    #[arg(default_value = ".")]
    pub directory: String,
    #[serde(default)]
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    #[serde(default = "page_size")]
    #[arg(long, default_value_t = 20)]
    pub page_size: usize,
}

pub fn folder(db: &Connection, q: &FolderQuery) -> Result<Value> {
    let directory = if q.directory == "." {
        ""
    } else {
        if !crate::files::canonical_relative_path(&q.directory) {
            return Err(failure(
                "Use a library folder path separated by /, or . for its root",
            ));
        }
        &q.directory
    };
    if q.page_size == 0 {
        return Err(failure("Use a positive page size"));
    }
    let assets = store::list(db, "asset")?;
    let mut paths = Vec::new();
    for (i, asset) in assets
        .iter()
        .enumerate()
        .filter(|(_, a)| a.body["retired"] != true)
    {
        paths.push((
            i,
            asset.body["path"]
                .as_str()
                .ok_or_else(|| failure("Missing library path"))?,
        ));
    }
    let entries = library_folders::entries(Some(directory), paths);
    let total = entries.len();
    let items: Vec<_> = entries.into_iter().skip(q.offset).take(q.page_size).map(|e| match e {
        Entry::Folder { name, files } => json!({"kind":"folder", "path":library_folders::child(directory,&name), "files_below":files}),
        Entry::File(i) => {
            let a = &assets[i];
            json!({"kind":"file", "path":a.body["path"], "reference":a.reference.handle(), "owner":a.body["owner"]})
        }
    }).collect();
    let end = q.offset.saturating_add(items.len());
    Ok(
        json!({"directory":q.directory,"scope":"current non-retired library editions across activities; immediate children, folders first, then files by name", "directory_exists":directory.is_empty() || total != 0,"body_supplied":false,
        "total":total,"offset":q.offset,"page_size":q.page_size,"next_offset":(end<total).then_some(end),"items":items}),
    )
}
