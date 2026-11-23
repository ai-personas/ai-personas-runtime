//! Internal links in a message identify immutable editions, never local filenames.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Package,
}

pub(crate) fn target(url: &str) -> Option<(Kind, &str, i64)> {
    let (scheme, rest) = url.split_once(':')?;
    let kind = match scheme {
        "asset" => Kind::File,
        "delivery" => Kind::Package,
        _ => return None,
    };
    let (id, rev) = rest.split_once('@')?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        || !matches!(rev.as_bytes().first(), Some(b'1'..=b'9'))
        || !rev.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some((kind, id, rev.parse().ok()?))
}

pub(crate) struct Link {
    pub reference: String,
    pub label: String,
    pub kind: Kind,
    pub revision: i64,
}

pub(crate) fn collect(text: &str) -> Vec<Link> {
    let mut seen = BTreeSet::new();
    let mut selected = None;
    let mut links = Vec::new();
    for event in Parser::new_ext(
        text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                selected = target(&dest_url).map(|(kind, _, revision)| Link {
                    reference: dest_url.to_string(),
                    label: String::new(),
                    kind,
                    revision,
                });
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(link) = &mut selected {
                    link.label.push_str(&t);
                }
            }
            Event::HardBreak | Event::SoftBreak => {
                if let Some(link) = &mut selected {
                    link.label.push(' ');
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = selected.take() {
                    if seen.insert(link.reference.clone()) {
                        links.push(link);
                    }
                }
            }
            _ => {}
        }
    }
    links
}
