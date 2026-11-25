//! A portable, printable view of the exact request revision and its attachments.
use super::{RequestSheet, SheetDelivery, SheetFile};
use crate::neural::human_requests::field_report;
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

const RETURN_SCRIPT: &str = include_str!("field_report.js");

/// All workspace strings are text, including filenames, references and quoted markup.
pub(super) fn append_text(html: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            _ => html.push(character),
        }
    }
}

pub(super) fn field(html: &mut String, label: &'static str, value: &str) {
    html.push_str("<div><dt>");
    html.push_str(label);
    html.push_str("</dt><dd dir=\"auto\">");
    append_text(html, value);
    html.push_str("</dd></div>");
}

pub(super) fn prose_section(html: &mut String, heading: &'static str, value: &str) {
    html.push_str("<section><h2>");
    html.push_str(heading);
    html.push_str("</h2><p class=\"verbatim\" dir=\"auto\">");
    append_text(html, value);
    html.push_str("</p></section>");
}

fn byte_count(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut count = String::new();
    for (index, digit) in digits.bytes().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            count.push(',');
        }
        count.push(char::from(digit));
    }
    count.push_str(if bytes == 1 { " byte" } else { " bytes" });
    count
}

pub(super) fn listed_values(html: &mut String, list: &[String], empty: &'static str) {
    match list {
        [] => html.push_str(empty),
        [only] => {
            html.push_str("<div class=\"verbatim\" dir=\"auto\">");
            append_text(html, only);
            html.push_str("</div>");
        }
        values => {
            html.push_str("<ul class=\"notes\">");
            for text in values {
                html.push_str("<li class=\"verbatim\" dir=\"auto\">");
                append_text(html, text);
                html.push_str("</li>");
            }
            html.push_str("</ul>");
        }
    }
}

pub(super) fn dispatch_slips(html: &mut String, package: &SheetDelivery, files: &[SheetFile]) {
    if package.dispatches.is_empty() { return; }
    html.push_str("<section><h2>Dispatch slips</h2><p>The issuer selected these files for each recipient. No transfer or acceptance is recorded by this sheet.</p><ol class=\"entries\">");
    for slip in &package.dispatches {
        html.push_str("<li><h3 class=\"delivery-title\" dir=\"auto\">");
        append_text(html, &slip.recipient);
        html.push_str("</h3><p class=\"verbatim\" dir=\"auto\">");
        append_text(html, &slip.instructions);
        html.push_str("</p><ul class=\"references\">");
        for reference in &slip.artifacts {
            html.push_str("<li dir=\"auto\">");
            append_text(html, files.iter().find(|f| &f.reference == reference).map_or(reference, |f| &f.path));
            html.push_str("</li>");
        }
        html.push_str("</ul></li>");
    }
    html.push_str("</ol></section>");
}

fn delivery(html: &mut String, package: &SheetDelivery, files: &[SheetFile]) {
    html.push_str("<li><h3 class=\"delivery-title\" dir=\"auto\">");
    append_text(html, &package.title);
    html.push_str("</h3><dl class=\"item-facts\">");
    field(html, "Delivery owner", &package.owner);
    field(
        html,
        "Declared scope",
        crate::neural::human_requests::declared_scope(&package.scope),
    );
    field(html, "Supported use", &package.supported_use);
    html.push_str("<div><dt>Remaining work</dt><dd>");
    listed_values(html, &package.remaining, "None listed");
    html.push_str("</dd></div></dl><dl class=\"identifiers delivery-id\">");
    field(html, "Delivery revision", &package.reference);
    html.push_str("</dl>");
    dispatch_slips(html, package, files);
    if !package.artifacts.is_empty() {
        html.push_str("<details class=\"membership\"><summary>Artifact revisions in this delivery</summary><ul class=\"references\">");
        for reference in &package.artifacts {
            html.push_str("<li dir=\"auto\">");
            append_text(html, reference);
            html.push_str("</li>");
        }
        html.push_str("</ul></details>");
    }
    html.push_str("</li>");
}

fn attachment(html: &mut String, file: &SheetFile, linked: bool) {
    html.push_str("<li><div class=\"file-heading\"><h3 class=\"filename\" dir=\"auto\">");
    if linked {
        // URL escaping is distinct from HTML escaping. A literal #, %, ? or
        // non-ASCII name must open the same member the issuer selected.
        use std::fmt::Write;
        html.push_str("<a href=\"Files/");
        for b in file.path.bytes() {
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { html.push(char::from(b)); }
            else { write!(html, "%{b:02X}").expect("write to string"); }
        }
        html.push_str("\">");
    }
    append_text(html, &file.path);
    if linked { html.push_str("</a>"); }
    html.push_str("</h3><span class=\"size\">");
    append_text(html, &byte_count(file.bytes));
    html.push_str("</span></div><dl class=\"item-facts\">");
    field(
        html,
        "Content author",
        file.author.as_deref().unwrap_or("Unrecorded"),
    );
    html.push_str("<div><dt>Qualifications</dt><dd>");
    listed_values(html, &file.qualifications, "None recorded");
    html.push_str("</dd></div></dl><details><summary>Revision and checksum</summary><dl class=\"identifiers\">");
    field(html, "File revision", &file.reference);
    field(html, "SHA-256", &file.sha256);
    html.push_str("</dl></details></li>");
}

