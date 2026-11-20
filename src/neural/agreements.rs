//! Voluntary, edition-specific terms and the positions actually taken on them.
use super::{context, failure, id, relationships::Contact, store, Result};
use clap::{Args, ValueEnum};
use rusqlite::{params, Connection};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// Exact current work brief under which these terms are being proposed.
    #[arg(long)]
    pub brief: String,
    /// Exact latest agreement edition to revise. Omit to start a separate proposal.
    #[arg(long)]
    pub base: Option<String>,
    #[arg(long)]
    pub title: String,
    /// Where and when the terms apply.
    #[arg(long)]
    pub scope: String,
    /// The complete terms of this edition.
    #[arg(long)]
    pub terms: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Endorse,
    Dissent,
    Withdraw,
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// Exact agreement edition whose terms you read.
    #[arg(long)]
    pub agreement: String,
    #[arg(long)]
    pub position: Position,
    #[arg(long)]
    pub reason: String,
    /// Your exact last position on this edition, if any, including a withdrawal.
    #[arg(long)]
    pub previous: Option<String>,
}

#[derive(Clone, Debug, Args, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    /// Omit for the edition index. Select an exact edition to read its terms and attributed responses.
    #[arg(long)]
    pub edition: Option<String>,
    /// Show only the newest edition of each proposal, or each signer's latest response on the selected edition.
    #[arg(long)]
    #[serde(default)]
    pub latest_only: bool,
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    #[arg(long, default_value_t = page_size())]
    #[serde(default = "page_size")]
    pub page_size: usize,
}
fn page_size() -> usize {
    20
}

pub(super) enum Author<'a> {
    Operator(&'a str),
    Participant(&'a context::Snapshot),
}
impl Author<'_> {
    fn contact(&self) -> Contact {
        match self {
            Self::Operator(name) => Contact::operator(name),
            Self::Participant(snapshot) => Contact::persona(&snapshot.actor),
        }
    }
    fn sender(&self) -> String {
        match self {
            Self::Operator(name) => format!("operator:{name}"),
            Self::Participant(snapshot) => snapshot.actor.clone(),
        }
    }
    fn call(&self) -> Option<&str> {
        match self {
            Self::Operator(_) => None,
            Self::Participant(s) => Some(&s.invocation),
        }
    }
    fn check_read(&self, record: &store::Record) -> Result<()> {
        if let Self::Participant(s) = self {
            let reference = record.reference.handle();
            if !s.data["context_recovery"].is_null()
                || !context::supplied_resources(&s.data).iter().any(|supply| {
                    supply.value["reference"] == reference && supply.value["record"] == record.body
                })
            {
                return Err(failure(format!("Read the complete terms of {reference} with work.agreements or asset.read before acting on them")));
            }
        }
        Ok(())
    }
}

fn edition(db: &Connection, work: &str, reference: &str) -> Result<store::Record> {
    let record = store::exact(db, reference)?;
    if record.reference.kind != "agreement"
        || record.reference.handle() != reference
        || record.body["work"] != work
    {
        return Err(failure(
            "Select an exact agreement edition from this activity",
        ));
    }
    Ok(record)
}

pub(super) fn propose(
    db: &Connection,
    work: &str,
    author: Author<'_>,
    proposal: &Proposal,
) -> Result<Value> {
    let brief = store::open_activity(db, work)?.reference.handle();
    if proposal.brief != brief {
        return Err(failure(
            "The brief changed. Read the current brief before proposing these terms",
        ));
    }
    if [&proposal.title, &proposal.scope, &proposal.terms]
        .iter()
        .any(|s| s.trim().is_empty())
    {
        return Err(failure(
            "Give the agreement a title, scope and complete terms",
        ));
    }
    let (thread, expected) = match &proposal.base {
        Some(base) => {
            let prior = edition(db, work, base)?;
            author.check_read(&prior)?;
            (prior.reference.id, prior.reference.revision)
        }
        None => (id("agreement")?, 0),
    };
    let contact = author.contact();
    let body = json!({"work":work, "brief":brief, "author":contact, "previous":proposal.base,
        "title":proposal.title, "scope":proposal.scope, "terms":proposal.terms});
    let reference =
        store::write(db, "agreement", &thread, expected, &body, author.call())?.handle();
    let notice = json!({"work":work,"reference":reference,"title":proposal.title,"author":contact});
    store::event(
        db,
        work,
        &author.sender(),
        "*",
        "agreement_proposed",
        &notice,
        author.call(),
    )?;
    Ok(notice)
}

fn position_id(edition: &str, author: &Contact) -> Result<String> {
    Ok(crate::store::sha256_bytes(&serde_json::to_vec(&(
        edition, author,
    ))?))
}

