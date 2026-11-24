//! Save a collection of issued editions, each in its own directory. The
//! collection is not a new integrated package or a review of the designs.
use super::{
    activity_sources::delivery, archive, failure, human_requests, package_review, store, Result,
};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct Exported {
    pub directory: PathBuf,
    pub delivery_count: usize,
    pub artifact_count: usize,
    pub manifest: &'static str,
}

/// CLI callers select an activity at command time. A desktop save dialog
/// additionally pins the handles and record digests it offered for export.
pub fn collection(
    root: &Path,
    directory: &Path,
    work: Option<&str>,
    listed: Option<&BTreeMap<String, String>>,
) -> Result<Exported> {
    let db = store::open_inspection(root)?;
    if let Some(id) = work {
        store::required(&db, "work", id)?;
    }
    let deliveries: Vec<_> = store::list(&db, "delivery")?
        .into_iter()
        .filter(|r| work.is_none_or(|w| r.body["work"] == w))
        .collect();
    if let Some(listed) = listed {
        if listed.len() != deliveries.len()
            || deliveries
                .iter()
                .any(|d| listed.get(&d.reference.handle()) != Some(&d.reference.digest))
        {
            return Err(failure("The issued editions changed since this export was opened. Reopen Export deliveries to see the current selection. Nothing was saved."));
        }
    }
    if deliveries.is_empty() {
        return Err(failure(
            "No issued deliveries in this selection. Nothing was exported.",
        ));
    }
    let parent = archive::parent(directory);
    let stage = tempfile::Builder::new()
        .prefix(".personas-export-")
        .tempdir_in(parent)?;
    let mut manifest = Vec::new();
    for head in &deliveries {
        let reference = head.reference.handle();
        let edition = delivery::edition(&db, &reference)
            .map_err(|e| failure(format!("Cannot export {reference}: {e}")))?;
        if head.reference != edition.issue.record.reference {
            return Err(failure(format!(
                "The listed delivery differs from its exact issued edition: {reference}"
            )));
        }
        let folder = PathBuf::from(reference.replace([':', '@'], "_"));
        // Use the same validated portable member tree as a single-package
        // export. Read and verify one file at a time, even across many issues.
        for (name, member) in human_requests::files(&db, &edition.members)? {
            let relative = folder.join(name);
            let asset = store::exact(&db, &member)?;
            let digest = asset.body["blob"]
                .as_str()
                .ok_or_else(|| failure(format!("No file bytes for {member}")))?;
            let bytes = store::bytes(&db, digest)?;
            let path = stage.path().join(&relative);
            std::fs::create_dir_all(path.parent().unwrap())?;
            let mut file = File::create_new(path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            manifest.push(
                json!({"delivery":reference,"reference":member,"export_path":relative,
                "metadata":asset.body,"bytes":bytes.len(),"sha256":digest}),
            );
        }
    }
    let reviews = package_review::exported(&db, &deliveries)?;
    let operators = store::operator_review_history(
        &db,
        &deliveries.iter().map(|d| d.reference.handle()).collect(),
    )?;
    let value = json!({"deliveries":deliveries,"delivery_mandates":store::delivery_mandates(&db, work)?,
        "package_reviews":reviews,"operator_review_history":operators,"artifacts":manifest,"quality_verified_by_export":false});
    drop(db);
    let mut index = File::create_new(stage.path().join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut index, &value)?;
    index.sync_all()?;
    drop(index);
    archive::publish_directory(stage.path(), directory)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(Exported {
        directory: directory.to_owned(),
        delivery_count: deliveries.len(),
        artifact_count: manifest.len(),
        manifest: "manifest.json",
    })
}
