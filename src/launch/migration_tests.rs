//! Independent synthetic fixtures. No sibling launch::tests helpers, real
//! credentials, provider construction, or model calls are used here.
use super::*;

fn provider_settings(model: &str) -> DirectProviderSettings {
    serde_json::from_value(json!({
        "mode":"platform_api_key", "model":model, "effort":"medium",
        "credentials":{"kind":"api_key",
            "key_env":{"env":"UNREAD_MIGRATION_FIXTURE_KEY"},
            "account_env":{"env":"UNREAD_MIGRATION_FIXTURE_ACCOUNT"}},
        "evidence":{"mode":"platform_api_key", "model":model, "efforts":["medium"],
            "account_env":{"env":"UNREAD_MIGRATION_FIXTURE_EVIDENCE_ACCOUNT"},
            "account_sha256":"3".repeat(64), "transport_profile":"platform_responses_v2",
            "catalog_evidence_sha256":"1".repeat(64),
            "structured_output_evidence_sha256":"2".repeat(64)},
        "limits":{"timeout_ms":1000, "max_prompt_bytes":196608,
            "max_schema_bytes":262144, "max_schema_nodes":65536, "max_schema_depth":64,
            "max_request_bytes":2097152, "max_response_bytes":32768,
            "max_stream_bytes":131072, "max_event_bytes":98304, "max_events":1024}
    }))
    .unwrap()
}

fn fixture(count: usize) -> (tempfile::TempDir, Runtime) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("working")).unwrap();
    let app = setup(
        &root.path().join("state"),
        "requester",
        Setup {
            cohort: count,
            owner: 1,
            destination: "fixture".into(),
            provider: provider_settings("old-model"),
            working_directory: root.path().join("working"),
            share_cohort: true,
            max_steps: 8,
            max_considerations: 128,
            max_work_attempts: 32,
            max_total_steps: 32,
            max_works: 4,
        },
        true,
    )
    .unwrap();
    (root, app)
}

fn request(app: &mut Runtime, id: &str, model: &str) -> ProviderMigrationRequest {
    let snapshot = migration_snapshot(app).unwrap();
    ProviderMigrationRequest {
        id: id.into(),
        operator: snapshot.operator,
        reason: "Explicit synthetic route renewal.".into(),
        expected_runtime: snapshot.runtime,
        expected_launch: snapshot.launch,
        expected_persona_bindings: snapshot.persona_bindings,
        provider: provider_settings(model),
        allow_direct_provider: true,
        model_confirmed: true,
        authorized_destination: snapshot.destination,
    }
}

fn heads(app: &mut Runtime) -> (s::StoredObject, s::StoredObject) {
    (app.load().unwrap().0, load(app).unwrap().0)
}

fn edit_state(app: &mut Runtime, edit: impl FnOnce(&mut State)) {
    let (old, mut state) = app.load().unwrap();
    edit(&mut state);
    app.save(&old, &state).unwrap();
}

fn connection(app: &Runtime) -> rusqlite::Connection {
    rusqlite::Connection::open(app.state_root().join("state.sqlite3")).unwrap()
}

type Cell = (String, String, Vec<u8>, i64, i64, i64);
fn cells(app: &Runtime) -> Vec<Cell> {
    let db = connection(app);
    let mut statement = db
        .prepare("SELECT ns,k,data,capacity,lane,slots FROM store_cells ORDER BY ns,k")
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<Cell>, _>>()
        .unwrap()
}

fn no_funding(before: &s::LedgerReport, after: &s::LedgerReport) {
    assert_eq!(before.limits, after.limits);
    assert_eq!(before.frozen, after.frozen);
    for (a, b) in [
        (&before.ordinary, &after.ordinary),
        (&before.closeout, &after.closeout),
    ] {
        assert_eq!(a.reserved, b.reserved);
        assert_eq!(a.uncertain, b.uncertain);
        assert_eq!(a.consumed.attempts, b.consumed.attempts);
        assert_eq!(a.consumed.input_bytes, b.consumed.input_bytes);
        assert_eq!(a.consumed.output_bytes, b.consumed.output_bytes);
        assert!(b.consumed.navigation_bytes >= a.consumed.navigation_bytes);
        assert!(b.consumed.retained_bytes >= a.consumed.retained_bytes);
    }
}

// Only use when no orphan Dispatched record is awaiting normal owner recovery.
fn rejected(app: &mut Runtime, request: &ProviderMigrationRequest) {
    let old = heads(app);
    let records = cells(app);
    let epoch = app.epoch().unwrap();
    let ledger = app.ledger().unwrap();
    assert!(preview_provider_migration(app, request, 10).is_err());
    assert!(migrate_provider(app, request, 10).is_err());
    assert_eq!(heads(app), old);
    assert_eq!(cells(app), records);
    assert_eq!(app.epoch().unwrap(), epoch);
    assert!(app
        .store
        .read(&format!("provider-migration:{}", request.id))
        .unwrap()
        .is_none());
    no_funding(&ledger, &app.ledger().unwrap());
}

fn command(id: &str, action: WorkAction, dependencies: &[&str]) -> p::Operation {
    p::Operation::Effect {
        id: id.into(),
        capability_id: "work.command.v1".into(),
        binding_revision: work_binding(),
        inputs: serde_json::to_value(action).unwrap(),
        depends_on: dependencies.iter().map(|id| (*id).into()).collect(),
    }
}

