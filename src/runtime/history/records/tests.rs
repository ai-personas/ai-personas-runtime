//! Mechanical retrieval tests with synthetic Decisions and disposable local state.
//! These do not establish that a persona will choose a useful revision.
use super::*;

const ACTOR: &str = "persona-0001";
const FIRST: &str = "work-00000001";
const SECOND: &str = "work-00000002";

pub(super) fn fixture() -> (tempfile::TempDir, Runtime) {
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
            max_total_steps: 32,
            max_works: 2,
        },
        true,
    )
    .unwrap();
    assert_eq!(
        launch::start(&mut app, "First synthetic history request").unwrap(),
        FIRST
    );
    let (head, mut state) = app.load().unwrap();
    let voice = f::FileRevision {
        source: "history-fixture-core".into(),
        revision: String::new(),
        path: "fragments/persona-0001/core.txt".into(),
        scope: f::Scope::Fragments,
        bytes: b"Synthetic history fixture core voice".to_vec(),
        author: ACTOR.into(),
        policy: state.config.policies.active_policies[0].clone(),
        qualifications: vec![],
        retired: false,
    }
    .seal()
    .unwrap();
    let mut reference = wire_reference(&voice.reference(f::Representation::Utf8));
    reference.lifetime = Some("work".into());
    state.sources.insert(
        voice.source.clone(),
        Source {
            owner: ACTOR.into(),
            protected: false,
            retired: false,
            head: voice.revision.clone(),
            imported_by: None,
            revisions: vec![ManagedRevision {
                record: voice,
                lineage: vec![],
                exposure: Some(Exposure::default()),
                origin: Some(RevisionOrigin::PersonaInvocation {
                    persona: ACTOR.into(),
                    invocation_id: "fixture-initialization".into(),
                    operation_id: "core".into(),
                }),
            }],
        },
    );
    state.personas.get_mut(ACTOR).unwrap().character = Some(Character {
        revision: "history-fixture-character".into(),
        ocean: BTreeMap::new(),
        vad: BTreeMap::new(),
        own_voice: vec![reference],
        attribution: "synthetic-fixture".into(),
        exposure: Some(Exposure::default()),
    });
    app.save(&head, &state).unwrap();
    (directory, app)
}