pub(super) fn respond(
    db: &Connection,
    work: &str,
    author: Author<'_>,
    response: &Response,
) -> Result<Value> {
    let brief = store::open_activity(db, work)?.reference.handle();
    let subject = edition(db, work, &response.agreement)?;
    author.check_read(&subject)?;
    if response.reason.trim().is_empty() {
        return Err(failure("Give your reason for this position"));
    }
    let contact = author.contact();
    let key = position_id(&response.agreement, &contact)?;
    let prior = store::head(db, "agreement_position", &key)?;
    if prior.as_ref().map(|r| r.reference.handle()) != response.previous {
        return Err(failure("Your recorded position changed. Read your latest response on this edition before changing it"));
    }
    if response.position == Position::Withdraw {
        if prior
            .as_ref()
            .is_none_or(|r| r.body["position"] == "withdraw")
        {
            return Err(failure(
                "There is no standing position of yours to withdraw on this edition",
            ));
        }
    } else if subject.body["brief"] != brief
        || store::required(db, "agreement", &subject.reference.id)?.reference != subject.reference
    {
        return Err(failure("These terms belong to an earlier edition or brief. Read the current edition or propose revised terms"));
    }
    let body = json!({"work":work,"brief":brief,"agreement":response.agreement,"author":contact,
        "position":response.position,"reason":response.reason,"previous":response.previous});
    let expected = prior.map_or(0, |r| r.reference.revision);
    let reference = store::write(
        db,
        "agreement_position",
        &key,
        expected,
        &body,
        author.call(),
    )?
    .handle();
    let notice = json!({"work":work,"reference":reference,"agreement":response.agreement,"title":subject.body["title"],"position":response.position,"author":contact});
    store::event(
        db,
        work,
        &author.sender(),
        "*",
        "agreement_response",
        &notice,
        author.call(),
    )?;
    Ok(notice)
}

pub(super) fn available(db: &Connection, work: &str) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE work=?1 AND target='*' AND kind='agreement_proposed')",
        [work], |row| row.get(0))?)
}

fn position_counts(db: &Connection, reference: &str) -> Result<Value> {
    let mut result = json!({"endorse":0,"dissent":0,"withdraw":0});
    let mut stmt = db.prepare(
        "SELECT json_extract(CAST(o.bytes AS TEXT),'$.position'),count(*)
        FROM heads h JOIN objects o ON h.digest=o.digest WHERE h.kind='agreement_position'
        AND json_extract(CAST(o.bytes AS TEXT),'$.agreement')=?1 GROUP BY 1",
    )?;
    let mut rows = stmt.query([reference])?;
    while let Some(row) = rows.next()? {
        let position: String = row.get(0)?;
        let count = result
            .get_mut(&position)
            .ok_or_else(|| failure("Unknown signed agreement position"))?;
        *count = json!(row.get::<_, i64>(1)?);
    }
    Ok(result)
}

/// Page through actual version writes. Repeated notifications do not create extra signatories.
pub fn inspect(db: &Connection, work: &str, reader: &Contact, query: &Lookup) -> Result<Value> {
    let brief = store::required(db, "work", work)?;
    if query.page_size == 0 {
        return Err(failure("Page size must be positive"));
    }
    let mut report = json!({"work":work,"brief":brief.reference.handle(),"ended":brief.body["phase"] == "dissolved"});
    let kind = if let Some(reference) = &query.edition {
        let record = edition(db, work, reference)?;
        report["reference"] = json!(reference);
        report["latest"] = json!(store::required(db, "agreement", &record.reference.id)?
            .reference
            .handle());
        report["record"] = record.body;
        report["positions"] = position_counts(db, reference)?;
        report["your_position"] = json!(store::head(
            db,
            "agreement_position",
            &position_id(reference, reader)?
        )?);
        "agreement_position"
    } else {
        "agreement"
    };
    let filter = "FROM versions v JOIN objects o ON o.digest=v.digest
        JOIN heads h USING(kind,id) LEFT JOIN invocations i ON v.invocation=i.id
        WHERE v.kind=?1 AND (NOT ?4 OR v.revision=h.revision) AND json_extract(CAST(o.bytes AS TEXT),'$.work')=?2
        AND (?3 IS NULL OR json_extract(CAST(o.bytes AS TEXT),'$.agreement')=?3)";
    let total: i64 = db.query_row(
        &format!("SELECT count(*) {filter}"),
        params![kind, work, query.edition, query.latest_only],
        |r| r.get(0),
    )?;
    let mut stmt = db.prepare(&format!(
        "SELECT v.id,v.revision,v.digest,v.created,h.revision,v.invocation,i.actor,i.work {filter}
        ORDER BY v.rowid DESC LIMIT ?5 OFFSET ?6"
    ))?;
    let mut rows = stmt.query(params![
        kind,
        work,
        query.edition,
        query.latest_only,
        i64::try_from(query.page_size).unwrap_or(i64::MAX),
        i64::try_from(query.offset).unwrap_or(i64::MAX)
    ])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let revision: i64 = row.get(1)?;
        let digest: String = row.get(2)?;
        let body: Value = store::object(db, &digest)?;
        let call: Option<String> = row.get(5)?;
        let author = &body["author"];
        let authored = match &call {
            Some(_) => {
                author["kind"] == "persona"
                    && row.get::<_, Option<String>>(6)?.as_deref() == author["id"].as_str()
                    && row.get::<_, Option<String>>(7)?.as_deref() == Some(work)
            }
            None => {
                author["kind"] == "operator"
                    && author["id"]
                        .as_str()
                        .is_some_and(|id| !id.trim().is_empty())
            }
        };
        if !authored {
            return Err(failure(
                "Agreement author differs from the source of its recorded version",
            ));
        }
        let mut item = json!({"reference":format!("{kind}:{id}@{revision}"),"revision":revision,"latest":revision == row.get::<_,i64>(4)?,
            "author":author,"brief":body["brief"],"at_unix":row.get::<_,i64>(3)?,"decision":call});
        if query.edition.is_some() {
            item["position"] = body["position"].clone();
            item["reason"] = body["reason"].clone();
            item["previous"] = body["previous"].clone();
        } else {
            item["title"] = body["title"].clone();
        }
        items.push(item);
    }
    report["items"] = json!(items);
    report["total"] = json!(total);
    report["offset"] = json!(query.offset);
    Ok(report)
}