// Exercise real synthetic adoption: own voice, character, responsibility,
// selected asset, evidence, delivery, messages, and later explicit human consent.
#[derive(Default)]
struct HistoryDriver {
    stage: usize,
}
impl Driver for HistoryDriver {
    async fn step(
        &mut self,
        app: &mut Runtime,
        mut request: PrepareRequest,
        _saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        request.profile = Profile::Synthetic;
        let prepared = app.prepare(request, now)?;
        let mut decision: p::Decision = serde_json::from_value(json!({
            "version":1, "invocation_id":prepared.request.id, "epoch":prepared.epoch,
            "work":{"kind":"ongoing","reason":"Synthetic unfinished work.","evidence":[]},
            "fragment_maintenance":{"kind":"no_change","reason":"Retain."},
            "asset_maintenance":{"kind":"no_change","reason":"Retain."},
            "next_fragments":{"kind":"clear","reason":"No optional fragments."},
            "next_assets":{"kind":"clear","reason":"No optional assets."},
            "continuation":{"kind":"stop","reason":"Explicit stop."},
            "reply":{"kind":"authored","audience":["requester"],"text":"Synthetic retained message."},
            "operations":[]
        }))?;
        if prepared.character_revision.is_none() {
            let source = format!("voice-{}", prepared.request.persona);
            decision.operations = serde_json::from_value(json!([
                {"kind":"file_write","id":"voice","scope":{"kind":"fragments"},
                 "source":source,"path":"voice.txt","base_revision":null,
                 "text":"My accepted synthetic own voice.","qualifications":[],"policy_id":POLICY},
                {"kind":"character_adopt","id":"character","base_revision":null,
                 "ocean":{"openness":"Modeled","conscientiousness":"Modeled",
                    "extraversion":"Modeled","agreeableness":"Modeled","neuroticism":"Modeled"},
                 "vad":{"valence":"Modeled","activation":"Modeled","control":"Modeled"},
                 "own_voice":[{"source":source,"revision":"proposal-op:voice","representation":"utf8"}],
                 "reason":"Explicit initial account.","evidence":[]}
            ]))?;
            decision.fragment_maintenance = p::Maintenance::Change {
                reason: "Initial character.".into(),
                operation_ids: vec!["voice".into(), "character".into()],
            };
        } else {
            let state = app.snapshot()?;
            let work = state
                .board
                .work(&w::WorkId(prepared.request.work.clone()))?;
            if let Some((id, _)) = work.offers.iter().find(|(_, offer)| {
                offer.to.0 == prepared.request.persona
                    && matches!(offer.status, w::OfferStatus::Pending)
            }) {
                decision.operations.push(p::Operation::Responsibility {
                    id: id.0.clone(),
                    work_id: prepared.request.work.clone(),
                    terms_revision: offer_revision(work, id)?,
                    disposition: "accept".into(),
                    reason: "Accept exact fixture terms.".into(),
                });
            }
            if prepared.request.persona == "persona-0001" {
                decision.continuation = p::Continuation::Immediate {
                    purpose: "Finish fixture.".into(),
                };
                match self.stage {
                    0 => {
                        let subject = p::ExactRef {
                            source: "result-source".into(),
                            revision: "proposal-op:save-result".into(),
                            representation: "utf8".into(),
                            start: None,
                            end: None,
                            lifetime: None,
                        };
                        decision.operations.push(p::Operation::FileWrite {
                            id: "save-result".into(),
                            scope: p::Scope::Assets {},
                            source: "result-source".into(),
                            path: "result.txt".into(),
                            base_revision: None,
                            text: "The complete synthetic result.".into(),
                            qualifications: Vec::new(),
                            policy_id: POLICY.into(),
                        });
                        decision.operations.push(command(
                            "register-result",
                            WorkAction::PutArtifact {
                                artifact: "result-artifact".into(),
                                base: None,
                                subject: subject.clone(),
                            },
                            &["save-result"],
                        ));
                        decision.asset_maintenance = p::Maintenance::Change {
                            reason: "Retain result.".into(),
                            operation_ids: vec!["save-result".into()],
                        };
                        decision.next_assets = p::SelectionIntent::Replace {
                            reason: "Examine exact body next.".into(),
                            references: vec![subject],
                        };
                    }
                    1 => {
                        assert!(prepared.prompt.contains("The complete synthetic result."));
                        let artifact = &work.artifacts[&w::ArtifactId("result-artifact".into())];
                        let key = ArtifactKey::from_native(
                            &artifact.revisions[&artifact.current].reference,
                        );
                        let artifacts = BTreeMap::from([("result".into(), key.clone())]);
                        decision.operations.extend([
                            command("evidence", WorkAction::RecordModelEvidence {
                                id: "result-evidence".into(), artifact: key, outcome: "result".into(),
                                explanation: "Examined the supplied exact body.".into(),
                                limitations: vec!["Synthetic attributed judgment only.".into()],
                            }, &[]),
                            command("judge", WorkAction::Judge {
                                id: "result-judgment".into(), artifacts: artifacts.clone(), sufficient: true,
                                claims: BTreeMap::from([("result".into(), ClaimInput {
                                    met: true, explanation: "Exact fixture text satisfies the writing criterion.".into(),
                                    evidence: BTreeSet::from(["result-evidence".into()]),
                                })]), explanation: "Synthetic judgment.".into(),
                            }, &[]),
                            command("share", WorkAction::DecideShared { id: "result-shared".into(), artifacts }, &[]),
                            command("release", WorkAction::Release { id: "result-release".into(), shared: "result-shared".into() }, &[]),
                            command("deliver", WorkAction::DeliverRelease {
                                release: "result-release".into(), recipients: BTreeSet::from(["requester".into()]),
                            }, &[]),
                        ]);
                        decision.next_assets = serde_json::from_value(json!({
                            "kind":"preserve","reason":"Retain the exact selection."
                        }))?;
                    }
                    _ => panic!("acceptance boundary should stop fixture execution"),
                }
                self.stage += 1;
            }
        }
        let report =
            app.submit_synthetic(&prepared.request.id, &serde_json::to_vec(&decision)?, now)?;
        assert_ne!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
        Ok(report)
    }
}

async fn history(app: &mut Runtime) -> String {
    let work = start_with_acceptance(app, "Write the complete synthetic result.", true).unwrap();
    let view = run_with(app, &work, &mut HistoryDriver::default())
        .await
        .unwrap();
    assert_eq!(view.status, "delivered-acceptance-unestablished");
    assert_eq!(view.local_deliveries.len(), 1);
    let target = &view.human_decision_pointers[0];
    let binding = {
        let state = app.snapshot().unwrap();
        let board = state.board.work(&w::WorkId(work.clone())).unwrap();
        WorkAuthority {
            work: work.clone(),
            version: board.version,
            authority: board.mandate().authority.clone(),
        }
    };
    app.record_stakeholder_decision(
        "requester",
        HumanStakeholderDecisionRequest {
            binding,
            id: "fixture-human-consent".into(),
            release: target.release.clone(),
            release_sha256: target.release_sha256.clone(),
            delivery: target.delivery.clone(),
            delivery_receipt: target.delivery_receipt.clone(),
            accepts: true,
            statement: "Explicit synthetic human acceptance of this exact delivery.".into(),
        },
        unix_now().unwrap(),
    )
    .unwrap();
    work
}

