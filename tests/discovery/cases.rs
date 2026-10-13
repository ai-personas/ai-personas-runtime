//! Only synthetic public fixtures; no inference or useful-work claim.
use super::*;

fn listing() -> f::DiscoveryRequest {
    f::DiscoveryRequest {
        scope: f::Scope::Assets,
        surface: f::SearchSurface::Path,
        mode: f::SearchMode::Listing,
        hidden_paths: f::HiddenPaths::Include,
        retirement: f::RetirementFilter::AllRetained,
    }
}

fn discover(
    id: &str,
    request: f::DiscoveryRequest,
    limit: usize,
    cursor: Option<String>,
) -> p::Operation {
    p::Operation::Discover {
        id: id.into(),
        request,
        limit,
        cursor,
    }
}

fn run_at(fixture: &mut Fixture, id: &str, now: i64, operations: Vec<p::Operation>) -> Prepared {
    let prepared = fixture
        .app
        .prepare(
            PrepareRequest {
                id: id.into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            now,
        )
        .unwrap();
    let mut response = decision(&prepared, true);
    response.operations = operations;
    fixture.app.dispatch_synthetic(id, now).unwrap();
    let report = fixture
        .app
        .adopt_response(id, &serde_json::to_vec(&response).unwrap(), now)
        .unwrap();
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    prepared
}

fn payload(fixture: &mut Fixture, invocation: &str) -> Value {
    fixture
        .app
        .snapshot()
        .unwrap()
        .observations
        .into_values()
        .find(|o| o.invocation.as_deref() == Some(invocation) && o.kind == "discovery")
        .unwrap()
        .payload
}

fn populated(count: usize) -> Fixture {
    let mut configuration = config();
    configuration.context_bytes = 1024 * 1024;
    configuration.navigation_per_call.result_bytes = 256 * 1024;
    configuration.navigation_per_work.result_bytes = 8 * 1024 * 1024;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    let records = (0..count)
        .map(|i| {
            record(
                &format!("item-{i:03}"),
                f::Scope::Assets,
                "searchable needle body",
            )
        })
        .collect();
    fixture
        .app
        .import_files("operator", "a", records, false, NOW)
        .unwrap();
    fixture
}

#[test]
fn discover_explicit_pages_survive_restart_and_clock_change_and_exceed_legacy_cap() {
    let mut fixture = populated(132);
    run_at(
        &mut fixture,
        "page-one",
        NOW,
        vec![discover("list", listing(), 130, None)],
    );
    let first = payload(&mut fixture, "page-one");
    assert_eq!(first["page"]["hits"].as_array().unwrap().len(), 130);
    assert_eq!(first["page"]["coverage_complete"], false);
    assert_eq!(first["continuation_available"], true);
    assert!(!first.to_string().contains("searchable needle body"));
    let handle = first["next_cursor"].as_str().unwrap().to_owned();
    let root = fixture.app.state_root().to_owned();
    fixture.app = Runtime::open(&root).unwrap();
    let prepared = run_at(
        &mut fixture,
        "page-two",
        NOW + 3,
        vec![discover("next", listing(), 130, Some(handle))],
    );
    assert!(prepared.prompt.contains("next_cursor"));
    assert!(!prepared.prompt.contains("next_index"));
    let second = payload(&mut fixture, "page-two");
    assert_eq!(second["page"]["hits"].as_array().unwrap().len(), 2);
    assert_eq!(second["page"]["hits"][0]["reference"]["source"], "item-130");
    assert_eq!(second["page"]["coverage_complete"], true);
    assert_eq!(second["next_cursor"], Value::Null);
}

#[test]
fn discover_path_matching_and_projected_body_search_are_distinct() {
    let mut fixture = populated(2);
    let mut request = listing();
    request.mode = f::SearchMode::Literal {
        needle: "item-001".into(),
    };
    run_at(
        &mut fixture,
        "path-match",
        NOW,
        vec![discover("path", request, 8, None)],
    );
    let page = payload(&mut fixture, "path-match");
    assert_eq!(page["page"]["hits"].as_array().unwrap().len(), 1);
    assert_eq!(page["page"]["hits"][0]["reference"]["source"], "item-001");
    let json_record = record(
        "projection",
        f::Scope::Assets,
        r#"{"field":"decoded needle"}"#,
    );
    fixture
        .app
        .import_files("operator", "a", vec![json_record], false, NOW)
        .unwrap();
    let mut request = listing();
    request.surface = f::SearchSurface::Text {
        representation: f::Representation::JsonString {
            pointer: "/field".into(),
        },
    };
    request.mode = f::SearchMode::Literal {
        needle: "decoded needle".into(),
    };
    // The two earlier plain-text projections are unavailable. Each requested
    // continuation advances one inspected revision without claiming full coverage.
    let mut cursor = None;
    for index in 0..3 {
        let invocation = format!("projection-{index}");
        run_at(
            &mut fixture,
            &invocation,
            NOW + index,
            vec![discover("body", request.clone(), 8, cursor)],
        );
        let result = payload(&mut fixture, &invocation);
        assert_eq!(result["page"]["coverage_complete"], false);
        cursor = result["next_cursor"].as_str().map(str::to_owned);
        if index == 2 {
            assert_eq!(
                result["page"]["hits"][0]["reference"]["source"],
                "projection"
            );
            assert!(cursor.is_none());
        } else {
            assert!(cursor.is_some());
        }
    }
}

#[test]
fn discover_binary_body_can_be_explicitly_passed_but_is_never_complete_coverage() {
    let mut fixture = populated(1);
    let mut binary = record("a-binary", f::Scope::Assets, "temporary");
    binary.bytes = vec![0xff, 0xfe];
    binary = binary.seal().unwrap();
    fixture
        .app
        .import_files("operator", "a", vec![binary], false, NOW)
        .unwrap();
    let mut request = listing();
    request.surface = f::SearchSurface::Text {
        representation: f::Representation::Utf8,
    };
    request.mode = f::SearchMode::Literal {
        needle: "needle".into(),
    };
    run_at(
        &mut fixture,
        "binary-first",
        NOW,
        vec![discover("body", request.clone(), 8, None)],
    );
    let first = payload(&mut fixture, "binary-first");
    assert_eq!(first["page"]["status"], "unavailable");
    let handle = first["next_cursor"].as_str().unwrap().to_owned();
    run_at(
        &mut fixture,
        "binary-next",
        NOW + 1,
        vec![discover("body", request, 8, Some(handle))],
    );
    let second = payload(&mut fixture, "binary-next");
    assert_eq!(second["page"]["hits"][0]["reference"]["source"], "item-000");
    assert_eq!(second["page"]["coverage_complete"], false);
    assert!(second["next_cursor"].is_null());
}

#[test]
fn discover_unknown_changed_query_and_changed_view_cursors_fail_closed() {
    for mutation in ["unknown", "query", "view"] {
        let mut fixture = populated(2);
        run_at(
            &mut fixture,
            "start",
            NOW,
            vec![discover("list", listing(), 1, None)],
        );
        let mut handle = payload(&mut fixture, "start")["next_cursor"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut request = listing();
        match mutation {
            "unknown" => handle = "discovery:invented".into(),
            "query" => request.hidden_paths = f::HiddenPaths::Exclude,
            "view" => fixture
                .app
                .import_files(
                    "operator",
                    "a",
                    vec![record("new-item", f::Scope::Assets, "new")],
                    false,
                    NOW,
                )
                .unwrap(),
            _ => unreachable!(),
        }
        run_at(
            &mut fixture,
            "resume",
            NOW + 1,
            vec![discover("list", request, 1, Some(handle))],
        );
        let result = payload(&mut fixture, "resume");
        assert_eq!(result["status"], "unavailable", "{mutation}");
        assert_eq!(result["coverage_complete"], false);
        assert!(result["next_cursor"].is_null());
        assert!(result["page"].is_null());
    }
}

#[test]
fn discover_and_legacy_search_share_the_same_call_allowance() {
    let mut configuration = config();
    configuration.navigation_per_call.calls = 1;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![record("item", f::Scope::Assets, "needle")],
            false,
            NOW,
        )
        .unwrap();
    run_at(
        &mut fixture,
        "shared-budget",
        NOW,
        vec![
            p::Operation::Search {
                id: "legacy".into(),
                scope: p::Scope::Assets {},
                query: "needle".into(),
                regex: false,
                limit: 8,
            },
            discover("second", listing(), 8, None),
        ],
    );
    let snapshot = fixture.app.snapshot().unwrap();
    let observations = snapshot
        .observations
        .values()
        .filter(|o| o.invocation.as_deref() == Some("shared-budget"))
        .collect::<Vec<_>>();
    let legacy = observations
        .iter()
        .find(|o| o.payload["cursor_retained"] == false)
        .unwrap();
    assert!(legacy.payload["page"].is_string());
    let explicit = observations
        .iter()
        .find(|o| o.payload["request"]["surface"]["kind"] == "path")
        .unwrap();
    assert_eq!(explicit.payload["status"], "unavailable");
    let accounting = observations
        .iter()
        .find(|o| o.kind == "host_accounting")
        .unwrap();
    assert_eq!(accounting.payload["actual_navigation"]["calls"], 1);
    assert!(
        accounting.payload["actual_navigation"]["result_bytes"]
            .as_u64()
            .unwrap()
            <= 32 * 1024
    );
}

#[test]
fn discover_continuation_rechecks_expiry_and_revocation_before_supplying_metadata() {
    for revoke in [false, true] {
        let mut restricted = restricted_policy();
        if !revoke {
            restricted.expires_at_unix = Some(NOW + 2);
        }
        let mut configuration = config();
        configuration
            .policies
            .active_policies
            .push(restricted.clone());
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        let records = ["restricted-a", "restricted-b"]
            .into_iter()
            .map(|source| {
                let mut item = record(source, f::Scope::Assets, "private body");
                item.policy = restricted.clone();
                item.seal().unwrap()
            })
            .collect();
        fixture
            .app
            .import_files("operator", "a", records, false, NOW)
            .unwrap();
        run_at(
            &mut fixture,
            "permitted-page",
            NOW,
            vec![discover("list", listing(), 1, None)],
        );
        let handle = payload(&mut fixture, "permitted-page")["next_cursor"]
            .as_str()
            .unwrap()
            .to_owned();
        if revoke {
            fixture
                .app
                .revoke_policy("operator", restricted.identity(), "synthetic revocation")
                .unwrap();
        }
        let prepared = run_at(
            &mut fixture,
            "denied-page",
            NOW + 2,
            vec![discover("next", listing(), 1, Some(handle))],
        );
        assert!(!prepared.prompt.contains("restricted-a.txt"));
        assert!(!prepared.prompt.contains("restricted-b.txt"));
        let result = payload(&mut fixture, "denied-page");
        assert_eq!(result["status"], "unavailable");
        assert_eq!(result["coverage_complete"], false);
        assert!(result["page"].is_null());
    }
}

#[test]
fn discover_cursor_cannot_cross_work_focus_or_be_predicted_in_its_creation_response() {
    let mut fixture = populated(2);
    // Even knowledge of the deterministic handle cannot use an unobserved result.
    let guessed = format!(
        "discovery:{}",
        s::sha256_bytes(&serde_json::to_vec(&("creation", "first")).unwrap())
    );
    run_at(
        &mut fixture,
        "creation",
        NOW,
        vec![
            discover("first", listing(), 1, None),
            discover("guessed", listing(), 1, Some(guessed)),
        ],
    );
    let observations = fixture.app.snapshot().unwrap().observations;
    let result = observations
        .values()
        .find(|o| {
            o.invocation.as_deref() == Some("creation") && o.payload["status"] == "unavailable"
        })
        .unwrap();
    assert!(result.payload["page"].is_null());
    let handle = observations
        .values()
        .find(|o| {
            o.invocation.as_deref() == Some("creation")
                && o.payload["continuation_available"] == true
        })
        .unwrap()
        .payload["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();

    let mut other = need();
    other.id = "other-work".into();
    fixture.app.add_need("operator", other).unwrap();
    let prepared = fixture
        .app
        .prepare(
            PrepareRequest {
                id: "cross-work".into(),
                persona: "a".into(),
                work: "other-work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW + 1,
        )
        .unwrap();
    let mut response = decision(&prepared, false);
    let snapshot = fixture.app.snapshot().unwrap();
    let other_work = snapshot
        .board
        .work(&w::WorkId("other-work".into()))
        .unwrap();
    response.operations.push(p::Operation::Responsibility {
        id: "offer-a".into(),
        work_id: "other-work".into(),
        terms_revision: offer_revision(other_work, &w::RecordId("offer-a".into())).unwrap(),
        disposition: "accept".into(),
        reason: "Accept the synthetic second work.".into(),
    });
    response
        .operations
        .push(discover("cross", listing(), 1, Some(handle)));
    fixture
        .app
        .dispatch_synthetic("cross-work", NOW + 1)
        .unwrap();
    let report = fixture
        .app
        .adopt_response(
            "cross-work",
            &serde_json::to_vec(&response).unwrap(),
            NOW + 1,
        )
        .unwrap();
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    let result = payload(&mut fixture, "cross-work");
    assert_eq!(result["status"], "unavailable");
    assert!(result["page"].is_null());
}

#[test]
fn discover_oversized_wrapper_uses_only_charged_fallback_without_scanning() {
    let mut configuration = config();
    configuration.navigation_per_call.result_bytes = 256;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![record("visible", f::Scope::Assets, "body")],
            false,
            NOW,
        )
        .unwrap();
    let mut request = listing();
    request.mode = f::SearchMode::Literal {
        needle: "x".repeat(1024),
    };
    run_at(
        &mut fixture,
        "bounded-envelope",
        NOW,
        vec![discover("too-large", request, 8, None)],
    );
    let result = payload(&mut fixture, "bounded-envelope");
    assert_eq!(result["status"], "exhausted");
    assert_eq!(result["coverage_complete"], false);
    assert!(result["request"].is_null());
    assert!(result["next_cursor"].is_null());
    let snapshot = fixture.app.snapshot().unwrap();
    let accounting = snapshot
        .observations
        .values()
        .find(|o| {
            o.invocation.as_deref() == Some("bounded-envelope") && o.kind == "host_accounting"
        })
        .unwrap();
    let used = &accounting.payload["actual_navigation"];
    assert_eq!(used["calls"], 1);
    assert_eq!(used["scanned_bytes"], 0);
    assert_eq!(used["inspected_revisions"], 0);
    assert_eq!(
        used["result_bytes"],
        serde_json::to_vec(&result).unwrap().len()
    );
}

// Saturate the real host bound in one synthetic invocation, then consume its
// observations without continuing them. This reproduces permanently orphaned
// handles preventing a later explicit query from retaining a continuation.
#[test]
fn discovery_cursor_lifecycle_reclaims_consumed_handles_at_capacity_and_after_restart() {
    let mut configuration = config();
    configuration.context_bytes = 1024 * 1024;
    configuration.response_bytes = 128 * 1024;
    configuration.output_reserve_bytes = 128 * 1024;
    configuration.corpus.max_revisions = 16;
    configuration.corpus.max_total_record_bytes = 64 * 1024;
    configuration.navigation_per_call = f::NavigationLimits {
        calls: 128,
        scanned_bytes: 16 * 1024 * 1024,
        inspected_revisions: 4096,
        matches: 256,
        result_bytes: 512 * 1024,
    };
    configuration.navigation_per_work = f::NavigationLimits {
        calls: 1024,
        scanned_bytes: 256 * 1024 * 1024,
        inspected_revisions: 32768,
        matches: 2048,
        result_bytes: 4 * 1024 * 1024,
    };
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![
                record("item-a", f::Scope::Assets, "a"),
                record("item-b", f::Scope::Assets, "b"),
            ],
            false,
            NOW,
        )
        .unwrap();
    run_at(
        &mut fixture,
        "fill-cursor-bound",
        NOW,
        (0..128)
            .map(|i| discover(&format!("list-{i}"), listing(), 1, None))
            .collect(),
    );
    let state = persisted_state(&fixture.app);
    let handles = state["personas"]["a"]["focus"]["work"]["discovery_cursors"]
        .as_object()
        .unwrap();
    assert_eq!(handles.len(), 128);
    let stale_handle = handles.keys().next().unwrap().clone();
    let root = fixture.app.state_root().to_owned();
    fixture.app = Runtime::open(&root).unwrap();

    run_at(
        &mut fixture,
        "replace-consumed-pages",
        NOW + 1,
        vec![discover("new-page", listing(), 1, None)],
    );
    let page = payload(&mut fixture, "replace-consumed-pages");
    assert_eq!(page["continuation_available"], true);
    assert!(page["next_cursor"].is_string());
    let state = persisted_state(&fixture.app);
    assert_eq!(
        state["personas"]["a"]["focus"]["work"]["discovery_cursors"]
            .as_object()
            .unwrap()
            .len(),
        1
    );

    run_at(&mut fixture, "consume-without-continuing", NOW + 2, vec![]);
    let state = persisted_state(&fixture.app);
    assert!(state["personas"]["a"]["focus"]["work"]["discovery_cursors"].is_null());
    fixture.app = Runtime::open(&root).unwrap();
    run_at(
        &mut fixture,
        "old-handle-after-reopen",
        NOW + 3,
        vec![discover("old", listing(), 1, Some(stale_handle))],
    );
    assert_eq!(
        payload(&mut fixture, "old-handle-after-reopen")["status"],
        "unavailable"
    );
}

fn retained_cursors(fixture: &Fixture) -> Value {
    persisted_state(&fixture.app)["personas"]["a"]["focus"]["work"]["discovery_cursors"].clone()
}

#[test]
fn discovery_cursor_lifecycle_preserves_held_and_failed_commit_observations() {
    for late_failure in [false, true] {
        let mut fixture = populated(2);
        run_at(
            &mut fixture,
            "protected-page",
            NOW,
            vec![discover("first", listing(), 1, None)],
        );
        let handle = payload(&mut fixture, "protected-page")["next_cursor"]
            .as_str()
            .unwrap()
            .to_owned();
        let before = retained_cursors(&fixture);
        let pending = fixture.app.snapshot().unwrap().personas["a"].focus["work"]
            .pending_observations
            .clone();
        let prepared = fixture.prepare("held-consumption");
        let mut response = decision(&prepared, true);
        let fault = if late_failure {
            let fault = Connection::open(fixture.app.state_root().join("state.sqlite3")).unwrap();
            fault
                .execute_batch(
                    "CREATE TRIGGER reject_cursor_adoption BEFORE UPDATE ON store_cells
                WHEN NEW.ns='object' AND NEW.k='runtime-state-v1'
                BEGIN SELECT RAISE(ABORT,'synthetic cursor adoption failure'); END;",
                )
                .unwrap();
            Some(fault)
        } else {
            response.operations.push(p::Operation::Effect {
                id: "unsupported".into(),
                capability_id: "unknown".into(),
                binding_revision: "unknown".into(),
                inputs: json!({}),
                depends_on: vec![],
            });
            None
        };
        fixture
            .app
            .dispatch_synthetic(&prepared.request.id, NOW)
            .unwrap();
        let result = fixture.app.adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&response).unwrap(),
            NOW,
        );
        if let Some(fault) = fault {
            assert!(result.is_err());
            fault
                .execute_batch("DROP TRIGGER reject_cursor_adoption;")
                .unwrap();
        } else {
            assert_eq!(result.unwrap().status, AdoptionStatus::Held);
        }
        assert_eq!(retained_cursors(&fixture), before);
        assert_eq!(
            fixture.app.snapshot().unwrap().personas["a"].focus["work"].pending_observations,
            pending
        );
        let root = fixture.app.state_root().to_owned();
        fixture.app = Runtime::open(&root).unwrap();
        assert_eq!(retained_cursors(&fixture), before);
        fixture.app.resume("operator", "a", "work").unwrap();
        run_at(
            &mut fixture,
            "after-held",
            NOW + 1,
            vec![discover("continue", listing(), 1, Some(handle))],
        );
        assert_eq!(
            payload(&mut fixture, "after-held")["page"]["coverage_complete"],
            true
        );
    }
}

