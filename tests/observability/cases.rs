use super::*;

fn accepted_write(f: &mut Fixture, invocation: &str, text: &str, base: Option<String>) -> Prepared {
    let prepared = f.prepare(invocation);
    let mut d = decision(&prepared, true);
    d.operations
        .push(write("save", p::Scope::Assets {}, "note", text, base));
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Explicit synthetic file maintenance.".into(),
        operation_ids: vec!["save".into()],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    prepared
}

#[test]
fn ordinary_save_receipt_supplies_exact_next_write_base_without_selection() {
    let mut f = Fixture::new(true);
    f.accept();
    accepted_write(&mut f, "save-first", "first exact content", None);
    let prepared = f.prepare("revise-from-receipt");
    let core = interface_control(&prepared);
    let receipt = core["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "accepted_file_write")
        .expect("accepted file receipt");
    assert_eq!(receipt["payload"]["operation_id"], "save");
    assert_eq!(receipt["payload"]["source"]["source"], "note");
    let head = receipt["payload"]["source"]["revision"].as_str().unwrap();
    assert_eq!(head, f.app.snapshot().unwrap().sources["note"].head);
    assert_eq!(receipt["payload"]["source_owner"], "a");
    assert_eq!(receipt["payload"]["source_protected"], false);
    assert!(!prepared.prompt.contains("first exact content"));
    assert!(core["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut d = decision(&prepared, true);
    d.operations.push(write(
        "revise",
        p::Scope::Assets {},
        "note",
        "second exact content",
        Some(head.into()),
    ));
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Explicit synthetic file maintenance.".into(),
        operation_ids: vec!["revise".into()],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let next = f.prepare("receipt-after-update");
    let receipt = interface_control(&next)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "accepted_file_write")
        .unwrap()
        .clone();
    assert_eq!(receipt["payload"]["operation_id"], "revise");
    assert_eq!(
        receipt["payload"]["source"]["revision"],
        f.app.snapshot().unwrap().sources["note"].head
    );
}

#[test]
fn historical_discovery_exposes_only_known_source_and_permitted_current_head() {
    let mut f = Fixture::new(true);
    f.accept();
    accepted_write(&mut f, "save-old", "needle only in history", None);
    let old = f.app.snapshot().unwrap().sources["note"].head.clone();
    accepted_write(&mut f, "save-new", "current note body", Some(old.clone()));
    let unrelated = record("unrelated", f::Scope::Assets, "unrelated private inventory");
    f.app
        .import_files("operator", "a", vec![unrelated], false, NOW)
        .unwrap();
    let prepared = f.prepare("discover-old");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Search {
        id: "search".into(),
        scope: p::Scope::Assets {},
        query: "needle".into(),
        regex: false,
        limit: 8,
    });
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let next = f.prepare("observe-old");
    let core = interface_control(&next);
    let metadata = core["file_mutation_metadata"].as_array().unwrap();
    let note = metadata
        .iter()
        .find(|m| m["source"] == "note")
        .expect("discovered source mutation metadata");
    assert_eq!(note["source_owner"], "a");
    assert_eq!(note["source_protected"], false);
    assert_eq!(note["scope"], "assets");
    assert_eq!(
        note["current_head"]["revision"],
        f.app.snapshot().unwrap().sources["note"].head
    );
    assert_ne!(note["current_head"]["revision"], old);
    assert!(!metadata.iter().any(|m| m["source"] == "unrelated"));
    assert!(core["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!next.prompt.contains("current note body"));
}

#[test]
fn selection_control_reports_lifetime_and_expiration_without_renewal() {
    let mut f = Fixture::new(true);
    f.accept();
    let asset = record(
        "one-admission",
        f::Scope::Assets,
        "chosen exact optional body",
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("choose-once");
    let mut d = decision(&prepared, true);
    let mut exact = reference(&asset);
    exact.lifetime = Some("next_call".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "One admission.".into(),
        references: vec![exact.clone()],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let once = f.prepare("one-admission");
    let control = interface_control(&once);
    let selected = &control["selection_state"]["assets"][0];
    assert_eq!(selected["reference"], serde_json::to_value(&exact).unwrap());
    assert_eq!(selected["last_admission"], once.admission_serial);
    assert_eq!(selected["active_this_admission"], true);
    assert_eq!(selected["preserved_for_next_admission"], false);
    assert_eq!(
        control["navigation_current_admission"],
        serde_json::to_value(once.navigation).unwrap()
    );
    assert_eq!(
        control["selection_state"]["admission_serial"],
        once.admission_serial
    );
    assert_eq!(
        f.run(&once, &decision(&once, true)).status,
        AdoptionStatus::Adopted
    );
    let after = f.prepare("after-expiration");
    let control = interface_control(&after);
    assert!(control["selection_state"]["assets_initialized"]
        .as_bool()
        .unwrap());
    assert!(control["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!after.prompt.contains("chosen exact optional body"));
}

#[test]
fn failed_write_has_no_receipt_and_unknown_recovery_withholds_untracked_cause() {
    let mut f = Fixture::new(true);
    f.accept();
    accepted_write(&mut f, "original", "accepted content", None);
    let prepared = f.prepare("stale-update");
    let mut d = decision(&prepared, true);
    d.operations.push(write(
        "stale",
        p::Scope::Assets {},
        "note",
        "DO_NOT_EXPOSE_RAW_FAILED_BODY",
        Some("0".repeat(64)),
    ));
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Explicit synthetic file maintenance.".into(),
        operation_ids: vec!["stale".into()],
    };
    let report = f.run(&prepared, &d);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.unwrap().contains("stale file base revision"));
    assert_eq!(
        f.app
            .invocation("stale-update")
            .unwrap()
            .runtime_failure_context,
        Some(json!({"kind":"unknown"}))
    );
    assert!(!f
        .app
        .snapshot()
        .unwrap()
        .observations
        .values()
        .any(
            |o| o.kind == "accepted_file_write" && o.invocation.as_deref() == Some("stale-update")
        ));
    let root = f.app.state_root().to_owned();
    f.app = Runtime::open(&root).unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let state = f.app.snapshot().unwrap();
    let recovery = state
        .observations
        .values()
        .find(|o| o.kind == "terminal_recovery" && o.invocation.as_deref() == Some("stale-update"))
        .unwrap();
    assert!(recovery.exposure.is_none());
    // Current-head validation consulted state whose exact failure lineage is not
    // tracked. The operator keeps the actual cause; inference cannot receive it.
    let recovered = f.prepare("fresh-after-stale");
    assert!(!recovered.prompt.contains("stale file base revision"));
    assert!(!recovered.prompt.contains("DO_NOT_EXPOSE_RAW_FAILED_BODY"));
    let core = interface_control(&recovered);
    assert!(!core["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["kind"] == "terminal_recovery"));
    assert!(!core["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["kind"] == "accepted_file_write" && o["invocation"] == "stale-update"));
    // Explicit resume permits a fresh voluntary decision without disclosing or
    // automatically replaying the failed write.
    assert_eq!(
        f.run(&recovered, &decision(&recovered, true)).status,
        AdoptionStatus::Adopted
    );
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn pure_schema_failure_recovery_retains_prepared_exposure_and_sanitized_cause() {
    // This uses only the existing local transport fixture executable; no model
    // service, external provider request or network access is involved.
    let mut f = live_fixture();
    let raw = r#"{"version":1,"DO_NOT_EXPOSE_QUOTED_PRIVATE_SCHEMA_NAME":true}"#;
    let outer = serde_json::to_string(&json!({"decision_json":raw})).unwrap();
    let (settings, working) = transport_fixture(&f, &outer, false);
    let report = f
        .app
        .step(
            PrepareRequest {
                id: "schema-only-failure".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::NonconformantResearch,
                mode: Mode::Ordinary,
            },
            settings,
            working,
            NOW,
        )
        .await
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.is_some());
    let invocation = f.app.invocation("schema-only-failure").unwrap();
    let admitted: Prepared = serde_json::from_value(invocation.request.unwrap()).unwrap();
    let terminal = f.app.terminal_evidence("schema-only-failure").unwrap();
    let context = invocation.runtime_failure_context.unwrap();
    assert_eq!(context["kind"], "known");
    assert_eq!(
        context["terminal_sha256"],
        terminal.exact_ref().unwrap().sha256
    );
    assert_eq!(
        context["exposure"],
        serde_json::to_value(admitted.exposure.unwrap()).unwrap()
    );
    assert_eq!(terminal.body["response_body"]["text"], raw);
    let root = f.app.state_root().to_owned();
    f.app = Runtime::open(&root).unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let recovered = f.prepare("after-schema-only-failure");
    let core = interface_control(&recovered);
    let recovery = core["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "terminal_recovery")
        .unwrap();
    let error = &recovery["payload"]["disposition"]["adoption_error"];
    assert_eq!(error["status"], "recorded");
    assert_eq!(error["response_body_supplied"], false);
    assert!(!recovered
        .prompt
        .contains("DO_NOT_EXPOSE_QUOTED_PRIVATE_SCHEMA_NAME"));
    assert!(!recovered.prompt.contains(raw));
    assert_eq!(
        f.app.terminal_evidence("schema-only-failure").unwrap(),
        terminal
    );
    assert_eq!(
        f.run(&recovered, &decision(&recovered, true)).status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn denied_current_head_is_not_inferred_from_permitted_history() {
    let mut configuration = config();
    let mut revocable = policy();
    revocable.id = "head-policy".into();
    revocable.revision = "head-policy-1".into();
    configuration
        .policies
        .active_policies
        .push(revocable.clone());
    let mut primary = seed("a");
    primary.grants.write_policies.insert(revocable.id.clone());
    let mut f = Fixture::configured(true, configuration, primary, need());
    f.accept();
    accepted_write(&mut f, "public-old", "historical search needle", None);
    let old = f.app.snapshot().unwrap().sources["note"].head.clone();
    let prepared = f.prepare("new-restricted-head");
    let mut d = decision(&prepared, true);
    let mut save = write(
        "new-head",
        p::Scope::Assets {},
        "note",
        "HEAD_BODY_MUST_STAY_PRIVATE",
        Some(old.clone()),
    );
    if let p::Operation::FileWrite { policy_id, .. } = &mut save {
        *policy_id = revocable.id.clone();
    }
    d.operations.push(save);
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Save under a separately revocable policy.".into(),
        operation_ids: vec!["new-head".into()],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let private_head = f.app.snapshot().unwrap().sources["note"].head.clone();
    f.app
        .revoke_policy("operator", revocable.identity(), "Withdraw the new head.")
        .unwrap();
    let prepared = f.prepare("find-public-history");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Search {
        id: "history".into(),
        scope: p::Scope::Assets {},
        query: "needle".into(),
        regex: false,
        limit: 8,
    });
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let next = f.prepare("metadata-with-denied-head");
    let core = interface_control(&next);
    let note = core["file_mutation_metadata"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["source"] == "note")
        .unwrap();
    assert_eq!(note["source_owner"], "a");
    assert!(note["current_head"].is_null());
    assert_eq!(note["current_head_status"], "unavailable");
    assert!(!next.prompt.contains(&private_head));
    assert!(!next.prompt.contains("HEAD_BODY_MUST_STAY_PRIVATE"));
}

#[test]
fn read_and_selected_context_report_recorded_owner_and_protection() {
    let mut f = Fixture::new(true);
    f.accept();
    let peer = record("peer-source", f::Scope::Assets, "peer protected body");
    f.app
        .import_files("operator", "b", vec![peer.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("read-peer");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Read {
        id: "read".into(),
        reference: reference(&peer),
    });
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let next = f.prepare("peer-read-result");
    let core = interface_control(&next);
    let metadata = core["file_mutation_metadata"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["source"] == "peer-source")
        .unwrap();
    assert_eq!(metadata["source_owner"], "b");
    assert_eq!(metadata["source_protected"], true);
    let mut d = decision(&next, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Use exact peer context.".into(),
        references: vec![reference(&peer)],
    };
    assert_eq!(f.run(&next, &d).status, AdoptionStatus::Adopted);
    let next = f.prepare("peer-selected-context");
    let core = interface_control(&next);
    let metadata = core["file_mutation_metadata"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["source"] == "peer-source")
        .unwrap();
    assert_eq!(metadata["source_owner"], "b");
    assert_eq!(metadata["source_protected"], true);
    assert_eq!(
        core["selection_state"]["assets"][0]["preserved_for_next_admission"],
        true
    );
}

#[test]
fn recovery_error_and_expired_selection_obey_original_exposure_restrictions() {
    let mut configuration = config();
    let mut revocable = policy();
    revocable.id = "transient-policy".into();
    revocable.revision = "transient-policy-1".into();
    configuration
        .policies
        .active_policies
        .push(revocable.clone());
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let mut asset = record(
        "transient-private",
        f::Scope::Assets,
        "TRANSIENT_PRIVATE_BODY",
    );
    asset.policy = revocable.clone();
    asset = asset.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("select-private-once");
    let mut d = decision(&prepared, true);
    let mut exact = reference(&asset);
    exact.lifetime = Some("next_call".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "One request only.".into(),
        references: vec![exact],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let once = f.prepare("private-held-response");
    let mut d = decision(&once, true);
    d.epoch += 1;
    assert_eq!(f.run(&once, &d).status, AdoptionStatus::Held);
    f.app
        .revoke_policy("operator", revocable.identity(), "Revoke before recovery.")
        .unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let next = f.prepare("after-private-recovery");
    let core = interface_control(&next);
    assert!(core["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!next.prompt.contains("transient-private"));
    assert!(!next.prompt.contains("TRANSIENT_PRIVATE_BODY"));
    assert!(!next.prompt.contains("response epoch mismatch"));
    assert!(!core["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["kind"] == "terminal_recovery"));
}

#[test]
fn current_navigation_reports_the_depleted_admission_not_remaining_work_pool() {
    let mut configuration = config();
    configuration.navigation_per_call.calls = 2;
    configuration.navigation_per_work.calls = 3;
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let prepared = f.prepare("last-navigation-allocation");
    let core = interface_control(&prepared);
    assert_eq!(prepared.navigation.calls, 1);
    assert_eq!(core["navigation_current_admission"]["calls"], 1);
    assert_eq!(core["navigation_left_after_this_admission"]["calls"], 0);
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    let next = f.prepare("no-navigation-allocation");
    assert_eq!(next.navigation.calls, 0);
    assert_eq!(
        interface_control(&next)["navigation_current_admission"]["calls"],
        0
    );
}

#[test]
fn failed_response_consumes_next_call_without_hiding_its_authored_expiration() {
    let mut f = Fixture::new(true);
    f.accept();
    let asset = record(
        "expires-after-failure",
        f::Scope::Assets,
        "ONE_ADMISSION_BODY",
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("choose-before-failure");
    let mut d = decision(&prepared, true);
    let mut exact = reference(&asset);
    exact.lifetime = Some("next_call".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Only next request.".into(),
        references: vec![exact.clone()],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let once = f.prepare("failing-admission");
    let mut d = decision(&once, true);
    d.epoch += 1;
    assert_eq!(f.run(&once, &d).status, AdoptionStatus::Held);
    f.app.resume("operator", "a", "work").unwrap();
    let next = f.prepare("after-failing-admission");
    let core = interface_control(&next);
    let selected = &core["selection_state"]["assets"][0];
    assert_eq!(selected["reference"], json!(exact));
    assert_eq!(selected["last_admission"], once.admission_serial);
    assert_eq!(selected["active_this_admission"], false);
    assert_eq!(selected["preserved_for_next_admission"], false);
    assert!(!next.prompt.contains("ONE_ADMISSION_BODY"));
}

#[test]
fn later_rejection_rolls_back_staged_file_receipt_and_source_together() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("save-with-invalid-message");
    let mut d = decision(&prepared, true);
    d.operations = vec![
        write(
            "save",
            p::Scope::Assets {},
            "atomic-note",
            "UNADOPTED_BODY",
            None,
        ),
        p::Operation::Communicate {
            id: "message".into(),
            recipient: "not-granted".into(),
            subject: "not sent".into(),
            text: "not sent".into(),
        },
    ];
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Attempt a save in an invalid response.".into(),
        operation_ids: vec!["save".into()],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Held);
    let state = f.app.snapshot().unwrap();
    assert!(!state.sources.contains_key("atomic-note"));
    assert!(!state
        .observations
        .values()
        .any(|o| o.kind == "accepted_file_write"
            && o.invocation.as_deref() == Some("save-with-invalid-message")));
}

#[test]
fn discover_object_pages_expose_only_first_and_continued_hits_mutation_metadata() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let own = record(
        "a-own",
        f::Scope::Assets,
        "OWN_BODY_NOT_SUPPLIED_BY_DISCOVERY",
    );
    let peer = record(
        "b-peer",
        f::Scope::Assets,
        "PEER_BODY_NOT_SUPPLIED_BY_DISCOVERY",
    );
    fixture
        .app
        .import_files("operator", "a", vec![own.clone()], false, NOW)
        .unwrap();
    fixture
        .app
        .import_files("operator", "b", vec![peer.clone()], true, NOW)
        .unwrap();
    let request = f::DiscoveryRequest {
        scope: f::Scope::Assets,
        surface: f::SearchSurface::Path,
        mode: f::SearchMode::Listing,
        hidden_paths: f::HiddenPaths::Include,
        retirement: f::RetirementFilter::AllRetained,
    };
    let first = fixture.prepare("discover-metadata-first");
    let mut response = decision(&first, true);
    response.operations.push(p::Operation::Discover {
        id: "first-page".into(),
        request: request.clone(),
        limit: 1,
        cursor: None,
    });
    assert_eq!(
        fixture.run(&first, &response).status,
        AdoptionStatus::Adopted
    );
    let second = fixture.prepare("discover-metadata-continued");
    let control = interface_control(&second);
    let page = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["kind"] == "discovery")
        .unwrap();
    assert!(page["payload"]["page"].is_object());
    assert_eq!(
        page["payload"]["page"]["hits"][0]["reference"]["source"],
        "a-own"
    );
    let metadata = control["file_mutation_metadata"].as_array().unwrap();
    let observed = metadata
        .iter()
        .find(|item| item["source"] == "a-own")
        .expect("Discover object-page hit must expose mutation metadata");
    assert_eq!(observed["source_owner"], "a");
    assert_eq!(observed["source_protected"], false);
    assert_eq!(observed["current_head"]["revision"], own.revision);
    assert!(
        !metadata.iter().any(|item| item["source"] == "b-peer"),
        "consulted sources are not page hits"
    );
    assert!(control["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!second.prompt.contains("OWN_BODY_NOT_SUPPLIED_BY_DISCOVERY"));
    assert!(!second
        .prompt
        .contains("PEER_BODY_NOT_SUPPLIED_BY_DISCOVERY"));
    let cursor = page["payload"]["next_cursor"].as_str().unwrap().to_owned();
    let mut response = decision(&second, true);
    response.operations.push(p::Operation::Discover {
        id: "next-page".into(),
        request,
        limit: 1,
        cursor: Some(cursor),
    });
    assert_eq!(
        fixture.run(&second, &response).status,
        AdoptionStatus::Adopted
    );
    let third = fixture.prepare("observe-discover-metadata-continuation");
    let control = interface_control(&third);
    let metadata = control["file_mutation_metadata"].as_array().unwrap();
    let observed = metadata
        .iter()
        .find(|item| item["source"] == "b-peer")
        .expect("continued Discover hit must expose mutation metadata");
    assert_eq!(observed["source_owner"], "b");
    assert_eq!(observed["source_protected"], true);
    assert_eq!(observed["current_head"]["revision"], peer.revision);
    assert!(!metadata.iter().any(|item| item["source"] == "a-own"));
    assert!(!third.prompt.contains("OWN_BODY_NOT_SUPPLIED_BY_DISCOVERY"));
    assert!(!third.prompt.contains("PEER_BODY_NOT_SUPPLIED_BY_DISCOVERY"));
    assert!(control["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn discover_historical_hit_supplies_current_write_base_without_refreshing_body() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    accepted_write(&mut fixture, "history-old", "historical-only needle", None);
    let old = fixture.app.snapshot().unwrap().sources["note"].head.clone();
    accepted_write(
        &mut fixture,
        "history-new",
        "CURRENT_HEAD_BODY_NOT_SUPPLIED",
        Some(old.clone()),
    );
    let prepared = fixture.prepare("discover-historical");
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Discover {
        id: "historical".into(),
        request: f::DiscoveryRequest {
            scope: f::Scope::Assets,
            surface: f::SearchSurface::Text {
                representation: f::Representation::Utf8,
            },
            mode: f::SearchMode::Literal {
                needle: "needle".into(),
            },
            hidden_paths: f::HiddenPaths::Include,
            retirement: f::RetirementFilter::AllRetained,
        },
        limit: 8,
        cursor: None,
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("revise-from-discover-metadata");
    let control = interface_control(&prepared);
    let page = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["kind"] == "discovery")
        .unwrap();
    assert_eq!(
        page["payload"]["page"]["hits"][0]["reference"]["revision"],
        old
    );
    let metadata = control["file_mutation_metadata"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["source"] == "note")
        .expect("historical Discover hit must expose separately permitted current head");
    let current = metadata["current_head"]["revision"].as_str().unwrap();
    assert_ne!(current, old);
    assert_eq!(
        current,
        fixture.app.snapshot().unwrap().sources["note"].head
    );
    assert!(!prepared.prompt.contains("CURRENT_HEAD_BODY_NOT_SUPPLIED"));
    assert!(control["selection_state"]["assets"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut response = decision(&prepared, true);
    response.operations.push(write(
        "revise-current",
        p::Scope::Assets {},
        "note",
        "explicit new content",
        Some(current.into()),
    ));
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Use separately permitted exact current metadata.".into(),
        operation_ids: vec!["revise-current".into()],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn successor_head_metadata_growth_enables_repair_for_unchanged_near_bound_selection() {
    fn publish_head(fixture: &mut Fixture, id: &str, base: String, policy_id: &str) -> String {
        let prepared = fixture
            .app
            .prepare(
                PrepareRequest {
                    id: id.into(),
                    persona: "a".into(),
                    work: "head-work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap();
        let control = interface_control(&prepared);
        let mut response = decision(&prepared, !control["own_acceptance"].is_null());
        if control["own_acceptance"].is_null() {
            let offer = &control["own_pending_offers"][0];
            response.operations.push(p::Operation::Responsibility {
                id: offer["id"].as_str().unwrap().into(),
                work_id: "head-work".into(),
                terms_revision: offer["terms_revision"].as_str().unwrap().into(),
                disposition: "accept".into(),
                reason: "Maintain this separate work's own files.".into(),
            });
        }
        let mut save = write(
            "head-save",
            p::Scope::Assets {},
            "large-history",
            "SUCCESSOR_HEAD_BODY_NOT_SELECTED",
            Some(base),
        );
        if let p::Operation::FileWrite {
            policy_id: selected_policy,
            ..
        } = &mut save
        {
            *selected_policy = policy_id.into();
        }
        response.operations.push(save);
        response.asset_maintenance = p::Maintenance::Change {
            reason: "Publish one explicitly authored successor in another work.".into(),
            operation_ids: vec!["head-save".into()],
        };
        let report = fixture.run(&prepared, &response);
        assert_eq!(
            report.status,
            AdoptionStatus::Adopted,
            "{:?}",
            report.reason
        );
        fixture.app.snapshot().unwrap().sources["large-history"]
            .head
            .clone()
    }

    fn fixture_at_bound(bound: usize) -> (Fixture, f::FileRevision, String) {
        let mut configuration = repair_config(1);
        configuration.context_bytes = bound;
        let mut hidden = policy();
        hidden.id = "hidden-successor-policy".into();
        hidden.revision = "hidden-successor-policy-1".into();
        configuration.policies.active_policies.push(hidden.clone());
        let mut primary = seed("a");
        primary.grants.write_policies.insert(hidden.id.clone());
        let mut fixture = Fixture::configured(true, configuration, primary, need());
        fixture.accept();
        let old = record("large-history", f::Scope::Assets, &"L".repeat(90_000));
        fixture
            .app
            .import_files("operator", "a", vec![old.clone()], false, NOW)
            .unwrap();
        let mut other_work = need();
        other_work.id = "head-work".into();
        other_work.invitations[0].id = "head-offer".into();
        fixture.app.add_need("operator", other_work).unwrap();
        let hidden_head = publish_head(
            &mut fixture,
            "hidden-head",
            old.revision.clone(),
            &hidden.id,
        );
        fixture
            .app
            .revoke_policy(
                "operator",
                hidden.identity(),
                "Withdraw the successor only.",
            )
            .unwrap();
        let prepared = fixture.prepare("select-big");
        let mut response = decision(&prepared, true);
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Retain the old permitted exact body without refreshing its head.".into(),
            references: vec![reference(&old)],
        };
        let report = fixture.run(&prepared, &response);
        assert_eq!(
            report.status,
            AdoptionStatus::Adopted,
            "{:?}",
            report.reason
        );
        (fixture, old, hidden_head)
    }

    // Calibrate from actual rendered bytes, then use a fresh runtime with the
    // boundary fixed at creation. No configuration/state rewrite bypasses admission.
    let (mut calibration, _, _) = fixture_at_bound(config().context_bytes);
    let calibrated = calibration.app.snapshot().unwrap().personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap()
        .ordinary_bytes;
    drop(calibration);
    let bound = calibrated + 32;
    let (mut fixture, old, hidden_head) = fixture_at_bound(bound);
    let accepted = fixture.app.snapshot().unwrap();
    let focus = &accepted.personas["a"].focus["work"];
    let fit = focus.selection_fit.clone().unwrap();
    let original_selection = json!([focus.fragments, focus.assets]);
    assert!(fit.ordinary_bytes <= bound);
    assert!(bound - fit.ordinary_bytes <= 40);

    // Formatting-only control growth cannot create repair authority. This
    // deliberately long fresh invocation id makes the same selection overflow.
    let control_only = fixture.app.prepare(
        PrepareRequest {
            id: format!("control-only-{}", "x".repeat(100)),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    let Err(RuntimeError::SelectionOverflow { growth_bytes, .. }) = control_only else {
        panic!("expected control-only overflow, got {control_only:?}")
    };
    assert_eq!(growth_bytes, fit.growth_bytes);
    assert!(fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .repair_trigger
        .is_none());

    // Only another work's source head changes. The target's exact historical
    // selection, pending observations, need, work version and evidence stay fixed.
    let visible_head = publish_head(&mut fixture, "public-head", hidden_head, "local-policy");
    let before = fixture.app.snapshot().unwrap();
    assert_eq!(
        json!([
            before.personas["a"].focus["work"].fragments,
            before.personas["a"].focus["work"].assets
        ]),
        original_selection
    );
    assert_eq!(
        before.personas["a"].focus["work"].pending_observations,
        focus.pending_observations
    );
    assert_eq!(
        json!(before.work_meta["work"].original),
        json!(accepted.work_meta["work"].original)
    );
    assert_eq!(
        before.work_meta["work"].human_evidence,
        accepted.work_meta["work"].human_evidence
    );
    assert_eq!(
        before
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .version,
        accepted
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .version
    );

    let overflow = fixture.app.prepare(
        PrepareRequest {
            id: "probe-bigx".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    let Err(RuntimeError::SelectionOverflow {
        required_bytes,
        mandatory_bytes,
        growth_bytes,
        selection_sha256,
        ..
    }) = overflow
    else {
        panic!("expected optional-only overflow, got {overflow:?}")
    };
    assert!(required_bytes > bound);
    assert!(mandatory_bytes < bound);
    assert_eq!(selection_sha256, fit.selection_sha256);
    let after = fixture.app.snapshot().unwrap();
    let trigger = after.personas["a"].focus["work"]
        .repair_trigger
        .as_ref()
        .expect(
            "newly permitted successor-head metadata must establish material growth for repair",
        );
    assert!(growth_bytes > fit.growth_bytes);
    assert_eq!(trigger.accepted_growth_bytes, fit.growth_bytes);
    assert_eq!(trigger.selection_sha256, fit.selection_sha256);
    assert_eq!(
        after.work_meta["work"].attempts_left,
        before.work_meta["work"].attempts_left
    );
    assert_eq!(after.work_meta["work"].repair_attempts_left, 1);
    assert!(after.personas["a"].active_invocation.is_none());
    assert_eq!(
        json!([
            after.personas["a"].focus["work"].fragments,
            after.personas["a"].focus["work"].assets
        ]),
        original_selection
    );

    let repair = fixture
        .app
        .prepare(repair_request("repair-head"), NOW)
        .unwrap();
    let control = interface_control(&repair);
    assert_eq!(control["mode"], "selection_repair");
    assert_eq!(control["selection_repair"]["ordinary_fits"], false);
    assert_eq!(
        control["selection_repair"]["recorded_trigger"]["selection_sha256"],
        fit.selection_sha256
    );
    assert_eq!(
        control["selection_state"]["assets"][0]["reference"]["revision"],
        old.revision
    );
    let metadata = control["file_mutation_metadata"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["source"] == "large-history")
        .unwrap();
    assert_eq!(metadata["current_head"]["revision"], visible_head);
    assert!(repair.assembled_bytes_with_reserve <= bound);
    assert!(!repair.prompt.contains(&"L".repeat(1000)));
    assert!(!repair.prompt.contains("SUCCESSOR_HEAD_BODY_NOT_SELECTED"));
    assert_eq!(
        fixture.app.snapshot().unwrap().work_meta["work"].repair_attempts_left,
        0
    );
}

fn store_capacity_private_fixture(private_bytes: usize) -> (Fixture, ArtifactKey) {
    let mut private = policy();
    private.id = "unrelated-private-capacity".into();
    private.revision = "unrelated-private-capacity-1".into();
    private.readers = set(&["b"]);
    let mut configuration = config();
    configuration.policies.active_policies.push(private.clone());
    configuration.response_bytes = 65536;
    configuration.output_reserve_bytes = 65536;
    configuration.context_bytes = 262144;
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let asset = record(
        "public-capacity-artifact",
        f::Scope::Assets,
        "Public artifact.",
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("register-capacity-artifact");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "register",
        WorkAction::PutArtifact {
            artifact: "public-artifact".into(),
            base: None,
            subject: reference(&asset),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("read-capacity-artifact-key");
    let key = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    // Only supported imports vary. All five records belong to B and deny A.
    let records = (0..5)
        .map(|index| {
            let len = private_bytes / 5 + usize::from(index < private_bytes % 5);
            assert!(len > 0 && len <= 262144);
            let mut record = record(
                &format!("unrelated-private-capacity-{index}"),
                f::Scope::Assets,
                &"P".repeat(len),
            );
            record.policy = private.clone();
            record.seal().unwrap()
        })
        .collect();
    f.app
        .import_files("operator", "b", records, false, NOW)
        .unwrap();
    (f, key)
}

#[test]
fn store_commit_failure_from_unrelated_private_capacity_has_unknown_recovery() {
    let (probe, _) = store_capacity_private_fixture(1_000_000);
    let original = closeout_head(probe.app.state_root());
    let body_bytes = serde_json::to_vec(&original).unwrap().len();
    let participants = original.body["board"]["works"]["work"]["participants"]
        .as_object()
        .unwrap()
        .len();
    let personas = original.body["personas"].as_object().unwrap().len();
    // Calibrate under the existing fixed format and prepaid control growth,
    // using read-only operator inspection. Never alter a Store cell or limit.
    let control_growth = ((participants + 4) * (6 * 4096 + 4096) + personas * 2 + 128) * 2;
    let guarded_object_limit =
        storage_limits().max_record_bytes as usize - 32768 - control_growth - 128;
    let desired_high_bytes = guarded_object_limit - 40000;
    assert!(desired_high_bytes > body_bytes);
    let high_private_bytes = 1_000_000 + (desired_high_bytes - body_bytes) / 3;
    let low_private_bytes = high_private_bytes - 20000;
    drop(probe);
    let mut prepared_views = Vec::new();
    for (higher, private_bytes) in [(false, low_private_bytes), (true, high_private_bytes)] {
        let (mut f, key) = store_capacity_private_fixture(private_bytes);
        let head = closeout_head(f.app.state_root());
        let retained_bytes = serde_json::to_vec(&head).unwrap().len();
        let prepared = f.prepare("capacity-sensitive-judgment");
        assert!(!prepared.prompt.contains("unrelated-private-capacity"));
        assert!(!serde_json::to_string(&prepared.exposure)
            .unwrap()
            .contains("unrelated-private-capacity"));
        prepared_views.push((prepared.prompt.clone(), prepared.exposure.clone()));
        if higher {
            assert_eq!(
                prepared_views[0], prepared_views[1],
                "actor's public request and exposure must be identical in the paired states"
            );
        }
        let before = serde_json::to_value(f.app.snapshot().unwrap()).unwrap();
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "judge-capacity",
            WorkAction::Judge {
                id: "capacity-judgment".into(),
                artifacts: BTreeMap::from([("result".into(), key)]),
                sufficient: false,
                claims: BTreeMap::from([(
                    "result".into(),
                    ClaimInput {
                        met: false,
                        explanation: "No sufficiency is claimed.".into(),
                        evidence: BTreeSet::new(),
                    },
                )]),
                explanation: "J".repeat(60000),
            },
            &[],
        ));
        let report = f.run(&prepared, &response);
        eprintln!("STORE-CAPACITY higher={higher} private_bytes={private_bytes} retained_bytes={retained_bytes} guarded_limit={guarded_object_limit} report={:?} reason={:?}",report.status,report.reason);
        if !higher {
            assert_eq!(
                report.status,
                AdoptionStatus::Adopted,
                "{:?}",
                report.reason
            );
            let after = f.app.snapshot().unwrap();
            assert_eq!(
                after
                    .board
                    .work(&w::WorkId("work".into()))
                    .unwrap()
                    .judgments
                    .len(),
                1
            );
            assert_eq!(
                serde_json::to_value(&after.sources).unwrap(),
                before["sources"]
            );
            continue;
        }
        assert_eq!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
        let expected = s::StoreError::TooLarge.to_string();
        assert_eq!(report.reason.as_deref(), Some(expected.as_str()));
        assert_eq!(
            serde_json::to_value(f.app.snapshot().unwrap()).unwrap(),
            before,
            "whole rejected candidate must roll back"
        );
        let invocation = f.app.invocation(&prepared.request.id).unwrap();
        assert!(invocation.history.iter().any(|receipt| receipt.status
            == s::ReceiptStatus::Failed
            && receipt
                .detail
                .as_deref()
                .is_some_and(|detail| detail.ends_with(&expected))));
        eprintln!(
            "STORE-CAPACITY failure_context={}",
            serde_json::to_string(&invocation.runtime_failure_context).unwrap()
        );
        let root = f.app.state_root().to_owned();
        f.app = Runtime::open(&root).unwrap();
        f.app.resume("operator", "a", "work").unwrap();
        let recovered = f.prepare("after-capacity-failure");
        let core = interface_control(&recovered);
        assert!(
            !core["actual_observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["kind"] == "terminal_recovery"),
            "whole-state capacity outcome may depend on denied retained records"
        );
        assert_eq!(
            invocation.runtime_failure_context,
            Some(json!({"kind":"unknown"}))
        );
        assert!(!recovered.prompt.contains(&expected));
        assert!(!recovered.prompt.contains("unrelated-private-capacity"));
        assert!(!recovered.prompt.contains(&"J".repeat(60000)));
        assert_eq!(
            f.run(&recovered, &decision(&recovered, true)).status,
            AdoptionStatus::Adopted
        );
    }
}
