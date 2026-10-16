//! Synthetic Store fixtures only. No provider, credential or external state use.
use super::*;

fn value<T: Serialize>(item: &T) -> Value {
    serde_json::to_value(item).unwrap()
}

fn zero_navigation() -> f::NavigationLimits {
    f::NavigationLimits {
        calls: 0,
        scanned_bytes: 0,
        inspected_revisions: 0,
        matches: 0,
        result_bytes: 0,
    }
}

fn times(n: f::NavigationLimits, count: usize) -> f::NavigationLimits {
    f::NavigationLimits {
        calls: n.calls * count,
        scanned_bytes: n.scanned_bytes * count,
        inspected_revisions: n.inspected_revisions * count,
        matches: n.matches * count,
        result_bytes: n.result_bytes * count,
    }
}

fn fixture() -> (tempfile::TempDir, Runtime) {
    let directory = tempfile::tempdir().unwrap();
    let working = directory.path().join("working");
    fs::create_dir(&working).unwrap();
    let provider = serde_json::from_value(json!({
        "mode":"platform_api_key", "model":"fixture-model", "effort":"high",
        "credentials":{"kind":"api_key", "key_env":{"env":"FIXTURE_UNUSED_KEY"},
            "account_env":{"env":"FIXTURE_UNUSED_ACCOUNT"}},
        "evidence":{"mode":"platform_api_key", "model":"fixture-model", "efforts":["high"],
            "account_env":{"env":"FIXTURE_UNUSED_ACCOUNT"}, "account_sha256":"0".repeat(64),
            "transport_profile":"platform_responses_v2", "catalog_evidence_sha256":"1".repeat(64),
            "structured_output_evidence_sha256":"2".repeat(64)},
        "limits":{"timeout_ms":1000, "max_prompt_bytes":196608, "max_schema_bytes":65536,
            "max_schema_nodes":10000,"max_schema_depth":64,"max_request_bytes":400000,
            "max_response_bytes":32768,"max_stream_bytes":1048576,
            "max_event_bytes":131072,"max_events":10000}
    }))
    .unwrap();
    let mut app = setup(
        &directory.path().join("state"),
        "fixture-human",
        Setup {
            cohort: 2,
            owner: 1,
            destination: "fixture-local".into(),
            provider,
            working_directory: working,
            share_cohort: true,
            max_steps: 1,
            max_considerations: 4,
            max_work_attempts: 32,
            max_total_steps: 2,
            max_works: 1,
        },
        true,
    )
    .unwrap();
    start(&mut app, "Preserve this existing fixture need").unwrap();
    let (head, mut saved) = load(&mut app).unwrap();
    saved.steps = 2;
    save(&mut app, Some(&head), &saved).unwrap();
    let (head, mut state) = app.load().unwrap();
    let meta = state.work_meta.get_mut("work-00000001").unwrap();
    meta.attempts_left = 0;
    meta.navigation_left = zero_navigation();
    for persona in state.personas.values_mut() {
        let focus = persona.focus.get_mut("work-00000001").unwrap();
        focus.permit = false;
        focus.explicit_resume_permit = false;
        focus.ordinary_continuation = false;
        focus.continuation = Some(p::Continuation::Stop {
            reason: "Fixture voluntary stop".into(),
        });
    }
    state
        .personas
        .get_mut("persona-0002")
        .unwrap()
        .failed_attempt_hold = Some(failed_attempt::FailedAttemptHold {
        invocation: "fixture-prior-failure".into(),
        work: "work-00000001".into(),
        acknowledgment: s::ObjectRef {
            id: "fixture-ack".into(),
            revision: 1,
            sha256: "3".repeat(64),
        },
    });
    app.save(&head, &state).unwrap();
    (directory, app)
}

