use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Person,
    Activity,
    Request,
    Delivery,
    File,
}

impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Person,
        Self::Activity,
        Self::Request,
        Self::Delivery,
        Self::File,
    ];
    pub fn filter(self) -> &'static str {
        match self {
            Self::Person => "Personas",
            Self::Activity => "Activities",
            Self::Request => "Requests",
            Self::Delivery => "Deliveries",
            Self::File => "Files",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            Self::Person => Icon::People,
            Self::Activity => Icon::Activity,
            Self::Request => Icon::Questions,
            Self::Delivery => Icon::Folder,
            Self::File => Icon::Files,
        }
    }
}

pub(super) struct Entry {
    pub kind: Kind,
    pub reference: String,
    pub work: String,
    pub title: String,
    pub subtitle: String,
    title_folded: String,
    search: String,
}

impl Entry {
    fn new(
        kind: Kind,
        reference: &str,
        work: &str,
        title: &str,
        subtitle: String,
        extra: &str,
    ) -> Self {
        let title_folded = title.to_lowercase();
        let search = format!(
            "{}\n{title_folded}\n{subtitle}\n{reference}\n{work}\n{extra}",
            kind.filter()
        )
        .to_lowercase();
        Self {
            kind,
            reference: reference.into(),
            work: work.into(),
            title: title.into(),
            subtitle,
            title_folded,
            search,
        }
    }
}

struct EarlierFile {
    path: String,
    work: String,
    recorded_by: String,
}

#[derive(Default)]
pub(super) struct Index {
    pub generation: Option<u64>,
    pub entries: Vec<Entry>,
    activity_titles: BTreeMap<String, String>,
    events_seen: usize,
    earlier: BTreeMap<String, EarlierFile>,
}

