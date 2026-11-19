use super::{blob, failure, store, Result};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    /// Exact source asset revision, STL asset, mesh object or render receipt.
    pub reference: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "page_size")]
    pub page_size: usize,
}

fn page_size() -> usize {
    20
}

impl Lookup {
    pub fn first(reference: &str) -> Self {
        Self {
            reference: reference.to_owned(),
            offset: 0,
            page_size: page_size(),
        }
    }
}

/// The outbox binds the resolved source to the real terminal receipt. Operator
/// renders have a separate identity and never acquire a persona decision.
pub fn lookup(db: &Connection, input: &Lookup) -> Result<Value> {
    if input.page_size == 0 {
        return Err(failure("Choose a positive render history page size"));
    }
    let mesh = format!("object:{}", blob(db, &input.reference)?);
    let mut sql=db.prepare("
        WITH renders AS (
            SELECT 'persona' AS origin, e.actor, e.work, e.invocation, e.result AS digest,
                   i.created AS stamp, e.id AS identity, e.ordinal,
                   json_extract(CAST(op.bytes AS TEXT),'$.input.reference') AS source,
                   json_extract(CAST(r.bytes AS TEXT),'$.mesh') AS mesh
            FROM outbox e JOIN invocations i ON i.id=e.invocation
            JOIN objects op ON op.digest=e.operation JOIN objects r ON r.digest=e.result
            WHERE e.state IN ('completed','superseded') AND json_extract(CAST(op.bytes AS TEXT),'$.tool')='geometry.render'
            UNION ALL
            SELECT 'operator',json_extract(CAST(r.bytes AS TEXT),'$.operator'),NULL,NULL,v.digest,
                   v.created,v.id,0,json_extract(CAST(r.bytes AS TEXT),'$.input.reference'),
                   json_extract(CAST(r.bytes AS TEXT),'$.result.mesh')
            FROM versions v JOIN objects r ON r.digest=v.digest WHERE v.kind='operator_render'
        )
        SELECT origin,actor,work,invocation,digest,source FROM renders
        WHERE source=?1 OR mesh=?2 OR ('object:'||digest)=?1
        ORDER BY stamp DESC,invocation DESC,ordinal DESC,identity DESC")?;
    let mut rows = sql.query(rusqlite::params![input.reference, mesh])?;
    let mut total = 0;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        total += 1;
        if total <= input.offset || results.len() >= input.page_size {
            continue;
        }
        let origin: String = row.get(0)?;
        let digest: String = row.get(4)?;
        let stored: Value = store::object(db, &digest)?;
        let result = if origin == "operator" {
            &stored["result"]
        } else {
            &stored
        };
        results.push(json!({"origin":origin,"actor":row.get::<_,String>(1)?,"work":row.get::<_,Option<String>>(2)?,
            "invocation":row.get::<_,Option<String>>(3)?,"receipt":format!("object:{digest}"),"source":row.get::<_,String>(5)?,
            "mesh":result["mesh"],"success":result["success"],"error":result["error"],"reason":result["reason"],
            "installation":result["installation"],"diagnostics":result["diagnostics"],"warnings":result["warnings"]}));
    }
    let end = input.offset.saturating_add(results.len());
    Ok(
        json!({"reference":input.reference,"total":total,"offset":input.offset,"renders":results,
        "next_offset":(end<total).then_some(end)}),
    )
}
