//! Change the credential source of a saved workspace without changing its work.
use super::{
    failure, platform_key, protocol::Persona, runner::{AuthRoute, Config, RunnerLock},
    store::{self, Store}, Result,
};
use crate::provider::CredentialReference;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) enum Route {
    Codex(AuthRoute),
    PlatformKeyFile(PathBuf),
}

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

pub(super) async fn change(root: &Path, operator: &str, route: Route) -> Result<Value> {
    let (root, operator) = (root.to_owned(), operator.to_owned());
    tokio::task::spawn_blocking(move || bind(&root, &operator, route)).await?
}

fn bind(root: &Path, operator: &str, route: Route) -> Result<Value> {
    let mut store = Store::open(root)?;
    let _owner = RunnerLock::acquire(&store.root)?;
    settled(&store.db)?;
    let prior = store::required(&store.db, "config", "runtime")?;
    let mut cfg: Config = serde_json::from_value(prior.body)?;
    let first = store::list(&store.db, "persona")?.into_iter().next()
        .ok_or_else(|| failure("This workspace has no personas"))?;
    let first: Persona = serde_json::from_value(first.body)?;
    let (model, effort) = (&first.provider.model, &first.provider.effort);
    // Resolve and bind the chosen credential once. The database only receives
    // its route and account-bound settings, never key bytes or an OAuth bearer.
    let (name, auth_route, key_file, bound) = match route {
        Route::Codex(route) => {
            let route = route.canonical()?;
            let bound = route.settings(model, effort)?;
            ("codex_oauth", Some(route), None, bound)
        }
        Route::PlatformKeyFile(path) => {
            let path = platform_key::canonical(&path, &cfg.working_directory)?;
            let bound = platform_key::settings(&path, model, effort)?;
            ("platform_key_file", None, Some(path), bound)
        }
    };
    let same_route = cfg.auth_route == auth_route && cfg.platform_key_file == key_file;
    cfg.auth_route = auth_route;
    cfg.platform_key_file = key_file;
    store.transaction(|db| {
        settled(db)?;
        let config = if same_route { prior.reference.clone() } else {
            store::write(db, "config", "runtime", prior.reference.revision, &cfg, None)?
        };
        let mut changes = Vec::new();
        for record in store::list(db, "persona")? {
            let mut p: Persona = serde_json::from_value(record.body)?;
            let previous_mode = p.provider.mode;
            let next = bound.selection(&p.provider.model, &p.provider.effort)?;
            // An OAuth expiry refresh alone is not a new connection. A different
            // API key in the same file, however, has a different account binding.
            let same_source = match (&p.provider.credentials, &next.credentials) {
                (CredentialReference::EphemeralAuthOnly { grant: a, .. },
                 CredentialReference::EphemeralAuthOnly { grant: b, .. }) => a == b,
                (CredentialReference::ApiKey { key_env: a, account_env: b },
                 CredentialReference::ApiKey { key_env: c, account_env: d }) => a == c && b == d,
                _ => false,
            };
            if same_route && previous_mode == next.mode
                && same_source && p.provider.evidence.account_env == next.evidence.account_env
                && p.provider.evidence.account_sha256 == next.evidence.account_sha256 {
                continue;
            }
            p.provider = next;
            let source = record.reference;
            let new = store::write(db, "persona", &source.id, source.revision, &p, None)?;
            changes.push(json!({"persona":p.id,"from":source.handle(),"to":new.handle(),
                "previous_mode":previous_mode,"model":p.provider.model,"effort":p.provider.effort}));
        }
        let result = json!({"workspace_connection":name,"mode":bound.mode,
            "transport_profile":bound.evidence.transport_profile,"prior_config":prior.reference.handle(),
            "config":config.handle(),"operator":operator,"personas_changed":changes,
            "changed":!same_route || !changes.is_empty(),"inference_started":false});
        if result["changed"] == true {
            store::event(db, "", operator, operator, "workspace_connection_changed", &result, None)?;
        }
        Ok(result)
    })
}
