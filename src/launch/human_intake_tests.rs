//! Local synthetic Store tests only: never resolves credentials or calls a provider.
use super::*;

fn value<T: Serialize>(item: &T) -> Value {
    serde_json::to_value(item).unwrap()
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
    assert_eq!(
        start(&mut app, "First exact fixture request").unwrap(),
        "work-00000001"
    );
    // Seed an exhausted launch counter in this newly created fixture only.
    let (head, mut saved) = load(&mut app).unwrap();
    saved.steps = 2;
    save(&mut app, Some(&head), &saved).unwrap();
    (directory, app)
}

fn request(app: &mut Runtime) -> HumanIntakeRequest {
    let snapshot = human_intake_snapshot(app, "fixture-human").unwrap();
    let statement = "Existing fixture approval permits five total calls and two human needs.";
    let body = "Plan an optional local session.\nPreserve this exact UTF-8 body: café.\n";
    let (_, state) = app.load().unwrap();
    HumanIntakeRequest {
        id: "fixture-renewal".into(),
        operator: "fixture-human".into(),
        purpose: "Admit one new bounded human need".into(),
        authorize_human_intake: true,
        authorization: HumanAuthorizationEvidence {
            reference: "fixture-approval-1".into(),
            statement: statement.into(),
            statement_sha256: s::sha256_bytes(statement.as_bytes()),
            authorized_max_total_steps: 5,
            authorized_max_works: 2,
            model: "fixture-model".into(),
            effort: "high".into(),
            destination: "fixture-local".into(),
            cumulative_token_ceiling: 1000,
        },
        expected: serde_json::from_value(snapshot["expected"].clone()).unwrap(),
        work_id: "work-00000002".into(),
        request: body.into(),
        request_sha256: s::sha256_bytes(body.as_bytes()),
        require_acceptance: true,
        allocation: WorkAllocation {
            attempts: 3,
            navigation: state.config.navigation_per_call,
            repair_attempts: 0,
        },
        new_max_total_steps: 5,
        new_max_works: 2,
        conservative_tokens_used: 100,
        per_call_reserve_tokens: 100,
    }
}

fn heads(app: &mut Runtime) -> (s::StoredObject, s::StoredObject) {
    (app.load().unwrap().0, load(app).unwrap().0)
}

