//! Fresh synthetic application fixtures; never a live provider or historical Store.
use super::*;
use std::os::unix::fs::PermissionsExt;

const NOW: i64 = 1_800_000_000;
const DEADLINE: u64 = (NOW + 100) as u64;
fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn policy(id: &str) -> f::Policy {
    f::Policy {
        id: id.into(),
        revision: "p1".into(),
        readers: set(&["a", "b", "c"]),
        processing_destinations: set(&["fixture"]),
        audience: set(&["a", "b", "c", "human"]),
        derivative_use: f::DerivativeUse::AllowDerivatives,
        expires_at_unix: None,
        revoked_at_unix: None,
    }
}
fn seed(id: &str) -> Seed {
    Seed {
        id: id.into(),
        label: id.into(),
        text: "Neutral synthetic seed.".into(),
        provenance: "Fresh synthetic fixture; no experience claim.".into(),
        requested_model: "synthetic-only".into(),
        requested_effort: "medium".into(),
        model_confirmed: false,
        destination: "fixture".into(),
        audience: set(&["a", "b", "c", "human"]),
        grants: Grants {
            fragments: true,
            assets: true,
            character: true,
            work_commands: true,
            write_policies: set(&["public", "message-policy"]),
            communicate_with: set(&["a", "b", "c", "human"]),
        },
    }
}
fn record(id: &str, scope: f::Scope, body: &str, policy_id: &str) -> f::FileRevision {
    record_with_policy(id, scope, body, policy(policy_id))
}
fn record_with_policy(id: &str, scope: f::Scope, body: &str, policy: f::Policy) -> f::FileRevision {
    f::FileRevision {
        source: id.into(),
        revision: String::new(),
        path: format!("{id}.txt"),
        scope,
        bytes: body.as_bytes().to_vec(),
        author: "operator:fixture".into(),
        policy,
        qualifications: Vec::new(),
        retired: false,
    }
    .seal()
    .unwrap()
}
fn exact(file: &f::FileRevision) -> p::ExactRef {
    p::ExactRef {
        source: file.source.clone(),
        revision: file.revision.clone(),
        representation: "utf8".into(),
        start: None,
        end: None,
        lifetime: Some("work".into()),
    }
}
fn need(id: &str) -> NeedInput {
    NeedInput {
        id: id.into(),
        origin: NeedOrigin::Human,
        request: "Consider an optional local contribution.".into(),
        boundaries: "Synthetic local task only.".into(),
        authorization: "Explicit fixture authorization.".into(),
        assumptions: "No external effects or verification claims.".into(),
        terms: "Voluntary bounded local work.".into(),
        policy_id: "public".into(),
        outcomes: BTreeMap::from([(
            "result".into(),
            OutcomeSpec {
                description: "An attributed result.".into(),
                criterion: "A described local result.".into(),
                required_reviews: BTreeSet::new(),
            },
        )]),
        review_roles: BTreeMap::new(),
        decision_rule: w::DecisionRule::Designated(w::PersonaId("a".into())),
        delivery_to: set(&["human"]),
        stakeholders: BTreeSet::new(),
        invitations: ["a", "b", "c"]
            .into_iter()
            .map(|actor| Invitation {
                id: format!("offer-{actor}"),
                to: actor.into(),
                terms: w::CommitmentTerms {
                    mandate_revision: 1,
                    exact_terms: "Optional local participation.".into(),
                    outcomes: if actor == "a" {
                        BTreeSet::from([w::OutcomeId("result".into())])
                    } else {
                        BTreeSet::new()
                    },
                    review_roles: BTreeSet::new(),
                },
            })
            .collect(),
    }
}
fn decision(prepared: &Prepared) -> p::Decision {
    p::Decision {
        version: 1,
        invocation_id: prepared.request.id.clone(),
        epoch: prepared.epoch,
        work: p::WorkDisposition::Ongoing {
            reason: "The work remains open.".into(),
            evidence: Vec::new(),
        },
        fragment_maintenance: p::Maintenance::NoChange {
            reason: "No justified change.".into(),
        },
        asset_maintenance: p::Maintenance::NoChange {
            reason: "No justified change.".into(),
        },
        next_fragments: p::SelectionIntent::Clear {
            reason: "No optional fragment selection is requested.".into(),
        },
        next_assets: p::SelectionIntent::Clear {
            reason: "No optional asset selection is requested.".into(),
        },
        continuation: p::Continuation::Immediate {
            purpose: "Continue within existing allowance.".into(),
        },
        reply: p::ReplyDisposition::None {
            reason: "No reply.".into(),
        },
        operations: Vec::new(),
    }
}
fn action(id: &str, value: WorkAction) -> p::Operation {
    p::Operation::WorkCommand {
        id: id.into(),
        inputs: serde_json::to_value(value).unwrap(),
        depends_on: Vec::new(),
    }
}
fn registration(prepared: &Prepared, id: &str, from: &str) -> p::Decision {
    let mut response = decision(prepared);
    response.operations.push(action(
        "register-wait",
        WorkAction::RegisterMessageWait {
            id: id.into(),
            from: from.into(),
        },
    ));
    response.continuation = p::Continuation::Wait {
        dependency_ids: vec![id.into()],
        deadline_unix: DEADLINE,
        reason: "Wait for one voluntary new message.".into(),
    };
    response
}
fn request(actor: &str, work: &str, id: &str) -> PrepareRequest {
    PrepareRequest {
        id: id.into(),
        persona: actor.into(),
        work: work.into(),
        profile: Profile::Synthetic,
        mode: Mode::Ordinary,
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    app: Runtime,
}
impl Fixture {
    fn new(accept_sender: bool) -> Self {
        Self::with_message_policy(accept_sender, policy("message-policy"))
    }
    fn with_message_policy(accept_sender: bool, message_policy: f::Policy) -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = Config::local(f::PolicySnapshot {
            active_policies: vec![policy("public"), message_policy],
            revoked_policies: BTreeSet::new(),
        });
        config.local_recipients = set(&["human"]);
        let mut app = Runtime::create(&temp.path().join("state"), config, "operator").unwrap();
        for actor in ["a", "b", "c"] {
            let ocean = [
                "openness",
                "conscientiousness",
                "extraversion",
                "agreeableness",
                "neuroticism",
            ]
            .into_iter()
            .map(|name| (name.into(), "Modeled neutral dimension.".into()))
            .collect();
            let vad = ["valence", "activation", "control"]
                .into_iter()
                .map(|name| (name.into(), "Modeled baseline.".into()))
                .collect();
            app.import_persona(
                "operator",
                PersonaImport {
                    seed: seed(actor),
                    ocean,
                    vad,
                    voice: record(
                        &format!("voice-{actor}"),
                        f::Scope::Fragments,
                        "I distinguish observation and inference.",
                        "public",
                    ),
                },
                NOW,
            )
            .unwrap();
        }
        app.add_need("operator", need("work")).unwrap();
        let mut fixture = Self { _temp: temp, app };
        fixture.accept("a", "accept-a");
        if accept_sender {
            fixture.accept("b", "accept-b");
        }
        fixture
    }
    fn prepare(&mut self, actor: &str, id: &str) -> Prepared {
        self.app.prepare(request(actor, "work", id), NOW).unwrap()
    }
    fn run(&mut self, prepared: &Prepared, response: &p::Decision) -> AdoptionReport {
        self.app
            .dispatch_synthetic(&prepared.request.id, NOW)
            .unwrap();
        let report = self
            .app
            .adopt_response(
                &prepared.request.id,
                &serde_json::to_vec(response).unwrap(),
                NOW,
            )
            .unwrap();
        if report.status != AdoptionStatus::Adopted {
            eprintln!("synthetic response {}: {report:?}", prepared.request.id);
        }
        report
    }
    fn accept(&mut self, actor: &str, id: &str) {
        let prepared = self.prepare(actor, id);
        let state = self.app.snapshot().unwrap();
        let current = state.board.work(&w::WorkId("work".into())).unwrap();
        let offer = format!("offer-{actor}");
        let mut response = decision(&prepared);
        response.operations.push(p::Operation::Responsibility {
            id: offer.clone(),
            work_id: "work".into(),
            terms_revision: offer_revision(current, &w::RecordId(offer)).unwrap(),
            disposition: "accept".into(),
            reason: "Accept these exact offered terms.".into(),
        });
        assert_eq!(
            self.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
    }
    fn arm(&mut self, id: &str) {
        let prepared = self.prepare("a", &format!("arm-{id}"));
        let before = self
            .app
            .snapshot()
            .unwrap()
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .version;
        assert_eq!(
            self.run(&prepared, &registration(&prepared, id, "b"))
                .status,
            AdoptionStatus::Adopted
        );
        let state = self.app.snapshot().unwrap();
        let after = state.board.work(&w::WorkId("work".into())).unwrap().version;
        assert_eq!(
            after.revision,
            before.revision + 1,
            "registration must not stale its own response"
        );
        assert_eq!(after.epoch, before.epoch);
    }
    fn send(&mut self, sender: &str, id: &str, body: &str, stop: bool) -> String {
        let prepared = self.prepare(sender, id);
        let mut response = decision(&prepared);
        response.operations.push(p::Operation::Communicate {
            id: "message".into(),
            recipient: "a".into(),
            subject: "Voluntary local context".into(),
            text: body.into(),
        });
        if stop {
            response.continuation = p::Continuation::Stop {
                reason: "I stop here.".into(),
            };
        }
        assert_eq!(
            self.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        self.app
            .snapshot()
            .unwrap()
            .messages
            .values()
            .find(|message| message.invocation == id)
            .unwrap()
            .id
            .clone()
    }
    fn assert_no_message_event(&mut self, id: &str) {
        let state = self.app.snapshot().unwrap();
        let wait = &state.work_meta["work"].message_waits[id];
        assert!(wait.message.is_none());
        assert!(state
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .events
            .iter()
            .all(|event| event.key != wait.key));
    }
}
fn opportunity(state: &State, now: i64) -> Option<w::Opportunity> {
    runtime_opportunity(
        state,
        &w::WorkId("work".into()),
        &w::PersonaId("a".into()),
        now as u64,
    )
}

#[test]
fn future_voluntary_reply_wakes_once_and_supplies_exact_body_through_normal_scheduler() {
    let mut fixture = Fixture::new(false);
    fixture.arm("next-message");
    let state = fixture.app.snapshot().unwrap();
    assert!(state
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .acceptance(&w::PersonaId("b".into()))
        .is_none());
    assert!(opportunity(&state, NOW).is_none());
    let id = fixture.send(
        "b",
        "voluntary-reply",
        "EXACT FRESH VOLUNTARY MESSAGE",
        true,
    );
    let state = fixture.app.snapshot().unwrap();
    let wait = state.work_meta["work"].message_waits["next-message"].clone();
    assert_eq!(wait.message.as_ref(), Some(&id));
    assert!(matches!(
        opportunity(&state, NOW).unwrap().cause,
        w::OpportunityCause::Event(_)
    ));
    assert!(!state.personas["b"].focus["work"].ordinary_continuation);
    let scheduled = fixture
        .app
        .scheduler_prepare(
            SchedulerPrepareRequest {
                invocation_id: "scheduled-recipient".into(),
                max_considerations: 128,
            },
            NOW,
        )
        .unwrap();
    let prepared = scheduled.prepared.unwrap();
    assert_eq!(prepared.request.persona, "a");
    assert!(prepared.supplied_observations.contains(&id));
    assert!(prepared.prompt.contains("EXACT FRESH VOLUNTARY MESSAGE"));
    let mut response = decision(&prepared);
    response.continuation = p::Continuation::Wait {
        dependency_ids: vec!["next-message".into()],
        deadline_unix: DEADLINE,
        reason: "Keep the independently authored deadline.".into(),
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = fixture.app.snapshot().unwrap();
    assert!(state
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .participants[&w::PersonaId("a".into())]
        .observed_events
        .contains(&wait.key));
    assert!(
        opportunity(&state, NOW).is_none(),
        "same message cannot wake twice"
    );
    assert!(matches!(
        opportunity(&state, DEADLINE as i64).unwrap().cause,
        w::OpportunityCause::Deadline
    ));
}

#[test]
fn pending_wrong_sender_and_other_work_messages_never_satisfy_a_new_wait() {
    let mut fixture = Fixture::new(true);
    fixture.send("b", "old-message", "OLD MESSAGE", false);
    fixture.arm("future-only");
    fixture.assert_no_message_event("future-only");
    fixture.send("c", "other-sender", "OTHER SENDER", true);
    fixture.assert_no_message_event("future-only");
    fixture
        .app
        .add_need("operator", need("other-work"))
        .unwrap();
    let prepared = fixture
        .app
        .prepare(request("b", "other-work", "other-work-message"), NOW)
        .unwrap();
    let mut response = decision(&prepared);
    response.operations.push(p::Operation::Communicate {
        id: "message".into(),
        recipient: "a".into(),
        subject: "Other work".into(),
        text: "OTHER WORK MESSAGE".into(),
    });
    response.continuation = p::Continuation::Stop {
        reason: "No acceptance inferred.".into(),
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    fixture.assert_no_message_event("future-only");
    assert!(opportunity(&fixture.app.snapshot().unwrap(), NOW).is_none());
}

#[test]
fn stop_pause_and_replaced_wait_do_not_reactivate_but_ordinary_messages_survive_resume() {
    for stop in [true, false] {
        let mut fixture = Fixture::new(true);
        fixture.arm("old-wait");
        let before_disposition = fixture.app.snapshot().unwrap();
        let retained_acceptance = before_disposition
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .acceptance(&w::PersonaId("a".into()))
            .map(|(id, acceptance)| (id.clone(), acceptance.clone()))
            .unwrap();
        fixture.app.resume("operator", "a", "work").unwrap();
        let prepared = fixture.prepare("a", "authored-disposition");
        let mut response = decision(&prepared);
        response.continuation = if stop {
            p::Continuation::Stop {
                reason: "Stop voluntarily.".into(),
            }
        } else {
            p::Continuation::Pause {
                reason: "Pause voluntarily.".into(),
            }
        };
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let id = fixture.send("b", "after-disposition", "PERMITTED QUEUED MESSAGE", true);
        let state = fixture.app.snapshot().unwrap();
        assert!(!state.work_meta["work"].message_waits["old-wait"].active);
        let current = state.board.work(&w::WorkId("work".into())).unwrap();
        assert_eq!(
            current
                .acceptance(&w::PersonaId("a".into()))
                .map(|(id, acceptance)| (id.clone(), acceptance.clone())),
            Some(retained_acceptance),
            "Stop and Pause retain the exact accepted responsibility"
        );
        assert_eq!(
            current.ownership(&w::OutcomeId("result".into())),
            w::Accountability::Owner(w::PersonaId("a".into()))
        );
        assert!(opportunity(&state, NOW).is_none());
        fixture.app.resume("operator", "a", "work").unwrap();
        let prepared = fixture.prepare("a", "explicit-later-resume");
        assert!(prepared.supplied_observations.contains(&id));
        assert!(prepared.prompt.contains("PERMITTED QUEUED MESSAGE"));
        let response = registration(&prepared, "old-wait", "b");
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Held,
            "old identity cannot rearm"
        );
    }
    let mut fixture = Fixture::new(true);
    fixture.arm("first");
    fixture.app.resume("operator", "a", "work").unwrap();
    fixture.arm("second");
    let id = fixture.send("b", "replacement-message", "ONLY CURRENT WAIT", true);
    let state = fixture.app.snapshot().unwrap();
    assert!(!state.work_meta["work"].message_waits["first"].active);
    assert_eq!(
        state.work_meta["work"].message_waits["second"]
            .message
            .as_ref(),
        Some(&id)
    );

    // Offering the current commitment does not transfer it. Only the recipient's
    // exact acceptance ends the sender's participation and cancels its old wait.
    let mut fixture = Fixture::new(true);
    let prepared = fixture.prepare("a", "wait-and-offer-handoff");
    let mut response = registration(&prepared, "handoff-wait", "b");
    response.operations.push(action(
        "offer-transfer",
        WorkAction::Handoff {
            offer: "transfer-to-c".into(),
            to: "c".into(),
        },
    ));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = fixture.app.snapshot().unwrap();
    let current = state.board.work(&w::WorkId("work".into())).unwrap();
    let terms = current
        .acceptance(&w::PersonaId("a".into()))
        .unwrap()
        .1
        .terms
        .clone();
    assert!(current.acceptance(&w::PersonaId("c".into())).is_none());
    assert!(state.work_meta["work"].message_waits["handoff-wait"].active);
    assert_eq!(
        current.ownership(&w::OutcomeId("result".into())),
        w::Accountability::Owner(w::PersonaId("a".into()))
    );
    let terms_revision = offer_revision(current, &w::RecordId("transfer-to-c".into())).unwrap();
    let prepared = fixture.prepare("c", "accept-exact-handoff");
    let mut response = decision(&prepared);
    response.operations.push(p::Operation::Responsibility {
        id: "transfer-to-c".into(),
        work_id: "work".into(),
        terms_revision,
        disposition: "accept".into(),
        reason: "Accept the exact offered commitment.".into(),
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    fixture.send(
        "b",
        "after-accepted-handoff",
        "MESSAGE AFTER TRANSFER",
        true,
    );
    let state = fixture.app.snapshot().unwrap();
    let current = state.board.work(&w::WorkId("work".into())).unwrap();
    assert!(current.acceptance(&w::PersonaId("a".into())).is_none());
    let (accepted_id, accepted) = current.acceptance(&w::PersonaId("c".into())).unwrap();
    assert_eq!(accepted_id, &w::RecordId("transfer-to-c".into()));
    assert_eq!(accepted.terms, terms);
    assert_eq!(
        current.ownership(&w::OutcomeId("result".into())),
        w::Accountability::Owner(w::PersonaId("c".into()))
    );
    assert!(!state.work_meta["work"].message_waits["handoff-wait"].active);
    assert!(state.work_meta["work"].message_waits["handoff-wait"]
        .message
        .is_none());
    assert!(!state.personas["a"].focus["work"].permit);
    assert!(!state.personas["a"].focus["work"].ordinary_continuation);
    assert!(opportunity(&state, NOW).is_none());
    assert!(opportunity(&state, DEADLINE as i64).is_none());
    fixture.assert_no_message_event("handoff-wait");
    assert!(fixture
        .app
        .prepare(request("a", "work", "no-revival-after-transfer"), NOW)
        .is_err());
}

#[test]
fn registration_failures_roll_back_board_files_and_message_side_effects() {
    for case in [
        "stop",
        "pause",
        "mismatch",
        "two",
        "self",
        "invalid-id",
        "unaccepted",
    ] {
        let mut fixture = Fixture::new(true);
        let actor = if case == "unaccepted" { "c" } else { "a" };
        let prepared = fixture.prepare(actor, &format!("invalid-{case}"));
        let mut response = registration(&prepared, "new-wait", "b");
        match case {
            "stop" => {
                response.continuation = p::Continuation::Stop {
                    reason: "Stop.".into(),
                }
            }
            "pause" => {
                response.continuation = p::Continuation::Pause {
                    reason: "Pause.".into(),
                }
            }
            "mismatch" => {
                response.continuation = p::Continuation::Wait {
                    dependency_ids: vec!["other".into()],
                    deadline_unix: DEADLINE,
                    reason: "Different wait.".into(),
                }
            }
            "two" => response.operations.push(action(
                "another",
                WorkAction::RegisterMessageWait {
                    id: "new-wait".into(),
                    from: "b".into(),
                },
            )),
            "self" => response = registration(&prepared, "new-wait", actor),
            "invalid-id" => {
                response = registration(&prepared, "not an id", "b");
                response.continuation = p::Continuation::Wait {
                    dependency_ids: vec!["new-wait".into()],
                    deadline_unix: DEADLINE,
                    reason: "Valid outer protocol; invalid capability identity.".into(),
                };
            }
            _ => (),
        }
        response.operations.push(p::Operation::FileWrite {
            id: "tentative-write".into(),
            scope: p::Scope::Assets {},
            source: "must-not-exist".into(),
            path: "must-not-exist.txt".into(),
            base_revision: None,
            text: "Tentative side effect.".into(),
            qualifications: Vec::new(),
            policy_id: "public".into(),
        });
        response.asset_maintenance = p::Maintenance::Change {
            reason: "Tentative save.".into(),
            operation_ids: vec!["tentative-write".into()],
        };
        response.operations.push(p::Operation::Communicate {
            id: "tentative-message".into(),
            recipient: "b".into(),
            subject: "Must roll back".into(),
            text: "TENTATIVE MESSAGE".into(),
        });
        let before = fixture.app.snapshot().unwrap();
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Held,
            "{case}"
        );
        let after = fixture.app.snapshot().unwrap();
        assert_eq!(
            serde_json::to_value(&before.board).unwrap(),
            serde_json::to_value(&after.board).unwrap(),
            "{case}"
        );
        assert_eq!(
            serde_json::to_value(&before.messages).unwrap(),
            serde_json::to_value(&after.messages).unwrap(),
            "{case}"
        );
        assert!(after.work_meta["work"].message_waits.is_empty());
        assert!(!after.sources.contains_key("must-not-exist"));
    }
}

#[test]
fn same_response_decline_cannot_arm_and_grant_is_required() {
    let mut fixture = Fixture::new(true);
    let state = fixture.app.snapshot().unwrap();
    let current = state.board.work(&w::WorkId("work".into())).unwrap();
    let mut cloned = state.clone();
    cloned
        .personas
        .get_mut("a")
        .unwrap()
        .seed
        .grants
        .work_commands = false;
    let prepared = fixture.prepare("a", "without-capability");
    let response = registration(&prepared, "ungranted", "b");
    let allowed = response
        .operations
        .iter()
        .map(|operation| operation.id().into())
        .collect();
    assert!(validate_operation_interfaces(&cloned, &prepared, &response, &allowed).is_err());
    // A real fresh invitation is used for the decline/registration rollback case.
    let mut response = decision(&prepared);
    response.operations.push(action(
        "invite-self",
        WorkAction::Invite {
            offer: "another-offer".into(),
            to: "a".into(),
            terms: Commitment {
                mandate_revision: current.current_mandate,
                exact_terms: "Optional separate terms.".into(),
                outcomes: BTreeSet::new(),
                review_roles: BTreeSet::new(),
            },
        },
    ));
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("a", "decline-and-wait");
    let current = fixture.app.snapshot().unwrap();
    let mut response = registration(&prepared, "decline-wait", "b");
    response.operations.push(p::Operation::Responsibility {
        id: "another-offer".into(),
        work_id: "work".into(),
        terms_revision: offer_revision(
            current.board.work(&w::WorkId("work".into())).unwrap(),
            &w::RecordId("another-offer".into()),
        )
        .unwrap(),
        disposition: "decline".into(),
        reason: "Decline new terms.".into(),
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Held
    );
    let state = fixture.app.snapshot().unwrap();
    assert!(state.work_meta["work"].message_waits.is_empty());
    assert!(matches!(
        state.board.work(&w::WorkId("work".into())).unwrap().offers
            [&w::RecordId("another-offer".into())]
            .status,
        w::OfferStatus::Pending
    ));
}

#[test]
fn revoked_sender_permission_blocks_only_event_not_deadline_or_explicit_resume() {
    let mut fixture = Fixture::new(true);
    fixture.arm("permission-wait");
    let id = fixture.send(
        "b",
        "permission-message",
        "NORMAL MESSAGE REMAINS READABLE",
        true,
    );
    let mut state = fixture.app.snapshot().unwrap();
    // No public grant-revocation API exists. Exercise the pure boundary on this fresh clone only.
    state
        .personas
        .get_mut("b")
        .unwrap()
        .seed
        .grants
        .communicate_with
        .remove("a");
    assert!(opportunity(&state, NOW).is_none());
    assert!(matches!(
        opportunity(&state, DEADLINE as i64).unwrap().cause,
        w::OpportunityCause::Deadline
    ));
    let rendered = render(
        &state,
        "a",
        "work",
        "deadline-view",
        1,
        DEADLINE as i64,
        false,
        None,
    )
    .unwrap();
    assert!(rendered.observations.contains(&id));
    assert!(rendered.prompt.contains("NORMAL MESSAGE REMAINS READABLE"));
    let current = state.board.work(&w::WorkId("work".into())).unwrap();
    let context = w::RuntimeContext::from_runtime(
        w::WorkId("work".into()),
        current.version,
        current.mandate().authority.clone(),
        w::Origin::Human("operator".into()),
        w::RuntimeRight::Governance,
    );
    state
        .board
        .observe(
            &context,
            w::RuntimeOperation::ResumeParticipant {
                persona: w::PersonaId("a".into()),
            },
        )
        .unwrap();
    assert!(matches!(
        opportunity(&state, NOW).unwrap().cause,
        w::OpportunityCause::ExplicitResume
    ));
}

#[test]
fn full_message_exposure_is_rechecked_after_policy_revocation_or_expiry() {
    for expires in [false, true] {
        let mut message_policy = policy("message-policy");
        if expires {
            message_policy.expires_at_unix = Some(NOW + 50);
        }
        let mut fixture = Fixture::with_message_policy(true, message_policy.clone());
        fixture.arm("protected-message");
        let asset = record_with_policy(
            "sender-private-context",
            f::Scope::Assets,
            "PRIVATE MESSAGE INPUT",
            message_policy.clone(),
        );
        fixture
            .app
            .import_files("operator", "b", vec![asset.clone()], false, NOW)
            .unwrap();
        let prepared = fixture.prepare("b", "select-private-context");
        let mut response = decision(&prepared);
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Use exact private context.".into(),
            references: vec![exact(&asset)],
        };
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let id = fixture.send("b", "protected-reply", "PROTECTED DERIVED MESSAGE", true);
        let state = fixture.app.snapshot().unwrap();
        assert!(state.messages[&id]
            .exposure
            .as_ref()
            .unwrap()
            .sources
            .iter()
            .any(|source| source.source == asset.source));
        assert!(matches!(
            opportunity(&state, NOW).unwrap().cause,
            w::OpportunityCause::Event(_)
        ));
        let denied_at = if expires {
            NOW + 51
        } else {
            fixture
                .app
                .revoke_policy(
                    "operator",
                    message_policy.identity(),
                    "Explicit synthetic revocation",
                )
                .unwrap();
            NOW
        };
        let state = fixture.app.snapshot().unwrap();
        assert!(opportunity(&state, denied_at).is_none());
        assert!(fixture
            .app
            .prepare(request("a", "work", "denied-event"), denied_at)
            .is_err());
        assert!(matches!(
            opportunity(&state, DEADLINE as i64).unwrap().cause,
            w::OpportunityCause::Deadline
        ));
        let prepared = fixture
            .app
            .prepare(request("a", "work", "authored-deadline"), DEADLINE as i64)
            .unwrap();
        assert!(!prepared.supplied_observations.contains(&id));
        assert!(!prepared.prompt.contains("PROTECTED DERIVED MESSAGE"));
        assert!(!prepared.prompt.contains("PRIVATE MESSAGE INPUT"));
    }
}

#[test]
fn wait_scope_capacity_and_stale_response_fences_are_retained() {
    let mut fixture = Fixture::new(true);
    fixture.arm("bounded-wait");
    let state = fixture.app.snapshot().unwrap();
    let wait = state.work_meta["work"].message_waits["bounded-wait"].clone();
    let mut clone = state.clone();
    clone
        .work_meta
        .get_mut("work")
        .unwrap()
        .message_waits
        .clear();
    for index in 0..128 {
        let mut entry = wait.clone();
        entry.id = format!("bounded-{index}");
        clone
            .work_meta
            .get_mut("work")
            .unwrap()
            .message_waits
            .insert(entry.id.clone(), entry);
    }
    assert!(check_local_dependency_capacity(&clone, "work").is_err());
    let mut obsolete = wait.clone();
    obsolete.key.epoch += 1;
    assert!(!message_wait_visible(&state, "work", &obsolete, "a", NOW));
    let mut other_authority = wait;
    other_authority.authority = w::ExactRef::new("other-authority", 1, b"other");
    assert!(!message_wait_visible(
        &state,
        "work",
        &other_authority,
        "a",
        NOW
    ));
    fixture.app.resume("operator", "a", "work").unwrap();
    let prepared = fixture.prepare("a", "stale-wait-response");
    fixture
        .app
        .dispatch_synthetic(&prepared.request.id, NOW)
        .unwrap();
    fixture
        .app
        .pause("operator", "work", "Changed after dispatch")
        .unwrap();
    let stale = fixture
        .app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&registration(&prepared, "stale-new", "b")).unwrap(),
            NOW,
        )
        .unwrap();
    assert_eq!(stale.status, AdoptionStatus::Held);
    assert!(!fixture.app.snapshot().unwrap().work_meta["work"]
        .message_waits
        .contains_key("stale-new"));
}
