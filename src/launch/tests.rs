use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

fn settings(root: &Path, steps: usize) -> Setup {
    Setup {
        cohort: 2,
        owner: 1,
        destination: "fixture".into(),
        working_directory: root.join("working"),
        share_cohort: true,
        max_steps: steps,
        max_considerations: 128,
        max_work_attempts: 32,
        max_total_steps: 32,
        max_works: 4,
        provider: ResearchSettings {
            executable: root.join("not-a-provider"),
            executable_sha256: "0".repeat(64),
            cli_version: provider::SUPPORTED_CLI_VERSION.into(),
            temporary_directory: root.into(),
            catalog_evidence_sha256: "1".repeat(64),
            verified_model: "fixture-model".into(),
            verified_efforts: BTreeSet::from(["medium".into()]),
            model: "fixture-model".into(),
            effort: "medium".into(),
            timeout_ms: 1000,
            max_stdout_bytes: 65_536,
            max_stderr_bytes: 4096,
            max_response_bytes: 32_768,
            max_executable_bytes: 1024,
            max_control_bytes: 65_536,
            supported_disable_features: BTreeSet::new(),
            disabled_features: BTreeSet::new(),
        },
    }
}
fn fixture(steps: usize) -> (tempfile::TempDir, Runtime) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("working")).unwrap();
    let app = setup(
        &root.path().join("state"),
        "requester",
        settings(root.path(), steps),
        true,
    )
    .unwrap();
    (root, app)
}
fn response(app: &mut Runtime, prepared: &Prepared) -> p::Decision {
    let mut d: p::Decision = serde_json::from_value(json!({
        "version":1,"invocation_id":prepared.request.id,"epoch":prepared.epoch,
        "work":{"kind":"ongoing","reason":"Unfinished fixture.","evidence":[]},
        "fragment_maintenance":{"kind":"no_change","reason":"No edit."},
        "asset_maintenance":{"kind":"no_change","reason":"No edit."},
        "next_fragments":{"kind":"clear","reason":"No optional context."},
        "next_assets":{"kind":"clear","reason":"No optional context."},
        "continuation":{"kind":"stop","reason":"Explicit fixture stop."},
        "reply":{"kind":"authored","audience":["requester"],"text":"Accepted reply."},
        "operations":[]
    }))
    .unwrap();
    if prepared.character_revision.is_none() {
        let source = format!("voice-{}", prepared.request.persona);
        d.operations = serde_json::from_value(json!([
            {"kind":"read","id":"fenced-read","reference":{"source":"absent","revision":"revision","representation":"utf8"}},
            {"kind":"file_write","id":"voice","scope":{"kind":"fragments"},"source":source,"path":"voice.txt","base_revision":null,
                "text":"I retain my own accepted account.","qualifications":[],"policy_id":POLICY},
            {"kind":"character_adopt","id":"character","base_revision":null,
                "ocean":{"openness":"Modeled","conscientiousness":"Modeled",
                    "extraversion":"Modeled","agreeableness":"Modeled","neuroticism":"Modeled"},
                "vad":{"valence":"Modeled","activation":"Modeled","control":"Modeled"},
                "own_voice":[{"source":source,"revision":"proposal-op:voice","representation":"utf8"}],
                "reason":"Initial own account.","evidence":[]}
        ])).unwrap();
        d.fragment_maintenance = p::Maintenance::Change {
            reason: "Initial account.".into(),
            operation_ids: vec!["voice".into(), "character".into()],
        };
        d.reply = p::ReplyDisposition::Authored {
            audience: vec!["requester".into()],
            text: "FENCED".into(),
        };
    } else {
        let state = app.snapshot().unwrap();
        let work = state
            .board
            .work(&w::WorkId(prepared.request.work.clone()))
            .unwrap();
        if let Some((id, offer)) = work.offers.iter().find(|(_, offer)| {
            offer.to.0 == prepared.request.persona
                && matches!(offer.status, w::OfferStatus::Pending)
        }) {
            d.operations.push(p::Operation::Responsibility {
                id: id.0.clone(),
                work_id: prepared.request.work.clone(),
                terms_revision: offer_revision(work, id).unwrap(),
                disposition: "accept".into(),
                reason: "I accept the exact offered terms.".into(),
            });
            assert_eq!(offer.terms.mandate_revision, 1);
        }
    }
    d
}
struct Script {
    calls: Vec<(String, bool)>,
    continuation: p::Continuation,
    reject: bool,
    interrupt: bool,
}
impl Script {
    fn new() -> Self {
        Self {
            calls: Vec::new(),
            continuation: p::Continuation::Stop {
                reason: "Explicit stop.".into(),
            },
            reject: false,
            interrupt: false,
        }
    }
}
impl Driver for Script {
    async fn step(
        &mut self,
        app: &mut Runtime,
        mut request: PrepareRequest,
        _saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        request.profile = Profile::Synthetic;
        let prepared = app.prepare(request, now)?;
        self.calls.push((
            prepared.request.persona.clone(),
            prepared.character_revision.is_none(),
        ));
        if self.interrupt {
            app.dispatch_synthetic(&prepared.request.id, now)?;
            return Err(RuntimeError::NotReady("synthetic interruption".into()));
        }
        let mut d = response(app, &prepared);
        if prepared.character_revision.is_some() {
            d.continuation = self.continuation.clone();
            if self.reject {
                d.operations.push(p::Operation::Effect {
                    id: "invalid".into(),
                    capability_id: "unavailable".into(),
                    binding_revision: "unavailable".into(),
                    inputs: json!({}),
                    depends_on: Vec::new(),
                });
                d.reply = p::ReplyDisposition::Authored {
                    audience: vec!["requester".into()],
                    text: "REJECTED".into(),
                };
            }
        }
        app.submit_synthetic(&prepared.request.id, &serde_json::to_vec(&d)?, now)
    }
}
#[tokio::test(flavor = "current_thread")]
async fn cohort_handover_reuse_and_result_without_inference() {
    let (_root, mut app) = fixture(8);
    let work = start(&mut app, "First exact need.").unwrap();
    let mut script = Script::new();
    let out = run_with(&mut app, &work, &mut script).await.unwrap();
    assert_eq!(script.calls.len(), 4);
    assert_eq!(
        script.calls.iter().filter(|(_, initial)| *initial).count(),
        2
    );
    assert_eq!(script.calls[0].0, "persona-0001");
    assert_eq!(script.calls[1].0, "persona-0002");
    assert_eq!(out.messages.len(), 2);
    assert!(!serde_json::to_string(&out).unwrap().contains("FENCED"));
    let state = app.snapshot().unwrap();
    let serials: Vec<_> = state
        .personas
        .values()
        .map(|p| p.admission_serial)
        .collect();
    for _ in 0..2 {
        let view = result(&mut app, &work).unwrap();
        assert_eq!(view.messages.len(), 2);
    }
    assert_eq!(
        app.snapshot()
            .unwrap()
            .personas
            .values()
            .map(|p| p.admission_serial)
            .collect::<Vec<_>>(),
        serials
    );
    let second = start(&mut app, "Independent second need.").unwrap();
    script.calls.clear();
    run_with(&mut app, &second, &mut script).await.unwrap();
    assert_eq!(script.calls.len(), 2);
    assert!(script.calls.iter().all(|(_, initial)| !initial));
}
#[tokio::test(flavor = "current_thread")]
async fn foreground_and_lifetime_bounds_survive_new_work_ids_and_pause() {
    let (_root, mut app) = fixture(4);
    let work = start(&mut app, "Bounded need.").unwrap();
    let mut script = Script::new();
    script.continuation = p::Continuation::Immediate {
        purpose: "Continue.".into(),
    };
    run_with(&mut app, &work, &mut script).await.unwrap();
    assert_eq!(script.calls.len(), 4);
    let (old, mut saved) = load(&mut app).unwrap();
    saved.setup.max_total_steps = 4;
    save(&mut app, Some(&old), &saved).unwrap();
    script.calls.clear();
    run_with(&mut app, &work, &mut script).await.unwrap();
    assert!(script.calls.is_empty());
    app.pause("requester", &work, "Explicit pause.").unwrap();
    let out = run_with(&mut app, &work, &mut script).await.unwrap();
    assert_eq!(out.status, "paused");
    assert!(script.calls.is_empty());
    let other = start(&mut app, "Separate authorization; same global ceiling.").unwrap();
    run_with(&mut app, &other, &mut script).await.unwrap();
    assert!(script.calls.is_empty());
}
#[tokio::test(flavor = "current_thread")]
async fn held_output_is_not_a_result_or_an_automatic_retry() {
    let (_root, mut app) = fixture(8);
    let work = start(&mut app, "Rejected response fixture.").unwrap();
    let mut script = Script::new();
    script.reject = true;
    let out = run_with(&mut app, &work, &mut script).await.unwrap();
    assert_eq!(out.stop.as_deref(), Some("held"));
    assert!(out.messages.is_empty());
    assert!(!serde_json::to_string(&out).unwrap().contains("REJECTED"));
    let calls = script.calls.len();
    run_with(&mut app, &work, &mut script).await.unwrap();
    assert_eq!(script.calls.len(), calls);
    let (_, saved) = load(&mut app).unwrap();
    let id = saved.works[&work].as_ref().unwrap();
    let before = app.invocation(id).unwrap();
    assert_eq!(before.state, s::InvocationState::Failed);
    app.resume("requester", "persona-0001", &work).unwrap();
    assert!(app.invocation(id).unwrap().history.len() >= before.history.len());
}
#[tokio::test(flavor = "current_thread")]
async fn uncertain_restart_never_dispatches_again() {
    let (root, mut app) = fixture(8);
    let work = start(&mut app, "Interrupted fixture.").unwrap();
    let mut script = Script::new();
    script.interrupt = true;
    run_with(&mut app, &work, &mut script).await.unwrap();
    assert_eq!(script.calls.len(), 1);
    drop(app);
    let mut app = open(&root.path().join("state"), None).unwrap();
    let (_, saved) = load(&mut app).unwrap();
    let id = saved.works[&work].as_ref().unwrap();
    assert_eq!(
        app.invocation(id).unwrap().state,
        s::InvocationState::Uncertain
    );
    run_with(&mut app, &work, &mut script).await.unwrap();
    result(&mut app, &work).unwrap();
    assert_eq!(script.calls.len(), 1);
}
#[tokio::test(flavor = "current_thread")]
async fn missing_message_lineage_and_revocation_withhold_retained_text() {
    let (_root, mut app) = fixture(8);
    let work = start(&mut app, "Permission fixture.").unwrap();
    run_with(&mut app, &work, &mut Script::new()).await.unwrap();
    let (old, mut state) = app.load().unwrap();
    state.messages.values_mut().next().unwrap().exposure = None;
    app.save(&old, &state).unwrap();
    assert_eq!(result(&mut app, &work).unwrap().messages.len(), 1);
    let policy = state.config.policies.active_policies[0].identity();
    app.revoke_policy("requester", policy, "Withdraw permission.")
        .unwrap();
    let out = result(&mut app, &work).unwrap();
    assert_eq!(out.status, "withheld");
    assert!(out.messages.is_empty());
    assert_eq!(out.completion_supported, None);
}
#[test]
fn setup_requires_opt_in_exact_model_and_private_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("working")).unwrap();
    let path = root.path().join("state");
    assert!(setup(&path, "requester", settings(root.path(), 8), false).is_err());
    let mut wrong = settings(root.path(), 8);
    wrong.provider.model = "substituted-model".into();
    assert!(setup(&path, "requester", wrong, true).is_err());
    assert!(!path.exists());
    symlink(root.path(), &path).unwrap();
    assert!(setup(&path, "requester", settings(root.path(), 8), true).is_err());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(setup(&path, "requester", settings(root.path(), 8), true).is_err());
    assert!(private_tree(&root.path().join("../escape")).is_err());
}
#[test]
fn setup_retains_provider_pins_and_creates_no_character() {
    let (_root, mut app) = fixture(8);
    let (_, saved) = load(&mut app).unwrap();
    assert_eq!(saved.setup.provider.executable_sha256, "0".repeat(64));
    assert_eq!(saved.setup.provider.catalog_evidence_sha256, "1".repeat(64));
    assert_eq!(saved.setup.provider.model, "fixture-model");
    assert!(!saved.setup.provider.executable.exists());
    let state = app.snapshot().unwrap();
    assert_eq!(state.personas.len(), 2);
    assert!(state.personas.values().all(|p| p.character.is_none()));
    assert!(state.board.works().is_empty());
    assert_eq!(saved.steps, 0);
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

#[derive(Default)]
struct CompleteWriting {
    calls: usize,
    stage: usize,
}
impl Driver for CompleteWriting {
    async fn step(
        &mut self,
        app: &mut Runtime,
        mut request: PrepareRequest,
        _saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        request.profile = Profile::Synthetic;
        let prepared = app.prepare(request, now)?;
        self.calls += 1;
        let mut decision = response(app, &prepared);
        if prepared.character_revision.is_some() && prepared.request.persona == "persona-0001" {
            decision.continuation = p::Continuation::Immediate {
                purpose: "Finish the finite synthetic writing task.".into(),
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
                        text: "The complete synthetic writing result.".into(),
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
                        reason: "Retain the complete result.".into(),
                        operation_ids: vec!["save-result".into()],
                    };
                    decision.next_assets = p::SelectionIntent::Replace {
                        reason: "Read the exact complete body on the fresh request.".into(),
                        references: vec![subject],
                    };
                }
                1 => {
                    assert!(prepared
                        .prompt
                        .contains("The complete synthetic writing result."));
                    let state = app.snapshot()?;
                    let work = state
                        .board
                        .work(&w::WorkId(prepared.request.work.clone()))?;
                    let artifact = &work.artifacts[&w::ArtifactId("result-artifact".into())];
                    let key =
                        ArtifactKey::from_native(&artifact.revisions[&artifact.current].reference);
                    let artifacts = BTreeMap::from([("result".into(), key.clone())]);
                    decision.operations.extend([
                        command("record-result", WorkAction::RecordModelEvidence {
                            id: "result-evidence".into(), artifact: key, outcome: "result".into(),
                            explanation: "I examined the exact supplied writing result.".into(),
                            limitations: vec!["Synthetic attributed text judgment only; no independent assurance or human acceptance.".into()],
                        }, &[]),
                        command("judge-result", WorkAction::Judge {
                            id: "result-judgment".into(), artifacts: artifacts.clone(), sufficient: true,
                            claims: BTreeMap::from([("result".into(), ClaimInput {
                                met: true, explanation: "The exact fixture text meets this writing-only criterion.".into(),
                                evidence: BTreeSet::from(["result-evidence".into()]),
                            })]),
                            explanation: "Attributed synthetic judgment.".into(),
                        }, &[]),
                        command("share-result", WorkAction::DecideShared { id: "result-shared".into(), artifacts }, &[]),
                        command("release-result", WorkAction::Release { id: "result-release".into(), shared: "result-shared".into() }, &[]),
                        command("deliver-result", WorkAction::DeliverRelease {
                            release: "result-release".into(), recipients: BTreeSet::from(["requester".into()]),
                        }, &[]),
                    ]);
                }
                2 => {
                    assert!(prepared.prompt.contains("accepted_local_delivery"));
                    let state = app.snapshot()?;
                    let work = state
                        .board
                        .work(&w::WorkId(prepared.request.work.clone()))?;
                    let release = work.deliveries.last().unwrap().release.0.clone();
                    decision.operations.push(command(
                        "close-result",
                        WorkAction::CloseGoal { release },
                        &[],
                    ));
                    decision.continuation = p::Continuation::Stop {
                        reason: "The exact local result is delivered and closure was authored."
                            .into(),
                    };
                }
                _ => panic!("completion must stop further inference"),
            }
            self.stage += 1;
        }
        let report =
            app.submit_synthetic(&prepared.request.id, &serde_json::to_vec(&decision)?, now)?;
        assert_ne!(report.status, AdoptionStatus::Held, "{:?}", report.reason);
        Ok(report)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn delivered_result_requires_fresh_close_and_completed_resume_uses_no_inference() {
    let (_root, mut app) = fixture(5);
    let work = start(
        &mut app,
        "Write the complete synthetic writing result; attributed model judgment suffices.",
    )
    .unwrap();
    assert!(app
        .snapshot()
        .unwrap()
        .board
        .work(&w::WorkId(work.clone()))
        .unwrap()
        .mandate()
        .stakeholders
        .is_empty());
    let mut driver = CompleteWriting::default();
    let delivered = run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(delivered.status, "delivered-unfinished");
    assert_eq!(delivered.completion_supported, Some(false));
    assert_eq!(delivered.local_deliveries.len(), 1);
    let bytes: Vec<u8> =
        serde_json::from_value(delivered.local_deliveries[0]["artifacts"][0]["bytes"].clone())
            .unwrap();
    assert_eq!(bytes, b"The complete synthetic writing result.");
    assert_eq!(driver.stage, 2);
    let completed = run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(completed.status, "completed-as-recorded");
    assert_eq!(completed.completion_supported, None);
    let calls = driver.calls;
    let serials: Vec<_> = app
        .snapshot()
        .unwrap()
        .personas
        .values()
        .map(|p| p.admission_serial)
        .collect();
    assert_eq!(
        run_with(&mut app, &work, &mut driver).await.unwrap().status,
        "completed-as-recorded"
    );
    assert_eq!(
        result(&mut app, &work).unwrap().status,
        "completed-as-recorded"
    );
    assert_eq!(driver.calls, calls);
    assert_eq!(
        app.snapshot()
            .unwrap()
            .personas
            .values()
            .map(|p| p.admission_serial)
            .collect::<Vec<_>>(),
        serials
    );
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_acceptance_requirement_stops_without_inventing_consent() {
    let (_root, mut app) = fixture(8);
    let work = start_with_acceptance(
        &mut app,
        "Write the complete synthetic writing result.",
        true,
    )
    .unwrap();
    let mut driver = CompleteWriting::default();
    let delivered = run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(delivered.status, "delivered-acceptance-unestablished");
    assert_eq!(driver.stage, 2);
    let calls = driver.calls;
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(driver.calls, calls);
    let state = app.snapshot().unwrap();
    assert!(state
        .board
        .work(&w::WorkId(work))
        .unwrap()
        .stakeholder_decisions
        .is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn future_wait_and_persona_pause_do_not_auto_resume() {
    for wait in [true, false] {
        let (_root, mut app) = fixture(8);
        let work = start(&mut app, "Wait or pause after acceptance.").unwrap();
        run_with(&mut app, &work, &mut Script::new()).await.unwrap();
        let deadline = unix_now().unwrap() as u64 + 3600;
        if wait {
            let state = app.snapshot().unwrap();
            let board = state.board.work(&w::WorkId(work.clone())).unwrap();
            app.register_dependency(
                "requester",
                RegisterDependencyRequest {
                    binding: WorkAuthority {
                        work: work.clone(),
                        version: board.version,
                        authority: board.mandate().authority.clone(),
                    },
                    id: "wait-source".into(),
                    source: state.personas["persona-0001"]
                        .character
                        .as_ref()
                        .unwrap()
                        .own_voice[0]
                        .clone(),
                    participants: BTreeSet::from(["persona-0001".into()]),
                    accountability: w::Accountability::Owner(w::PersonaId("persona-0001".into())),
                    origin: LocalEventOrigin::OperatorLocal,
                },
                unix_now().unwrap(),
            )
            .unwrap();
        }
        app.resume("requester", "persona-0001", &work).unwrap();
        let mut driver = Script::new();
        driver.continuation = if wait {
            p::Continuation::Wait {
                dependency_ids: vec!["wait-source".into()],
                deadline_unix: deadline,
                reason: "Future deadline.".into(),
            }
        } else {
            p::Continuation::Pause {
                reason: "Explicit persona pause.".into(),
            }
        };
        run_with(&mut app, &work, &mut driver).await.unwrap();
        assert_eq!(driver.calls.len(), 1);
        assert!(app.snapshot().unwrap().personas["persona-0001"]
            .active_invocation
            .is_none());
        run_with(&mut app, &work, &mut driver).await.unwrap();
        assert_eq!(driver.calls.len(), 1);
        if wait {
            assert_eq!(
                select(&mut app, &work, 128, deadline as i64)
                    .unwrap()
                    .1
                    .as_deref(),
                Some("persona-0001")
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn missing_delivery_origin_and_withdrawal_never_expose_artifact_body() {
    let (_root, mut app) = fixture(5);
    let work = start(&mut app, "Write the complete synthetic writing result.").unwrap();
    run_with(&mut app, &work, &mut CompleteWriting::default())
        .await
        .unwrap();
    assert_eq!(result(&mut app, &work).unwrap().local_deliveries.len(), 1);
    let (old, mut state) = app.load().unwrap();
    let observation = state
        .observations
        .values_mut()
        .find(|o| o.kind == "accepted_local_delivery")
        .unwrap();
    observation.exposure = None;
    app.save(&old, &state).unwrap();
    assert!(result(&mut app, &work).unwrap().local_deliveries.is_empty());
    let policy = state.config.policies.active_policies[0].identity();
    app.revoke_policy("requester", policy, "Explicit withdrawal.")
        .unwrap();
    let out = result(&mut app, &work).unwrap();
    assert_eq!(out.status, "withheld");
    assert!(out.local_deliveries.is_empty() && out.messages.is_empty());
}

struct LaterCharacter {
    calls: usize,
}
impl Driver for LaterCharacter {
    async fn step(
        &mut self,
        app: &mut Runtime,
        mut request: PrepareRequest,
        _saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        request.profile = Profile::Synthetic;
        let prepared = app.prepare(request, now)?;
        self.calls += 1;
        let state = app.snapshot()?;
        let character = state.personas[&prepared.request.persona]
            .character
            .as_ref()
            .unwrap();
        let mut decision = response(app, &prepared);
        let mut ocean = character.ocean.clone();
        ocean.insert("openness".into(), "A later modeled revision.".into());
        decision.operations.push(p::Operation::CharacterAdopt {
            id: "later-character".into(),
            base_revision: Some(character.revision.clone()),
            ocean,
            vad: character.vad.clone(),
            own_voice: character.own_voice.clone(),
            reason: "An explicit later character revision.".into(),
            evidence: Vec::new(),
        });
        decision.fragment_maintenance = p::Maintenance::Change {
            reason: "Later character maintenance.".into(),
            operation_ids: vec!["later-character".into()],
        };
        decision.continuation = p::Continuation::Immediate {
            purpose: "This continuation must be fenced.".into(),
        };
        app.submit_synthetic(&prepared.request.id, &serde_json::to_vec(&decision)?, now)
    }
}
#[tokio::test(flavor = "current_thread")]
async fn later_character_revision_does_not_receive_another_automatic_handover() {
    let (_root, mut app) = fixture(8);
    let work = start(&mut app, "Character fence fixture.").unwrap();
    run_with(&mut app, &work, &mut Script::new()).await.unwrap();
    app.resume("requester", "persona-0001", &work).unwrap();
    let mut driver = LaterCharacter { calls: 0 };
    run_with(&mut app, &work, &mut driver).await.unwrap();
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(driver.calls, 1);
    let state = app.snapshot().unwrap();
    assert!(!state.personas["persona-0001"].focus[&work].permit);
    assert_eq!(
        state
            .operator_events
            .iter()
            .filter(|e| e["kind"] == "launch_initial_handover")
            .count(),
        2
    );
}

#[tokio::test(flavor = "current_thread")]
async fn initial_handover_preserves_other_participants_explicit_pause() {
    let (_root, mut app) = fixture(8);
    let work = start(&mut app, "Independent participant lifecycle.").unwrap();
    let mut driver = Script::new();
    let (request, saved) = reserve(&mut app, &work, "persona-0001").unwrap();
    driver
        .step(&mut app, request, &saved, unix_now().unwrap())
        .await
        .unwrap();
    assert!(handover(&mut app, &work).unwrap());
    driver.continuation = p::Continuation::Pause {
        reason: "My own explicit pause.".into(),
    };
    let (request, saved) = reserve(&mut app, &work, "persona-0001").unwrap();
    driver
        .step(&mut app, request, &saved, unix_now().unwrap())
        .await
        .unwrap();
    let before = app.snapshot().unwrap();
    let participant_before = before
        .board
        .work(&w::WorkId(work.clone()))
        .unwrap()
        .participants[&w::PersonaId("persona-0001".into())]
        .clone();
    let persona_before = serde_json::to_value(&before.personas["persona-0001"]).unwrap();
    let (request, saved) = reserve(&mut app, &work, "persona-0002").unwrap();
    driver
        .step(&mut app, request, &saved, unix_now().unwrap())
        .await
        .unwrap();
    assert!(handover(&mut app, &work).unwrap());
    let after = app.snapshot().unwrap();
    assert_eq!(
        after
            .board
            .work(&w::WorkId(work.clone()))
            .unwrap()
            .participants[&w::PersonaId("persona-0001".into())],
        participant_before
    );
    assert_eq!(
        serde_json::to_value(&after.personas["persona-0001"]).unwrap(),
        persona_before
    );
    assert!(after.personas["persona-0002"].focus[&work].permit);
}

// Only synthetic test states are changed directly here, to hold public phase
// and authority metadata fixed while isolating a hidden fact's disclosure.
fn alter_projection_fixture(app: &mut Runtime, change: impl FnOnce(&mut Value)) {
    let (old, state) = app.load().unwrap();
    let mut body = serde_json::to_value(state).unwrap();
    change(&mut body);
    app.save(&old, &serde_json::from_value(body).unwrap())
        .unwrap();
}
fn output_value(app: &mut Runtime, work: &str) -> Value {
    serde_json::to_value(result(app, work).unwrap()).unwrap()
}
fn denied_projection_exposure(body: &mut Value, work: &str) -> Value {
    let mut policy = body["config"]["policies"]["active_policies"][0].clone();
    policy["id"] = json!("fixture-hidden-origin");
    policy["readers"] = json!(["persona-0002"]);
    body["config"]["policies"]["active_policies"]
        .as_array_mut()
        .unwrap()
        .push(policy.clone());
    let mut exposure = body["work_meta"][work]["exposure"].clone();
    exposure["policies"].as_array_mut().unwrap().push(json!({
        "source":"fixture-hidden-origin","revision":"1","policy":policy
    }));
    exposure
}
#[tokio::test(flavor = "current_thread")]
async fn hidden_delivery_and_absent_delivery_have_the_same_operator_view() {
    let (_root, mut app) = fixture(5);
    let work = start_with_acceptance(&mut app, "Exact fixture result.", true).unwrap();
    run_with(&mut app, &work, &mut CompleteWriting::default())
        .await
        .unwrap();
    assert_eq!(result(&mut app, &work).unwrap().local_deliveries.len(), 1);
    alter_projection_fixture(&mut app, |body| {
        let denied = denied_projection_exposure(body, &work);
        for observation in body["observations"].as_object_mut().unwrap().values_mut() {
            if observation["kind"] == "accepted_local_delivery" {
                observation["exposure"] = denied.clone();
            }
        }
    });
    let hidden = output_value(&mut app, &work);
    assert_eq!(hidden["local_deliveries"], json!([]));
    alter_projection_fixture(&mut app, |body| {
        body["board"]["works"][&work]["deliveries"] = json!([]);
        body["local_deliveries"] = json!({});
    });
    assert_eq!(output_value(&mut app, &work), hidden);
}
#[tokio::test(flavor = "current_thread")]
async fn hidden_acceptance_and_absent_acceptance_have_the_same_operator_view() {
    let (_root, mut app) = fixture(8);
    let work = start_with_acceptance(&mut app, "Exact fixture result.", true).unwrap();
    run_with(&mut app, &work, &mut CompleteWriting::default())
        .await
        .unwrap();
    let absent = output_value(&mut app, &work);
    alter_projection_fixture(&mut app, |body| {
        let delivery = body["board"]["works"][&work]["deliveries"][0].clone();
        let mut by = delivery["by"].clone();
        by["principal"] = json!({"Human":"requester"});
        let receipt = w::ExactRef::new("fixture-consent", 1, b"Synthetic fixture statement.");
        let decision = json!({"release":delivery["release"],"stakeholder":"requester",
            "accepts":true,"receipt":receipt,"by":by});
        let denied = denied_projection_exposure(body, &work);
        body["board"]["works"][&work]["stakeholder_decisions"] = json!([decision.clone()]);
        let revision = body["board"]["works"][&work]["version"]["revision"]
            .as_u64()
            .unwrap();
        body["board"]["works"][&work]["version"]["revision"] = json!(revision + 1);
        body["observations"]["fixture-hidden-consent"] = json!({
            "id":"fixture-hidden-consent","actor":"persona-0001","work":work,"invocation":null,
            "kind":"human_stakeholder_decision","at_unix":0,
            "payload":{"release":delivery["release"],"record":decision,"receipt":receipt,
                "source":"explicit_local_human_decision","metadata_only":true},
            "sources":[],"exposure":denied,"qualified_roots":[]
        });
    });
    assert_eq!(output_value(&mut app, &work), absent);
}
#[tokio::test(flavor = "current_thread")]
async fn recorded_completion_does_not_query_hidden_current_evidence() {
    let (_root, mut app) = fixture(8);
    let work = start(&mut app, "Exact fixture result.").unwrap();
    run_with(&mut app, &work, &mut CompleteWriting::default())
        .await
        .unwrap();
    assert!(app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&w::WorkId(work.clone())));
    let completed = output_value(&mut app, &work);
    assert_eq!(completed["status"], "completed-as-recorded");
    assert!(completed["completion_supported"].is_null());
    alter_projection_fixture(&mut app, |body| {
        for evidence in body["board"]["works"][&work]["evidence"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            evidence["availability"] = json!({"Unavailable":"Synthetic hidden freshness change."});
        }
    });
    assert!(!app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&w::WorkId(work.clone())));
    assert_eq!(output_value(&mut app, &work), completed);
}
#[tokio::test(flavor = "current_thread")]
async fn explicit_foreground_continuation_crosses_partial_acceptance_boundary_once() {
    let (_root, mut app) = fixture(8);
    let work = start_with_acceptance(&mut app, "Exact fixture result.", true).unwrap();
    run_with(&mut app, &work, &mut CompleteWriting::default())
        .await
        .unwrap();
    let mut driver = Script::new();
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert!(driver.calls.is_empty());
    let output = run_with_options(&mut app, &work, &mut driver, true)
        .await
        .unwrap();
    assert_eq!(driver.calls.len(), 1);
    assert_eq!(output.status, "delivered-acceptance-unestablished");
    assert!(app
        .snapshot()
        .unwrap()
        .board
        .work(&w::WorkId(work))
        .unwrap()
        .stakeholder_decisions
        .is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn latest_permitted_delivery_status_does_not_depend_on_output_truncation() {
    let (_root, mut app) = fixture(8);
    let work = start_with_acceptance(&mut app, "Exact fixture result.", true).unwrap();
    run_with(&mut app, &work, &mut CompleteWriting::default())
        .await
        .unwrap();
    let (mut request, _) = reserve(&mut app, &work, "persona-0001").unwrap();
    request.profile = Profile::Synthetic;
    let prepared = app.prepare(request, unix_now().unwrap()).unwrap();
    let mut decision = response(&mut app, &prepared);
    decision.operations.extend([
        command(
            "later-release",
            WorkAction::Release {
                id: "later-release".into(),
                shared: "result-shared".into(),
            },
            &[],
        ),
        command(
            "later-delivery",
            WorkAction::DeliverRelease {
                release: "later-release".into(),
                recipients: BTreeSet::from(["requester".into()]),
            },
            &[],
        ),
    ]);
    decision.continuation = p::Continuation::Immediate {
        purpose: "Await explicit decision.".into(),
    };
    assert_eq!(
        app.submit_synthetic(
            &prepared.request.id,
            &serde_json::to_vec(&decision).unwrap(),
            unix_now().unwrap()
        )
        .unwrap()
        .status,
        AdoptionStatus::Adopted
    );
    let view = result(&mut app, &work).unwrap();
    assert_eq!(view.human_decision_pointers[0].release, "later-release");
    let target = &view.human_decision_pointers[0];
    let binding = {
        let state = app.snapshot().unwrap();
        let work_state = state.board.work(&w::WorkId(work.clone())).unwrap();
        WorkAuthority {
            work: work.clone(),
            version: work_state.version,
            authority: work_state.mandate().authority.clone(),
        }
    };
    app.record_stakeholder_decision(
        "requester",
        HumanStakeholderDecisionRequest {
            binding,
            id: "explicit-fixture-decision".into(),
            release: target.release.clone(),
            release_sha256: target.release_sha256.clone(),
            delivery: target.delivery.clone(),
            delivery_receipt: target.delivery_receipt.clone(),
            accepts: true,
            statement: "Synthetic test human acceptance of this exact release.".into(),
        },
        unix_now().unwrap(),
    )
    .unwrap();
    // A real later redelivery of the older release must not replace focus.
    let (mut request, _) = reserve(&mut app, &work, "persona-0001").unwrap();
    request.profile = Profile::Synthetic;
    let prepared = app.prepare(request, unix_now().unwrap()).unwrap();
    let mut decision = response(&mut app, &prepared);
    decision.operations.push(command(
        "redeliver-old",
        WorkAction::DeliverRelease {
            release: "result-release".into(),
            recipients: BTreeSet::from(["requester".into()]),
        },
        &[],
    ));
    assert_eq!(
        app.submit_synthetic(
            &prepared.request.id,
            &serde_json::to_vec(&decision).unwrap(),
            unix_now().unwrap()
        )
        .unwrap()
        .status,
        AdoptionStatus::Adopted
    );
    let before = result(&mut app, &work).unwrap();
    assert_eq!(before.status, "delivered-unfinished");
    assert_eq!(before.focus_release.as_deref(), Some("later-release"));
    assert_eq!(before.local_deliveries[0]["release"], "later-release");
    assert_eq!(
        before.closure_evidence[0]["unestablished_stakeholder_acceptance"],
        json!([])
    );
    alter_projection_fixture(&mut app, |body| {
        let template = body["messages"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        for n in 0..6 {
            let mut message = template.clone();
            let id = format!("large-message-{n}");
            message["id"] = json!(id);
            message["text"] = json!("Visible synthetic context. ".repeat(10_000));
            body["messages"][id] = message;
        }
    });
    let after = result(&mut app, &work).unwrap();
    assert!(after.truncated);
    assert_eq!(after.status, before.status);
    assert_eq!(after.status_delivery, before.status_delivery);
    assert_eq!(after.closure_evidence, before.closure_evidence);
    assert_eq!(after.human_decision_pointers[0].release, "later-release");
}

#[derive(Default)]
struct HandoffScript {
    calls: Vec<(String, bool)>,
    handoff_accepted: bool,
    sender_calls_after_handoff: usize,
    contributor_calls: usize,
}
impl Driver for HandoffScript {
    async fn step(
        &mut self,
        app: &mut Runtime,
        mut request: PrepareRequest,
        _saved: &Saved,
        now: i64,
    ) -> Result<AdoptionReport> {
        request.profile = Profile::Synthetic;
        let prepared = app.prepare(request, now)?;
        let actor = prepared.request.persona.as_str();
        let initial = prepared.character_revision.is_none();
        assert!(
            !app.snapshot()?.personas[actor].focus[&prepared.request.work].explicit_resume_permit,
            "resume authorization is consumed atomically by admission, before dispatch"
        );
        assert!(!prepared.prompt.contains("explicit_resume_permit"));
        self.calls.push((actor.into(), initial));
        let mut decision = response(app, &prepared);
        if !initial {
            match actor {
                "persona-0001" if !self.handoff_accepted => {
                    decision.operations.push(command(
                        "offer-transfer",
                        WorkAction::Handoff {
                            offer: "exact-handoff".into(),
                            to: "persona-0002".into(),
                        },
                        &[],
                    ));
                    decision.continuation = p::Continuation::Immediate {
                        purpose: "Remain responsible until the recipient accepts.".into(),
                    };
                }
                "persona-0001" => self.sender_calls_after_handoff += 1,
                "persona-0002" => {
                    let state = app.snapshot()?;
                    let work = state
                        .board
                        .work(&w::WorkId(prepared.request.work.clone()))?;
                    assert!(work
                        .acceptance(&w::PersonaId("persona-0001".into()))
                        .is_some());
                    assert!(
                        state.personas["persona-0001"].focus[&prepared.request.work].permit,
                        "offering a handoff does not revoke continuation before acceptance"
                    );
                    decision.operations = vec![p::Operation::Responsibility {
                        id: "exact-handoff".into(),
                        work_id: prepared.request.work.clone(),
                        terms_revision: offer_revision(work, &w::RecordId("exact-handoff".into()))?,
                        disposition: "accept".into(),
                        reason: "Accept the sender's exact existing commitment.".into(),
                    }];
                }
                "persona-0003" => {
                    self.contributor_calls += 1;
                    if self.contributor_calls == 1 {
                        decision.continuation = p::Continuation::Immediate {
                            purpose: "Continue an accepted contribution without an exclusive role."
                                .into(),
                        };
                    }
                }
                _ => unreachable!(),
            }
        }
        let report =
            app.submit_synthetic(&prepared.request.id, &serde_json::to_vec(&decision)?, now)?;
        assert_eq!(
            report.status,
            if initial {
                AdoptionStatus::CharacterOnly
            } else {
                AdoptionStatus::Adopted
            },
            "{:?}",
            report.reason
        );
        if !initial && actor == "persona-0002" {
            self.handoff_accepted = true;
        }
        Ok(report)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn accepted_handoff_stops_sender_dispatch_without_stopping_roleless_contributor() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("working")).unwrap();
    let mut config = settings(root.path(), 12);
    config.cohort = 3;
    let mut app = setup(&root.path().join("state"), "requester", config, true).unwrap();
    let work = start(
        &mut app,
        "Voluntary exact handoff and a separate contribution.",
    )
    .unwrap();
    let mut driver = HandoffScript::default();
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert!(driver.handoff_accepted);
    assert_eq!(
        driver.sender_calls_after_handoff, 0,
        "an accepted handoff must not authorize another paid sender dispatch"
    );
    assert_eq!(
        driver.contributor_calls, 2,
        "a retained roleless contributor's ordinary continuation remains valid"
    );
    assert_eq!(
        driver.calls.iter().filter(|(_, initial)| *initial).count(),
        3,
        "untouched initial invitation admissions remain available"
    );
    let state = app.snapshot().unwrap();
    let board = state.board.work(&w::WorkId(work.clone())).unwrap();
    assert!(board
        .acceptance(&w::PersonaId("persona-0001".into()))
        .is_none());
    assert!(board
        .acceptance(&w::PersonaId("persona-0002".into()))
        .is_some());
    let (_, contributor) = board
        .acceptance(&w::PersonaId("persona-0003".into()))
        .unwrap();
    assert!(contributor.terms.outcomes.is_empty() && contributor.terms.review_roles.is_empty());
    let former = &state.personas["persona-0001"].focus[&work];
    assert!(!former.permit && !former.ordinary_continuation);
    assert!(
        matches!(former.continuation, Some(p::Continuation::Immediate { .. })),
        "retain the sender's historical authored continuation, without a live permit"
    );
    let calls = driver.calls.len();
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(driver.calls.len(), calls);
    assert!(app
        .prepare(
            PrepareRequest {
                id: "no-implicit-post-handoff-resume".into(),
                persona: "persona-0001".into(),
                work,
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            unix_now().unwrap()
        )
        .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn retained_ended_sender_requires_fresh_one_admission_operator_resume() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("working")).unwrap();
    let mut config = settings(root.path(), 12);
    config.cohort = 3;
    let mut app = setup(&root.path().join("state"), "requester", config, true).unwrap();
    let work = start(&mut app, "Retained exact handoff state.").unwrap();
    let mut driver = HandoffScript::default();
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert!(driver.handoff_accepted);
    let (old, state) = app.load().unwrap();
    let mut legacy = serde_json::to_value(&state).unwrap();
    let focus = &mut legacy["personas"]["persona-0001"]["focus"][&work];
    focus["permit"] = json!(true);
    focus["ordinary_continuation"] = json!(true);
    focus
        .as_object_mut()
        .unwrap()
        .remove("explicit_resume_permit");
    app.save(&old, &serde_json::from_value(legacy).unwrap())
        .unwrap();
    drop(app);
    let mut app = open(&root.path().join("state"), None).unwrap();
    let before_steps = load(&mut app).unwrap().1.steps;
    let before_calls = driver.calls.len();
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(
        driver.calls.len(),
        before_calls,
        "a retained ended sender's stale permit must not cause paid dispatch"
    );
    assert_eq!(load(&mut app).unwrap().1.steps, before_steps);
    assert!(
        app.snapshot().unwrap().personas["persona-0001"].focus[&work].permit,
        "read-time denial does not silently rewrite historical state"
    );
    assert!(app
        .prepare(
            PrepareRequest {
                id: "legacy-stale-permit".into(),
                persona: "persona-0001".into(),
                work: work.clone(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            unix_now().unwrap()
        )
        .is_err());
    app.resume("requester", "persona-0001", &work).unwrap();
    let resumed_state = app.snapshot().unwrap();
    assert!(
        resumed_state
            .board
            .work(&w::WorkId(work.clone()))
            .unwrap()
            .acceptance(&w::PersonaId("persona-0001".into()))
            .is_none(),
        "explicit resume does not reclaim the transferred responsibility"
    );
    let resumed = serde_json::to_value(resumed_state).unwrap();
    assert_eq!(
        resumed["personas"]["persona-0001"]["focus"][&work]["explicit_resume_permit"],
        true
    );
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(driver.calls.len(), before_calls + 1);
    assert_eq!(driver.sender_calls_after_handoff, 1);
    let consumed = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    assert!(
        !consumed["personas"]["persona-0001"]["focus"][&work]["explicit_resume_permit"]
            .as_bool()
            .unwrap_or(false)
    );
    run_with(&mut app, &work, &mut driver).await.unwrap();
    assert_eq!(
        driver.calls.len(),
        before_calls + 1,
        "one grant cannot be replayed"
    );
    app.resume("requester", "persona-0001", &work).unwrap();
    app.pause(
        "requester",
        &work,
        "Cancel the outstanding local permission.",
    )
    .unwrap();
    let paused = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    assert!(
        !paused["personas"]["persona-0001"]["focus"][&work]["explicit_resume_permit"]
            .as_bool()
            .unwrap_or(false)
    );
    app.resume("requester", "persona-0001", &work).unwrap();
    app.closeout(
        "requester",
        &work,
        false,
        "End without manufactured success.",
    )
    .unwrap();
    let ended = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    assert!(
        !ended["personas"]["persona-0001"]["focus"][&work]["explicit_resume_permit"]
            .as_bool()
            .unwrap_or(false)
    );
}
