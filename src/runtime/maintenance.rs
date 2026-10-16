//! Explicit, bounded dispositions of retained maintenance. No content judgment,
//! transfer, wake, warning transition, or successful repair is inferred here.
use super::*;

const MAX_RECORDS: usize = 128;
const MAX_DISPOSITIONS: usize = 256;
const MAX_RETAINED_BYTES: usize = 262_144;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispositionReceipt {
    pub revision: String,
    pub persona: String,
    pub work: String,
    pub invocation: String,
    pub owner_epoch: u64,
    pub authority: WorkAuthority,
    pub at_unix: i64,
    pub operation: p::Operation,
    pub exposure: Exposure,
}

fn identity_for(actor: &str, work: &str, index: usize, record: &Value) -> Result<(String, String)> {
    let revision = digest(&("protected-maintenance-v1", actor, work, index, record))?;
    Ok((format!("maintenance:{revision}"), revision))
}

fn latest<'a>(focus: &'a Focus, id: &str) -> Option<&'a DispositionReceipt> {
    focus.maintenance_dispositions.iter().rev().find(|receipt| {
        matches!(&receipt.operation, p::Operation::MaintenanceDispose {maintenance_id, ..} if maintenance_id == id)
    })
}

fn bounds(focus: &Focus) -> Result<()> {
    require(
        focus.protected_maintenance.len() <= MAX_RECORDS
            && focus.maintenance_dispositions.len() <= MAX_DISPOSITIONS,
        "maintenance retention record bound",
    )?;
    require(
        serde_json::to_vec(&(
            &focus.protected_maintenance,
            &focus.maintenance_exposures,
            &focus.maintenance_dispositions,
        ))?
        .len()
            <= MAX_RETAINED_BYTES,
        "maintenance retention byte bound",
    )
}

/// Existing legacy records keep their exact bytes and stable owner/work-bound
/// identities. Missing legacy exposure grants neither display nor disposition.
pub(super) fn project(
    state: &State,
    actor: &str,
    work: &str,
    admission: &f::Admission,
    exposure: &mut Exposure,
) -> Result<Vec<Value>> {
    let focus = &state.personas[actor].focus[work];
    let mut projected = Vec::new();
    for (index, item) in focus.protected_maintenance.iter().enumerate() {
        // Unavailable history establishes no current authored disposition.
        let hidden = json!({"status":"withheld_unavailable","body_supplied":false});
        let Some(inherited) = focus.maintenance_exposures.get(index) else {
            projected.push(hidden);
            continue;
        };
        if !exposure_permitted(state, inherited, admission) {
            projected.push(hidden);
            continue;
        }
        let (id, original_revision) = identity_for(actor, work, index, item)?;
        let current = latest(focus, &id);
        if current.is_some_and(|r| !exposure_permitted(state, &r.exposure, admission)) {
            projected.push(hidden);
            continue;
        }
        extend_exposure(exposure, inherited);
        let mut value = item.clone();
        let object = value
            .as_object_mut()
            .ok_or_else(|| held("maintenance record unavailable"))?;
        object.insert("id".into(), json!(id));
        object.insert(
            "revision".into(),
            json!(current.map_or(&original_revision, |r| &r.revision)),
        );
        object.insert("owner".into(), json!(actor));
        object.insert("work".into(), json!(work));
        object.insert("original_revision".into(), json!(original_revision));
        if let Some(receipt) = current {
            extend_exposure(exposure, &receipt.exposure);
            let p::Operation::MaintenanceDispose { disposition, .. } = &receipt.operation else {
                return Err(held("maintenance disposition unavailable"));
            };
            object.insert("resolution".into(), serde_json::to_value(disposition)?);
            object.insert(
                "disposition".into(),
                json!({
                    "revision":receipt.revision,"persona":receipt.persona,"work":receipt.work,
                    "invocation":receipt.invocation,"owner_epoch":receipt.owner_epoch,
                    "authority":receipt.authority,"at_unix":receipt.at_unix,
                    "operation":receipt.operation,"semantic_truth_verified":false,
                    "warning_resolved":false
                }),
            );
        }
        projected.push(value);
    }
    Ok(projected)
}

pub(super) fn retain_outcomes(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    now: i64,
) -> Result<()> {
    let focus = state
        .personas
        .get_mut(&prepared.request.persona)
        .unwrap()
        .focus
        .get_mut(&prepared.request.work)
        .unwrap();
    let mut changed = false;
    for (scope, outcome) in [
        ("fragments", &decision.fragment_maintenance),
        ("assets", &decision.asset_maintenance),
    ] {
        if matches!(
            outcome,
            p::Maintenance::Deferred { .. } | p::Maintenance::Blocked { .. }
        ) {
            // An older missing manifest cannot acquire today's exposure merely
            // because a new record is appended to these parallel histories.
            require(
                focus.maintenance_exposures.len() == focus.protected_maintenance.len(),
                "legacy maintenance exposure alignment unavailable",
            )?;
            focus.protected_maintenance.push(json!({
                "invocation":prepared.request.id,"scope":scope,"authored_outcome":outcome,
                "owner":prepared.request.persona,"work":prepared.request.work,"at_unix":now,
                "mode":prepared.request.mode,"automatic_reconsideration":false,
                "resolution":"unresolved"
            }));
            focus
                .maintenance_exposures
                .push(prepared_exposure(prepared)?.clone());
            changed = true;
        }
    }
    if changed {
        bounds(focus)?;
    }
    Ok(())
}