#[test]
fn discovery_cursor_lifecycle_preserves_character_fenced_observations() {
    let mut fixture = populated(2);
    run_at(
        &mut fixture,
        "before-character",
        NOW,
        vec![discover("first", listing(), 1, None)],
    );
    let handle = payload(&mut fixture, "before-character")["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = retained_cursors(&fixture);
    let prepared = fixture.prepare("character-fence");
    let current = fixture.app.snapshot().unwrap().personas["a"]
        .character
        .clone()
        .unwrap();
    let mut response = decision(&prepared, true);
    let (mut ocean, vad) = profiles();
    ocean.insert(
        "openness".into(),
        "A changed synthetic character description.".into(),
    );
    response.operations = vec![
        discover("fenced-discovery", listing(), 1, Some(handle.clone())),
        p::Operation::CharacterAdopt {
            id: "character".into(),
            base_revision: prepared.character_revision.clone(),
            ocean,
            vad,
            own_voice: current.own_voice,
            reason: "Synthetic character fence.".into(),
            evidence: vec![],
        },
    ];
    response.fragment_maintenance = p::Maintenance::Change {
        reason: "Explicit synthetic character revision.".into(),
        operation_ids: vec!["character".into()],
    };
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::CharacterOnly);
    assert!(report.fenced.contains(&"fenced-discovery".into()));
    assert_eq!(retained_cursors(&fixture), before);
    let root = fixture.app.state_root().to_owned();
    fixture.app = Runtime::open(&root).unwrap();
    fixture.app.resume("operator", "a", "work").unwrap();
    run_at(
        &mut fixture,
        "after-character",
        NOW + 1,
        vec![discover("continue", listing(), 1, Some(handle))],
    );
    assert_eq!(
        payload(&mut fixture, "after-character")["page"]["coverage_complete"],
        true
    );
}

