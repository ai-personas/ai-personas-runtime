//! Attribute correspondence to the named peer or operator who actually took part.
use super::{Contact, Kind};
use crate::neural::{experience, failure, store, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;

const KINDS: &str = "'invitation','invitation_response','message','reply','user_message','contribution_proposal',
    'handoff_offered','handoff_resolved','asset_feedback','user_acceptance','requirements_amended',
    'commitment_changed','delivery','outcome_changed','package_review','participation_left','responsibility_accepted'";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Direction {
    Sent,
    Received,
    SharedInActivity,
}

#[derive(Serialize)]
pub(super) struct Encounter {
    pub reference: String,
    pub counterpart: Contact,
    pub direction: Direction,
    pub sequence: i64,
    pub work: String,
    pub kind: String,
    pub created: i64,
    pub created_utc: String,
    actor: String,
    target: String,
    /// When a reply, review or answer has a specific parent, retain its actual address.
    linked_to: Option<String>,
}

struct Event {
    sequence: i64,
    work: String,
    actor: String,
    target: String,
    kind: String,
    digest: String,
    invocation: Option<String>,
    created: i64,
    created_utc: String,
}

impl Event {
    fn row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            sequence: row.get(0)?,
            work: row.get(1)?,
            actor: row.get(2)?,
            target: row.get(3)?,
            kind: row.get(4)?,
            digest: row.get(5)?,
            invocation: row.get(6)?,
            created: row.get(7)?,
            created_utc: row.get(8)?,
        })
    }

    fn sender(&self) -> Contact {
        if self.invocation.is_some() {
            Contact::persona(&self.actor)
        } else {
            let name = if matches!(self.kind.as_str(), "asset_feedback" | "package_review") {
                self.actor.strip_prefix("operator:").unwrap_or(&self.actor)
            } else {
                &self.actor
            };
            Contact::operator(name)
        }
    }

    fn classify(self, db: &Connection, owner: &Contact) -> Result<Option<Encounter>> {
        if self.invocation.is_none()
            && !matches!(
                self.kind.as_str(),
                "invitation"
                    | "user_message"
                    | "user_acceptance"
                    | "requirements_amended"
                    | "package_review"
                    | "asset_feedback"
            )
        {
            return Ok(None);
        }
        let from = self.sender();
        let mut parent = None;
        // A public contribution is visible to the group. Only an explicit
        // addressee or an identified source exchange establishes a personal link.
        let to = match self.kind.as_str() {
            "asset_feedback" => {
                let body: Value = store::object(db, &self.digest)?;
                let role = serde_json::from_value(body["recipient_kind"].clone())?;
                let name = body["recipient"]
                    .as_str()
                    .ok_or_else(|| failure("Feedback lacks its recipient"))?;
                parent = body["reply_to"].as_str().map(str::to_owned);
                Some(Contact {
                    kind: role,
                    id: name.to_owned(),
                })
            }
            "invitation_response" => {
                let invite: Option<(i64, String, Option<String>)> = db
                    .query_row(
                        "SELECT seq,actor,invocation FROM events WHERE work=?1 AND target=?2
                     AND kind='invitation' AND seq<?3 ORDER BY seq DESC LIMIT 1",
                        params![self.work, self.actor, self.sequence],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?;
                invite.map(|(seq, actor, call)| {
                    parent = Some(format!("experience:event-{seq}@1"));
                    if call.is_some() {
                        Contact::persona(&actor)
                    } else {
                        Contact::operator(&actor)
                    }
                })
            }
            "handoff_resolved" => {
                let body: Value = store::object(db, &self.digest)?;
                parent = body["prior_offer"].as_str().map(str::to_owned);
                let other = if body["from"] == self.actor {
                    &body["to"]
                } else {
                    &body["from"]
                };
                other.as_str().map(Contact::persona)
            }
            "user_acceptance" => {
                let body: Value = store::object(db, &self.digest)?;
                if let Some(delivery) = body["decision"]["delivery"].as_str() {
                    let issued = store::exact(db, delivery)?;
                    parent = Some(delivery.to_owned());
                    issued.body["owner"].as_str().map(Contact::persona)
                } else {
                    None
                }
            }
            "message" | "reply" if self.target == "user" => {
                let body: Value = store::object(db, &self.digest)?;
                parent = body["reply_to"].as_str().map(str::to_owned);
                if let Some(address) = &parent {
                    let source = experience::metadata(db, Some(&self.actor), address)?;
                    let human = source["invocation"].is_null()
                        && matches!(
                            source["kind"].as_str(),
                            Some(
                                "user_message"
                                    | "user_acceptance"
                                    | "requirements_amended"
                                    | "invitation"
                            )
                        );
                    source["actor"]
                        .as_str()
                        .filter(|_| human)
                        .map(Contact::operator)
                } else {
                    None
                }
            }
            "message" | "user_message" => {
                let body: Value = store::object(db, &self.digest)?;
                parent = body["reply_to"].as_str().map(str::to_owned);
                Some(Contact::persona(&self.target))
            }
            "invitation" | "handoff_offered" | "contribution_proposal" => {
                Some(Contact::persona(&self.target))
            }
            _ => None,
        };
        let (who, direction) = if &from == owner {
            let Some(recipient) = to.filter(|c| c != owner) else {
                return Ok(None);
            };
            (recipient, Direction::Sent)
        } else if to.as_ref() == Some(owner) {
            (from, Direction::Received)
        } else if self.target == "*" {
            (from, Direction::SharedInActivity)
        } else {
            return Ok(None);
        };
        Ok(Some(Encounter {
            reference: format!("experience:event-{}@1", self.sequence),
            counterpart: who,
            direction,
            sequence: self.sequence,
            work: self.work,
            kind: self.kind,
            created: self.created,
            created_utc: self.created_utc,
            actor: self.actor,
            target: self.target,
            linked_to: parent,
        }))
    }
}

