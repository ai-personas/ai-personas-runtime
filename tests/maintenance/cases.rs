//! Synthetic mechanism evidence only; no semantic repair or benefit is inferred.
use super::*;

fn deferred(deadline: Option<u64>) -> p::Maintenance {
    p::Maintenance::Deferred {
        reason: "Reconsider the supporting note after an actual check.".into(),
        consequence: "The note is not claimed corrected.".into(),
        owner_or_gap: "a retains this concern.".into(),
        reconsideration: "An explicit later funded request; no automatic retry.".into(),
        deadline_unix: deadline,
    }
}
fn fixture(deadline: Option<u64>) -> Fixture {
    let mut f = Fixture::new(true);
    f.accept();
    let p = f.prepare("defer-maintenance");
    let mut d = decision(&p, true);
    d.asset_maintenance = deferred(deadline);
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    f
}
fn dispose(p: &Prepared, kind: p::MaintenanceDisposition) -> p::Decision {
    let entry = &interface_control(p)["protected_maintenance"][0];
    let mut d = decision(p, true);
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Explicit attributed disposition, not a verified repair.".into(),
        operation_ids: vec!["dispose".into()],
    };
    d.operations.push(p::Operation::MaintenanceDispose {
        id: "dispose".into(),
        scope: p::Scope::Assets {},
        maintenance_id: entry["id"].as_str().unwrap().into(),
        maintenance_revision: entry["revision"].as_str().unwrap().into(),
        disposition: kind,
        reason: "This is my explicit disposition with no independent verification.".into(),
        evidence: Vec::new(),
    });
    d
}

