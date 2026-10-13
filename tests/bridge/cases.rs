//! Synthetic mechanism fixtures. No semantic verdict or live execution claim.
use super::*;
use ai_personas::mock_effects as m;

fn bridge_authority(app: &mut Runtime, work: &str) -> WorkAuthority {
    let state = app.snapshot().unwrap();
    let current = state.board.work(&w::WorkId(work.into())).unwrap();
    WorkAuthority {
        work: work.into(),
        version: current.version,
        authority: current.mandate().authority.clone(),
    }
}
fn bridge_bounds() -> m::Bounds {
    m::Bounds {
        response_bytes: 32 * 1024,
        input_bytes: 4096,
        result_bytes: 4096,
        evidence_bytes: 16 * 1024,
    }
}
fn bridge_attach(store: &mut s::Store) -> m::MockHarness<'_> {
    m::MockHarness::attach(store, "bridge-fixture".into(), bridge_bounds()).unwrap()
}
fn bridge_access(mock: &m::MockHarness<'_>, basis: &s::ObjectRef) -> m::HostAdmission {
    m::HostAdmission::from_trusted_host(
        mock.epoch().unwrap(),
        "a".into(),
        "work".into(),
        vec![basis.clone()],
        m::Rights {
            submit_fixture: true,
            dispatch: true,
            cancel: true,
            observe: true,
        },
    )
}
fn bridge_admit(
    fixture: &mut Fixture,
    id: &str,
    output_policy: f::Policy,
    qualifications: Vec<f::Reference>,
) -> (s::Store, s::ObjectRef, m::AdmissionReceipt) {
    let request = m::MockRequest {
        invocation_id: format!("mock-invocation-{id}"),
        id: id.into(),
        actor: "a".into(),
        work: "work".into(),
        interface_version: m::INTERFACE_VERSION.into(),
        connection: "bridge-fixture".into(),
        request_text: "Observe the bounded synthetic fixture.".into(),
        inputs: "exact opaque fixture input".into(),
    };
    let binding = bridge_authority(&mut fixture.app, "work");
    let basis = fixture.app.bind_mock_evidence("operator", MockEvidenceBasisRequest {
        binding, mock_request: request.clone(), policy: output_policy, lineage: qualifications.clone(),
        qualifications, attestation: "Host fixture attests that these exact sources and policy cover the mock input and result; no independent authentication.".into(),
    }, NOW).unwrap();
    let mut store = s::Store::open_for_inspection(
        &fixture.app.state_root().join("state.sqlite3"),
        storage_limits(),
    )
    .unwrap();
    let admitted = {
        let mut mock = bridge_attach(&mut store);
        let spec = m::IntentSpec::new(request.clone(), vec![basis.clone()], 4096);
        let response = json!({
            "version":1,"invocation_id":request.invocation_id,"epoch":mock.epoch().unwrap(),
            "work":{"kind":"ongoing","reason":"Synthetic observation remains pending.","evidence":[]},
            "fragment_maintenance":{"kind":"no_change","reason":"Fixture performs no persona authorship."},
            "asset_maintenance":{"kind":"no_change","reason":"Fixture performs no asset authorship."},
            "next_fragments":{"kind":"preserve","reason":"Keep fixture context."},
            "next_assets":{"kind":"preserve","reason":"Keep fixture context."},
            "continuation":{"kind":"stop","reason":"Explicit local fixture boundary."},
            "reply":{"kind":"none","reason":"No communication."},"operations":[spec.operation()]
        });
        let access = bridge_access(&mock, &basis);
        mock.submit_fixture(&access, &spec, &serde_json::to_vec(&response).unwrap())
            .unwrap()
    };
    (store, basis, admitted)
}
fn bridge_register(
    fixture: &mut Fixture,
    basis: &s::ObjectRef,
    admitted: &m::AdmissionReceipt,
) -> PendingMockRegistration {
    let binding = bridge_authority(&mut fixture.app, "work");
    fixture
        .app
        .register_pending_mock_dependency(
            "operator",
            RegisterPendingMockRequest {
                binding,
                id: format!("dependency-{}", admitted.intent_id),
                basis: basis.clone(),
                admitted_request: admitted.request.clone(),
                participants: set(&["a"]),
                accountability: w::Accountability::Owner(w::PersonaId("a".into())),
            },
            NOW,
        )
        .unwrap()
}
fn bridge_dispatch_epoch(fixture: &mut Fixture, basis: &s::ObjectRef) -> u64 {
    let binding = bridge_authority(&mut fixture.app, "work");
    fixture
        .app
        .check_mock_evidence_dispatch("operator", binding, basis.clone(), NOW)
        .unwrap()
}
fn bridge_finish(
    fixture: &mut Fixture,
    store: &mut s::Store,
    basis: &s::ObjectRef,
    id: &str,
) -> m::MockTerminal {
    let epoch = bridge_dispatch_epoch(fixture, basis);
    let mut mock = bridge_attach(store);
    assert_eq!(mock.epoch().unwrap(), epoch);
    let access = bridge_access(&mock, basis);
    let pending = mock.dispatch(&access, id).unwrap();
    assert_eq!(pending.state, m::MockState::Pending);
    assert_eq!(pending.observed_mock_executions, Some(1));
    let access = bridge_access(&mock, basis);
    mock.simulate_terminal(
        &access,
        id,
        m::MockOutcome::Completed,
        "exact \\n λ\n synthetic result",
    )
    .unwrap()
    .terminal
    .unwrap()
}
fn bridge_ingestion(
    app: &mut Runtime,
    id: &str,
    basis: &s::ObjectRef,
    admitted: &m::AdmissionReceipt,
    evidence: &s::ObjectRef,
) -> IngestMockEvidenceRequest {
    IngestMockEvidenceRequest {
        binding: bridge_authority(app, "work"),
        id: id.into(),
        actor: "a".into(),
        intent_id: admitted.intent_id.clone(),
        basis: basis.clone(),
        admitted_request: admitted.request.clone(),
        evidence: evidence.clone(),
        source: format!("result-{id}"),
        path: format!("result-{id}.json"),
        max_evidence_bytes: 16 * 1024,
    }
}
fn bridge_event(
    app: &mut Runtime,
    registration: &PendingMockRegistration,
    ingestion: &str,
) -> DeliverPendingMockEventRequest {
    DeliverPendingMockEventRequest {
        binding: bridge_authority(app, "work"),
        id: format!("event-{}", registration.id),
        dependency_id: registration.id.clone(),
        binding_revision: registration.revision.clone(),
        ingestion_id: ingestion.into(),
        max_delivery_bytes: 32 * 1024,
    }
}
fn bridge_wait(fixture: &mut Fixture, invocation: &str, dependency: &str) -> Prepared {
    let prepared = fixture.prepare(invocation);
    let mut response = decision(&prepared, true);
    response.continuation = p::Continuation::Wait {
        dependency_ids: vec![dependency.into()],
        deadline_unix: (NOW + 100) as u64,
        reason: "Await this exact local dependency within the existing allowance.".into(),
    };
    assert_eq!(
        fixture
            .app
            .submit_synthetic(invocation, &serde_json::to_vec(&response).unwrap(), NOW)
            .unwrap()
            .status,
        AdoptionStatus::Adopted
    );
    prepared
}
fn bridge_control(prepared: &Prepared) -> Value {
    serde_json::from_str(
        prepared
            .prompt
            .split("\nCONTROL\n")
            .nth(1)
            .unwrap()
            .split("\nQUALIFIED_CONTEXT\n")
            .next()
            .unwrap(),
    )
    .unwrap()
}
fn bridge_supplied_object(prepared: &Prepared, source: &str) -> s::StoredObject {
    let control = bridge_control(prepared);
    let entry = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|observation| observation["payload"]["result"]["entries"].as_array())
        .flatten()
        .find(|entry| entry["reference"]["source"] == source)
        .unwrap();
    serde_json::from_str(entry["text"].as_str().unwrap()).unwrap()
}
fn bridge_hold_count(root: &std::path::Path) -> usize {
    closeout_snapshot(root)
        .1
        .iter()
        .filter(|cell| cell.0 == "reservation" && cell.1.starts_with("s:runtime-navigation:"))
        .count()
}
fn bridge_assert_only_work_navigation_changed(mut before: Value, after: Value) {
    let old = before["work_meta"]["work"]["navigation_left"]["calls"]
        .as_u64()
        .unwrap();
    let new = after["work_meta"]["work"]["navigation_left"]["calls"]
        .as_u64()
        .unwrap();
    assert!(new < old);
    before["work_meta"]["work"]["navigation_left"] =
        after["work_meta"]["work"]["navigation_left"].clone();
    assert_eq!(before, after);
}

