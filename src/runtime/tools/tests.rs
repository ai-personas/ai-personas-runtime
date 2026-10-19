use super::*;

const NOW: i64 = 100;
const PROGRAM: &str = "(module (memory (export \"memory\") 1) (func (export \"run\") (param i32 i32) (result i64) local.get 1 i64.extend_i32_u))";

fn policy() -> f::Policy {
    serde_json::from_value(json!({"id":"fixture-policy","revision":"one","readers":["persona"],
        "processing_destinations":["local"],"audience":["operator"],"derivative_use":"allow_derivatives",
        "expires_at_unix":null,"revoked_at_unix":null})).unwrap()
}
fn file(source: &str, text: &str, scope: f::Scope) -> f::FileRevision {
    f::FileRevision {
        source: source.into(),
        revision: String::new(),
        path: format!("{source}.txt"),
        scope,
        bytes: text.as_bytes().to_vec(),
        author: "operator".into(),
        policy: policy(),
        qualifications: Vec::new(),
        retired: false,
    }
    .seal()
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, Runtime, p::ExactRef, p::ExactRef) {
    let tmp = tempfile::tempdir().unwrap();
    let config = Config::local(f::PolicySnapshot {
        active_policies: vec![policy()],
        revoked_policies: BTreeSet::new(),
    });
    let mut app = Runtime::create(&tmp.path().join("state"), config, "operator").unwrap();
    let seed: Seed = serde_json::from_value(json!({"id":"persona","label":"Fixture","text":"Fixture identity",
        "provenance":"operator test fixture","requested_model":"fixture","requested_effort":"high","model_confirmed":false,
        "destination":"local","audience":["operator"],"grants":{"fragments":true,"assets":true,"character":true,
            "work_commands":true,"write_policies":["fixture-policy"],"communicate_with":[]}})).unwrap();
    let map = |keys: &[&str]| {
        keys.iter()
            .map(|s| (s.to_string(), "fixture disposition".to_owned()))
            .collect()
    };
    app.import_persona(
        "operator",
        PersonaImport {
            seed,
            ocean: map(&[
                "openness",
                "conscientiousness",
                "extraversion",
                "agreeableness",
                "neuroticism",
            ]),
            vad: map(&["valence", "activation", "control"]),
            voice: file(
                "voice",
                "I check observed results before drawing conclusions.",
                f::Scope::Fragments,
            ),
        },
        NOW,
    )
    .unwrap();
    app.add_need(
        "operator",
        NeedInput {
            id: "work".into(),
            origin: NeedOrigin::Human,
            request: "Exercise a computation".into(),
            boundaries: "Local computation only".into(),
            authorization: "Explicit fixture".into(),
            assumptions: "No outside evidence".into(),
            terms: "Check returned bytes".into(),
            policy_id: "fixture-policy".into(),
            outcomes: BTreeMap::from([(
                "result".into(),
                OutcomeSpec {
                    description: "Observed result".into(),
                    criterion: "Exact bytes".into(),
                    required_reviews: BTreeSet::new(),
                },
            )]),
            review_roles: BTreeMap::new(),
            decision_rule: w::DecisionRule::Designated(w::PersonaId("persona".into())),
            delivery_to: BTreeSet::from(["operator".into()]),
            stakeholders: BTreeSet::from(["operator".into()]),
            invitations: vec![Invitation {
                id: "offer".into(),
                to: "persona".into(),
                terms: w::CommitmentTerms {
                    mandate_revision: 1,
                    exact_terms: "Check returned bytes".into(),
                    outcomes: BTreeSet::from([w::OutcomeId("result".into())]),
                    review_roles: BTreeSet::new(),
                },
            }],
        },
    )
    .unwrap();
    let guide = file("guide", "A reusable procedure: run the program with known input; compare the observed returned bytes; report limits. No imports, run(i32,i32)->i64, memory export. Packed result: pointer high 32 bits, length low 32 bits.", f::Scope::Assets);
    let program = file("program", PROGRAM, f::Scope::Assets);
    let refs = (
        wire_reference(&guide.reference(f::Representation::Utf8)),
        wire_reference(&program.reference(f::Representation::Utf8)),
    );
    app.import_files("operator", "persona", vec![guide, program], true, NOW)
        .unwrap();
    (tmp, app, refs.0, refs.1)
}
fn grant(app: &mut Runtime, guide: &p::ExactRef) -> ComputationInterface {
    app.grant_computation(
        "operator",
        ComputationGrantRequest {
            id: "grant".into(),
            actor: "persona".into(),
            work: "work".into(),
            guide: guide.clone(),
            attempts: 3,
        },
        NOW,
    )
    .unwrap()
}
fn prepare(app: &mut Runtime, id: &str) -> Prepared {
    app.prepare(
        PrepareRequest {
            id: id.into(),
            persona: "persona".into(),
            work: "work".into(),
            mode: Mode::Ordinary,
            profile: Profile::Synthetic,
        },
        NOW,
    )
    .unwrap()
}
fn decision(prepared: &Prepared) -> p::Decision {
    let mut decision: p::Decision = serde_json::from_value(
        json!({"version":1,"invocation_id":prepared.request.id,"epoch":prepared.epoch,
        "work":{"kind":"ongoing","reason":"Check exact output","evidence":[]},
        "fragment_maintenance":{"kind":"no_change","reason":"Voice remains applicable"},
        "asset_maintenance":{"kind":"no_change","reason":"Retain exact assets"},
        "next_fragments":{"kind":"clear","reason":"Only mandatory voice needed"},
        "next_assets":{"kind":"preserve","reason":"Retain selected guidance"},
        "continuation":{"kind":"immediate","purpose":"Inspect the actual observation"},
        "reply":{"kind":"none","reason":"Work remains"},"operations":[]}),
    )
    .unwrap();
    let control: Value = serde_json::from_str(
        prepared
            .prompt
            .split_once("\nCONTROL\n")
            .unwrap()
            .1
            .split_once("\nQUALIFIED_CONTEXT\n")
            .unwrap()
            .0,
    )
    .unwrap();
    for offer in control["own_pending_offers"].as_array().unwrap() {
        decision.operations.push(p::Operation::Responsibility {
            id: offer["id"].as_str().unwrap().into(),
            work_id: "work".into(),
            terms_revision: offer["terms_revision"].as_str().unwrap().into(),
            disposition: "accept".into(),
            reason: "Accept this fixture's limited scope".into(),
        });
    }
    decision
}
fn submit(app: &mut Runtime, d: &p::Decision) -> AdoptionReport {
    app.submit_synthetic(&d.invocation_id, &serde_json::to_vec(d).unwrap(), NOW)
        .unwrap()
}
fn select_guide(app: &mut Runtime, guide: &p::ExactRef) {
    let prepared = prepare(app, "select");
    assert!(prepared.computation_interfaces.is_empty());
    let mut d = decision(&prepared);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Use the reusable procedure".into(),
        references: vec![guide.clone()],
    };
    let report = submit(app, &d);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
}
fn effect(interface: &ComputationInterface, program: &p::ExactRef) -> p::Operation {
    p::Operation::Effect {
        id: "compute".into(),
        capability_id: interface.capability_id.clone(),
        binding_revision: interface.binding_revision.clone(),
        inputs: json!({"program":program,"input":"known input λ"}),
        depends_on: Vec::new(),
    }
}