#[tokio::test(flavor = "current_thread")]
async fn both_formats_preserve_every_field_outside_the_allowed_delta() {
    for format in [1, 2] {
        let (_root, mut app) = fixture(2);
        let work = history(&mut app).await;
        let (_, saved) = load(&mut app).unwrap();
        let mut visitor = seed_for(&saved, &provider_settings("visitor-model"), "visitor");
        visitor.audience.insert("visitor".into());
        visitor.grants.communicate_with.insert("visitor".into());
        app.add_persona_seed("requester", visitor).unwrap();
        if format == 1 {
            app.pause("requester", &work, "Retain paused work.")
                .unwrap();
        } else {
            app.closeout(
                "requester",
                &work,
                false,
                "Retain ended work without invented success.",
            )
            .unwrap();
        }
        edit_state(&mut app, |state| {
            state
                .personas
                .get_mut("persona-0002")
                .unwrap()
                .seed
                .requested_model = "different-old-model".into();
            state
                .personas
                .get_mut("persona-0002")
                .unwrap()
                .seed
                .requested_effort = "low".into();
            state.work_meta.get_mut(&work).unwrap().attempts_left = 0;
            state.config.allow_nonconformant_research = true;
            if format == 1 {
                state.config.allow_direct_provider = false;
            }
        });
        let (old, mut saved) = load(&mut app).unwrap();
        saved.steps = saved.setup.max_total_steps;
        if format == 1 {
            saved.format = 1;
            saved.research_opt_in = Some(true);
            saved.direct_provider_opt_in = None;
            // Deliberately not a valid legacy or direct settings document.
            saved.setup.provider = json!({"executable":false,"model":17,
                "opaque":"OLD_PRIVATE_PROVIDER_MARKER","unrecognized":[null,{"anything":true}]});
        }
        save(&mut app, Some(&old), &saved).unwrap();
        // Preserve a genuinely omitted default, not merely its decoded value.
        let (mut old, _) = app.load().unwrap();
        assert_eq!(old.body["semantic_warnings"], json!({}));
        let revision = old.revision;
        old.revision += 1;
        old.body
            .as_object_mut()
            .unwrap()
            .remove("semantic_warnings");
        app.store.compare_and_swap(Some(revision), old).unwrap();

        let before = heads(&mut app);
        let before_view = serde_json::to_value(result(&mut app, &work).unwrap()).unwrap();
        let root = app.state_root().to_path_buf();
        let req = request(&mut app, "preserve", "new-model");
        let snapshot = migration_snapshot(&mut app).unwrap();
        assert_eq!(
            snapshot.provider_config_sha256,
            digest(&before.1.body["setup"]["provider"]).unwrap()
        );
        let epoch = app.epoch().unwrap();
        let ledger = app.ledger().unwrap();
        let preview = preview_provider_migration(&mut app, &req, 10).unwrap();
        assert_eq!(heads(&mut app), before);
        assert_eq!(app.epoch().unwrap(), epoch);
        let outcome = migrate_provider(&mut app, &req, 10).unwrap();
        assert!(!outcome.replayed);
        assert_eq!(outcome.receipt, preview);
        assert_eq!(app.epoch().unwrap(), epoch + 1);
        assert_eq!(app.state_root(), root.as_path());

        let after = heads(&mut app);
        let mut expected = before.clone();
        expected.0.revision += 1;
        expected.1.revision += 1;
        expected.0.body["config"]["allow_direct_provider"] = json!(true);
        for id in people(2) {
            let persona = &mut expected.0.body["personas"][&id];
            persona["owner_epoch"] = json!(req.expected_persona_bindings[&id].owner_epoch + 1);
            persona["seed"]["requested_model"] = json!("new-model");
            persona["seed"]["requested_effort"] = json!("medium");
            persona["seed"]["model_confirmed"] = json!(true);
        }
        expected.0.body["operator_events"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "kind":"provider_migration", "id":req.id, "request_sha256":preview.request_sha256,
                "operator":req.operator, "reason":req.reason
            }));
        expected.1.body["format"] = json!(2);
        expected.1.body["direct_provider_opt_in"] = json!(true);
        expected
            .1
            .body
            .as_object_mut()
            .unwrap()
            .remove("research_opt_in");
        expected.1.body["setup"]["provider"] = serde_json::to_value(&req.provider).unwrap();
        assert_eq!(
            after, expected,
            "entire state and launch must obey the exact delta"
        );
        assert_eq!(
            after.0.body["personas"]["visitor"],
            before.0.body["personas"]["visitor"]
        );
        assert_eq!(
            app.store.read_exact(&preview.prior_runtime).unwrap(),
            before.0
        );
        assert_eq!(
            app.store.read_exact(&preview.prior_launch).unwrap(),
            before.1
        );
        assert_eq!(
            app.store.read_exact(&preview.target_runtime).unwrap(),
            after.0
        );
        assert_eq!(
            app.store.read_exact(&preview.target_launch).unwrap(),
            after.1
        );
        assert_eq!(
            serde_json::to_value(result(&mut app, &work).unwrap()).unwrap(),
            before_view
        );
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("OLD_PRIVATE_PROVIDER_MARKER"));
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("UNREAD_MIGRATION"));
        let (_, saved) = load(&mut app).unwrap();
        ensure_cohort(&mut app, &saved).unwrap();
        assert_eq!(
            heads(&mut app),
            after,
            "ensure_cohort must neither create nor save"
        );
        assert_eq!(app.epoch().unwrap(), epoch + 1);
        assert!(reserve(&mut app, &work, "persona-0001").is_err());
        assert!(app
            .prepare(
                PrepareRequest {
                    id: "no-implicit-resume".into(),
                    persona: "persona-0001".into(),
                    work: work.clone(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                10
            )
            .is_err());
        assert_eq!(heads(&mut app), after);
        no_funding(&ledger, &app.ledger().unwrap());
    }
}

#[test]
fn snapshot_and_preview_do_not_take_execution_ownership() {
    let (_root, mut app) = fixture(2);
    let req = request(&mut app, "owner", "new-model");
    let before = heads(&mut app);
    let guard = app.store.acquire_execution_owner().unwrap();
    let ledger = app.ledger().unwrap();
    assert_eq!(
        migration_snapshot(&mut app).unwrap().runtime,
        req.expected_runtime
    );
    let preview = preview_provider_migration(&mut app, &req, 10).unwrap();
    assert!(
        app.ledger().unwrap().ordinary.consumed.navigation_bytes
            > ledger.ordinary.consumed.navigation_bytes
    );
    assert!(migrate_provider(&mut app, &req, 10).is_err());
    assert_eq!(heads(&mut app), before);
    assert!(app
        .store
        .read("provider-migration:owner")
        .unwrap()
        .is_none());
    drop(guard);
    assert_eq!(
        migrate_provider(&mut app, &req, 10).unwrap().receipt,
        preview
    );
}

#[test]
fn complete_eight_person_cohort_and_explicit_route_change() {
    let (_root, mut app) = fixture(8);
    let mut req = request(&mut app, "all-eight", "new-model");
    let mut incomplete = req.clone();
    incomplete.expected_persona_bindings.remove("persona-0008");
    rejected(&mut app, &incomplete);
    req.provider.mode = provider::AccessMode::SiwcOAuth;
    req.provider.evidence.mode = req.provider.mode;
    req.provider.evidence.transport_profile = req.provider.mode.profile().into();
    req.provider.credentials = provider::CredentialReference::OAuth {
        grant: provider::OAuthGrant::SiwcTokenSharing,
        token_env: provider::SecretReference {
            env: "UNREAD_MIGRATION_FIXTURE_OAUTH".into(),
        },
        account_env: provider::SecretReference {
            env: "UNREAD_MIGRATION_FIXTURE_ACCOUNT".into(),
        },
        expires_at_unix: u64::MAX,
    };
    let out = migrate_provider(&mut app, &req, 10).unwrap();
    assert_eq!(out.receipt.after_bindings.len(), 8);
    for (id, binding) in out.receipt.after_bindings {
        assert_eq!(binding.requested_model, "new-model");
        assert_eq!(
            binding.owner_epoch,
            req.expected_persona_bindings[&id].owner_epoch + 1
        );
    }
    let (_, saved) = load(&mut app).unwrap();
    let before = heads(&mut app);
    ensure_cohort(&mut app, &saved).unwrap();
    assert_eq!(heads(&mut app), before);
}

