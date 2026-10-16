//! Synthetic local fixtures only; no live state, credentials or provider calls.
use super::*;

const ACTOR: &str = "persona-0001";
const WORK: &str = "work-00000001";
const BODY: &str = "Synthetic optional habit: check simultaneous duties before promising capacity.";

#[test]
fn inventory_private_persona_import_cannot_bypass_global_corpus_bound() {
    let (_directory, mut app) = fixture();
    let (head, mut state) = app.load().unwrap();
    initialized(&mut state);
    add(&mut state, "optional");
    state.config.corpus.max_revisions = 2;
    let mut peer_policy = state.config.policies.active_policies[0].clone();
    peer_policy.id = "peer-private".into();
    peer_policy.readers = BTreeSet::from(["persona-0003".into()]);
    peer_policy.audience = BTreeSet::from(["fixture-human".into()]);
    state
        .config
        .policies
        .active_policies
        .push(peer_policy.clone());
    let mut seed = state.personas["persona-0002"].seed.clone();
    seed.id = "persona-0003".into();
    seed.audience = peer_policy.audience.clone();
    seed.grants.write_policies = BTreeSet::from([peer_policy.id.clone()]);
    let voice = f::FileRevision {
        source: "peer-private-voice".into(),
        revision: String::new(),
        path: "fragments/peer/voice.txt".into(),
        scope: f::Scope::Fragments,
        bytes: b"Synthetic imported peer voice".to_vec(),
        author: seed.id.clone(),
        policy: peer_policy,
        qualifications: vec![],
        retired: false,
    }
    .seal()
    .unwrap();
    let import = PersonaImport {
        seed,
        voice,
        ocean: [
            "openness",
            "conscientiousness",
            "extraversion",
            "agreeableness",
            "neuroticism",
        ]
        .into_iter()
        .map(|k| (k.into(), "Fixture trait".into()))
        .collect(),
        vad: ["valence", "activation", "control"]
            .into_iter()
            .map(|k| (k.into(), "Fixture trait".into()))
            .collect(),
    };
    app.save(&head, &state).unwrap();
    let before = app.load().unwrap();
    let expected = inventory(&state).0;
    let error = app
        .import_persona("fixture-human", import, 100)
        .unwrap_err();
    assert!(error.to_string().contains("input revisions"), "{error}");
    let after = app.load().unwrap();
    assert_eq!(before.0, after.0);
    assert_eq!(
        serde_json::to_value(&before.1).unwrap(),
        serde_json::to_value(&after.1).unwrap()
    );
    assert_eq!(inventory(&after.1).0, expected);
}