#[test]
fn human_intake_atomic_preview_preserves_old_work_holds_and_accounting() {
    let (_directory, mut app) = fixture();
    let (head, mut state) = app.load().unwrap();
    for persona in state.personas.values_mut() {
        let focus = persona.focus.get_mut("work-00000001").unwrap();
        focus.permit = false;
        focus.ordinary_continuation = false;
        focus.continuation = Some(p::Continuation::Stop {
            reason: "Fixture completed participation".into(),
        });
    }
    // A retained hold is deliberately seeded, with no external failed attempt.
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
    let before = heads(&mut app);
    let ledger_before = app.store.ledger().unwrap();
    let req = request(&mut app);
    let preview = preview_human_intake(&mut app, &req, 100).unwrap();
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
    let applied = admit_human_need(&mut app, &req, 100).unwrap();
    assert!(!applied.replayed);
    assert_eq!(value(&applied.receipt), value(&preview));
    let after = heads(&mut app);
    let (_, next) = app.load().unwrap();
    let (_, saved) = load(&mut app).unwrap();
    assert_eq!(saved.steps, 2);
    assert_eq!(saved.setup.max_total_steps, 5);
    assert_eq!(saved.setup.max_works, 2);
    assert_eq!(saved.works.len(), 2);
    assert_eq!(next.work_meta[&req.work_id].attempts_left, 3);
    assert_eq!(
        next.work_meta[&req.work_id].navigation_left,
        req.allocation.navigation
    );
    assert_eq!(next.work_meta[&req.work_id].repair_attempts_left, 0);
    assert!(next.work_meta[&req.work_id].authored_origin.is_none());
    assert_eq!(
        value(&next.work_meta[&req.work_id].original.origin),
        json!({"kind":"human"})
    );
    assert_eq!(next.work_meta[&req.work_id].original.request, req.request);
    assert_eq!(
        next.work_meta[&req.work_id].original.outcomes["result"].criterion,
        req.request
    );
    assert_eq!(
        next.board.works()[&w::WorkId(req.work_id.clone())]
            .need
            .request,
        req.request
    );
    // Normalize only the precisely expected additions, then compare the entire
    // old runtime/launch documents, including all histories and persona grants.
    let mut normalized = after.0.body.clone();
    normalized["work_meta"]
        .as_object_mut()
        .unwrap()
        .remove(&req.work_id);
    normalized["board"] = before.0.body["board"].clone();
    normalized["operator_events"] = before.0.body["operator_events"].clone();
    for persona in normalized["personas"].as_object_mut().unwrap().values_mut() {
        persona["focus"]
            .as_object_mut()
            .unwrap()
            .remove(&req.work_id);
    }
    assert_eq!(normalized, before.0.body);
    assert_eq!(
        value(&next.board.works()[&w::WorkId("work-00000001".into())]),
        value(&state.board.works()[&w::WorkId("work-00000001".into())])
    );
    assert_eq!(
        &next.operator_events[..state.operator_events.len()],
        &state.operator_events
    );
    let mut launch_normalized = after.1.body.clone();
    launch_normalized["works"]
        .as_object_mut()
        .unwrap()
        .remove(&req.work_id);
    launch_normalized["setup"]["max_total_steps"] = json!(2);
    launch_normalized["setup"]["max_works"] = json!(1);
    assert_eq!(launch_normalized, before.1.body);
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
        ledger_after.closeout.reserved,
        ledger_before.closeout.reserved
    );
    assert_eq!(
        ledger_after.closeout.uncertain,
        ledger_before.closeout.uncertain
    );
    // A new runtime revision pays the existing prepaid closeout-storage charge.
    // Intake must retain it and every previous charge, rather than reset them.
    let mut closeout_consumed = ledger_after.closeout.consumed;
    assert!(closeout_consumed.retained_bytes >= ledger_before.closeout.consumed.retained_bytes);
    closeout_consumed.retained_bytes = ledger_before.closeout.consumed.retained_bytes;
    assert_eq!(closeout_consumed, ledger_before.closeout.consumed);
    assert_eq!(ledger_after.limits, ledger_before.limits);
    // A new invitation never bypasses an old persona-level failure hold.
    let held = app.prepare(
        PrepareRequest {
            id: "held-fixture-call".into(),
            persona: "persona-0002".into(),
            work: req.work_id,
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        101,
    );
    assert!(held.is_err());
}

#[test]
fn human_intake_exact_replay_and_stale_fences() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app);
    let first = admit_human_need(&mut app, &req, 100).unwrap();
    let before_replay = heads(&mut app);
    let repeated = admit_human_need(&mut app, &req, 200).unwrap();
    assert!(repeated.replayed);
    assert_eq!(value(&first.receipt), value(&repeated.receipt));
    assert_eq!(heads(&mut app), before_replay);
    let mut changed = req.clone();
    changed.purpose.push_str(" changed");
    assert!(admit_human_need(&mut app, &changed, 200).is_err());
    changed = req;
    changed.id = "new-id-stale-fence".into();
    assert!(admit_human_need(&mut app, &changed, 200).is_err());
    assert_eq!(heads(&mut app), before_replay);
}