#[test]
fn stale_heads_hashes_and_binding_fences_are_atomic() {
    for case in 0..6 {
        let (_root, mut app) = fixture(2);
        let mut req = request(&mut app, "stale", "new-model");
        match case {
            0 => edit_state(&mut app, |state| {
                state.operator_events.push(json!({"kind":"fixture-change"}))
            }),
            1 => {
                let (old, mut saved) = load(&mut app).unwrap();
                saved.steps += 1;
                save(&mut app, Some(&old), &saved).unwrap();
            }
            2 => req.expected_runtime.sha256 = "0".repeat(64),
            3 => req.expected_launch.sha256 = "0".repeat(64),
            4 => {
                req.expected_persona_bindings
                    .get_mut("persona-0001")
                    .unwrap()
                    .owner_epoch += 1
            }
            5 => {
                let binding = req
                    .expected_persona_bindings
                    .remove("persona-0002")
                    .unwrap();
                req.expected_persona_bindings
                    .insert("visitor".into(), binding);
            }
            _ => unreachable!(),
        }
        rejected(&mut app, &req);
    }
}

#[test]
fn missing_personas_and_changed_static_seed_metadata_are_not_repaired() {
    for case in 0..7 {
        let (_root, mut app) = fixture(2);
        let mut req = request(&mut app, "static", "new-model");
        edit_state(&mut app, |state| {
            if case == 0 {
                state.personas.remove("persona-0002");
                return;
            }
            let seed = &mut state.personas.get_mut("persona-0002").unwrap().seed;
            match case {
                1 => seed.label = "different".into(),
                2 => seed.text = "Different creation material.".into(),
                3 => seed.provenance = "Different provenance.".into(),
                4 => seed.destination = "different".into(),
                5 => {
                    seed.audience.remove("requester");
                }
                6 => seed.grants.character = false,
                _ => unreachable!(),
            }
        });
        req.expected_runtime = heads(&mut app).0.exact_ref().unwrap();
        rejected(&mut app, &req);
    }
}

#[test]
fn bounded_syntax_is_checked_before_any_navigation_even_for_replay() {
    let (_root, mut app) = fixture(2);
    let req = request(&mut app, "syntax", "new-model");
    migrate_provider(&mut app, &req, 10).unwrap();
    for case in 0..25 {
        let mut bad = req.clone();
        let mut now = 11;
        match case {
            0 => bad.id = "x".repeat(97),
            1 => bad.id = "é".repeat(65),
            2 => bad.id = "bad id".into(),
            3 => bad.operator = "x".repeat(129),
            4 => bad.reason = "é".repeat(2049),
            5 => bad.reason = " ".into(),
            6 => bad.allow_direct_provider = false,
            7 => bad.model_confirmed = false,
            8 => now = -1,
            9 => bad.authorized_destination = "x".repeat(129),
            10 => bad.expected_runtime.id = "x".repeat(300),
            11 => bad.expected_launch.revision = 0,
            12 => bad.expected_runtime.sha256 = "A".repeat(64),
            13 => bad.expected_launch.sha256 = "0".repeat(65),
            14 => {
                bad.expected_persona_bindings.remove("persona-0002");
            }
            15 => {
                let binding = bad.expected_persona_bindings["persona-0001"].clone();
                for n in 0..7 {
                    bad.expected_persona_bindings
                        .insert(format!("extra-{n}"), binding.clone());
                }
            }
            16 => {
                bad.expected_persona_bindings
                    .get_mut("persona-0001")
                    .unwrap()
                    .requested_model = "x".repeat(129)
            }
            17 => bad.provider.model = "unsubstantiated-model".into(),
            18 => bad.provider.effort = "unsupported-effort".into(),
            19 => bad.provider.evidence.structured_output_evidence_sha256 = None,
            20 => bad.provider.evidence.account_sha256 = "z".repeat(64),
            21 => bad.provider.evidence.transport_profile = "wrong-profile".into(),
            22 => bad.provider.evidence.account_env.env = "x".repeat(129),
            23 => {
                bad.provider.mode = provider::AccessMode::ChatgptOAuth;
                bad.provider.evidence.mode = bad.provider.mode;
                bad.provider.evidence.transport_profile = bad.provider.mode.profile().into();
            }
            24 => bad.provider.limits.max_prompt_bytes = 0,
            _ => unreachable!(),
        }
        let ledger = app.ledger().unwrap();
        assert!(
            preview_provider_migration(&mut app, &bad, now).is_err(),
            "case {case}"
        );
        assert!(
            migrate_provider(&mut app, &bad, now).is_err(),
            "case {case}"
        );
        assert_eq!(
            app.ledger().unwrap(),
            ledger,
            "syntax failure must precede reads: {case}"
        );
    }
}

#[test]
fn authorization_and_unchanged_runtime_reservations_are_required() {
    let (_root, mut app) = fixture(2);
    let req = request(&mut app, &"x".repeat(96), "new-model");
    let config = app.snapshot().unwrap().config;
    for case in 0..5 {
        let mut bad = req.clone();
        match case {
            0 => bad.operator = "intruder".into(),
            1 => bad.authorized_destination = "different-destination".into(),
            2 => bad.provider.limits.max_prompt_bytes = config.context_bytes + 1,
            3 => bad.provider.limits.max_response_bytes = config.response_bytes + 1,
            4 => {
                bad.provider.limits.max_stream_bytes =
                    config.transport_output_reserve_bytes as usize + 1
            }
            _ => unreachable!(),
        }
        rejected(&mut app, &bad);
    }
    assert!(!migrate_provider(&mut app, &req, 10).unwrap().replayed);
}

#[test]
fn active_markers_including_noncohort_personas_block_every_phase() {
    // These markers isolate the global active-reference gate; they do not claim
    // to execute the Store lifecycle or establish provider evidence.
    for phase in [
        s::InvocationState::Prepared,
        s::InvocationState::Uncertain,
        s::InvocationState::Responded,
        s::InvocationState::Failed,
    ] {
        let (_root, mut app) = fixture(2);
        let (_, saved) = load(&mut app).unwrap();
        let visitor = seed_for(&saved, &provider_settings("visitor-model"), "visitor");
        app.add_persona_seed("requester", visitor).unwrap();
        let marker = s::StoredObject {
            id: "active-fixture-marker".into(),
            revision: 1,
            kind: "synthetic-lifecycle-marker".into(),
            body: json!({"state":phase,"terminal_evidence":"retain this exact synthetic marker"}),
        };
        app.store.compare_and_swap(None, marker.clone()).unwrap();
        edit_state(&mut app, |state| {
            state.personas.get_mut("visitor").unwrap().active_invocation = Some(marker.id.clone());
        });
        let req = request(&mut app, "active", "new-model");
        rejected(&mut app, &req);
        assert_eq!(
            app.store.read_exact(&marker.exact_ref().unwrap()).unwrap(),
            marker
        );
    }
}