#[test]
fn inventory_larger_next_window_cannot_hold_stop_and_cursor_replays_after_reopen() {
    let (directory, mut app) = fixture();
    let (head, mut state) = app.load().unwrap();
    initialized(&mut state);
    for n in 0..MAX_WINDOW_HEADS {
        let id = format!("a-{n:03}");
        add(&mut state, &id);
        state.sources.get_mut(&id).unwrap().revisions[0].record.path =
            format!("{id}/{}", "x".repeat(9000));
        reseal(&mut state, &id);
    }
    add(&mut state, "z-large");
    state.sources.get_mut("z-large").unwrap().revisions[0]
        .record
        .path = "x".repeat(6000);
    reseal(&mut state, "z-large");
    let first = render(&state, ACTOR, WORK, "fixture-stop", 1, 100, false, None).unwrap();
    assert!(control(&first)["retained_optional_fragments"]["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    state.config.context_bytes = first.total + 3500;
    app.save(&head, &state).unwrap();
    let request = PrepareRequest {
        id: "fixture-stop".into(),
        persona: ACTOR.into(),
        work: WORK.into(),
        profile: Profile::Synthetic,
        mode: Mode::Ordinary,
    };
    let prepared = app.prepare(request.clone(), 100).unwrap();
    let (_, admitted) = app.load().unwrap();
    assert_eq!(
        admitted.personas[ACTOR].fragment_inventory_after.as_deref(),
        Some("a-063")
    );
    let next = render(&admitted, ACTOR, WORK, "fixture-stop", 2, 100, false, None).unwrap();
    assert!(control(&next).get("retained_optional_fragments").is_none());
    let attempts = app.store.ledger().unwrap().ordinary.consumed.attempts;
    drop(app);
    let mut app = Runtime::open(&directory.path().join("state")).unwrap();
    let replay = app.prepare(request, 100).unwrap();
    assert_eq!(replay.prompt_sha256, prepared.prompt_sha256);
    assert_eq!(
        app.store.ledger().unwrap().ordinary.consumed.attempts,
        attempts
    );
    let decision = json!({"version":1,"invocation_id":prepared.request.id,"epoch":prepared.epoch,
        "work":{"kind":"blocked","reason":"Fixture voluntary yield","evidence":[]},
        "fragment_maintenance":{"kind":"no_change","reason":"No fixture revision"},
        "asset_maintenance":{"kind":"no_change","reason":"No fixture revision"},
        "next_fragments":{"kind":"clear","reason":"No optional selection"},
        "next_assets":{"kind":"clear","reason":"No optional selection"},
        "continuation":{"kind":"stop","reason":"Fixture voluntary stop"},
        "reply":{"kind":"none","reason":"No fixture message"},"operations":[]});
    let report = app
        .submit_synthetic("fixture-stop", &serde_json::to_vec(&decision).unwrap(), 101)
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Adopted);
    let (_, after) = app.load().unwrap();
    assert!(matches!(
        after.personas[ACTOR].focus[WORK].continuation,
        Some(p::Continuation::Stop { .. })
    ));
    assert_eq!(
        after.personas[ACTOR].fragment_inventory_after,
        admitted.personas[ACTOR].fragment_inventory_after
    );
    assert_eq!(
        after.personas["persona-0002"].fragment_inventory_after,
        None
    );
    assert_eq!(
        app.store.ledger().unwrap().ordinary.consumed.attempts,
        attempts
    );
}

#[test]
fn inventory_byte_limited_windows_retry_and_wrap_without_skipping_fitting_entries() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    for n in 0..80 {
        add(&mut state, &format!("item-{n:03}"));
    }
    let mut seen = BTreeSet::new();
    let mut completed = false;
    for _ in 0..81 {
        let mut exposure = Exposure::default();
        let (value, next) = context(
            &state,
            ACTOR,
            &admission(&state.personas[ACTOR], 100),
            &mut exposure,
        )
        .unwrap();
        for entry in value["entries"].as_array().unwrap() {
            seen.insert(entry["reference"]["source"].as_str().unwrap().to_owned());
        }
        assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_METADATA_BYTES);
        state
            .personas
            .get_mut(ACTOR)
            .unwrap()
            .fragment_inventory_after = next.clone();
        if next.is_none() {
            completed = true;
            break;
        }
    }
    assert!(completed);
    assert_eq!(seen.len(), 80);
    assert_eq!(
        inventory(&state).0["entries"][0]["reference"]["source"],
        "item-000"
    );
}

#[test]
fn inventory_yields_to_selection_and_keeps_repair_ordinary_measurement_consistent() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    add(&mut state, "optional");
    add(&mut state, "selected-asset");
    let source = state.sources.get_mut("selected-asset").unwrap();
    source.revisions[0].record.scope = f::Scope::Assets;
    source.revisions[0].record.bytes = vec![b'x'; 20000];
    reseal(&mut state, "selected-asset");
    let reference = f::Reference::full(
        "selected-asset",
        &state.sources["selected-asset"].head,
        f::Representation::Utf8,
    );
    state
        .personas
        .get_mut(ACTOR)
        .unwrap()
        .focus
        .get_mut(WORK)
        .unwrap()
        .assets = Some(vec![Selected {
        reference: wire_reference(&reference),
        last_admission: None,
    }]);
    let baseline = render(&state, ACTOR, WORK, "fixture", 1, 100, false, None).unwrap();
    // This bound exceeds mandatory material but is below accepted selection size.
    state.config.context_bytes = baseline.total - MAX_METADATA_BYTES - 1000;
    let ordinary = render_mode(&state, ACTOR, WORK, "fixture", 1, 100, false, None, None).unwrap();
    assert!(ordinary.ordinary_total > state.config.context_bytes);
    assert!(control(&ordinary)
        .get("retained_optional_fragments")
        .is_none());
    let repair = render_mode(
        &state,
        ACTOR,
        WORK,
        "fixture",
        1,
        100,
        false,
        Some(state.config.navigation_per_call),
        None,
    )
    .unwrap();
    assert_eq!(ordinary.ordinary_total, repair.ordinary_total);
    assert_eq!(ordinary.growth_bytes, repair.growth_bytes);
    assert!(repair.next_fragment_inventory_after.is_none());
}