#[test]
fn human_intake_rejects_invalid_authorization_and_allocation() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app);
    let before = heads(&mut app);
    let mut invalid = Vec::new();
    macro_rules! case {
        ($field:ident $(.$sub:ident)*, $value:expr) => {{
            let mut r = req.clone();
            r.$field $(.$sub)* = $value;
            invalid.push(r);
        }};
    }
    case!(authorize_human_intake, false);
    case!(operator, "persona-0001".into());
    case!(authorization.reference, " ".into());
    case!(authorization.statement_sha256, "0".repeat(64));
    case!(request_sha256, "0".repeat(64));
    case!(authorization.authorized_max_works, 1);
    case!(authorization.authorized_max_total_steps, 4);
    case!(new_max_works, 3);
    case!(new_max_total_steps, 6);
    case!(allocation.repair_attempts, 4);
    case!(allocation.navigation.calls, 1000);
    case!(conservative_tokens_used, 950);
    case!(work_id, "work-00000003".into());
    case!(authorization.model, "other-model".into());
    case!(authorization.effort, "other-effort".into());
    case!(authorization.destination, "other-destination".into());
    for r in invalid {
        assert!(
            admit_human_need(&mut app, &r, 100).is_err(),
            "accepted invalid request: {r:?}"
        );
    }
    assert_eq!(heads(&mut app), before);
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
    let mut unknown = value(&req);
    unknown["delegation"] = json!(true);
    assert!(serde_json::from_value::<HumanIntakeRequest>(unknown).is_err());
}

#[test]
fn human_intake_rejects_nonterminal_invocations_without_resetting() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app);
    let inv = app
        .store
        .prepare_invocation(s::InvocationSpec {
            id: "standalone-pending-fixture".into(),
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
    assert!(admit_human_need(&mut app, &req, 100).is_err());
    assert_eq!(heads(&mut app), before);
    assert_eq!(app.store.invocation(&inv.id).unwrap(), inv);
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
}

#[test]
fn human_intake_frozen_store_remains_frozen() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app);
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
    let ledger = app.store.ledger().unwrap();
    let before = heads(&mut app);
    assert!(ledger.frozen);
    assert!(admit_human_need(&mut app, &req, 100).is_err());
    let after = app.store.ledger().unwrap();
    assert!(after.frozen);
    assert_eq!(heads(&mut app), before);
    assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
    assert_eq!(after.ordinary.reserved, ledger.ordinary.reserved);
    assert_eq!(after.ordinary.uncertain, ledger.ordinary.uncertain);
    let mut charged = after.ordinary.consumed;
    assert!(charged.navigation_bytes >= ledger.ordinary.consumed.navigation_bytes);
    charged.navigation_bytes = ledger.ordinary.consumed.navigation_bytes;
    assert_eq!(charged, ledger.ordinary.consumed);
    assert_eq!(after.closeout, ledger.closeout);
    assert_eq!(after.overrun_count, ledger.overrun_count);
}

