use super::*;

fn inspect(collection: p::WorkCollection, limit: usize, cursor: Option<String>) -> p::Operation {
    p::Operation::InspectWork {
        id: "inspect".into(),
        collection,
        limit,
        cursor,
    }
}
fn inspected(prepared: &Prepared) -> Value {
    interface_control(prepared)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["kind"] == "work_inspection")
        .expect("fresh request must supply requested work inspection")["payload"]
        .clone()
}
fn inspect_run(
    f: &mut Fixture,
    prepared: &Prepared,
    collection: p::WorkCollection,
    limit: usize,
    cursor: Option<String>,
) {
    let mut response = decision(prepared, true);
    response.operations.push(inspect(collection, limit, cursor));
    let report = f.run(prepared, &response);
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
}

#[test]
fn inspect_work_discovers_old_release_then_delivery_then_close_without_remembered_ids() {
    let (mut f, prepared, key) = model_evidence_fixture("full", false);
    let mut response = decision(&prepared, true);
    response.operations.push(model_evidence_operation(&key));
    response.operations.extend(model_judgment_and_shared(&key));
    // Deliberately no receipt enhancement or retrospective observation backfill.
    let historical_id = format!(
        "retained-release-{}",
        s::sha256_bytes(prepared.request.id.as_bytes())
    );
    response.operations.push(effect(
        "create-historical-release",
        WorkAction::Release {
            id: historical_id,
            shared: "model-note-shared".into(),
        },
        &[],
    ));
    response.next_assets = p::SelectionIntent::Clear {
        reason: "Do not carry artifact bodies.".into(),
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    // Consume any previously supplied creation observations. This also models
    // historical work after newer optional receipt enhancements: the new
    // inspecting invocation must discover the ID without any pending receipt.
    let drained = f.prepare("consume-prior-release-observations");
    assert_eq!(
        f.run(&drained, &decision(&drained, true)).status,
        AdoptionStatus::Adopted
    );
    let root = f.app.state_root().to_owned();
    f.app = Runtime::open(&root).unwrap();
    let prepared = f.prepare("find-unknown-release");
    assert!(!prepared.prompt.contains("retained-release-"));
    assert!(!prepared.prompt.contains(MODEL_NOTE));
    let before = f.app.snapshot().unwrap();
    inspect_run(&mut f, &prepared, p::WorkCollection::Releases, 8, None);
    let prepared = f.prepare("use-discovered-release");
    let page = inspected(&prepared);
    assert_eq!(page["coverage_complete"], true);
    assert_eq!(page["artifact_bodies_supplied"], false);
    assert_eq!(page["semantic_truth_verified"], false);
    let release = page["records"][0]["record_id"].as_str().unwrap().to_owned();
    assert_eq!(page["records"][0]["record"]["by"]["persona"], "a");
    assert!(!prepared.prompt.contains(MODEL_NOTE));
    let after = f.app.snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(&before.personas["a"].focus["work"].assets).unwrap(),
        serde_json::to_value(&after.personas["a"].focus["work"].assets).unwrap()
    );
    assert!(after.local_deliveries.is_empty());
    assert!(after
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .events
        .is_empty());
    assert!(!after.board.completion_supported(&w::WorkId("work".into())));
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "deliver-discovered",
        WorkAction::DeliverRelease {
            release,
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("inspect-deposit-receipt");
    inspect_run(&mut f, &prepared, p::WorkCollection::Deliveries, 8, None);
    let prepared = f.prepare("close-after-inspected-receipt");
    let page = inspected(&prepared);
    assert_eq!(page["records"].as_array().unwrap().len(), 1);
    assert_eq!(page["recipient_attention_established"], false);
    assert_eq!(page["stakeholder_acceptance_established"], false);
    let receipt = &page["records"][0]["record"];
    assert_eq!(receipt["recipient"], "human");
    assert!(receipt["receipt"]["id"]
        .as_str()
        .unwrap()
        .starts_with("delivery:"));
    let release = receipt["release"].as_str().unwrap().to_owned();
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "close-discovered",
        WorkAction::CloseGoal { release },
        &[],
    ));
    response.continuation = p::Continuation::Stop {
        reason: "Close after actual local receipt.".into(),
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    assert!(f
        .app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&w::WorkId("work".into())));
}