#[test]
fn real_orphan_dispatch_recovery_is_separate_from_migration_rejection() {
    let (_root, mut app) = fixture(2);
    let work = start(&mut app, "Synthetic interrupted invocation.").unwrap();
    let (mut input, _) = reserve(&mut app, &work, "persona-0001").unwrap();
    input.profile = Profile::Synthetic;
    let prepared = app.prepare(input, 10).unwrap();
    app.dispatch_synthetic(&prepared.request.id, 10).unwrap();
    let req = request(&mut app, "orphan", "new-model");
    let before = heads(&mut app);
    assert!(preview_provider_migration(&mut app, &req, 10).is_err());
    assert_eq!(
        app.invocation(&prepared.request.id).unwrap().state,
        s::InvocationState::Dispatched
    );
    assert!(app.synthetic_owner.is_some());
    drop(app.synthetic_owner.take()); // Synthetic owner loss, not migration recovery.
    assert!(migrate_provider(&mut app, &req, 10).is_err());
    assert_eq!(heads(&mut app), before);
    assert!(app
        .store
        .read("provider-migration:orphan")
        .unwrap()
        .is_none());
    let recovered = app.invocation(&prepared.request.id).unwrap();
    assert_eq!(recovered.state, s::InvocationState::Uncertain);
    let reservation = app.store.reservation(&recovered.reservation).unwrap();
    assert_eq!(reservation.state, s::ReservationState::Uncertain);
    rejected(&mut app, &req);
    assert_eq!(app.invocation(&prepared.request.id).unwrap(), recovered);
    assert_eq!(app.store.reservation(&reservation.id).unwrap(), reservation);
}

#[test]
fn replay_is_historical_authorized_and_normalized_after_a_later_migration() {
    let (_root, mut app) = fixture(2);
    let first = request(&mut app, "first", "middle-model");
    let receipt = migrate_provider(&mut app, &first, 10).unwrap().receipt;
    let second = request(&mut app, "second", "final-model");
    migrate_provider(&mut app, &second, 20).unwrap();
    let current = heads(&mut app);
    assert_ne!(current.0.exact_ref().unwrap(), receipt.target_runtime);
    let normalized: ProviderMigrationRequest =
        serde_json::from_str(&serde_json::to_string_pretty(&first).unwrap()).unwrap();
    let records = cells(&app);
    let epoch = app.epoch().unwrap();
    let ledger = app.ledger().unwrap();
    let guard = app.store.acquire_execution_owner().unwrap();
    let replay = migrate_provider(&mut app, &normalized, 999).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.receipt, receipt);
    assert_eq!(
        preview_provider_migration(&mut app, &normalized, 1000).unwrap(),
        receipt
    );
    drop(guard);
    for case in 0..5 {
        let mut changed = first.clone();
        match case {
            0 => changed.reason = "Different request.".into(),
            1 => changed.provider = provider_settings("other-model"),
            2 => changed.expected_runtime = current.0.exact_ref().unwrap(),
            3 => {
                changed
                    .expected_persona_bindings
                    .get_mut("persona-0001")
                    .unwrap()
                    .owner_epoch += 1
            }
            4 => changed.operator = "intruder".into(),
            _ => unreachable!(),
        }
        let error = migrate_provider(&mut app, &changed, 1001).unwrap_err();
        if case != 4 {
            assert!(matches!(
                error,
                RuntimeError::Store(s::StoreError::Conflict(_))
            ));
        }
    }
    assert_eq!(heads(&mut app), current);
    assert_eq!(cells(&app), records);
    assert_eq!(app.epoch().unwrap(), epoch);
    let after = app.ledger().unwrap();
    no_funding(&ledger, &after);
    assert_eq!(
        ledger.ordinary.consumed.retained_bytes,
        after.ordinary.consumed.retained_bytes
    );
    assert_eq!(ledger.permanent_records, after.permanent_records);
    assert_eq!(
        ledger.allocated_history_events,
        after.allocated_history_events
    );
}

#[test]
fn receipt_record_kind_version_and_revision_are_validated() {
    for case in 0..3 {
        let (_root, mut app) = fixture(2);
        let req = request(&mut app, "bad-receipt", "new-model");
        let receipt = preview_provider_migration(&mut app, &req, 10).unwrap();
        let mut object = s::StoredObject {
            id: "provider-migration:bad-receipt".into(),
            revision: 1,
            kind: "provider-migration".into(),
            body: serde_json::to_value(receipt).unwrap(),
        };
        if case == 0 {
            object.kind = "different-kind".into();
        }
        if case == 1 {
            object.body["version"] = json!(2);
        }
        app.store.compare_and_swap(None, object.clone()).unwrap();
        if case == 2 {
            object.revision = 2;
            app.store.compare_and_swap(Some(1), object).unwrap();
        }
        let records = cells(&app);
        assert!(preview_provider_migration(&mut app, &req, 10).is_err());
        assert!(migrate_provider(&mut app, &req, 10).is_err());
        assert_eq!(cells(&app), records);
    }
}

#[test]
fn late_receipt_insert_failure_rolls_back_both_heads_and_all_revisions() {
    let (_root, mut app) = fixture(2);
    let req = request(&mut app, "atomic", "new-model");
    let planned = preview_provider_migration(&mut app, &req, 10).unwrap();
    let before = heads(&mut app);
    let records = cells(&app);
    let epoch = app.epoch().unwrap();
    let ledger = app.ledger().unwrap();
    let db = connection(&app);
    // Use the receipt's immutable revision insert: Store's staged object heads
    // are already visible at this point, including with namespace-ordered flush.
    db.execute_batch(&format!(
        r#"
        CREATE TRIGGER migration_abort_receipt BEFORE INSERT ON store_cells
        WHEN NEW.ns='revision'
          AND json_extract(CAST(NEW.data AS TEXT),'$.id')='provider-migration:atomic'
        BEGIN
          SELECT CASE WHEN
            (SELECT json_extract(CAST(data AS TEXT),'$.revision') FROM store_cells
             WHERE ns='object' AND k='runtime-state-v1')={}
            AND
            (SELECT json_extract(CAST(data AS TEXT),'$.revision') FROM store_cells
             WHERE ns='object' AND k='runtime-launch-v1')={}
          THEN RAISE(ABORT,'migration fixture late receipt failure')
          ELSE RAISE(ABORT,'migration fixture did not reach both heads') END;
        END;
    "#,
        planned.target_runtime.revision, planned.target_launch.revision
    ))
    .unwrap();
    let error = migrate_provider(&mut app, &req, 10).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("migration fixture late receipt failure"),
        "{error}"
    );
    assert_eq!(heads(&mut app), before);
    assert_eq!(cells(&app), records);
    assert_eq!(app.epoch().unwrap(), epoch);
    assert!(app
        .store
        .read("provider-migration:atomic")
        .unwrap()
        .is_none());
    assert!(app.store.read_exact(&planned.target_runtime).is_err());
    assert!(app.store.read_exact(&planned.target_launch).is_err());
    let after = app.ledger().unwrap();
    no_funding(&ledger, &after);
    assert_eq!(ledger.permanent_records, after.permanent_records);
    assert_eq!(
        ledger.allocated_history_events,
        after.allocated_history_events
    );
    assert_eq!(
        ledger.ordinary.consumed.retained_bytes,
        after.ordinary.consumed.retained_bytes
    );
    db.execute_batch("DROP TRIGGER migration_abort_receipt")
        .unwrap();
    assert_eq!(
        migrate_provider(&mut app, &req, 10).unwrap().receipt,
        planned
    );
}