enum Filter<'a> {
    All,
    Event(i64),
    Decision(&'a str),
}

fn visit(
    db: &Connection,
    owner: &str,
    filter: Filter<'_>,
    mut receive: impl FnMut(Encounter) -> Result<()>,
) -> Result<()> {
    use rusqlite::types::Value as SqlValue;
    let (condition, value) = match filter {
        Filter::All => ("?2 IS NULL", SqlValue::Null),
        Filter::Event(sequence) => ("e.seq=?2", SqlValue::Integer(sequence)),
        Filter::Decision(call) => ("e.invocation=?2", SqlValue::Text(call.to_owned())),
    };
    let query = format!(
        "SELECT e.seq,e.work,e.actor,e.target,e.kind,e.digest,e.invocation,e.created,strftime('%Y-%m-%d %H:%M UTC',e.created,'unixepoch')
         FROM events e WHERE {} AND e.kind IN ({KINDS}) AND {condition} ORDER BY e.seq DESC",
        experience::VISIBLE
    );
    let mut statement = db.prepare(&query)?;
    let person = Contact::persona(owner);
    for row in statement.query_map(params![owner, value], Event::row)? {
        if let Some(encounter) = row?.classify(db, &person)? {
            receive(encounter)?;
        }
    }
    Ok(())
}

pub(super) fn history(
    db: &Connection,
    owner: &str,
    receive: impl FnMut(Encounter) -> Result<()>,
) -> Result<()> {
    visit(db, owner, Filter::All, receive)
}

pub(super) fn at(db: &Connection, owner: &str, sequence: i64) -> Result<Option<Encounter>> {
    let mut result = None;
    visit(db, owner, Filter::Event(sequence), |encounter| {
        result = Some(encounter);
        Ok(())
    })?;
    Ok(result)
}

pub(super) fn evidence(
    db: &Connection,
    owner: &str,
    who: &Contact,
    reference: &str,
) -> Result<Value> {
    let meta = experience::metadata(db, Some(owner), reference)?;
    let mut linked = Vec::new();
    let mut collect = |e: Encounter| {
        if &e.counterpart == who {
            linked.push(serde_json::json!({"experience":e.reference,"direction":e.direction}));
        }
        Ok(())
    };
    if meta["kind"] == "accepted_decision" {
        let call = meta["invocation"]
            .as_str()
            .ok_or_else(|| failure("Decision has no invocation"))?;
        visit(db, owner, Filter::Decision(call), &mut collect)?;
    } else {
        let sequence = reference
            .strip_prefix("experience:event-")
            .and_then(|v| v.strip_suffix("@1"))
            .and_then(|v| v.parse::<i64>().ok())
            .ok_or_else(|| failure("Choose an exact experience"))?;
        visit(db, owner, Filter::Event(sequence), &mut collect)?;
    }
    Ok(
        serde_json::json!({"experience":reference,"work":meta["work"],"kind":meta["kind"],"exchanges":linked}),
    )
}

pub(super) fn validate_contact(db: &Connection, owner: &str, contact: &Contact) -> Result<()> {
    if contact.id.trim().is_empty() {
        return Err(failure("Choose a named counterpart"));
    }
    if contact.kind == Kind::Persona {
        if contact.id == owner {
            return Err(failure("Choose another person for a relationship note"));
        }
        store::required(db, "persona", &contact.id)?;
    }
    Ok(())
}
