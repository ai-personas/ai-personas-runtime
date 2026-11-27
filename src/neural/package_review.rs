//! A finding belongs to the constituent versions and brief the reviewer assessed.
//! Matching those identities does not establish the truth of the finding.
pub mod after_issue;
mod notice;
pub use notice::FindingNotice;
mod comparisons;
mod files;
pub use files::EvidenceFile;
pub use comparisons::lookup;
pub(super) use comparisons::{edition_notes, lookup_for};
pub(super) mod peer;
mod publication;
use super::{failure, id, operations, store, Result};
use clap::{Args, ValueEnum};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    /// Review an issued delivery and the exact brief under which it was submitted.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<String>,
    /// Or select exact assets as a proposed package under the reporting activity's current brief.
    #[arg(long = "artifact")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    /// Extra exact inputs governing a proposed package, such as a material specification. An issued delivery supplies its own declared inputs.
    #[arg(long = "input")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Supported,
    Concern,
    Inconclusive,
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Review {
    #[command(flatten)]
    pub subject: Subject,
    /// Exact brief the reviewer saw. Publishing fails if this package now resolves to another brief.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_mandate: Option<String>,
    /// The particular claim, interface or intended use you assessed.
    #[arg(long)]
    pub criterion: String,
    /// Your judgment on that criterion, with observations, conditions and remaining uncertainty.
    #[arg(long)]
    pub finding: String,
    #[arg(long)]
    pub verdict: Verdict,
    /// Supporting exact references, including observed files, receipts or experiences.
    #[arg(long = "evidence")]
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    #[command(flatten)]
    pub subject: Subject,
    /// Optionally compare one exact package_review, even if it shares no files with this package.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long,default_value_t=page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    20
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Member {
    reference: String,
    content: String,
    content_author: Option<String>,
}
type Members = BTreeMap<String, Member>;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Package {
    delivery: Option<String>,
    title: Option<String>,
    mandate: Option<String>,
    members: Members,
    #[serde(default)]
    inputs: BTreeSet<String>,
}

impl Package {
    fn same_basis(&self, other: &Self) -> bool {
        self.mandate.is_some()
            && self.mandate == other.mandate
            && self.members == other.members
            && self.inputs == other.inputs
    }
}

struct Target {
    package: Package,
    release_order: Option<i64>,
    cited: BTreeSet<String>,
    relied_on: BTreeSet<String>,
}

fn typed(db: &Connection, reference: &str, kind: &str) -> Result<store::Record> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != kind || record.reference.handle() != reference {
        return Err(failure(format!("Choose an exact {kind} reference")));
    }
    if kind == "package_review" && record.body["reference"] != reference {
        return Err(failure("Review identity differs from its retained version"));
    }
    Ok(record)
}

pub(super) fn check_reference(db: &Connection, reference: &str) -> Result<()> {
    typed(db, reference, "package_review").map(|_| ())
}

/// Used on open and save, including Studio's raw record viewer. An index row
/// or a once-verified comparison page is not sufficient to attribute the words.
pub fn published_record(db: &Connection, reference: &str) -> Result<store::Record> {
    let record = typed(db, reference, "package_review")?;
    publication::checked(db, &record, None)?;
    Ok(record)
}

/// An exact address can be selected before its publishing decision commits.
/// Leave a missing proof readable as a gap, without quoting the record or
/// preventing a later healthy decision from inspecting it afresh.
pub(super) fn read_finding(db: &Connection, reference: &str, record: &store::Record) -> Result<Value> {
    if record.reference.handle() != reference { return Err(failure("Choose a canonical exact review revision")); }
    Ok(match publication::checked(db, record, None) {
        Ok(proof) => json!({"reference":reference,"record":record.body,"publication_event":proof.sequence}),
        Err(e) => json!({"reference":reference,"unavailable":format!("Cannot verify review publication: {e}")}),
    })
}

