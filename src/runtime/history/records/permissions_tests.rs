//! Synthetic permission and qualification regressions, never provider execution.
use super::tests::{fixture, prepare};
use super::*;
const ACTOR: &str = "persona-0001";
const WORK: &str = "work-00000001";

fn observation(state: &mut State, id: &str, kind: &str, payload: Value) -> Observation {
    let value = Observation {
        id: id.into(),
        actor: ACTOR.into(),
        work: WORK.into(),
        invocation: Some("synthetic-producer".into()),
        kind: kind.into(),
        at_unix: 10,
        payload,
        sources: vec![],
        exposure: Some(Exposure::default()),
        qualified_roots: vec![],
    };
    state.observations.insert(id.into(), value.clone());
    value
}
fn descriptor(state: &State, id: &str, a: &f::Admission, full: bool) -> Option<Record> {
    observation_record(
        state,
        ACTOR,
        id,
        a,
        &corpus(state, a).ok()?,
        &Permissions::new(state, a).ok()?,
        full,
    )
}
fn source(state: &mut State, id: &str, text: &str) -> f::Reference {
    let mut value = state.sources["history-fixture-core"].clone();
    let managed = &mut value.revisions[0];
    managed.record.source = id.into();
    managed.record.path = format!("assets/{id}.txt");
    managed.record.scope = f::Scope::Assets;
    managed.record.bytes = text.as_bytes().to_vec();
    managed.record.revision = managed.record.computed_revision().unwrap();
    value.head = managed.record.revision.clone();
    let reference = managed.record.reference(f::Representation::Utf8);
    state.sources.insert(id.into(), value);
    reference
}

