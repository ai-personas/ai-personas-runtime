//! Keep provider output items available to local verifiers outside the resource catalogue.
use super::{failure, store, Result};
use crate::{provider, store::sha256_bytes};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn take_items(receipt: &mut Value) -> Result<Option<(String, Vec<u8>)>> {
    let Some(items) = receipt
        .as_object_mut()
        .and_then(|r| r.remove("private_output_items"))
        .filter(|v| !v.is_null())
    else {
        return Ok(None);
    };
    let summary = provider::retained_output_metadata(&items)
        .ok_or_else(|| failure("Invalid retained provider output metadata"))?;
    let bytes = serde_json::to_vec(&items)?;
    let digest = sha256_bytes(&bytes);
    receipt["output_items"] = summary;
    receipt["output_items_sha256"] = json!(digest);
    Ok(Some((digest, bytes)))
}

/// The caller writes the invocation and this evidence in the same SQLite transaction.
pub(super) fn save(db: &Connection, attempt: &provider::AttemptReceipt) -> Result<String> {
    if attempt.private_output_items.is_none() {
        return store::put(db, attempt);
    }
    let mut public = serde_json::to_value(attempt)?;
    let (digest, bytes) =
        take_items(&mut public)?.ok_or_else(|| failure("Missing returned provider items"))?;
    let receipt = store::put(db, &public)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS provider_outputs(
        receipt TEXT PRIMARY KEY REFERENCES objects(digest),digest TEXT NOT NULL,bytes BLOB NOT NULL)")?;
    db.execute(
        "INSERT OR IGNORE INTO provider_outputs(receipt,digest,bytes) VALUES(?1,?2,?3)",
        params![receipt, digest, bytes],
    )?;
    Ok(receipt)
}

fn verified(public: &Value, digest: &str, bytes: &[u8]) -> Result<Value> {
    if public["output_items_sha256"] != digest || sha256_bytes(bytes) != digest {
        return Err(failure("Retained provider output differs from its receipt"));
    }
    let items: Value = serde_json::from_slice(bytes)?;
    if provider::retained_output_metadata(&items).as_ref() != public.get("output_items") {
        return Err(failure(
            "Retained provider output summary differs from its receipt",
        ));
    }
    Ok(items)
}

pub(super) fn final_matches(
    db: &Connection,
    id: &str,
    receipt: &Value,
    response: &[u8],
) -> Result<Option<bool>> {
    let profile = receipt["audit"]["transport_profile"]
        .as_str()
        .unwrap_or_default();
    if receipt.get("output_items_sha256").is_some() {
        let (digest, bytes): (String, Vec<u8>) = db.query_row(
            "SELECT digest,bytes FROM provider_outputs WHERE receipt=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let items = verified(receipt, &digest, &bytes)?;
        return Ok(provider::retained_decision_matches(
            &items, response, profile,
        ));
    }
    Ok(receipt
        .get("private_output_items")
        .filter(|v| !v.is_null())
        .and_then(|items| provider::retained_decision_matches(items, response, profile)))
}

/// A receipt retained in the older inline representation can still be opened as
/// structured metadata. An ordinary artifact containing similar words keeps its bytes.
pub fn historical_view(db: &Connection, digest: &str, bytes: &[u8]) -> Result<Option<Value>> {
    if bytes.first() != Some(&b'{')
        || !std::str::from_utf8(bytes).is_ok_and(|s| s.contains("\"private_output_items\""))
    {
        return Ok(None);
    }
    let Ok(mut body) = serde_json::from_slice::<Value>(bytes) else {
        return Ok(None);
    };
    if !body["private_output_items"].is_object() || !body["audit"]["invocation_id"].is_string() {
        return Ok(None);
    }
    let is_receipt: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM invocations WHERE receipt=?1)",
        [digest],
        |r| r.get(0),
    )?;
    if !is_receipt {
        return Ok(None);
    }
    take_items(&mut body)?;
    Ok(Some(body))
}

pub(super) fn resource_bytes(db: &Connection, digest: &str) -> Result<Vec<u8>> {
    let bytes = store::bytes(db, digest)?;
    if historical_view(db, digest, &bytes)?.is_some() {
        return Err(failure(
            "Use asset.read to inspect the public metadata of this historical provider receipt",
        ));
    }
    Ok(bytes)
}

fn table_exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='provider_outputs')",
        [],
        |r| r.get(0),
    )?)
}

pub(super) fn storage(db: &Connection) -> Result<Value> {
    let counts = if table_exists(db)? {
        db.query_row(
            "SELECT count(*),coalesce(sum(length(bytes)),0) FROM provider_outputs",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?
    } else {
        (0, 0)
    };
    Ok(json!({"receipts":counts.0,"payload_bytes":counts.1}))
}

pub(super) fn archive_check(db: &Connection) -> Result<()> {
    let has = table_exists(db)?;
    let owned = "FROM invocations i JOIN objects o ON o.digest=i.receipt WHERE json_type(CAST(o.bytes AS TEXT),'$.output_items_sha256')='text'";
    let missing_sql = if has {
        format!("SELECT EXISTS(SELECT 1 {owned} AND NOT EXISTS(SELECT 1 FROM provider_outputs p WHERE p.receipt=i.receipt))")
    } else {
        format!("SELECT EXISTS(SELECT 1 {owned})")
    };
    if db.query_row(&missing_sql, [], |r| r.get::<_, bool>(0))? {
        return Err(failure("Archive is missing retained provider output"));
    }
    if has {
        let mut stmt=db.prepare("SELECT p.digest,p.bytes,o.bytes FROM provider_outputs p JOIN objects o ON o.digest=p.receipt")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let public = serde_json::from_slice(row.get_ref(2)?.as_blob()?)?;
            verified(
                &public,
                row.get_ref(0)?.as_str()?,
                row.get_ref(1)?.as_blob()?,
            )?;
        }
    }
    Ok(())
}
