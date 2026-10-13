use super::*;

fn fixture_with_peers(
    configuration: Config,
    primary: Seed,
    peer: Seed,
    original: NeedInput,
) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut app = Runtime::create(&temp.path().join("state"), configuration, "operator").unwrap();
    for persona in [primary, peer] {
        let (ocean, vad) = profiles();
        let voice = record(
            &format!("voice-{}", persona.id),
            f::Scope::Fragments,
            "I distinguish observed evidence from inference.",
        );
        app.import_persona(
            "operator",
            PersonaImport {
                seed: persona,
                ocean,
                vad,
                voice,
            },
            NOW,
        )
        .unwrap();
    }
    app.add_need("operator", original).unwrap();
    Fixture { _temp: temp, app }
}

fn peer_request(id: &str) -> PrepareRequest {
    PrepareRequest {
        id: id.into(),
        persona: "b".into(),
        work: "work".into(),
        profile: Profile::Synthetic,
        mode: Mode::Ordinary,
    }
}

fn invite(id: &str) -> p::Operation {
    effect(
        id,
        WorkAction::Invite {
            offer: id.into(),
            to: "b".into(),
            terms: Commitment {
                mandate_revision: 1,
                exact_terms: "Consider an optional contribution without exclusive ownership."
                    .into(),
                outcomes: BTreeSet::new(),
                review_roles: BTreeSet::new(),
            },
        },
        &[],
    )
}

fn message(id: &str, recipient: &str) -> p::Operation {
    p::Operation::Communicate {
        id: id.into(),
        recipient: recipient.into(),
        subject: "Local context only".into(),
        text: "A message does not accept work or authorize a wake.".into(),
    }
}