mod dependency_capacity {
    use super::*;

    fn navigation(per_call: f::NavigationLimits, allocations: usize) -> f::NavigationLimits {
        f::NavigationLimits {
            calls: per_call.calls * allocations,
            scanned_bytes: per_call.scanned_bytes * allocations,
            inspected_revisions: per_call.inspected_revisions * allocations,
            matches: per_call.matches * allocations,
            result_bytes: per_call.result_bytes * allocations,
        }
    }

    fn admit_pending(app: &mut Runtime, work: &str, id: &str) -> RegisterPendingMockRequest {
        let request = m::MockRequest {
            invocation_id: format!("capacity-fixture-{id}"),
            id: id.into(),
            actor: "a".into(),
            work: work.into(),
            interface_version: m::INTERFACE_VERSION.into(),
            connection: "bridge-fixture".into(),
            request_text: "Observe a bounded synthetic input.".into(),
            inputs: "opaque fixture input".into(),
        };
        let binding = bridge_authority(app, work);
        let basis = app
            .bind_mock_evidence(
                "operator",
                MockEvidenceBasisRequest {
                    binding,
                    mock_request: request.clone(),
                    policy: policy(),
                    lineage: Vec::new(),
                    qualifications: Vec::new(),
                    attestation: "Host fixture attests that this policy covers the complete synthetic input and result.".into(),
                },
                NOW,
            )
            .unwrap();
        let mut store = s::Store::open_for_inspection(
            &app.state_root().join("state.sqlite3"),
            storage_limits(),
        )
        .unwrap();
        let admitted = {
            let mut mock = bridge_attach(&mut store);
            let epoch = mock.epoch().unwrap();
            let spec = m::IntentSpec::new(request.clone(), vec![basis.clone()], 4096);
            let response = json!({
                "version":1,"invocation_id":request.invocation_id,"epoch":epoch,
                "work":{"kind":"ongoing","reason":"Synthetic observation remains pending.","evidence":[]},
                "fragment_maintenance":{"kind":"no_change","reason":"No fragment authorship."},
                "asset_maintenance":{"kind":"no_change","reason":"No asset authorship."},
                "next_fragments":{"kind":"preserve","reason":"Retain fixture context."},
                "next_assets":{"kind":"preserve","reason":"Retain fixture context."},
                "continuation":{"kind":"stop","reason":"Explicit fixture boundary."},
                "reply":{"kind":"none","reason":"No communication."},
                "operations":[spec.operation()]
            });
            let access = m::HostAdmission::from_trusted_host(
                epoch,
                "a".into(),
                work.into(),
                vec![basis.clone()],
                m::Rights {
                    submit_fixture: true,
                    observe: true,
                    ..m::Rights::default()
                },
            );
            mock.submit_fixture(&access, &spec, &serde_json::to_vec(&response).unwrap())
                .unwrap()
        };
        let retained = store.intent(id).unwrap();
        assert_eq!(retained.state, s::IntentState::Admitted);
        assert!(retained
            .history
            .iter()
            .all(|receipt| receipt.status != s::ReceiptStatus::Dispatched));
        RegisterPendingMockRequest {
            binding: bridge_authority(app, work),
            id: format!("pending-{id}"),
            basis,
            admitted_request: admitted.request,
            participants: set(&["a"]),
            accountability: w::Accountability::Gap("No dependency owner is inferred.".into()),
        }
    }

    fn exact_request(
        app: &mut Runtime,
        work: &str,
        id: &str,
        source: &f::FileRevision,
    ) -> RegisterDependencyRequest {
        RegisterDependencyRequest {
            binding: bridge_authority(app, work),
            id: id.into(),
            source: reference(source),
            participants: set(&["a"]),
            accountability: w::Accountability::Gap("No dependency owner is inferred.".into()),
            origin: LocalEventOrigin::OperatorLocal,
        }
    }

