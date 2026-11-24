//! Discover a receipt without supplying its literal arguments or treating an
//! output with identical bytes as a run that selected the queried edition.
use super::{failure, store, workspace, Connection, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct History {
    /// Exact installation, delivery, supplied asset, byte object or command_run receipt. Only a receipt request includes the full run.
    pub reference: Option<String>,
    /// Starting position in each of the two independently ordered result lists.
    #[serde(default)]
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    #[serde(default = "default_page")]
    #[arg(long, default_value_t = 20)]
    #[schemars(range(min = 1))]
    pub page_size: usize,
}
fn default_page() -> usize {
    20
}

struct Filter<'a> {
    reference: &'a str,
    object: Option<String>,
}
impl Filter<'_> {
    // Prefer the edition explicitly named by the run. A match on the byte
    // object alone (even if that object was captured before publication) is
    // useful discovery, not evidence that this asset revision was selected.
    fn add(
        &self,
        hits: &mut Vec<Value>,
        role: &str,
        path: Option<&str>,
        named: Option<&str>,
        object: Option<&str>,
    ) {
        let basis = if named == Some(self.reference) {
            "exact_reference"
        } else if object.is_some() && object == self.object.as_deref() {
            "same_content_address"
        } else {
            return;
        };
        hits.push(json!({"role":role,"basis":basis,"path":path,"recorded_reference":named,"object":object}));
    }
    fn run_matches(&self, receipt: &str, body: &Value) -> Vec<Value> {
        let mut hits = Vec::new();
        self.add(&mut hits, "receipt", None, Some(receipt), None);
        self.add(
            &mut hits,
            "installation",
            None,
            body["installation"].as_str(),
            None,
        );
        self.add(
            &mut hits,
            "receiving_edition",
            None,
            body["receiving"]["delivery"].as_str(),
            None,
        );
        self.add(&mut hits, "program", None, None, body["program"].as_str());
        for role in ["stdin", "stdout", "stderr"] {
            let stream = &body[role];
            self.add(
                &mut hits,
                role,
                None,
                stream["reference"].as_str(),
                stream["object"].as_str(),
            );
        }
        for (field, role) in [
            ("files_supplied", "file_input"),
            ("outputs", "captured_output"),
        ] {
            for (path, file) in body[field].as_object().into_iter().flatten() {
                self.add(
                    &mut hits,
                    role,
                    Some(path),
                    file["reference"].as_str(),
                    file["object"].as_str(),
                );
            }
        }
        hits
    }
}

pub fn history(db: &Connection, query: &History) -> Result<Value> {
    if query.page_size == 0 {
        return Err(failure("Choose a positive page size"));
    }
    let mut selected = None;
    let filter = query
        .reference
        .as_deref()
        .map(|reference| -> Result<Filter<'_>> {
            let object = match reference.split_once(':').map(|(kind, _)| kind) {
                Some("asset" | "object") => {
                    Some(format!("object:{}", workspace::byte_digest(db, reference)?))
                }
                Some("installation" | "delivery" | "command_run") => {
                    let record = store::exact(db, reference)?;
                    if record.reference.handle() != reference {
                        return Err(failure("Choose a canonical exact reference"));
                    }
                    if record.reference.kind == "command_run" {
                        selected = Some(record.body);
                    }
                    None
                }
                _ => return Err(failure(
                    "Choose an exact installation, delivery, command receipt, asset or byte object",
                )),
            };
            Ok(Filter { reference, object })
        })
        .transpose()?;
    let on_page = |ordinal: usize| {
        (query.offset..query.offset.saturating_add(query.page_size)).contains(&ordinal)
    };
    let mut runs = Vec::new();
    let mut total = 0;
    let summary = |handle: &str, body: &Value, matches: Option<Vec<Value>>| {
        let mut row = json!({"receipt":handle,"actor":body["actor"],"origin":body["origin"],"work":body["work"],
                "installation":body["installation"],"engine":body["engine"],"argv0":body["argv0"],
                "argument_count":body["arguments"].as_array().map(Vec::len),
                "captured_file_count":body["outputs"].as_object().map(|o|o.len()),
                "process_started":body["process_started"],"exit_code":body["exit_code"],"success":body["success"],
                "outputs_complete":body["outputs_complete"],"finished":body["finished"]});
        if let Some(subject) = body.get("receiving") {
            row["receiving"] = subject.clone();
        }
        if let Some(matches) = matches {
            row["matches"] = json!(matches);
        }
        row
    };
    if let (Some(filter), Some(body)) = (&filter, &selected) {
        total = 1;
        if on_page(0) {
            runs.push(summary(
                filter.reference,
                body,
                Some(filter.run_matches(filter.reference, body)),
            ));
        }
    } else {
        let mut stmt = db.prepare(
            "SELECT id,revision,digest FROM versions WHERE kind='command_run' ORDER BY rowid DESC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            // Counting version rows in an unfiltered list doesn't need the
            // full command (possibly an inline program) outside the page.
            if filter.is_none() && !on_page(total) {
                total += 1;
                continue;
            }
            let (id, rev): (String, i64) = (row.get(0)?, row.get(1)?);
            let handle = format!("command_run:{id}@{rev}");
            let body = store::object(db, &row.get::<_, String>(2)?)?;
            let matches = filter.as_ref().map(|q| q.run_matches(&handle, &body));
            if matches.as_ref().is_some_and(Vec::is_empty) {
                continue;
            }
            if on_page(total) {
                runs.push(summary(&handle, &body, matches));
            }
            total += 1;
        }
    }
    let mut installations = Vec::new();
    let mut installation_total = 0;
    if selected.is_none() {
        for r in store::list(db, "installation")? {
            if r.body["format"] != "portable-command-v1" {
                continue;
            }
            let id = r.reference.handle();
            let source = r.body["source"]
                .as_str()
                .filter(|s| s.starts_with("asset:") || s.starts_with("object:"));
            let program = r.body["program"].as_str();
            let matches = filter.as_ref().map(|q| {
                let mut hits = Vec::new();
                q.add(&mut hits, "installation", None, Some(&id), None);
                q.add(&mut hits, "installed_source", None, source, program);
                hits
            });
            if matches.as_ref().is_some_and(Vec::is_empty) {
                continue;
            }
            if on_page(installation_total) {
                let mut row = json!({"reference":id,"name":r.body["name"],"engine":r.body["engine"],"program":program,"installed_by":r.body["installed_by"]});
                if let Some(matches) = matches {
                    row["matches"] = json!(matches);
                }
                installations.push(row);
            }
            installation_total += 1;
        }
    }
    let next = |shown, total| {
        let end = query.offset.saturating_add(shown);
        (end < total).then_some(end)
    };
    Ok(
        json!({"scope":"retained command runs (newest first) and current portable installations (by id)",
        "match_surface":"recorded references and content addresses, not file contents or command arguments",
        "reference":query.reference,"page_size":query.page_size,"offset":query.offset,
        "total":total,"next_offset":next(runs.len(), total),"runs":runs,
        "installation_total":installation_total,"next_installation_offset":next(installations.len(),installation_total),"installations":installations,
        "selected":selected.map(|body|json!({"receipt":query.reference,"result":body}))}),
    )
}
