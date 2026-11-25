//! Recipient-facing facts for one issued edition. References in a release or
//! review identify its basis. Only the edition's own members travel as files.
use super::sheet::{append_text, field, file_section, header, listed_values, prose_section};
use super::{SheetDelivery, SheetFile};
use crate::neural::{
    activity_sources::delivery::Edition, delivery_feedback::Issued, failure, human_requests,
    package_review, store, Result,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct Release {
    #[serde(flatten)]
    package: SheetDelivery,
    work: String,
    mandate: Option<String>,
    evidence: Vec<String>,
    #[serde(default)]
    review_inputs: Vec<String>,
    #[serde(default)]
    reviews: Vec<String>,
}

#[derive(Deserialize)]
struct Brief {
    original_need: String,
    amendments: Vec<Amendment>,
}

#[derive(Deserialize)]
struct Amendment {
    author: String,
    text: String,
    time: i64,
}

#[derive(Deserialize)]
struct Finding {
    review: Review,
    same_files: bool,
    same_inputs: bool,
    same_mandate: Option<bool>,
    available_at_delivery: bool,
    author_label_at_review: Option<String>,
    cited_as_evidence: bool,
    listed_in_reviews: bool,
    created: i64,
}

#[derive(Deserialize)]
struct Review {
    reference: String,
    author: String,
    author_kind: String,
    subject: ReviewedPackage,
    #[serde(default)]
    attachments: Vec<crate::neural::input_files::Material>,
    criterion: String,
    finding: String,
    verdict: package_review::Verdict,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct ReviewedPackage {
    delivery: Option<String>,
}

#[derive(Deserialize)]
struct Disposition {
    reference: String,
    reviewer: String,
    accepted: bool,
    reason: String,
    created: i64,
    brief_at_review: Option<String>,
}

fn utc(db: &Connection, recorded: i64) -> Result<String> {
    Ok(db.query_row(
        "SELECT strftime('%Y-%m-%d %H:%M:%S UTC',?1,'unixepoch')",
        [recorded],
        |row| row.get(0),
    )?)
}

fn brief(db: &Connection, release: &Release, issued: &Issued) -> Result<(String, Brief)> {
    let source: String = db.query_row(
        "SELECT snapshot FROM invocations WHERE id=?1",
        [&issued.invocation],
        |row| row.get(0),
    )?;
    let snapshot: Value = store::object(db, &source)?;
    let supplied = &snapshot["data"]["work"];
    let mandate = supplied["reference"]
        .as_str()
        .ok_or_else(|| failure("The issuing decision lacks its brief revision"))?;
    let record = store::exact(db, mandate)?;
    if snapshot["invocation"] != issued.invocation
        || snapshot["work"] != release.work
        || snapshot["actor"] != release.package.owner
        || record.reference.kind != "work"
        || record.reference.id != release.work
        || record.reference.handle() != mandate
        || record.body["id"] != release.work
        || record.body != supplied["record"]
        || release
            .mandate
            .as_ref()
            .is_some_and(|declared| declared != mandate)
    {
        return Err(failure(
            "The recorded delivery and its issuing brief differ",
        ));
    }
    Ok((mandate.to_owned(), serde_json::from_value(record.body)?))
}

fn references(
    html: &mut String,
    label: &'static str,
    refs: &[String],
    files: &[SheetFile],
    mandate: &str,
) {
    if refs.is_empty() {
        return;
    }
    html.push_str("<div><dt>");
    html.push_str(label);
    html.push_str("</dt><dd><ul class=\"references\">");
    for reference in refs {
        html.push_str("<li dir=\"auto\">");
        if let Some(file) = files.iter().find(|file| file.reference == *reference) {
            append_text(html, &file.path);
            html.push_str(" · ");
        } else if reference == mandate {
            html.push_str("Brief at issue · ");
        }
        append_text(html, reference);
        html.push_str("</li>");
    }
    html.push_str("</ul></dd></div>");
}

fn finding(
    html: &mut String,
    db: &Connection,
    item: &Finding,
    files: &[SheetFile],
    mandate: &str,
    edition: &str,
    first_concern: Option<&str>,
) -> Result<()> {
    let review = &item.review;
    html.push_str(if first_concern == Some(review.reference.as_str()) {
        "<li id=\"later-concern\">"
    } else {
        "<li>"
    });
    html.push_str("<h3 class=\"delivery-title\" dir=\"auto\">");
    append_text(html, &review.criterion);
    html.push_str("</h3><p class=\"verbatim\" dir=\"auto\">");
    append_text(html, &review.finding);
    html.push_str("</p><dl class=\"item-facts\">");
    field(
        html,
        if review.author_kind == "operator" {
            "Reviewing operator"
        } else {
            "Reviewing persona"
        },
        item.author_label_at_review
            .as_deref()
            .unwrap_or(&review.author),
    );
    field(html, "Published", &utc(db, item.created)?);
    field(
        html,
        "Package assessed",
        match review.subject.delivery.as_deref() {
            None => "Proposed package",
            Some(selected) if selected == edition => "This issued edition",
            Some(_) => "Another issued edition",
        },
    );
    field(
        html,
        "Finding",
        match review.verdict {
            package_review::Verdict::Supported => "Supported",
            package_review::Verdict::Concern => "Concern",
            package_review::Verdict::Inconclusive => "Inconclusive",
        },
    );
    if let Some(other) = review.subject.delivery.as_deref().filter(|r| *r != edition) {
        field(html, "Reviewed edition", other);
    }
    let mut differences = Vec::new();
    if !item.same_files {
        differences.push("Different file revisions");
    }
    if !item.same_inputs {
        differences.push("Different governing inputs");
    }
    match item.same_mandate {
        Some(false) => differences.push("Different brief"),
        None => differences.push("Brief unrecorded"),
        Some(true) => {}
    }
    field(
        html,
        "Compared with this edition",
        &if differences.is_empty() {
            "Exact files, governing inputs and brief".to_owned()
        } else {
            differences.join(" · ")
        },
    );
    if item.listed_in_reviews {
        field(
            html,
            "At issue",
            "Named by the issuer as a review of this edition",
        );
    } else if item.cited_as_evidence {
        field(html, "At issue", "Cited by the issuer as evidence");
    }
    references(
        html,
        "Reviewer's evidence",
        &review.evidence,
        files,
        mandate,
    );
    if !review.attachments.is_empty() {
        html.push_str("<div><dt>Files supplied by the reviewer</dt><dd>Listed as published; not included in this handoff. Open the finding in the source workspace to save an original.<ul class=\"references\">");
        for f in &review.attachments {
            html.push_str("<li dir=\"auto\">");
            append_text(html, &f.name);
            html.push_str(&format!(" · {} bytes · SHA-256 {}", f.bytes, f.sha256));
            html.push_str("</li>");
        }
        html.push_str("</ul></dd></div>");
    }
    field(html, "Review record", &review.reference);
    html.push_str("</dl></li>");
    Ok(())
}

impl Finding {
    fn heading(&self) -> &'static str {
        if !self.available_at_delivery {
            "Findings published after issue"
        } else if self.listed_in_reviews || self.cited_as_evidence {
            "Reviews cited at issue"
        } else {
            "Other findings available at issue"
        }
    }
}

pub(super) fn render(
    db: &Connection,
    edition: &Edition,
    files: &[SheetFile],
) -> Result<(String, Value)> {
    if files.is_empty() {
        return Err(failure("The issued delivery has no files"));
    }
    let issued = &edition.issue;
    let record = &issued.record;
    let reference = record.reference.handle();
    let release: Release = serde_json::from_value(record.body.clone())?;
    let package = &release.package;
    let (mandate, brief) = brief(db, &release, issued)?;
    let notes = package_review::edition_notes(db, edition)?;
    let mut notice = super::edition_notes::Notice::from_entries(&notes.citations);
    let mut findings: Vec<Finding> = Vec::new();
    for row in notes.reviews {
        if row.get("unavailable").is_some() {
            notice.unavailable += 1;
        } else {
            findings.push(serde_json::from_value(row)?);
        }
    }
    let first_concern = findings
        .iter()
        .find(|f| {
            !f.available_at_delivery
                && f.review.subject.delivery.as_ref() == Some(&reference)
                && f.review.verdict == package_review::Verdict::Concern
        })
        .map(|f| f.review.reference.as_str());
    notice.findings = findings.len();
    notice.concern = first_concern.is_some();
    let history = store::operator_review_history(db, &BTreeSet::from([reference.clone()]))?
        .remove(&reference)
        .unwrap_or_default();
    let dispositions = history
        .into_iter()
        .map(serde_json::from_value)
        .collect::<serde_json::Result<Vec<Disposition>>>()?;
    let mut html = header(&package.title, "Issued delivery");
    field(
        &mut html,
        "Delivery owner",
        edition.issuer_label.as_deref().unwrap_or(&package.owner),
    );
    field(&mut html, "Issued", &utc(db, issued.time)?);
    field(
        &mut html,
        "Declared scope",
        human_requests::declared_scope(&package.scope),
    );
    field(
        &mut html,
        "Operator disposition at export",
        match dispositions.last() {
            Some(d) if d.accepted => "Accepted",
            Some(_) => "Rejected",
            None => "None recorded for this edition",
        },
    );
    html.push_str("</dl></header>");
    notice.render(&mut html, true);
    prose_section(
        &mut html,
        "Supported use declared by the issuer",
        &package.supported_use,
    );
    html.push_str("<section><h2>Remaining work declared by the issuer</h2>");
    listed_values(&mut html, &package.remaining, "None listed");
    html.push_str("</section>");
    super::sheet::dispatch_slips(&mut html, package, files);
    file_section(&mut html, "Package files", files);
    html.push_str("<section><h2>Activity brief at issue</h2><p class=\"verbatim\" dir=\"auto\">");
    append_text(&mut html, &brief.original_need);
    html.push_str("</p>");
    if !brief.amendments.is_empty() {
        html.push_str("<h3 class=\"delivery-title\">Amendments in force</h3><ol class=\"notes\">");
        for amendment in &brief.amendments {
            html.push_str("<li><p class=\"verbatim\" dir=\"auto\">");
            append_text(&mut html, &amendment.text);
            html.push_str("</p><small>");
            append_text(&mut html, &amendment.author);
            html.push_str(" · ");
            append_text(&mut html, &utc(db, amendment.time)?);
            html.push_str("</small></li>");
        }
        html.push_str("</ol>");
    }
    html.push_str("<dl class=\"item-facts\">");
    field(&mut html, "Brief revision", &mandate);
    html.push_str("</dl></section>");
    if !release.review_inputs.is_empty()
        || !release.evidence.is_empty()
        || !release.reviews.is_empty()
    {
        html.push_str("<section><h2>Basis declared at issue</h2><dl class=\"item-facts\">");
        references(
            &mut html,
            "Governing inputs",
            &release.review_inputs,
            files,
            &mandate,
        );
        references(
            &mut html,
            "Evidence cited by the issuer",
            &release.evidence,
            files,
            &mandate,
        );
        references(
            &mut html,
            "Reviews named by the issuer",
            &release.reviews,
            files,
            &mandate,
        );
        html.push_str("</dl></section>");
    }
    let mut first_group = true;
    for heading in [
        "Reviews cited at issue",
        "Other findings available at issue",
        "Findings published after issue",
    ] {
        let mut group = findings
            .iter()
            .filter(|f| f.heading() == heading)
            .peekable();
        if group.peek().is_none() {
            continue;
        }
        html.push_str(if first_group {
            "<section id=\"review-notes\"><h2>"
        } else {
            "<section><h2>"
        });
        first_group = false;
        html.push_str(heading);
        html.push_str("</h2><ul class=\"entries\">");
        for item in group {
            finding(
                &mut html,
                db,
                item,
                files,
                &mandate,
                &reference,
                first_concern,
            )?;
        }
        html.push_str("</ul></section>");
    }
    super::edition_notes::citations(&mut html, &notes.citations);
    if notice.unavailable > 0 {
        html.push_str("<section id=\"unavailable-notes\"><h2>Some edition notes are unavailable</h2><p>The workspace could not verify the publication or its relationship to this edition. No unverified finding or later issuer’s instructions are included. Check the source workspace for details.</p></section>");
    }
    if !dispositions.is_empty() {
        html.push_str("<section><h2>Operator disposition history</h2><ol class=\"entries\">");
        for disposition in &dispositions {
            html.push_str("<li><h3 class=\"delivery-title\">");
            html.push_str(if disposition.accepted {
                "Accepted"
            } else {
                "Rejected"
            });
            html.push_str("</h3><p class=\"verbatim\" dir=\"auto\">");
            append_text(&mut html, &disposition.reason);
            html.push_str("</p><dl class=\"item-facts\">");
            field(&mut html, "Operator", &disposition.reviewer);
            field(&mut html, "Recorded", &utc(db, disposition.created)?);
            field(
                &mut html,
                "Brief at review",
                disposition
                    .brief_at_review
                    .as_deref()
                    .unwrap_or("Unrecorded"),
            );
            if disposition
                .brief_at_review
                .as_ref()
                .is_some_and(|r| r != &mandate)
            {
                field(&mut html, "Brief comparison", "Changed since issue");
            }
            field(&mut html, "Disposition record", &disposition.reference);
            html.push_str("</dl></li>");
        }
        html.push_str("</ol></section>");
    }
    html.push_str("<footer><dl class=\"identifiers\">");
    field(&mut html, "Delivery revision", &reference);
    field(&mut html, "Issuing activity", &release.work);
    html.push_str("</dl></footer></main></body></html>\n");
    Ok((html, json!({"delivery":reference,"issued_under":mandate})))
}