#[test]
fn human_intake_literal_body_reaches_normal_prepared_context() {
    let (_directory, mut app) = fixture();
    let req = request(&mut app);
    admit_human_need(&mut app, &req, 100).unwrap();
    let prepared = app
        .prepare(
            PrepareRequest {
                id: "fixture-prepare-only".into(),
                persona: "persona-0001".into(),
                work: req.work_id,
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            101,
        )
        .unwrap();
    // Structured sections accompany fixed instructions. The complete literal
    // request must survive JSON escaping inside that ordinary rendered context.
    assert!(prepared
        .prompt
        .contains(&serde_json::to_string(&req.request).unwrap()));
    assert!(prepared.need_interface.is_none());
    assert!(prepared.intake_episode.is_none());
    assert_eq!(
        app.store.invocation("fixture-prepare-only").unwrap().state,
        s::InvocationState::Prepared
    );
}

#[test]
fn human_intake_carries_only_matching_episode_bindings_across_cap_changes() {
    let now = unix_now().unwrap();
    // Cover a call-cap change, a work-cap change, and both; stale bindings must
    // stay unusable in every case. These grants exist only in synthetic Stores.
    for (old_calls, old_works, fill_work_slots) in [(2, 2, false), (10, 2, true), (2, 2, true)] {
        for stale in [false, true] {
            let (_directory, mut app) = fixture();
            let (head, mut saved) = load(&mut app).unwrap();
            saved.steps = 1;
            saved.setup.max_total_steps = old_calls;
            saved.setup.max_works = old_works;
            save(&mut app, Some(&head), &saved).unwrap();
            let snapshot =
                intake_delegation::intake_delegation_snapshot(&mut app, "fixture-human", now)
                    .unwrap();
            let parent = &snapshot.works["work-00000001"];
            let grant = intake_delegation::IntakeDelegationRequest {
                id: "fixture-existing-episode".into(),
                operator: "fixture-human".into(),
                purpose: "Existing bounded fixture episode".into(),
                boundaries: "Local fixture only".into(),
                activate: true,
                expected: snapshot.expected.clone(),
                expected_cohort: snapshot.cohort.clone(),
                requesting_personas: BTreeSet::from(["persona-0001".into()]),
                proposal_work_ids: BTreeSet::from(["work-00000001".into()]),
                allowed_child_participants: snapshot.cohort.clone(),
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
                intake_delegation::intake_delegation_snapshot(&mut app, "fixture-human", now)
                    .unwrap();
            if fill_work_slots {
                // Issue the existing grant while a work slot is available, then
                // use the normal human path to fill it. Work registration alone
                // is deliberately excluded from the episode launch fingerprint.
                start(&mut app, "Another existing synthetic human need").unwrap();
            }
            let selected = p::need::GrantReference {
                id: grant.id.clone(),
                sha256: issued.grant_sha256[&grant.id].clone(),
            };
            let old_work_call = PrepareRequest {
                id: "fixture-existing-call".into(),
                persona: "persona-0001".into(),
                work: "work-00000001".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            };
            let (head, mut state) = app.load().unwrap();
            assert!(need_proposals::attach(
                &mut app,
                &mut state,
                &old_work_call,
                Some(&selected),
                now
            )
            .unwrap());
            if stale {
                state
                    .intake_episodes
                    .get_mut(&grant.id)
                    .unwrap()
                    .launch_binding_sha256 = "e".repeat(64);
            }
            app.save(&head, &state).unwrap();
            assert_eq!(
                need_proposals::check_work(&mut app, &state, &old_work_call.work, now + 1).is_err(),
                stale
            );
            let old_episode = value(&state.intake_episodes[&grant.id]);
            let mut req = request(&mut app);
            req.new_max_total_steps = old_calls.max(4);
            req.work_id = format!("work-{:08}", req.expected.work_count + 1);
            req.new_max_works = old_works.max(req.expected.work_count + 1);
            req.authorization.authorized_max_total_steps = 20;
            req.authorization.authorized_max_works = 3;
            let before = heads(&mut app);
            preview_human_intake(&mut app, &req, now + 1).unwrap();
            assert_eq!(heads(&mut app), before);
            admit_human_need(&mut app, &req, now + 1).unwrap();
            let (_, next) = app.load().unwrap();
            let (_, next_saved) = load(&mut app).unwrap();
            let mut normalized_episode = value(&next.intake_episodes[&grant.id]);
            if stale {
                assert_eq!(normalized_episode, old_episode);
            } else {
                assert_eq!(
                    next.intake_episodes[&grant.id].launch_binding_sha256,
                    intake_delegation::episode_launch_binding(&next_saved).unwrap()
                );
                normalized_episode["launch_binding_sha256"] =
                    old_episode["launch_binding_sha256"].clone();
                assert_eq!(normalized_episode, old_episode);
            }
            assert_eq!(
                value(&next.work_meta["work-00000001"]),
                value(&state.work_meta["work-00000001"])
            );
            assert_eq!(
                value(&next.personas["persona-0001"].focus["work-00000001"]),
                value(&state.personas["persona-0001"].focus["work-00000001"])
            );
            assert_eq!(next_saved.steps, 1);
            assert_eq!(next.work_meta[&req.work_id].attempts_left, 3);
            assert!(need_proposals::reference(&next, &req.work_id).is_none());
            assert_eq!(
                need_proposals::check_work(&mut app, &next, &old_work_call.work, now + 2).is_err(),
                stale
            );
            let after_registry =
                intake_delegation::intake_delegation_snapshot(&mut app, "fixture-human", now + 2)
                    .unwrap();
            assert_eq!(after_registry.registry, issued.registry);
            let prepared = app.prepare(old_work_call, now + 2);
            assert_eq!(
                prepared.is_err(),
                stale,
                "old_calls={old_calls}, old_works={old_works}, result={prepared:?}"
            );
            if !stale {
                let (_, charged) = app.load().unwrap();
                assert_eq!(charged.intake_episodes[&grant.id].remaining.attempts, 0);
                assert_eq!(charged.work_meta[&req.work_id].attempts_left, 3);
            }
        }
    }
}

fn control_projection(object: &s::StoredObject) -> u64 {
    serde_json::to_vec(object).unwrap().len() as u64
        + 128
        + runtime_control_requirements(object)
            .unwrap()
            .iter()
            .map(|requirement| requirement.growth_bytes)
            .sum::<u64>()
}

#[test]
fn human_intake_control_bound_covers_maximum_escaped_pause_and_end() {
    let (_directory, mut app) = fixture();
    let (mut old, _) = app.load().unwrap();
    old.body["retained_extension"] = json!({"evidence":"keep exactly"});
    old.body["board"]["works"]["work-00000001"]["version"] =
        json!({"revision":u64::MAX - 2,"epoch":u64::MAX - 1});
    let maximum = control_projection(&old);
    let reason = "\0".repeat(4096); // Six JSON bytes for every input byte.
    let operator = "x".repeat(128);
    for action in [
        s::RuntimeControlAction::Canceled,
        s::RuntimeControlAction::Unfinished,
    ] {
        for pause_first in [false, true] {
            let mut current = old.clone();
            if pause_first {
                current = runtime_control_object(
                    &current,
                    &s::RuntimeControl {
                        operator: operator.clone(),
                        work: "work-00000001".into(),
                        reason: reason.clone(),
                        action: s::RuntimeControlAction::Pause,
                    },
                )
                .unwrap();
                assert_eq!(
                    current.body["board"]["works"]["work-00000001"]["participants"],
                    old.body["board"]["works"]["work-00000001"]["participants"]
                );
                assert!(control_projection(&current) <= maximum);
            }
            let ended = runtime_control_object(
                &current,
                &s::RuntimeControl {
                    operator: operator.clone(),
                    work: "work-00000001".into(),
                    reason: reason.clone(),
                    action,
                },
            )
            .unwrap();
            assert!(control_projection(&ended) <= maximum);
            assert_eq!(
                ended.body["retained_extension"],
                old.body["retained_extension"]
            );
            assert_eq!(ended.body["work_meta"], old.body["work_meta"]);
            assert_eq!(ended.body["sources"], old.body["sources"]);
        }
    }
}

fn padded_intake_fixture(margin: i64) -> (tempfile::TempDir, Runtime, HumanIntakeRequest) {
    let (directory, mut app) = fixture();
    let (head, mut state) = app.load().unwrap();
    state
        .operator_events
        .push(json!({"fixture_retained_evidence":""}));
    app.save(&head, &state).unwrap();
    let req = request(&mut app);
    let planned = plan(&mut app, &req, digest(&req).unwrap(), 100).unwrap();
    let candidate = &planned.changes[0].object;
    let maximum = storage_limits().max_record_bytes - 32 * 1024;
    let padding = i64::try_from(maximum - control_projection(candidate)).unwrap() - margin;
    assert!(padding > 0);
    let (head, mut state) = app.load().unwrap();
    state.operator_events.last_mut().unwrap()["fixture_retained_evidence"] =
        json!("x".repeat(padding as usize));
    app.save(&head, &state).unwrap();
    let req = request(&mut app);
    (directory, app, req)
}

#[test]
fn human_intake_control_capacity_admits_without_double_counting_pause_participants() {
    let (_directory, mut app, req) = padded_intake_fixture(16_384);
    let before = heads(&mut app);
    let planned = plan(&mut app, &req, digest(&req).unwrap(), 100).unwrap();
    let candidate = &planned.changes[0].object;
    let projected = control_projection(candidate);
    let maximum = storage_limits().max_record_bytes - 32 * 1024;
    // Reproduce the original estimate independently: it budgeted participant
    // ending copies during pause and assumed fixed-size authority references.
    let old_growth: u64 = candidate.body["board"]["works"]
        .as_object()
        .unwrap()
        .values()
        .map(|work| {
            let open = work["phase"] == "Open";
            let paused = work["phase"].get("Paused").is_some();
            let participants = work["participants"].as_object().unwrap().len() as u64;
            let personas = candidate.body["personas"].as_object().unwrap().len() as u64;
            ((participants + 4) * (6 * 4096 + 4096) + 2 * personas + 320)
                * (u64::from(open) + u64::from(open || paused))
        })
        .sum();
    let old_projection = serde_json::to_vec(candidate).unwrap().len() as u64 + 128 + old_growth;
    assert!(projected <= maximum && old_projection > maximum);
    let ledger_before = app.store.ledger().unwrap();
    preview_human_intake(&mut app, &req, 100).unwrap();
    assert_eq!(heads(&mut app), before);
    let ledger_preview = app.store.ledger().unwrap();
    assert_eq!(ledger_preview.closeout, ledger_before.closeout);
    assert_eq!(
        ledger_preview.ordinary.reserved,
        ledger_before.ordinary.reserved
    );
    assert_eq!(
        ledger_preview.ordinary.uncertain,
        ledger_before.ordinary.uncertain
    );
    admit_human_need(&mut app, &req, 100).unwrap();
    let admitted = heads(&mut app).0;
    assert_eq!(
        admitted.body["operator_events"][0],
        before.0.body["operator_events"][0]
    );
    assert_eq!(
        admitted.body["board"]["works"]["work-00000001"],
        before.0.body["board"]["works"]["work-00000001"]
    );
    assert_eq!(
        admitted.body["work_meta"]["work-00000001"],
        before.0.body["work_meta"]["work-00000001"]
    );
    // Freeze this invented Store via a recorded overrun, then prove both prepaid
    // controls remain usable for every work with maximum JSON-expanding reasons.
    let reserve = app
        .store
        .reserve(
            "fixture-control-freeze",
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
            &reserve.id,
            s::Usage::Measured(s::Resources {
                attempts: 2,
                ..Default::default()
            })
        )
        .is_err());
    let frozen = app.store.ledger().unwrap();
    assert!(frozen.frozen);
    for (work, canceled) in [("work-00000001", true), ("work-00000002", false)] {
        app.pause_in_class(
            "fixture-human",
            work,
            &"\0".repeat(4096),
            s::BudgetClass::Closeout,
        )
        .unwrap();
        app.closeout_in_class(
            "fixture-human",
            work,
            canceled,
            &"\0".repeat(4096),
            s::BudgetClass::Closeout,
        )
        .unwrap();
    }
    let after = app.store.ledger().unwrap();
    assert!(after.frozen);
    assert_eq!(after.ordinary, frozen.ordinary);
    // Protected reads are still charged. Only the prepaid retained allocation
    // and all reserved/uncertain exposure must remain unchanged by controls.
    assert!(after.closeout.consumed.navigation_bytes > frozen.closeout.consumed.navigation_bytes);
    let mut normalized_closeout = after.closeout.clone();
    normalized_closeout.consumed.navigation_bytes = frozen.closeout.consumed.navigation_bytes;
    assert_eq!(normalized_closeout, frozen.closeout);
    println!(
        "CONTROL_BOUND_SYNTHETIC projected={projected} maximum={maximum} old_projection={}",
        old_projection
    );
}

#[test]
fn human_intake_control_capacity_boundary_diagnostics_and_rollback() {
    for margin in [1, 0, -1] {
        let (_directory, mut app, req) = padded_intake_fixture(margin);
        let before = heads(&mut app);
        let epoch_before = app.store.epoch().unwrap();
        let planned = plan(&mut app, &req, digest(&req).unwrap(), 100).unwrap();
        let projected = control_projection(&planned.changes[0].object);
        let maximum = storage_limits().max_record_bytes - 32 * 1024;
        assert_eq!(projected as i64, maximum as i64 - margin);
        // This must exercise projection, not a raw body/record serialization bound.
        for change in &planned.changes {
            assert!(
                serde_json::to_vec(&change.object.body).unwrap().len() as u64
                    <= storage_limits().max_body_bytes
            );
            assert!(
                serde_json::to_vec(&change.object).unwrap().len() as u64
                    <= storage_limits().max_record_bytes
            );
        }
        assert!(
            serde_json::to_vec(&planned.changes[0].object)
                .unwrap()
                .len() as u64
                <= maximum
        );
        let ledger_before = app.store.ledger().unwrap();
        let result = preview_human_intake(&mut app, &req, 100);
        if margin >= 0 {
            result.unwrap();
        } else {
            // Preview and apply must both reject the same synthetic candidate.
            for error in [
                result.unwrap_err(),
                admit_human_need(&mut app, &req, 100).unwrap_err(),
            ] {
                assert!(matches!(
                    &error,
                    RuntimeError::Store(s::StoreError::RuntimeControlCapacity {
                        projected_bytes,
                        limit_bytes,
                    }) if *projected_bytes == projected && *limit_bytes == maximum
                ));
                assert_eq!(
                    error.to_string(),
                    format!(
                        "runtime-control projection exceeds its declared byte bound: projected_bytes={projected}, limit_bytes={maximum}"
                    )
                );
            }
        }
        assert_eq!(app.store.epoch().unwrap(), epoch_before);
        assert_eq!(heads(&mut app), before);
        assert!(app.store.read(&receipt_id(&req.id)).unwrap().is_none());
        // Only separately committed ordinary reads may change the ledger.
        // Pending control slots, revisions and receipt allocations must roll back.
        let mut after = app.store.ledger().unwrap();
        assert!(
            after.ordinary.consumed.navigation_bytes
                >= ledger_before.ordinary.consumed.navigation_bytes
        );
        after.ordinary.consumed.navigation_bytes = ledger_before.ordinary.consumed.navigation_bytes;
        assert_eq!(after, ledger_before);
    }
}

#[test]
fn human_intake_control_bound_counts_oversized_escaped_retained_authority() {
    let (_directory, mut app) = fixture();
    let (mut old, _) = app.load().unwrap();
    let authority = &mut old.body["board"]["works"]["work-00000001"]["mandates"]["1"]["authority"];
    authority["id"] = json!("\"".repeat(128_000));
    // This is invented retained data accepted by WorkBoard's reference contract,
    // deliberately far beyond the IDs made by the current host constructor.
    let authority_bytes = serde_json::to_vec(authority).unwrap().len() as u64;
    let requirement = runtime_control_requirements(&old).unwrap().remove(0);
    assert!(requirement.growth_bytes >= 4 * authority_bytes);
    let maximum = control_projection(&old);
    let reason = "\0".repeat(4096);
    let paused = runtime_control_object(
        &old,
        &s::RuntimeControl {
            operator: "fixture-human".into(),
            work: "work-00000001".into(),
            reason: reason.clone(),
            action: s::RuntimeControlAction::Pause,
        },
    )
    .unwrap();
    assert!(control_projection(&paused) <= maximum);
    for action in [
        s::RuntimeControlAction::Canceled,
        s::RuntimeControlAction::Unfinished,
    ] {
        let ended = runtime_control_object(
            &paused,
            &s::RuntimeControl {
                operator: "fixture-human".into(),
                work: "work-00000001".into(),
                reason: reason.clone(),
                action,
            },
        )
        .unwrap();
        assert!(control_projection(&ended) <= maximum);
        assert_eq!(ended.body["work_meta"], old.body["work_meta"]);
    }
}