fn members(db: &Connection, artifacts: &[String]) -> Result<Members> {
    if artifacts.is_empty() {
        return Err(failure(
            "Choose at least one exact asset for a package review",
        ));
    }
    operations::package_paths(db, artifacts)?
        .into_iter()
        .map(|(path, reference)| {
            let asset = typed(db, &reference, "asset")?;
            let digest = asset.body["blob"]
                .as_str()
                .ok_or_else(|| failure("Package asset is missing its content reference"))?;
            let author = asset.body["last_content_author"]
                .as_str()
                .or(asset.body["owner"].as_str())
                .map(str::to_owned);
            Ok((
                path.to_string_lossy().into_owned(),
                Member {
                    reference,
                    content: format!("object:{digest}"),
                    content_author: author,
                },
            ))
        })
        .collect()
}
fn refs(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
fn version_order(db: &Connection, r: &store::Reference) -> Result<i64> {
    Ok(db.query_row(
        "SELECT rowid FROM versions WHERE kind=?1 AND id=?2 AND revision=?3",
        params![r.kind, r.id, r.revision],
        |row| row.get(0),
    )?)
}

fn target(
    db: &Connection,
    subject: &Subject,
    work: Option<&str>,
    reader: Option<&str>,
) -> Result<Target> {
    if let Some(reference) = &subject.delivery {
        if !subject.artifacts.is_empty() || !subject.inputs.is_empty() {
            return Err(failure(
                "Choose one issued delivery or a proposed set of files and inputs",
            ));
        }
        let item = typed(db, reference, "delivery")?;
        let artifacts: Vec<String> = serde_json::from_value(item.body["artifacts"].clone())?;
        let work = item.body["work"]
            .as_str()
            .ok_or_else(|| failure("Delivery has no recorded activity"))?;
        let mandate = match item.body["mandate"].as_str() {
            Some(m) => Some(m.to_owned()),
            None => store::delivery_mandates(db, Some(work))?
                .get(reference)
                .and_then(|r| r["delivered_under"].as_str())
                .map(str::to_owned),
        };
        Ok(Target {
            package: Package {
                delivery: Some(reference.clone()),
                title: item.body["title"].as_str().map(str::to_owned),
                mandate,
                members: members(db, &artifacts)?,
                inputs: refs(&item.body["review_inputs"]),
            },
            release_order: Some(version_order(db, &item.reference)?),
            cited: refs(&item.body["evidence"]),
            relied_on: refs(&item.body["reviews"]),
        })
    } else {
        operations::check_shared_evidence(db, reader, &subject.inputs)?;
        let mandate = work
            .map(|w| store::required(db, "work", w).map(|r| r.reference.handle()))
            .transpose()?;
        Ok(Target {
            package: Package {
                delivery: None,
                title: None,
                mandate,
                members: members(db, &subject.artifacts)?,
                inputs: subject.inputs.iter().cloned().collect(),
            },
            release_order: None,
            cited: BTreeSet::new(),
            relied_on: BTreeSet::new(),
        })
    }
}

pub(super) enum Publisher<'a> {
    Persona(&'a str),
    Operator(&'a crate::neural::input_files::InputFiles),
}

pub(super) fn publish(
    db: &Connection,
    work: &str,
    author: &str,
    publisher: Publisher<'_>,
    input: &Review,
) -> Result<Value> {
    let call = match publisher {
        Publisher::Persona(call) => Some(call),
        Publisher::Operator(_) => {
            if let Some(r) = &input.subject.delivery { after_issue::public_issue(db, r)?; }
            None
        }
    };
    if input.criterion.trim().is_empty() || input.finding.trim().is_empty() {
        return Err(failure(
            "Give the criterion you assessed and what you found",
        ));
    }
    let reporting = store::required(db, "work", work)?;
    let subject = target(db, &input.subject, Some(work), call.map(|_| author))?.package;
    if let Some(expected) = &input.expected_mandate {
        if subject.mandate.as_ref() != Some(expected) {
            return Err(failure(
                "The reviewed brief changed. Reload the package before publishing your finding.",
            ));
        }
    }
    operations::check_shared_evidence(db, call.map(|_| author), &input.evidence)?;
    let key = id("review")?;
    let reference = format!("package_review:{key}@1");
    let mut body = json!({"reference":reference,"subject":subject,"author":author,"author_kind":if call.is_some(){"persona"}else{"operator"},
        "work":work,"reporting_mandate":reporting.reference.handle(),"decision":call,"criterion":input.criterion,
        "finding":input.finding,"verdict":input.verdict,"evidence":input.evidence});
    if let Publisher::Operator(files) = publisher {
        if !files.is_empty() {
            body["attachments"] = json!(files.retain(db)?);
            files::manifest(&body)?;
        }
    }
    let written = store::write(db, "package_review", &key, 0, &body, call)?;
    let sender = if call.is_some() {
        author.to_owned()
    } else {
        format!("operator:{author}")
    };
    let mut notice = super::protocol::PublicationKind::PackageReview.notice(&reference, &body);
    // Operators have no retained model choice against which to check the
    // full finding. Commit its exact text and evidence to the broadcast.
    if call.is_none() { notice["published_record"] = json!(format!("object:{}", written.digest)); }
    store::event(db, work, &sender, "*", "package_review", &notice, call)?;
    let mut result = json!({"reference":reference,"verdict":input.verdict,"reviewed_delivery":subject.delivery,"reviewed_mandate":subject.mandate});
    if let (Some(call), Some(delivery)) = (call, subject.delivery.as_deref()) {
        if let Some(address) = peer::notify_issuer(db, work, author, call, delivery, &notice)? {
            result["issuer_notice"] = json!(address);
        }
    }
    Ok(result)
}

fn asset_id(reference: &str) -> &str {
    reference.rsplit_once('@').map_or(reference, |(key, _)| key)
}

fn related(before: &Members, after: &Members) -> bool {
    let ids: BTreeSet<_> = before.values().map(|m| asset_id(&m.reference)).collect();
    after
        .iter()
        .any(|(path, m)| before.contains_key(path) || ids.contains(asset_id(&m.reference)))
}

fn compare(target: &Target, before: &Package, review: &Value, order: i64) -> Value {
    let after = &target.package;
    let paths: BTreeSet<_> = before.members.keys().chain(after.members.keys()).collect();
    let mut files = Vec::new();
    for path in paths {
        let was = before.members.get(path);
        let now = after.members.get(path);
        let change = match (was, now) {
            (Some(a), Some(b)) if a.reference == b.reference => "same",
            (Some(_), Some(_)) => "changed",
            (None, Some(_)) => "added",
            _ => "removed",
        };
        files.push(
            json!({"path":path,"change":change,"reviewed":was,"selected":now,
            "same_content":was.zip(now).map(|(a,b)|a.content==b.content)}),
        );
    }
    let same_files = before.members == after.members;
    let same_mandate = before
        .mandate
        .as_ref()
        .zip(after.mandate.as_ref())
        .map(|(a, b)| a == b);
    let reference = review["reference"].as_str().unwrap_or("");
    json!({"review":review,"files":files,"same_files":same_files,"same_mandate":same_mandate,
        "same_inputs":before.inputs==after.inputs, "same_basis":before.same_basis(after),
        "available_at_delivery":target.release_order.map(|released|order<released),
        "cited_as_evidence":target.cited.contains(reference),"listed_in_reviews":target.relied_on.contains(reference)})
}

pub(super) fn check_reliance(
    db: &Connection,
    artifacts: &[String],
    mandate: &str,
    inputs: &[String],
    reviews: &[String],
    issuing_call: &str,
) -> Result<()> {
    let selected = members(db, artifacts)?;
    let inputs: BTreeSet<_> = inputs.iter().cloned().collect();
    for reference in reviews {
        let record = typed(db, reference, "package_review")?;
        // A review just written by an earlier operation in this accepting
        // transaction is committed atomically with the issue. For an existing
        // review require its publication, not just matching file identities.
        if !store::written_by(db, &record, issuing_call)? { publication::checked(db, &record, None)?; }
        let reviewed: Package = serde_json::from_value(record.body["subject"].clone())?;
        if reviewed.members != selected
            || reviewed.inputs != inputs
            || reviewed.mandate.as_deref() != Some(mandate)
        {
            return Err(failure(format!("{reference} assessed different files, declared inputs or a different brief. Use result.reviews to inspect the differences. Historical context belongs in evidence; a current review must assess this package and brief.")));
        }
    }
    Ok(())
}