fn accept_peer(fixture: &mut Fixture, id: &str) {
    let prepared = fixture.app.prepare(peer_request(id), NOW).unwrap();
    let mut response = decision(&prepared, false);
    let core = interface_control(&prepared);
    let offer = &core["own_pending_offers"][0];
    response.operations.push(p::Operation::Responsibility {
        id: offer["id"].as_str().unwrap().into(),
        work_id: "work".into(),
        terms_revision: offer["terms_revision"].as_str().unwrap().into(),
        disposition: "accept".into(),
        reason: "I explicitly accept these offered terms.".into(),
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn message_before_first_invite_gets_one_admission_without_acceptance_or_new_allowance() {
    let mut fixture = fixture_with_peers(config(), seed("a"), seed("b"), need());
    fixture.accept();
    let prepared = fixture.prepare("message-before-invite");
    let mut response = decision(&prepared, true);
    response.operations.push(message("context-for-b", "b"));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = fixture.app.snapshot().unwrap();
    let focus = &state.personas["b"].focus["work"];
    assert!(!focus.permit && !focus.ordinary_continuation);
    assert_eq!(focus.pending_observations.len(), 1);
    assert!(!state
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .participants
        .contains_key(&w::PersonaId("b".into())));
    assert!(fixture
        .app
        .prepare(peer_request("message-is-not-admission"), NOW)
        .is_err());

    let prepared = fixture.prepare("first-invite");
    let before = persisted_state(&fixture.app);
    let mut response = decision(&prepared, true);
    response.operations.push(invite("offer-b"));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = fixture.app.snapshot().unwrap();
    assert!(state.personas["b"].focus["work"].permit);
    assert!(!state.personas["b"].focus["work"].ordinary_continuation);
    assert_eq!(
        state.personas["b"].focus["work"].pending_observations.len(),
        1
    );
    assert!(state
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .acceptance(&w::PersonaId("b".into()))
        .is_none());
    let after = persisted_state(&fixture.app);
    for field in ["attempts_left", "navigation_left", "repair_attempts_left"] {
        assert_eq!(
            after["work_meta"]["work"][field],
            before["work_meta"]["work"][field]
        );
    }
    assert_eq!(
        after["personas"]["b"]["admission_serial"],
        before["personas"]["b"]["admission_serial"]
    );
    let prepared = fixture
        .app
        .prepare(peer_request("consider-first-offer"), NOW)
        .unwrap();
    assert!(prepared.prompt.contains("A message does not accept work"));
    let mut response = decision(&prepared, false);
    let core = interface_control(&prepared);
    response.operations.push(p::Operation::Responsibility {
        id: "offer-b".into(),
        work_id: "work".into(),
        terms_revision: core["own_pending_offers"][0]["terms_revision"]
            .as_str()
            .unwrap()
            .into(),
        disposition: "accept".into(),
        reason: "I explicitly accept this exact invitation.".into(),
    });
    response.continuation = p::Continuation::Stop {
        reason: "Retain the accepted responsibility without another wake.".into(),
    };
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    assert!(fixture
        .app
        .snapshot()
        .unwrap()
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .acceptance(&w::PersonaId("b".into()))
        .is_some());
    assert!(fixture
        .app
        .prepare(peer_request("no-automatic-second-admission"), NOW)
        .is_err());
}

#[test]
fn later_invites_and_messages_preserve_stop_pause_character_fence_and_hold() {
    for mode in ["stop", "pause", "character", "held"] {
        let mut fixture = if mode == "character" {
            Fixture::configured(true, config(), seed("a"), interface_peer_need(false))
        } else {
            fixture_with_peers(config(), seed("a"), seed("b"), interface_peer_need(false))
        };
        fixture.accept();
        if mode == "character" {
            // Initialization is fenced until explicit operator resume.
            let prepared = fixture
                .app
                .prepare(peer_request("character-fence"), NOW)
                .unwrap();
            let (ocean, vad) = profiles();
            let mut response = decision(&prepared, false);
            response.operations = vec![
                write(
                    "save-voice",
                    p::Scope::Fragments {},
                    "peer-voice",
                    "I retain explicit authority.",
                    None,
                ),
                p::Operation::CharacterAdopt {
                    id: "adopt-voice".into(),
                    base_revision: None,
                    ocean,
                    vad,
                    own_voice: vec![proposed("peer-voice", "save-voice")],
                    reason: "Initialize this character.".into(),
                    evidence: Vec::new(),
                },
            ];
            response.fragment_maintenance = p::Maintenance::Change {
                reason: "Retain this initial character.".into(),
                operation_ids: vec!["save-voice".into(), "adopt-voice".into()],
            };
            assert_eq!(
                fixture.run(&prepared, &response).status,
                AdoptionStatus::CharacterOnly
            );
        } else {
            accept_peer(&mut fixture, "accept-b");
            let prepared = fixture
                .app
                .prepare(peer_request("peer-disposition"), NOW)
                .unwrap();
            let mut response = decision(&prepared, true);
            response.continuation = if mode == "stop" {
                p::Continuation::Stop {
                    reason: "Explicit local stop.".into(),
                }
            } else {
                p::Continuation::Pause {
                    reason: "Explicit local pause.".into(),
                }
            };
            if mode == "held" {
                response.operations.push(p::Operation::Effect {
                    id: "unknown-command".into(),
                    capability_id: "unknown.capability".into(),
                    binding_revision: "unknown-binding".into(),
                    inputs: Value::Null,
                    depends_on: Vec::new(),
                });
            }
            let expected = if mode == "held" {
                AdoptionStatus::Held
            } else {
                AdoptionStatus::Adopted
            };
            assert_eq!(fixture.run(&prepared, &response).status, expected);
        }
        let before = persisted_state(&fixture.app);
        let prepared = fixture.prepare("reinvite-existing-peer");
        let mut response = decision(&prepared, true);
        response.operations = vec![invite("another-offer-b"), message("later-context", "b")];
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let after = persisted_state(&fixture.app);
        assert_eq!(
            after["personas"]["b"]["active_invocation"],
            before["personas"]["b"]["active_invocation"]
        );
        assert_eq!(after["personas"]["b"]["focus"]["work"]["permit"], false);
        for field in [
            "ordinary_continuation",
            "continuation",
            "last_judgment",
            "selection_fit",
        ] {
            assert_eq!(
                after["personas"]["b"]["focus"]["work"][field],
                before["personas"]["b"]["focus"]["work"][field],
                "{mode}: {field}"
            );
        }
        assert!(
            fixture
                .app
                .prepare(peer_request("must-not-wake"), NOW)
                .is_err(),
            "{mode}"
        );
    }
}

#[test]
fn rejected_first_invite_is_atomic_and_does_not_wake_passive_inbox() {
    let mut fixture = fixture_with_peers(config(), seed("a"), seed("b"), need());
    fixture.accept();
    let prepared = fixture.prepare("message-first");
    let mut response = decision(&prepared, true);
    response.operations.push(message("context", "b"));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("invalid-invite");
    let mut response = decision(&prepared, true);
    response.operations = vec![invite("duplicate-offer"), invite("duplicate-offer-second")];
    if let p::Operation::Effect { inputs, .. } = &mut response.operations[1] {
        inputs["offer"] = json!("duplicate-offer");
    }
    let before = persisted_state(&fixture.app);
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.applied.is_empty());
    assert_eq!(persisted_state(&fixture.app), before);
    assert!(fixture
        .app
        .prepare(peer_request("no-phantom-invite"), NOW)
        .is_err());
}

fn restricted_collaboration_fixture(peer_blocked: bool) -> (Fixture, f::Policy) {
    let mut private = policy();
    private.id = "a-only-policy".into();
    private.revision = "a-only-1".into();
    private.readers = set(&["a"]);
    let mut configuration = config();
    configuration.policies.active_policies.push(private.clone());
    let mut primary = seed("a");
    primary.grants.write_policies.insert(private.id.clone());
    let mut original = interface_peer_need(false);
    if peer_blocked {
        original.policy_id = private.id.clone();
    }
    let mut fixture = fixture_with_peers(configuration, primary, seed("b"), original);
    fixture.accept();
    let mut asset = record(
        "private-context",
        f::Scope::Assets,
        "PRIVATE AUTHOR CONTEXT",
    );
    asset.policy = private.clone();
    asset = asset.seal().unwrap();
    fixture
        .app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = fixture.prepare("select-private-context");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select a permitted exact source for the next invocation.".into(),
        references: vec![reference(&asset)],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    (fixture, private)
}

#[test]
fn shared_command_cannot_remove_previously_permitted_participant_core_access() {
    let (mut fixture, private) = restricted_collaboration_fixture(false);
    let prepared = fixture.prepare("restricted-shared-command");
    assert!(prepared.prompt.contains("PRIVATE AUTHOR CONTEXT"));
    let mut response = decision(&prepared, true);
    response.operations = vec![
        invite("new-offer-b"),
        write(
            "private-write",
            p::Scope::Assets {},
            "unaccepted-private-result",
            "Candidate only.",
            None,
        ),
        message("candidate-message", "human"),
    ];
    if let p::Operation::FileWrite { policy_id, .. } = &mut response.operations[1] {
        *policy_id = private.id;
    }
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Account for the candidate asset.".into(),
        operation_ids: vec!["private-write".into()],
    };
    let before = persisted_state(&fixture.app);
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report
        .reason
        .as_deref()
        .unwrap()
        .contains("existing participant's work core access"));
    assert!(report.applied.is_empty());
    assert_eq!(persisted_state(&fixture.app), before);
    assert!(fixture.app.adopted(&prepared.request.id).is_err());
    let peer = fixture
        .app
        .prepare(peer_request("peer-core-still-readable"), NOW)
        .unwrap();
    assert!(!peer.prompt.contains("PRIVATE AUTHOR CONTEXT"));
    assert!(!peer.prompt.contains("new-offer-b"));
}

