//! The paper trail travels beside the issued files, not inside them. A single
//! recipient's slip gets a notice, not the full reports or another slip's terms.
use super::sheet::{append_text, field, listed_values};
use crate::neural::package_review::{after_issue::Entry, Verdict};

#[derive(Default)]
pub(super) struct Notice {
    pub concern: bool,
    pub findings: usize,
    pub editions: usize,
    pub unavailable: usize,
}

impl Notice {
    pub(super) fn from_entries(entries: &[Entry]) -> Self {
        let mut note = Self::default();
        for entry in entries {
            match entry {
                Entry::Finding { verdict, .. } => {
                    note.findings += 1;
                    note.concern |= *verdict == Verdict::Concern;
                }
                Entry::Citation { .. } => note.editions += 1,
                Entry::Unavailable { .. } => note.unavailable += 1,
            }
        }
        note
    }

    pub(super) fn render(&self, html: &mut String, full: bool) {
        if self.findings + self.editions + self.unavailable == 0 {
            return;
        }
        html.push_str(if self.concern {
            "<aside class=\"revision-note concern\">"
        } else {
            "<aside class=\"revision-note\">"
        });
        html.push_str("<h2>Edition notes at export</h2>");
        if self.concern {
            html.push_str("<p><strong>A reviewer raised a concern about this edition after it was issued.</strong>");
            if full {
                html.push_str(" <a href=\"#later-concern\">Read the finding</a>.");
            }
            html.push_str("</p>");
        }
        if full {
            html.push_str("<p>This sheet includes the records listed below. The files are still the ones originally issued.</p><ul>");
            if self.findings > 0 {
                html.push_str(&format!(
                    "<li><a href=\"#review-notes\">Package findings</a> · {}</li>",
                    self.findings
                ));
            }
            if self.editions > 0 {
                html.push_str(&format!(
                    "<li><a href=\"#later-editions\">Later editions citing this one</a> · {}</li>",
                    self.editions
                ));
            }
            if self.unavailable > 0 {
                html.push_str(&format!("<li><a href=\"#unavailable-notes\">Records that could not be verified</a> · {}</li>", self.unavailable));
            }
            html.push_str("</ul>");
        } else {
            if self.findings > 0 && !self.concern {
                html.push_str("<p>A later finding was published on this edition.</p>");
            }
            if self.editions > 0 {
                html.push_str("<p>A later edition cites this one.</p>");
            }
            if self.unavailable > 0 {
                html.push_str("<p>A later record could not be verified.</p>");
            }
            html.push_str("<p>Ask for the full delivery handoff to read the edition notes. This packet contains only your original slip and its files.</p>");
        }
        html.push_str("</aside>");
    }
}

pub(super) fn citations(html: &mut String, entries: &[Entry]) {
    let mut editions = entries
        .iter()
        .filter(|e| matches!(e, Entry::Citation { .. }))
        .peekable();
    if editions.peek().is_none() {
        return;
    }
    html.push_str("<section id=\"later-editions\"><h2>Later editions citing this one</h2><p>A citation is not an endorsement or a replacement instruction. Read what the later issuer says. To get that edition, paste its revision into Find in the source Studio workspace. Its files are not in this handoff.</p><ul class=\"entries\">");
    for entry in editions {
        let Entry::Citation {
            reference,
            issuer,
            issuer_label,
            title,
            in_evidence,
            in_review_inputs,
            terms,
            ..
        } = entry
        else {
            continue;
        };
        html.push_str("<li><h3 class=\"delivery-title\" dir=\"auto\">");
        append_text(html, title);
        html.push_str("</h3><dl class=\"item-facts\">");
        field(
            html,
            "Later issuer",
            issuer_label.as_deref().unwrap_or(issuer),
        );
        field(
            html,
            "Cited this edition as",
            match (in_evidence, in_review_inputs) {
                (true, true) => "Evidence and a governing input",
                (true, _) => "Evidence",
                _ => "A governing input",
            },
        );
        field(
            html,
            "Declared scope",
            match terms.scope {
                crate::neural::tools::DeliveryScope::Partial => "Partial",
                crate::neural::tools::DeliveryScope::ClaimedComplete => "Author claims complete",
            },
        );
        field(html, "Later issuer’s supported use", &terms.supported_use);
        html.push_str("<div><dt>Later issuer’s remaining work</dt><dd>");
        listed_values(html, &terms.remaining, "None listed");
        html.push_str("</dd></div></dl><dl class=\"identifiers\">");
        field(html, "Edition to find", reference);
        html.push_str("</dl></li>");
    }
    html.push_str("</ul></section>");
}