#[test]
fn inspect_work_cursor_survives_reopen_clock_and_ordinary_continuation_changes() {
    let (mut f, prepared, key) = model_evidence_fixture("full", false);
    let mut response = decision(&prepared, true);
    response.operations.push(model_evidence_operation(&key));
    response.operations.extend(model_judgment_and_shared(&key));
    for id in ["first-release", "second-release", "third-release"] {
        response.operations.push(effect(
            id,
            WorkAction::Release {
                id: id.into(),
                shared: "model-note-shared".into(),
            },
            &[],
        ));
    }
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("page-one-request");
    inspect_run(&mut f, &prepared, p::WorkCollection::Releases, 1, None);
    let prepared = f.prepare("page-one-result");
    let page = inspected(&prepared);
    assert_eq!(page["coverage_complete"], false);
    assert_eq!(page["records"][0]["record_id"], "first-release");
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    // An ordinary empty decision changes Work.version, not the collection.
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    let root = f.app.state_root().to_owned();
    f.app = Runtime::open(&root).unwrap();
    let prepared = f
        .app
        .prepare(
            PrepareRequest {
                id: "page-two-request".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW + 50,
        )
        .unwrap();
    let mut response = decision(&prepared, true);
    response
        .operations
        .push(inspect(p::WorkCollection::Releases, 1, Some(cursor)));
    f.app
        .dispatch_synthetic(&prepared.request.id, NOW + 50)
        .unwrap();
    assert_eq!(
        f.app
            .adopt_response(
                &prepared.request.id,
                &serde_json::to_vec(&response).unwrap(),
                NOW + 50
            )
            .unwrap()
            .status,
        AdoptionStatus::Adopted
    );
    let prepared = f
        .app
        .prepare(
            PrepareRequest {
                id: "page-two-result".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW + 60,
        )
        .unwrap();
    let page = inspected(&prepared);
    assert_eq!(page["records"][0]["record_id"], "second-release");
    assert_eq!(page["coverage_complete"], false);
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "mutate-release-collection",
        WorkAction::Release {
            id: "fourth-release".into(),
            shared: "model-note-shared".into(),
        },
        &[],
    ));
    response
        .operations
        .push(inspect(p::WorkCollection::Releases, 1, Some(cursor)));
    f.app
        .dispatch_synthetic(&prepared.request.id, NOW + 60)
        .unwrap();
    assert_eq!(
        f.app
            .adopt_response(
                &prepared.request.id,
                &serde_json::to_vec(&response).unwrap(),
                NOW + 60
            )
            .unwrap()
            .status,
        AdoptionStatus::Adopted
    );
    let prepared = f
        .app
        .prepare(
            PrepareRequest {
                id: "stale-result".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW + 70,
        )
        .unwrap();
    let page = inspected(&prepared);
    assert_eq!(page["status"], "stale_cursor");
    assert_eq!(page["restart_required"], true);
    assert!(page["records"].as_array().unwrap().is_empty());
}

#[test]
fn inspect_work_reports_peer_commitment_and_formal_judgment_without_accepting_or_reviewing() {
    let (mut f, prepared, key) = model_evidence_fixture_with_peer("full", false, true);
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    initialize_interface_peer(&mut f);
    let prepared = f
        .app
        .prepare(
            PrepareRequest {
                id: "peer-accept-and-judge".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let mut response = decision(&prepared, true);
    let work = f.app.snapshot().unwrap();
    response.operations.push(p::Operation::Responsibility {
        id: "offer-b".into(),
        work_id: "work".into(),
        terms_revision: offer_revision(
            work.board.work(&w::WorkId("work".into())).unwrap(),
            &w::RecordId("offer-b".into()),
        )
        .unwrap(),
        disposition: "accept".into(),
        reason: "Offer exact feedback without taking ownership.".into(),
    });
    response.operations.push(effect(
        "peer-judgment",
        WorkAction::Judge {
            id: "peer-negative-judgment".into(),
            artifacts: BTreeMap::from([("result".into(), key)]),
            sufficient: false,
            claims: BTreeMap::from([(
                "result".into(),
                ClaimInput {
                    met: false,
                    explanation: "The peer requests more detail.".into(),
                    evidence: BTreeSet::new(),
                },
            )]),
            explanation: "Attributed peer feedback, not independent assurance.".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("inspect-peer-commitments");
    inspect_run(
        &mut f,
        &prepared,
        p::WorkCollection::CurrentCommitments,
        8,
        None,
    );
    let prepared = f.prepare("inspect-peer-judgments");
    let page = inspected(&prepared);
    let peer = page["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["record"]["persona"] == "b")
        .unwrap();
    assert_eq!(
        peer["record"]["acceptance"]["by"]["invocation"],
        "peer-accept-and-judge"
    );
    assert!(peer["record"]["acceptance"]["terms"]["outcomes"]
        .as_array()
        .unwrap()
        .is_empty());
    inspect_run(&mut f, &prepared, p::WorkCollection::Judgments, 8, None);
    let prepared = f.prepare("receive-peer-judgment");
    let page = inspected(&prepared);
    assert_eq!(page["records"][0]["record"]["by"]["persona"], "b");
    assert_eq!(page["records"][0]["record"]["input"]["sufficient"], false);
    assert!(prepared.prompt.contains("The peer requests more detail."));
    assert_eq!(page["artifact_bodies_supplied"], false);
}

#[test]
fn inspect_work_protocol_is_closed_and_requires_finite_explicit_limit() {
    let mut f = Fixture::new(true);
    let prepared = f.prepare("protocol-shape");
    for operation in [
        json!({"kind":"inspect_work","id":"inspect","collection":"interpretations","limit":1}),
        json!({"kind":"inspect_work","id":"inspect","collection":"releases","limit":0}),
        json!({"kind":"inspect_work","id":"inspect","collection":"releases","limit":129}),
        json!({"kind":"inspect_work","id":"inspect","collection":"releases"}),
        json!({"kind":"inspect_work","id":"inspect","collection":"releases","limit":1,"work_id":"other"}),
    ] {
        let mut response = serde_json::to_value(decision(&prepared, true)).unwrap();
        response["operations"] = json!([operation]);
        assert!(p::parse_decision(&serde_json::to_vec(&response).unwrap(), 32768).is_err());
    }
}

fn judgment_records_fixture(configuration: Config, long: bool) -> Fixture {
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let asset = record(
        "inspection-asset",
        f::Scope::Assets,
        "ASSET BODY NOT INSPECTION CONTENT",
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("register-inspection-artifact");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "register-inspection",
        WorkAction::PutArtifact {
            artifact: "inspection-artifact".into(),
            base: None,
            subject: reference(&asset),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("make-inspection-judgments");
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    let mut response = decision(&prepared, true);
    for (id, explanation) in [
        (
            "large-first",
            if long {
                "L".repeat(14_000)
            } else {
                "First attributed feedback".into()
            },
        ),
        ("small-second", "Second attributed feedback".into()),
    ] {
        response.operations.push(effect(
            id,
            WorkAction::Judge {
                id: id.into(),
                artifacts: BTreeMap::from([("result".into(), key.clone())]),
                sufficient: false,
                claims: BTreeMap::new(),
                explanation,
            },
            &[],
        ));
    }
    let report = f.run(&prepared, &response);
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    f
}

#[test]
fn inspect_work_oversized_records_are_explicit_and_do_not_hide_later_records() {
    for bound in ["result", "scan"] {
        let mut configuration = config();
        if bound == "result" {
            configuration.navigation_per_call.result_bytes = 2500;
        } else {
            configuration.navigation_per_call.scanned_bytes = 32 * 1024 * 1024 + 4000;
        }
        let mut f = judgment_records_fixture(configuration, true);
        let prepared = f.prepare("bounded-page-request");
        inspect_run(&mut f, &prepared, p::WorkCollection::Judgments, 1, None);
        let prepared = f.prepare("bounded-page-result");
        let page = inspected(&prepared);
        assert_eq!(page["records"][0]["record_id"], "large-first");
        assert_eq!(
            page["records"][0]["status"],
            if bound == "result" {
                "record_exceeds_result_bound"
            } else {
                "record_exceeds_scan_bound"
            }
        );
        assert!(
            page["records"][0].get("record").is_none() || page["records"][0]["record"].is_null()
        );
        assert!(page["records"][0]["retry_cursor"].is_string());
        assert_eq!(page["coverage_complete"], false);
        let cursor = page["next_cursor"].as_str().unwrap().to_owned();
        inspect_run(
            &mut f,
            &prepared,
            p::WorkCollection::Judgments,
            1,
            Some(cursor),
        );
        let prepared = f.prepare("later-record-result");
        let page = inspected(&prepared);
        assert_eq!(page["records"][0]["record_id"], "small-second");
        assert_eq!(page["records"][0]["status"], "record");
        assert_eq!(
            page["records"][0]["record"]["input"]["explanation"],
            "Second attributed feedback"
        );
        assert_eq!(page["end_reached"], true);
        assert!(!prepared
            .prompt
            .contains("ASSET BODY NOT INSPECTION CONTENT"));
    }
}

#[test]
fn inspect_work_unrelated_policy_preserves_cursor_and_source_withdrawal_blocks_supply() {
    let mut configuration = config();
    let spare_policy = restricted_policy();
    configuration
        .policies
        .active_policies
        .push(spare_policy.clone());
    let mut f = judgment_records_fixture(configuration, false);
    let prepared = f.prepare("permission-page-request");
    inspect_run(&mut f, &prepared, p::WorkCollection::Judgments, 1, None);
    let prepared = f.prepare("permission-page-result");
    let cursor = inspected(&prepared)["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    f.app
        .revoke_policy(
            "operator",
            spare_policy.identity(),
            "Explicit synthetic permission change",
        )
        .unwrap();
    let prepared = f.prepare("changed-permission-request");
    inspect_run(
        &mut f,
        &prepared,
        p::WorkCollection::Judgments,
        1,
        Some(cursor),
    );
    let prepared = f.prepare("changed-permission-result");
    assert_eq!(inspected(&prepared)["status"], "page");
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    let prepared = f.prepare("fresh-before-withdrawal");
    inspect_run(&mut f, &prepared, p::WorkCollection::Judgments, 1, None);
    f.app
        .withdraw_source(
            "operator",
            "inspection-asset",
            "Explicit synthetic withdrawal",
        )
        .unwrap();
    assert!(f
        .app
        .prepare(
            PrepareRequest {
                id: "withheld-after-withdrawal".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary
            },
            NOW
        )
        .is_err());
}

#[test]
fn inspect_work_budget_failures_never_claim_complete_coverage() {
    for boundary in ["calls", "scan", "inspections", "matches", "result"] {
        let mut configuration = config();
        match boundary {
            "calls" => configuration.navigation_per_call.calls = 0,
            "scan" => configuration.navigation_per_call.scanned_bytes = 0,
            "inspections" => configuration.navigation_per_call.inspected_revisions = 0,
            "matches" => configuration.navigation_per_call.matches = 0,
            "result" => configuration.navigation_per_call.result_bytes = 0,
            _ => unreachable!(),
        }
        let mut f = Fixture::with_config(true, configuration);
        f.accept();
        let prepared = f.prepare("budget-request");
        let mut response = decision(&prepared, true);
        response
            .operations
            .push(inspect(p::WorkCollection::CurrentCommitments, 1, None));
        let report = f.run(&prepared, &response);
        if boundary == "result" {
            assert_eq!(report.status, AdoptionStatus::Held);
            assert!(report
                .reason
                .unwrap()
                .contains("remaining navigation or context bound"));
        } else {
            assert_eq!(
                report.status,
                AdoptionStatus::Adopted,
                "{:?}",
                report.reason
            );
            let prepared = f.prepare("budget-result");
            let page = inspected(&prepared);
            assert_eq!(page["coverage_complete"], false);
            assert!(page["records"].as_array().unwrap().is_empty());
            if boundary != "matches" {
                assert_eq!(page["status"], "budget_exhausted");
            }
        }
    }
}

#[test]
fn inspect_work_returns_exact_formal_assessment_snapshot() {
    let mut original = need();
    original.review_roles.insert(
        "review".into(),
        RoleSpec {
            terms: "Assess this exact synthetic result.".into(),
            funding: "Fixture allowance".into(),
            policy: "Attributed self assessment is allowed".into(),
            independence: w::Independence::NotRequired,
        },
    );
    original
        .outcomes
        .get_mut("result")
        .unwrap()
        .required_reviews
        .insert("review".into());
    original.invitations[0]
        .terms
        .review_roles
        .insert(w::RoleId("review".into()));
    let mut f = Fixture::configured(true, config(), seed("a"), original);
    f.accept();
    let asset = record("assessment-asset", f::Scope::Assets, "ASSESSMENT BODY");
    f.app
        .import_files("operator", "a", vec![asset.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("register-assessment-artifact");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "register-assessment",
        WorkAction::PutArtifact {
            artifact: "assessment-artifact".into(),
            base: None,
            subject: reference(&asset),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("obtain-assessment-key");
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    f.app
        .record_human_evidence(
            "operator",
            "work",
            "testimony",
            "Synthetic human testimony",
            vec![key.clone()],
            set(&["result"]),
        )
        .unwrap();
    let prepared = f.prepare("formal-assessment");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "assess",
        WorkAction::Assess {
            id: "formal-assessment-record".into(),
            artifact: key,
            outcome: "result".into(),
            role: "review".into(),
            evidence: set(&["testimony"]),
            passes: false,
            explanation: "The attributed review has a caveat.".into(),
            limitations: vec!["This fixture is not independent verification.".into()],
        },
        &[],
    ));
    let report = f.run(&prepared, &response);
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    let prepared = f.prepare("inspect-formal-assessment");
    inspect_run(&mut f, &prepared, p::WorkCollection::Assessments, 1, None);
    let prepared = f.prepare("formal-assessment-result");
    let page = inspected(&prepared);
    let record = &page["records"][0]["record"];
    assert_eq!(record["input"]["id"], "formal-assessment-record");
    assert_eq!(record["by"]["invocation"], "formal-assessment");
    assert_eq!(record["input"]["passes"], false);
    assert_eq!(record["snapshot"][0]["id"], "testimony");
    assert_eq!(page["semantic_truth_verified"], false);
    assert!(!prepared.prompt.contains("ASSESSMENT BODY"));
}

#[test]
fn inspect_work_metadata_does_not_authorize_model_evidence_without_artifact_body() {
    let mut f = judgment_records_fixture(config(), false);
    let prepared = f.prepare("metadata-only-inspection");
    inspect_run(&mut f, &prepared, p::WorkCollection::Judgments, 1, None);
    let prepared = f.prepare("metadata-is-not-body");
    assert_eq!(inspected(&prepared)["records"][0]["status"], "record");
    assert!(!prepared
        .prompt
        .contains("ASSET BODY NOT INSPECTION CONTENT"));
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    let mut response = decision(&prepared, true);
    response.operations.push(model_evidence_operation(&key));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
    assert!(f.app.snapshot().unwrap().work_meta["work"]
        .model_evidence
        .is_empty());
}

fn inspection_prepare_for(f: &mut Fixture, actor: &str, id: &str) -> Prepared {
    f.app
        .prepare(
            PrepareRequest {
                id: id.into(),
                persona: actor.into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap()
}
fn inspection_adopt(f: &mut Fixture, prepared: &Prepared, response: &p::Decision) {
    let report = f.run(prepared, response);
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
}
fn off_page_delivery_fixture() -> (Fixture, f::Policy) {
    off_page_delivery_fixture_with_budget(8, 32768)
}

fn off_page_delivery_fixture_with_budget(
    calls: usize,
    result_bytes: usize,
) -> (Fixture, f::Policy) {
    // Every record is created through supported Runtime operations. B's second
    // delivery has a private originating request; A can inspect it, C cannot.
    let mut public = policy();
    public.readers.insert("c".into());
    public.audience.insert("c".into());
    let mut private = public.clone();
    private.id = "off-page-private".into();
    private.revision = "off-page-private-1".into();
    private.readers = set(&["a", "b"]);
    let mut configuration = Config::local(f::PolicySnapshot {
        active_policies: vec![public.clone(), private.clone()],
        revoked_policies: BTreeSet::new(),
    });
    configuration.local_recipients = set(&["human"]);
    configuration.navigation_per_call.calls = calls;
    configuration.navigation_per_call.result_bytes = result_bytes;
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut app = Runtime::create(&temp.path().join("state"), configuration, "operator").unwrap();
    for actor in ["a", "b", "c"] {
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
    let mut original = need();
    let mut terms = original.invitations[0].terms.clone();
    terms.outcomes.clear();
    terms.exact_terms = "Optional public contribution.".into();
    original.invitations.push(Invitation {
        id: "offer-b".into(),
        to: "b".into(),
        terms,
    });
    app.add_need("operator", original).unwrap();
    let mut f = Fixture { _temp: temp, app };
    f.accept();
    let prepared = inspection_prepare_for(&mut f, "b", "accept-producer");
    let state = f.app.snapshot().unwrap();
    let work = state.board.work(&w::WorkId("work".into())).unwrap();
    let mut response = decision(&prepared, false);
    response.operations.push(p::Operation::Responsibility {
        id: "offer-b".into(),
        work_id: "work".into(),
        terms_revision: offer_revision(work, &w::RecordId("offer-b".into())).unwrap(),
        disposition: "accept".into(),
        reason: "Accept public contribution.".into(),
    });
    inspection_adopt(&mut f, &prepared, &response);
    let mut asset = record(
        "public-delivery-artifact",
        f::Scope::Assets,
        "PUBLIC DELIVERY BODY",
    );
    asset.policy = public;
    let asset = asset.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![asset.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("register-public-delivery-artifact");
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
    inspection_adopt(&mut f, &prepared, &response);
    let prepared = f.prepare("read-public-artifact-identity");
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    inspection_adopt(&mut f, &prepared, &decision(&prepared, true));
    f.app
        .record_human_evidence(
            "operator",
            "work",
            "model-note-evidence",
            "Synthetic exact-criterion testimony",
            vec![key.clone()],
            set(&["result"]),
        )
        .unwrap();
    let prepared = f.prepare("publish-public-release");
    let mut response = decision(&prepared, true);
    response.operations.extend(model_judgment_and_shared(&key));
    response.operations.push(effect(
        "release",
        WorkAction::Release {
            id: "public-release".into(),
            shared: "model-note-shared".into(),
        },
        &[],
    ));
    inspection_adopt(&mut f, &prepared, &response);
    let prepared = f.prepare("public-first-delivery");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "deliver-public",
        WorkAction::DeliverRelease {
            release: "public-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    inspection_adopt(&mut f, &prepared, &response);
    let prepared = f.prepare("consume-public-delivery-receipt");
    inspection_adopt(&mut f, &prepared, &decision(&prepared, true));

    let mut secret = record(
        "private-delivery-input",
        f::Scope::Assets,
        "PRIVATE DELIVERY INPUT BODY",
    );
    secret.policy = private.clone();
    let secret = secret.seal().unwrap();
    f.app
        .import_files("operator", "b", vec![secret.clone()], false, NOW)
        .unwrap();
    let prepared = inspection_prepare_for(&mut f, "b", "read-private-delivery-input");
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Read {
        id: "read-private".into(),
        reference: reference(&secret),
    });
    inspection_adopt(&mut f, &prepared, &response);
    let prepared = inspection_prepare_for(&mut f, "b", "private-second-delivery");
    assert!(prepared.prompt.contains("PRIVATE DELIVERY INPUT BODY"));
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "deliver-private",
        WorkAction::DeliverRelease {
            release: "public-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    inspection_adopt(&mut f, &prepared, &response);
    let state = f.app.snapshot().unwrap();
    assert_eq!(
        state
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .deliveries
            .len(),
        2
    );
    assert!(!serde_json::to_string(&state.work_meta["work"].exposure)
        .unwrap()
        .contains("private-delivery-input"));
    (f, private)
}

#[test]
fn inspect_work_off_page_exposure_blocks_forwarding_pagination_to_excluded_peer() {
    let (mut f, _) = off_page_delivery_fixture();
    let prepared = f.prepare("inspect-public-first-page");
    assert!(!serde_json::to_string(&prepared.exposure)
        .unwrap()
        .contains("private-delivery-input"));
    inspect_run(&mut f, &prepared, p::WorkCollection::Deliveries, 1, None);
    let prepared = f.prepare("forward-inspected-pagination");
    let page = inspected(&prepared);
    assert_eq!(page["records"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_string());
    assert!(!prepared.prompt.contains("PRIVATE DELIVERY INPUT BODY"));
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Communicate {
        id: "forward-pagination".into(),
        recipient: "c".into(),
        subject: "Inspection metadata".into(),
        text: serde_json::to_string(&page).unwrap(),
    });
    let report = f.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
    assert!(report
        .reason
        .unwrap()
        .contains("restricted supplied material"));
    assert!(f.app.snapshot().unwrap().messages.is_empty());
}

#[test]
fn inspect_work_off_page_revocation_withholds_page_and_budget_metadata() {
    let (mut f, private) = off_page_delivery_fixture();
    let prepared = f.prepare("inspect-before-off-page-revocation");
    inspect_run(&mut f, &prepared, p::WorkCollection::Deliveries, 1, None);
    f.app
        .revoke_policy(
            "operator",
            private.identity(),
            "Synthetic off-page revocation",
        )
        .unwrap();
    let prepared = f.prepare("after-off-page-revocation");
    let control = interface_control(&prepared);
    let actual = control["actual_observations"].as_array().unwrap();
    assert!(
        !actual.iter().any(|o| o["kind"] == "work_inspection"),
        "pagination derived from revoked receipt must be withheld"
    );
    assert!(
        !actual.iter().any(|o| o["kind"] == "host_accounting"),
        "view-dependent accounting must be withheld"
    );
    assert!(!prepared.prompt.contains("PRIVATE DELIVERY INPUT BODY"));
}

#[test]
fn inspect_work_metadata_exhaustion_after_reservation_is_adopted_status() {
    let mut configuration = config();
    configuration.navigation_per_call.scanned_bytes = 32 * 1024 * 1024 + 1;
    configuration.navigation_per_call.inspected_revisions = 8193;
    configuration.navigation_per_call.calls = 1;
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let prepared = f.prepare("inspect-one-metadata-byte");
    let mut response = decision(&prepared, true);
    response
        .operations
        .push(inspect(p::WorkCollection::CurrentCommitments, 1, None));
    response.operations.push(p::Operation::Communicate {
        id: "valid-companion".into(),
        recipient: "b".into(),
        subject: "Synthetic companion".into(),
        text: "A bounded unavailable inspection is not an adoption failure.".into(),
    });
    inspection_adopt(&mut f, &prepared, &response);
    assert_eq!(f.app.snapshot().unwrap().messages.len(), 1);
    let fresh = f.prepare("metadata-exhaustion-result");
    let page = inspected(&fresh);
    assert_eq!(page["status"], "budget_exhausted");
    assert_eq!(page["coverage_complete"], false);
    assert_eq!(page["end_reached"], false);
    assert!(page["snapshot"].is_null());
    assert!(page["next_cursor"].is_null());
    assert!(page["records"].as_array().unwrap().is_empty());
    // Exact usage is operator-retained; inference receives a sanitized account.
    let state = f.app.snapshot().unwrap();
    let accounting = state
        .observations
        .values()
        .find(|o| {
            o.kind == "host_accounting"
                && o.invocation.as_deref() == Some("inspect-one-metadata-byte")
        })
        .unwrap();
    assert_eq!(
        accounting.payload["actual_navigation"]["scanned_bytes"],
        32 * 1024 * 1024 + 1
    );
    assert_eq!(accounting.payload["actual_navigation"]["calls"], 1);
}

fn inspection_selected_context_fixture(context_bytes: usize) -> (Fixture, Prepared) {
    let mut configuration = config();
    configuration.context_bytes = context_bytes;
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let asset = record(
        "removable-inspection-context",
        f::Scope::Assets,
        &"S".repeat(24_000),
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("select-removable-inspection-context");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select exact removable fixture context.".into(),
        references: vec![reference(&asset)],
    };
    inspection_adopt(&mut f, &prepared, &response);
    let prepared = f.prepare("clear-context-and-inspect");
    (f, prepared)
}

#[test]
fn inspect_work_selection_clear_reclaims_consumed_context_headroom() {
    let (_, probe) = inspection_selected_context_fixture(config().context_bytes);
    let context = probe.assembled_bytes_with_reserve + 1024;
    let (mut f, prepared) = inspection_selected_context_fixture(context);
    assert!(context.saturating_sub(prepared.assembled_bytes_with_reserve) < 4096);
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Clear {
        reason: "Free the optional body before inspection.".into(),
    };
    response
        .operations
        .push(inspect(p::WorkCollection::CurrentCommitments, 1, None));
    inspection_adopt(&mut f, &prepared, &response);
    let fresh = f.prepare("cleared-context-inspection-result");
    assert_eq!(inspected(&fresh)["status"], "page");
    assert_eq!(inspected(&fresh)["records"].as_array().unwrap().len(), 1);
    assert!(!fresh.prompt.contains(&"S".repeat(128)));
    assert!(fresh.assembled_bytes_with_reserve <= context);
}

fn inspection_large_page_fixture(context_bytes: usize) -> (Fixture, Prepared) {
    let mut configuration = config();
    configuration.context_bytes = context_bytes;
    let mut f = judgment_records_fixture(configuration, true);
    let prepared = f.prepare("obtain-large-first-page");
    inspect_run(&mut f, &prepared, p::WorkCollection::Judgments, 1, None);
    let prepared = f.prepare("replace-large-first-page");
    assert_eq!(inspected(&prepared)["records"][0]["status"], "record");
    (f, prepared)
}

#[test]
fn inspect_work_continuation_replaces_near_full_consumed_page() {
    let (_, probe) = inspection_large_page_fixture(config().context_bytes);
    let context = probe.assembled_bytes_with_reserve + 4000;
    let (mut f, prepared) = inspection_large_page_fixture(context);
    assert!(context.saturating_sub(prepared.assembled_bytes_with_reserve) < 4096);
    let cursor = inspected(&prepared)["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    inspect_run(
        &mut f,
        &prepared,
        p::WorkCollection::Judgments,
        1,
        Some(cursor),
    );
    let fresh = f.prepare("replacement-page-result");
    let page = inspected(&fresh);
    assert_eq!(page["records"][0]["record_id"], "small-second");
    assert_eq!(page["end_reached"], true);
    assert!(!fresh.prompt.contains(&"L".repeat(128)));
    assert!(fresh.assembled_bytes_with_reserve <= context);
}

#[test]
fn inspect_work_view_exposure_protects_later_shared_budget_navigation() {
    let (mut f, private) = off_page_delivery_fixture();
    let prepared = f.prepare("inspect-then-navigate-public-sources");
    let public_source: p::ExactRef =
        serde_json::from_value(interface_control(&prepared)["work_artifacts"][0]["source"].clone())
            .unwrap();
    let mut response = decision(&prepared, true);
    response.operations = vec![
        inspect(p::WorkCollection::Deliveries, 1, None),
        p::Operation::Read {
            id: "later-public-read".into(),
            reference: public_source,
        },
        p::Operation::Search {
            id: "later-public-search".into(),
            scope: p::Scope::Fragments {},
            query: "Synthetic".into(),
            regex: false,
            limit: 1,
        },
        p::Operation::Discover {
            id: "later-public-discovery".into(),
            request: f::DiscoveryRequest {
                scope: f::Scope::Fragments,
                surface: f::SearchSurface::Path,
                mode: f::SearchMode::Listing,
                hidden_paths: f::HiddenPaths::Include,
                retirement: f::RetirementFilter::AllRetained,
            },
            limit: 1,
            cursor: None,
        },
    ];
    inspection_adopt(&mut f, &prepared, &response);
    let state = f.app.snapshot().unwrap();
    let dependent = state
        .observations
        .values()
        .filter(|o| o.invocation.as_deref() == Some("inspect-then-navigate-public-sources"))
        .collect::<Vec<_>>();
    assert_eq!(dependent.len(), 5);
    for observation in dependent {
        assert!(
            serde_json::to_string(&observation.exposure)
                .unwrap()
                .contains("private-delivery-input"),
            "{} must inherit the consulted view restriction",
            observation.kind
        );
    }
    f.app
        .revoke_policy(
            "operator",
            private.identity(),
            "Synthetic budget-causality revocation",
        )
        .unwrap();
    let fresh = f.prepare("recheck-budget-dependent-navigation");
    let control = interface_control(&fresh);
    assert!(control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o["status"] == "withheld_unavailable"));
    assert!(!fresh.prompt.contains("PRIVATE DELIVERY INPUT BODY"));
}

fn inspection_private_context_fixture(
    context_bytes: usize,
    scan_bytes: Option<usize>,
) -> (Fixture, f::Policy, f::FileRevision) {
    let mut private = restricted_policy();
    private.id = "private-result-cap".into();
    private.revision = "private-result-cap-1".into();
    private.readers = set(&["a"]);
    let mut configuration = config();
    configuration.context_bytes = context_bytes;
    configuration.policies.active_policies.push(private.clone());
    if let Some(scan) = scan_bytes {
        configuration.navigation_per_call.scanned_bytes = scan;
    }
    let mut f = judgment_records_fixture(configuration, true);
    let mut asset = record(
        "private-result-cap-asset",
        f::Scope::Assets,
        &"PRIVATE CAP BODY ".repeat(2048),
    );
    asset.policy = private.clone();
    let asset = asset.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![asset.clone()], false, NOW)
        .unwrap();
    (f, private, asset)
}
fn inspection_private_context_limit() -> usize {
    let (mut f, _, asset) = inspection_private_context_fixture(config().context_bytes, None);
    let prepared = f.prepare("calibrate-private-selected-context");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select exact private context for the bounded fixture.".into(),
        references: vec![reference(&asset)],
    };
    inspection_adopt(&mut f, &prepared, &response);
    f.prepare("calibrated-private-context")
        .assembled_bytes_with_reserve
        + 8000
}
fn inspection_select_private(f: &mut Fixture, asset: &f::FileRevision, id: &str, later_read: bool) {
    let prepared = f.prepare(id);
    assert!(!serde_json::to_string(&prepared.exposure)
        .unwrap()
        .contains("private-result-cap-asset"));
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Use newly selected private context.".into(),
        references: vec![reference(asset)],
    };
    response
        .operations
        .push(inspect(p::WorkCollection::Judgments, 1, None));
    if later_read {
        let public_source: p::ExactRef = serde_json::from_value(
            interface_control(&prepared)["work_artifacts"][0]["source"].clone(),
        )
        .unwrap();
        response.operations.push(p::Operation::Read {
            id: "after-private-cap".into(),
            reference: public_source,
        });
    }
    inspection_adopt(f, &prepared, &response);
}

#[test]
fn inspect_work_new_private_selection_cap_revocation_withholds_derived_metadata() {
    let context = inspection_private_context_limit();
    for status_only in [false, true] {
        let (mut f, private, asset) =
            inspection_private_context_fixture(context, status_only.then_some(0));
        inspection_select_private(&mut f, &asset, "select-private-and-inspect", true);
        let state = f.app.snapshot().unwrap();
        let page = state
            .observations
            .values()
            .find(|o| {
                o.kind == "work_inspection"
                    && o.invocation.as_deref() == Some("select-private-and-inspect")
            })
            .unwrap();
        if status_only {
            assert_eq!(page.payload["status"], "budget_exhausted");
        } else {
            assert_eq!(
                page.payload["records"][0]["status"],
                "record_exceeds_result_bound"
            );
        }
        assert!(!page.sources.iter().any(|r| r.source == asset.source));
        assert!(page.qualified_roots.is_empty());
        f.app
            .revoke_policy(
                "operator",
                private.identity(),
                "Revoke newly selected sizing input",
            )
            .unwrap();
        let fresh = f.prepare("private-sizing-input-revoked");
        let control = interface_control(&fresh);
        let observations = control["actual_observations"].as_array().unwrap();
        assert!(observations.iter().all(|o| o["status"] == "withheld_unavailable"),
            "inspection, later Read and accounting must retain private sizing input: {observations:?}");
        assert!(!fresh.prompt.contains("PRIVATE CAP BODY"));
    }
}

#[test]
fn inspect_work_new_private_selection_cap_preserves_forwarding_restriction() {
    let context = inspection_private_context_limit();
    let (mut f, _, asset) = inspection_private_context_fixture(context, None);
    inspection_select_private(&mut f, &asset, "select-private-cap-for-forwarding", false);
    let state = f.app.snapshot().unwrap();
    let page = state
        .observations
        .values()
        .find(|o| {
            o.kind == "work_inspection"
                && o.invocation.as_deref() == Some("select-private-cap-for-forwarding")
        })
        .unwrap();
    assert!(
        serde_json::to_string(&page.exposure)
            .unwrap()
            .contains("private-result-cap-asset"),
        "the metadata's own exposure must retain the sizing input"
    );
    assert!(!page.sources.iter().any(|r| r.source == asset.source));
    let fresh = f.prepare("forward-private-cap-metadata");
    let payload = inspected(&fresh);
    let mut response = decision(&fresh, true);
    response.operations.push(p::Operation::Communicate {
        id: "forward-private-sized-page".into(),
        recipient: "b".into(),
        subject: "Sized page".into(),
        text: serde_json::to_string(&payload).unwrap(),
    });
    let report = f.run(&fresh, &response);
    assert_eq!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
    assert!(report
        .reason
        .unwrap()
        .contains("restricted supplied material"));
    assert!(f.app.snapshot().unwrap().messages.is_empty());
}

#[test]
fn inspect_work_discover_without_fitting_fallback_keeps_valid_companion() {
    let mut configuration = config();
    configuration.navigation_per_call.result_bytes = 0;
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let prepared = f.prepare("discover-no-result-and-communicate");
    let mut response = decision(&prepared, true);
    response.operations = vec![
        p::Operation::Discover {
            id: "no-result".into(),
            request: f::DiscoveryRequest {
                scope: f::Scope::Assets,
                surface: f::SearchSurface::Path,
                mode: f::SearchMode::Listing,
                hidden_paths: f::HiddenPaths::Include,
                retirement: f::RetirementFilter::AllRetained,
            },
            limit: 1,
            cursor: None,
        },
        p::Operation::Communicate {
            id: "still-valid".into(),
            recipient: "b".into(),
            subject: "Valid companion".into(),
            text: "No fitting discovery result is an intentional omission.".into(),
        },
    ];
    inspection_adopt(&mut f, &prepared, &response);
    let state = f.app.snapshot().unwrap();
    assert_eq!(state.messages.len(), 1);
    assert!(!state.observations.values().any(|o| o.kind == "discovery"));
    let accounting = state
        .observations
        .values()
        .find(|o| {
            o.kind == "host_accounting"
                && o.invocation.as_deref() == Some("discover-no-result-and-communicate")
        })
        .unwrap();
    assert_eq!(accounting.payload["actual_navigation"]["calls"], 1);
    assert_eq!(accounting.payload["actual_navigation"]["result_bytes"], 0);
}

fn terminal_failure_response(prepared: &Prepared) -> p::Decision {
    let mut response = decision(prepared, true);
    response
        .operations
        .push(inspect(p::WorkCollection::Deliveries, 1, None));
    response.operations.push(p::Operation::InspectWork {
        id: "second-inspect".into(),
        collection: p::WorkCollection::Deliveries,
        limit: 1,
        cursor: None,
    });
    response
}
fn terminal_failure_held_fixture() -> (Fixture, f::Policy) {
    let (mut f, private) = off_page_delivery_fixture_with_budget(1, 1835);
    let prepared = f.prepare("terminal-failure-high-view");
    assert!(!serde_json::to_string(&prepared.exposure)
        .unwrap()
        .contains("private-delivery-input"));
    let before = serde_json::to_value(f.app.snapshot().unwrap()).unwrap();
    let report = f.run(&prepared, &terminal_failure_response(&prepared));
    assert_eq!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
    assert!(report
        .reason
        .unwrap()
        .contains("work inspection result/status exceeds remaining"));
    assert_eq!(
        serde_json::to_value(f.app.snapshot().unwrap()).unwrap(),
        before,
        "held response must roll back all substantive state"
    );
    let root = f.app.state_root().to_owned();
    f.app = Runtime::open(&root).unwrap();
    (f, private)
}
#[test]
fn terminal_failure_budget_boundary_preserves_lower_view_adoption() {
    let (mut f, private) = off_page_delivery_fixture_with_budget(1, 1835);
    f.app
        .revoke_policy(
            "operator",
            private.identity(),
            "Synthetic lower permission view",
        )
        .unwrap();
    let prepared = f.prepare("terminal-failure-lower-view");
    let report = f.run(&prepared, &terminal_failure_response(&prepared));
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    let state = f.app.snapshot().unwrap();
    let observations = state
        .observations
        .values()
        .filter(|o| {
            o.kind == "work_inspection"
                && o.invocation.as_deref() == Some("terminal-failure-lower-view")
        })
        .collect::<Vec<_>>();
    assert_eq!(observations.len(), 2);
    assert_eq!(
        observations
            .iter()
            .map(|o| serde_json::to_vec(&o.payload).unwrap().len())
            .sum::<usize>(),
        1835
    );
    assert!(observations
        .iter()
        .any(|o| o.payload["status"] == "budget_exhausted"));
}
#[test]
fn terminal_failure_private_budget_recovery_is_withheld_after_revocation() {
    let (mut f, private) = terminal_failure_held_fixture();
    f.app
        .revoke_policy(
            "operator",
            private.identity(),
            "Synthetic restriction revocation before recovery",
        )
        .unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let prepared = f.prepare("terminal-failure-after-revocation");
    let control = interface_control(&prepared);
    assert!(
        !control["actual_observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["kind"] == "terminal_recovery"),
        "held outcome derived from revoked off-page origin must be withheld"
    );
    assert!(!prepared.prompt.contains("PRIVATE DELIVERY INPUT BODY"));
}
#[test]
fn terminal_failure_private_budget_recovery_cannot_forward_to_excluded_peer() {
    let (mut f, _) = terminal_failure_held_fixture();
    f.app.resume("operator", "a", "work").unwrap();
    let prepared = f.prepare("terminal-failure-forward-attempt");
    let control = interface_control(&prepared);
    let recovered = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "terminal_recovery")
        .expect("permitted exact failure lineage remains useful after recovery");
    assert_eq!(
        recovered["payload"]["disposition"]["adoption_error"]["response_body_supplied"],
        false
    );
    assert!(!prepared.prompt.contains("PRIVATE DELIVERY INPUT BODY"));
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Communicate {
        id: "forward-terminal".into(),
        recipient: "c".into(),
        subject: "Recovered outcome".into(),
        text: serde_json::to_string(&recovered["payload"]).unwrap(),
    });
    let report = f.run(&prepared, &response);
    assert_eq!(
        report.status,
        AdoptionStatus::Held,
        "recovery provenance must restrict forwarding: {:?}",
        report.reason
    );
    assert!(report
        .reason
        .unwrap()
        .contains("restricted supplied material"));
    assert!(f.app.snapshot().unwrap().messages.is_empty());
}

#[test]
fn inspect_work_context_exposure_does_not_change_collection_cursor_identity() {
    let cap = inspection_private_context_limit();
    let (mut f, _, asset) = inspection_private_context_fixture(cap, None);
    inspection_select_private(&mut f, &asset, "private-context-first-page", false);
    let prepared = f.prepare("clear-private-context-and-continue");
    let first = inspected(&prepared);
    let cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let snapshot = first["snapshot"].clone();
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Clear {
        reason: "Reclaim optional private context.".into(),
    };
    response
        .operations
        .push(inspect(p::WorkCollection::Judgments, 1, Some(cursor)));
    inspection_adopt(&mut f, &prepared, &response);
    let fresh = f.prepare("receive-cleared-context-continuation");
    let continued = inspected(&fresh);
    assert_eq!(continued["status"], "page");
    assert_eq!(continued["snapshot"], snapshot);
    assert_eq!(continued["start_position"], 1);
    assert!(continued["next_cursor"].is_null());
}