#[test]
fn discovery_cursor_lifecycle_keeps_unavailable_observations_pending() {
    let mut restricted = restricted_policy();
    restricted.expires_at_unix = Some(NOW + 1);
    let mut configuration = config();
    configuration
        .policies
        .active_policies
        .push(restricted.clone());
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    let records = ["private-a", "private-b"]
        .into_iter()
        .map(|source| {
            let mut record = record(source, f::Scope::Assets, "private");
            record.policy = restricted.clone();
            record.seal().unwrap()
        })
        .collect();
    fixture
        .app
        .import_files("operator", "a", records, false, NOW)
        .unwrap();
    run_at(
        &mut fixture,
        "before-expiry",
        NOW,
        vec![discover("first", listing(), 1, None)],
    );
    let before = retained_cursors(&fixture);
    let observation = before.as_object().unwrap().values().next().unwrap()["observation"]
        .as_str()
        .unwrap()
        .to_owned();
    let prepared = run_at(&mut fixture, "consume-permitted-only", NOW + 2, vec![]);
    assert!(!prepared.supplied_observations.contains(&observation));
    assert_eq!(retained_cursors(&fixture), before);
    assert!(fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .pending_observations
        .contains(&observation));
}

#[test]
fn discovery_cursor_lifecycle_preserves_selection_repair_observations() {
    let mut fixture = Fixture::with_config(true, repair_config(1));
    fixture.accept();
    let large = record("repair-large", f::Scope::Assets, &"L".repeat(90_000));
    let small = record("repair-small", f::Scope::Assets, "small");
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![large.clone(), small.clone()],
            false,
            NOW,
        )
        .unwrap();
    let prepared = fixture.prepare("repair-cursor-page");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select the exact fitting synthetic assets.".into(),
        references: vec![reference(&large), reference(&small)],
    };
    response
        .operations
        .push(discover("first", listing(), 1, None));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let handle = payload(&mut fixture, "repair-cursor-page")["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = retained_cursors(&fixture);
    let observation = before[&handle]["observation"].as_str().unwrap().to_owned();
    let snapshot = fixture.app.snapshot().unwrap();
    let fit = snapshot.personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap();
    let growth = snapshot.config.context_bytes - fit.ordinary_bytes + 8192;
    for index in 0..growth.div_ceil(8192) {
        fixture
            .app
            .record_human_evidence(
                "operator",
                "work",
                &format!("cursor-repair-growth-{index}"),
                &"Observed mandatory evidence. ".repeat(300),
                Vec::new(),
                set(&["result"]),
            )
            .unwrap();
    }
    assert!(matches!(
        fixture.app.prepare(
            PrepareRequest {
                id: "cursor-growth-block".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW
        ),
        Err(RuntimeError::SelectionOverflow { .. })
    ));
    let repair = fixture
        .app
        .prepare(repair_request("cursor-repair"), NOW)
        .unwrap();
    assert!(repair.supplied_observations.contains(&observation));
    assert_eq!(
        fixture
            .run(&repair, &repair_decision(&repair, false))
            .status,
        AdoptionStatus::Adopted
    );
    assert_eq!(retained_cursors(&fixture), before);
    assert!(fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .pending_observations
        .contains(&observation));
    let root = fixture.app.state_root().to_owned();
    fixture.app = Runtime::open(&root).unwrap();
    fixture.app.resume("operator", "a", "work").unwrap();
    run_at(
        &mut fixture,
        "continue-after-repair",
        NOW + 1,
        vec![discover("continue", listing(), 1, Some(handle))],
    );
    assert_eq!(
        payload(&mut fixture, "continue-after-repair")["page"]["coverage_complete"],
        true
    );
}

// A public cursor can become stale because a newly permitted private record
// joins its view. Even the unavailable result then depends on that private view.
fn stale_private_view() -> (Fixture, f::Policy, String) {
    let mut public = policy();
    public.readers.insert("c".into());
    public.audience.insert("c".into());
    let mut private = public.clone();
    private.id = "stale-private".into();
    private.revision = "stale-private-1".into();
    private.readers = set(&["a"]);
    let mut hidden = public.clone();
    hidden.id = "stale-hidden".into();
    hidden.revision = "stale-hidden-1".into();
    hidden.readers = set(&["c"]);
    let mut configuration = Config::local(f::PolicySnapshot {
        active_policies: vec![public.clone(), private.clone(), hidden.clone()],
        revoked_policies: BTreeSet::new(),
    });
    configuration.local_recipients = set(&["human"]);
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut app = Runtime::create(&temp.path().join("state"), configuration, "operator").unwrap();
    for actor in ["a", "c"] {
        let mut primary = seed(actor);
        primary.audience.insert("c".into());
        primary.grants.communicate_with.insert("c".into());
        let (ocean, vad) = profiles();
        let mut voice = record(
            &format!("voice-{actor}"),
            f::Scope::Fragments,
            "Synthetic public own voice.",
        );
        voice.policy = public.clone();
        app.import_persona(
            "operator",
            PersonaImport {
                seed: primary,
                ocean,
                vad,
                voice: voice.seal().unwrap(),
            },
            NOW,
        )
        .unwrap();
    }
    app.add_need("operator", need()).unwrap();
    let mut fixture = Fixture { _temp: temp, app };
    fixture.accept();
    let records = ["public-a", "public-b"]
        .into_iter()
        .map(|source| {
            let mut item = record(source, f::Scope::Assets, "PUBLIC BODY NOT SUPPLIED");
            item.policy = public.clone();
            item.seal().unwrap()
        })
        .collect();
    fixture
        .app
        .import_files("operator", "a", records, false, NOW)
        .unwrap();
    run_at(
        &mut fixture,
        "public-page",
        NOW,
        vec![
            discover("first", listing(), 1, None),
            p::Operation::Communicate {
                id: "public-control".into(),
                recipient: "c".into(),
                subject: "Public control".into(),
                text: "Public communication is permitted.".into(),
            },
        ],
    );
    let handle = payload(&mut fixture, "public-page")["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let records = [
        (
            "new-private-member",
            "PRIVATE BODY NOT SUPPLIED",
            private.clone(),
        ),
        ("hidden-member", "HIDDEN BODY NOT SUPPLIED", hidden),
    ]
    .into_iter()
    .map(|(source, body, policy)| {
        let mut item = record(source, f::Scope::Assets, body);
        item.policy = policy;
        item.seal().unwrap()
    })
    .collect();
    fixture
        .app
        .import_files("operator", "a", records, false, NOW)
        .unwrap();
    let prepared = run_at(
        &mut fixture,
        "stale-continuation",
        NOW,
        vec![discover("resume", listing(), 1, Some(handle))],
    );
    assert!(!prepared
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .iter()
        .any(|r| r.source == "new-private-member"));
    let snapshot = fixture.app.snapshot().unwrap();
    let observation = snapshot
        .observations
        .values()
        .find(|o| o.invocation.as_deref() == Some("stale-continuation") && o.kind == "discovery")
        .unwrap();
    assert_eq!(observation.payload["status"], "unavailable");
    assert_eq!(observation.payload["coverage_complete"], false);
    assert!(observation.payload["page"].is_null());
    assert!(observation.payload["next_cursor"].is_null());
    assert!(!observation
        .sources
        .iter()
        .any(|r| r.source == "hidden-member"));
    let rendered = observation.payload.to_string();
    for value in ["new-private-member", "hidden-member", "BODY NOT SUPPLIED"] {
        assert!(!rendered.contains(value));
    }
    assert!(observation.qualified_roots.is_empty());
    let id = observation.id.clone();
    (fixture, private, id)
}

#[test]
fn discovery_stale_private_view_blocks_forwarding_unavailable_result() {
    let (mut fixture, _, observation) = stale_private_view();
    let prepared = fixture.prepare("forward-stale-result");
    assert!(prepared.supplied_observations.contains(&observation));
    assert!(!prepared.prompt.contains("new-private-member"));
    assert!(!prepared.prompt.contains("hidden-member"));
    assert!(!prepared.prompt.contains("BODY NOT SUPPLIED"));
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Communicate {
        id: "forward".into(),
        recipient: "c".into(),
        subject: "Cursor result".into(),
        text: "The continuation is unavailable.".into(),
    });
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
    assert!(report
        .reason
        .unwrap()
        .contains("restricted supplied material"));
    assert_eq!(fixture.app.snapshot().unwrap().messages.len(), 1);
    assert!(prepared
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .iter()
        .any(|r| r.source == "new-private-member"));
    assert!(!prepared
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .iter()
        .any(|r| r.source == "hidden-member"));
}

#[test]
fn discovery_stale_private_view_revocation_withholds_result_and_accounting() {
    let (mut fixture, private, observation) = stale_private_view();
    fixture
        .app
        .revoke_policy(
            "operator",
            private.identity(),
            "Synthetic private-view revocation",
        )
        .unwrap();
    let prepared = fixture.prepare("after-stale-view-revocation");
    assert!(!prepared.supplied_observations.contains(&observation));
    let control = interface_control(&prepared);
    let actual = control["actual_observations"].as_array().unwrap();
    assert!(!actual.iter().any(|o| o["kind"] == "discovery"));
    assert!(!actual.iter().any(|o| o["kind"] == "host_accounting"));
    assert!(!prepared.prompt.contains("BODY NOT SUPPLIED"));
    assert!(fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .pending_observations
        .contains(&observation));
}
