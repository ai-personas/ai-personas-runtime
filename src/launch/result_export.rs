//! Save freshly projected deliveries and explicitly cited supporting files.
use super::*;
use std::collections::btree_map::Entry;
use std::io::Write;

/// Export does not execute artifacts, infer task completion, or resume work.
/// A new destination avoids silently replacing the user's existing files.
pub fn export_result_at(root: &Path, operator: Option<&str>, work: &str, destination: &Path) -> Result<Value> {
    identity(work)?;
    inspection::project(root, |snapshot, records| {
        require(operator.is_none_or(|value| value == snapshot.saved.operator), "saved operator mismatch")?;
        let now = unix_now()?;
        let output = results::project_state(&snapshot.state, work, &snapshot.saved, now, records)?;
        export(&snapshot.state, &output, destination, now)
    })
}

fn export(state: &State, output: &Output, destination: &Path, now: i64) -> Result<Value> {
    let mut files = Vec::new();
    let mut delivered_index = Vec::new();
    let mut cited_index = Vec::new();
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
            let content_path = format!("{artifact_prefix}/content/{name}");
            let bundle_path = format!("{artifact_prefix}/qualified-bundle.json");
            delivered_index.push(IndexEntry {
                name: name.into(), content_path: content_path.clone(),
                content_sha256: s::sha256_bytes(&bytes),
                attribution: format!("Delivery package {}", delivery_index + 1),
                source_path: format!("{prefix}/receipt.json"), bundle_path: bundle_path.clone(),
            });
            files.push((content_path, bytes));
            files.push((bundle_path, bundle_text.as_bytes().to_vec()));
        }
    }
    let mut cited = BTreeSet::new();
    let mut citation_corpora = BTreeMap::new();
    for (message_index, message) in output.messages.iter().enumerate() {
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
            // State and admission time are fixed for this export. Reuse only
            // the same sender/recipient catalogue within this view; each exact
            // root still receives its own lineage and qualification checks.
            let corpus = match citation_corpora.entry((message.sender.as_str(), message.recipient.as_str())) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(corpus_for_delivery(state, &a)?),
            };
            let assembly = f::assemble(corpus,
                std::slice::from_ref(&metadata), &[], &[], &a, usize::MAX, 0, 0)?;
            let references = assembly.entries.iter().map(|entry| entry.reference.clone()).collect::<Vec<_>>();
            let bundle = attributed_context(state, &assembly.rendered_context, &references)?;
            require(f::canonical_relative_path(&stored.record.path), "export cited path is not canonical")?;
            let name = Path::new(&stored.record.path).file_name().and_then(|name| name.to_str())
                .ok_or_else(|| invalid("export cited filename unavailable"))?;
            let prefix = format!("referenced-file-{}", cited.len());
            let content_path = format!("{prefix}/content/{name}");
            let bundle_path = format!("{prefix}/qualified-bundle.json");
            let source_path = format!("{prefix}/source.json");
            cited_index.push(IndexEntry {
                name: name.into(), content_path: content_path.clone(),
                content_sha256: s::sha256_bytes(&stored.record.bytes),
                attribution: format!("{}, reply {}: {}", sender.seed.label, message_index + 1, message.subject),
                source_path: source_path.clone(), bundle_path: bundle_path.clone(),
            });
            files.push((content_path, stored.record.bytes.clone()));
            files.push((bundle_path, bundle.as_bytes().to_vec()));
            files.push((source_path, serde_json::to_vec_pretty(&json!({
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
    files.push(("index.html".into(), render_index(&delivered_index, &cited_index).into_bytes()));
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
    Ok(json!({"work":output.work_id,"status":"exported","destination":destination,
        "index_path":destination.join("index.html"),"files":written,
        "delivery_count":output.local_deliveries.len(),"cited_file_count":cited.len(),"inference_performed":false,
        "artifacts_executed":false,"goal_completed":false}))
}

struct IndexEntry {
    name: String,
    content_path: String,
    content_sha256: String,
    attribution: String,
    source_path: String,
    bundle_path: String,
}

fn html_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#39;")
}

fn relative_url(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Only escaped labels and relative links are rendered. Artifact bodies remain
/// separate files; opening the index neither embeds nor executes their content.
fn render_index(delivered: &[IndexEntry], cited: &[IndexEntry]) -> String {
    let mut html = String::from(r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Exported files</title>
<style>body{font:1rem/1.6 system-ui,sans-serif;max-width:54rem;margin:3rem auto;padding:0 1.5rem;color:#20252b;background:#fff}h1,h2{line-height:1.2}a{color:#0758ae;overflow-wrap:anywhere}li{margin:1.2rem 0;padding-bottom:1rem;border-bottom:1px solid #dde1e6}p{margin:.35rem 0}details{font-size:.9rem}summary{cursor:pointer}ul{padding-left:1.3rem}</style>
</head><body><h1>Exported files</h1>
<p>Open a file below. Identical contents appear together; changed contents remain separate. Every copy keeps its own attribution, source record and qualifications.</p>
"#);
    for (heading, entries) in [("Delivered files", delivered), ("Files cited in replies", cited)] {
        if entries.is_empty() { continue; }
        html.push_str(&format!("<h2>{heading}</h2><ul>\n"));
        // Group the index only. Every exact copy and its provenance remains
        // exported; equal bytes do not merge authorship or qualifications.
        let mut groups: Vec<Vec<&IndexEntry>> = Vec::new();
        let mut positions = BTreeMap::new();
        for entry in entries {
            let position = *positions.entry(&entry.content_sha256).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[position].push(entry);
        }
        for group in groups {
            let entry = group[0];
            if group.len() > 1 {
                html.push_str(&format!(
                    "<li><a href=\"{}\" download>{}</a><p>Identical contents in {} exported copies.</p><details><summary>All copies, sources and qualifications</summary>\n",
                    relative_url(&entry.content_path), html_text(&entry.name), group.len(),
                ));
                for copy in group {
                    html.push_str(&format!(
                        "<p><a href=\"{}\" download>{}</a><br>{}<br><a href=\"{}\">Source record</a> · <a href=\"{}\">Qualifications</a></p>\n",
                        relative_url(&copy.content_path), html_text(&copy.name), html_text(&copy.attribution),
                        relative_url(&copy.source_path), relative_url(&copy.bundle_path),
                    ));
                }
                html.push_str("</details></li>\n");
                continue;
            }
            html.push_str(&format!(
                "<li><a href=\"{}\" download>{}</a><p>{}</p><details><summary>Source and qualifications</summary><a href=\"{}\">Source record</a> · <a href=\"{}\">Qualifications</a></details></li>\n",
                relative_url(&entry.content_path), html_text(&entry.name), html_text(&entry.attribution),
                relative_url(&entry.source_path), relative_url(&entry.bundle_path),
            ));
        }
        html.push_str("</ul>\n");
    }
    html.push_str("</body></html>\n");
    html
}
