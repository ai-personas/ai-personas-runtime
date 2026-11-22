use super::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Identity {
    Persona(String),
    Operator(String),
    Inbox,
    Shared,
}

impl Identity {
    fn name<'a>(&'a self, personas: &BTreeMap<&str, &'a str>) -> &'a str {
        match self {
            Self::Persona(id) => personas.get(id.as_str()).copied().unwrap_or(id),
            Self::Operator(name) => name,
            Self::Inbox => "Human inbox",
            Self::Shared => "Shared activity",
        }
    }

    pub(super) fn role(&self) -> &'static str {
        match self {
            Self::Persona(_) => "Persona",
            Self::Operator(_) => "Human",
            Self::Inbox => "Requester address",
            Self::Shared => "Broadcast",
        }
    }

    pub(super) fn color(&self, ui: &Ui) -> Color32 {
        let p = theme::palette(ui);
        match self {
            Self::Persona(id) => p.ink(persona_color(id)),
            Self::Operator(_) | Self::Inbox => p.ink(theme::ACCENT),
            Self::Shared => p.muted,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Entry {
    pub event: usize,
    pub from: usize,
    pub to: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Scope {
    pub from: Option<usize>,
    pub to: Option<usize>,
}

#[derive(Default)]
pub(super) struct History {
    pub nodes: Vec<Identity>,
    names: Vec<String>,
    ids: BTreeMap<Identity, usize>,
    pub entries: Vec<Entry>,
    pub sent: BTreeMap<usize, Vec<usize>>,
    pub received: BTreeMap<usize, Vec<usize>>,
    pub routes: BTreeMap<(usize, usize), Vec<usize>>,
    works: BTreeSet<String>,
    pub work_starts: Vec<usize>,
    // Addressed, shared and individual contributions, in commit order.
    pub channels: [Vec<usize>; 3],
}

pub(super) fn through(list: &[usize], end: usize) -> &[usize] {
    &list[..list.partition_point(|index| *index < end)]
}

impl History {
    fn node(&mut self, identity: Identity) -> usize {
        *self.ids.entry(identity).or_insert_with_key(|key| {
            self.nodes.push(key.clone());
            self.nodes.len() - 1
        })
    }

    pub(super) fn observe(&mut self, status: &Value, events: &[Value], start: usize) {
        let mut personas = BTreeMap::new();
        for record in status["personas"].as_array().into_iter().flatten() {
            if let Some(id) = record["body"]["id"].as_str() {
                self.node(Identity::Persona(id.into()));
                if let Some(label) = record["body"]["label"].as_str().filter(|s| !s.is_empty()) {
                    personas.insert(id, label);
                }
            }
        }
        for (offset, event) in events.iter().enumerate() {
            let Some((author, address)) = contribution(event) else {
                continue;
            };
            let channel = match address {
                Some(Identity::Shared) => 1,
                None => 2,
                _ => 0,
            };
            let from = self.node(author);
            let to = address.map(|id| self.node(id));
            let index = self.entries.len();
            let work = s(event, "work");
            if !self.works.contains(work) {
                self.works.insert(work.into());
                self.work_starts.push(index);
            }
            self.entries.push(Entry {
                event: start + offset,
                from,
                to,
            });
            self.channels[channel].push(index);
            self.sent.entry(from).or_default().push(index);
            if let Some(to) = to {
                self.received.entry(to).or_default().push(index);
                self.routes.entry((from, to)).or_default().push(index);
            }
        }
        let mut counts = BTreeMap::new();
        for node in &self.nodes {
            *counts.entry(node.name(&personas)).or_insert(0) += 1;
        }
        self.names = self
            .nodes
            .iter()
            .map(|node| {
                let name = node.name(&personas);
                if counts[name] == 1 {
                    return name.into();
                }
                match node {
                    Identity::Persona(id) => format!("{name} ({id})"),
                    _ => format!("{name} ({})", node.role()),
                }
            })
            .collect();
    }

    pub(super) fn name(&self, node: usize) -> &str {
        &self.names[node]
    }

    pub(super) fn matches(&self, scope: Scope, end: usize) -> Option<&[usize]> {
        let indices = match (scope.from, scope.to) {
            (Some(from), Some(to)) => self.routes.get(&(from, to)),
            (Some(from), None) => self.sent.get(&from),
            (None, Some(to)) => self.received.get(&to),
            (None, None) => return None,
        };
        Some(through(indices.map(Vec::as_slice).unwrap_or_default(), end))
    }

    pub(super) fn last_by(&self, from: usize, end: usize) -> Option<Entry> {
        let last = through(self.sent.get(&from)?, end).last()?;
        Some(self.entries[*last])
    }
}

// Count the original exchange once across activity boundaries. Requester messages
// use their addressed arrival. Returned replies and deliveries already have an
// addressed source in the child. A delivery's second public notice is also a copy.
fn contribution(event: &Value) -> Option<(Identity, Option<Identity>)> {
    let actor = event["actor"].as_str().filter(|id| !id.is_empty())?;
    event["work"].as_str().filter(|work| !work.is_empty())?;
    event["event"].as_i64().filter(|seq| *seq > 0)?;
    let kind = s(event, "kind");
    let target = s(event, "target");
    let body = &event["body"];
    let human = event["invocation"].is_null();
    let author = if human {
        match kind {
            "invitation"
            | "user_message"
            | "human_request_progress"
            | "user_acceptance"
            | "requirements_amended"
            | "explicit_resume" => Identity::Operator(actor.into()),
            "agreement_proposed" | "agreement_response" | "package_review" | "asset_feedback" => {
                Identity::Operator(actor.strip_prefix("operator:").unwrap_or(actor).into())
            }
            _ => return None,
        }
    } else {
        Identity::Persona(actor.into())
    };
    let address = match kind {
        "model_changed" | "appraisal_selected" | "initialization_note"
            if !human && actor == target =>
        {
            None
        }
        "tool_result" if !human && actor == target && body["bootstrap"] != true => {
            let tool = body["tool"].as_str()?;
            // These successful writes already emitted their own human-readable event.
            let has_notice = matches!(
                tool,
                "peer.respond"
                    | "peer.invite"
                    | "peer.message"
                    | "work.handoff"
                    | "work.outcome"
                    | "work.commitment"
                    | "work.leave"
                    | "work.respond"
                    | "work.propose_agreement"
                    | "work.agreement_position"
                    | "result.deliver"
                    | "result.review"
                    | "asset.feedback"
                    | "human.request"
            );
            if has_notice && event["tool_outcome"]["problem"] == false {
                return None;
            }
            None
        }
        "delivery" if target == "*" => return None,
        "asset_feedback" => {
            let recipient = body["recipient"].as_str()?;
            Some(match body["recipient_kind"].as_str()? {
                "persona" => Identity::Persona(recipient.into()),
                "operator" => Identity::Operator(recipient.into()),
                _ => return None,
            })
        }
        "message"
        | "reply"
        | "user_message"
        | "requester_message"
        | "requester_review"
        | "invitation"
        | "invitation_response"
        | "contribution_proposal"
        | "human_request"
        | "human_request_withdrawn"
        | "human_request_progress"
        | "delivery"
        | "user_acceptance"
        | "package_review"
        | "requirements_amended"
        | "handoff_offered"
        | "handoff_resolved"
        | "outcome_changed"
        | "commitment_changed"
        | "agreement_proposed"
        | "agreement_response"
        | "participation_left"
        | "responsibility_accepted"
        | "request_joined"
        | "request_declined"
        | "request_left"
        | "explicit_resume" => Some(match target {
            "" => return None,
            "*" => Identity::Shared,
            "user" if !human => Identity::Inbox,
            id => Identity::Persona(id.into()),
        }),
        _ => return None,
    };
    Some((author, address))
}