#[test]
fn inventory_unrelated_and_denied_sources_cannot_shift_actor_visible_window() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    let wanted = add(&mut state, "z-own-fragment");
    let (expected, expected_exposure) = inventory(&state);
    for n in 0..MAX_WINDOW_HEADS {
        let id = format!("a-unrelated-{n:03}");
        add(&mut state, &id);
        let source = state.sources.get_mut(&id).unwrap();
        if n % 2 == 0 {
            source.owner = "persona-0002".into();
        } else {
            source.revisions[0].record.scope = f::Scope::Assets;
            reseal(&mut state, &id);
        }
    }
    add(&mut state, "a-denied-own");
    state.sources.get_mut("a-denied-own").unwrap().revisions[0]
        .record
        .policy
        .processing_destinations
        .clear();
    reseal(&mut state, "a-denied-own");
    state
        .personas
        .get_mut(ACTOR)
        .unwrap()
        .fragment_inventory_after = Some("a-denied-own".into());
    let (actual, exposure) = inventory(&state);
    assert_eq!(actual, expected);
    assert_eq!(exposure.sources, expected_exposure.sources);
    assert_eq!(
        actual["entries"][0]["reference"],
        serde_json::to_value(wire_reference(&wanted)).unwrap()
    );
    let rendered = render(&state, ACTOR, WORK, "fixture", 1, 100, false, None).unwrap();
    assert!(rendered.exposure.sources.contains(&wanted));
    assert!(!rendered.sources.contains(&wanted));
    assert!(!rendered.prompt.contains(BODY));
}