#[test]
fn frozen_and_retention_exhausted_stores_use_ordinary_admission() {
    for frozen in [true, false] {
        let (_root, mut app) = fixture(2);
        let req = request(&mut app, "capacity", "new-model");
        if frozen {
            let reservation = app
                .store
                .reserve(
                    "fixture-overrun",
                    s::Resources {
                        attempts: 1,
                        ..s::Resources::default()
                    },
                    s::BudgetClass::Ordinary,
                )
                .unwrap();
            assert!(matches!(
                app.store.settle(
                    &reservation.id,
                    s::Usage::Measured(s::Resources {
                        attempts: 2,
                        ..s::Resources::default()
                    })
                ),
                Err(s::StoreError::Overrun(_))
            ));
            assert!(app.ledger().unwrap().frozen);
        } else {
            // Public reservations consume existing capacity; failed reservations
            // roll back. Descending sizes leave less than another ordinary
            // control allocation, and hence less than a new receipt object.
            for bit in (0..32u32).rev() {
                match app.store.reserve(
                    &format!("fixture-retention-{bit}"),
                    s::Resources {
                        retained_bytes: 1u64 << bit,
                        ..s::Resources::default()
                    },
                    s::BudgetClass::Ordinary,
                ) {
                    Ok(_) => (),
                    Err(s::StoreError::Quota {
                        dimension: s::Resource::RetainedBytes,
                        class: s::BudgetClass::Ordinary,
                    }) => (),
                    Err(error) => panic!("unexpected capacity fixture failure: {error}"),
                }
            }
            assert!(!app.ledger().unwrap().frozen);
        }
        let records = cells(&app);
        let epoch = app.epoch().unwrap();
        let ledger = app.ledger().unwrap();
        let error = migrate_provider(&mut app, &req, 10).unwrap_err();
        if frozen {
            assert!(matches!(error, RuntimeError::Store(s::StoreError::Frozen)));
        } else {
            assert!(matches!(
                error,
                RuntimeError::Store(s::StoreError::Quota {
                    dimension: s::Resource::RetainedBytes,
                    class: s::BudgetClass::Ordinary
                })
            ));
        }
        assert_eq!(cells(&app), records);
        assert_eq!(app.epoch().unwrap(), epoch);
        no_funding(&ledger, &app.ledger().unwrap());
    }
}

#[test]
fn owner_epoch_overflow_rejects_the_entire_cohort() {
    let (_root, mut app) = fixture(2);
    edit_state(&mut app, |state| {
        state.personas.get_mut("persona-0002").unwrap().owner_epoch = u64::MAX;
    });
    let req = request(&mut app, "overflow", "new-model");
    rejected(&mut app, &req);
}

// Additional review regression cases.

#[test]
fn same_model_account_and_expiry_renewals_preserve_state_except_owner_fences() {
    let (_root, mut app) = fixture(2);
    let work = start(&mut app, "Retain paused work across route renewals.").unwrap();
    app.pause("requester", &work, "No implicit resume during renewal.")
        .unwrap();
    edit_state(&mut app, |state| {
        state.work_meta.get_mut(&work).unwrap().attempts_left = 7;
    });
    let (old, mut saved) = load(&mut app).unwrap();
    saved.steps = 3;
    save(&mut app, Some(&old), &saved).unwrap();

    // Explicit synthetic references only; no environment lookup or provider construction.
    let mut account = provider_settings("old-model");
    account.evidence.account_sha256 = "4".repeat(64);
    account.evidence.catalog_evidence_sha256 = "5".repeat(64);
    account.evidence.account_env.env = "UNREAD_MIGRATION_RENEWED_ACCOUNT".into();
    account.credentials = provider::CredentialReference::ApiKey {
        key_env: provider::SecretReference {
            env: "UNREAD_MIGRATION_RENEWED_KEY".into(),
        },
        account_env: provider::SecretReference {
            env: "UNREAD_MIGRATION_RENEWED_ACCOUNT".into(),
        },
    };
    let mut oauth = account.clone();
    oauth.mode = provider::AccessMode::SiwcOAuth;
    oauth.evidence.mode = oauth.mode;
    oauth.evidence.transport_profile = oauth.mode.profile().into();
    oauth.credentials = provider::CredentialReference::OAuth {
        grant: provider::OAuthGrant::SiwcTokenSharing,
        token_env: provider::SecretReference {
            env: "UNREAD_MIGRATION_RENEWED_OAUTH".into(),
        },
        account_env: provider::SecretReference {
            env: "UNREAD_MIGRATION_RENEWED_ACCOUNT".into(),
        },
        expires_at_unix: 100,
    };
    let mut expiry = oauth.clone();
    if let provider::CredentialReference::OAuth {
        expires_at_unix, ..
    } = &mut expiry.credentials
    {
        *expires_at_unix = 200;
    } else {
        unreachable!();
    }

    for (id, target, now) in [
        ("account-renewal", account, 10),
        ("oauth-route", oauth, 20),
        ("expiry-renewal", expiry, 30),
    ] {
        let mut req = request(&mut app, id, "old-model");
        req.provider = target;
        let before = heads(&mut app);
        let epoch = app.epoch().unwrap();
        let ledger = app.ledger().unwrap();
        let preview = preview_provider_migration(&mut app, &req, now).unwrap();
        assert_eq!(heads(&mut app), before);
        assert_eq!(app.epoch().unwrap(), epoch);
        let outcome = migrate_provider(&mut app, &req, now).unwrap();
        assert!(!outcome.replayed);
        let receipt = outcome.receipt;
        assert_eq!(receipt, preview);
        assert_eq!(receipt.target_model, "old-model");
        assert_eq!(receipt.target_effort, "medium");
        assert_eq!(receipt.before_bindings, req.expected_persona_bindings);
        assert_ne!(receipt.old_provider_sha256, receipt.new_provider_sha256);
        assert_eq!(
            receipt.old_provider_sha256,
            digest(&before.1.body["setup"]["provider"]).unwrap()
        );

        let after = heads(&mut app);
        let mut expected = before.clone();
        expected.0.revision += 1;
        expected.1.revision += 1;
        for (persona, binding) in &req.expected_persona_bindings {
            let mut expected_persona = before.0.body["personas"][persona.as_str()].clone();
            expected_persona["owner_epoch"] = json!(binding.owner_epoch + 1);
            assert_eq!(after.0.body["personas"][persona.as_str()], expected_persona);
            expected.0.body["personas"][persona.as_str()] = expected_persona;
            let mut expected_binding = binding.clone();
            expected_binding.owner_epoch += 1;
            assert_eq!(receipt.after_bindings[persona], expected_binding);
        }
        expected.0.body["operator_events"].as_array_mut().unwrap().push(json!({
            "kind":"provider_migration", "id":req.id,
            "request_sha256":receipt.request_sha256, "operator":req.operator, "reason":req.reason
        }));
        expected.1.body["setup"]["provider"] = serde_json::to_value(&req.provider).unwrap();
        assert_eq!(
            after, expected,
            "renewal must retain counters, work, grants and resume state"
        );
        assert_eq!(receipt.target_runtime, after.0.exact_ref().unwrap());
        assert_eq!(receipt.target_launch, after.1.exact_ref().unwrap());
        assert_eq!(app.epoch().unwrap(), epoch + 1);
        assert_eq!(
            migration_snapshot(&mut app).unwrap().provider_config_sha256,
            receipt.new_provider_sha256
        );

        if id == "expiry-renewal" {
            let prior = &before.1.body["setup"]["provider"];
            let current = &after.1.body["setup"]["provider"];
            let mut expected_provider = prior.clone();
            expected_provider["credentials"]["expires_at_unix"] = json!(200);
            assert_eq!(*current, expected_provider);
            assert_eq!(current["evidence"], prior["evidence"]);
            assert_eq!(
                current["credentials"]["account_env"],
                prior["credentials"]["account_env"]
            );
            assert_eq!(
                current["credentials"]["grant"],
                prior["credentials"]["grant"]
            );
            assert_eq!(
                current["credentials"]["token_env"],
                prior["credentials"]["token_env"]
            );
            assert_eq!(receipt.target_mode, provider::AccessMode::SiwcOAuth);
            assert_eq!(
                receipt.target_profile,
                provider::AccessMode::SiwcOAuth.profile()
            );
        }
        no_funding(&ledger, &app.ledger().unwrap());
    }
}