pub(super) fn prepare(app: &mut Runtime, id: &str, work: &str, now: i64) -> Prepared {
    app.prepare(
        PrepareRequest {
            id: id.into(),
            persona: ACTOR.into(),
            work: work.into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        now,
    )
    .unwrap()
}

pub(super) fn decision(prepared: &Prepared, mut operations: Vec<Value>, reason: &str) -> Value {
    let supplied = control(prepared);
    if supplied["own_acceptance"].is_null() {
        let offer = supplied["own_pending_offers"]
            .as_array()
            .unwrap()
            .first()
            .unwrap();
        operations.insert(0, json!({"kind":"responsibility","id":offer["id"],
            "work_id":prepared.request.work,"terms_revision":offer["terms_revision"],
            "disposition":"accept","reason":"Accept the exact supplied synthetic fixture responsibility"}));
    }
    json!({"version":1,"invocation_id":prepared.request.id,"epoch":prepared.epoch,
        "work":{"kind":"ongoing","reason":reason,"evidence":[]},
        "fragment_maintenance":{"kind":"no_change","reason":"No synthetic fragment revision"},
        "asset_maintenance":{"kind":"no_change","reason":"No synthetic asset revision"},
        "next_fragments":{"kind":"clear","reason":"No optional fragment selection"},
        "next_assets":{"kind":"clear","reason":"No optional asset selection"},
        "continuation":{"kind":"immediate","purpose":"Consume the explicitly requested synthetic retrieval"},
        "reply":{"kind":"none","reason":"No synthetic message"},"operations":operations})
}

pub(super) fn submit(
    app: &mut Runtime,
    prepared: &Prepared,
    operations: Vec<Value>,
    reason: &str,
    now: i64,
) -> String {
    // Formatting is intentionally noncanonical; archive retrieval must preserve it.
    let raw = format!(
        "  {}\n",
        serde_json::to_string_pretty(&decision(prepared, operations, reason)).unwrap()
    );
    let report = app
        .submit_synthetic(&prepared.request.id, raw.as_bytes(), now)
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    raw
}

pub(super) fn control(prepared: &Prepared) -> Value {
    serde_json::from_str(
        prepared
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

pub(super) fn supplied(prepared: &Prepared, kind: &str, operation: &str) -> Value {
    let expected = observation_id(&prepared.request.id, kind, operation);
    // The observation is produced by the preceding invocation, so select its
    // operation suffix via the retained payload's origin instead of this request.
    let values = control(prepared);
    let candidates = values["actual_observations"].as_array().unwrap();
    candidates
        .iter()
        .find(|value| {
            value["kind"] == kind
                && (value["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with(operation))
                    || value["id"] == expected)
        })
        .or_else(|| {
            let mut values = candidates.iter().filter(|value| value["kind"] == kind);
            let value = values.next()?;
            values.next().is_none().then_some(value)
        })
        .unwrap_or_else(|| {
            panic!(
                "missing {kind}/{operation} in {}",
                values["actual_observations"]
            )
        })
        .clone()
}

pub(super) fn query(kind: &str, id: &str, needle: &str) -> Value {
    json!({"kind":"history_records_query", "id":id, "record_kind":kind,
        "mode":{"kind":"literal","needle":needle}, "limit":16, "cursor":null})
}

#[test]
fn consumed_observation_is_selectively_retrievable_across_work_and_reopen() {
    let (directory, mut app) = fixture();
    let produce = prepare(&mut app, "observation-produce", FIRST, 100);
    submit(
        &mut app,
        &produce,
        vec![json!({"kind":"history_query","id":"old-probe",
        "mode":{"kind":"literal","needle":"synthetic-absent-message"},"limit":1,"cursor":null})],
        "Create one synthetic historical observation",
        101,
    );
    let old_id = observation_id("observation-produce", "history_query", "old-probe");
    let consume = prepare(&mut app, "observation-consume", FIRST, 102);
    assert!(consume.supplied_observations.contains(&old_id));
    submit(
        &mut app,
        &consume,
        vec![],
        "Consume the original observation",
        103,
    );
    let (_, before) = app.load().unwrap();
    assert!(!before.personas[ACTOR].focus[FIRST]
        .pending_observations
        .contains(&old_id));
    let original = before.observations[&old_id].clone();
    assert_eq!(
        launch::start(&mut app, "Second synthetic history request").unwrap(),
        SECOND
    );
    let search = prepare(&mut app, "observation-search", SECOND, 104);
    assert!(!search.supplied_observations.contains(&old_id));
    submit(
        &mut app,
        &search,
        vec![query("observation", "find-old", &old_id)],
        "Find consumed observation",
        105,
    );
    let read = prepare(&mut app, "observation-read", SECOND, 106);
    let search_result = supplied(&read, "history_records_query", "find-old");
    assert_eq!(search_result["payload"]["status"], "complete");
    let hits = search_result["payload"]["records"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["work"], FIRST);
    assert_eq!(hits[0]["actor"], ACTOR);
    assert_eq!(hits[0]["producing_invocation"], "observation-produce");
    let reference = hits[0]["reference"].clone();
    submit(
        &mut app,
        &read,
        vec![json!({"kind":"history_record_read","id":"read-old","reference":reference})],
        "Read the exact consumed record",
        107,
    );
    let receipt_id = observation_id("observation-read", "history_record_read", "read-old");
    let (_, retained) = app.load().unwrap();
    let receipt = &retained.observations[&receipt_id];
    assert_eq!(receipt.payload["reference"], reference);
    assert!(receipt.payload.get("historical_record").is_none());
    assert!(receipt.payload["record"].is_null());
    assert!(!serde_json::to_string(&receipt.payload)
        .unwrap()
        .contains("synthetic-absent-message"));
    assert_eq!(
        serde_json::to_value(&retained.observations[&old_id]).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    drop(app);
    let mut app = Runtime::open(&directory.path().join("state")).unwrap();
    let fresh = prepare(&mut app, "observation-fresh", SECOND, 108);
    let projected = supplied(&fresh, "history_record_read", "read-old");
    assert_eq!(projected["payload"]["status"], "read");
    assert_eq!(projected["payload"]["record"]["reference"], reference);
    let historical = &projected["payload"]["record"]["historical_record"];
    assert_eq!(historical["id"], old_id);
    assert_eq!(historical["work"], FIRST);
    assert_eq!(historical["actor"], ACTOR);
    assert_eq!(historical["invocation"], "observation-produce");
    assert_eq!(historical["payload"]["status"], "no_match");
    assert_eq!(projected["payload"]["record"]["historical_only"], true);
    assert_eq!(
        projected["payload"]["record"]["operative_evidence_or_event"],
        false
    );
    assert!(fresh.supplied_observations.contains(&receipt_id));
    assert!(!fresh.supplied_observations.contains(&old_id));
    assert!(projected["current_required_qualifications"]
        .as_array()
        .unwrap()
        .is_empty());
    let (_, after) = app.load().unwrap();
    assert_eq!(
        serde_json::to_value(&after.personas[ACTOR].character).unwrap(),
        serde_json::to_value(&before.personas[ACTOR].character).unwrap()
    );
    assert!(after.personas[ACTOR].focus[SECOND]
        .fragments
        .as_ref()
        .is_none_or(Vec::is_empty));
    assert!(after.personas[ACTOR].focus[SECOND]
        .assets
        .as_ref()
        .is_none_or(Vec::is_empty));
}

#[test]
fn adopted_authored_decision_preserves_raw_bytes_and_exact_archive_identity() {
    let (directory, mut app) = fixture();
    let original = prepare(&mut app, "decision-origin", FIRST, 100);
    let raw = submit(
        &mut app,
        &original,
        vec![],
        "unique-historical-authored-decision",
        101,
    );
    let object = app
        .store
        .read(&archive_id("decision-origin"))
        .unwrap()
        .unwrap();
    let exact = object.exact_ref().unwrap();
    let proof = app.store.retained_decision("decision-origin").unwrap();
    assert!(proof.objects.contains(&exact));
    assert_eq!(
        launch::start(&mut app, "Second synthetic history request").unwrap(),
        SECOND
    );
    let search = prepare(&mut app, "decision-search", SECOND, 102);
    submit(
        &mut app,
        &search,
        vec![query(
            "adopted_decision",
            "find-decision",
            "unique-historical-authored-decision",
        )],
        "Find an earlier authored Decision",
        103,
    );
    let read = prepare(&mut app, "decision-read", SECOND, 104);
    let result = supplied(&read, "history_records_query", "find-decision");
    assert_eq!(result["payload"]["status"], "complete");
    let hits = result["payload"]["records"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["work"], FIRST);
    let reference = hits[0]["reference"].clone();
    assert_eq!(reference["invocation_id"], "decision-origin");
    assert_eq!(reference["archive_id"], exact.id);
    assert_eq!(reference["revision"], exact.revision);
    assert_eq!(reference["sha256"], exact.sha256);
    assert_eq!(
        reference["response_sha256"],
        s::sha256_bytes(raw.as_bytes())
    );
    assert_ne!(reference["sha256"], reference["response_sha256"]);
    submit(
        &mut app,
        &read,
        vec![json!({"kind":"history_record_read","id":"read-decision","reference":reference})],
        "Retrieve exact authored bytes",
        105,
    );
    let receipt_id = observation_id("decision-read", "history_record_read", "read-decision");
    let (_, retained) = app.load().unwrap();
    assert!(retained.observations[&receipt_id].payload["record"].is_null());
    assert!(!serde_json::to_string(&retained.observations[&receipt_id])
        .unwrap()
        .contains("unique-historical-authored-decision"));
    drop(app);
    let mut app = Runtime::open(&directory.path().join("state")).unwrap();
    let fresh = prepare(&mut app, "decision-fresh", SECOND, 106);
    let result = supplied(&fresh, "history_record_read", "read-decision");
    assert_eq!(result["payload"]["status"], "read");
    let body = &result["payload"]["record"]["historical_record"];
    assert_eq!(body["authored_json"], raw);
    assert_eq!(body["actor"], ACTOR);
    assert_eq!(body["work"], FIRST);
    assert_eq!(body["invocation"], "decision-origin");
    assert_eq!(body["authorship_establishes_execution"], false);
    assert_eq!(body["provider_metadata_supplied"], false);
    assert_eq!(
        body["operation_dispositions"],
        "unavailable_original_disclosure_provenance"
    );
    assert!(body.get("provider_observation").is_none());
    assert!(body.get("report").is_none());
    assert!(fresh.supplied_observations.contains(&receipt_id));
    assert!(!fresh
        .supplied_observations
        .contains(&"decision-origin".to_owned()));
}

#[test]
fn missing_and_wrong_observation_identities_produce_unavailable_receipts() {
    for missing in [true, false] {
        let (_directory, mut app) = fixture();
        if !missing {
            let seed = prepare(&mut app, "wrong-hash-seed", FIRST, 98);
            submit(
                &mut app,
                &seed,
                vec![json!({"kind":"history_query", "id":"seed-record",
                "mode":{"kind":"listing"}, "limit":1, "cursor":null})],
                "Create the exact record for the wrong-hash check",
                99,
            );
        }
        let prepared = prepare(
            &mut app,
            if missing {
                "missing-reference"
            } else {
                "wrong-reference"
            },
            FIRST,
            100,
        );
        let (_, state) = app.load().unwrap();
        let id = if missing {
            "not-a-retained-observation".into()
        } else {
            state.observations.keys().next().unwrap().clone()
        };
        let reference = json!({"kind":"observation","observation_id":id,"sha256":"0".repeat(64)});
        submit(
            &mut app,
            &prepared,
            vec![
                json!({"kind":"history_record_read","id":"read-unavailable","reference":reference}),
            ],
            "Exercise unavailable historical identity",
            101,
        );
        let fresh = prepare(&mut app, "unavailable-result", FIRST, 102);
        let result = supplied(&fresh, "history_record_read", "read-unavailable");
        assert_eq!(result["payload"]["status"], "unavailable");
        assert_eq!(result["payload"]["body_supplied"], false);
        assert!(result["payload"]["record"].is_null());
    }
}

#[test]
fn legacy_prepared_admission_does_not_gain_record_retrieval_capability() {
    let (_directory, mut app) = fixture();
    let prepared = prepare(&mut app, "legacy-history", FIRST, 100);
    let mut legacy = serde_json::to_value(&prepared).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("history_records_interface");
    let legacy: Prepared = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.history_records_interface, None);
    let (_, mut state) = app.load().unwrap();
    let before = serde_json::to_value(&state).unwrap();
    let exposure = prepared.exposure.as_ref().unwrap();
    let mut planning = PlanningExposure::new(Some(exposure));
    let mut navigation = f::NavigationBudget::new(prepared.navigation);
    let error = apply(
        &mut state,
        &legacy,
        &p::Operation::HistoryRecordsQuery {
            id: "forbidden-new-query".into(),
            record_kind: p::HistoryRecordKind::Observation,
            mode: f::SearchMode::Listing,
            limit: 1,
            cursor: None,
        },
        &mut navigation,
        0,
        exposure,
        &mut planning,
        101,
        &mut app.store,
        &mut Material::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("not offered"), "{error}");
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    assert_eq!(navigation.used(), f::NavigationUsage::default());
}

#[test]
fn record_query_cursor_survives_reopen_and_keeps_original_membership() {
    let (directory, mut app) = fixture();
    let seed = prepare(&mut app, "cursor-seed", FIRST, 98);
    submit(
        &mut app,
        &seed,
        vec![json!({"kind":"history_query", "id":"seed-page-record",
        "mode":{"kind":"listing"}, "limit":1, "cursor":null})],
        "Retain a record and its actual accounting observation",
        99,
    );
    let first = prepare(&mut app, "cursor-first", FIRST, 100);
    submit(
        &mut app,
        &first,
        vec![json!({"kind":"history_records_query", "id":"page-one",
        "record_kind":"observation", "mode":{"kind":"listing"}, "limit":1, "cursor":null})],
        "Request a bounded first page",
        101,
    );
    let (_, retained) = app.load().unwrap();
    let receipt_id = observation_id("cursor-first", "history_records_query", "page-one");
    let receipt = &retained.observations[&receipt_id];
    assert_eq!(receipt.payload["status"], "incomplete");
    assert_eq!(receipt.payload["records"].as_array().unwrap().len(), 1);
    let handle = receipt.payload["next_cursor"].as_str().unwrap().to_owned();
    let snapshot = retained.personas[ACTOR].focus[FIRST].history_cursors[&handle]
        .records
        .clone()
        .unwrap();
    assert!(snapshot.len() > 1);
    assert!(!snapshot.iter().any(|reference| matches!(reference,
        p::HistoryRecordReference::Observation { observation_id, .. } if observation_id == &receipt_id)));
    drop(app);
    let mut app = Runtime::open(&directory.path().join("state")).unwrap();
    let next = prepare(&mut app, "cursor-next", FIRST, 102);
    assert!(next.supplied_observations.contains(&receipt_id));
    submit(
        &mut app,
        &next,
        vec![json!({"kind":"history_records_query", "id":"page-two",
        "record_kind":"observation", "mode":{"kind":"listing"}, "limit":1, "cursor":handle})],
        "Read the next page of the retained snapshot",
        103,
    );
    let fresh = prepare(&mut app, "cursor-result", FIRST, 104);
    let result = supplied(&fresh, "history_records_query", "page-two");
    assert_ne!(result["payload"]["status"], "stale_cursor");
    assert_eq!(
        result["payload"]["records"][0]["reference"],
        serde_json::to_value(&snapshot[1]).unwrap()
    );
    let (_, after) = app.load().unwrap();
    assert!(!after.personas[ACTOR].focus[FIRST]
        .history_cursors
        .contains_key(&handle));
}
