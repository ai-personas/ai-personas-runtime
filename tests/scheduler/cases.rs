use super::*;

fn authority(app: &mut Runtime, work: &str) -> WorkAuthority {
    let state = app.snapshot().unwrap();
    let current = state.board.work(&w::WorkId(work.into())).unwrap();
    WorkAuthority {
        work: work.into(),
        version: current.version,
        authority: current.mandate().authority.clone(),
    }
}
fn schedule(app: &mut Runtime, id: &str, limit: usize, now: i64) -> SchedulerPrepareResult {
    app.scheduler_prepare(
        SchedulerPrepareRequest {
            invocation_id: id.into(),
            max_considerations: limit,
        },
        now,
    )
    .unwrap()
}
fn inspect(app: &mut Runtime, limit: usize, now: i64) -> SchedulerInspection {
    app.scheduler_inspect(SchedulerInspectRequest { max_entries: limit }, now)
        .unwrap()
}
fn register(fixture: &mut Fixture, id: &str) -> LocalDependency {
    let source = record(
        &format!("source-{id}"),
        f::Scope::Assets,
        "EXACT LOCAL EVENT PAYLOAD",
    );
    fixture
        .app
        .import_files("operator", "a", vec![source.clone()], true, NOW)
        .unwrap();
    let binding = authority(&mut fixture.app, "work");
    fixture
        .app
        .register_dependency(
            "operator",
            RegisterDependencyRequest {
                binding,
                id: id.into(),
                source: reference(&source),
                participants: set(&["a"]),
                accountability: w::Accountability::Owner(w::PersonaId("a".into())),
                origin: LocalEventOrigin::LocalMock,
            },
            NOW,
        )
        .unwrap()
}
fn event_request(app: &mut Runtime, dependency: &LocalDependency) -> DeliverLocalEventRequest {
    DeliverLocalEventRequest {
        binding: authority(app, "work"),
        id: format!("event-{}", dependency.id),
        dependency_id: dependency.id.clone(),
        binding_revision: dependency.revision.clone(),
        observation: dependency.source.clone(),
        max_delivery_bytes: 32 * 1024,
    }
}
fn wait_decision(prepared: &Prepared, dependency: &LocalDependency) -> p::Decision {
    let mut response = decision(prepared, true);
    response.continuation = p::Continuation::Wait {
        dependency_ids: vec![dependency.id.clone()],
        deadline_unix: (NOW + 100) as u64,
        reason: "Await the registered local source within this allowance.".into(),
    };
    response
}
fn submit(app: &mut Runtime, prepared: &Prepared, response: &p::Decision) {
    assert_eq!(
        app.submit_synthetic(
            &prepared.request.id,
            &serde_json::to_vec(response).unwrap(),
            NOW
        )
        .unwrap()
        .status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn scheduler_queue_is_not_prepared_and_active_lease_prevents_epoch_mutation() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let before = fixture.app.ledger().unwrap();
    let epoch = fixture.app.epoch().unwrap();
    let view = inspect(&mut fixture.app, 1, NOW);
    assert!(matches!(
        view.queue[0].disposition,
        SchedulerDisposition::Queued
    ));
    assert!(view.active_invocations.is_empty());
    assert_eq!(fixture.app.epoch().unwrap(), epoch);
    assert_only_navigation_charged(before, fixture.app.ledger().unwrap());
    let result = schedule(&mut fixture.app, "scheduled", 1, NOW);
    let prepared = result.prepared.unwrap();
    assert!(result.persisted);
    assert_eq!(prepared.request.profile, Profile::Synthetic);
    assert!(prepared.provider_preparation.is_none());
    assert_eq!(
        fixture.app.invocation("scheduled").unwrap().state,
        s::InvocationState::Prepared
    );
    assert!(fixture.app.terminal_evidence("scheduled").is_err());
    let state = persisted_state(&fixture.app);
    let epoch = fixture.app.epoch().unwrap();
    let attempts = fixture.app.ledger().unwrap().ordinary.consumed.attempts;
    let blocked = schedule(&mut fixture.app, "busy", 128, NOW);
    assert!(!blocked.persisted && blocked.prepared.is_none());
    assert!(blocked.blocked.unwrap().contains("lease"));
    assert_eq!(fixture.app.epoch().unwrap(), epoch);
    assert_eq!(persisted_state(&fixture.app), state);
    assert_eq!(
        fixture.app.ledger().unwrap().ordinary.consumed.attempts,
        attempts
    );
    submit(&mut fixture.app, &prepared, &decision(&prepared, true));
    assert!(fixture
        .app
        .scheduler_prepare(
            SchedulerPrepareRequest {
                invocation_id: "scheduled".into(),
                max_considerations: 1,
            },
            NOW
        )
        .is_err());
}

#[test]
fn scheduler_admission_and_cursor_roll_back_together_on_late_sqlite_failure() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let root = fixture.app.state_root().to_owned();
    let connection = Connection::open(root.join("state.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_scheduler_admission BEFORE INSERT ON store_cells
        WHEN NEW.ns='invocation' AND NEW.k='atomic-scheduler'
        BEGIN SELECT RAISE(ABORT,'injected scheduler admission failure'); END;",
        )
        .unwrap();
    let state = persisted_state(&fixture.app);
    let before = fixture.app.ledger().unwrap();
    let result = schedule(&mut fixture.app, "atomic-scheduler", 1, NOW);
    assert!(result.prepared.is_none() && result.persisted);
    assert!(matches!(&result.scheduler.last[0].disposition,
        SchedulerDisposition::Blocked { reason } if reason.contains("injected scheduler")));
    assert_eq!(persisted_state(&fixture.app), state);
    assert!(fixture.app.invocation("atomic-scheduler").is_err());
    assert_eq!(
        fixture.app.ledger().unwrap().ordinary.consumed.attempts,
        before.ordinary.consumed.attempts
    );
    assert_eq!(
        fixture.app.ledger().unwrap().ordinary.reserved,
        before.ordinary.reserved
    );
    connection
        .execute_batch("DROP TRIGGER fail_scheduler_admission")
        .unwrap();
    drop(connection);
    drop(fixture.app);
    fixture.app = Runtime::open(&root).unwrap();
    let result = schedule(&mut fixture.app, "after-fault", 1, NOW);
    assert_eq!(result.scheduler.considered, 2);
    assert!(result.prepared.is_some());
    let prepared = result.prepared.unwrap();
    let connection =
        Connection::open_with_flags(root.join("state.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let bytes: Vec<u8> = connection.query_row(
        "SELECT data FROM store_cells WHERE ns='revision' AND json_extract(data,'$.id')='runtime-ordinary-scheduler-v1' ORDER BY json_extract(data,'$.revision') DESC LIMIT 1",
        [], |row| row.get(0)).unwrap();
    let object: s::StoredObject = serde_json::from_slice(&bytes).unwrap();
    let scheduler: SchedulerState = serde_json::from_value(object.body).unwrap();
    assert!(matches!(&scheduler.last[0].disposition,
        SchedulerDisposition::Prepared { invocation_id } if invocation_id == &prepared.request.id));
    assert_eq!(fixture.app.epoch().unwrap(), prepared.epoch);
    assert_eq!(
        persisted_state(&fixture.app)["personas"]["a"]["active_invocation"],
        "after-fault"
    );
}

#[test]
fn wait_delivery_duplicate_restart_and_payload_acknowledgment_are_distinct() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let dependency = register(&mut fixture, "input");
    let prepared = fixture.prepare("wait");
    submit(
        &mut fixture.app,
        &prepared,
        &wait_decision(&prepared, &dependency),
    );
    assert!(schedule(&mut fixture.app, "not-due", 1, NOW)
        .prepared
        .is_none());
    let request = event_request(&mut fixture.app, &dependency);
    let receipt = fixture
        .app
        .deliver_local_event("operator", request.clone(), NOW)
        .unwrap();
    assert_eq!(receipt.delivered_to, set(&["a"]));
    assert!(
        !receipt.duplicate && !receipt.independent_authentication && !receipt.external_subscription
    );
    let state = fixture.app.snapshot().unwrap();
    assert!(!state.personas["a"].focus["work"].permit);
    assert!(state.personas["a"].focus["work"].ordinary_continuation);
    let pending = state.personas["a"].focus["work"]
        .pending_observations
        .clone();
    assert!(!state.personas["b"].focus.contains_key("work"));
    let root = fixture.app.state_root().to_owned();
    drop(fixture.app);
    fixture.app = Runtime::open(&root).unwrap();
    // Replay the original bytes, including the now-stale work version.
    let duplicate = request.clone();
    let before = fixture.app.ledger().unwrap();
    let epoch = fixture.app.epoch().unwrap();
    let old = persisted_state(&fixture.app);
    assert!(
        fixture
            .app
            .deliver_local_event("operator", duplicate, NOW + 1)
            .unwrap()
            .duplicate
    );
    assert_eq!(persisted_state(&fixture.app), old);
    assert_eq!(fixture.app.epoch().unwrap(), epoch);
    assert_only_navigation_charged(before, fixture.app.ledger().unwrap());
    let prepared = schedule(&mut fixture.app, "event-decision", 1, NOW)
        .prepared
        .unwrap();
    assert!(prepared.prompt.contains("EXACT LOCAL EVENT PAYLOAD"));
    assert!(prepared.prompt.contains("local_mock"));
    assert!(prepared
        .supplied_observations
        .iter()
        .any(|id| pending.contains(id)));
    assert_eq!(
        control(&prepared)["selection_state"]["assets_initialized"],
        true
    );
    submit(
        &mut fixture.app,
        &prepared,
        &wait_decision(&prepared, &dependency),
    );
    let state = fixture.app.snapshot().unwrap();
    assert!(state
        .board
        .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
        .unwrap()
        .observed_events
        .contains(&dependency.key));
    assert!(state.personas["a"].focus["work"]
        .assets
        .as_ref()
        .unwrap()
        .is_empty());
    let duplicate = event_request(&mut fixture.app, &dependency);
    fixture
        .app
        .deliver_local_event("operator", duplicate, NOW)
        .unwrap();
    assert!(schedule(&mut fixture.app, "no-repeat-wake", 1, NOW)
        .prepared
        .is_none());
    let deadline = schedule(&mut fixture.app, "deadline", 1, NOW + 100)
        .prepared
        .unwrap();
    assert!(matches!(
        inspect(&mut fixture.app, 1, NOW + 100).scheduler.last[0].cause,
        Some(w::OpportunityCause::Deadline)
    ));
    assert!(deadline.provider_preparation.is_none());
}

#[test]
fn event_between_preparation_and_wait_registration_fences_old_response_without_losing_evidence() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let dependency = register(&mut fixture, "race");
    let old = fixture.prepare("racing-wait");
    fixture
        .app
        .dispatch_synthetic(&old.request.id, NOW)
        .unwrap();
    let request = event_request(&mut fixture.app, &dependency);
    fixture
        .app
        .deliver_local_event("operator", request, NOW)
        .unwrap();
    let report = fixture
        .app
        .adopt_response(
            &old.request.id,
            &serde_json::to_vec(&wait_decision(&old, &dependency)).unwrap(),
            NOW,
        )
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.unwrap().contains("epoch"));
    assert!(schedule(&mut fixture.app, "no-implicit-recovery", 1, NOW)
        .prepared
        .is_none());
    fixture.app.resume("operator", "a", "work").unwrap();
    let fresh = schedule(&mut fixture.app, "race-observation", 1, NOW)
        .prepared
        .unwrap();
    assert!(fresh.prompt.contains("EXACT LOCAL EVENT PAYLOAD"));
    submit(
        &mut fixture.app,
        &fresh,
        &wait_decision(&fresh, &dependency),
    );
    assert!(schedule(&mut fixture.app, "already-observed", 1, NOW)
        .prepared
        .is_none());
}

