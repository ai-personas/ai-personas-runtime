use super::*;

fn released(configuration: Config) -> Fixture {
    released_with(configuration, seed("a"), true, false)
}
fn released_with(configuration: Config, primary: Seed, human: bool, peer: bool) -> Fixture {
    let mut original = need();
    original.stakeholders = if human {
        set(&["human"])
    } else {
        BTreeSet::new()
    };
    if peer {
        original.invitations.push(Invitation {
            id: "offer-b".into(),
            to: "b".into(),
            terms: w::CommitmentTerms {
                mandate_revision: 1,
                exact_terms: "Optional independent contribution.".into(),
                outcomes: BTreeSet::new(),
                review_roles: BTreeSet::new(),
            },
        });
    }
    original.outcomes.get_mut("result").unwrap().criterion =
        "An attributed judgment of the supplied exact writing is sufficient; human acceptance is separately required.".into();
    let mut f = Fixture::configured(true, configuration, primary, original);
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
fn request(f: &mut Fixture, id: &str, accepts: bool) -> HumanStakeholderDecisionRequest {
    let state = f.app.snapshot().unwrap();
    let w = state.board.work(&"work".into()).unwrap();
    let d = &w.deliveries[0];
    HumanStakeholderDecisionRequest {
        binding: WorkAuthority {
            work: "work".into(),
            version: w.version,
            authority: w.mandate().authority.clone(),
        },
        id: id.into(),
        release: "model-note-release".into(),
        release_sha256: s::sha256_bytes(
            &serde_json::to_vec(
                &serde_json::to_value(&w.releases[&"model-note-release".into()]).unwrap(),
            )
            .unwrap(),
        ),
        delivery: d.id.0.clone(),
        delivery_receipt: d.receipt.clone(),
        accepts,
        statement: if accepts {
            "Synthetic human explicitly accepts this exact delivered note."
        } else {
            "Synthetic human explicitly declines this exact delivered note."
        }
        .into(),
    }
}
fn rejected_unchanged(
    f: &mut Fixture,
    principal: &str,
    input: HumanStakeholderDecisionRequest,
    now: i64,
) {
    let before = persisted_state(&f.app);
    assert!(f
        .app
        .record_stakeholder_decision(principal, input, now)
        .is_err());
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn human_stakeholder_ingress_is_explicit_durable_and_enables_only_later_authored_close() {
    let mut f = released(config());
    deliver(&mut f);
    f.app
        .pause(
            "operator",
            "work",
            "Explicit synthetic human review boundary.",
        )
        .unwrap();
    let initial = f.app.snapshot().unwrap();
    let w = initial.board.work(&"work".into()).unwrap();
    assert!(w.stakeholder_decisions.is_empty());
    assert!(matches!(w.phase, w::Phase::Paused { .. }));
    let input = request(&mut f, "explicit-accept", true);
    let receipt = f
        .app
        .record_stakeholder_decision("human", input.clone(), NOW)
        .unwrap();
    let state = f.app.snapshot().unwrap();
    let w = state.board.work(&"work".into()).unwrap();
    assert_eq!(w.stakeholder_decisions.len(), 1);
    let accepted = &w.stakeholder_decisions[0];
    assert_eq!(accepted.by.principal, w::Origin::Human("human".into()));
    assert_eq!(accepted.receipt, receipt.receipt);
    assert_eq!(accepted.by.authority, input.binding.authority);
    assert_eq!(accepted.by.version, input.binding.version);
    assert!(matches!(w.phase, w::Phase::Paused { .. }));
    assert!(!state.board.completion_supported(&"work".into()));
    assert_eq!(
        state.personas["a"].character.as_ref().unwrap().revision,
        initial.personas["a"].character.as_ref().unwrap().revision
    );
    assert_eq!(
        state.personas["a"].focus["work"].permit,
        initial.personas["a"].focus["work"].permit
    );
    assert_eq!(
        state.work_meta["work"].attempts_left,
        initial.work_meta["work"].attempts_left
    );
    let event = state
        .operator_events
        .iter()
        .find(|e| e["kind"] == "human_stakeholder_decision")
        .unwrap();
    assert_eq!(event["body"]["request"]["statement"], input.statement);
    assert_eq!(
        hex::encode(receipt.receipt.digest),
        s::sha256_bytes(&serde_json::to_vec(&event["body"]).unwrap())
    );
    assert!(state.work_meta["work"].human_evidence.is_empty());
    let root = f.app.state_root().to_owned();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    assert!(!f.app.snapshot().unwrap().personas["a"].focus["work"].permit);
    f.app.resume("operator", "a", "work").unwrap();
    let prepared = f.prepare("close-after-human-acceptance");
    let control = interface_control(&prepared);
    let observation = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "human_stakeholder_decision")
        .unwrap();
    assert_eq!(
        observation["payload"]["record"]["by"]["principal"]["Human"],
        "human"
    );
    assert_eq!(observation["payload"]["metadata_only"], true);
    assert!(!prepared.prompt.contains(&input.statement));
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "close",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    assert!(f
        .app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&"work".into()));
}