#[test]
fn maintenance_disposition_is_explicit_exact_attributed_and_durable() {
    let mut f = fixture(None);
    let original = f.app.snapshot().unwrap().personas["a"].focus["work"]
        .protected_maintenance
        .clone();
    let p = f.prepare("ordinary-no-change");
    let d = decision(&p, true);
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("ordinary-save");
    let mut d = decision(&p, true);
    d.operations.push(write(
        "save",
        p::Scope::Assets {},
        "new-note",
        "Exact new note, not a maintenance resolution.",
        None,
    ));
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Save only.".into(),
        operation_ids: vec!["save".into()],
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("explicit-addressed");
    let prior = interface_control(&p)["protected_maintenance"][0].clone();
    assert_eq!(prior["resolution"], "unresolved");
    let mut d = dispose(&p, p::MaintenanceDisposition::Addressed);
    let actual_write = interface_control(&p)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "accepted_file_write")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    if let p::Operation::MaintenanceDispose { evidence, .. } = &mut d.operations[0] {
        evidence.push(actual_write);
    }
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    let focus = &state.personas["a"].focus["work"];
    assert_eq!(focus.protected_maintenance, original);
    let receipt = &focus.maintenance_dispositions[0];
    assert_eq!(receipt.persona, "a");
    assert_eq!(receipt.work, "work");
    assert_eq!(receipt.invocation, p.request.id);
    assert_eq!(receipt.owner_epoch, p.owner_epoch);
    assert_eq!(receipt.authority.version, p.work_version);
    assert_eq!(receipt.operation, d.operations[0]);
    assert_ne!(receipt.revision, prior["revision"]);
    let root = f.app.state_root().to_owned();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    let p = f.prepare("after-reopen");
    let shown = &interface_control(&p)["protected_maintenance"][0];
    assert_eq!(shown["resolution"], "addressed");
    assert_eq!(shown["disposition"]["semantic_truth_verified"], false);
    assert_eq!(shown["disposition"]["warning_resolved"], false);
    assert_eq!(shown["original_revision"], prior["original_revision"]);
    let mut stale = dispose(&p, p::MaintenanceDisposition::Dismissed);
    if let p::Operation::MaintenanceDispose {
        maintenance_revision,
        ..
    } = &mut stale.operations[0]
    {
        *maintenance_revision = prior["revision"].as_str().unwrap().into();
    }
    let before = persisted_state(&f.app);
    assert_eq!(f.run(&p, &stale).status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn maintenance_disposition_rejects_unobserved_stale_wrong_scope_evidence_and_early_expiry_atomically(
) {
    for failure in [
        "id",
        "revision",
        "scope",
        "evidence",
        "duplicate",
        "early-expiry",
        "failed-close",
    ] {
        let mut f = fixture(Some((NOW + 500) as u64));
        let p = f.prepare("invalid-disposition");
        let mut d = dispose(&p, p::MaintenanceDisposition::Dismissed);
        if let p::Operation::MaintenanceDispose {
            maintenance_id,
            maintenance_revision,
            scope,
            evidence,
            disposition,
            ..
        } = &mut d.operations[0]
        {
            match failure {
                "id" => *maintenance_id = "someone-elses-record".into(),
                "revision" => *maintenance_revision = "0".repeat(64),
                "scope" => *scope = p::Scope::Fragments {},
                "evidence" => evidence.push("invented".into()),
                "early-expiry" | "failed-close" => {
                    *disposition = p::MaintenanceDisposition::Expired
                }
                _ => (),
            }
        }
        if failure == "scope" {
            d.fragment_maintenance = d.asset_maintenance.clone();
            d.asset_maintenance = p::Maintenance::NoChange {
                reason: "No asset transition.".into(),
            };
        }
        if failure == "duplicate" {
            let mut other = d.operations[0].clone();
            if let p::Operation::MaintenanceDispose { id, .. } = &mut other {
                *id = "second".into();
            }
            d.operations.push(other);
            if let p::Maintenance::Change { operation_ids, .. } = &mut d.asset_maintenance {
                operation_ids.push("second".into());
            }
        }
        d.operations.push(write(
            "save-companion",
            p::Scope::Assets {},
            "must-not-save",
            "Invalid companion must roll back.",
            None,
        ));
        match &mut d.asset_maintenance {
            p::Maintenance::Change { operation_ids, .. } => {
                operation_ids.push("save-companion".into())
            }
            _ => {
                d.asset_maintenance = p::Maintenance::Change {
                    reason: "Companion.".into(),
                    operation_ids: vec!["save-companion".into()],
                }
            }
        }
        if failure == "failed-close" {
            d.operations.push(effect(
                "invalid-close",
                WorkAction::CloseGoal {
                    release: "absent".into(),
                },
                &[],
            ));
        }
        let before = persisted_state(&f.app);
        let result = f.run(&p, &d);
        assert_eq!(
            result.status,
            AdoptionStatus::Held,
            "{failure}: {:?}",
            result.reason
        );
        assert_eq!(persisted_state(&f.app), before, "{failure}");
    }
}

#[test]
fn maintenance_disposition_supports_dismissed_unresolved_and_expired_without_repair_claims() {
    for kind in [
        p::MaintenanceDisposition::Dismissed,
        p::MaintenanceDisposition::Unresolved,
        p::MaintenanceDisposition::Expired,
    ] {
        let mut f = fixture(Some((NOW - 1) as u64));
        let p = f.prepare("explicit-disposition");
        let mut d = dispose(&p, kind);
        d.continuation = p::Continuation::Stop {
            reason: "No further admission requested.".into(),
        };
        assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
        let state = f.app.snapshot().unwrap();
        let focus = &state.personas["a"].focus["work"];
        assert_eq!(focus.protected_maintenance[0]["resolution"], "unresolved");
        assert_eq!(focus.maintenance_dispositions.len(), 1);
        assert!(!focus.permit);
        assert!(state.semantic_warnings.is_empty());
        assert!(matches!(
            state.board.work(&"work".into()).unwrap().phase,
            w::Phase::Open
        ));
    }
}

#[test]
fn maintenance_disposition_requires_current_permitted_original_and_does_not_reveal_withheld_identity(
) {
    let mut f = Fixture::new(true);
    let private = record(
        "private-maintenance-input",
        f::Scope::Assets,
        "Private maintenance basis.",
    );
    f.app
        .import_files("operator", "a", vec![private.clone()], false, NOW)
        .unwrap();
    f.accept();
    let p = f.prepare("select-maintenance-basis");
    let mut d = decision(&p, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Use actual supplied input next.".into(),
        references: vec![reference(&private)],
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("defer-with-private-input");
    let mut d = decision(&p, true);
    d.asset_maintenance = deferred(None);
    d.next_assets = p::SelectionIntent::Clear {
        reason: "No longer select this input.".into(),
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("remember-permitted-record");
    let operation = dispose(&p, p::MaintenanceDisposition::Dismissed)
        .operations
        .remove(0);
    assert_eq!(
        f.run(&p, &decision(&p, true)).status,
        AdoptionStatus::Adopted
    );
    f.app
        .withdraw_source("operator", &private.source, "Revoke basis access.")
        .unwrap();
    let p = f.prepare("after-basis-withdrawal");
    let shown = &interface_control(&p)["protected_maintenance"][0];
    assert_eq!(shown["status"], "withheld_unavailable");
    assert!(shown.get("id").is_none());
    let mut d = decision(&p, true);
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Attempt a remembered unavailable record.".into(),
        operation_ids: vec!["dispose".into()],
    };
    d.operations.push(operation);
    let before = persisted_state(&f.app);
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn maintenance_disposition_work_end_is_honest_unresolved_without_rewriting_history() {
    for canceled in [false, true] {
        let mut f = fixture(None);
        let before = f.app.snapshot().unwrap().personas["a"].focus["work"]
            .protected_maintenance
            .clone();
        f.app
            .closeout("operator", "work", canceled, "Explicit synthetic ending.")
            .unwrap();
        let state = f.app.snapshot().unwrap();
        let focus = &state.personas["a"].focus["work"];
        assert_eq!(focus.protected_maintenance, before);
        assert!(focus.maintenance_dispositions.is_empty());
        assert!(!focus.permit);
        let end = state.operator_events.last().unwrap();
        assert_eq!(end["kind"], "closeout");
        assert_eq!(end["remaining_maintenance"], "unresolved_at_work_end");
        assert_eq!(end["resolves_maintenance"], false);
        assert_eq!(end["persona_judgment_inferred"], false);
    }
}

#[test]
fn maintenance_disposition_does_not_block_an_independent_valid_goal_or_infer_fix_from_close() {
    let (mut f, p, key) = model_evidence_fixture("full", false);
    let mut d = decision(&p, true);
    d.fragment_maintenance = deferred(None);
    d.operations.push(model_evidence_operation(&key));
    d.operations.extend(model_judgment_and_shared(&key));
    d.operations.push(effect(
        "release",
        WorkAction::Release {
            id: "model-note-release".into(),
            shared: "model-note-shared".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("deliver-result");
    let mut d = decision(&p, true);
    d.operations.push(effect(
        "deliver",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let original = f.app.snapshot().unwrap().personas["a"].focus["work"]
        .protected_maintenance
        .clone();
    let p = f.prepare("close-result");
    let mut d = decision(&p, true);
    d.operations.push(effect(
        "close",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    assert!(state.board.completion_supported(&"work".into()));
    assert_eq!(
        state.personas["a"].focus["work"].protected_maintenance,
        original
    );
    assert!(state.personas["a"].focus["work"]
        .maintenance_dispositions
        .is_empty());
    let end = state
        .operator_events
        .iter()
        .find(|v| v["kind"] == "maintenance_work_end")
        .unwrap();
    assert_eq!(end["remaining_maintenance"], "unresolved_at_work_end");
    assert_eq!(
        end["phase"],
        serde_json::to_value(&state.board.work(&"work".into()).unwrap().phase).unwrap()
    );
    assert_eq!(end["resolves_maintenance"], false);
}

#[test]
fn maintenance_disposition_never_resolves_an_independent_semantic_warning() {
    let mut f = Fixture::new(true);
    let target = record("warning-target", f::Scope::Assets, "Exact warning subject.");
    let basis = record("warning-basis", f::Scope::Assets, "Exact warning basis.");
    f.app
        .import_files(
            "operator",
            "a",
            vec![target.clone(), basis.clone()],
            false,
            NOW,
        )
        .unwrap();
    f.accept();
    let p = f.prepare("select-warning-basis");
    let mut d = decision(&p, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Supply exact warning subject and basis.".into(),
        references: vec![reference(&target), reference(&basis)],
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("warning-and-deferral");
    let mut d = decision(&p, true);
    d.fragment_maintenance = deferred(None);
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Explicit independent warning.".into(),
        operation_ids: vec!["warning".into()],
    };
    d.operations.push(p::Operation::WarningAdopt {
        id: "warning".into(),
        warning_id: "warning-record".into(),
        scope: p::Scope::Assets {},
        targets: vec![reference(&target)],
        uses: p::WarningUses::AllRepresentations {},
        meaning: "Attributable fixture qualification, not a semantic verdict.".into(),
        basis: vec![reference(&basis)],
        disposition: p::WarningDisposition::Qualify {
            required: vec![reference(&basis)],
        },
        policy_id: "local-policy".into(),
        depends_on: Vec::new(),
    });
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let warnings = serde_json::to_value(f.app.snapshot().unwrap().semantic_warnings).unwrap();
    assert_eq!(warnings.as_object().unwrap().len(), 1);
    let p = f.prepare("dispose-fragment-concern");
    let mut d = dispose(&p, p::MaintenanceDisposition::Addressed);
    if let p::Operation::MaintenanceDispose { scope, .. } = &mut d.operations[0] {
        *scope = p::Scope::Fragments {};
    }
    d.fragment_maintenance = d.asset_maintenance.clone();
    d.asset_maintenance = p::Maintenance::NoChange {
        reason: "Warning remains separately active.".into(),
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    assert_eq!(
        serde_json::to_value(f.app.snapshot().unwrap().semantic_warnings).unwrap(),
        warnings
    );
    f.app
        .closeout(
            "operator",
            "work",
            false,
            "Explicit bounded closeout, not warning resolution.",
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(f.app.snapshot().unwrap().semantic_warnings).unwrap(),
        warnings
    );
}

#[test]
fn maintenance_disposition_requires_scope_authorship_without_suppressing_blocked_recording() {
    for fragments in [false, true] {
        let mut primary = seed("a");
        if fragments {
            primary.grants.fragments = false;
        } else {
            primary.grants.assets = false;
        }
        let mut f = Fixture::configured(true, config(), primary, need());
        f.accept();
        let p = f.prepare("blocked-without-authorship");
        let mut d = decision(&p, true);
        let blocked = p::Maintenance::Blocked {
            reason: "This scope has no authoring grant.".into(),
            consequence: "No mutation or correction is claimed.".into(),
        };
        if fragments {
            d.fragment_maintenance = blocked;
        } else {
            d.asset_maintenance = blocked;
        }
        assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
        let p = f.prepare("disposition-without-authorship");
        let mut d = dispose(&p, p::MaintenanceDisposition::Dismissed);
        if fragments {
            if let p::Operation::MaintenanceDispose { scope, .. } = &mut d.operations[0] {
                *scope = p::Scope::Fragments {};
            }
            d.fragment_maintenance = d.asset_maintenance.clone();
            d.asset_maintenance = p::Maintenance::NoChange {
                reason: "No asset changes.".into(),
            };
        }
        let before = persisted_state(&f.app);
        let report = f.run(&p, &d);
        assert_eq!(report.status, AdoptionStatus::Held);
        assert!(report
            .reason
            .unwrap()
            .contains("authorship scope not granted"));
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn independent_unavailable_latest_disposition_does_not_assert_unresolved() {
    let mut f = fixture(None);
    let secret = record(
        "later-private-disposition",
        f::Scope::Assets,
        "Private later basis.",
    );
    f.app
        .import_files("operator", "a", vec![secret.clone()], false, NOW)
        .unwrap();
    let p = f.prepare("select-later-basis");
    let original = interface_control(&p)["protected_maintenance"][0].clone();
    let mut d = decision(&p, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Use permitted later basis.".into(),
        references: vec![reference(&secret)],
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("address-with-later-basis");
    assert!(p.prompt.contains("Private later basis."));
    let mut d = dispose(&p, p::MaintenanceDisposition::Addressed);
    d.next_assets = p::SelectionIntent::Clear {
        reason: "No further optional need.".into(),
    };
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    assert!(matches!(
        state.personas["a"].focus["work"].maintenance_dispositions[0].operation,
        p::Operation::MaintenanceDispose {
            disposition: p::MaintenanceDisposition::Addressed,
            ..
        }
    ));
    assert_eq!(
        state.personas["a"].focus["work"].protected_maintenance[0]["resolution"],
        "unresolved"
    );
    f.app
        .withdraw_source(
            "operator",
            &secret.source,
            "Withdraw only later disposition basis.",
        )
        .unwrap();
    let p = f.prepare("after-later-basis-withdrawal");
    let shown = interface_control(&p)["protected_maintenance"][0].clone();
    assert_eq!(shown["status"], "withheld_unavailable");
    assert!(shown.get("id").is_none());
    assert_eq!(original["resolution"], "unresolved");
    assert_ne!(
        shown["resolution"], "unresolved",
        "Unavailable latest disposition must not be reported as an explicit unresolved disposition"
    );
    assert!(shown.get("resolution").is_none());
    assert!(shown.get("revision").is_none());
    let after = f.app.snapshot().unwrap();
    assert_eq!(
        after.personas["a"].focus["work"].protected_maintenance,
        state.personas["a"].focus["work"].protected_maintenance
    );
    assert_eq!(
        serde_json::to_value(&after.personas["a"].focus["work"].maintenance_dispositions).unwrap(),
        serde_json::to_value(&state.personas["a"].focus["work"].maintenance_dispositions).unwrap()
    );
}

#[test]
fn maximum_legal_write_identity_emits_citable_supplied_maintenance_evidence() {
    let mut f = fixture(None);
    let invocation = "i".repeat(128);
    let operation = "w".repeat(128);
    let p = f.prepare(&invocation);
    let mut d = decision(&p, true);
    d.operations.push(write(
        &operation,
        p::Scope::Assets {},
        "long-id-note",
        "Actual saved note.",
        None,
    ));
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Save a note without implicitly resolving maintenance.".into(),
        operation_ids: vec![operation.clone()],
    };
    p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let p = f.prepare("cite-long-id-write");
    let control = interface_control(&p);
    let receipt = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "accepted_file_write")
        .unwrap();
    assert_eq!(receipt["invocation"], invocation);
    assert_eq!(receipt["payload"]["operation_id"], operation);
    let actual_id = receipt["id"].as_str().unwrap().to_owned();
    let mut d = dispose(&p, p::MaintenanceDisposition::Addressed);
    if let p::Operation::MaintenanceDispose { evidence, .. } = &mut d.operations[0] {
        evidence.push(actual_id.clone());
    }
    p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes)
        .expect("a generated, supplied observation must fit the strict evidence wire bound");
    assert!(actual_id.len() <= p::MAX_ID_BYTES);
    assert!(p.supplied_observations.contains(&actual_id));
    assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    assert_eq!(state.observations[&actual_id].id, actual_id);
    assert_eq!(
        state.personas["a"].focus["work"]
            .maintenance_dispositions
            .len(),
        1
    );
}

#[test]
fn legacy_long_observation_evidence_requires_exact_actual_supply() {
    for supplied in [true, false] {
        let mut f = fixture(None);
        let invocation = "i".repeat(128);
        let operation = "w".repeat(128);
        let p = f.prepare(&invocation);
        let mut d = decision(&p, true);
        d.operations.push(write(
            &operation,
            p::Scope::Assets {},
            "legacy-note",
            "Actual saved note.",
            None,
        ));
        d.asset_maintenance = p::Maintenance::Change {
            reason: "Save without an implicit maintenance disposition.".into(),
            operation_ids: vec![operation.clone()],
        };
        assert_eq!(f.run(&p, &d).status, AdoptionStatus::Adopted);
        let legacy =
            format!("11:observation:128:{invocation}:19:accepted_file_write:128:{operation}");
        assert_eq!(legacy.len(), 303);
        // Construct an explicitly synthetic retained pre-upgrade state through
        // ordinary Store CAS. Production code never rewrites observation IDs.
        let root = f.app.state_root().to_owned();
        drop(f.app);
        let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        let old = host.read("runtime-state-v1").unwrap().unwrap();
        let mut object = old.clone();
        object.revision += 1;
        let observations = object.body["observations"].as_object_mut().unwrap();
        let current = observations
            .iter()
            .find(|(_, v)| v["kind"] == "accepted_file_write")
            .unwrap()
            .0
            .clone();
        let mut retained = observations.remove(&current).unwrap();
        retained["id"] = json!(legacy);
        observations.insert(legacy.clone(), retained.clone());
        let pending = object.body["personas"]["a"]["focus"]["work"]["pending_observations"]
            .as_array_mut()
            .unwrap();
        for id in pending.iter_mut() {
            if id == &json!(current) {
                *id = json!(legacy);
            }
        }
        if !supplied {
            pending.retain(|id| id != &json!(legacy));
        }
        host.compare_and_swap_batch(vec![s::Change {
            expected_revision: Some(old.revision),
            object,
            depends_on: Vec::new(),
        }])
        .unwrap();
        drop(host);
        f.app = Runtime::open(&root).unwrap();
        let p = f.prepare("cite-legacy-evidence");
        assert_eq!(p.supplied_observations.contains(&legacy), supplied);
        if supplied {
            assert!(interface_control(&p)["actual_observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["id"] == legacy));
        }
        let mut d = dispose(&p, p::MaintenanceDisposition::Addressed);
        if let p::Operation::MaintenanceDispose { evidence, .. } = &mut d.operations[0] {
            evidence.push(legacy.clone());
        }
        p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
        let before = persisted_state(&f.app);
        let report = f.run(&p, &d);
        assert_eq!(
            report.status,
            if supplied {
                AdoptionStatus::Adopted
            } else {
                AdoptionStatus::Held
            }
        );
        if !supplied {
            assert_eq!(persisted_state(&f.app), before);
        }
        assert_eq!(
            serde_json::to_value(&f.app.snapshot().unwrap().observations[&legacy]).unwrap(),
            retained
        );
    }
}

fn message_evidence_fixture() -> Fixture {
    let mut f = Fixture::configured(true, config(), seed("a"), interface_peer_need(false));
    f.accept();
    initialize_interface_peer(&mut f);
    let prepared = f
        .app
        .prepare(
            PrepareRequest {
                id: "peer-defer".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let mut d = decision(&prepared, false);
    let core = interface_control(&prepared);
    let offer = &core["own_pending_offers"][0];
    d.operations.push(p::Operation::Responsibility {
        id: offer["id"].as_str().unwrap().into(),
        work_id: "work".into(),
        terms_revision: offer["terms_revision"].as_str().unwrap().into(),
        disposition: "accept".into(),
        reason: "Accept this exact contribution.".into(),
    });
    d.asset_maintenance = deferred(None);
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    f
}

#[test]
fn long_message_observation_is_citable_by_its_participating_recipient() {
    let mut f = message_evidence_fixture();
    let invocation = "i".repeat(128);
    let operation = "m".repeat(128);
    let prepared = f.prepare(&invocation);
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Communicate {
        id: operation,
        recipient: "b".into(),
        subject: "An attributed note".into(),
        text: "Observed message content, not an automatic repair.".into(),
    });
    p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let received = f
        .app
        .prepare(
            PrepareRequest {
                id: "peer-cite-message".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let control = interface_control(&received);
    let observation = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "received_local_persona_message")
        .unwrap();
    assert_eq!(observation["actor"], "b");
    assert_eq!(observation["payload"]["sender"], "a");
    assert_eq!(observation["payload"]["recipient"], "b");
    assert_eq!(observation["invocation"], invocation);
    let id = observation["id"].as_str().unwrap().to_owned();
    let mut d = dispose(&received, p::MaintenanceDisposition::Addressed);
    if let p::Operation::MaintenanceDispose { evidence, .. } = &mut d.operations[0] {
        evidence.push(id.clone());
    }
    p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes)
        .expect("supplied message observation must be usable in a complete strict Decision");
    assert!(id.len() <= p::MAX_ID_BYTES);
    assert_eq!(f.run(&received, &d).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    assert_eq!(state.messages[&id].sender, "a");
    assert_eq!(state.messages[&id].recipient, "b");
    assert_eq!(state.messages[&id].invocation, invocation);
    assert_eq!(state.observations[&id].actor, "b");
    assert_eq!(
        state.personas["b"].focus["work"]
            .maintenance_dispositions
            .len(),
        1
    );
}

#[test]
fn legacy_long_message_evidence_preserves_identity_and_requires_actual_supply() {
    for operation_bytes in [128, 256] {
        for supplied in [true, false] {
            let mut f = message_evidence_fixture();
            let invocation = "i".repeat(128);
            let operation = "m".repeat(operation_bytes);
            let prepared = f.prepare(&invocation);
            let mut d = decision(&prepared, true);
            d.operations.push(p::Operation::Communicate {
                id: operation.clone(),
                recipient: "b".into(),
                subject: "A retained message".into(),
                text: "An actual attributed local message.".into(),
            });
            p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
            assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
            let suffix = format!("9:operation:{operation_bytes}:{operation}");
            let legacy = format!("7:message:128:{invocation}:{}:{suffix}:1:b", suffix.len());
            if operation_bytes == 128 {
                assert_eq!(legacy.len(), 295);
            }
            // Synthetic pre-upgrade identity fixture, not a production migration.
            let root = f.app.state_root().to_owned();
            drop(f.app);
            let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
            let old = host.read("runtime-state-v1").unwrap().unwrap();
            let mut object = old.clone();
            object.revision += 1;
            let messages = object.body["messages"].as_object_mut().unwrap();
            let current = messages.keys().next().unwrap().clone();
            let mut message = messages.remove(&current).unwrap();
            message["id"] = json!(legacy);
            messages.insert(legacy.clone(), message.clone());
            let observations = object.body["observations"].as_object_mut().unwrap();
            let mut observation = observations.remove(&current).unwrap();
            observation["id"] = json!(legacy);
            observation["payload"]["id"] = json!(legacy);
            observations.insert(legacy.clone(), observation.clone());
            let pending = object.body["personas"]["b"]["focus"]["work"]["pending_observations"]
                .as_array_mut()
                .unwrap();
            for id in pending.iter_mut() {
                if id == &json!(current) {
                    *id = json!(legacy);
                }
            }
            if !supplied {
                pending.retain(|id| id != &json!(legacy));
            }
            host.compare_and_swap_batch(vec![s::Change {
                expected_revision: Some(old.revision),
                object,
                depends_on: vec![],
            }])
            .unwrap();
            drop(host);
            f.app = Runtime::open(&root).unwrap();
            let prepared = f
                .app
                .prepare(
                    PrepareRequest {
                        id: "cite-retained-message".into(),
                        persona: "b".into(),
                        work: "work".into(),
                        profile: Profile::Synthetic,
                        mode: Mode::Ordinary,
                    },
                    NOW,
                )
                .unwrap();
            assert_eq!(prepared.supplied_observations.contains(&legacy), supplied);
            let mut d = dispose(&prepared, p::MaintenanceDisposition::Addressed);
            if let p::Operation::MaintenanceDispose { evidence, .. } = &mut d.operations[0] {
                evidence.push(legacy.clone());
            }
            p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
            let before = persisted_state(&f.app);
            let report = f.run(&prepared, &d);
            assert_eq!(
                report.status,
                if supplied {
                    AdoptionStatus::Adopted
                } else {
                    AdoptionStatus::Held
                }
            );
            if !supplied {
                assert_eq!(persisted_state(&f.app), before);
            }
            let state = f.app.snapshot().unwrap();
            assert_eq!(
                serde_json::to_value(&state.messages[&legacy]).unwrap(),
                message
            );
            assert_eq!(
                serde_json::to_value(&state.observations[&legacy]).unwrap(),
                observation
            );
        }
    }
}