    fn assert_one_state_read(mut before: CloseoutSnapshot, after: CloseoutSnapshot) {
        let cell = before
            .1
            .iter()
            .find(|cell| cell.0 == "object" && cell.1 == "runtime-state-v1")
            .unwrap();
        let head: s::StoredObject = serde_json::from_slice(&cell.2).unwrap();
        let bytes = serde_json::to_vec(&Some(head)).unwrap().len() as u64;
        let consumed = before.0["ordinary"]["consumed"]["navigation_bytes"]
            .as_u64()
            .unwrap();
        before.0["ordinary"]["consumed"]["navigation_bytes"] =
            json!(consumed.checked_add(bytes).unwrap());
        // Compare every retained cell and all accounting, including work
        // allowances, holds, histories, control capacity, epoch and sequence.
        assert_eq!(
            before.0, after.0,
            "Store metadata changed beyond one state read"
        );
        assert!(
            before.1 == after.1,
            "Retained cells changed during a read-only operation"
        );
    }

    fn read_only<T>(app: &mut Runtime, operation: impl FnOnce(&mut Runtime) -> T) -> T {
        let before = closeout_snapshot(app.state_root());
        let result = operation(app);
        assert_one_state_read(before, closeout_snapshot(app.state_root()));
        result
    }

    fn assert_held<T>(result: ai_personas::runtime::Result<T>, expected: &str) {
        match result {
            Err(RuntimeError::Held(reason)) => assert_eq!(reason, expected),
            Err(error) => panic!("expected {expected:?}, got {error:?}"),
            Ok(_) => panic!("expected {expected:?}"),
        }
    }

    fn assert_counts(app: &Runtime, work: &str, exact: usize, pending: usize) {
        let state: State = serde_json::from_value(persisted_state(app)).unwrap();
        let files = &state.work_meta[work].local_dependencies;
        let mocks = state
            .local_evidence
            .pending
            .values()
            .filter(|entry| entry.registration.request.binding.work == work)
            .collect::<Vec<_>>();
        assert_eq!(files.len(), exact);
        assert_eq!(mocks.len(), pending);
        let keys = files
            .values()
            .map(|entry| entry.key.clone())
            .chain(mocks.iter().map(|entry| entry.registration.key.clone()))
            .collect::<BTreeSet<_>>();
        let current = state.board.work(&w::WorkId(work.into())).unwrap();
        assert_eq!(keys.len(), exact + pending);
        assert_eq!(current.dependencies.len(), exact + pending);
        assert_eq!(
            current
                .dependencies
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<BTreeSet<_>>(),
            keys
        );
        assert!(files.values().all(|entry| entry.event.is_none()));
        assert!(mocks.iter().all(|entry| entry.event.is_none()));
        assert!(current.events.is_empty());
        assert!(state.local_evidence.ingestions.is_empty());
    }