#[test]
fn human_stakeholder_ingress_rejects_wrong_principal_authority_release_receipt_and_replay() {
    let mut f = released(config());
    deliver(&mut f);
    let good = request(&mut f, "accept", true);
    rejected_unchanged(&mut f, "a", good.clone(), NOW);
    rejected_unchanged(&mut f, "someone-else", good.clone(), NOW);
    let mut bad = good.clone();
    bad.binding.authority.digest[0] ^= 1;
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.binding.version.revision += 1;
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.release = "missing".into();
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.release_sha256 = "0".repeat(64);
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.delivery_receipt.digest[0] ^= 1;
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.delivery = "missing".into();
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.statement = " ".into();
    rejected_unchanged(&mut f, "human", bad, NOW);
    let mut bad = good.clone();
    bad.statement = "x".repeat(4097);
    rejected_unchanged(&mut f, "human", bad, NOW);
    f.app
        .record_stakeholder_decision("human", good, NOW)
        .unwrap();
    let replay = request(&mut f, "accept", true);
    rejected_unchanged(&mut f, "human", replay, NOW);
    let decline = request(&mut f, "decline", false);
    f.app
        .record_stakeholder_decision("human", decline, NOW)
        .unwrap();
    let prepared = f.prepare("close-after-decline");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "close",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
}

#[test]
fn human_stakeholder_ingress_rechecks_current_source_policy_and_expiry() {
    let mut configuration = config();
    let mut restricted = policy();
    restricted.id = "delivery-input-policy".into();
    restricted.expires_at_unix = Some(NOW + 100);
    configuration
        .policies
        .active_policies
        .push(restricted.clone());
    let mut f = released(configuration);
    let mut input_file = record(
        "delivery-input",
        f::Scope::Assets,
        "Private delivery-only input.",
    );
    input_file.policy = restricted.clone();
    input_file = input_file.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![input_file.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("select-delivery-input");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Explicitly select delivery-only input.".into(),
        references: vec![p::ExactRef {
            source: input_file.source,
            revision: input_file.revision,
            representation: "utf8".into(),
            start: None,
            end: None,
            lifetime: Some("work".into()),
        }],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    deliver(&mut f);
    let state = f.app.snapshot().unwrap();
    assert!(!state.work_meta["work"]
        .exposure
        .as_ref()
        .unwrap()
        .policies
        .iter()
        .any(|p| p.policy.id == restricted.id));
    let input = request(&mut f, "accept", true);
    rejected_unchanged(&mut f, "human", input.clone(), NOW + 100);
    f.app
        .revoke_policy(
            "operator",
            restricted.identity(),
            "Synthetic delivery-origin revocation.",
        )
        .unwrap();
    rejected_unchanged(&mut f, "human", input, NOW);

    let mut f = released(config());
    deliver(&mut f);
    let input = request(&mut f, "accept", true);
    f.app
        .withdraw_source("operator", "model-note", "Explicit synthetic withdrawal.")
        .unwrap();
    rejected_unchanged(&mut f, "human", input, NOW);
}

#[test]
fn human_stakeholder_ingress_rejects_a_now_stale_released_artifact() {
    let mut f = released(config());
    deliver(&mut f);
    let prepared = f.prepare("revise-after-release");
    let state = f.app.snapshot().unwrap();
    let old = state.sources["model-note"]
        .revisions
        .last()
        .unwrap()
        .record
        .revision
        .clone();
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Replace fixture artifact.".into(),
        operation_ids: vec!["replace-note".into()],
    };
    response.operations = vec![
        write(
            "replace-note",
            p::Scope::Assets {},
            "model-note",
            "Changed exact note.",
            Some(old),
        ),
        effect(
            "register-revision",
            WorkAction::PutArtifact {
                artifact: "model-artifact".into(),
                base: Some(key),
                subject: proposed("model-note", "replace-note"),
            },
            &["replace-note"],
        ),
    ];
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let input = request(&mut f, "accept-stale", true);
    rejected_unchanged(&mut f, "human", input, NOW);
}

