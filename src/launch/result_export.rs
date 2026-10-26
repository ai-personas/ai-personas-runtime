//! Save freshly projected deliveries and explicitly cited supporting files.
use super::*;
use std::io::Write;

/// Export does not execute artifacts, infer task completion, or resume work.
/// A new destination avoids silently replacing the user's existing files.
pub fn export_result_at(root: &Path, operator: Option<&str>, work: &str, destination: &Path) -> Result<Value> {
    identity(work)?;
    inspection::project(root, |snapshot, _| {
        require(operator.is_none_or(|value| value == snapshot.saved.operator), "saved operator mismatch")?;
        let now = unix_now()?;
        let output = results::project_state(&snapshot.state, work, &snapshot.saved, now)?;
        export(&snapshot.state, &output, destination, now)
    })
}

fn export(state: &State, output: &Output, destination: &Path, now: i64) -> Result<Value> {
    let mut files = Vec::new();
    for (delivery_index, record) in output.local_deliveries.iter().enumerate() {
        let delivery = &record.body;
        let artifacts = delivery["artifacts"].as_array()
            .ok_or_else(|| invalid("delivery artifacts unavailable"))?;
        let prefix = format!("delivery-{}", delivery_index + 1);
        files.push((format!("{prefix}/receipt.json"), serde_json::to_vec_pretty(record)?));
        for (artifact_index, artifact) in artifacts.iter().enumerate() {
            let bytes: Vec<u8> = serde_json::from_value(artifact["bytes"].clone())?;
            require(artifact["reference"]["digest"] == s::sha256_bytes(&bytes),
                "export artifact digest differs")?;
            let bundle_text = artifact["qualified_bundle_json"].as_str()
                .ok_or_else(|| invalid("export qualifications unavailable"))?;
            require(artifact["qualified_bundle_sha256"] == s::sha256_bytes(bundle_text.as_bytes()),
                "export qualification digest differs")?;
            let bundle: Value = serde_json::from_str(bundle_text)?;
            let subject: p::ExactRef = serde_json::from_value(artifact["source"].clone())?;
            let reference = file_reference(&subject)?;
            let root = bundle["entries"].as_array().into_iter().flatten()
                .find(|entry| observability::context_reference(&bundle, &entry["reference"])
                    .is_ok_and(|entry_reference| entry_reference == reference))
                .ok_or_else(|| invalid("export artifact source unavailable"))?;
            let path = root["path"].as_str().ok_or_else(|| invalid("export artifact path unavailable"))?;
            require(f::canonical_relative_path(path), "export artifact path is not canonical")?;
            let name = Path::new(path).file_name().and_then(|name| name.to_str())
                .ok_or_else(|| invalid("export artifact filename unavailable"))?;
            let artifact_prefix = format!("{prefix}/artifact-{}", artifact_index + 1);
            files.push((format!("{artifact_prefix}/content/{name}"), bytes));
            files.push((format!("{artifact_prefix}/qualified-bundle.json"), bundle_text.as_bytes().to_vec()));
        }
    }
    let mut cited = BTreeSet::new();
    for message in &output.messages {
        let sender = state.personas.get(&message.sender)
            .ok_or_else(|| invalid("export citation sender unavailable"))?;
        let mut a = admission(sender, now);
        a.audience = BTreeSet::from([message.recipient.clone()]);
        for subject in &message.file_references {
            // A range or decoded string is a citation to a projection, not an
            // instruction to export its entire containing file.
            if subject.start.is_some() || subject.end.is_some()
                || !matches!(subject.representation.as_str(), "utf8" | "bytes_metadata") {
                continue;
            }
            let reference = file_reference(subject)?;
            let stored = record(state, &reference)?;
            if stored.record.scope != f::Scope::Assets
                || !cited.insert((reference.source.clone(), reference.revision.clone())) {
                continue;
            }
            require(lineage_permitted(state, std::slice::from_ref(&reference), &a),
                "cited file export unavailable")?;
            // Export is an explicit reader action. Resolve current qualifications
            // without copying the file body into the metadata document as well.
            let metadata = f::Reference::full(&reference.source, &reference.revision,
                f::Representation::BytesMetadata);
            let assembly = f::assemble(&corpus_for_delivery(state, &a)?,
                std::slice::from_ref(&metadata), &[], &[], &a, usize::MAX, 0, 0)?;
            let references = assembly.entries.iter().map(|entry| entry.reference.clone()).collect::<Vec<_>>();
            let bundle = attributed_context(state, &assembly.rendered_context, &references)?;
            require(f::canonical_relative_path(&stored.record.path), "export cited path is not canonical")?;
            let name = Path::new(&stored.record.path).file_name().and_then(|name| name.to_str())
                .ok_or_else(|| invalid("export cited filename unavailable"))?;
            let prefix = format!("referenced-file-{}", cited.len());
            files.push((format!("{prefix}/content/{name}"), stored.record.bytes.clone()));
            files.push((format!("{prefix}/qualified-bundle.json"), bundle.as_bytes().to_vec()));
            files.push((format!("{prefix}/source.json"), serde_json::to_vec_pretty(&json!({
                "kind":"requested_cited_file_export", "cited_reference":subject,
                "metadata_reference":wire_reference(&metadata),
                "bytes":stored.record.bytes.len(),"sha256":s::sha256_bytes(&stored.record.bytes),
                "recorded_author":stored.record.author,"origin":stored.origin,
                "citation_message":{"id":message.id,"sender":message.sender,
                    "recipient":message.recipient,"invocation":message.invocation},
                "qualified_bundle_sha256":s::sha256_bytes(bundle.as_bytes()),
                "meaning":"The reader requested these exact cited file bytes. The original message citation supplied no body; this export is not a formal release, acceptance or task completion."
            }))?));
        }
    }
    if files.is_empty() {
        return Ok(json!({"work":output.work_id,"status":"no_exportable_files",
            "destination":null,"files":[],"delivery_count":0,"cited_file_count":0,
            "inference_performed":false,"artifacts_executed":false,"goal_completed":false}));
    }
    if let Some(parent) = destination.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(destination)?;
    let destination = std::fs::canonicalize(destination)?;
    let mut written = Vec::new();
    for (relative, bytes) in files {
        let path = destination.join(&relative);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        written.push(json!({"path":path,"bytes":bytes.len(),"sha256":s::sha256_bytes(&bytes)}));
    }
    Ok(json!({"work":output.work_id,"status":"exported","destination":destination,"files":written,
        "delivery_count":output.local_deliveries.len(),"cited_file_count":cited.len(),"inference_performed":false,
        "artifacts_executed":false,"goal_completed":false}))
}