    fn check_order(pending_first: bool) {
        for initial_total in [127usize, 128] {
            let mut configuration = config();
            let allocation = configuration.navigation_per_call;
            // Predeclare eight finite allocations per work. The primary uses
            // four: acceptance, two bases and one pending registration. Reuse
            // one small exact file for the intentional 128-entry boundary.
            // The controlling Store limits and production limits are unchanged.
            configuration.navigation_per_work = navigation(allocation, 8);
            let mut fixture = Fixture::with_config(true, configuration);
            fixture.accept();
            let mut other = need();
            other.id = "other-work".into();
            fixture.app.add_need("operator", other).unwrap();
            let source = record(
                "capacity-source",
                f::Scope::Assets,
                "Exact synthetic input.",
            );
            fixture
                .app
                .import_files("operator", "a", vec![source.clone()], true, NOW)
                .unwrap();

            let mut pending = admit_pending(&mut fixture.app, "work", "capacity-main");
            pending.id = "pending".into();
            // A separate actual admission makes the refused pending request
            // eligible independently of the one-registration-per-request gate.
            let mut overflow_pending = admit_pending(&mut fixture.app, "work", "capacity-spare");
            let mut other_pending = admit_pending(&mut fixture.app, "other-work", "capacity-other");
            other_pending.id = pending.id.clone();
            other_pending.binding = bridge_authority(&mut fixture.app, "other-work");
            fixture
                .app
                .register_pending_mock_dependency("operator", other_pending, NOW)
                .unwrap();
            let isolated = exact_request(&mut fixture.app, "other-work", "exact-000", &source);
            fixture
                .app
                .register_dependency("operator", isolated, NOW)
                .unwrap();
            assert_counts(&fixture.app, "other-work", 1, 1);

            let early = if pending_first {
                pending.binding = bridge_authority(&mut fixture.app, "work");
                Some(
                    fixture
                        .app
                        .register_pending_mock_dependency("operator", pending.clone(), NOW)
                        .unwrap(),
                )
            } else {
                None
            };
            let mut first = None;
            for index in 0..initial_total - 1 {
                let request = exact_request(
                    &mut fixture.app,
                    "work",
                    &format!("exact-{index:03}"),
                    &source,
                );
                let registered = fixture
                    .app
                    .register_dependency("operator", request.clone(), NOW)
                    .unwrap();
                if index == 0 {
                    first = Some((request, registered));
                }
            }
            let pending_receipt = if let Some(registration) = early {
                registration
            } else {
                pending.binding = bridge_authority(&mut fixture.app, "work");
                fixture
                    .app
                    .register_pending_mock_dependency("operator", pending.clone(), NOW)
                    .unwrap()
            };
            assert_counts(&fixture.app, "work", initial_total - 1, 1);
            if initial_total == 127 {
                let last = exact_request(&mut fixture.app, "work", "exact-126", &source);
                fixture
                    .app
                    .register_dependency("operator", last, NOW)
                    .unwrap();
            }
            assert_counts(&fixture.app, "work", 127, 1);

            // Another work already has both dependency kinds and can still
            // register after this work fills. Identical IDs are work-scoped.
            let before = persisted_state(&fixture.app);
            let isolated = exact_request(&mut fixture.app, "other-work", "exact-001", &source);
            fixture
                .app
                .register_dependency("operator", isolated, NOW)
                .unwrap();
            let after = persisted_state(&fixture.app);
            assert_eq!(after["work_meta"]["work"], before["work_meta"]["work"]);
            assert_eq!(
                after["board"]["works"]["work"],
                before["board"]["works"]["work"]
            );
            assert_counts(&fixture.app, "other-work", 2, 1);
            let state: State = serde_json::from_value(after).unwrap();
            assert_eq!(
                serde_json::to_value(state.work_meta["work"].navigation_left).unwrap(),
                serde_json::to_value(navigation(allocation, 4)).unwrap()
            );
            assert_eq!(
                serde_json::to_value(state.work_meta["other-work"].navigation_left).unwrap(),
                serde_json::to_value(navigation(allocation, 6)).unwrap()
            );
            assert!(!fixture.app.ledger().unwrap().frozen);

            let (stale_exact, saved_exact) = first.unwrap();
            let mut exact_replay = stale_exact.clone();
            exact_replay.binding = bridge_authority(&mut fixture.app, "work");
            assert_ne!(stale_exact.binding.version, exact_replay.binding.version);
            assert_ne!(pending.binding.version, exact_replay.binding.version);
            let overflow_exact = exact_request(&mut fixture.app, "work", "overflow-exact", &source);
            overflow_pending.binding = exact_replay.binding.clone();
            let root = fixture.app.state_root().to_owned();

            for reopen in [false, true] {
                if reopen {
                    let before = closeout_snapshot(&root);
                    drop(fixture.app);
                    fixture.app = Runtime::open(&root).unwrap();
                    assert_one_state_read(before, closeout_snapshot(&root));
                }
                // Exact-file replay retains the existing current-authority
                // requirement; the original stale request remains refused.
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_dependency("operator", stale_exact.clone(), NOW)
                    }),
                    "stale local event work binding",
                );
                let replayed = read_only(&mut fixture.app, |app| {
                    app.register_dependency("operator", exact_replay.clone(), NOW)
                })
                .unwrap();
                assert_eq!(
                    serde_json::to_value(replayed).unwrap(),
                    serde_json::to_value(&saved_exact).unwrap()
                );
                // Pending replay instead requires the exact original request,
                // including its historical work binding. Refreshing it conflicts.
                let replayed = read_only(&mut fixture.app, |app| {
                    app.register_pending_mock_dependency("operator", pending.clone(), NOW)
                })
                .unwrap();
                assert_eq!(
                    serde_json::to_value(replayed).unwrap(),
                    serde_json::to_value(&pending_receipt).unwrap()
                );
                let mut refreshed = pending.clone();
                refreshed.binding = exact_replay.binding.clone();
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_pending_mock_dependency("operator", refreshed, NOW)
                    }),
                    "pending mock registration conflict",
                );
                let mut conflict = exact_replay.clone();
                conflict.accountability =
                    w::Accountability::Gap("Different requested accountability.".into());
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_dependency("operator", conflict, NOW)
                    }),
                    "dependency identity/content conflict",
                );
                let mut exact_collision = overflow_exact.clone();
                exact_collision.id = pending.id.clone();
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_dependency("operator", exact_collision, NOW)
                    }),
                    "dependency identity already belongs to a pending mock request",
                );
                let mut pending_collision = overflow_pending.clone();
                pending_collision.id = stale_exact.id.clone();
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_pending_mock_dependency("operator", pending_collision, NOW)
                    }),
                    "dependency identity already belongs to an exact file",
                );
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_dependency("operator", overflow_exact.clone(), NOW)
                    }),
                    "work local dependency limit exhausted",
                );
                assert_held(
                    read_only(&mut fixture.app, |app| {
                        app.register_pending_mock_dependency(
                            "operator",
                            overflow_pending.clone(),
                            NOW,
                        )
                    }),
                    "work local dependency limit exhausted",
                );
                assert_counts(&fixture.app, "work", 127, 1);
                assert_counts(&fixture.app, "other-work", 2, 1);
            }
        }
    }

    #[test]
    fn exact_before_pending_shares_the_retained_work_limit() {
        check_order(false);
    }

    #[test]
    fn pending_before_exact_shares_the_retained_work_limit() {
        check_order(true);
    }
}