#[test]
fn inventory_metadata_and_exposure_are_reserved_by_normal_preparation() {
    let (_directory, mut app) = fixture();
    let (head, mut state) = app.load().unwrap();
    initialized(&mut state);
    let reference = add(&mut state, "optional");
    app.save(&head, &state).unwrap();
    let before = app.store.ledger().unwrap();
    let prepared = app
        .prepare(
            PrepareRequest {
                id: "fixture-prepared".into(),
                persona: ACTOR.into(),
                work: WORK.into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            100,
        )
        .unwrap();
    let after = app.store.ledger().unwrap();
    let value = serde_json::to_value(&prepared).unwrap();
    assert_eq!(
        after.ordinary.reserved.input_bytes - before.ordinary.reserved.input_bytes,
        2 * serde_json::to_vec(&value).unwrap().len() as u64
    );
    assert_eq!(
        after.ordinary.consumed.attempts - before.ordinary.consumed.attempts,
        1
    );
    assert_eq!(
        after.ordinary.reserved.navigation_bytes - before.ordinary.reserved.navigation_bytes,
        (prepared.navigation.scanned_bytes + prepared.navigation.result_bytes) as u64
    );
    assert!(prepared
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .contains(&reference));
    assert!(!prepared.sources.contains(&reference));
    assert!(!prepared.prompt.contains(BODY));
    assert!(prepared.bundle_roots.as_ref().unwrap().fragments.is_empty());
    let (_, after_state) = app.load().unwrap();
    assert!(after_state.personas[ACTOR].focus[WORK].fragments.is_none());
    assert_eq!(
        after_state.personas[ACTOR]
            .character
            .as_ref()
            .unwrap()
            .revision,
        "fixture-character"
    );
    assert_eq!(after.ordinary.uncertain, before.ordinary.uncertain);
}

fn add(state: &mut State, id: &str) -> f::Reference {
    let file = f::FileRevision {
        source: id.into(),
        revision: String::new(),
        path: format!("fragments/{ACTOR}/{id}.txt"),
        scope: f::Scope::Fragments,
        bytes: BODY.as_bytes().to_vec(),
        author: ACTOR.into(),
        policy: state.config.policies.active_policies[0].clone(),
        qualifications: vec![],
        retired: false,
    }
    .seal()
    .unwrap();
    let reference = file.reference(f::Representation::Utf8);
    state.sources.insert(
        id.into(),
        Source {
            owner: ACTOR.into(),
            protected: false,
            retired: false,
            head: file.revision.clone(),
            imported_by: None,
            revisions: vec![ManagedRevision {
                record: file,
                lineage: vec![],
                exposure: Some(Exposure::default()),
                origin: Some(RevisionOrigin::PersonaInvocation {
                    persona: ACTOR.into(),
                    invocation_id: "fixture-invocation".into(),
                    operation_id: "fixture-write".into(),
                }),
            }],
        },
    );
    reference
}

fn initialized(state: &mut State) {
    let core = add(state, "core");
    state.sources.get_mut("core").unwrap().revisions[0]
        .record
        .bytes = b"Synthetic core voice".to_vec();
    reseal(state, "core");
    let core = f::Reference {
        revision: state.sources["core"].head.clone(),
        ..core
    };
    let mut exact = wire_reference(&core);
    exact.lifetime = Some("work".into());
    state.personas.get_mut(ACTOR).unwrap().character = Some(Character {
        revision: "fixture-character".into(),
        ocean: BTreeMap::new(),
        vad: BTreeMap::new(),
        own_voice: vec![exact],
        attribution: "fixture-accepted".into(),
        exposure: Some(Exposure::default()),
    });
}

fn reseal(state: &mut State, id: &str) {
    let source = state.sources.get_mut(id).unwrap();
    let record = &mut source.revisions[0].record;
    record.revision = record.computed_revision().unwrap();
    source.head = record.revision.clone();
}

fn inventory(state: &State) -> (Value, Exposure) {
    let mut exposure = Exposure::default();
    let (value, _) = context(
        state,
        ACTOR,
        &admission(&state.personas[ACTOR], 100),
        &mut exposure,
    )
    .unwrap();
    (value, exposure)
}

fn control(rendered: &Rendered) -> Value {
    serde_json::from_str(
        rendered
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

#[test]
fn inventory_filters_owner_scope_provenance_and_visibility_without_mutation() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    let wanted = add(&mut state, "optional");
    for id in [
        "peer",
        "author",
        "import",
        "legacy",
        "asset",
        "protected",
        "retired",
        "hidden",
        "other-origin",
    ] {
        add(&mut state, id);
    }
    state.sources.get_mut("peer").unwrap().owner = "persona-0002".into();
    state.sources.get_mut("author").unwrap().revisions[0]
        .record
        .author = "persona-0002".into();
    state.sources.get_mut("import").unwrap().revisions[0].origin =
        Some(RevisionOrigin::OperatorImport {
            operator: "fixture-human".into(),
            event_index: 0,
        });
    state.sources.get_mut("legacy").unwrap().revisions[0].origin = None;
    state.sources.get_mut("asset").unwrap().revisions[0]
        .record
        .scope = f::Scope::Assets;
    state.sources.get_mut("protected").unwrap().protected = true;
    state.sources.get_mut("retired").unwrap().retired = true;
    state.sources.get_mut("hidden").unwrap().revisions[0]
        .record
        .path = "fragments/.private/habit.txt".into();
    state.sources.get_mut("other-origin").unwrap().revisions[0].origin =
        Some(RevisionOrigin::PersonaInvocation {
            persona: "persona-0002".into(),
            invocation_id: "fixture".into(),
            operation_id: "write".into(),
        });
    for id in ["author", "asset", "hidden"] {
        reseal(&mut state, id);
    }
    let before = serde_json::to_value(&state).unwrap();
    let (value, exposure) = inventory(&state);
    assert_eq!(value["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["entries"][0]["reference"],
        serde_json::to_value(wire_reference(&wanted)).unwrap()
    );
    assert_eq!(value["entries"][0]["optional"], true);
    assert_eq!(value["body_supplied"], false);
    assert_eq!(value["truncated"], false);
    assert_eq!(exposure.sources, vec![wanted]);
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
}

#[test]
fn inventory_rechecks_policy_inherited_lineage_and_binding_eligibility() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    let reference = add(&mut state, "optional");
    assert_eq!(inventory(&state).0["entries"].as_array().unwrap().len(), 1);
    let mut policy = state.config.policies.active_policies[0].clone();
    policy.id = "restricted".into();
    policy.expires_at_unix = Some(100);
    state.config.policies.active_policies.push(policy.clone());
    state.sources.get_mut("optional").unwrap().revisions[0]
        .exposure
        .as_mut()
        .unwrap()
        .policies
        .push(scoped_policy("fixture", "basis", "1", &policy));
    assert!(inventory(&state).0["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    state.sources.get_mut("optional").unwrap().revisions[0].exposure = Some(Exposure::default());
    state.accepted_bindings.push(f::AcceptedBinding {
        id: "fixture-binding".into(),
        revision: "1".into(),
        source: reference.source,
        file_revision: reference.revision,
        policy: state.config.policies.active_policies[0].clone(),
        disposition: f::BindingDisposition::Ineligible {
            reason: "fixture".into(),
        },
    });
    assert!(inventory(&state).0["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    state.accepted_bindings.clear();
    let mut denied = admission(&state.personas[ACTOR], 100);
    denied.destination = "different-destination".into();
    let mut exposure = Exposure::default();
    assert!(
        context(&state, ACTOR, &denied, &mut exposure).unwrap().0["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(exposure.sources.is_empty());
    state.sources.get_mut("optional").unwrap().revisions[0].exposure = None;
    assert!(inventory(&state).0["entries"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn inventory_bounds_inspection_output_and_reports_omission() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    for n in 0..80 {
        add(&mut state, &format!("item-{n:03}"));
    }
    let (value, exposure) = inventory(&state);
    assert_eq!(value["truncated"], true);
    assert!(value["entries"].as_array().unwrap().len() <= MAX_ENTRIES);
    assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_METADATA_BYTES);
    assert_eq!(exposure.sources.len(), 80);
    for source in state.sources.values_mut() {
        source.protected = true;
    }
    let (value, _) = inventory(&state);
    assert_eq!(value["truncated"], false);
    assert!(value["entries"].as_array().unwrap().is_empty());
    state.sources.clear();
    add(&mut state, "oversized");
    state.sources.get_mut("oversized").unwrap().revisions[0]
        .record
        .path = "x".repeat(MAX_METADATA_BYTES);
    reseal(&mut state, "oversized");
    add(&mut state, "z-small");
    let (value, exposure) = inventory(&state);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        exposure
            .sources
            .iter()
            .map(|r| r.source.as_str())
            .collect::<Vec<_>>(),
        vec!["oversized", "z-small"]
    );
}

#[test]
fn inventory_fresh_request_is_metadata_only_until_explicit_selection_and_counts_bytes() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    initialized(&mut state);
    let empty = render(&state, ACTOR, WORK, "fixture-render", 1, 100, false, None).unwrap();
    let reference = add(&mut state, "optional");
    let before = serde_json::to_value(&state).unwrap();
    let ordinary = render(&state, ACTOR, WORK, "fixture-render", 1, 100, false, None).unwrap();
    assert!(ordinary.total > empty.total);
    assert!(ordinary.growth_bytes > empty.growth_bytes);
    assert_eq!(
        ordinary.total - empty.total,
        ordinary.growth_bytes - empty.growth_bytes
    );
    assert!(!ordinary.prompt.contains(BODY));
    assert!(!ordinary.sources.contains(&reference));
    assert!(ordinary.roots.fragments.is_empty());
    assert!(ordinary.exposure.sources.contains(&reference));
    assert_eq!(
        control(&ordinary)["retained_optional_fragments"]["entries"][0]["reference"],
        serde_json::to_value(wire_reference(&reference)).unwrap()
    );
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    state.config.context_bytes = ordinary.total - 1;
    let tight = render(&state, ACTOR, WORK, "fixture-render", 1, 100, false, None).unwrap();
    assert!(control(&tight).get("retained_optional_fragments").is_none());
    assert!(tight.next_fragment_inventory_after.is_none());
    state.config.context_bytes = empty.total - 2048;
    assert!(render(&state, ACTOR, WORK, "fixture-render", 1, 100, false, None).is_err());
    state.config.context_bytes = 196608;
    // Synthetic explicit choice, not a claim that a live model chose or improved.
    state
        .personas
        .get_mut(ACTOR)
        .unwrap()
        .focus
        .get_mut(WORK)
        .unwrap()
        .fragments = Some(vec![Selected {
        reference: wire_reference(&reference),
        last_admission: None,
    }]);
    let chosen = render(&state, ACTOR, WORK, "fixture-next", 1, 100, false, None).unwrap();
    assert!(chosen.prompt.contains(BODY));
    assert!(chosen.sources.contains(&reference));
    assert_eq!(chosen.roots.fragments, vec![reference]);
}

#[test]
fn inventory_is_absent_during_initialization_and_restricted_repair() {
    let (_directory, mut app) = fixture();
    let (_, mut state) = app.load().unwrap();
    add(&mut state, "optional");
    let initializing = render(&state, ACTOR, WORK, "fixture-render", 1, 100, false, None).unwrap();
    assert!(control(&initializing)
        .get("retained_optional_fragments")
        .is_none());
    initialized(&mut state);
    let repair = render_mode(
        &state,
        ACTOR,
        WORK,
        "fixture-repair",
        1,
        100,
        false,
        Some(state.config.navigation_per_call),
        None,
    )
    .unwrap();
    assert!(control(&repair)
        .get("retained_optional_fragments")
        .is_none());
    assert!(!repair.prompt.contains(BODY));
    assert!(!repair
        .exposure
        .sources
        .iter()
        .any(|r| r.source == "optional"));
}
fn fixture() -> (tempfile::TempDir, Runtime) {
    let directory = tempfile::tempdir().unwrap();
    let working = directory.path().join("working");
    fs::create_dir(&working).unwrap();
    let provider = serde_json::from_value(json!({
        "mode":"platform_api_key", "model":"fixture-model", "effort":"high",
        "credentials":{"kind":"api_key", "key_env":{"env":"FIXTURE_UNUSED_KEY"},
            "account_env":{"env":"FIXTURE_UNUSED_ACCOUNT"}},
        "evidence":{"mode":"platform_api_key", "model":"fixture-model", "efforts":["high"],
            "account_env":{"env":"FIXTURE_UNUSED_ACCOUNT"}, "account_sha256":"0".repeat(64),
            "transport_profile":"platform_responses_v2", "catalog_evidence_sha256":"1".repeat(64),
            "structured_output_evidence_sha256":"2".repeat(64)},
        "limits":{"timeout_ms":1000, "max_prompt_bytes":196608, "max_schema_bytes":65536,
            "max_schema_nodes":10000,"max_schema_depth":64,"max_request_bytes":400000,
            "max_response_bytes":32768,"max_stream_bytes":1048576,
            "max_event_bytes":131072,"max_events":10000}
    }))
    .unwrap();
    let mut app = launch::setup(
        &directory.path().join("state"),
        "fixture-human",
        launch::Setup {
            cohort: 2,
            owner: 1,
            destination: "fixture-local".into(),
            provider,
            working_directory: working,
            share_cohort: true,
            max_steps: 1,
            max_considerations: 4,
            max_work_attempts: 32,
            max_total_steps: 2,
            max_works: 1,
        },
        true,
    )
    .unwrap();
    assert_eq!(
        launch::start(&mut app, "First exact fixture request").unwrap(),
        "work-00000001"
    );
    (directory, app)
}