impl Index {
    pub fn refresh(&mut self, app: &App) {
        self.generation = Some(app.home_index.generation);
        self.activity_titles = app
            .home_index
            .activities
            .iter()
            .map(|a| (a.id.clone(), a.title.to_lowercase()))
            .collect();
        // Only committed and resolved file operations receive this projection.
        // Read each one once even if live calls keep refreshing the workspace.
        for e in &app.events[self.events_seen..] {
            let outcome = &e["tool_outcome"];
            if let (Some(reference), Some(path)) = (
                outcome["file_reference"].as_str(),
                outcome["file_path"].as_str(),
            ) {
                if !path.is_empty() && reference.starts_with("asset:") {
                    self.earlier
                        .entry(reference.into())
                        .or_insert_with(|| EarlierFile {
                            path: path.into(),
                            work: s(e, "work").into(),
                            recorded_by: s(e, "actor").into(),
                        });
                }
            }
        }
        self.events_seen = app.events.len();
        let files: BTreeMap<_, _> = app
            .snapshot
            .assets
            .iter()
            .map(|a| (s(a, "reference"), s(&a["body"], "path")))
            .collect();
        let file_names = |body: &Value| {
            body["artifacts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|r| files.get(r).copied().unwrap_or(r))
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.entries.clear();
        let rows = &mut self.entries;
        let requests = app.records("human_requests");
        let mut request_rows = requests
            .iter()
            .map(|r| {
                let body = &r["body"];
                let author = app.label(s(body, "requester"));
                let state = s(body, "state");
                let subtitle = format!(
                    "Human request  ·  {}  ·  From {author}",
                    inbox::phase(state).0
                );
                let extra = format!(
                    "{}\n{}\n{}",
                    s(body, "action"),
                    s(body, "return_requested"),
                    file_names(body)
                );
                let entry = Entry::new(
                    Kind::Request,
                    &inbox::handle(r),
                    s(body, "work"),
                    s(body, "title"),
                    subtitle,
                    &extra,
                );
                (
                    !matches!(state, "open" | "in_progress"),
                    std::cmp::Reverse(body["updated"].as_i64()),
                    entry,
                )
            })
            .collect::<Vec<_>>();
        request_rows.sort_by_key(|(closed, updated, _)| (*closed, *updated));
        let mut requests = request_rows.into_iter().peekable();
        while requests.peek().is_some_and(|(closed, _, _)| !closed) {
            if let Some((_, _, entry)) = requests.next() {
                rows.push(entry);
            }
        }
        for r in app.records("personas") {
            let body = &r["body"];
            let id = s(body, "id");
            let author = s(body, "seed_author");
            let subtitle = if author.is_empty() {
                "Persona".into()
            } else {
                format!("Persona  ·  Starting profile by {author}")
            };
            rows.push(Entry::new(
                Kind::Person,
                id,
                "",
                app.label(id),
                subtitle,
                "",
            ));
        }
        for &i in &app.home_index.deliveries {
            let r = &app.records("deliveries")[i];
            let body = &r["body"];
            let author = app.label(s(body, "owner"));
            let count = body["artifacts"].as_array().map_or(0, Vec::len);
            let subtitle = format!(
                "Delivery  ·  By {author}  ·  {}",
                counted(count as u64, "file", "files")
            );
            let extra = format!("{}\n{}", s(body, "supported_use"), file_names(body));
            rows.push(Entry::new(
                Kind::Delivery,
                &inbox::handle(r),
                s(body, "work"),
                s(body, "title"),
                subtitle,
                &extra,
            ));
        }
        for activity in &app.home_index.activities {
            let names = activity
                .members
                .iter()
                .map(|id| app.label(id))
                .collect::<Vec<_>>()
                .join(", ");
            let kind = if activity.kind == "social" {
                "Conversation"
            } else {
                "Activity"
            };
            let subtitle = if names.is_empty() {
                kind.into()
            } else {
                format!("{kind}  ·  {names}")
            };
            rows.push(Entry::new(
                Kind::Activity,
                &activity.id,
                &activity.id,
                &activity.title,
                subtitle,
                &activity.phase,
            ));
        }
        for asset in &app.snapshot.assets {
            let body = &asset["body"];
            let reference = s(asset, "reference");
            let state = if body["retired"] == true {
                "Retired"
            } else if asset["current"] == true {
                "Current"
            } else {
                "Earlier edition"
            };
            let mut subtitle = format!("File  ·  {}  ·  {state}", edition(reference));
            if let Some(author) = body["last_content_author"]
                .as_str()
                .filter(|a| !a.is_empty())
            {
                subtitle.push_str(&format!("  ·  By {}", app.label(author)));
            }
            let path = files[reference];
            rows.push(Entry::new(
                Kind::File,
                reference,
                s(body, "work"),
                if path.is_empty() { reference } else { path },
                subtitle,
                s(asset, "media"),
            ));
        }
        for (reference, file) in &self.earlier {
            if files.contains_key(reference.as_str()) {
                continue;
            }
            let subtitle = format!(
                "File  ·  {}  ·  Earlier edition  ·  Recorded by {}",
                edition(reference),
                app.label(&file.recorded_by)
            );
            rows.push(Entry::new(
                Kind::File,
                reference,
                &file.work,
                &file.path,
                subtitle,
                "",
            ));
        }
        rows.extend(requests.map(|(_, _, entry)| entry));
    }

    pub fn search(&self, query: &str, kind: Option<Kind>) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let tokens = query.split_whitespace().collect::<Vec<_>>();
        let exact = tokens.len() == 1
            && query.split_once(':').is_some_and(|(kind, id)| {
                matches!(kind, "asset" | "delivery" | "human_request")
                    && id
                        .rsplit_once('@')
                        .is_some_and(|(id, n)| !id.is_empty() && n.parse::<u64>().is_ok())
            });
        let mut hits: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| kind.is_none_or(|k| k == e.kind))
            .filter(|(_, e)| {
                if exact {
                    e.reference.to_lowercase() == query
                } else {
                    let activity = self.activity_titles.get(&e.work);
                    tokens
                        .iter()
                        .all(|t| e.search.contains(t) || activity.is_some_and(|a| a.contains(t)))
                }
            })
            .map(|(i, _)| i)
            .collect();
        if !tokens.is_empty() && !exact {
            hits.sort_by_key(|&i| {
                let e = &self.entries[i];
                if e.title_folded == query {
                    0
                } else if e.title_folded.starts_with(&query) {
                    1
                } else if tokens.iter().all(|t| e.title_folded.contains(t)) {
                    2
                } else {
                    3
                }
            });
        }
        hits
    }
}

fn edition(reference: &str) -> String {
    reference.rsplit_once('@').map_or_else(
        || "Edition unavailable".into(),
        |(_, n)| format!("Revision {n}"),
    )
}