fn local_event_budget_fixture() -> (Fixture, DeliverLocalEventRequest) {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let dependency = register(&mut fixture, "budget");
    let request = event_request(&mut fixture.app, &dependency);
    (fixture, request)
}

fn local_event_budget_read_bytes(head: &s::StoredObject) -> u64 {
    serde_json::to_vec(&Some(head)).unwrap().len() as u64
}

fn assert_local_event_budget_read_only(
    mut before: CloseoutSnapshot,
    after: CloseoutSnapshot,
    bytes: u64,
) {
    let consumed = before.0["ordinary"]["consumed"]["navigation_bytes"]
        .as_u64()
        .unwrap();
    before.0["ordinary"]["consumed"]["navigation_bytes"] = json!(consumed + bytes);
    assert_eq!(before, after);
}

fn local_event_budget_reservations(snapshot: &CloseoutSnapshot) -> Vec<s::ReservationRecord> {
    snapshot
        .1
        .iter()
        .filter(|cell| cell.0 == "reservation" && cell.1.starts_with("s:runtime-navigation:"))
        .map(|cell| serde_json::from_slice(&cell.2).unwrap())
        .collect()
}

#[test]
fn local_event_root_navigation_exhaustion_and_shortfall_preserve_work_allowance() {
    let allocation = config().navigation_per_call;
    let envelope = allocation.scanned_bytes as u64 + allocation.result_bytes as u64;
    for (class, unknown) in [
        (s::BudgetClass::Ordinary, false),
        (s::BudgetClass::Ordinary, true),
        (s::BudgetClass::Closeout, true),
    ] {
        // None exhausts even state inspection. The other cases admit that read
        // but leave zero or one byte too little for the payload envelope.
        for payload_headroom in [None, Some(0), Some(envelope - 1)] {
            let (mut fixture, request) = local_event_budget_fixture();
            let root = fixture.app.state_root().to_owned();
            let head = closeout_head(&root);
            let state: State = serde_json::from_value(head.body.clone()).unwrap();
            let remaining = state.work_meta["work"].navigation_left;
            assert!(remaining.calls >= allocation.calls);
            assert!(remaining.scanned_bytes >= allocation.scanned_bytes);
            assert!(remaining.result_bytes >= allocation.result_bytes);
            let read_bytes = local_event_budget_read_bytes(&head);
            let headroom = payload_headroom.map_or(0, |bytes| read_bytes + bytes);
            let mut host =
                s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits())
                    .unwrap();
            let ledger = host.ledger().unwrap();
            let ordinary = ledger.ordinary.exposure().unwrap().navigation_bytes;
            let closeout = ledger.closeout.exposure().unwrap().navigation_bytes;
            let aggregate = ledger.limits.total.navigation_bytes - ordinary - closeout;
            let available = match class {
                s::BudgetClass::Ordinary => aggregate.min(
                    ledger.limits.total.navigation_bytes
                        - ledger.limits.protected_closeout.navigation_bytes
                        - ordinary,
                ),
                s::BudgetClass::Closeout => aggregate,
            };
            let pressure = host
                .reserve(
                    "local-event-navigation-pressure",
                    s::Resources {
                        navigation_bytes: available.checked_sub(headroom).unwrap(),
                        ..s::Resources::default()
                    },
                    class,
                )
                .unwrap();
            if unknown {
                host.settle(&pressure.id, s::Usage::Unknown).unwrap();
            }
            let before = closeout_snapshot(&root);
            assert!(matches!(
                fixture.app.deliver_local_event("operator", request, NOW),
                Err(RuntimeError::Store(s::StoreError::Quota {
                    dimension: s::Resource::NavigationBytes,
                    class: s::BudgetClass::Ordinary,
                }))
            ));
            assert_local_event_budget_read_only(
                before,
                closeout_snapshot(&root),
                if payload_headroom.is_some() {
                    read_bytes
                } else {
                    0
                },
            );
        }
    }
}