#[test]
fn complete_decision_executes_once_and_fresh_context_gets_actual_result() {
    let (_tmp, mut app, guide, program) = fixture();
    let interface = grant(&mut app, &guide);
    select_guide(&mut app, &guide);
    let prepared = prepare(&mut app, "compute-call");
    assert_eq!(prepared.computation_interfaces, vec![interface.clone()]);
    let mut d = decision(&prepared);
    d.operations.push(effect(&interface, &program));
    let accepted = submit(&mut app, &d);
    assert_eq!(accepted.status, AdoptionStatus::Adopted, "{accepted:?}");
    let state = app.snapshot().unwrap();
    let (id, marker) = state.computation_jobs.iter().next().unwrap();
    assert_eq!(marker.state, s::IntentState::Succeeded);
    let result = &state.observations[marker.observation.as_ref().unwrap()];
    assert_eq!(result.payload["result"]["output"], "known input λ");
    let retained = app.store.intent(id).unwrap();
    assert_eq!(
        retained
            .history
            .iter()
            .filter(|r| r.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
    let replay = app
        .adopt_response(&d.invocation_id, &serde_json::to_vec(&d).unwrap(), NOW)
        .unwrap();
    assert_eq!(replay.accepted_epoch, accepted.accepted_epoch);
    assert_eq!(app.recover_computations(NOW).unwrap(), 0);
    assert_eq!(app.store.intent(id).unwrap().history, retained.history);
    let next = prepare(&mut app, "interpret-call");
    assert!(next.prompt.contains("known input λ"));
    assert!(next.supplied_observations.contains(&result.id));
}

#[test]
fn grant_without_supplied_guide_and_invalid_complete_response_execute_nothing() {
    let (_tmp, mut app, guide, program) = fixture();
    let interface = grant(&mut app, &guide);
    let prepared = prepare(&mut app, "unexposed");
    let mut d = decision(&prepared);
    d.next_assets = p::SelectionIntent::Clear {
        reason: "No selected assets".into(),
    };
    d.operations.push(effect(&interface, &program));
    let held = submit(&mut app, &d);
    assert_eq!(held.status, AdoptionStatus::Held);
    assert!(app.snapshot().unwrap().computation_jobs.is_empty());

    let (_tmp, mut app, guide, program) = fixture();
    let interface = grant(&mut app, &guide);
    select_guide(&mut app, &guide);
    let prepared = prepare(&mut app, "invalid");
    let mut d = decision(&prepared);
    d.operations.push(effect(&interface, &program));
    let mut bytes = serde_json::to_value(&d).unwrap();
    bytes
        .as_object_mut()
        .unwrap()
        .remove("fragment_maintenance");
    assert!(app
        .submit_synthetic("invalid", &serde_json::to_vec(&bytes).unwrap(), NOW)
        .is_err());
    assert!(app.snapshot().unwrap().computation_jobs.is_empty());
    assert_eq!(
        app.invocation("invalid").unwrap().state,
        s::InvocationState::Prepared
    );
}

#[test]
fn revocation_is_current_and_cannot_refill_a_work_allocation() {
    let (_tmp, mut app, guide, program) = fixture();
    let interface = grant(&mut app, &guide);
    select_guide(&mut app, &guide);
    let prepared = prepare(&mut app, "revoke");
    let mut d = decision(&prepared);
    d.operations.push(effect(&interface, &program));
    // Test the complete authority check independently of the Store's additional
    // current-epoch fence, which also rejects a concurrent operator mutation.
    let (_, mut state) = app.load().unwrap();
    state.computation_grants.get_mut("grant").unwrap().revoked = true;
    assert!(validate(&state, &prepared, &d, &d.operations[0], NOW).is_err());
    assert!(app
        .grant_computation(
            "operator",
            ComputationGrantRequest {
                id: "second-grant".into(),
                actor: "persona".into(),
                work: "work".into(),
                guide,
                attempts: 3
            },
            NOW
        )
        .is_err());
}

#[test]
fn same_response_program_is_bound_after_save_and_result_does_not_resume_stop() {
    let (_tmp, mut app, guide, _) = fixture();
    let interface = grant(&mut app, &guide);
    select_guide(&mut app, &guide);
    let prepared = prepare(&mut app, "author-and-run");
    let mut d = decision(&prepared);
    d.operations.push(p::Operation::FileWrite {
        id: "save-program".into(),
        scope: p::Scope::Assets {},
        source: "authored-program".into(),
        path: "authored-program.wat".into(),
        base_revision: None,
        text: PROGRAM.into(),
        qualifications: vec![guide],
        policy_id: "fixture-policy".into(),
    });
    let program = p::ExactRef {
        source: "authored-program".into(),
        revision: "proposal-op:save-program".into(),
        representation: "utf8".into(),
        start: None,
        end: None,
        lifetime: None,
    };
    let mut op = effect(&interface, &program);
    if let p::Operation::Effect { depends_on, .. } = &mut op {
        depends_on.push("save-program".into());
    }
    d.operations.push(op);
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Retain the exact computation".into(),
        operation_ids: vec!["save-program".into()],
    };
    d.continuation = p::Continuation::Stop {
        reason: "Store this result without another decision".into(),
    };
    let report = submit(&mut app, &d);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let state = app.snapshot().unwrap();
    let marker = state.computation_jobs.values().next().unwrap();
    assert_eq!(marker.state, s::IntentState::Succeeded);
    let result = &state.observations[marker.observation.as_ref().unwrap()];
    assert_eq!(
        result.payload["program"]["revision"],
        state.sources["authored-program"].head
    );
    let a = admission(&state.personas["persona"], NOW);
    let projected =
        observation_projection::project(&state, result, &a, &corpus(&state, &a).unwrap())
            .unwrap()
            .0;
    assert!(projected["current_required_qualifications"]
        .to_string()
        .contains("compare the observed returned bytes"));
    assert!(!state.personas["persona"].focus["work"].permit);
    assert!(matches!(
        state.personas["persona"].focus["work"].continuation,
        Some(p::Continuation::Stop { .. })
    ));
}

#[test]
fn effective_character_change_fences_computation_and_its_charge() {
    let (_tmp, mut app, guide, program) = fixture();
    let interface = grant(&mut app, &guide);
    select_guide(&mut app, &guide);
    let prepared = prepare(&mut app, "character-change");
    let mut d = decision(&prepared);
    let character = app.snapshot().unwrap().personas["persona"]
        .character
        .clone()
        .unwrap();
    let mut ocean = character.ocean;
    ocean.insert(
        "openness".into(),
        "I now seek a broader range of alternatives.".into(),
    );
    d.operations.push(p::Operation::CharacterAdopt {
        id: "character".into(),
        base_revision: Some(character.revision),
        ocean,
        vad: character.vad,
        own_voice: character.own_voice,
        reason: "Voluntary fixture change".into(),
        evidence: Vec::new(),
    });
    d.operations.push(effect(&interface, &program));
    d.fragment_maintenance = p::Maintenance::Change {
        reason: "Adopt this characteristic change".into(),
        operation_ids: vec!["character".into()],
    };
    let report = submit(&mut app, &d);
    assert_eq!(report.status, AdoptionStatus::CharacterOnly, "{report:?}");
    assert!(report.fenced.contains(&"compute".into()));
    let state = app.snapshot().unwrap();
    assert!(state.computation_jobs.is_empty());
    assert_eq!(state.computation_grants["grant"].used, 0);
}

/// Create a durable accepted pending job without starting the host runner. This
/// fixture uses the production planner and Store transaction, then deliberately
/// stops at the crash boundaries the synchronous public API normally crosses.
fn admitted_job(
    app: &mut Runtime,
    guide: &p::ExactRef,
    program: &p::ExactRef,
) -> (String, s::ExecutionOwner) {
    let interface = grant(app, guide);
    select_guide(app, guide);
    let prepared = prepare(app, "interrupted");
    let mut d = decision(&prepared);
    d.operations.push(effect(&interface, program));
    let owner = app.store.acquire_execution_owner().unwrap();
    let (old, state) = app.load().unwrap();
    let (next, _, _, mut intents) = plan_response(
        state,
        &prepared,
        &d,
        NOW,
        &mut PlanningExposure::new(prepared.exposure.as_ref()),
        &mut app.store,
    )
    .unwrap();
    let object = state_object(&old, &next).unwrap();
    intents[0].depends_on.push(object.exact_ref().unwrap());
    let id = intents[0].id.clone();
    app.store.dispatch_invocation(&prepared.request.id).unwrap();
    let hash = digest(&d).unwrap();
    app.store
        .record_response(&prepared.request.id, &hash)
        .unwrap();
    app.store
        .commit_decision(s::Decision {
            expected_epoch: prepared.epoch,
            invocation_id: prepared.request.id,
            response_hash: hash,
            changes: vec![s::Change {
                expected_revision: Some(old.revision),
                object,
                depends_on: Vec::new(),
            }],
            intents,
        })
        .unwrap();
    (id, owner)
}

#[test]
fn crash_after_dispatch_preserves_unknown_without_reexecuting() {
    let (tmp, mut app, guide, program) = fixture();
    let (id, owner) = admitted_job(&mut app, &guide, &program);
    app.store
        .trusted_host()
        .reserve_terminal_evidence(
            &s::EntityId::Intent(id.clone()),
            &composite_id("computation-result", &[&id]),
            RESULT_CAPACITY,
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    app.store
        .dispatch_intent(&id, app.store.epoch().unwrap())
        .unwrap();
    drop(owner);
    drop(app);
    let mut app = Runtime::open(&tmp.path().join("state")).unwrap();
    app.recover_computations(NOW).unwrap();
    let intent = app.store.intent(&id).unwrap();
    assert_eq!(intent.state, s::IntentState::Uncertain);
    assert_eq!(
        intent
            .history
            .iter()
            .filter(|r| r.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
    let state = app.snapshot().unwrap();
    let marker = &state.computation_jobs[&id];
    assert!(state.observations[marker.observation.as_ref().unwrap()].payload["result"].is_null());
    assert!(exposed(
        &state,
        "persona",
        "work",
        &BTreeSet::from([file_reference(&guide).unwrap()]),
        NOW
    )
    .unwrap()
    .is_empty());
    assert_eq!(app.recover_computations(NOW).unwrap(), 0);
    let mut inherited = Exposure::default();
    assert_eq!(
        control(
            &state,
            "persona",
            "work",
            &admission(&state.personas["persona"], NOW),
            &mut inherited
        )["unresolved"][0]["state"],
        "Uncertain"
    );
}

#[test]
fn exact_recorded_terminal_reconciles_after_crash_without_execution_replay() {
    let (tmp, mut app, guide, program) = fixture();
    let (id, owner) = admitted_job(&mut app, &guide, &program);
    let intent = app.store.intent(&id).unwrap();
    let job: Job = serde_json::from_value(intent.intent.body.clone()).unwrap();
    let slot = composite_id("computation-result", &[&id]);
    app.store
        .trusted_host()
        .reserve_terminal_evidence(
            &s::EntityId::Intent(id.clone()),
            &slot,
            RESULT_CAPACITY,
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    app.store
        .dispatch_intent(&id, app.store.epoch().unwrap())
        .unwrap();
    let terminal = Terminal {
        intent: id.clone(),
        request_sha256: digest(&intent.intent.body).unwrap(),
        engine_revision: c::engine_revision(),
        program_sha256: s::sha256_bytes(job.program.as_bytes()),
        input_sha256: s::sha256_bytes(job.input.as_bytes()),
        outcome: c::execute(&job.program, &job.input),
    };
    app.store
        .trusted_host()
        .record_terminal_evidence(
            s::StoredObject {
                id: slot,
                revision: 1,
                kind: "computation-terminal".into(),
                body: serde_json::to_value(terminal).unwrap(),
            },
            |_, _| Ok(()),
        )
        .unwrap();
    drop(owner);
    drop(app);
    let mut app = Runtime::open(&tmp.path().join("state")).unwrap();
    app.recover_computations(NOW).unwrap();
    let intent = app.store.intent(&id).unwrap();
    assert_eq!(intent.state, s::IntentState::Succeeded);
    assert!(intent.reconciliation.is_some());
    assert_eq!(
        intent
            .history
            .iter()
            .filter(|r| r.status == s::ReceiptStatus::Dispatched)
            .count(),
        1
    );
    let state = app.snapshot().unwrap();
    assert_eq!(
        state.observations[state.computation_jobs[&id].observation.as_ref().unwrap()].payload
            ["result"]["output"],
        "known input λ"
    );
}

#[test]
fn changed_authority_cancels_before_dispatch_and_result_privacy_is_rechecked() {
    let (_tmp, mut app, guide, program) = fixture();
    let (id, _owner) = admitted_job(&mut app, &guide, &program);
    app.revoke_computation("operator", "grant").unwrap();
    app.finish_computation(&id, NOW).unwrap();
    let intent = app.store.intent(&id).unwrap();
    assert_eq!(intent.state, s::IntentState::Canceled);
    assert!(intent
        .history
        .iter()
        .all(|r| r.status != s::ReceiptStatus::Dispatched));
    let mut state = app.snapshot().unwrap();
    let marker = &state.computation_jobs[&id];
    let observation = state.observations[marker.observation.as_ref().unwrap()].clone();
    state.sources.get_mut("guide").unwrap().retired = true;
    let a = admission(&state.personas["persona"], NOW);
    assert!(observation_projection::project(
        &state,
        &observation,
        &a,
        &corpus(&state, &a).unwrap()
    )
    .is_err());
}