pub(super) fn header(title: &str, kind: &'static str) -> String {
    let mut html = String::from(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>"#,
    );
    append_text(&mut html, title);
    html.push_str(" · Persona Studio</title>");
    if kind == "Human request" {
        let hash = STANDARD.encode(Sha256::digest(RETURN_SCRIPT.as_bytes()));
        html.push_str(r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; base-uri 'none'; form-action 'none'; style-src 'unsafe-inline'; script-src 'sha256-"#);
        html.push_str(&hash);
        html.push_str(r#"'">"#);
    }
    html.push_str("<style>");
    html.push_str(include_str!("sheet.css"));
    if kind == "Human request" {
        html.push_str(include_str!("field_report.css"));
    }
    html.push_str(
        r#"</style></head><body><main class="sheet">
<header><div class="masthead"><span class="brand">Persona Studio</span><span>"#,
    );
    html.push_str(kind);
    html.push_str(
        r#"</span></div>
<h1 dir="auto">"#,
    );
    append_text(&mut html, title);
    html.push_str("</h1><dl class=\"request-facts\">");
    html
}

pub(super) fn file_section(html: &mut String, heading: &'static str, files: &[SheetFile]) {
    html.push_str("<section><div class=\"attachments-heading\"><h2>");
    html.push_str(heading);
    if files.is_empty() {
        html.push_str("</h2></div><p class=\"empty\">None attached.</p>");
    } else {
        html.push_str(" <span class=\"count\">(");
        append_text(html, &files.len().to_string());
        html.push_str(r#")</span></h2><a class="archive" href="Files.zip" download="Files.zip"><span>Download </span>Files.zip</a></div><ul class="entries">"#);
        for file in files {
            attachment(html, file, false);
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
}

pub(super) fn recipient(
    package: &SheetDelivery, issuer: &str, number: usize, slip: &crate::neural::tools::Dispatch,
    reference: &str, files: &[SheetFile], notice: &super::edition_notes::Notice,
) -> String {
    let mut html = header(&slip.recipient, "Recipient dispatch");
    field(&mut html, "From edition", &package.title);
    field(&mut html, "Issuing persona", issuer);
    field(&mut html, "Dispatch", &number.to_string());
    html.push_str("</dl></header>");
    notice.render(&mut html, false);
    prose_section(&mut html, "Your instructions", &slip.instructions);
    html.push_str("<section><h2>Your files</h2><p>Unpack this packet, then open the files below. Their names and contents are as issued.</p><ul class=\"entries\">");
    for file in files { attachment(&mut html, file, true); }
    html.push_str("</ul></section>");
    prose_section(&mut html, "Supported use at issue", &package.supported_use);
    html.push_str("<section><h2>Remaining at issue</h2>");
    listed_values(&mut html, &package.remaining, "None listed");
    html.push_str("</section><footer><dl class=\"identifiers\">");
    field(&mut html, "Delivery revision", reference);
    field(&mut html, "Declared scope", crate::neural::human_requests::declared_scope(&package.scope));
    html.push_str("</dl></footer></main></body></html>");
    html
}

pub(super) fn render(request: &RequestSheet<'_>) -> String {
    let mut html = header(request.title, "Human request");
    field(&mut html, "Requested by", request.requester);
    field(&mut html, "Recorded status", request.state);
    if let Some(person) = request.claimed_by {
        field(
            &mut html,
            if request.state == "In progress" {
                "Taken by"
            } else {
                "Reported by"
            },
            person,
        );
    }
    html.push_str("</dl></header>");
    if let Some((reference, status)) = request.current {
        html.push_str("<aside class=\"revision-note\"><h2>Newer revision at export</h2><dl>");
        field(&mut html, "Latest status", status);
        field(&mut html, "Latest revision", reference);
        html.push_str("</dl></aside>");
        if request.newer_packet {
            html.push_str("<p><strong>A newer field packet has been issued.</strong> This sheet and its files belong to the earlier edition. Open the current request in Studio for the replacement.</p>");
        }
    }
    if let Some(packet) = request.packet {
        prose_section(&mut html, "Field packet · issuer’s instructions", packet["instructions"].as_str().unwrap_or(""));
    }
    prose_section(&mut html, "Requested action", request.action);
    prose_section(
        &mut html,
        "What to return",
        request
            .return_requested
            .filter(|text| !text.trim().is_empty())
            .unwrap_or("Unspecified by the requester."),
    );
    if request.return_digest.is_some() {
        html.push_str(
            r##"<a class="report-jump" href="#field-report">Fill out the field report</a>"##,
        );
    }
    if !request.deliveries.is_empty() {
        html.push_str("<section><h2>Included deliveries</h2><ul class=\"entries\">");
        for package in &request.deliveries {
            delivery(&mut html, package, request.files);
        }
        html.push_str("</ul></section>");
    } else if !request.files.is_empty() {
        html.push_str("<p>The requester attached the individual file editions listed below. Their qualifications accompany each file.</p>");
    }
    file_section(&mut html, "Attached files", request.files);
    if let Some(digest) = request.return_digest {
        html.push_str(r#"<section class="field-return" id="field-report" data-request=""#);
        append_text(&mut html, request.reference);
        html.push_str(r#"" data-digest=""#);
        append_text(&mut html, digest);
        html.push_str(r#"" data-format=""#);
        html.push_str(field_report::FORMAT);
        html.push_str("\">");
        html.push_str(include_str!("field_report.html"));
        html.push_str("</section><script>");
        html.push_str(RETURN_SCRIPT);
        html.push_str("</script>");
    }
    html.push_str("<footer><dl class=\"identifiers\">");
    field(&mut html, "Request revision", request.reference);
    field(&mut html, "Activity", request.activity);
    match request.origin {
        Some(origin) => {
            if let Some(decision) = origin["decision"].as_str() {
                field(&mut html, "Requesting decision", decision);
            }
            if let Some(event) = origin["experience"].as_str() {
                field(&mut html, "Original request", event);
            }
        }
        None => field(&mut html, "Original request", "The recorded publication could not be verified. A digital field report is unavailable for this edition."),
    }
    html.push_str("</dl></footer></main></body></html>\n");
    html
}