#[test]
fn local_event_navigation_holds_cover_success_read_bound_and_persistence_failures() {
    let allocation = config().navigation_per_call;
    let envelope = allocation.scanned_bytes as u64 + allocation.result_bytes as u64;
    let mut rendered_bytes = 0usize;
    for outcome in ["success", "read", "aggregate", "persistence"] {
        let (mut fixture, mut request) = local_event_budget_fixture();
        if outcome == "read" {
            request.max_delivery_bytes = 1;
        } else if outcome == "aggregate" {
            // The identical successful fixture supplies the exact rendered-body
            // bound. The containing observation necessarily needs more bytes.
            assert!(rendered_bytes > 0);
            request.max_delivery_bytes = rendered_bytes;
        }
        let root = fixture.app.state_root().to_owned();
        let fault = if outcome == "persistence" {
            let connection = Connection::open(root.join("state.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER fail_local_event_delivery BEFORE UPDATE ON store_cells
                 WHEN NEW.ns='object' AND NEW.k='runtime-state-v1'
                      AND json_type(NEW.data,'$.body.work_meta.work.local_dependencies.budget.event')='object'
                 BEGIN
                     SELECT CASE WHEN NOT EXISTS (
                         SELECT 1 FROM store_cells WHERE ns='reservation'
                           AND k LIKE 's:runtime-navigation:%'
                           AND json_extract(data,'$.state')='Reserved'
                           AND json_extract(data,'$.amount.navigation_bytes')>0
                     ) THEN RAISE(ABORT,'event navigation hold missing') END;
                     SELECT RAISE(ABORT,'injected event delivery failure');
                 END;",
                )
                .unwrap();
            Some(connection)
        } else {
            None
        };
        let head = closeout_head(&root);
        let read_bytes = local_event_budget_read_bytes(&head);
        let before = closeout_snapshot(&root);
        let before_ledger = fixture.app.ledger().unwrap();
        let result = fixture
            .app
            .deliver_local_event("operator", request.clone(), NOW);
        let succeeded = outcome == "success";
        if succeeded {
            let receipt = result.unwrap();
            assert_eq!(receipt.delivered_to, set(&["a"]));
            assert!(!receipt.duplicate);
            assert!(!receipt.independent_authentication && !receipt.external_subscription);
        } else {
            let error = result.unwrap_err();
            match outcome {
                "read" => assert!(matches!(
                    error,
                    RuntimeError::Files(f::FilesError::ReadOverflow { .. })
                )),
                "aggregate" => assert!(matches!(
                    error,
                    RuntimeError::Held(ref reason) if reason == "aggregate event delivery byte limit"
                )),
                "persistence" => {
                    assert!(error
                        .to_string()
                        .contains("injected event delivery failure"));
                }
                _ => unreachable!(),
            }
        }
        let after = closeout_snapshot(&root);
        let after_head = closeout_head(&root);
        let mut charged = head.clone();
        charged.revision += 1;
        let amounts = serde_json::to_value(allocation).unwrap();
        for dimension in [
            "calls",
            "scanned_bytes",
            "inspected_revisions",
            "matches",
            "result_bytes",
        ] {
            let remaining = head.body["work_meta"]["work"]["navigation_left"][dimension]
                .as_u64()
                .unwrap();
            let amount = amounts[dimension].as_u64().unwrap();
            charged.body["work_meta"]["work"]["navigation_left"][dimension] =
                json!(remaining.checked_sub(amount).unwrap());
        }
        assert_eq!(
            after_head.body["work_meta"]["work"]["navigation_left"],
            charged.body["work_meta"]["work"]["navigation_left"]
        );
        assert!(after.1.iter().any(|cell| {
            cell.0 == "revision"
                && serde_json::from_slice::<s::StoredObject>(&cell.2).unwrap() == charged
        }));
        if succeeded {
            let observation = after_head.body["observations"]
                .as_object()
                .unwrap()
                .values()
                .find(|value| value["kind"] == "local_dependency_event")
                .unwrap();
            rendered_bytes = observation["payload"]["result"]["context_bytes"]
                .as_u64()
                .unwrap() as usize;
            assert_eq!(observation["payload"]["origin"], "local_mock");
            assert_eq!(observation["payload"]["independent_authentication"], false);
            assert_eq!(observation["payload"]["external_subscription"], false);
            let used = &after_head.body["work_meta"]["work"]["local_dependencies"]["budget"]
                ["event"]["navigation"];
            let used_bytes =
                used["scanned_bytes"].as_u64().unwrap() + used["result_bytes"].as_u64().unwrap();
            assert!(used_bytes > 0 && used_bytes <= envelope);
        } else {
            // No payload, event, wake or other semantic change escaped failure.
            assert_eq!(after_head, charged);
        }
        let reservations = local_event_budget_reservations(&after);
        assert_eq!(reservations.len(), 1);
        let reservation = &reservations[0];
        assert_eq!(reservation.class, s::BudgetClass::Ordinary);
        assert_eq!(reservation.state, s::ReservationState::Reserved);
        assert_eq!(reservation.measured, None);
        assert_eq!(
            reservation.amount,
            s::Resources {
                navigation_bytes: envelope,
                ..s::Resources::default()
            }
        );
        assert_eq!(reservation.history.len(), 1);
        assert_eq!(reservation.history[0].status, s::ReceiptStatus::Reserved);
        assert_eq!(
            reservation.history[0].epoch,
            before.0["epoch"].as_u64().unwrap() + 1
        );
        // Verify the allocation binds the complete request, operator, time,
        // recipients, allowance and exact pre-debit/post-debit state heads.
        let request_hash = s::sha256_bytes(&serde_json::to_vec(&("operator", &request)).unwrap());
        let attempt_hash = s::sha256_bytes(
            &serde_json::to_vec(&(
                "local-event-navigation-v1",
                &request_hash,
                NOW,
                &vec!["a".to_owned()],
                allocation,
            ))
            .unwrap(),
        );
        let binding = serde_json::to_value((
            "runtime-navigation-v1",
            head.exact_ref().unwrap(),
            charged.exact_ref().unwrap(),
            attempt_hash,
            envelope,
        ))
        .unwrap();
        assert_eq!(
            reservation.id,
            s::ReservationId::standalone(format!(
                "runtime-navigation:{}",
                s::sha256_bytes(&serde_json::to_vec(&binding).unwrap())
            ))
        );
        let added = after
            .1
            .iter()
            .filter(|cell| {
                !before
                    .1
                    .iter()
                    .any(|old| old.0 == cell.0 && old.1 == cell.1)
            })
            .collect::<Vec<_>>();
        assert_eq!(added.len(), 2 + usize::from(succeeded));
        assert!(added.iter().all(|cell| cell.4 == 0));
        let retention = added
            .iter()
            .map(|cell| 2 * (cell.3 as u64 + cell.0.len() as u64 + cell.1.len() as u64 + 128))
            .sum::<u64>();
        let mut expected = before_ledger;
        // Ordinary Store inspection is consumed navigation, separately from the
        // full payload envelope held in reserved navigation.
        expected.ordinary.consumed.navigation_bytes += read_bytes;
        expected.ordinary.consumed.retained_bytes += retention;
        expected.ordinary.reserved.navigation_bytes += envelope;
        expected.permanent_records += added.len() as u64;
        expected.allocated_history_events += 4;
        assert_eq!(fixture.app.ledger().unwrap(), expected);
        assert_eq!(
            after.0["sequence"].as_u64().unwrap(),
            before.0["sequence"].as_u64().unwrap() + 1
        );
        assert_eq!(
            fixture.app.epoch().unwrap(),
            before.0["epoch"].as_u64().unwrap() + 1 + u64::from(succeeded)
        );
        if let Some(connection) = fault {
            connection
                .execute_batch("DROP TRIGGER fail_local_event_delivery")
                .unwrap();
        }
        let reopen_bytes = local_event_budget_read_bytes(&after_head);
        drop(fixture.app);
        fixture.app = Runtime::open(&root).unwrap();
        assert_local_event_budget_read_only(after, closeout_snapshot(&root), reopen_bytes);
        if succeeded {
            let before_replay = closeout_snapshot(&root);
            // The original work version is now stale. This is receipt-only
            // replay, with no additional envelope, work debit or journal.
            assert!(
                fixture
                    .app
                    .deliver_local_event("operator", request, NOW + 1)
                    .unwrap()
                    .duplicate
            );
            assert_local_event_budget_read_only(
                before_replay,
                closeout_snapshot(&root),
                reopen_bytes,
            );
        } else if outcome == "read" {
            let before_retry = fixture.app.ledger().unwrap();
            assert!(matches!(
                fixture
                    .app
                    .deliver_local_event("operator", request, NOW + 1),
                Err(RuntimeError::Files(f::FilesError::ReadOverflow { .. }))
            ));
            let retried = local_event_budget_reservations(&closeout_snapshot(&root));
            assert_eq!(retried.len(), 2);
            assert_ne!(retried[0].id, retried[1].id);
            assert!(retried.contains(reservation));
            assert_eq!(
                fixture
                    .app
                    .ledger()
                    .unwrap()
                    .ordinary
                    .reserved
                    .navigation_bytes,
                before_retry.ordinary.reserved.navigation_bytes + envelope
            );
            assert_eq!(
                fixture
                    .app
                    .ledger()
                    .unwrap()
                    .ordinary
                    .consumed
                    .navigation_bytes,
                before_retry.ordinary.consumed.navigation_bytes + reopen_bytes
            );
        }
    }
}

#[test]
fn local_event_navigation_late_admission_failure_rolls_back_hold_and_work_debit() {
    let (mut fixture, request) = local_event_budget_fixture();
    let root = fixture.app.state_root().to_owned();
    let head = closeout_head(&root);
    let read_bytes = local_event_budget_read_bytes(&head);
    let connection = Connection::open(root.join("state.sqlite3")).unwrap();
    // Flush order has already written the changed head and reservation before
    // inserting the immutable revision. Abort there to exercise SQLite rollback.
    connection
        .execute_batch(
            "CREATE TRIGGER fail_local_event_navigation BEFORE INSERT ON store_cells
         WHEN NEW.ns='revision' AND json_extract(NEW.data,'$.id')='runtime-state-v1'
         BEGIN
             SELECT CASE WHEN NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='object' AND k='runtime-state-v1'
                   AND json_extract(data,'$.revision')=json_extract(NEW.data,'$.revision')
             ) OR NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='reservation'
                   AND k LIKE 's:runtime-navigation:%'
                   AND json_extract(data,'$.state')='Reserved'
                   AND json_extract(data,'$.amount.navigation_bytes')>0
             ) THEN RAISE(ABORT,'atomic event navigation writes missing') END;
             SELECT RAISE(ABORT,'injected event navigation admission failure');
         END;",
        )
        .unwrap();
    let before = closeout_snapshot(&root);
    let error = fixture
        .app
        .deliver_local_event("operator", request.clone(), NOW)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("injected event navigation admission failure"));
    assert_local_event_budget_read_only(before, closeout_snapshot(&root), read_bytes);
    assert_eq!(closeout_head(&root), head);
    connection
        .execute_batch("DROP TRIGGER fail_local_event_navigation")
        .unwrap();
    drop(connection);
    let durable = closeout_snapshot(&root);
    drop(fixture.app);
    fixture.app = Runtime::open(&root).unwrap();
    assert_local_event_budget_read_only(durable, closeout_snapshot(&root), read_bytes);
    let before_retry = fixture.app.ledger().unwrap();
    assert!(
        !fixture
            .app
            .deliver_local_event("operator", request, NOW)
            .unwrap()
            .duplicate
    );
    let allocation = config().navigation_per_call;
    assert_eq!(
        fixture
            .app
            .ledger()
            .unwrap()
            .ordinary
            .reserved
            .navigation_bytes,
        before_retry.ordinary.reserved.navigation_bytes
            + allocation.scanned_bytes as u64
            + allocation.result_bytes as u64
    );
    assert_eq!(
        local_event_budget_reservations(&closeout_snapshot(&root)).len(),
        1
    );
}