#[test]
fn planned_cas_rejects_semantic_writers_while_execution_owner_is_held() {
    for change in 0..3 {
        let (_root, mut app) = fixture(2);
        let work = start(&mut app, "Synthetic concurrent semantic update.").unwrap();
        let req = request(&mut app, "racing-heads", "new-model");
        let owner = app.store.acquire_execution_owner().unwrap();
        let planned = plan(&mut app, &req, validate_request(&req, 10).unwrap(), 10).unwrap();
        let target_runtime = planned.receipt.target_runtime.clone();
        let target_launch = planned.receipt.target_launch.clone();

        // Each writer opens after planning and succeeds while ownership remains held.
        match change {
            0 => {
                let mut other = Runtime::open(app.state_root()).unwrap();
                edit_state(&mut other, |state| {
                    state
                        .operator_events
                        .push(json!({"kind":"other-operator-write"}));
                });
            }
            1 => {
                let mut other = Runtime::open_for_closeout(app.state_root()).unwrap();
                other
                    .pause("requester", &work, "Explicit concurrent closeout pause.")
                    .unwrap();
            }
            2 => {
                let mut other = Runtime::open(app.state_root()).unwrap();
                let (old, mut saved) = load(&mut other).unwrap();
                saved.steps += 1;
                save(&mut other, Some(&old), &saved).unwrap();
            }
            _ => unreachable!(),
        }
        let raced_heads = heads(&mut app);
        let conflict_id = if change == 2 {
            assert_eq!(raced_heads.0.exact_ref().unwrap(), req.expected_runtime);
            assert_ne!(raced_heads.1.exact_ref().unwrap(), req.expected_launch);
            ID
        } else {
            assert_ne!(raced_heads.0.exact_ref().unwrap(), req.expected_runtime);
            assert_eq!(raced_heads.1.exact_ref().unwrap(), req.expected_launch);
            STATE_ID
        };
        let records = cells(&app);
        let epoch = app.epoch().unwrap();
        let ledger = app.ledger().unwrap();
        let error = app
            .store
            .compare_and_swap_batch(planned.changes)
            .unwrap_err();
        assert!(
            matches!(&error, s::StoreError::RevisionConflict(id) if id.as_str() == conflict_id),
            "expected stale CAS conflict for {conflict_id}: {error}"
        );
        assert_eq!(heads(&mut app), raced_heads);
        assert_eq!(cells(&app), records);
        assert_eq!(app.epoch().unwrap(), epoch);
        assert!(app
            .store
            .read("provider-migration:racing-heads")
            .unwrap()
            .is_none());
        assert!(app.store.read_exact(&target_runtime).is_err());
        assert!(app.store.read_exact(&target_launch).is_err());
        let after = app.ledger().unwrap();
        no_funding(&ledger, &after);
        assert_eq!(ledger.permanent_records, after.permanent_records);
        assert_eq!(
            ledger.allocated_history_events,
            after.allocated_history_events
        );
        assert_eq!(
            ledger.ordinary.consumed.retained_bytes,
            after.ordinary.consumed.retained_bytes
        );
        drop(owner);
    }
}