#[test]
fn original_manifest_current_revocation_destination_and_audience_all_gate_history() {
    let (_dir, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    let mut policy = state.config.policies.active_policies[0].clone();
    policy.id = "history-only-policy".into();
    state.config.policies.active_policies.push(policy.clone());
    let mut original = observation(
        &mut state,
        "protected-history",
        "fixture",
        json!({"secret":"history-private-body"}),
    );
    original.exposure = Some(Exposure {
        sources: vec![],
        policies: vec![ScopedPolicy {
            source: "history-fixture-core".into(),
            revision: state.sources["history-fixture-core"].head.clone(),
            policy: policy.clone(),
        }],
    });
    state
        .observations
        .insert(original.id.clone(), original.clone());
    let a = admission(&state.personas[ACTOR], 100);
    assert!(descriptor(&state, &original.id, &a, true).is_some());
    for changed in ["destination", "audience", "current", "original", "missing"] {
        let mut next = state.clone();
        let mut admission = a.clone();
        match changed {
            "destination" => admission.destination = "unapproved-destination".into(),
            "audience" => {
                admission.audience.insert("unapproved-audience".into());
            }
            "current" => {
                next.config
                    .policies
                    .active_policies
                    .last_mut()
                    .unwrap()
                    .revoked_at_unix = Some(100)
            }
            "original" => next
                .observations
                .get_mut(&original.id)
                .unwrap()
                .exposure
                .as_mut()
                .unwrap()
                .policies[0]
                .policy
                .readers
                .clear(),
            "missing" => next.observations.get_mut(&original.id).unwrap().exposure = None,
            _ => unreachable!(),
        }
        assert!(
            descriptor(&next, &original.id, &admission, true).is_none(),
            "{changed}"
        );
    }
}

#[test]
fn matching_cannot_recover_suppressed_host_accounting_fields() {
    let (_dir, mut app) = fixture();
    let prepared = prepare(&mut app, "sanitized-query", WORK, 100);
    let (_, mut state) = app.load().unwrap();
    observation(
        &mut state,
        "accounting-record",
        "host_accounting",
        json!({"reserved_allowance":{},"actual_navigation":"unsupplied-unique-private-counter"}),
    );
    let a = admission(&state.personas[ACTOR], 100);
    let permissions = Permissions::new(&state, &a).unwrap();
    let corpus = corpus(&state, &a).unwrap();
    let mut navigation = f::NavigationBudget::new(prepared.navigation);
    let result = query(
        &state,
        &prepared,
        "search-hidden",
        p::HistoryRecordKind::Observation,
        &f::SearchMode::Literal {
            needle: "unsupplied-unique-private-counter".into(),
        },
        128,
        None,
        &a,
        &permissions,
        &corpus,
        &Material::default(),
        &mut navigation,
        100000,
        &mut Exposure::default(),
    )
    .unwrap();
    assert_eq!(result.payload["status"], "no_match");
    assert!(result.payload["records"].as_array().unwrap().is_empty());
    let body = descriptor(&state, "accounting-record", &a, true)
        .unwrap()
        .body;
    assert!(!body
        .to_string()
        .contains("unsupplied-unique-private-counter"));
}

#[test]
fn fresh_history_supply_replaces_old_qualification_and_keeps_it_nonoperative() {
    let (_dir, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    let root = source(&mut state, "historical-subject", "historical-subject-body");
    let old = source(&mut state, "old-qualification", "old-qualification-marker");
    let new = source(&mut state, "new-qualification", "new-qualification-marker");
    let mut binding = f::AcceptedBinding {
        id: "history-binding".into(),
        revision: "binding-one".into(),
        source: root.source.clone(),
        file_revision: root.revision.clone(),
        policy: state.config.policies.active_policies[0].clone(),
        disposition: f::BindingDisposition::Qualify {
            required: vec![old.clone()],
        },
    };
    binding.revision = binding.computed_revision().unwrap();
    state.accepted_bindings.push(binding);
    let a = admission(&state.personas[ACTOR], 100);
    let original_bundle = f::assemble(
        &corpus(&state, &a).unwrap(),
        std::slice::from_ref(&root),
        &[],
        &[],
        &a,
        state.config.context_bytes,
        0,
        0,
    )
    .unwrap();
    let mut old_observation = observation(
        &mut state,
        "historical-qualified",
        "qualified_read",
        json!({"result":original_bundle}),
    );
    old_observation.sources = vec![root.clone(), old];
    old_observation.qualified_roots = vec![root];
    old_observation.exposure = Some(Exposure {
        sources: old_observation.sources.clone(),
        policies: vec![],
    });
    state
        .observations
        .insert(old_observation.id.clone(), old_observation.clone());
    let reference = descriptor(&state, &old_observation.id, &a, false)
        .unwrap()
        .reference;
    let last = state.accepted_bindings.last_mut().unwrap();
    last.revision = "binding-two".into();
    last.disposition = f::BindingDisposition::Qualify {
        required: vec![new.clone()],
    };
    last.revision = last.computed_revision().unwrap();
    let mut receipt = observation(
        &mut state,
        "fresh-wrapper",
        "history_record_read",
        json!({"reference":reference,"supply_byte_limit":100000,"record":null}),
    );
    receipt.exposure = old_observation.exposure.clone();
    let corpus = corpus(&state, &a).unwrap();
    let (value, exposure, roots, groups) = supply(
        &state,
        &receipt,
        &a,
        &corpus,
        &Material::default(),
        &mut supply_budget(),
        &Permissions::new(&state, &a).unwrap(),
    )
    .unwrap();
    assert_eq!(value["payload"]["status"], "read");
    let text = value["payload"]["record"]["historical_record"].to_string();
    assert!(text.contains("new-qualification-marker"));
    assert!(!text.contains("old-qualification-marker"));
    assert!(value["current_required_qualifications"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(roots.contains(&new));
    assert_eq!(groups, vec![old_observation.qualified_roots.clone()]);
    assert!(exposure.sources.contains(&new));
    assert_eq!(value["id"], "fresh-wrapper");
    assert_eq!(
        serde_json::to_value(&state.observations[&old_observation.id]).unwrap(),
        serde_json::to_value(old_observation).unwrap()
    );
    let mut zero = f::NavigationBudget::new(f::NavigationLimits {
        calls: 0,
        scanned_bytes: 0,
        inspected_revisions: 0,
        matches: 0,
        result_bytes: 0,
    });
    assert!(supply(
        &state,
        &receipt,
        &a,
        &corpus,
        &Material::default(),
        &mut zero,
        &Permissions::new(&state, &a).unwrap()
    )
    .is_err());
}

#[test]
fn missing_authored_bytes_and_fenced_proposals_do_not_become_executed_history() {
    let (_dir, mut app) = fixture();
    let prepared = prepare(&mut app, "archive-source", WORK, 100);
    tests::submit(
        &mut app,
        &prepared,
        vec![],
        "synthetic authored record",
        101,
    );
    let (_, state) = app.load().unwrap();
    let entries = app
        .store
        .history_archives(Some(&["archive-source".into()]))
        .unwrap()
        .unwrap();
    let mut material = Material::default();
    assert!(material.insert(entries));
    let a = admission(&state.personas[ACTOR], 102);
    let corpus = corpus(&state, &a).unwrap();
    let permissions = Permissions::new(&state, &a).unwrap();
    assert!(decision_record(
        &state,
        ACTOR,
        "archive-source",
        &a,
        &corpus,
        &material,
        &permissions,
        true
    )
    .is_some());
    let (proof, object) = material.archives.get_mut("archive-source").unwrap();
    object.body["report"]["status"] = json!("character_only");
    object.body["report"]["fenced"] = json!(["unexecuted-proposal"]);
    let archive_ref = object.exact_ref().unwrap();
    for reference in &mut proof.objects {
        if reference.id == archive_ref.id {
            *reference = archive_ref.clone();
        }
    }
    let candidate = decision_record(
        &state,
        ACTOR,
        "archive-source",
        &a,
        &corpus,
        &material,
        &permissions,
        true,
    )
    .unwrap();
    assert_eq!(candidate.body["authorship_establishes_execution"], false);
    assert!(!candidate.body.to_string().contains("unexecuted-proposal"));
    let (proof, object) = material.archives.get_mut("archive-source").unwrap();
    object.body["authored_json"] = Value::Null;
    let archive_ref = object.exact_ref().unwrap();
    for reference in &mut proof.objects {
        if reference.id == archive_ref.id {
            *reference = archive_ref.clone();
        }
    }
    assert!(decision_record(
        &state,
        ACTOR,
        "archive-source",
        &a,
        &corpus,
        &material,
        &permissions,
        true
    )
    .is_none());
}

#[test]
fn denied_and_wrong_digest_archive_reads_never_occupy_reusable_material() {
    let (_dir, mut app) = fixture();
    let origin = prepare(&mut app, "cache-origin", WORK, 100);
    tests::submit(&mut app, &origin, vec![], "synthetic cache source", 101);
    let (_, mut state) = app.load().unwrap();
    let entries = app
        .store
        .history_archives(Some(&["cache-origin".into()]))
        .unwrap()
        .unwrap();
    let mut seed = Material::default();
    assert!(seed.insert(entries));
    let a = admission(&state.personas[ACTOR], 102);
    let corpus = corpus(&state, &a).unwrap();
    let permissions = Permissions::new(&state, &a).unwrap();
    let good = decision_record(
        &state,
        ACTOR,
        "cache-origin",
        &a,
        &corpus,
        &seed,
        &permissions,
        false,
    )
    .unwrap()
    .reference;
    for deny_actor in [false, true] {
        let mut prepared = origin.clone();
        prepared.request.id = format!("denied-cache-{deny_actor}");
        if deny_actor {
            prepared.request.persona = "persona-0002".into();
        }
        let mut reference = good.clone();
        if !deny_actor {
            if let p::HistoryRecordReference::AdoptedDecision { sha256, .. } = &mut reference {
                *sha256 = "0".repeat(64);
            }
        }
        let mut material = Material::default();
        let mut navigation = f::NavigationBudget::new(prepared.navigation);
        let mut planning = PlanningExposure::new(Some(&Exposure::default()));
        apply(
            &mut state,
            &prepared,
            &p::Operation::HistoryRecordRead {
                id: "denied-read".into(),
                reference,
            },
            &mut navigation,
            0,
            &Exposure::default(),
            &mut planning,
            102,
            &mut app.store,
            &mut material,
        )
        .unwrap();
        assert!(material.archives.is_empty());
        let receipt = &state.observations
            [&observation_id(&prepared.request.id, "history_record_read", "denied-read")];
        assert_eq!(receipt.payload["status"], "unavailable");
    }
}

#[test]
fn membership_cap_failure_retains_consulted_history_restrictions() {
    let (_dir, mut app) = fixture();
    let prepared = prepare(&mut app, "private-cap", WORK, 100);
    let (_, mut state) = app.load().unwrap();
    let mut policy = state.config.policies.active_policies[0].clone();
    policy.id = "cap-private".into();
    state.config.policies.active_policies.push(policy.clone());
    let protected = Exposure {
        sources: vec![],
        policies: vec![ScopedPolicy {
            source: "history-fixture-core".into(),
            revision: state.sources["history-fixture-core"].head.clone(),
            policy,
        }],
    };
    for n in 0..900 {
        let id = format!("history-{n:04}-{}", "x".repeat(220));
        let mut value = observation(&mut state, &id, "fixture", json!({"value":"bounded"}));
        value.exposure = Some(protected.clone());
        state.observations.insert(id, value);
    }
    let a = admission(&state.personas[ACTOR], 100);
    let permissions = Permissions::new(&state, &a).unwrap();
    let corpus = corpus(&state, &a).unwrap();
    let mut consulted = Exposure::default();
    let result = query(
        &state,
        &prepared,
        "private-cap-query",
        p::HistoryRecordKind::Observation,
        &f::SearchMode::Listing,
        128,
        None,
        &a,
        &permissions,
        &corpus,
        &Material::default(),
        &mut f::NavigationBudget::new(prepared.navigation),
        100000,
        &mut consulted,
    )
    .unwrap();
    assert_eq!(result.payload["status"], "cursor_retention_unavailable");
    assert!(consulted.policies.contains(&protected.policies[0]));
    state
        .config
        .policies
        .active_policies
        .last_mut()
        .unwrap()
        .revoked_at_unix = Some(100);
    assert!(!exposure_permitted(&state, &consulted, &a));
}

#[test]
fn unqualified_history_resupply_consumes_one_shared_nonzero_envelope() {
    let (_dir, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    observation(
        &mut state,
        "repeat-unqualified",
        "fixture",
        json!({"body":"x".repeat(10000)}),
    );
    let a = admission(&state.personas[ACTOR], 100);
    let reference = descriptor(&state, "repeat-unqualified", &a, false)
        .unwrap()
        .reference;
    let receipt = observation(
        &mut state,
        "repeat-wrapper",
        "history_record_read",
        json!({"reference":reference,"supply_byte_limit":30000}),
    );
    let permissions = Permissions::new(&state, &a).unwrap();
    let corpus = corpus(&state, &a).unwrap();
    let mut budget = supply_budget();
    let mut successful = 0;
    for _ in 0..16 {
        if let Ok((value, _, _, _)) = supply(
            &state,
            &receipt,
            &a,
            &corpus,
            &Material::default(),
            &mut budget,
            &permissions,
        ) {
            assert_eq!(value["payload"]["status"], "read");
            successful += 1;
        } else {
            break;
        }
    }
    assert!(successful > 0 && successful < 16);
    assert!(budget.used().scanned_bytes > 0 && budget.used().inspected_revisions > 0);
}

#[test]
fn historical_single_root_chain_passes_fresh_dispatch_guard_with_original_grouping() {
    let (_dir, mut app) = fixture();
    let (head, mut state) = app.load().unwrap();
    let mut next = None;
    for n in (0..23).rev() {
        let id = format!("chain-{n:02}");
        source(&mut state, &id, "Synthetic qualification chain entry");
        let managed = &mut state.sources.get_mut(&id).unwrap().revisions[0];
        managed.record.qualifications = next.iter().cloned().collect();
        managed.record.revision = managed.record.computed_revision().unwrap();
        let reference = managed.record.reference(f::Representation::Utf8);
        state.sources.get_mut(&id).unwrap().head = reference.revision.clone();
        next = Some(reference);
    }
    let root = next.unwrap();
    let a = admission(&state.personas[ACTOR], 100);
    let original_bundle = f::assemble(
        &corpus(&state, &a).unwrap(),
        std::slice::from_ref(&root),
        &[],
        &[],
        &a,
        state.config.context_bytes,
        0,
        0,
    )
    .unwrap();
    assert_eq!(original_bundle.entries.len(), 23);
    let all = original_bundle
        .entries
        .iter()
        .map(|entry| entry.reference.clone())
        .collect::<Vec<_>>();
    // The prior bug used the entire returned closure as separately quota-divided
    // roots. Demonstrate that it fails while the original one-root group fits.
    assert!(f::assemble(
        &corpus(&state, &a).unwrap(),
        &all,
        &[],
        &[],
        &a,
        usize::MAX,
        0,
        0
    )
    .is_err());
    let mut old = observation(
        &mut state,
        "old-chain-read",
        "qualified_read",
        json!({"result":original_bundle}),
    );
    old.sources = all;
    old.qualified_roots = vec![root];
    old.exposure = Some(Exposure {
        sources: old.sources.clone(),
        policies: vec![],
    });
    state.observations.insert(old.id.clone(), old.clone());
    let reference = descriptor(&state, &old.id, &a, false).unwrap().reference;
    let mut receipt = observation(
        &mut state,
        "chain-wrapper",
        "history_record_read",
        json!({"reference":reference,"supply_byte_limit":32768}),
    );
    receipt.exposure = old.exposure;
    state
        .observations
        .insert(receipt.id.clone(), receipt.clone());
    state
        .personas
        .get_mut(ACTOR)
        .unwrap()
        .focus
        .get_mut(WORK)
        .unwrap()
        .pending_observations
        .push(receipt.id.clone());
    app.save(&head, &state).unwrap();
    let prepared = prepare(&mut app, "chain-fresh", WORK, 100);
    let projected = tests::supplied(&prepared, "history_record_read", "chain-wrapper");
    assert_eq!(projected["payload"]["status"], "read");
    assert!(prepared.supplied_observations.contains(&receipt.id));
    assert!(!prepared.supplied_observations.contains(&old.id));
    let (_, admitted) = app.load().unwrap();
    app.current(&admitted, &prepared, 101).unwrap();
}