#[test]
fn frozen_local_event_admission_cannot_allocate_or_write_the_work_debit() {
    let (mut fixture, request) = local_event_budget_fixture();
    let root = fixture.app.state_root().to_owned();
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let trip = host
        .reserve(
            "local-event-freeze-trip",
            s::Resources {
                output_bytes: 1,
                ..s::Resources::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    assert!(matches!(
        host.settle(
            &trip.id,
            s::Usage::Measured(s::Resources {
                output_bytes: 2,
                ..s::Resources::default()
            })
        ),
        Err(s::StoreError::Overrun(_))
    ));
    let read_bytes = local_event_budget_read_bytes(&closeout_head(&root));
    let before = closeout_snapshot(&root);
    assert!(matches!(
        fixture.app.deliver_local_event("operator", request, NOW),
        Err(RuntimeError::Store(s::StoreError::Frozen))
    ));
    assert_local_event_budget_read_only(before, closeout_snapshot(&root), read_bytes);
}

#[test]
fn event_bounds_stale_binding_conflict_and_source_withdrawal_fail_without_delivery() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let dependency = register(&mut fixture, "bounded");
    for variant in 0..4 {
        let mut request = event_request(&mut fixture.app, &dependency);
        match variant {
            0 => request.binding_revision.push('x'),
            1 => request.binding.version.revision += 1,
            2 => request.observation.revision.push('x'),
            _ => request.max_delivery_bytes = 1,
        }
        let before = persisted_state(&fixture.app);
        let attempts = fixture.app.ledger().unwrap().ordinary.consumed.attempts;
        assert!(fixture
            .app
            .deliver_local_event("operator", request, NOW)
            .is_err());
        let mut after = persisted_state(&fixture.app);
        if variant == 3 {
            let old_navigation = &before["work_meta"]["work"]["navigation_left"];
            assert!(
                after["work_meta"]["work"]["navigation_left"]["calls"]
                    .as_u64()
                    .unwrap()
                    < old_navigation["calls"].as_u64().unwrap()
            );
            after["work_meta"]["work"]["navigation_left"] = old_navigation.clone();
        }
        assert_eq!(after, before);
        assert_eq!(
            fixture.app.ledger().unwrap().ordinary.consumed.attempts,
            attempts
        );
    }
    let request = event_request(&mut fixture.app, &dependency);
    fixture
        .app
        .deliver_local_event("operator", request, NOW)
        .unwrap();
    let mut conflict = event_request(&mut fixture.app, &dependency);
    conflict.id = "different-event".into();
    let before = persisted_state(&fixture.app);
    assert!(fixture
        .app
        .deliver_local_event("operator", conflict, NOW)
        .is_err());
    assert_eq!(persisted_state(&fixture.app), before);
    let second = register(&mut fixture, "withdrawn");
    fixture
        .app
        .withdraw_source("operator", &second.source.source, "Withdraw exact source.")
        .unwrap();
    let request = event_request(&mut fixture.app, &second);
    let before = persisted_state(&fixture.app);
    assert!(fixture
        .app
        .deliver_local_event("operator", request, NOW)
        .is_err());
    assert_eq!(persisted_state(&fixture.app), before);
}

#[test]
fn scheduler_does_not_resume_local_stop_task_pause_cancel_or_exhausted_allowance() {
    for mode in ["stop", "pause", "cancel", "exhausted"] {
        let mut configuration = config();
        if mode == "exhausted" {
            configuration.max_work_attempts = 1;
        }
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        if mode == "stop" {
            let prepared = fixture.prepare("local-stop");
            let mut response = decision(&prepared, true);
            response.continuation = p::Continuation::Stop {
                reason: "Explicit local stop.".into(),
            };
            submit(&mut fixture.app, &prepared, &response);
        } else if mode == "pause" {
            fixture
                .app
                .pause("operator", "work", "Explicit task pause.")
                .unwrap();
        } else if mode == "cancel" {
            fixture
                .app
                .closeout("operator", "work", true, "Explicit cancellation.")
                .unwrap();
        }
        let attempts = fixture.app.ledger().unwrap().ordinary.consumed.attempts;
        let resources = fixture.app.snapshot().unwrap().work_meta["work"].attempts_left;
        let result = schedule(&mut fixture.app, "blocked", 128, NOW + 1000);
        assert!(result.prepared.is_none());
        assert_eq!(
            fixture.app.snapshot().unwrap().work_meta["work"].attempts_left,
            resources
        );
        assert_eq!(
            fixture.app.ledger().unwrap().ordinary.consumed.attempts,
            attempts
        );
        assert_eq!(fixture.app.ledger().unwrap().closeout.consumed.attempts, 0);
    }
}

#[test]
fn paused_event_is_retained_without_wake_or_resume_and_canceled_binding_is_stale() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let dependency = register(&mut fixture, "paused");
    let prepared = fixture.prepare("wait-before-pause");
    submit(
        &mut fixture.app,
        &prepared,
        &wait_decision(&prepared, &dependency),
    );
    fixture
        .app
        .pause("operator", "work", "Pause before event.")
        .unwrap();
    let request = event_request(&mut fixture.app, &dependency);
    fixture
        .app
        .deliver_local_event("operator", request, NOW)
        .unwrap();
    let state = fixture.app.snapshot().unwrap();
    assert!(state
        .board
        .participant(&w::WorkId("work".into()), &w::PersonaId("a".into()))
        .unwrap()
        .wake
        .is_none());
    assert!(schedule(&mut fixture.app, "paused-event", 1, NOW + 100)
        .prepared
        .is_none());
    fixture
        .app
        .closeout("operator", "work", true, "Cancel after receipt.")
        .unwrap();
    let mut request = event_request(&mut fixture.app, &dependency);
    let old = persisted_state(&fixture.app);
    let epoch = fixture.app.epoch().unwrap();
    assert!(
        fixture
            .app
            .deliver_local_event("operator", request.clone(), NOW)
            .unwrap()
            .duplicate
    );
    assert_eq!(persisted_state(&fixture.app), old);
    assert_eq!(fixture.app.epoch().unwrap(), epoch);
    request.id.push_str("-new-delivery");
    assert!(fixture
        .app
        .deliver_local_event("operator", request, NOW)
        .is_err());
    assert!(schedule(&mut fixture.app, "canceled-event", 1, NOW + 100)
        .prepared
        .is_none());
}