fn private_delivery_fixture(human: bool, peer: bool) -> (Fixture, f::Policy) {
    let mut configuration = config();
    let mut private = policy();
    private.id = "delivery-only-private".into();
    private.readers = set(&["a"]);
    private.audience = set(&["a", "human"]);
    configuration.policies.active_policies.push(private.clone());
    let mut primary = seed("a");
    primary.audience = set(&["a", "human"]);
    let mut f = released_with(configuration, primary, human, peer);
    let mut source = record(
        "private-delivery-context",
        f::Scope::Assets,
        "Private delivery rationale.",
    );
    source.policy = private.clone();
    source = source.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![source.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("select-private-delivery-context");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select private delivery context.".into(),
        references: vec![reference(&source)],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("deliver-with-private-context");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Clear {
        reason: "No further private context needed.".into(),
    };
    response.operations.push(effect(
        "private-delivery",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    (f, private)
}
fn close(f: &mut Fixture, id: &str) -> AdoptionReport {
    let prepared = f.prepare(id);
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "close",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    f.run(&prepared, &response)
}

#[test]
fn closure_ingress_fix_revoked_delivery_cannot_authorize_close() {
    let (mut f, private) = private_delivery_fixture(false, false);
    f.app
        .revoke_policy(
            "operator",
            private.identity(),
            "Revoke only private delivery provenance.",
        )
        .unwrap();
    let before = f
        .app
        .snapshot()
        .unwrap()
        .board
        .work(&"work".into())
        .unwrap()
        .clone();
    assert_eq!(
        close(&mut f, "close-with-hidden-receipt").status,
        AdoptionStatus::Held
    );
    assert_eq!(
        f.app
            .snapshot()
            .unwrap()
            .board
            .work(&"work".into())
            .unwrap(),
        &before
    );
}

#[test]
fn closure_ingress_fix_consumed_receipt_requires_fresh_explicit_inspection() {
    let mut f = released_with(config(), seed("a"), false, false);
    deliver(&mut f);
    let prepared = f.prepare("consume-delivery-receipt");
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    assert_eq!(
        close(&mut f, "close-without-supplied-receipt").status,
        AdoptionStatus::Held
    );
    f.app.resume("operator", "a", "work").unwrap();
    let prepared = f.prepare("inspect-known-release-for-close");
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::InspectWork {
        id: "inspect-release".into(),
        collection: p::WorkCollection::Deliveries,
        limit: 1,
        cursor: None,
    });
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    assert_eq!(
        close(&mut f, "close-with-fresh-receipt-evidence").status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn closure_ingress_fix_new_qualification_requires_new_exact_delivery() {
    let mut f = released(config());
    deliver(&mut f);
    add_required_qualification(&mut f);
    let input = request(&mut f, "old-qualified-receipt", true);
    rejected_unchanged(&mut f, "human", input, NOW);
    let prepared = f.prepare("redeliver-current-complete-bundle");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "redeliver",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let mut input = request(&mut f, "new-qualified-receipt", true);
    let state = f.app.snapshot().unwrap();
    let current = state
        .board
        .work(&"work".into())
        .unwrap()
        .deliveries
        .last()
        .unwrap();
    input.delivery = current.id.0.clone();
    input.delivery_receipt = current.receipt.clone();
    f.app
        .record_stakeholder_decision("human", input, NOW)
        .unwrap();
}

#[test]
fn closure_ingress_fix_private_human_decision_preserves_peer_core_and_stays_hidden() {
    let (mut f, _) = private_delivery_fixture(true, true);
    let core = f.app.snapshot().unwrap().work_meta["work"].exposure.clone();
    let before = f
        .app
        .closure_readiness_evidence("b", "work", "model-note-release", NOW)
        .unwrap();
    let input = request(&mut f, "restricted-human-acceptance", true);
    let receipt = f
        .app
        .record_stakeholder_decision("human", input, NOW)
        .unwrap();
    assert_eq!(
        f.app
            .closure_readiness_evidence("b", "work", "model-note-release", NOW)
            .unwrap(),
        before
    );
    let after = f.app.snapshot().unwrap();
    assert_eq!(after.work_meta["work"].exposure, core);
    assert_eq!(
        after
            .board
            .work(&"work".into())
            .unwrap()
            .stakeholder_decisions
            .len(),
        1
    );
    let peer = f
        .app
        .prepare(
            PrepareRequest {
                id: "peer-after-private-consent".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    assert!(!peer.prompt.contains(&receipt.receipt.id));
}

#[test]
fn closure_ingress_fix_same_response_delivery_is_not_supplied_evidence() {
    let mut f = released_with(config(), seed("a"), false, false);
    let prepared = f.prepare("deliver-and-close-without-receipt");
    let before = f.app.snapshot().unwrap();
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "deliver",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    response.operations.push(effect(
        "close",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
    let after = f.app.snapshot().unwrap();
    assert_eq!(after.local_deliveries, before.local_deliveries);
    assert_eq!(
        after.board.work(&"work".into()).unwrap(),
        before.board.work(&"work".into()).unwrap()
    );
    f.app.resume("operator", "a", "work").unwrap();
    deliver(&mut f);
    assert_eq!(
        close(&mut f, "close-after-actual-receipt").status,
        AdoptionStatus::Adopted
    );
}

fn add_required_qualification(f: &mut Fixture) {
    let before_key = f
        .app
        .snapshot()
        .unwrap()
        .board
        .work(&"work".into())
        .unwrap()
        .releases[&"model-note-release".into()]
        .candidate
        .clone();
    let qualification = record(
        "new-required-qualification",
        f::Scope::Assets,
        "Required qualification added after old delivery.",
    );
    f.app
        .import_files("operator", "a", vec![qualification.clone()], false, NOW)
        .unwrap();
    let state = f.app.snapshot().unwrap();
    let target = state.sources["model-note"]
        .revisions
        .last()
        .unwrap()
        .record
        .clone();
    let prepared = f.prepare("select-new-qualification");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Supply basis and new required qualification.".into(),
        references: vec![reference(&target), reference(&qualification)],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("adopt-new-qualification");
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Qualify the exact unchanged artifact.".into(),
        operation_ids: vec!["qualify".into()],
    };
    response.operations.push(p::Operation::WarningAdopt {
        id: "qualify".into(),
        warning_id: "new-required-qualification".into(),
        scope: p::Scope::Assets {},
        targets: vec![reference(&target)],
        uses: p::WarningUses::AllRepresentations {},
        meaning: "The supplied qualification must accompany this exact artifact.".into(),
        basis: vec![reference(&target)],
        disposition: p::WarningDisposition::Qualify {
            required: vec![reference(&qualification)],
        },
        policy_id: "local-policy".into(),
        depends_on: vec![],
    });
    let report = f.run(&prepared, &response);
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    assert_eq!(
        f.app
            .snapshot()
            .unwrap()
            .board
            .work(&"work".into())
            .unwrap()
            .releases[&"model-note-release".into()]
            .candidate,
        before_key
    );
}

#[test]
fn closure_ingress_fix_historical_acceptance_does_not_cover_new_qualification() {
    let mut f = released(config());
    deliver(&mut f);
    let input = request(&mut f, "accepted-before-qualification", true);
    f.app
        .record_stakeholder_decision("human", input, NOW)
        .unwrap();
    let historical = f
        .app
        .snapshot()
        .unwrap()
        .board
        .work(&"work".into())
        .unwrap()
        .stakeholder_decisions
        .clone();
    add_required_qualification(&mut f);
    let prepared = f.prepare("redeliver-after-historical-acceptance");
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "redeliver",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    assert_eq!(
        close(&mut f, "close-with-historical-acceptance").status,
        AdoptionStatus::Held
    );
    assert_eq!(
        f.app
            .snapshot()
            .unwrap()
            .board
            .work(&"work".into())
            .unwrap()
            .stakeholder_decisions,
        historical
    );
    f.app.resume("operator", "a", "work").unwrap();
    let mut input = request(&mut f, "accepted-current-qualification", true);
    let state = f.app.snapshot().unwrap();
    let current = state
        .board
        .work(&"work".into())
        .unwrap()
        .deliveries
        .last()
        .unwrap();
    input.delivery = current.id.0.clone();
    input.delivery_receipt = current.receipt.clone();
    f.app
        .record_stakeholder_decision("human", input, NOW)
        .unwrap();
    assert_eq!(
        close(&mut f, "close-after-current-acceptance").status,
        AdoptionStatus::Adopted
    );
    assert_eq!(
        f.app
            .snapshot()
            .unwrap()
            .board
            .work(&"work".into())
            .unwrap()
            .stakeholder_decisions[0],
        historical[0]
    );
}

#[test]
fn consent_status_fix_expired_or_withdrawn_consent_does_not_block_work_core() {
    for withdraw in [false, true] {
        let mut configuration = config();
        let mut delivery_policy = policy();
        delivery_policy.id = "consent-delivery-only-policy".into();
        delivery_policy.expires_at_unix = Some(NOW + 100);
        configuration
            .policies
            .active_policies
            .push(delivery_policy.clone());
        let mut f = released_with(configuration, seed("a"), true, true);
        let mut source = record(
            "consent-delivery-context",
            f::Scope::Assets,
            "Optional delivery-only context.",
        );
        source.policy = delivery_policy.clone();
        source = source.seal().unwrap();
        f.app
            .import_files("operator", "a", vec![source.clone()], false, NOW)
            .unwrap();
        let prepared = f.prepare("select-consent-delivery-context");
        let mut response = decision(&prepared, true);
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Select delivery-only context.".into(),
            references: vec![reference(&source)],
        };
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let prepared = f.prepare("deliver-and-clear-consent-context");
        let mut response = decision(&prepared, true);
        response.next_assets = p::SelectionIntent::Clear {
            reason: "Delivery context no longer needed.".into(),
        };
        response.operations.push(effect(
            "deliver",
            WorkAction::DeliverRelease {
                release: "model-note-release".into(),
                recipients: set(&["human"]),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let core_before = f.app.snapshot().unwrap().work_meta["work"].exposure.clone();
        let input = request(&mut f, "shared-expiring-consent", true);
        let receipt = f
            .app
            .record_stakeholder_decision("human", input, NOW)
            .unwrap();
        assert_eq!(
            f.app
                .closure_readiness_evidence("a", "work", "model-note-release", NOW)
                .unwrap()["unestablished_stakeholder_acceptance"],
            json!([])
        );
        if withdraw {
            f.app
                .withdraw_source(
                    "operator",
                    "consent-delivery-context",
                    "Withdraw optional consent provenance.",
                )
                .unwrap();
        }
        let later = if withdraw { NOW } else { NOW + 100 };
        for actor in ["a", "b"] {
            let prepared = f
                .app
                .prepare(
                    PrepareRequest {
                        id: format!("core-after-consent-unavailable-{actor}"),
                        persona: actor.into(),
                        work: "work".into(),
                        profile: Profile::Synthetic,
                        mode: Mode::Ordinary,
                    },
                    later,
                )
                .expect("decision-only lineage must not block ordinary work core");
            assert!(!prepared.prompt.contains(&receipt.receipt.id));
        }
        let after = f.app.snapshot().unwrap();
        assert_eq!(after.work_meta["work"].exposure, core_before);
        assert_eq!(
            after
                .board
                .work(&"work".into())
                .unwrap()
                .stakeholder_decisions
                .len(),
            1
        );
        assert!(after
            .operator_events
            .iter()
            .any(|e| e["receipt"]["id"] == receipt.receipt.id));
        let projected = f
            .app
            .closure_readiness_evidence("a", "work", "model-note-release", later)
            .unwrap();
        assert_eq!(
            projected["unestablished_stakeholder_acceptance"],
            json!(["human"])
        );
        assert_eq!(projected["stakeholder_decisions"], json!([]));
    }
}

#[test]
fn consent_status_fix_completed_source_withdrawal_removes_current_support() {
    let mut f = released(config());
    deliver(&mut f);
    let input = request(&mut f, "accept-before-completion", true);
    f.app
        .record_stakeholder_decision("human", input, NOW)
        .unwrap();
    assert_eq!(
        close(&mut f, "close-before-source-withdrawal").status,
        AdoptionStatus::Adopted
    );
    let before = f.app.status().unwrap();
    assert_eq!(before["works"][0]["completion_supported"], true);
    let phase = before["works"][0]["phase"].clone();
    f.app
        .withdraw_source(
            "operator",
            "model-note",
            "Withdraw the completed artifact source.",
        )
        .unwrap();
    assert!(f
        .app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&"work".into()));
    let after = f.app.status().unwrap();
    assert_eq!(after["works"][0]["phase"], phase);
    assert_eq!(after["works"][0]["completion_supported"], false);
}

#[test]
fn review_v3_release_identity_contract_accepts_all_protocol_boundaries() {
    let mut failures = Vec::new();
    for release in [
        "r".repeat(128),
        "r".repeat(129),
        "r".repeat(256),
        "é".repeat(128),
    ] {
        let mut f = released(config());
        let prepared = f.prepare("create-boundary-release");
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "release-boundary",
            WorkAction::Release {
                id: release.clone(),
                shared: "model-note-shared".into(),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let prepared = f.prepare("deliver-boundary-release");
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "deliver-boundary",
            WorkAction::DeliverRelease {
                release: release.clone(),
                recipients: set(&["human"]),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        // Load the accepted historical identity through the ordinary recovery path.
        let root = f.app.state_root().to_owned();
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        if let Err(error) = f.app.closure_readiness_evidence("a", "work", &release, NOW) {
            failures.push(format!("{} bytes: inspection {error}", release.len()));
        }
        let mut input = request(&mut f, "boundary-human-decision", true);
        input.release = release.clone();
        let state = f.app.snapshot().unwrap();
        let work = state.board.work(&"work".into()).unwrap();
        input.release_sha256 = s::sha256_bytes(
            &serde_json::to_vec(
                &serde_json::to_value(&work.releases[&w::RecordId(release.clone())]).unwrap(),
            )
            .unwrap(),
        );
        match f.app.record_stakeholder_decision("human", input, NOW) {
            Ok(receipt) => assert_eq!(receipt.release, release),
            Err(error) => failures.push(format!("{} bytes: acceptance {error}", release.len())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn review_v3_release_identity_contract_rejects_unusable_new_ids() {
    let mut accepted = Vec::new();
    for release in [
        String::new(),
        "r".repeat(257),
        "a b".into(),
        "r\n".into(),
        "é".repeat(129),
    ] {
        let mut f = released(config());
        deliver(&mut f);
        let prepared = f.prepare("reject-invalid-release");
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "release-invalid",
            WorkAction::Release {
                id: release.clone(),
                shared: "model-note-shared".into(),
            },
            &[],
        ));
        let before = serde_json::to_value(&f.app.snapshot().unwrap().board).unwrap();
        if f.run(&prepared, &response).status == AdoptionStatus::Adopted {
            accepted.push(format!("{} bytes: {release:?}", release.len()));
        } else {
            assert_eq!(
                serde_json::to_value(&f.app.snapshot().unwrap().board).unwrap(),
                before
            );
        }
        // A known valid historical delivery supplies every unrelated binding.
        let mut input = request(&mut f, "invalid-human-decision", true);
        input.release = release.clone();
        // Identifier rejection must happen before nonexistent delivery lookup.
        assert!(f
            .app
            .record_stakeholder_decision("human", input, NOW)
            .is_err());
        assert!(f
            .app
            .closure_readiness_evidence("a", "work", &release, NOW)
            .is_err());
    }
    assert!(accepted.is_empty(), "{}", accepted.join("\n"));
}
