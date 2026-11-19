//! A portable, printable view of the exact request revision and its attachments.
use super::{RequestSheet, SheetDelivery, SheetFile};

/// All workspace strings are text, including filenames, references and quoted markup.
fn append_text(html: &mut String, text: &str) {
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

fn field(html: &mut String, label: &'static str, value: &str) {
    html.push_str("<div><dt>");
    html.push_str(label);
    html.push_str("</dt><dd dir=\"auto\">");
    append_text(html, value);
    html.push_str("</dd></div>");
}

fn prose_section(html: &mut String, heading: &'static str, value: &str) {
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

fn listed_values(html: &mut String, list: &[String], empty: &'static str) {
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

fn delivery(html: &mut String, package: &SheetDelivery) {
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

fn attachment(html: &mut String, file: &SheetFile) {
    html.push_str("<li><div class=\"file-heading\"><h3 class=\"filename\" dir=\"auto\">");
    append_text(html, &file.path);
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

pub(super) fn render(request: &RequestSheet<'_>) -> String {
    let mut html = String::from(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>"#,
    );
    append_text(&mut html, request.title);
    html.push_str(" · Persona Studio</title><style>");
    html.push_str(include_str!("sheet.css"));
    html.push_str(r#"</style></head><body><main class="sheet">
<header><div class="masthead"><span class="brand">Persona Studio</span><span>Human request</span></div>
<h1 dir="auto">"#);
    append_text(&mut html, request.title);
    html.push_str("</h1><dl class=\"request-facts\">");
    field(&mut html, "Requested by", request.requester);
    field(&mut html, "Recorded status", request.state);
    if let Some(person) = request.claimed_by {
        field(&mut html, "Taken by", person);
    }
    html.push_str("</dl></header>");
    if let Some((reference, status)) = request.current {
        html.push_str("<aside class=\"revision-note\"><h2>Newer revision at export</h2><dl>");
        field(&mut html, "Latest status", status);
        field(&mut html, "Latest revision", reference);
        html.push_str("</dl></aside>");
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
    if !request.deliveries.is_empty() {
        html.push_str("<section><h2>Included deliveries</h2><ul class=\"entries\">");
        for package in &request.deliveries {
            delivery(&mut html, package);
        }
        html.push_str("</ul></section>");
    }
    html.push_str("<section><div class=\"attachments-heading\"><h2>Attached files");
    if request.files.is_empty() {
        html.push_str("</h2></div><p class=\"empty\">None attached.</p>");
    } else {
        html.push_str(" <span class=\"count\">(");
        append_text(&mut html, &request.files.len().to_string());
        html.push_str(r#")</span></h2><a class="archive" href="Files.zip" download="Files.zip"><span>Download </span>Files.zip</a></div><ul class="entries">"#);
        for file in &request.files {
            attachment(&mut html, file);
        }
        html.push_str("</ul>");
    }
    html.push_str("</section><footer><dl class=\"identifiers\">");
    field(&mut html, "Request revision", request.reference);
    field(&mut html, "Activity", request.activity);
    html.push_str("</dl></footer></main></body></html>\n");
    html
}