fn accept_pair(app: &mut Runtime, work: &str, actor: &str) {
    let prepared = app
        .prepare(
            PrepareRequest {
                id: format!("accept-{work}-{actor}"),
                persona: actor.into(),
                work: work.into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let core = control(&prepared);
    let offer = &core["own_pending_offers"][0];
    let mut response = decision(&prepared, false);
    response.operations.push(p::Operation::Responsibility {
        id: offer["id"].as_str().unwrap().into(),
        work_id: work.into(),
        terms_revision: offer["terms_revision"].as_str().unwrap().into(),
        disposition: "accept".into(),
        reason: "I accept the exact supplied commitment.".into(),
    });
    submit(app, &prepared, &response);
}

#[test]
fn persisted_round_robin_crosses_paused_future_works_and_multiple_personas() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = directory.path().join("state");
    let mut app = Runtime::create(&root, config(), "operator").unwrap();
    for actor in ["a", "b"] {
        let (ocean, vad) = profiles();
        app.import_persona(
            "operator",
            PersonaImport {
                seed: seed(actor),
                ocean,
                vad,
                voice: record(
                    &format!("voice-{actor}"),
                    f::Scope::Fragments,
                    "I retain the actual scope.",
                ),
            },
            NOW,
        )
        .unwrap();
    }
    for id in ["00-paused", "10-active", "20-active"] {
        let mut input = need();
        input.id = id.into();
        let mut peer = input.invitations[0].clone();
        peer.id = "offer-b".into();
        peer.to = "b".into();
        peer.terms.outcomes.clear();
        input.invitations.push(peer);
        app.add_need("operator", input).unwrap();
        accept_pair(&mut app, id, "a");
        accept_pair(&mut app, id, "b");
    }
    app.pause(
        "operator",
        "00-paused",
        "Earlier paused entries must not hide later work.",
    )
    .unwrap();
    let mut admitted = Vec::new();
    for index in 0..12 {
        let result = schedule(&mut app, &format!("round-{index}"), 1, NOW);
        assert_eq!(result.scheduler.considered, index + 1);
        if let Some(prepared) = result.prepared {
            admitted.push((
                prepared.request.work.clone(),
                prepared.request.persona.clone(),
            ));
            submit(&mut app, &prepared, &decision(&prepared, true));
        }
        drop(app);
        app = Runtime::open(&root).unwrap();
    }
    let cycle = vec![
        ("10-active".to_owned(), "a".to_owned()),
        ("10-active".to_owned(), "b".to_owned()),
        ("20-active".to_owned(), "a".to_owned()),
        ("20-active".to_owned(), "b".to_owned()),
    ];
    assert_eq!(
        admitted,
        cycle.iter().chain(&cycle).cloned().collect::<Vec<_>>()
    );
    assert_eq!(app.ledger().unwrap().ordinary.consumed.attempts, 14);
    assert_eq!(app.ledger().unwrap().closeout.consumed.attempts, 0);
}

#[test]
fn future_wait_before_eligible_work_does_not_block_a_bounded_page() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let dependency = register(&mut fixture, "future");
    let prepared = fixture.prepare("future-wait");
    submit(
        &mut fixture.app,
        &prepared,
        &wait_decision(&prepared, &dependency),
    );
    let mut other = need();
    other.id = "z-work".into();
    fixture.app.add_need("operator", other).unwrap();
    accept_pair(&mut fixture.app, "z-work", "a");
    let first = schedule(&mut fixture.app, "future-considered", 1, NOW);
    assert!(first.prepared.is_none());
    assert_eq!(first.scheduler.after.unwrap().0 .0, "work");
    let second = schedule(&mut fixture.app, "later-admitted", 1, NOW);
    assert_eq!(second.prepared.unwrap().request.work, "z-work");
}