pub(super) fn apply(
    state: &mut State,
    prepared: &Prepared,
    decision: &p::Decision,
    _actions: &BTreeMap<String, WorkAction>,
    now: i64,
) -> Result<()> {
    if !decision
        .operations
        .iter()
        .any(|o| matches!(o, p::Operation::MaintenanceDispose { .. }))
    {
        return Ok(());
    }
    require(
        prepared.request.mode == Mode::Ordinary,
        "maintenance disposition requires ordinary mode",
    )?;
    let (_, tail) = prepared
        .prompt
        .split_once("\nCONTROL\n")
        .ok_or_else(|| held("maintenance requires supplied control"))?;
    let (control, _) = tail
        .split_once("\nQUALIFIED_CONTEXT\n")
        .ok_or_else(|| held("maintenance requires supplied context"))?;
    let control: Value = serde_json::from_str(control)?;
    let actor = &prepared.request.persona;
    let work = &prepared.request.work;
    let admission = admission(&state.personas[actor], now);
    let mut inherited = prepared_exposure(prepared)?.clone();
    let current = project(state, actor, work, &admission, &mut inherited)?;
    let mut seen = BTreeSet::new();
    for operation in &decision.operations {
        let p::Operation::MaintenanceDispose {
            scope: requested_scope,
            maintenance_id,
            maintenance_revision,
            disposition,
            evidence,
            ..
        } = operation
        else {
            continue;
        };
        let grants = &state.personas[actor].seed.grants;
        require(
            match requested_scope {
                p::Scope::Fragments {} => grants.fragments,
                p::Scope::Assets {} => grants.assets,
            },
            "maintenance disposition authorship scope not granted",
        )?;
        require(
            seen.insert(maintenance_id),
            "duplicate maintenance disposition",
        )?;
        let supplied = control["protected_maintenance"]
            .as_array()
            .is_some_and(|records| {
                records.iter().any(|record| {
                    record["id"].as_str() == Some(maintenance_id)
                        && record["revision"].as_str() == Some(maintenance_revision)
                })
            });
        require(supplied, "exact maintenance revision was not supplied")?;
        let record = current
            .iter()
            .find(|record| {
                record["id"].as_str() == Some(maintenance_id)
                    && record["revision"].as_str() == Some(maintenance_revision)
            })
            .ok_or_else(|| held("maintenance revision unavailable or stale"))?;
        let expected_scope = match requested_scope {
            p::Scope::Fragments {} => "fragments",
            p::Scope::Assets {} => "assets",
        };
        require(
            record["scope"] == expected_scope,
            "maintenance scope differs",
        )?;
        if *disposition == p::MaintenanceDisposition::Expired {
            require(
                record["authored_outcome"]["deadline_unix"]
                    .as_u64()
                    .is_some_and(|deadline| u64::try_from(now).is_ok_and(|time| time >= deadline)),
                "maintenance deadline has not elapsed",
            )?;
        }
        for id in evidence {
            require(
                prepared.supplied_observations.contains(id),
                "maintenance evidence was not supplied",
            )?;
            let observation = state
                .observations
                .get(id)
                .ok_or_else(|| held("maintenance evidence absent"))?;
            require(
                observation.actor == *actor && observation.work == *work,
                "maintenance evidence attribution differs",
            )?;
            let source = observation
                .exposure
                .as_ref()
                .ok_or_else(|| held("maintenance evidence exposure unavailable"))?;
            require(
                exposure_permitted(state, source, &admission),
                "maintenance evidence currently unavailable",
            )?;
            extend_exposure(&mut inherited, source);
        }
        require(
            exposure_permitted(state, &inherited, &admission),
            "maintenance disposition exposure unavailable",
        )?;
        let current_work = state.board.work(&w::WorkId(work.clone()))?;
        let mut receipt = DispositionReceipt {
            revision: String::new(),
            persona: actor.clone(),
            work: work.clone(),
            invocation: prepared.request.id.clone(),
            owner_epoch: prepared.owner_epoch,
            authority: WorkAuthority {
                work: work.clone(),
                version: current_work.version,
                authority: current_work.mandate().authority.clone(),
            },
            at_unix: now,
            operation: operation.clone(),
            exposure: inherited.clone(),
        };
        receipt.revision = digest(&receipt)?;
        let focus = state
            .personas
            .get_mut(actor)
            .unwrap()
            .focus
            .get_mut(work)
            .unwrap();
        focus.maintenance_dispositions.push(receipt);
        bounds(focus)?;
    }
    Ok(())
}