#[test]
fn pending_request_wait_dispatch_reopen_ingestion_delivery_and_actual_acknowledgment() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let qualification = record(
        "bridge-qualification",
        f::Scope::Assets,
        "Required exact synthetic qualification.",
    );
    fixture
        .app
        .import_files("operator", "a", vec![qualification.clone()], true, NOW)
        .unwrap();
    let (mut store, basis, admitted) = bridge_admit(
        &mut fixture,
        "pending",
        policy(),
        vec![qualification.reference(f::Representation::Utf8)],
    );
    let registration = bridge_register(&mut fixture, &basis, &admitted);
    assert!(fixture
        .app
        .snapshot()
        .unwrap()
        .sources
        .keys()
        .all(|key| !key.starts_with("result-")));
    let waiting = bridge_wait(&mut fixture, "bridge-wait", &registration.id);
    let metadata = &bridge_control(&waiting)["dependencies"][0];
    assert_eq!(metadata["kind"], "pending_mock_request");
    assert!(metadata.get("source").is_none());
    assert_eq!(
        metadata["admitted_request"],
        serde_json::to_value(&admitted.request).unwrap()
    );
    let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
    let actual = store.read_exact(terminal.evidence()).unwrap();
    let root = fixture.app.state_root().to_owned();
    drop(store);
    drop(fixture.app);
    fixture.app = Runtime::open(&root).unwrap();
    let request = bridge_ingestion(
        &mut fixture.app,
        "pending-ingestion",
        &basis,
        &admitted,
        terminal.evidence(),
    );
    let receipt = fixture
        .app
        .ingest_retained_mock_terminal("operator", request, NOW)
        .unwrap();
    let state = fixture.app.snapshot().unwrap();
    let saved = &state.sources[&receipt.file.source].revisions[0];
    assert_eq!(
        serde_json::from_slice::<s::StoredObject>(&saved.record.bytes).unwrap(),
        actual
    );
    assert_eq!(s::sha256_bytes(&saved.record.bytes), receipt.bytes_sha256);
    assert_eq!(
        saved.record.computed_revision().unwrap(),
        receipt.file.revision
    );
    assert_eq!(saved.record.scope, f::Scope::Assets);
    assert!(state.sources[&receipt.file.source].protected);
    assert!(
        matches!(&saved.origin, Some(RevisionOrigin::LocalEvidence { evidence, terminal_receipt, .. })
        if evidence.as_ref() == terminal.evidence() && terminal_receipt.as_ref() == terminal.receipt())
    );
    assert!(state
        .board
        .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
        .unwrap()
        .wake
        .is_none());
    let event = bridge_event(&mut fixture.app, &registration, &receipt.id);
    let delivered = fixture
        .app
        .deliver_pending_mock_event("operator", event, NOW)
        .unwrap();
    assert_eq!(delivered.delivered_to, set(&["a"]));
    assert!(!delivered.independent_authentication && !delivered.external_subscription);
    let prepared = fixture
        .app
        .scheduler_prepare(
            SchedulerPrepareRequest {
                invocation_id: "bridge-result-decision".into(),
                max_considerations: 1,
            },
            NOW,
        )
        .unwrap()
        .prepared
        .unwrap();
    assert_eq!(
        bridge_supplied_object(&prepared, &receipt.file.source),
        actual
    );
    assert!(prepared
        .prompt
        .contains("Required exact synthetic qualification."));
    assert!(prepared
        .prompt
        .contains("host-retained local mock evidence"));
    assert!(!prepared.supplied_observations.is_empty());
    let mut response = decision(&prepared, true);
    response.continuation = p::Continuation::Wait {
        dependency_ids: vec![registration.id.clone()],
        deadline_unix: (NOW + 100) as u64,
        reason: "This supplied result is acknowledged; no new execution is requested.".into(),
    };
    assert_eq!(
        fixture
            .app
            .submit_synthetic(
                &prepared.request.id,
                &serde_json::to_vec(&response).unwrap(),
                NOW
            )
            .unwrap()
            .status,
        AdoptionStatus::Adopted
    );
    let state = fixture.app.snapshot().unwrap();
    let pending = state.local_evidence.pending.values().next().unwrap();
    assert_eq!(
        serde_json::to_value(&pending.registration).unwrap(),
        serde_json::to_value(&registration).unwrap()
    );
    assert!(pending.event.is_some());
    assert!(state
        .board
        .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
        .unwrap()
        .observed_events
        .contains(&registration.key));
    assert!(state.personas["a"].focus["work"]
        .assets
        .as_ref()
        .unwrap()
        .is_empty());
    assert!(!state.board.completion_supported(&w::WorkId("work".into())));
    assert!(fixture
        .app
        .scheduler_prepare(
            SchedulerPrepareRequest {
                invocation_id: "bridge-no-repeat".into(),
                max_considerations: 1,
            },
            NOW
        )
        .unwrap()
        .prepared
        .is_none());
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    assert_eq!(
        host.intent(&admitted.intent_id)
            .unwrap()
            .history
            .iter()
            .filter(|receipt| receipt.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
}

#[test]
fn actual_result_before_wait_works_for_pending_and_existing_exact_file_modes() {
    for pending_mode in [true, false] {
        let mut fixture = Fixture::new(true);
        fixture.accept();
        let (mut store, basis, admitted) =
            bridge_admit(&mut fixture, "early", policy(), Vec::new());
        let pending = pending_mode.then(|| bridge_register(&mut fixture, &basis, &admitted));
        let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
        let actual = store.read_exact(terminal.evidence()).unwrap();
        let request = bridge_ingestion(
            &mut fixture.app,
            "early-ingestion",
            &basis,
            &admitted,
            terminal.evidence(),
        );
        let receipt = fixture
            .app
            .ingest_mock_terminal("operator", request, terminal, NOW)
            .unwrap();
        let dependency_id = if let Some(registration) = pending {
            let event = bridge_event(&mut fixture.app, &registration, &receipt.id);
            fixture
                .app
                .deliver_pending_mock_event("operator", event, NOW)
                .unwrap();
            registration.id
        } else {
            let binding = bridge_authority(&mut fixture.app, "work");
            let dependency = fixture
                .app
                .register_dependency(
                    "operator",
                    RegisterDependencyRequest {
                        binding,
                        id: "exact-terminal-file".into(),
                        source: receipt.file.clone(),
                        participants: set(&["a"]),
                        accountability: w::Accountability::Owner(w::PersonaId("a".into())),
                        origin: LocalEventOrigin::LocalMock,
                    },
                    NOW,
                )
                .unwrap();
            let binding = bridge_authority(&mut fixture.app, "work");
            fixture
                .app
                .deliver_local_event(
                    "operator",
                    DeliverLocalEventRequest {
                        binding,
                        id: "early-exact-event".into(),
                        dependency_id: dependency.id.clone(),
                        binding_revision: dependency.revision,
                        observation: receipt.file.clone(),
                        max_delivery_bytes: 32 * 1024,
                    },
                    NOW,
                )
                .unwrap();
            dependency.id
        };
        let supplied = bridge_wait(&mut fixture, "wait-after-result", &dependency_id);
        assert_eq!(
            bridge_supplied_object(&supplied, &receipt.file.source),
            actual
        );
        assert!(fixture
            .app
            .snapshot()
            .unwrap()
            .board
            .opportunity_for(
                &w::WorkId("work".into()),
                &w::PersonaId("a".into()),
                NOW as u64,
            )
            .is_none());
    }
}

#[test]
fn owner_loss_requires_actual_late_terminal_and_never_reexecutes() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let (mut store, basis, admitted) =
        bridge_admit(&mut fixture, "owner-loss", policy(), Vec::new());
    let registration = bridge_register(&mut fixture, &basis, &admitted);
    bridge_wait(&mut fixture, "owner-loss-wait", &registration.id);
    bridge_dispatch_epoch(&mut fixture, &basis);
    {
        let mut mock = bridge_attach(&mut store);
        let access = bridge_access(&mock, &basis);
        mock.dispatch(&access, &admitted.intent_id).unwrap();
    }
    let root = fixture.app.state_root().to_owned();
    drop(store);
    drop(fixture.app);
    fixture.app = Runtime::open(&root).unwrap();
    let mut store =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let record = store.intent(&admitted.intent_id).unwrap();
    assert_eq!(record.state, s::IntentState::Uncertain);
    let hold = store.reservation(&record.reservation).unwrap();
    assert_eq!(hold.state, s::ReservationState::Uncertain);
    assert!(hold.measured.is_none());
    let absent = s::ObjectRef {
        id: format!(
            "mock-terminal:{}",
            s::sha256_bytes(admitted.intent_id.as_bytes())
        ),
        revision: 1,
        sha256: "0".repeat(64),
    };
    let request = bridge_ingestion(
        &mut fixture.app,
        "unknown-refused",
        &basis,
        &admitted,
        &absent,
    );
    assert!(fixture
        .app
        .ingest_retained_mock_terminal("operator", request, NOW)
        .is_err());
    let terminal = {
        let mut mock = bridge_attach(&mut store);
        let access = bridge_access(&mock, &basis);
        let view = mock.dispatch(&access, &admitted.intent_id).unwrap();
        assert_eq!(view.state, m::MockState::EffectUnknown);
        assert!(view.terminal.is_none());
        assert_eq!(view.observed_mock_executions, Some(1));
        let access = bridge_access(&mock, &basis);
        mock.simulate_terminal(
            &access,
            &admitted.intent_id,
            m::MockOutcome::Completed,
            "late synthetic result",
        )
        .unwrap()
        .terminal
        .unwrap()
    };
    let request = bridge_ingestion(
        &mut fixture.app,
        "late-ingestion",
        &basis,
        &admitted,
        terminal.evidence(),
    );
    let receipt = fixture
        .app
        .ingest_mock_terminal("operator", request, terminal, NOW)
        .unwrap();
    let event = bridge_event(&mut fixture.app, &registration, &receipt.id);
    fixture
        .app
        .deliver_pending_mock_event("operator", event, NOW)
        .unwrap();
    let record = store.intent(&admitted.intent_id).unwrap();
    assert!(record.reconciliation.is_some());
    assert_eq!(
        record
            .history
            .iter()
            .filter(|receipt| receipt.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
}

#[test]
fn cancellation_and_late_outcomes_can_be_retained_after_work_ending_without_event_authority() {
    for outcome in [
        m::MockOutcome::Completed,
        m::MockOutcome::Failed,
        m::MockOutcome::CanceledToObservedExtent,
    ] {
        let mut fixture = Fixture::new(true);
        fixture.accept();
        let (mut store, basis, admitted) =
            bridge_admit(&mut fixture, "cancel", policy(), Vec::new());
        let registration = bridge_register(&mut fixture, &basis, &admitted);
        bridge_wait(&mut fixture, "cancel-wait", &registration.id);
        bridge_dispatch_epoch(&mut fixture, &basis);
        let terminal = {
            let mut mock = bridge_attach(&mut store);
            let access = bridge_access(&mock, &basis);
            mock.dispatch(&access, &admitted.intent_id).unwrap();
            let access = bridge_access(&mock, &basis);
            let canceled = mock
                .request_cancel(
                    &access,
                    &admitted.intent_id,
                    "Stop this synthetic execution.",
                )
                .unwrap();
            assert_eq!(canceled.state, m::MockState::CancelRequested);
            assert!(canceled.terminal.is_none());
            fixture
                .app
                .closeout("operator", "work", true, "End the synthetic work.")
                .unwrap();
            let access = bridge_access(&mock, &basis);
            mock.simulate_terminal(
                &access,
                &admitted.intent_id,
                outcome,
                "Late observed synthetic extent.",
            )
            .unwrap()
            .terminal
            .unwrap()
        };
        let request = bridge_ingestion(
            &mut fixture.app,
            "cancel-ingestion",
            &basis,
            &admitted,
            terminal.evidence(),
        );
        let receipt = fixture
            .app
            .ingest_mock_terminal("operator", request, terminal, NOW)
            .unwrap();
        let event = bridge_event(&mut fixture.app, &registration, &receipt.id);
        assert!(fixture
            .app
            .deliver_pending_mock_event("operator", event, NOW)
            .is_err());
        let state = fixture.app.snapshot().unwrap();
        assert!(state
            .local_evidence
            .pending
            .values()
            .all(|entry| entry.event.is_none()));
        assert!(state
            .board
            .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
            .unwrap()
            .wake
            .is_none());
        assert!(!state.board.completion_supported(&w::WorkId("work".into())));
        assert_eq!(
            store
                .intent(&admitted.intent_id)
                .unwrap()
                .history
                .iter()
                .filter(|receipt| receipt.status == s::ReceiptStatus::Dispatched)
                .count(),
            1
        );
    }
}

#[test]
fn retained_but_unapplied_terminal_is_refused_and_exact_observation_retry_does_not_execute() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let (mut store, basis, admitted) =
        bridge_admit(&mut fixture, "unapplied", policy(), Vec::new());
    bridge_register(&mut fixture, &basis, &admitted);
    bridge_dispatch_epoch(&mut fixture, &basis);
    let fault = Connection::open(fixture.app.state_root().join("state.sqlite3")).unwrap();
    let mut mock = bridge_attach(&mut store);
    let access = bridge_access(&mock, &basis);
    mock.dispatch(&access, &admitted.intent_id).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER bridge_terminal_fault BEFORE UPDATE ON store_cells
        WHEN NEW.ns='intent' AND NEW.k='unapplied' AND json_extract(NEW.data,'$.state')='Succeeded'
        BEGIN SELECT RAISE(ABORT,'injected bridge terminal failure'); END;",
        )
        .unwrap();
    let access = bridge_access(&mock, &basis);
    assert!(mock
        .simulate_terminal(
            &access,
            &admitted.intent_id,
            m::MockOutcome::Completed,
            "retained exact synthetic result"
        )
        .is_err());
    let access = bridge_access(&mock, &basis);
    let view = mock.inspect(&access, &admitted.intent_id).unwrap();
    assert!(view.terminal.is_none());
    let reference = view.unapplied_terminal.unwrap();
    let request = bridge_ingestion(
        &mut fixture.app,
        "unapplied-ingestion",
        &basis,
        &admitted,
        &reference,
    );
    assert!(fixture
        .app
        .ingest_retained_mock_terminal("operator", request.clone(), NOW)
        .is_err());
    assert!(!fixture
        .app
        .snapshot()
        .unwrap()
        .sources
        .contains_key(&request.source));
    fault
        .execute_batch("DROP TRIGGER bridge_terminal_fault")
        .unwrap();
    let access = bridge_access(&mock, &basis);
    let terminal = mock
        .simulate_terminal(
            &access,
            &admitted.intent_id,
            m::MockOutcome::Completed,
            "retained exact synthetic result",
        )
        .unwrap()
        .terminal
        .unwrap();
    assert_eq!(terminal.evidence(), &reference);
    let access = bridge_access(&mock, &basis);
    assert_eq!(
        mock.inspect(&access, &admitted.intent_id)
            .unwrap()
            .observed_mock_executions,
        Some(1)
    );
    drop(mock);
    fixture
        .app
        .ingest_mock_terminal("operator", request, terminal, NOW)
        .unwrap();
}

