//! Change the credential source of a saved workspace without changing its work.
use super::{
    failure, protocol::Persona, runner::{AuthRoute, Config, RunnerLock},
    store::{self, Store}, Result,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;

fn settled(db: &Connection) -> Result<()> {
    let outstanding: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM invocations WHERE state IN
         ('prepared','dispatched','responded','probe_prepared','probe_dispatched'))
         OR EXISTS(SELECT 1 FROM outbox WHERE state IN ('pending','dispatched'))",
        [], |r| r.get(0),
    )?;
    if outstanding {
        return Err(failure("The previous run has unsettled work. Settle it before switching connections."));
    }
    Ok(())
}

pub(super) fn connect_codex(root: &Path, operator: &str, route: AuthRoute) -> Result<Value> {
    let mut store = Store::open(root)?;
    let _owner = RunnerLock::acquire(&store.root)?;
    settled(&store.db)?;
    let route = route.canonical()?;
    let first = store::list(&store.db, "persona")?.into_iter().next()
        .ok_or_else(|| failure("This workspace has no personas"))?;
    let first: Persona = serde_json::from_value(first.body)?;
    // Authenticate once, outside the write transaction. Only account-bound
    // settings come back from the helper; the bearer stays in the adapter.
    let bound = route.settings(&first.provider.model, &first.provider.effort)?;
    store.transaction(|db| {
        settled(db)?;
        let prior = store::required(db, "config", "runtime")?;
        let mut cfg: Config = serde_json::from_value(prior.body)?;
        let same_route = cfg.platform_key_file.is_none() && cfg.auth_route.as_ref() == Some(&route);
        cfg.platform_key_file = None;
        cfg.auth_route = Some(route);
        let config = if same_route { prior.reference.clone() } else {
            store::write(db, "config", "runtime", prior.reference.revision, &cfg, None)?
        };
        let mut changes = Vec::new();
        for record in store::list(db, "persona")? {
            let mut p: Persona = serde_json::from_value(record.body)?;
            let previous_mode = p.provider.mode;
            let next = bound.selection(&p.provider.model, &p.provider.effort)?;
            // A refreshed expiry alone is not a new connection. The call itself
            // obtains a fresh token and checks the pinned account and its expiry.
            if same_route && previous_mode == next.mode
                && matches!(p.provider.credentials, crate::provider::CredentialReference::EphemeralAuthOnly { .. })
                && p.provider.evidence.account_sha256 == next.evidence.account_sha256 {
                continue;
            }
            p.provider = next;
            let source = record.reference;
            let new = store::write(db, "persona", &source.id, source.revision, &p, None)?;
            changes.push(json!({"persona":p.id,"from":source.handle(),"to":new.handle(),
                "previous_mode":previous_mode,"model":p.provider.model,"effort":p.provider.effort}));
        }
        let result = json!({"workspace_connection":"codex_oauth","mode":bound.mode,
            "transport_profile":bound.evidence.transport_profile,"prior_config":prior.reference.handle(),
            "config":config.handle(),"operator":operator,"personas_changed":changes,
            "changed":!same_route || !changes.is_empty(),"inference_started":false});
        if result["changed"] == true {
            store::event(db, "", operator, operator, "workspace_connection_changed", &result, None)?;
        }
        Ok(result)
    })
}