#[test]
fn native_prepared_responded_and_failed_invocations_survive_migration_rejection() {
    for phase in [
        s::InvocationState::Prepared,
        s::InvocationState::Responded,
        s::InvocationState::Failed,
    ] {
        let (_root, mut app) = fixture(2);
        let work = start(&mut app, "Retain native synthetic lifecycle records.").unwrap();
        let (mut input, _) = reserve(&mut app, &work, "persona-0001").unwrap();
        input.profile = Profile::Synthetic;
        let prepared = app.prepare(input, 10).unwrap();
        if phase == s::InvocationState::Responded {
            app.dispatch_synthetic(&prepared.request.id, 10).unwrap();
            // Exercise the native lifecycle only; this is not a provider completion receipt.
            app.store
                .record_response(
                    &prepared.request.id,
                    &s::sha256_bytes(b"Synthetic response-state fixture; no provider completion."),
                )
                .unwrap();
            assert!(app.synthetic_owner.is_some());
            drop(app.synthetic_owner.take());
        } else if phase == s::InvocationState::Failed {
            app.store
                .reject_invocation(&prepared.request.id, "Explicit synthetic local rejection.")
                .unwrap();
        }
        assert!(app.synthetic_owner.is_none());
        let invocation = app.invocation(&prepared.request.id).unwrap();
        assert_eq!(invocation.state, phase);
        let reservation = app.store.reservation(&invocation.reservation).unwrap();
        assert_eq!(
            app.snapshot().unwrap().personas["persona-0001"]
                .active_invocation
                .as_deref(),
            Some(prepared.request.id.as_str())
        );
        let req = request(&mut app, "active-native", "new-model");
        rejected(&mut app, &req);
        assert_eq!(app.invocation(&prepared.request.id).unwrap(), invocation);
        assert_eq!(app.store.reservation(&reservation.id).unwrap(), reservation);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn migration_fences_initial_handover_but_retains_historical_prepare() {
    let (_root, mut app) = fixture(2);
    let work = start(&mut app, "Synthetic character accepted before handover.").unwrap();
    let (input, saved) = reserve(&mut app, &work, "persona-0001").unwrap();
    let old_id = input.id.clone();
    let now = unix_now().unwrap();
    // Calling the driver directly leaves the launch handover unfinished.
    let report = HistoryDriver::default()
        .step(&mut app, input, &saved, now)
        .await
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::CharacterOnly);
    let adopted = app.adopted(&old_id).unwrap();
    let adopted_value = serde_json::to_value(&adopted).unwrap();
    let old_bytes = serde_json::to_vec(&adopted.response).unwrap();
    assert_eq!(s::sha256_bytes(&old_bytes), adopted.response_hash);
    let old_invocation = app.invocation(&old_id).unwrap();
    assert_eq!(old_invocation.state, s::InvocationState::Adopted);
    let old_reservation = app.store.reservation(&old_invocation.reservation).unwrap();
    let old_prepared = decode_prepared(&old_invocation).unwrap();
    assert_eq!(old_prepared.requested_model, "old-model");
    assert_eq!(old_prepared.requested_effort, "medium");
    assert!(old_prepared.character_revision.is_none());
    let initial = app.snapshot().unwrap();
    let initial_actor = &initial.personas["persona-0001"];
    let character_revision = initial_actor.character.as_ref().unwrap().revision.clone();
    assert!(!initial_actor.focus[&work].permit);
    assert!(!handed_over(&initial, "persona-0001"));
    assert_eq!(initial_actor.owner_epoch, old_prepared.owner_epoch + 1);
    assert_eq!(
        initial_actor.admission_serial,
        old_prepared.admission_serial
    );

    let mut req = request(&mut app, "before-handover", "new-model");
    let mut target = serde_json::to_value(&req.provider).unwrap();
    target["effort"] = json!("high");
    target["evidence"]["efforts"] = json!(["high"]);
    req.provider = serde_json::from_value(target).unwrap();
    let before = heads(&mut app);
    let ledger = app.ledger().unwrap();
    migrate_provider(&mut app, &req, now).unwrap();
    let migrated_heads = heads(&mut app);
    let migrated = app.snapshot().unwrap();
    let migrated_actor = &migrated.personas["persona-0001"];
    assert!(!migrated_actor.focus[&work].permit);
    assert!(!handed_over(&migrated, "persona-0001"));
    assert_eq!(migrated_actor.owner_epoch, old_prepared.owner_epoch + 2);
    assert_eq!(
        migrated_actor.admission_serial,
        old_prepared.admission_serial
    );
    assert_eq!(
        migrated_actor.character.as_ref().unwrap().revision,
        character_revision
    );
    assert_eq!(
        migrated_heads.0.body["personas"]["persona-0001"]["focus"],
        before.0.body["personas"]["persona-0001"]["focus"]
    );
    for field in ["board", "work_meta", "sources"] {
        assert_eq!(migrated_heads.0.body[field], before.0.body[field]);
    }
    let records = cells(&app);
    let epoch = app.epoch().unwrap();
    let error = handover(&mut app, &work).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("initial handover history or current authority changed"),
        "{error}"
    );
    let error = app.submit_synthetic(&old_id, &old_bytes, now).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("synthetic submission requires a prepared invocation; no replay"),
        "{error}"
    );
    assert_eq!(heads(&mut app), migrated_heads);
    assert_eq!(cells(&app), records);
    assert_eq!(app.epoch().unwrap(), epoch);
    assert_eq!(app.invocation(&old_id).unwrap(), old_invocation);
    assert_eq!(
        app.store.reservation(&old_reservation.id).unwrap(),
        old_reservation
    );
    no_funding(&ledger, &app.ledger().unwrap());

    app.resume("requester", "persona-0001", &work).unwrap();
    let resumed = app.snapshot().unwrap();
    let resumed_actor = &resumed.personas["persona-0001"];
    assert!(resumed_actor.focus[&work].permit);
    assert!(resumed_actor.focus[&work].explicit_resume_permit);
    assert!(!handed_over(&resumed, "persona-0001"));
    assert_eq!(
        resumed.work_meta[&work].attempts_left,
        initial.work_meta[&work].attempts_left
    );
    let resumed_heads = heads(&mut app);
    let resumed_cells = cells(&app);
    let resumed_epoch = app.epoch().unwrap();
    let resumed_ledger = app.ledger().unwrap();
    let serial = resumed_actor.admission_serial;

    // Existing identity lookup returns historical Prepared data, even after resume.
    // It grants no fresh admission and cannot authorize dispatch replay.
    let historical = app.prepare(old_prepared.request.clone(), now).unwrap();
    assert_eq!(historical.requested_model, "old-model");
    assert_eq!(historical.requested_effort, "medium");
    assert_eq!(historical.prompt, old_prepared.prompt);
    assert_eq!(historical.prompt_sha256, old_prepared.prompt_sha256);
    assert_eq!(
        serde_json::to_value(&historical.sources).unwrap(),
        serde_json::to_value(&old_prepared.sources).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&historical).unwrap(),
        serde_json::to_value(&old_prepared).unwrap()
    );
    assert_eq!(heads(&mut app), resumed_heads);
    assert_eq!(cells(&app), resumed_cells);
    assert_eq!(app.epoch().unwrap(), resumed_epoch);
    assert_eq!(
        app.snapshot().unwrap().personas["persona-0001"].admission_serial,
        serial
    );
    let error = app.submit_synthetic(&old_id, &old_bytes, now).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("synthetic submission requires a prepared invocation; no replay"),
        "{error}"
    );
    assert_eq!(heads(&mut app), resumed_heads);
    assert_eq!(cells(&app), resumed_cells);
    assert_eq!(app.epoch().unwrap(), resumed_epoch);
    assert_eq!(app.invocation(&old_id).unwrap(), old_invocation);
    assert_eq!(
        app.store.reservation(&old_reservation.id).unwrap(),
        old_reservation
    );
    let after_lookup = app.ledger().unwrap();
    no_funding(&resumed_ledger, &after_lookup);
    assert_eq!(
        resumed_ledger.permanent_records,
        after_lookup.permanent_records
    );
    assert_eq!(
        resumed_ledger.allocated_history_events,
        after_lookup.allocated_history_events
    );
    assert_eq!(
        resumed_ledger.ordinary.consumed.retained_bytes,
        after_lookup.ordinary.consumed.retained_bytes
    );

    let fresh = app
        .prepare(
            PrepareRequest {
                id: "fresh-after-explicit-resume".into(),
                persona: "persona-0001".into(),
                work: work.clone(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            now,
        )
        .unwrap();
    assert_ne!(fresh.request.id, old_id);
    assert_eq!(fresh.requested_model, req.provider.model);
    assert_eq!(fresh.requested_effort, "high");
    assert_eq!(
        fresh.character_revision.as_deref(),
        Some(character_revision.as_str())
    );
    assert_eq!(fresh.owner_epoch, resumed_actor.owner_epoch + 1);
    assert_eq!(fresh.admission_serial, serial + 1);
    assert_eq!(
        app.invocation(&fresh.request.id).unwrap().state,
        s::InvocationState::Prepared
    );
    let admitted = app.snapshot().unwrap();
    let actor = &admitted.personas["persona-0001"];
    assert_eq!(
        actor.active_invocation.as_deref(),
        Some(fresh.request.id.as_str())
    );
    assert_eq!(actor.admission_serial, serial + 1);
    assert!(!actor.focus[&work].permit);
    assert!(!handed_over(&admitted, "persona-0001"));
    assert_eq!(
        actor.character.as_ref().unwrap().revision,
        character_revision
    );
    assert_eq!(
        admitted.work_meta[&work].attempts_left,
        resumed.work_meta[&work].attempts_left - 1
    );
    assert_eq!(app.invocation(&old_id).unwrap(), old_invocation);
    assert_eq!(
        app.store.reservation(&old_reservation.id).unwrap(),
        old_reservation
    );
    assert_eq!(
        serde_json::to_value(app.adopted(&old_id).unwrap()).unwrap(),
        adopted_value
    );
}