fn request(app: &mut Runtime, work: &str) -> WorkResourceGrantRequest {
    let snapshot = work_resource_grant_snapshot(app, "fixture-human", work).unwrap();
    let expected: WorkResourceGrantFence =
        serde_json::from_value(snapshot["expected"].clone()).unwrap();
    let navigation = serde_json::from_value(snapshot["navigation_per_call"].clone()).unwrap();
    WorkResourceGrantRequest {
        id: "fixture-resource-addition".into(),
        operator: "fixture-human".into(),
        work_id: work.into(),
        reason: "Explicit bounded continuation resources; no resume".into(),
        authorization_reference: "fixture-approval-1".into(),
        authorize_grant: true,
        new_max_total_steps: expected
            .max_total_steps
            .max(expected.steps + expected.remaining.attempts + 3),
        expected,
        increase: WorkResourceIncrease {
            attempts: 3,
            navigation: times(navigation, 3),
        },
        token_attestation: intake_delegation::IntakeTokenAttestation {
            conservative_tokens_used: 100,
            conservative_token_ceiling: 1000,
            per_call_reserve_tokens: 100,
        },
    }
}

fn heads(app: &mut Runtime) -> (s::StoredObject, s::StoredObject) {
    (app.load().unwrap().0, load(app).unwrap().0)
}