#[test]
fn already_policy_blocked_participant_does_not_force_declassification() {
    let (mut fixture, private) = restricted_collaboration_fixture(true);
    assert!(fixture
        .app
        .prepare(peer_request("already-blocked"), NOW)
        .is_err());
    let prepared = fixture.prepare("permitted-private-command");
    let mut response = decision(&prepared, true);
    response.operations.push(invite("new-private-offer-b"));
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let state = fixture.app.snapshot().unwrap();
    let exposure = state.work_meta["work"].exposure.as_ref().unwrap();
    assert!(exposure
        .sources
        .iter()
        .any(|source| source.source == "private-context"));
    assert!(exposure
        .policies
        .iter()
        .any(|restriction| restriction.policy.id == private.id));
    assert!(fixture
        .app
        .prepare(peer_request("still-policy-blocked"), NOW)
        .is_err());
}

#[test]
fn current_policy_revocation_still_blocks_shared_commands_atomically() {
    let (mut fixture, private) = restricted_collaboration_fixture(false);
    let prepared = fixture.prepare("revoked-shared-command");
    let mut response = decision(&prepared, true);
    response.operations.push(invite("revoked-offer-b"));
    fixture
        .app
        .dispatch_synthetic(&prepared.request.id, NOW)
        .unwrap();
    fixture
        .app
        .revoke_policy(
            "operator",
            private.identity(),
            "Current revocation remains authoritative.",
        )
        .unwrap();
    let before = persisted_state(&fixture.app);
    let report = fixture
        .app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&response).unwrap(),
            NOW,
        )
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&fixture.app), before);
    assert!(!fixture
        .app
        .snapshot()
        .unwrap()
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .offers
        .contains_key(&w::RecordId("revoked-offer-b".into())));
}

#[test]
fn mutually_permitted_shared_exposure_remains_available_to_pending_participant() {
    let mut fixture =
        fixture_with_peers(config(), seed("a"), seed("b"), interface_peer_need(false));
    fixture.accept();
    let asset = record(
        "shared-context",
        f::Scope::Assets,
        "SHARED COLLABORATION CONTEXT",
    );
    fixture
        .app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = fixture.prepare("select-shared-context");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select mutually permitted material.".into(),
        references: vec![reference(&asset)],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("shared-command");
    let mut response = decision(&prepared, true);
    response.operations.push(invite("mutually-permitted-offer"));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let peer = fixture
        .app
        .prepare(peer_request("permitted-peer-core"), NOW)
        .unwrap();
    assert!(peer
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .iter()
        .any(|source| source.source == asset.source));
    assert!(interface_control(&peer)["own_acceptance"].is_null());
}

#[test]
fn first_invite_requires_authorized_work_commands_even_with_a_passive_inbox() {
    let mut primary = seed("a");
    primary.grants.work_commands = false;
    let mut fixture = fixture_with_peers(config(), primary, seed("b"), need());
    fixture.accept();
    let prepared = fixture.prepare("authorized-message");
    let mut response = decision(&prepared, true);
    response.operations.push(message("passive-context", "b"));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("unauthorized-invitation");
    let mut response = decision(&prepared, true);
    response.operations.push(invite("unauthorized-offer"));
    let before = persisted_state(&fixture.app);
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.applied.is_empty());
    assert_eq!(persisted_state(&fixture.app), before);
    assert!(fixture
        .app
        .prepare(peer_request("no-unauthorized-wake"), NOW)
        .is_err());
}