#[test]
fn scheduler_source_expiry_blocks_preparation_without_spending_an_attempt() {
    let mut timed = restricted_policy();
    timed.expires_at_unix = Some(NOW + 1);
    let mut configuration = config();
    configuration.policies.active_policies.push(timed.clone());
    let mut input = need();
    input.policy_id = timed.id;
    let mut fixture = Fixture::configured(true, configuration, seed("a"), input);
    fixture.accept();
    let attempts = fixture.app.ledger().unwrap().ordinary.consumed.attempts;
    let result = schedule(&mut fixture.app, "expired", 1, NOW + 1);
    assert!(result.prepared.is_none());
    assert!(matches!(
        result.scheduler.last[0].disposition,
        SchedulerDisposition::Blocked { .. }
    ));
    assert!(fixture.app.invocation("expired").is_err());
    assert_eq!(
        fixture.app.ledger().unwrap().ordinary.consumed.attempts,
        attempts
    );
}

#[tokio::test(flavor = "current_thread")]
async fn executable_dependency_delivery_inspection_and_scheduler_prepare_remain_local() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let source = record(
        "cli-event-source",
        f::Scope::Assets,
        "CLI LOCAL EVENT PAYLOAD",
    );
    fixture
        .app
        .import_files("operator", "a", vec![source.clone()], true, NOW)
        .unwrap();
    let root = fixture.app.state_root().to_owned();
    let request = RegisterDependencyRequest {
        binding: authority(&mut fixture.app, "work"),
        id: "cli-source".into(),
        source: reference(&source),
        participants: set(&["a"]),
        accountability: w::Accountability::Owner(w::PersonaId("a".into())),
        origin: LocalEventOrigin::OperatorLocal,
    };
    let path = fixture_json(fixture._temp.path(), "register.json", &request);
    let dependency: LocalDependency = serde_json::from_value(cli_success(
        cli_output(&root, &["register-dependency", path.to_str().unwrap()]).await,
    ))
    .unwrap();
    let prepared = fixture.prepare("cli-wait");
    let mut response = wait_decision(&prepared, &dependency);
    // Use the actual clock for an executable deadline, without granting resources.
    response.continuation = p::Continuation::Wait {
        dependency_ids: vec![dependency.id.clone()],
        deadline_unix: (unix_now().unwrap().max(NOW) + 3600) as u64,
        reason: "Await the actual bounded local event.".into(),
    };
    submit(&mut fixture.app, &prepared, &response);
    let request = event_request(&mut fixture.app, &dependency);
    let path = fixture_json(fixture._temp.path(), "event.json", &request);
    let receipt =
        cli_success(cli_output(&root, &["deliver-local-event", path.to_str().unwrap()]).await);
    assert_eq!(receipt["duplicate"], false);
    assert_eq!(receipt["external_subscription"], false);
    assert_eq!(receipt["independent_authentication"], false);
    let view = cli_success(cli_output(&root, &["scheduler-inspect", "--max-entries", "1"]).await);
    assert_eq!(view["queue"][0]["disposition"]["kind"], "queued");
    let attempts = fixture.app.ledger().unwrap().ordinary.consumed.attempts;
    let result = cli_success(
        cli_output(
            &root,
            &[
                "scheduler-prepare",
                "--invocation",
                "cli-scheduled",
                "--max-considerations",
                "1",
            ],
        )
        .await,
    );
    let prepared: Prepared = serde_json::from_value(result["prepared"].clone()).unwrap();
    assert!(prepared.prompt.contains("CLI LOCAL EVENT PAYLOAD"));
    assert_eq!(prepared.request.profile, Profile::Synthetic);
    assert!(prepared.provider_preparation.is_none());
    assert_eq!(
        fixture.app.invocation("cli-scheduled").unwrap().state,
        s::InvocationState::Prepared
    );
    assert!(fixture.app.terminal_evidence("cli-scheduled").is_err());
    assert_eq!(
        fixture.app.ledger().unwrap().ordinary.consumed.attempts,
        attempts + 1
    );
    let epoch = fixture.app.epoch().unwrap();
    let mut duplicate = request;
    duplicate.binding = authority(&mut fixture.app, "work");
    let path = fixture_json(fixture._temp.path(), "duplicate.json", &duplicate);
    assert_eq!(
        cli_success(cli_output(&root, &["deliver-local-event", path.to_str().unwrap()]).await)
            ["duplicate"],
        true
    );
    assert_eq!(fixture.app.epoch().unwrap(), epoch);
    assert!(!cli_output(
        &root,
        &[
            "scheduler-prepare",
            "--invocation",
            "invalid-bound",
            "--max-considerations",
            "0"
        ]
    )
    .await
    .status
    .success());
}
