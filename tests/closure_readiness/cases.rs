use super::*;

fn released(configuration: Config) -> Fixture {
    let mut original = need();
    original.stakeholders = set(&["human"]);
    original.outcomes.get_mut("result").unwrap().criterion =
        "An attributed judgment of the supplied exact writing is sufficient; human acceptance is separately required.".into();
    let mut f = Fixture::configured(true, configuration, seed("a"), original);
    f.accept();
    let prepared = f.prepare("author-human-release");
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Write exact fixture note.".into(),
        operation_ids: vec!["save-note".into()],
    };
    response.operations = vec![
        write(
            "save-note",
            p::Scope::Assets {},
            "model-note",
            MODEL_NOTE,
            None,
        ),
        effect(
            "register-note",
            WorkAction::PutArtifact {
                artifact: "model-artifact".into(),
                base: None,
                subject: proposed("model-note", "save-note"),
            },
            &["save-note"],
        ),
    ];
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Read own actual note on next call.".into(),
        references: vec![proposed("model-note", "save-note")],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("judge-human-release");
    assert!(prepared.prompt.contains(MODEL_NOTE));
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    let mut response = decision(&prepared, true);
    response.operations.push(model_evidence_operation(&key));
    response.operations.extend(model_judgment_and_shared(&key));
    response.operations.push(effect(
        "release-note",
        WorkAction::Release {
            id: "model-note-release".into(),
            shared: "model-note-shared".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    f
}
fn deliver(f: &mut Fixture) {
    let prepared = f.prepare("deliver-human-release");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "deliver-note",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
}

// These helpers alter only ephemeral synthetic fixtures to isolate disclosure
// and byte-growth dependencies. They are not a human-acceptance ingress.
fn synthetic_state(f: &mut Fixture, change: impl FnOnce(&mut Value)) {
    let root = f.app.state_root().to_owned();
    let old = closeout_head(&root);
    let mut next = old.clone();
    change(&mut next.body);
    next.revision += 1;
    let mut store = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    store.compare_and_swap(Some(old.revision), next).unwrap();
}
fn evidence(f: &mut Fixture) -> Value {
    f.app
        .closure_readiness_evidence("a", "work", "model-note-release", NOW)
        .unwrap()
}
fn synthetic_decision(f: &mut Fixture, accepts: bool, restricted: bool) {
    let state = f.app.snapshot().unwrap();
    let work = state.board.work(&"work".into()).unwrap();
    let delivery = &work.deliveries[0];
    let input = HumanStakeholderDecisionRequest {
        binding: WorkAuthority {
            work: "work".into(),
            version: work.version,
            authority: work.mandate().authority.clone(),
        },
        id: format!(
            "synthetic-{}-{}",
            if accepts {
                "accept"
            } else {
                "explicit-current-rejection-receipt"
            },
            state.operator_events.len()
        ),
        release: "model-note-release".into(),
        release_sha256: s::sha256_bytes(
            &serde_json::to_vec(
                &serde_json::to_value(&work.releases[&"model-note-release".into()]).unwrap(),
            )
            .unwrap(),
        ),
        delivery: delivery.id.0.clone(),
        delivery_receipt: delivery.receipt.clone(),
        accepts,
        statement: "Explicit synthetic human statement for a disclosure-only fixture.".into(),
    };
    let receipt = f
        .app
        .record_stakeholder_decision("human", input, NOW)
        .unwrap();
    // The receipt is created through real ingress. Only disclosure visibility and
    // pending status are isolated below; this is not a live human interaction.
    synthetic_state(f, |state| {
        let decision = state["board"]["works"]["work"]["stakeholder_decisions"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        state["board"]["works"]["work"]["stakeholder_decisions"] = json!([decision]);
        let observation_id = state["observations"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, o)| o["actor"] == "a" && o["payload"]["receipt"] == json!(receipt.receipt))
            .unwrap()
            .0
            .clone();
        state["personas"]["a"]["focus"]["work"]["pending_observations"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id != &observation_id);
        let mut observation = state["observations"]
            .as_object_mut()
            .unwrap()
            .remove(&observation_id)
            .unwrap();
        observation["id"] = json!("synthetic-consent");
        if restricted {
            let mut policy = policy();
            policy.id = "synthetic-denied-consent".into();
            policy.readers = set(&["b"]);
            state["config"]["policies"]["active_policies"]
                .as_array_mut()
                .unwrap()
                .push(json!(policy.clone()));
            observation["exposure"]["policies"]
                .as_array_mut()
                .unwrap()
                .push(json!({"source":"synthetic-consent-origin","revision":"1","policy":policy}));
        }
        state["observations"]["synthetic-consent"] = observation;
    });
}

fn inspect_release(f: &mut Fixture, id: &str, asset: Option<&f::FileRevision>) {
    let prepared = f.prepare(id);
    let mut response = decision(&prepared, true);
    response.next_assets = if let Some(asset) = asset {
        p::SelectionIntent::Replace {
            reason: "Preserve a fitting synthetic optional selection.".into(),
            references: vec![reference(asset)],
        }
    } else {
        p::SelectionIntent::Clear {
            reason: "No optional context needed.".into(),
        }
    };
    response.operations.push(p::Operation::InspectWork {
        id: "inspect-known-release".into(),
        collection: p::WorkCollection::Releases,
        limit: 1,
        cursor: None,
    });
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
}

#[test]
fn closure_readiness_hidden_delivery_equals_absent_and_permitted_receipt_is_actionable() {
    let mut configuration = config();
    let mut restricted = policy();
    restricted.id = "delivery-input-private".into();
    configuration
        .policies
        .active_policies
        .push(restricted.clone());
    let mut f = released(configuration);
    let absent = evidence(&mut f);
    let mut input = record(
        "private-delivery-only",
        f::Scope::Assets,
        "Synthetic private delivery context.",
    );
    input.policy = restricted.clone();
    input = input.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![input.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("select-private-origin");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Use synthetic private delivery context.".into(),
        references: vec![reference(&input)],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    deliver(&mut f);
    let permitted = evidence(&mut f);
    assert_eq!(permitted["unestablished_deliveries"], json!([]));
    assert_eq!(
        permitted["unestablished_stakeholder_acceptance"],
        json!(["human"])
    );
    inspect_release(&mut f, "inspect-visible-delivery", None);
    let supplied = f.prepare("observe-visible-delivery");
    assert!(supplied
        .exposure
        .as_ref()
        .unwrap()
        .policies
        .iter()
        .any(|p| p.policy.id == restricted.id));
    assert!(interface_control(&supplied)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o["kind"] != "accepted_local_delivery"));
    assert_eq!(
        interface_control(&supplied)["closure_readiness"]["records"][0],
        permitted
    );
    // Finish this synthetic call; no readiness fact is newly supplied by it.
    assert_eq!(
        f.run(&supplied, &decision(&supplied, true)).status,
        AdoptionStatus::Adopted
    );
    f.app
        .revoke_policy(
            "operator",
            restricted.identity(),
            "Synthetic receipt-origin revocation.",
        )
        .unwrap();
    assert_eq!(
        evidence(&mut f),
        absent,
        "hidden receipt existence must not change the public projection"
    );
    inspect_release(&mut f, "inspect-after-revocation", None);
    let supplied = f.prepare("observe-hidden-delivery");
    assert_eq!(
        interface_control(&supplied)["closure_readiness"]["records"][0],
        absent
    );
    assert!(!supplied
        .exposure
        .as_ref()
        .unwrap()
        .policies
        .iter()
        .any(|p| p.policy.id == restricted.id));
}

#[test]
fn closure_readiness_hidden_positive_equals_absent_and_unknown_origin_is_not_consent() {
    let mut f = released(config());
    deliver(&mut f);
    let absent = evidence(&mut f);
    synthetic_decision(&mut f, true, true);
    assert_eq!(evidence(&mut f), absent);
    synthetic_state(&mut f, |state| {
        state["observations"]
            .as_object_mut()
            .unwrap()
            .remove("synthetic-consent");
    });
    assert_eq!(
        evidence(&mut f),
        absent,
        "legacy board-only decision has unknown disclosure origin"
    );
    synthetic_decision(&mut f, true, false);
    let permitted = evidence(&mut f);
    assert_eq!(permitted["unestablished_stakeholder_acceptance"], json!([]));
    assert_eq!(permitted["permitted_prerequisites_observed"], true);
    assert_eq!(permitted["release_current"], "unestablished");
    assert_eq!(permitted["completion_established"], false);
    // A hidden newer rejection must neither be disclosed nor turn this partial
    // view into a false claim of globally latest consent or closure authority.
    synthetic_state(&mut f, |state| {
        let mut hidden = state["board"]["works"]["work"]["stakeholder_decisions"][0].clone();
        hidden["accepts"] = json!(false);
        hidden["receipt"] = json!(w::ExactRef::new("hidden-decline", 1, b"hidden fixture"));
        state["board"]["works"]["work"]["stakeholder_decisions"]
            .as_array_mut()
            .unwrap()
            .push(hidden);
    });
    assert_eq!(evidence(&mut f), permitted);
    assert!(
        !f.app
            .snapshot()
            .unwrap()
            .board
            .closure_readiness(&"work".into(), &"model-note-release".into())
            .unwrap()
            .ready
    );
}

#[test]
fn closure_readiness_only_receipt_growth_triggers_funded_selection_repair() {
    let mut f = released(repair_config(1));
    deliver(&mut f);
    synthetic_decision(&mut f, true, false);
    let filler = record("growth-filler", f::Scope::Assets, &"F".repeat(45_000));
    f.app
        .import_files("operator", "a", vec![filler.clone()], true, NOW)
        .unwrap();
    inspect_release(&mut f, "retain-release-observation", Some(&filler));
    let baseline_state = persisted_state(&f.app);
    let fit: SelectionFit = serde_json::from_value(
        baseline_state["personas"]["a"]["focus"]["work"]["selection_fit"].clone(),
    )
    .unwrap();
    let prepared = f.prepare("growth-baseline-00");
    let cap = prepared.assembled_bytes_with_reserve;
    let baseline_control = interface_control(&prepared);
    let baseline_diagnostic = baseline_control["closure_readiness"].clone();
    assert_eq!(
        baseline_diagnostic["records"][0]["permitted_prerequisites_observed"],
        true
    );
    // Restore this test-only exact state (no provider dispatch), set its actual
    // measured fit as the cap, and change only the retained permitted decision.
    synthetic_state(&mut f, |state| {
        *state = baseline_state.clone();
        state["config"]["context_bytes"] = json!(cap);
    });
    synthetic_decision(&mut f, false, false);
    let after_diagnostic = evidence(&mut f);
    let diagnostic_growth = serde_json::to_vec(&after_diagnostic)
        .unwrap()
        .len()
        .checked_sub(
            serde_json::to_vec(&baseline_diagnostic["records"][0])
                .unwrap()
                .len(),
        )
        .unwrap();
    assert!(diagnostic_growth > 0);
    let result = f.app.prepare(
        PrepareRequest {
            id: "growth-overflow-0".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    let Err(RuntimeError::SelectionOverflow {
        growth_bytes,
        selection_sha256,
        ..
    }) = result
    else {
        panic!("expected exact readiness-only overflow: {result:?}")
    };
    assert_eq!(selection_sha256, fit.selection_sha256);
    assert_eq!(
        growth_bytes,
        fit.growth_bytes + diagnostic_growth,
        "only the exact receipt-bearing readiness material grew"
    );
    let state = f.app.snapshot().unwrap();
    assert!(
        state.personas["a"].focus["work"].repair_trigger.is_some(),
        "diagnostic growth must fund the declared repair path"
    );
    assert_eq!(state.work_meta["work"].repair_attempts_left, 1);
    let repair = f
        .app
        .prepare(repair_request("repair-readiness-growth"), NOW)
        .unwrap();
    assert!(!repair.prompt.contains(&"F".repeat(1000)));
    assert_eq!(
        f.run(&repair, &repair_decision(&repair, false)).status,
        AdoptionStatus::Adopted
    );
}

fn later_exact_decision(f: &mut Fixture, accepts: bool) -> HumanStakeholderDecisionReceipt {
    let state = f.app.snapshot().unwrap();
    let work = state.board.work(&"work".into()).unwrap();
    let delivery = work.deliveries.last().unwrap();
    f.app
        .record_stakeholder_decision(
            "human",
            HumanStakeholderDecisionRequest {
                binding: WorkAuthority {
                    work: "work".into(),
                    version: work.version,
                    authority: work.mandate().authority.clone(),
                },
                id: "later-exact-decision".into(),
                release: "model-note-release".into(),
                release_sha256: s::sha256_bytes(
                    &serde_json::to_vec(
                        &serde_json::to_value(&work.releases[&"model-note-release".into()])
                            .unwrap(),
                    )
                    .unwrap(),
                ),
                delivery: delivery.id.0.clone(),
                delivery_receipt: delivery.receipt.clone(),
                accepts,
                statement: "The later explicit synthetic human decision about this exact delivery."
                    .into(),
            },
            NOW,
        )
        .unwrap()
}

fn close_with_supplied_evidence(f: &mut Fixture, prepared: &Prepared) -> AdoptionReport {
    let mut response = decision(prepared, true);
    response.operations.push(effect(
        "close-with-exact-consent",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    f.run(prepared, &response)
}

fn hidden_latest_close(accepts: bool) -> (Value, AdoptionReport) {
    let mut f = released(config());
    deliver(&mut f);
    synthetic_decision(&mut f, true, false);
    let permitted = evidence(&mut f);
    let later = later_exact_decision(&mut f, accepts);
    // Model the retained state of an actor who lacked an acceptance when the
    // later human decision was recorded: its exact Board/event receipts remain,
    // but no actor observation exists for that later decision.
    synthetic_state(&mut f, |state| {
        let hidden_ids = state["observations"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, o)| o["actor"] == "a" && o["payload"]["receipt"] == json!(later.receipt))
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(hidden_ids.len(), 1);
        state["observations"]
            .as_object_mut()
            .unwrap()
            .retain(|id, _| !hidden_ids.contains(id));
        state["personas"]["a"]["focus"]["work"]["pending_observations"]
            .as_array_mut()
            .unwrap()
            .retain(|id| !hidden_ids.contains(id.as_str().unwrap()));
    });
    assert_eq!(evidence(&mut f), permitted);
    let prepared = f.prepare("close-after-hidden-latest-consent");
    let supplied = interface_control(&prepared)["closure_readiness"]["records"][0].clone();
    assert_eq!(
        supplied, permitted,
        "diagnostics must remain permission-filtered"
    );
    assert!(!prepared.prompt.contains(&later.receipt.id));
    let before = f.app.snapshot().unwrap().board;
    let result = close_with_supplied_evidence(&mut f, &prepared);
    if result.status == AdoptionStatus::Held {
        assert_eq!(f.app.snapshot().unwrap().board, before);
    }
    (supplied, result)
}

#[test]
fn closure_exact_latest_hidden_positive_and_negative_hold_without_disclosing_value() {
    let (negative_view, negative) = hidden_latest_close(false);
    let (positive_view, positive) = hidden_latest_close(true);
    eprintln!(
        "hidden latest negative: {:?} {:?}; positive: {:?} {:?}",
        negative.status, negative.reason, positive.status, positive.reason
    );
    assert_eq!(negative_view, positive_view);
    assert_eq!(positive.status, AdoptionStatus::Held);
    assert_eq!(negative.status, AdoptionStatus::Held);
    assert_eq!(negative.reason, positive.reason);
    assert!(positive
        .reason
        .unwrap()
        .contains("required exact permitted human acceptance was not supplied"));
}

#[test]
fn closure_exact_latest_permitted_supplied_positive_can_close() {
    let mut f = released(config());
    deliver(&mut f);
    synthetic_decision(&mut f, false, false);
    let later = later_exact_decision(&mut f, true);
    let prepared = f.prepare("close-after-latest-supplied-consent");
    let supplied = interface_control(&prepared)["closure_readiness"]["records"][0].clone();
    assert_eq!(
        supplied["stakeholder_decisions"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        supplied["stakeholder_decisions"][0]["receipt"],
        json!(later.receipt)
    );
    assert_eq!(
        close_with_supplied_evidence(&mut f, &prepared).status,
        AdoptionStatus::Adopted
    );
    assert!(f
        .app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&"work".into()));
}

#[test]
fn closure_exact_latest_permitted_supplied_negative_cannot_use_older_positive() {
    let mut f = released(config());
    deliver(&mut f);
    synthetic_decision(&mut f, true, false);
    let later = later_exact_decision(&mut f, false);
    let prepared = f.prepare("close-after-latest-supplied-rejection");
    let supplied = interface_control(&prepared)["closure_readiness"]["records"][0].clone();
    assert_eq!(
        supplied["stakeholder_decisions"][0]["receipt"],
        json!(later.receipt)
    );
    assert_eq!(supplied["stakeholder_decisions"][0]["accepts"], false);
    let before = f.app.snapshot().unwrap().board;
    assert_eq!(
        close_with_supplied_evidence(&mut f, &prepared).status,
        AdoptionStatus::Held
    );
    assert_eq!(f.app.snapshot().unwrap().board, before);
}