#[test]
fn excluded_participant_gets_neither_dependency_identity_nor_result_bytes() {
    let mut restricted = policy();
    restricted.id = "bridge-private".into();
    restricted.readers = set(&["a"]);
    let mut configuration = config();
    configuration
        .policies
        .active_policies
        .push(restricted.clone());
    let mut original = need();
    let mut peer = original.invitations[0].clone();
    peer.id = "offer-b".into();
    peer.to = "b".into();
    peer.terms.outcomes.clear();
    original.invitations.push(peer);
    let mut fixture = Fixture::configured(true, configuration, seed("a"), original);
    fixture.accept();
    let (mut store, basis, admitted) =
        bridge_admit(&mut fixture, "private-intent", restricted, Vec::new());
    let binding = bridge_authority(&mut fixture.app, "work");
    assert!(fixture
        .app
        .register_pending_mock_dependency(
            "operator",
            RegisterPendingMockRequest {
                binding,
                id: "private-dependency".into(),
                basis: basis.clone(),
                admitted_request: admitted.request.clone(),
                participants: set(&["a", "b"]),
                accountability: w::Accountability::Owner(w::PersonaId("a".into())),
            },
            NOW
        )
        .is_err());
    let registration = bridge_register(&mut fixture, &basis, &admitted);
    let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
    let request = bridge_ingestion(
        &mut fixture.app,
        "private-ingestion",
        &basis,
        &admitted,
        terminal.evidence(),
    );
    let receipt = fixture
        .app
        .ingest_mock_terminal("operator", request, terminal, NOW)
        .unwrap();
    let event = bridge_event(&mut fixture.app, &registration, &receipt.id);
    assert_eq!(
        fixture
            .app
            .deliver_pending_mock_event("operator", event, NOW)
            .unwrap()
            .delivered_to,
        set(&["a"])
    );
    let prepared = fixture
        .app
        .prepare(
            PrepareRequest {
                id: "private-excluded-view".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    for private in [
        &registration.id,
        &admitted.request.id,
        &receipt.file.source,
        &receipt.evidence.sha256,
    ] {
        assert!(!prepared.prompt.contains(private));
    }
    assert!(bridge_control(&prepared)["dependencies"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn wrong_actor_work_subject_and_digest_do_not_ingest_or_substitute_a_pending_result() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let (mut store, basis, admitted) = bridge_admit(&mut fixture, "binding", policy(), Vec::new());
    let registration = bridge_register(&mut fixture, &basis, &admitted);
    let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
    let original = bridge_ingestion(
        &mut fixture.app,
        "bound-ingestion",
        &basis,
        &admitted,
        terminal.evidence(),
    );
    for variant in 0..5 {
        let mut request = original.clone();
        match variant {
            0 => request.actor = "b".into(),
            1 => request.binding.work = "other-work".into(),
            2 => request.intent_id = "other-intent".into(),
            3 => request.evidence.sha256 = "0".repeat(64),
            _ => request.admitted_request.sha256 = "0".repeat(64),
        }
        assert!(fixture
            .app
            .ingest_retained_mock_terminal("operator", request, NOW)
            .is_err());
        assert!(!fixture
            .app
            .snapshot()
            .unwrap()
            .sources
            .contains_key(&original.source));
    }
    let receipt = fixture
        .app
        .ingest_mock_terminal("operator", original, terminal, NOW)
        .unwrap();
    let binding = bridge_authority(&mut fixture.app, "work");
    assert!(fixture
        .app
        .deliver_local_event(
            "operator",
            DeliverLocalEventRequest {
                binding,
                id: "cannot-cast".into(),
                dependency_id: registration.id.clone(),
                binding_revision: registration.revision.clone(),
                observation: receipt.file.clone(),
                max_delivery_bytes: 32 * 1024,
            },
            NOW
        )
        .is_err());
    let mut event = bridge_event(&mut fixture.app, &registration, &receipt.id);
    event.ingestion_id = "not-ingested".into();
    assert!(fixture
        .app
        .deliver_pending_mock_event("operator", event, NOW)
        .is_err());
    assert!(fixture
        .app
        .snapshot()
        .unwrap()
        .local_evidence
        .pending
        .values()
        .all(|entry| entry.event.is_none()));
}

#[test]
fn exact_ingestion_and_event_replay_after_reopen_and_ending_are_receipt_only() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let (mut store, basis, admitted) = bridge_admit(&mut fixture, "replay", policy(), Vec::new());
    let registration = bridge_register(&mut fixture, &basis, &admitted);
    let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
    let request = bridge_ingestion(
        &mut fixture.app,
        "replay-ingestion",
        &basis,
        &admitted,
        terminal.evidence(),
    );
    let receipt = fixture
        .app
        .ingest_mock_terminal("operator", request.clone(), terminal.clone(), NOW)
        .unwrap();
    let event = bridge_event(&mut fixture.app, &registration, &receipt.id);
    fixture
        .app
        .deliver_pending_mock_event("operator", event.clone(), NOW)
        .unwrap();
    fixture
        .app
        .closeout("operator", "work", true, "End before receipt-only replay.")
        .unwrap();
    let root = fixture.app.state_root().to_owned();
    drop(store);
    drop(fixture.app);
    fixture.app = Runtime::open(&root).unwrap();
    let before = persisted_state(&fixture.app);
    let epoch = fixture.app.epoch().unwrap();
    let holds = bridge_hold_count(&root);
    let ledger = fixture.app.ledger().unwrap();
    assert_eq!(
        fixture
            .app
            .ingest_mock_terminal("operator", request, terminal, NOW + 1)
            .unwrap(),
        receipt
    );
    assert!(
        fixture
            .app
            .deliver_pending_mock_event("operator", event, NOW + 1)
            .unwrap()
            .duplicate
    );
    assert_eq!(persisted_state(&fixture.app), before);
    assert_eq!(fixture.app.epoch().unwrap(), epoch);
    assert_eq!(bridge_hold_count(&root), holds);
    let after = fixture.app.ledger().unwrap();
    assert_eq!(after.ordinary.reserved, ledger.ordinary.reserved);
    assert_eq!(
        after.ordinary.consumed.attempts,
        ledger.ordinary.consumed.attempts
    );
    assert_eq!(
        after.ordinary.consumed.retained_bytes,
        ledger.ordinary.consumed.retained_bytes
    );
    assert!(after.ordinary.consumed.navigation_bytes > ledger.ordinary.consumed.navigation_bytes);
}

#[test]
fn ingestion_final_failure_is_atomic_and_a_fresh_retry_spends_a_new_allocation() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let (mut store, basis, admitted) = bridge_admit(&mut fixture, "atomic", policy(), Vec::new());
    let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
    let request = bridge_ingestion(
        &mut fixture.app,
        "atomic",
        &basis,
        &admitted,
        terminal.evidence(),
    );
    let root = fixture.app.state_root().to_owned();
    let fault = Connection::open(root.join("state.sqlite3")).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER bridge_ingestion_fault BEFORE UPDATE ON store_cells
        WHEN NEW.ns='object' AND NEW.k='runtime-state-v1'
          AND json_type(NEW.data,'$.body.local_evidence.ingestions.atomic')='object'
        BEGIN SELECT RAISE(ABORT,'injected atomic ingestion failure'); END;",
        )
        .unwrap();
    let before = persisted_state(&fixture.app);
    let holds = bridge_hold_count(&root);
    assert!(fixture
        .app
        .ingest_mock_terminal("operator", request.clone(), terminal.clone(), NOW)
        .unwrap_err()
        .to_string()
        .contains("injected atomic ingestion failure"));
    bridge_assert_only_work_navigation_changed(before, persisted_state(&fixture.app));
    assert_eq!(bridge_hold_count(&root), holds + 1);
    fault
        .execute_batch("DROP TRIGGER bridge_ingestion_fault")
        .unwrap();
    let receipt = fixture
        .app
        .ingest_mock_terminal("operator", request, terminal, NOW)
        .unwrap();
    assert_eq!(bridge_hold_count(&root), holds + 2);
    let state = fixture.app.snapshot().unwrap();
    assert_eq!(state.local_evidence.ingestions.len(), 1);
    assert_eq!(state.sources[&receipt.file.source].revisions.len(), 1);
}

#[test]
fn work_and_root_navigation_exhaustion_cannot_fund_ingestion_from_closeout() {
    for exhaust_work in [true, false] {
        let mut configuration = config();
        if exhaust_work {
            // Acceptance, basis creation and current dispatch check each consume
            // one existing call allocation. There is no fourth allocation.
            configuration.navigation_per_work.calls = configuration.navigation_per_call.calls * 3;
        }
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        let (mut store, basis, admitted) =
            bridge_admit(&mut fixture, "exhausted", policy(), Vec::new());
        let terminal = bridge_finish(&mut fixture, &mut store, &basis, &admitted.intent_id);
        let request = bridge_ingestion(
            &mut fixture.app,
            "exhausted-ingestion",
            &basis,
            &admitted,
            terminal.evidence(),
        );
        let root = fixture.app.state_root().to_owned();
        if !exhaust_work {
            let head = closeout_head(&root);
            let read = serde_json::to_vec(&Some(head)).unwrap().len() as u64;
            let envelope = config().navigation_per_call.scanned_bytes as u64
                + config().navigation_per_call.result_bytes as u64;
            let ledger = store.ledger().unwrap();
            let ordinary = ledger.ordinary.exposure().unwrap().navigation_bytes;
            let closeout = ledger.closeout.exposure().unwrap().navigation_bytes;
            let available = (ledger.limits.total.navigation_bytes - ordinary - closeout).min(
                ledger.limits.total.navigation_bytes
                    - ledger.limits.protected_closeout.navigation_bytes
                    - ordinary,
            );
            store
                .reserve(
                    "bridge-root-pressure",
                    s::Resources {
                        navigation_bytes: available - read - envelope + 1,
                        ..s::Resources::default()
                    },
                    s::BudgetClass::Ordinary,
                )
                .unwrap();
        }
        let before = persisted_state(&fixture.app);
        let holds = bridge_hold_count(&root);
        let closeout = fixture.app.ledger().unwrap().closeout;
        assert!(fixture
            .app
            .ingest_mock_terminal("operator", request, terminal, NOW)
            .is_err());
        assert_eq!(persisted_state(&fixture.app), before);
        assert_eq!(bridge_hold_count(&root), holds);
        assert_eq!(fixture.app.ledger().unwrap().closeout, closeout);
    }
}

#[test]
fn current_source_withdrawal_fences_stable_basis_dispatch_and_pending_metadata() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let source = record(
        "bridge-withdrawn",
        f::Scope::Assets,
        "Exact synthetic input restriction.",
    );
    fixture
        .app
        .import_files("operator", "a", vec![source.clone()], true, NOW)
        .unwrap();
    let (mut store, basis, admitted) = bridge_admit(
        &mut fixture,
        "withdrawal",
        policy(),
        vec![source.reference(f::Representation::Utf8)],
    );
    let registration = bridge_register(&mut fixture, &basis, &admitted);
    fixture
        .app
        .withdraw_source("operator", &source.source, "Withdraw the exact source.")
        .unwrap();
    let binding = bridge_authority(&mut fixture.app, "work");
    assert!(fixture
        .app
        .check_mock_evidence_dispatch("operator", binding, basis, NOW)
        .is_err());
    assert_eq!(
        store.intent(&admitted.intent_id).unwrap().state,
        s::IntentState::Admitted
    );
    let prepared = fixture.prepare("withdrawal-visible-state");
    assert!(!prepared.prompt.contains(&registration.id));
    assert!(!prepared.prompt.contains(&admitted.request.id));
}