#[test]
fn work_grant_preview_and_apply_preserve_stops_holds_and_prior_accounting() {
    let (_directory, mut app) = fixture();
    let uncertain = app
        .store
        .reserve(
            "prior-uncertain",
            s::Resources {
                attempts: 1,
                input_bytes: 10,
                ..Default::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    app.store.settle(&uncertain.id, s::Usage::Unknown).unwrap();
    let reserved = app
        .store
        .reserve(
            "prior-reserved",
            s::Resources {
                attempts: 1,
                ..Default::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    let reservations = (
        app.store.reservation(&uncertain.id).unwrap(),
        app.store.reservation(&reserved.id).unwrap(),
    );
    let req = request(&mut app, "work-00000001");
    let before = heads(&mut app);
    let ledger_before = app.store.ledger().unwrap();
    let preview = preview_work_resource_grant(&mut app, &req, 100).unwrap();
    assert_eq!(heads(&mut app), before);
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
    let preview_ledger = app.store.ledger().unwrap();
    assert_eq!(preview_ledger.closeout, ledger_before.closeout);
    assert_eq!(
        preview_ledger.ordinary.reserved,
        ledger_before.ordinary.reserved
    );
    assert_eq!(
        preview_ledger.ordinary.uncertain,
        ledger_before.ordinary.uncertain
    );
    assert_eq!(
        preview_ledger.ordinary.consumed.retained_bytes,
        ledger_before.ordinary.consumed.retained_bytes
    );
    let applied = grant_work_resources(&mut app, &req, 100).unwrap();
    assert!(!applied.replayed);
    assert_eq!(value(&applied.receipt), value(&preview));
    let after = heads(&mut app);
    let mut normalized = after.0.body.clone();
    normalized["work_meta"][&req.work_id]["attempts_left"] = json!(0);
    normalized["work_meta"][&req.work_id]["navigation_left"] = json!(zero_navigation());
    normalized["operator_events"] = before.0.body["operator_events"].clone();
    assert_eq!(normalized, before.0.body);
    let mut launch = after.1.body.clone();
    launch["setup"]["max_total_steps"] = json!(2);
    assert_eq!(launch, before.1.body);
    assert_eq!(after.0.body["work_meta"][&req.work_id]["attempts_left"], 3);
    assert_eq!(after.1.body["setup"]["max_total_steps"], 5);
    assert_eq!(
        app.store.reservation(&uncertain.id).unwrap(),
        reservations.0
    );
    assert_eq!(app.store.reservation(&reserved.id).unwrap(), reservations.1);
    let ledger_after = app.store.ledger().unwrap();
    assert_eq!(
        ledger_after.ordinary.reserved,
        ledger_before.ordinary.reserved
    );
    assert_eq!(
        ledger_after.ordinary.uncertain,
        ledger_before.ordinary.uncertain
    );
    assert_eq!(
        ledger_after.ordinary.consumed.attempts,
        ledger_before.ordinary.consumed.attempts
    );
    assert_eq!(
        ledger_after.ordinary.consumed.input_bytes,
        ledger_before.ordinary.consumed.input_bytes
    );
    assert_eq!(
        ledger_after.ordinary.consumed.output_bytes,
        ledger_before.ordinary.consumed.output_bytes
    );
    assert_eq!(
        ledger_after.closeout.reserved,
        ledger_before.closeout.reserved
    );
    assert_eq!(
        ledger_after.closeout.uncertain,
        ledger_before.closeout.uncertain
    );
    assert_eq!(ledger_after.limits, ledger_before.limits);
    // Resource availability cannot manufacture continuation or clear a hold.
    for actor in ["persona-0001", "persona-0002"] {
        assert!(app
            .prepare(
                PrepareRequest {
                    id: format!("unresumed-{actor}"),
                    persona: actor.into(),
                    work: req.work_id.clone(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                101
            )
            .is_err());
    }
}

#[test]
fn work_grant_exact_replay_survives_later_pause_and_conflicting_ids_fail() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    let first = grant_work_resources(&mut app, &req, 100).unwrap();
    app.pause(
        "fixture-human",
        &req.work_id,
        "Later independent fixture pause",
    )
    .unwrap();
    let before = heads(&mut app);
    let repeated = grant_work_resources(&mut app, &req, 200).unwrap();
    assert!(repeated.replayed);
    assert_eq!(value(&first.receipt), value(&repeated.receipt));
    assert_eq!(heads(&mut app), before);
    let mut changed = req.clone();
    changed.reason.push_str(" changed");
    assert!(grant_work_resources(&mut app, &changed, 200).is_err());
    changed = req;
    changed.id = "new-request-stale-fences".into();
    assert!(grant_work_resources(&mut app, &changed, 200).is_err());
    assert!(work_resource_grant_snapshot(&mut app, "fixture-human", &changed.work_id).is_err());
    assert_eq!(heads(&mut app), before);
}

#[test]
fn work_grant_replay_rejects_forced_mixed_authorization_snapshot() {
    fn replace_authority(app: &mut Runtime, operator: &str, recipients: &[&str]) {
        let (runtime_head, launch_head) = heads(app);
        let mut runtime = runtime_head.clone();
        runtime.revision += 1;
        runtime.body["config"]["local_recipients"] = json!(recipients);
        let mut launch = launch_head.clone();
        launch.revision += 1;
        launch.body["operator"] = json!(operator);
        // A genuine atomic generic CAS changes both persisted authorities.
        // No fabricated return values or elapsed-time synchronization are used.
        app.store
            .compare_and_swap_batch(vec![
                s::Change {
                    expected_revision: Some(runtime_head.revision),
                    object: runtime,
                    depends_on: Vec::new(),
                },
                s::Change {
                    expected_revision: Some(launch_head.revision),
                    object: launch,
                    depends_on: Vec::new(),
                },
            ])
            .unwrap();
    }

    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    let original = grant_work_resources(&mut app, &req, 100).unwrap();
    let receipt = app.store.read(&receipt_id(&req.id)).unwrap().unwrap();
    replace_authority(
        &mut app,
        "fixture-other",
        &["fixture-human", "fixture-other"],
    );
    let (_, initial_state) = app.load().unwrap();
    let (_, initial_saved) = load(&mut app).unwrap();
    assert!(
        admission_extension::authorize(&initial_state, &initial_saved, "fixture-human").is_err()
    );

    let mut interleaved = false;
    let result = replay_with_read_hook(&mut app, &req, &digest(&req).unwrap(), |app| {
        replace_authority(app, "fixture-human", &["fixture-other"]);
        interleaved = true;
        let (_, final_state) = app.load()?;
        let (_, final_saved) = load(app)?;
        assert!(
            admission_extension::authorize(&final_state, &final_saved, "fixture-human").is_err()
        );
        // The old runtime and new launch would authorize A even though neither
        // real atomic state did. This is the exact rejected mixed-read case.
        admission_extension::authorize(&initial_state, &final_saved, "fixture-human")?;
        Ok(())
    });
    assert!(interleaved);
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("replay authority changed"));
    assert!(grant_work_resources(&mut app, &req, 200).is_err());
    assert_eq!(app.store.read(&receipt.id).unwrap().unwrap(), receipt);
    let (_, state) = app.load().unwrap();
    let (_, saved) = load(&mut app).unwrap();
    assert_eq!(
        state.work_meta[&req.work_id].attempts_left,
        original.receipt.remaining_after.attempts
    );
    assert_eq!(saved.setup.max_total_steps, req.new_max_total_steps);
    assert_eq!(
        state.operator_events.len(),
        initial_state.operator_events.len()
    );
}

#[test]
fn work_grant_rejects_stale_unauthorized_overflow_and_out_of_config_requests() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    let before = heads(&mut app);
    let mut invalid = Vec::new();
    macro_rules! case {
        ($field:ident $(.$sub:ident)*, $value:expr) => {{
            let mut r = req.clone(); r.$field $(.$sub)* = $value; invalid.push(r);
        }};
    }
    case!(authorize_grant, false);
    case!(operator, "persona-0001".into());
    case!(authorization_reference, " ".into());
    case!(increase.attempts, 0);
    case!(increase.attempts, u64::MAX);
    case!(increase.navigation.calls, 129);
    case!(increase.navigation.result_bytes, usize::MAX);
    case!(expected.runtime.sha256, "0".repeat(64));
    case!(expected.launch.sha256, "0".repeat(64));
    case!(expected.work_epoch, req.expected.work_epoch + 1);
    case!(expected.work_revision, req.expected.work_revision + 1);
    case!(expected.remaining.navigation.calls, usize::MAX);
    case!(expected.remaining.attempts, u64::MAX);
    case!(new_max_total_steps, 6);
    case!(token_attestation.conservative_tokens_used, 950);
    let mut r = req.clone();
    r.increase.attempts = 33;
    r.new_max_total_steps = r.expected.steps + 33;
    invalid.push(r);
    for r in invalid {
        assert!(
            grant_work_resources(&mut app, &r, 100).is_err(),
            "accepted {r:?}"
        );
    }
    assert!(req.validate("other-operator").is_err());
    for path in [
        vec!["extra"],
        vec!["expected", "runtime", "extra"],
        vec!["increase", "repair_attempts"],
        vec!["increase", "navigation", "extra"],
    ] {
        let mut unknown = value(&req);
        let mut pointer = &mut unknown;
        for key in &path[..path.len() - 1] {
            pointer = &mut pointer[*key];
        }
        pointer[path[path.len() - 1]] = json!(1);
        assert!(serde_json::from_value::<WorkResourceGrantRequest>(unknown).is_err());
    }
    assert_eq!(heads(&mut app), before);
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
}

#[test]
fn work_grant_request_rejects_positional_arrays_at_every_object_boundary() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    let before = heads(&mut app);
    let reference = &["id", "revision", "sha256"][..];
    let navigation = &[
        "calls",
        "scanned_bytes",
        "inspected_revisions",
        "matches",
        "result_bytes",
    ][..];
    let boundaries: &[(&str, &[&str])] = &[
        (
            "",
            &[
                "id",
                "operator",
                "work_id",
                "reason",
                "authorization_reference",
                "authorize_grant",
                "expected",
                "increase",
                "new_max_total_steps",
                "token_attestation",
            ],
        ),
        (
            "/expected",
            &[
                "runtime",
                "launch",
                "work_revision",
                "work_epoch",
                "remaining",
                "steps",
                "max_total_steps",
                "max_steps",
            ],
        ),
        ("/expected/runtime", reference),
        ("/expected/launch", reference),
        (
            "/expected/remaining",
            &["attempts", "navigation", "repair_attempts"],
        ),
        ("/expected/remaining/navigation", navigation),
        ("/increase", &["attempts", "navigation"]),
        ("/increase/navigation", navigation),
        (
            "/token_attestation",
            &[
                "conservative_tokens_used",
                "conservative_token_ceiling",
                "per_call_reserve_tokens",
            ],
        ),
    ];
    for (path, keys) in boundaries {
        let mut encoded = value(&req);
        let object = encoded.pointer_mut(path).unwrap();
        let positional = keys.iter().map(|key| object[*key].clone()).collect();
        *object = Value::Array(positional);
        assert!(
            serde_json::from_value::<WorkResourceGrantRequest>(encoded).is_err(),
            "accepted tuple at {path}"
        );
    }
    assert_eq!(heads(&mut app), before);
}

#[test]
fn work_grant_refuses_retained_repair_capacity_and_wrong_original_need_id() {
    for repair in [false, true] {
        let (_directory, mut app) = fixture();
        let mut req = request(&mut app, "work-00000001");
        let (head, mut state) = app.load().unwrap();
        let meta = state.work_meta.get_mut(&req.work_id).unwrap();
        if repair {
            // Ordinary exhaustion can legitimately leave repair capacity. This
            // narrow API refuses it; it must never normalize or spend it.
            state.config.max_repair_attempts = 1;
            meta.repair_attempts_left = 1;
            req.expected.remaining.repair_attempts = 1;
        } else {
            meta.original.id = "different-original-need".into();
        }
        app.save(&head, &state).unwrap();
        let before = heads(&mut app);
        req.expected.runtime = intake_delegation::IntakeReference::of(&before.0).unwrap();
        let error =
            work_resource_grant_snapshot(&mut app, "fixture-human", &req.work_id).unwrap_err();
        assert!(error.to_string().contains(if repair {
            "retained repair allowance"
        } else {
            "original need identity"
        }));
        assert!(grant_work_resources(&mut app, &req, 100).is_err());
        assert_eq!(heads(&mut app), before);
        assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
    }
}

#[test]
fn work_grant_receipt_head_cannot_be_replaced_by_generic_store_cas() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    grant_work_resources(&mut app, &req, 100).unwrap();
    let original = app.store.read(&receipt_id(&req.id)).unwrap().unwrap();
    let before = heads(&mut app);
    let mut replacement = original.clone();
    replacement.revision = 2;
    replacement.body["created_at_unix"] = json!(200);
    assert!(app.store.compare_and_swap(Some(1), replacement).is_err());
    assert_eq!(app.store.read(&original.id).unwrap().unwrap(), original);
    assert_eq!(
        app.store
            .read_exact(&original.exact_ref().unwrap())
            .unwrap(),
        original
    );
    assert_eq!(heads(&mut app), before);
    assert!(grant_work_resources(&mut app, &req, 200).unwrap().replayed);
}

#[test]
fn work_grant_adds_to_balance_without_rewriting_an_adequate_launch_cap() {
    let (_directory, mut app) = fixture();
    let (head, mut saved) = load(&mut app).unwrap();
    saved.setup.max_total_steps = 20;
    save(&mut app, Some(&head), &saved).unwrap();
    let (head, mut state) = app.load().unwrap();
    let navigation = state.config.navigation_per_call;
    state
        .work_meta
        .get_mut("work-00000001")
        .unwrap()
        .attempts_left = 2;
    state
        .work_meta
        .get_mut("work-00000001")
        .unwrap()
        .navigation_left = navigation;
    app.save(&head, &state).unwrap();
    let req = request(&mut app, "work-00000001");
    let before = heads(&mut app);
    let applied = grant_work_resources(&mut app, &req, 100).unwrap();
    assert_eq!(heads(&mut app).1, before.1);
    assert_eq!(applied.receipt.target_launch, before.1.exact_ref().unwrap());
    assert_eq!(applied.receipt.remaining_after.attempts, 5);
    assert_eq!(
        applied.receipt.remaining_after.navigation,
        times(navigation, 4)
    );
    assert!(applied.receipt.carried_episode_bindings.is_empty());
}

#[test]
fn work_grant_rejects_active_or_unlinked_nonterminal_invocations() {
    for pointer in [false, true] {
        let (_directory, mut app) = fixture();
        if pointer {
            let (head, mut state) = app.load().unwrap();
            state
                .personas
                .get_mut("persona-0001")
                .unwrap()
                .active_invocation = Some("fixture-current".into());
            app.save(&head, &state).unwrap();
        }
        let req = request(&mut app, "work-00000001");
        let inv = app
            .store
            .prepare_invocation(s::InvocationSpec {
                id: "fixture-current".into(),
                expected_epoch: app.store.epoch().unwrap(),
                request: json!({"fixture":true}),
                reservation: s::Resources {
                    attempts: 1,
                    input_bytes: 128,
                    ..Default::default()
                },
                class: s::BudgetClass::Ordinary,
            })
            .unwrap();
        let before = heads(&mut app);
        assert!(preview_work_resource_grant(&mut app, &req, 100).is_err());
        assert!(grant_work_resources(&mut app, &req, 100).is_err());
        assert_eq!(heads(&mut app), before);
        assert_eq!(app.store.invocation(&inv.id).unwrap(), inv);
        assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
    }
}

#[test]
fn work_grant_root_capacity_failure_is_atomic_and_keeps_reservations() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    let planned = plan(&mut app, &req, digest(&req).unwrap(), 100).unwrap();
    let ledger = app.store.ledger().unwrap();
    let input = ledger.limits.total.input_bytes
        - ledger.limits.protected_closeout.input_bytes
        - ledger.ordinary.exposure().unwrap().input_bytes;
    let reservation = app
        .store
        .reserve(
            "fixture-capacity-use",
            s::Resources {
                input_bytes: input - planned.minimum.input_bytes + 1,
                ..Default::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    let before = heads(&mut app);
    let ledger = app.store.ledger().unwrap();
    assert!(preview_work_resource_grant(&mut app, &req, 100).is_err());
    assert!(grant_work_resources(&mut app, &req, 100).is_err());
    assert_eq!(heads(&mut app), before);
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
    assert_eq!(app.store.reservation(&reservation.id).unwrap(), reservation);
    let after = app.store.ledger().unwrap();
    assert_eq!(after.ordinary.reserved, ledger.ordinary.reserved);
    assert_eq!(after.ordinary.uncertain, ledger.ordinary.uncertain);
    assert_eq!(
        after.ordinary.consumed.retained_bytes,
        ledger.ordinary.consumed.retained_bytes
    );
    assert_eq!(after.closeout, ledger.closeout);
}

#[test]
fn work_grant_frozen_store_stays_frozen() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app, "work-00000001");
    let reserved = app
        .store
        .reserve(
            "fixture-overrun",
            s::Resources {
                attempts: 1,
                ..Default::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    assert!(app
        .store
        .settle(
            &reserved.id,
            s::Usage::Measured(s::Resources {
                attempts: 2,
                ..Default::default()
            })
        )
        .is_err());
    let before = heads(&mut app);
    let ledger = app.store.ledger().unwrap();
    assert!(preview_work_resource_grant(&mut app, &req, 100).is_err());
    assert!(grant_work_resources(&mut app, &req, 100).is_err());
    assert_eq!(heads(&mut app), before);
    assert!(app.store.ledger().unwrap().frozen);
    assert_eq!(
        app.store.ledger().unwrap().ordinary.uncertain,
        ledger.ordinary.uncertain
    );
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
}

#[test]
fn work_grant_rejects_episode_target_and_carries_only_matching_other_bindings() {
    let now = unix_now().unwrap();
    for stale in [false, true] {
        let (_directory, mut app) = fixture();
        let (head, mut saved) = load(&mut app).unwrap();
        saved.steps = 1;
        saved.setup.max_works = 3;
        save(&mut app, Some(&head), &saved).unwrap();
        start(&mut app, "Independent second fixture need").unwrap();
        let (head, mut state) = app.load().unwrap();
        let allocation = WorkAllocation::configured(&state.config);
        let meta = state.work_meta.get_mut("work-00000001").unwrap();
        meta.attempts_left = allocation.attempts;
        meta.navigation_left = allocation.navigation;
        let focus = state
            .personas
            .get_mut("persona-0001")
            .unwrap()
            .focus
            .get_mut("work-00000001")
            .unwrap();
        focus.permit = true;
        let target_meta = state.work_meta.get_mut("work-00000002").unwrap();
        target_meta.attempts_left = 0;
        target_meta.navigation_left = zero_navigation();
        app.save(&head, &state).unwrap();
        let snapshot =
            intake_delegation::intake_delegation_snapshot(&mut app, "fixture-human", now).unwrap();
        let parent = &snapshot.works["work-00000001"];
        let grant = intake_delegation::IntakeDelegationRequest {
            id: "fixture-existing-episode".into(),
            operator: "fixture-human".into(),
            purpose: "Existing independent fixture episode".into(),
            boundaries: "Local fixture".into(),
            activate: true,
            expected: snapshot.expected.clone(),
            expected_cohort: snapshot.cohort.clone(),
            requesting_personas: BTreeSet::from(["persona-0001".into()]),
            proposal_work_ids: BTreeSet::from(["work-00000001".into()]),
            // The common fixture deliberately retains persona-0002's failure
            // hold. The independent episode grants participation only to the
            // eligible persona; the unrelated hold remains unchanged.
            allowed_child_participants: BTreeSet::from(["persona-0001".into()]),
            initial_consideration_recipients: BTreeSet::new(),
            policy: snapshot.launch_policy.clone(),
            destination: snapshot.destination.clone(),
            expires_at_unix: now + 600,
            creation_rights: 1,
            allow_further_delegation: false,
            episode_ceiling: WorkAllocation {
                attempts: 1,
                navigation: parent.remaining.navigation,
                repair_attempts: 0,
            },
            funding: intake_delegation::IntakeFundingBasis::ExistingParentTransfer {
                work_id: "work-00000001".into(),
                expected_work_revision: parent.work_revision,
                expected_work_epoch: parent.work_epoch,
                expected_remaining: parent.remaining,
            },
            additional_launch_steps: 0,
            additional_launch_work_ids: 0,
            token_attestation: intake_delegation::IntakeTokenAttestation {
                conservative_tokens_used: 0,
                conservative_token_ceiling: 1000,
                per_call_reserve_tokens: 100,
            },
        };
        intake_delegation::grant_intake_delegation(&mut app, &grant, now).unwrap();
        let issued =
            intake_delegation::intake_delegation_snapshot(&mut app, "fixture-human", now).unwrap();
        let selected = p::need::GrantReference {
            id: grant.id.clone(),
            sha256: issued.grant_sha256[&grant.id].clone(),
        };
        let old_call = PrepareRequest {
            id: "fixture-episode-call".into(),
            persona: "persona-0001".into(),
            work: "work-00000001".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        };
        let (head, mut state) = app.load().unwrap();
        assert!(
            need_proposals::attach(&mut app, &mut state, &old_call, Some(&selected), now).unwrap()
        );
        if stale {
            state
                .intake_episodes
                .get_mut(&grant.id)
                .unwrap()
                .launch_binding_sha256 = "e".repeat(64);
        }
        app.save(&head, &state).unwrap();
        assert!(
            work_resource_grant_snapshot(&mut app, "fixture-human", &old_call.work)
                .unwrap_err()
                .to_string()
                .contains("episode-bound")
        );
        let old_episode = value(&state.intake_episodes[&grant.id]);
        let req = request(&mut app, "work-00000002");
        let mut denied = req.clone();
        denied.work_id = old_call.work.clone();
        assert!(grant_work_resources(&mut app, &denied, now)
            .unwrap_err()
            .to_string()
            .contains("episode-bound"));
        let before = heads(&mut app);
        preview_work_resource_grant(&mut app, &req, now).unwrap();
        assert_eq!(heads(&mut app), before);
        let applied = grant_work_resources(&mut app, &req, now).unwrap();
        let (_, next) = app.load().unwrap();
        let (_, next_saved) = load(&mut app).unwrap();
        let mut episode = value(&next.intake_episodes[&grant.id]);
        if stale {
            assert!(applied.receipt.carried_episode_bindings.is_empty());
        } else {
            assert_eq!(
                applied.receipt.carried_episode_bindings,
                vec![grant.id.clone()]
            );
            assert_eq!(
                next.intake_episodes[&grant.id].launch_binding_sha256,
                intake_delegation::episode_launch_binding(&next_saved).unwrap()
            );
            episode["launch_binding_sha256"] = old_episode["launch_binding_sha256"].clone();
        }
        assert_eq!(episode, old_episode);
        assert_eq!(
            value(&next.work_meta[&old_call.work]),
            value(&state.work_meta[&old_call.work])
        );
        assert_eq!(value(&next.personas), value(&state.personas));
        assert_eq!(value(&next.board), value(&state.board));
        let after_registry =
            intake_delegation::intake_delegation_snapshot(&mut app, "fixture-human", now).unwrap();
        assert_eq!(after_registry.registry, issued.registry);
        assert_eq!(
            need_proposals::check_work(&mut app, &next, &old_call.work, now + 1).is_err(),
            stale
        );
    }
}
