//! Application-boundary fixtures. All provider transport is explicitly synthetic.
//! These tests inspect persisted SQLite objects and actual prepared prompt bytes.
#![cfg(unix)]
use ai_personas::{files as f, protocol as p, runtime::*, store as s, work as w};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

#[path = "bridge/cases.rs"]
mod bridge_cases;
#[path = "scheduler/cases.rs"]
mod scheduler_cases;

#[path = "warnings/cases.rs"]
mod warning_cases;

#[path = "discovery/cases.rs"]
mod discovery_cases;

#[path = "combined/cases.rs"]
mod combined_cases;

#[path = "collaboration/cases.rs"]
mod collaboration_cases;

// Body bundles are host projections, while selection-repair receipts retain
// file-layer references. Check the explicit format instead of accepting either
// representation indiscriminately.
fn rendered_body_reference(bundle: &Value, value: &Value) -> f::Reference {
    if bundle.get("reference_format").is_none() {
        return serde_json::from_value(value.clone()).unwrap();
    }
    assert_eq!(bundle["reference_format"], "decision_exact_ref");
    let reference: p::ExactRef = serde_json::from_value(value.clone()).unwrap();
    assert!(reference.lifetime.is_none());
    let representation = match reference.representation.as_str() {
        "utf8" => f::Representation::Utf8,
        other => f::Representation::JsonString {
            pointer: other.strip_prefix("json_string:").unwrap().into(),
        },
    };
    f::Reference {
        source: reference.source,
        revision: reference.revision,
        representation,
        start: reference.start.map(|v| usize::try_from(v).unwrap()),
        end: reference.end.map(|v| usize::try_from(v).unwrap()),
    }
}

#[path = "actionable_refs/cases.rs"]
mod actionable_refs_cases;

#[path = "observability/cases.rs"]
mod observability_cases;

#[path = "work_inspection/cases.rs"]
mod work_inspection_cases;

#[path = "closure_readiness/cases.rs"]
mod closure_readiness_cases;

#[path = "stakeholder/cases.rs"]
mod stakeholder_cases;

#[path = "maintenance/cases.rs"]
mod maintenance_cases;

#[path = "direct_provider/cases.rs"]
mod direct_provider_cases;

const NOW: i64 = 1_800_000_000;
fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|s| (*s).into()).collect()
}
fn policy() -> f::Policy {
    f::Policy {
        id: "local-policy".into(),
        revision: "policy-1".into(),
        readers: set(&["a", "b"]),
        processing_destinations: set(&["fixture"]),
        audience: set(&["a", "b", "human"]),
        derivative_use: f::DerivativeUse::AllowDerivatives,
        expires_at_unix: None,
        revoked_at_unix: None,
    }
}
fn config() -> Config {
    let mut config = Config::local(f::PolicySnapshot {
        active_policies: vec![policy()],
        revoked_policies: BTreeSet::new(),
    });
    config.local_recipients = set(&["human"]);
    config
}
fn seed(id: &str) -> Seed {
    Seed {
        id: id.into(),
        label: id.into(),
        text: "A bounded operator-supplied candidate seed.".into(),
        provenance: "synthetic integration fixture; no lived experience asserted".into(),
        requested_model: "fixture-unavailable-model".into(),
        requested_effort: "medium".into(),
        model_confirmed: false,
        destination: "fixture".into(),
        audience: set(&["a", "b", "human"]),
        grants: Grants {
            fragments: true,
            assets: true,
            character: true,
            work_commands: true,
            write_policies: set(&["local-policy"]),
            communicate_with: set(&["a", "b", "human"]),
        },
    }
}
fn profiles() -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    (
        [
            "openness",
            "conscientiousness",
            "extraversion",
            "agreeableness",
            "neuroticism",
        ]
        .into_iter()
        .map(|k| (k.into(), "operator supplied modeled dimension".into()))
        .collect(),
        ["valence", "activation", "control"]
            .into_iter()
            .map(|k| (k.into(), "modeled baseline".into()))
            .collect(),
    )
}
fn record(source: &str, scope: f::Scope, body: &str) -> f::FileRevision {
    f::FileRevision {
        source: source.into(),
        revision: String::new(),
        path: format!("{source}.txt"),
        scope,
        bytes: body.as_bytes().to_vec(),
        author: "operator:fixture".into(),
        policy: policy(),
        qualifications: Vec::new(),
        retired: false,
    }
    .seal()
    .unwrap()
}
fn reference(record: &f::FileRevision) -> p::ExactRef {
    p::ExactRef {
        source: record.source.clone(),
        revision: record.revision.clone(),
        representation: "utf8".into(),
        start: None,
        end: None,
        lifetime: Some("work".into()),
    }
}
fn need() -> NeedInput {
    NeedInput {
        id: "work".into(),
        origin: NeedOrigin::Human,
        request: "Preserve an exact authored result and account for its status.".into(),
        boundaries: "Local fixture scope only.".into(),
        authorization: "Operator explicitly authorizes this local fixture.".into(),
        assumptions: "No external effects or engineering assurance.".into(),
        terms: "One exact local artifact with an attributed assessment.".into(),
        policy_id: "local-policy".into(),
        outcomes: BTreeMap::from([(
            "result".into(),
            OutcomeSpec {
                description: "Exact authored result".into(),
                criterion: "The human testimony concerns the exact result.".into(),
                required_reviews: BTreeSet::new(),
            },
        )]),
        review_roles: BTreeMap::new(),
        decision_rule: w::DecisionRule::Designated(w::PersonaId("a".into())),
        delivery_to: set(&["human"]),
        stakeholders: BTreeSet::new(),
        invitations: vec![Invitation {
            id: "offer-a".into(),
            to: "a".into(),
            terms: w::CommitmentTerms {
                mandate_revision: 1,
                exact_terms: "Own the required result.".into(),
                outcomes: BTreeSet::from([w::OutcomeId("result".into())]),
                review_roles: BTreeSet::new(),
            },
        }],
    }
}
struct Fixture {
    _temp: TempDir,
    app: Runtime,
}
impl Fixture {
    fn new(import: bool) -> Self {
        Self::configured(import, config(), seed("a"), need())
    }
    fn with_config(import: bool, configuration: Config) -> Self {
        Self::configured(import, configuration, seed("a"), need())
    }
    fn configured(import: bool, configuration: Config, primary: Seed, original: NeedInput) -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = temp.path().join("state");
        let mut app = Runtime::create(&root, configuration, "operator").unwrap();
        app.add_persona_seed("operator", seed("b")).unwrap();
        if import {
            let (ocean, vad) = profiles();
            app.import_persona(
                "operator",
                PersonaImport {
                    seed: primary,
                    ocean,
                    vad,
                    voice: record(
                        "voice",
                        f::Scope::Fragments,
                        "I state what I observed and what I only infer.",
                    ),
                },
                NOW,
            )
            .unwrap();
        } else {
            app.add_persona_seed("operator", primary).unwrap();
        }
        app.add_need("operator", original).unwrap();
        Self { _temp: temp, app }
    }
    fn prepare(&mut self, id: &str) -> Prepared {
        self.app
            .prepare(
                PrepareRequest {
                    id: id.into(),
                    persona: "a".into(),
                    work: "work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap()
    }
    fn run(&mut self, prepared: &Prepared, decision: &p::Decision) -> AdoptionReport {
        self.app
            .dispatch_synthetic(&prepared.request.id, NOW)
            .unwrap();
        self.app
            .adopt_response(
                &prepared.request.id,
                &serde_json::to_vec(decision).unwrap(),
                NOW,
            )
            .unwrap()
    }
    fn accept(&mut self) {
        let prepared = self.prepare("accept");
        let mut decision = decision(&prepared, false);
        let snapshot = self.app.snapshot().unwrap();
        let work = snapshot.board.work(&w::WorkId("work".into())).unwrap();
        decision.operations.push(p::Operation::Responsibility {
            id: "offer-a".into(),
            work_id: "work".into(),
            terms_revision: offer_revision(work, &w::RecordId("offer-a".into())).unwrap(),
            disposition: "accept".into(),
            reason: "I accept these exact terms.".into(),
        });
        assert_eq!(
            self.run(&prepared, &decision).status,
            AdoptionStatus::Adopted
        );
    }
}
fn decision(prepared: &Prepared, preserve: bool) -> p::Decision {
    let selection = if preserve {
        p::SelectionIntent::Preserve {
            reason: "Keep the exact existing choice without renewing lifetime.".into(),
        }
    } else {
        p::SelectionIntent::Clear {
            reason: "No optional context is needed yet.".into(),
        }
    };
    p::Decision {
        version: 1,
        invocation_id: prepared.request.id.clone(),
        epoch: prepared.epoch,
        work: p::WorkDisposition::Ongoing {
            reason: "Current work remains open.".into(),
            evidence: Vec::new(),
        },
        fragment_maintenance: p::Maintenance::NoChange {
            reason: "No justified fragment edit.".into(),
        },
        asset_maintenance: p::Maintenance::NoChange {
            reason: "No justified asset edit.".into(),
        },
        next_fragments: selection.clone(),
        next_assets: selection,
        continuation: p::Continuation::Immediate {
            purpose: "Continue under the existing allowance.".into(),
        },
        reply: p::ReplyDisposition::None {
            reason: "No communication needed.".into(),
        },
        operations: Vec::new(),
    }
}
fn write(
    id: &str,
    scope: p::Scope,
    source: &str,
    text: &str,
    base_revision: Option<String>,
) -> p::Operation {
    p::Operation::FileWrite {
        id: id.into(),
        scope,
        source: source.into(),
        path: format!("{source}.txt"),
        base_revision,
        text: text.into(),
        qualifications: Vec::new(),
        policy_id: "local-policy".into(),
    }
}
fn proposed(source: &str, id: &str) -> p::ExactRef {
    p::ExactRef {
        source: source.into(),
        revision: format!("proposal-op:{id}"),
        representation: "utf8".into(),
        start: None,
        end: None,
        lifetime: Some("work".into()),
    }
}
fn effect(id: &str, input: WorkAction, depends_on: &[&str]) -> p::Operation {
    p::Operation::Effect {
        id: id.into(),
        capability_id: "work.command.v1".into(),
        binding_revision: work_binding(),
        inputs: serde_json::to_value(input).unwrap(),
        depends_on: depends_on.iter().map(|s| (*s).into()).collect(),
    }
}
fn persisted_state(app: &Runtime) -> Value {
    let connection = Connection::open_with_flags(
        app.state_root().join("state.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let bytes: Vec<u8> = connection
        .query_row(
            "SELECT data FROM store_cells WHERE ns='object' AND k='runtime-state-v1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["body"].clone()
}

type CloseoutCell = (String, String, Vec<u8>, i64, i64, i64);
type CloseoutSnapshot = (Value, Vec<CloseoutCell>);

// Read-only test inspection must not spend navigation or obscure rollback.
fn closeout_snapshot(root: &std::path::Path) -> CloseoutSnapshot {
    let connection =
        Connection::open_with_flags(root.join("state.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let meta: Vec<u8> = connection
        .query_row("SELECT data FROM store_meta WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut statement = connection
        .prepare("SELECT ns,k,data,capacity,lane,slots FROM store_cells ORDER BY ns,k")
        .unwrap();
    let cells = statement
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
        .collect::<std::result::Result<Vec<CloseoutCell>, _>>()
        .unwrap();
    (serde_json::from_slice(&meta).unwrap(), cells)
}

fn closeout_head(root: &std::path::Path) -> s::StoredObject {
    let (_, cells) = closeout_snapshot(root);
    let cell = cells
        .into_iter()
        .find(|cell| cell.0 == "object" && cell.1 == "runtime-state-v1")
        .unwrap();
    serde_json::from_slice(&cell.2).unwrap()
}

fn assert_closeout_write_rollback(mut before: CloseoutSnapshot, after: CloseoutSnapshot) {
    let previous = before.0["closeout"]["consumed"]["navigation_bytes"]
        .as_u64()
        .unwrap();
    let current = after.0["closeout"]["consumed"]["navigation_bytes"]
        .as_u64()
        .unwrap();
    assert!(current > previous);
    // The necessary control read committed before the refused write. Everything
    // else, including epoch, sequence, identities, histories and cells, rolls back.
    before.0["closeout"]["consumed"]["navigation_bytes"] = json!(current);
    assert_eq!(before, after);
}

fn exhaust_control_navigation(host: &mut s::Store, class: s::BudgetClass) {
    let ledger = host.ledger().unwrap();
    let ordinary = ledger.ordinary.exposure().unwrap().navigation_bytes;
    let closeout = ledger.closeout.exposure().unwrap().navigation_bytes;
    let remaining = match class {
        s::BudgetClass::Ordinary => {
            ledger.limits.total.navigation_bytes
                - ledger.limits.protected_closeout.navigation_bytes
                - ordinary
        }
        s::BudgetClass::Closeout => ledger.limits.total.navigation_bytes - ordinary - closeout,
    };
    host.reserve(
        "control-navigation-pressure",
        s::Resources {
            navigation_bytes: remaining,
            ..s::Resources::default()
        },
        class,
    )
    .unwrap();
}

fn exhaust_control_retention(host: &mut s::Store, class: s::BudgetClass) {
    // Use real finite reservations and immutable journals, without editing
    // controlling counters or allocating gigabytes of fixture payloads.
    let mut object = s::StoredObject {
        id: "control-retention-pressure".into(),
        revision: 1,
        kind: "budget-pressure".into(),
        body: Value::Null,
    };
    host.compare_and_swap_batch_in_class(
        vec![s::Change {
            expected_revision: None,
            object: object.clone(),
            depends_on: Vec::new(),
        }],
        class,
    )
    .unwrap();
    let ledger = host.ledger().unwrap();
    let ordinary = ledger.ordinary.exposure().unwrap().retained_bytes;
    let closeout = ledger.closeout.exposure().unwrap().retained_bytes;
    let remaining = match class {
        s::BudgetClass::Ordinary => {
            ledger.limits.total.retained_bytes
                - ledger.limits.protected_closeout.retained_bytes
                - ordinary
        }
        s::BudgetClass::Closeout => ledger.limits.total.retained_bytes - ordinary - closeout,
    };
    // Leave bounded room for the reservation's own control cell. Small journal
    // entries then exhaust the remainder more tightly than any runtime save.
    host.reserve(
        "control-retention-hold",
        s::Resources {
            retained_bytes: remaining.checked_sub(64 * 1024).unwrap(),
            ..s::Resources::default()
        },
        class,
    )
    .unwrap();
    for revision in 2..=512u64 {
        object.revision = revision;
        match host.compare_and_swap_batch_in_class(
            vec![s::Change {
                expected_revision: Some(revision - 1),
                object: object.clone(),
                depends_on: Vec::new(),
            }],
            class,
        ) {
            Ok(_) => (),
            Err(s::StoreError::Quota {
                dimension: s::Resource::RetainedBytes,
                class: exhausted,
            }) if exhausted == class => return,
            other => panic!("unexpected retention pressure result: {other:?}"),
        }
    }
    panic!("retention did not exhaust within the bounded fixture");
}

fn closeout_terminal(prepared: &Prepared) -> s::StoredObject {
    let bytes = serde_json::to_vec(&decision(prepared, true)).unwrap();
    let hash = s::sha256_bytes(&bytes);
    s::StoredObject {
        id: format!(
            "terminal:{}",
            s::sha256_bytes(prepared.request.id.as_bytes())
        ),
        revision: 1,
        kind: "runtime-terminal-transport".into(),
        body: json!({
            "invocation_id":prepared.request.id,
            "prepared_epoch":prepared.epoch,
            "prompt_sha256":prepared.prompt_sha256,
            "receipt":{"kind":"synthetic_complete","synthetic":true,
                "inner_sha256":hash,"transport_sha256":hash,
                "response_bytes":bytes.len(),"provider_usage":"unknown"}
        }),
    }
}

fn closeout_cli(root: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_ai-personas"))
        .arg("--state-root")
        .arg(root)
        .args(["--operator", "operator"])
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn protected_closeout_reopens_after_ordinary_exhaustion_without_funding_new_work() {
    for exhaust_navigation in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("protected-terminal");
        let root = f.app.state_root().to_owned();
        // Open the cooperating Store before dispatch, preserving the existing
        // recovery barrier and avoiding any assumption about a new constructor.
        let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
        host.mark_invocation_uncertain(&prepared.request.id, "Terminal observation pending")
            .unwrap();
        if exhaust_navigation {
            exhaust_control_navigation(&mut host, s::BudgetClass::Ordinary);
        }
        exhaust_control_retention(&mut host, s::BudgetClass::Ordinary);
        let state_before = persisted_state(&f.app);
        let protected_before = f.app.ledger().unwrap().closeout;
        assert!(matches!(
            f.app.add_persona_seed("operator", seed("additional")),
            Err(RuntimeError::Store(s::StoreError::Quota {
                class: s::BudgetClass::Ordinary,
                ..
            }))
        ));
        assert!(matches!(
            f.app.pause(
                "operator",
                "work",
                "Ordinary control capacity is exhausted."
            ),
            Err(RuntimeError::Store(s::StoreError::Quota {
                class: s::BudgetClass::Ordinary,
                ..
            }))
        ));
        if exhaust_navigation {
            assert!(matches!(
                f.app.snapshot(),
                Err(RuntimeError::Store(s::StoreError::Quota {
                    dimension: s::Resource::NavigationBytes,
                    class: s::BudgetClass::Ordinary,
                }))
            ));
            assert!(matches!(
                f.app.resume("operator", "a", "work"),
                Err(RuntimeError::Store(s::StoreError::Quota {
                    dimension: s::Resource::NavigationBytes,
                    class: s::BudgetClass::Ordinary,
                }))
            ));
            assert!(matches!(
                f.app.prepare(
                    PrepareRequest {
                        id: "unfunded".into(),
                        persona: "a".into(),
                        work: "work".into(),
                        profile: Profile::Synthetic,
                        mode: Mode::Ordinary,
                    },
                    NOW,
                ),
                Err(RuntimeError::Store(s::StoreError::Quota {
                    dimension: s::Resource::NavigationBytes,
                    class: s::BudgetClass::Ordinary,
                }))
            ));
        }
        assert_eq!(persisted_state(&f.app), state_before);
        assert_eq!(f.app.ledger().unwrap().closeout, protected_before);

        // Evidence arriving after exhaustion still fills only its prepaid slot.
        let terminal = closeout_terminal(&prepared);
        let before_fill = host.ledger().unwrap();
        let reference = host
            .trusted_host()
            .record_terminal_evidence(terminal.clone(), |subject, object| {
                assert_eq!(
                    subject,
                    &s::EntityId::Invocation(prepared.request.id.clone())
                );
                assert_eq!(object, &terminal);
                Ok(())
            })
            .unwrap();
        assert_eq!(host.ledger().unwrap(), before_fill);
        let before = host.ledger().unwrap();
        let old = closeout_head(&root);
        drop(host);
        drop(f.app);
        if exhaust_navigation {
            let durable = closeout_snapshot(&root);
            assert!(matches!(
                Runtime::open(&root),
                Err(RuntimeError::Store(s::StoreError::Quota {
                    dimension: s::Resource::NavigationBytes,
                    class: s::BudgetClass::Ordinary,
                }))
            ));
            assert_eq!(closeout_snapshot(&root), durable);
        }
        let mut app = Runtime::open_for_closeout(&root).unwrap();
        assert_eq!(
            app.terminal_evidence(&prepared.request.id).unwrap(),
            terminal
        );
        app.acknowledge_complete_rejection("operator", &prepared.request.id, reference)
            .unwrap();
        app.pause(
            "operator",
            "work",
            "Pause under existing protected capacity.",
        )
        .unwrap();
        drop(app);

        // Exercise executable routing and the adopted archive lookup, which
        // previously performed an ordinary read inside terminal inspection.
        let output = closeout_cli(&root, &["terminal-evidence", "accept"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let accepted_terminal: s::StoredObject = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            accepted_terminal.body["receipt"]["kind"],
            "synthetic_complete"
        );
        let output = closeout_cli(
            &root,
            &[
                "closeout",
                "work",
                "Record an unfinished ending within existing capacity.",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["closed_out"], true);
        assert_eq!(result["goal_completed"], false);

        let app = Runtime::open_for_closeout(&root).unwrap();
        let after = app.ledger().unwrap();
        assert_eq!(after.ordinary, before.ordinary);
        assert_eq!(
            after.closeout.consumed.attempts,
            before.closeout.consumed.attempts
        );
        assert!(
            after.closeout.consumed.navigation_bytes > before.closeout.consumed.navigation_bytes
        );
        assert_eq!(
            after.closeout.consumed.retained_bytes,
            before.closeout.consumed.retained_bytes
        );
        assert_eq!(after.permanent_records, before.permanent_records);
        assert_eq!(
            after.allocated_history_events,
            before.allocated_history_events
        );
        let head = closeout_head(&root);
        let mut preserved = old.body.clone();
        preserved["board"] = head.body["board"].clone();
        preserved["operator_events"] = head.body["operator_events"].clone();
        assert_eq!(head.body, preserved);
        assert_eq!(head.revision, old.revision + 2);
        let state: State = serde_json::from_value(head.body).unwrap();
        assert!(!matches!(
            state.board.work(&w::WorkId("work".into())).unwrap().phase,
            w::Phase::Open
        ));
        assert!(!state.board.completion_supported(&w::WorkId("work".into())));
        assert!(!state.personas["a"].focus["work"].permit);
        let (_, cells) = closeout_snapshot(&root);
        let head_cell = cells
            .iter()
            .find(|cell| cell.0 == "object" && cell.1 == "runtime-state-v1")
            .unwrap();
        assert_eq!(
            head_cell.4, 0,
            "the ordinary prepaid head must not be relabeled"
        );
        let mut journals: Vec<s::StoredObject> = cells
            .iter()
            .filter(|cell| cell.0 == "runtime_control" && cell.4 == 1)
            .filter_map(|cell| {
                let slot: Value = serde_json::from_slice(&cell.2).unwrap();
                slot.get("commit")
                    .filter(|value| !value.is_null())
                    .map(|record| serde_json::from_value(record["object"].clone()).unwrap())
            })
            .collect();
        journals.sort_by_key(|object| object.revision);
        assert_eq!(journals.len(), 2);
        assert_eq!(journals[0].revision, old.revision + 1);
        assert_eq!(journals[1].revision, old.revision + 2);
        drop(app);
        let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        let invocation = host
            .trusted_host()
            .invocation(&prepared.request.id)
            .unwrap();
        assert_eq!(invocation.state, s::InvocationState::Failed);
        assert!(host.dispatch_invocation(&prepared.request.id).is_err());
        let reservation = host
            .trusted_host()
            .reservation(&invocation.reservation)
            .unwrap();
        assert_eq!(reservation.state, s::ReservationState::Uncertain);
        assert_eq!(reservation.measured, None);
        assert_eq!(host.ledger().unwrap().ordinary, before.ordinary);
    }
}

fn control_records(root: &std::path::Path, work: &str) -> Vec<Value> {
    let mut records = closeout_snapshot(root)
        .1
        .into_iter()
        .filter(|cell| cell.0 == "runtime_control")
        .filter_map(|cell| {
            let slot: Value = serde_json::from_slice(&cell.2).unwrap();
            (slot["work"] == work && !slot["commit"].is_null()).then(|| slot["commit"].clone())
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|record| record["object"]["revision"].as_u64().unwrap());
    records
}

fn assert_resume_write_rollback(mut before: CloseoutSnapshot, after: CloseoutSnapshot) {
    for class in ["ordinary", "closeout"] {
        let previous = before.0[class]["consumed"]["navigation_bytes"]
            .as_u64()
            .unwrap();
        let current = after.0[class]["consumed"]["navigation_bytes"]
            .as_u64()
            .unwrap();
        assert!(current >= previous);
        before.0[class]["consumed"]["navigation_bytes"] = json!(current);
    }
    assert_eq!(before, after);
}

// With no active invocation, resume adds exactly one pause allocation and one
// ordinary immutable snapshot. Account for their keys, envelopes and peak copies.
fn assert_resume_allocation(before: &CloseoutSnapshot, after: &CloseoutSnapshot, work: &str) {
    let added = after
        .1
        .iter()
        .filter(|cell| {
            !before
                .1
                .iter()
                .any(|old| old.0 == cell.0 && old.1 == cell.1)
        })
        .collect::<Vec<_>>();
    assert_eq!(added.len(), 2);
    let pause = added
        .iter()
        .find(|cell| cell.0 == "runtime_control")
        .unwrap();
    assert_eq!(pause.3 as u64, storage_limits().max_record_bytes);
    assert_eq!((pause.4, pause.5), (1, 1));
    let slot: Value = serde_json::from_slice(&pause.2).unwrap();
    assert_eq!(slot["work"], work);
    assert_eq!(slot["ending"], false);
    assert!(slot["commit"].is_null());
    let revision = added.iter().find(|cell| cell.0 == "revision").unwrap();
    assert_eq!((revision.4, revision.5), (0, 0));
    let mut expected = before.0.clone();
    expected["epoch"] = json!(before.0["epoch"].as_u64().unwrap() + 1);
    expected["identities"] = json!(before.0["identities"].as_u64().unwrap() + 2);
    expected["history_slots"] = json!(before.0["history_slots"].as_u64().unwrap() + 1);
    for (class, lane) in [("ordinary", 0), ("closeout", 1)] {
        let charge = added
            .iter()
            .filter(|cell| cell.4 == lane)
            .map(|cell| 2 * (cell.3 as u64 + cell.0.len() as u64 + cell.1.len() as u64 + 128))
            .sum::<u64>();
        expected[class]["consumed"]["retained_bytes"] = json!(
            before.0[class]["consumed"]["retained_bytes"]
                .as_u64()
                .unwrap()
                + charge
        );
    }
    assert!(
        after.0["ordinary"]["consumed"]["navigation_bytes"]
            .as_u64()
            .unwrap()
            > before.0["ordinary"]["consumed"]["navigation_bytes"]
                .as_u64()
                .unwrap()
    );
    expected["ordinary"]["consumed"]["navigation_bytes"] =
        after.0["ordinary"]["consumed"]["navigation_bytes"].clone();
    assert_eq!(expected, after.0);
    for cell in before.1.iter().filter(|cell| cell.0 != "object") {
        assert_eq!(
            after
                .1
                .iter()
                .find(|next| next.0 == cell.0 && next.1 == cell.1),
            Some(cell)
        );
    }
}

#[test]
fn protected_pause_resume_cycles_preserve_receipts_and_interleaved_work() {
    let mut f = Fixture::new(true);
    f.accept();
    let mut second = need();
    second.id = "second".into();
    f.app.add_need("operator", second).unwrap();
    let root = f.app.state_root().to_owned();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let mut original_request = control_request(
        &host,
        &root,
        "placeholder",
        "work",
        s::RuntimeControlAction::Pause,
    );
    // Exercise compatibility with an already committed pre-fix deterministic ID.
    original_request.id = format!(
        "control:{}",
        s::sha256_bytes(&serde_json::to_vec(&original_request.control).unwrap(),)
    );
    let receipt = host
        .trusted_host()
        .commit_runtime_control(original_request.clone())
        .unwrap();
    let original_pause = closeout_head(&root);
    let original_record = control_records(&root, "work")[0].clone();
    let reason = original_request.control.reason.as_str();
    let mut closeout = Runtime::open_for_closeout(&root).unwrap();
    closeout.pause("operator", "second", reason).unwrap();
    let before_retry = closeout_snapshot(&root);
    closeout.pause("operator", "work", reason).unwrap();
    assert_closeout_write_rollback(before_retry, closeout_snapshot(&root));
    let paused = closeout_head(&root);
    let original_cells = closeout_snapshot(&root)
        .1
        .into_iter()
        .filter(|cell| {
            cell.0 == "runtime_control"
                && !serde_json::from_slice::<Value>(&cell.2).unwrap()["commit"].is_null()
        })
        .collect::<Vec<_>>();
    drop(closeout);
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    let before_resume = closeout_snapshot(&root);
    f.app.resume("operator", "a", "work").unwrap();
    let after_resume = closeout_snapshot(&root);
    assert_resume_allocation(&before_resume, &after_resume, "work");
    let reopened = closeout_head(&root);
    assert_eq!(reopened.body["board"]["works"]["work"]["phase"], "Open");
    assert_eq!(
        reopened.body["board"]["works"]["second"],
        paused.body["board"]["works"]["second"]
    );
    assert_eq!(reopened.body["work_meta"], paused.body["work_meta"]);
    assert_eq!(reopened.body["sources"], paused.body["sources"]);
    assert_eq!(
        reopened.body["personas"]["a"]["focus"]["work"]["permit"],
        true
    );
    // Historical replay remains exact and cannot restore the old paused head.
    let before_replay = closeout_snapshot(&root);
    assert_eq!(
        host.trusted_host()
            .commit_runtime_control(original_request.clone())
            .unwrap(),
        receipt
    );
    let mut conflict = original_request.clone();
    conflict.control.reason.push('!');
    assert!(matches!(
        host.trusted_host().commit_runtime_control(conflict),
        Err(s::StoreError::Conflict(_))
    ));
    assert_eq!(closeout_snapshot(&root), before_replay);
    assert_eq!(closeout_head(&root), reopened);
    let mut closeout = Runtime::open_for_closeout(&root).unwrap();
    let mut ledger = closeout.ledger().unwrap();
    closeout.pause("operator", "work", reason).unwrap();
    let second_pause = closeout_head(&root);
    assert_eq!(second_pause.revision, reopened.revision + 1);
    assert_eq!(
        second_pause.body["board"]["works"]["work"]["phase"]["Paused"]["reason"],
        reason
    );
    let records = control_records(&root, "work");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0], original_record);
    assert_ne!(records[1]["request"]["id"], original_request.id);
    let second_record = records[1].clone();
    let after_pause = closeout.ledger().unwrap();
    ledger.closeout.consumed.navigation_bytes = after_pause.closeout.consumed.navigation_bytes;
    assert_eq!(ledger, after_pause);
    let before_second_resume = closeout_snapshot(&root);
    f.app.resume("operator", "a", "second").unwrap();
    assert_resume_allocation(&before_second_resume, &closeout_snapshot(&root), "second");
    assert_eq!(
        closeout_head(&root).body["board"]["works"]["work"],
        second_pause.body["board"]["works"]["work"]
    );
    let before_retry = closeout_snapshot(&root);
    closeout.pause("operator", "work", reason).unwrap();
    assert_closeout_write_rollback(before_retry, closeout_snapshot(&root));
    assert_eq!(
        closeout_head(&root).body["board"]["works"]["second"]["phase"],
        "Open"
    );
    let mut ledger = closeout.ledger().unwrap();
    closeout.pause("operator", "second", reason).unwrap();
    closeout
        .closeout("operator", "work", false, "Finite unfinished ending.")
        .unwrap();
    let after_ending = closeout.ledger().unwrap();
    ledger.closeout.consumed.navigation_bytes = after_ending.closeout.consumed.navigation_bytes;
    assert_eq!(ledger, after_ending);
    let ended = closeout_head(&root);
    assert_eq!(
        ended.body["board"]["works"]["work"]["phase"]["Ended"]["kind"],
        "Unfinished"
    );
    assert!(ended.body["board"]["works"]["second"]["phase"]
        .get("Paused")
        .is_some());
    assert_eq!(ended.body["work_meta"], original_pause.body["work_meta"]);
    let state: State = serde_json::from_value(ended.body.clone()).unwrap();
    assert!(!state.board.completion_supported(&w::WorkId("work".into())));
    let records = control_records(&root, "work");
    assert_eq!(records.len(), 3);
    assert_eq!(records[0], original_record);
    assert_eq!(records[1], second_record);
    let final_snapshot = closeout_snapshot(&root);
    for cell in &original_cells {
        assert_eq!(
            final_snapshot
                .1
                .iter()
                .find(|next| next.0 == cell.0 && next.1 == cell.1),
            Some(cell)
        );
    }
    assert_eq!(
        host.trusted_host()
            .commit_runtime_control(original_request.clone())
            .unwrap(),
        receipt
    );
    assert_eq!(closeout_snapshot(&root), final_snapshot);
    assert_eq!(
        host.trusted_host()
            .read_runtime_control_request(&original_request.id, &original_request.control)
            .unwrap(),
        Some(original_request.clone())
    );
    assert_eq!(
        host.trusted_host().read_exact(&receipt.objects[0]).unwrap(),
        original_pause
    );
    let second_receipt: s::BatchReceipt =
        serde_json::from_value(second_record["receipt"].clone()).unwrap();
    assert_eq!(
        host.trusted_host()
            .read_exact(&second_receipt.objects[0])
            .unwrap(),
        second_pause
    );
}

#[test]
fn insufficient_resume_allocation_and_freeze_preserve_pending_recovery() {
    for responded in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("resume-recovery");
        let root = f.app.state_root().to_owned();
        let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        let mut closeout = Runtime::open_for_closeout(&root).unwrap();
        if responded {
            f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
            host.record_response(
                &prepared.request.id,
                &s::sha256_bytes(b"retained fixture response"),
            )
            .unwrap();
        }
        let invocation = f.app.invocation(&prepared.request.id).unwrap();
        closeout
            .pause("operator", "work", "Pause before explicit recovery.")
            .unwrap();
        let trip = host
            .reserve(
                "resume-freeze-trip",
                s::Resources {
                    output_bytes: 1,
                    ..s::Resources::default()
                },
                s::BudgetClass::Ordinary,
            )
            .unwrap();
        exhaust_control_retention(&mut host, s::BudgetClass::Closeout);
        let before = closeout_snapshot(&root);
        assert!(matches!(
            f.app.resume("operator", "a", "work"),
            Err(RuntimeError::Store(s::StoreError::Quota {
                dimension: s::Resource::RetainedBytes,
                class: s::BudgetClass::Closeout,
            }))
        ));
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        assert_eq!(f.app.invocation(&prepared.request.id).unwrap(), invocation);
        assert_eq!(
            closeout_head(&root).body["personas"]["a"]["active_invocation"],
            prepared.request.id
        );
        assert!(matches!(
            host.settle(
                &trip.id,
                s::Usage::Measured(s::Resources {
                    output_bytes: 2,
                    ..s::Resources::default()
                })
            ),
            Err(s::StoreError::Overrun(_))
        ));
        let frozen = closeout_snapshot(&root);
        assert!(matches!(
            f.app.resume("operator", "a", "work"),
            Err(RuntimeError::Store(s::StoreError::Frozen))
        ));
        assert_eq!(closeout_snapshot(&root), frozen);
        closeout
            .closeout(
                "operator",
                "work",
                true,
                "Prepaid cancellation remains available.",
            )
            .unwrap();
        assert_eq!(f.app.invocation(&prepared.request.id).unwrap(), invocation);
        assert_eq!(
            closeout_head(&root).body["board"]["works"]["work"]["phase"]["Ended"]["kind"],
            "Canceled"
        );
    }
}

#[test]
fn resume_late_write_failure_rolls_back_new_slot_head_and_abandonment() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("atomic-resume");
    let root = f.app.state_root().to_owned();
    let mut closeout = Runtime::open_for_closeout(&root).unwrap();
    closeout
        .pause("operator", "work", "Pause the prepared local decision.")
        .unwrap();
    let original_record = control_records(&root, "work")[0].clone();
    let fault = Connection::open(root.join("state.sqlite3")).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER resume_slot_fault BEFORE INSERT ON store_cells
         WHEN NEW.ns='runtime_control' AND json_extract(NEW.data,'$.work')='work'
              AND json_extract(NEW.data,'$.ending')=0
         BEGIN
             SELECT CASE WHEN NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='object' AND k='runtime-state-v1'
                   AND json_extract(data,'$.body.board.works.work.phase')='Open'
             ) OR NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='invocation' AND k='atomic-resume'
                   AND json_extract(data,'$.state')='Failed'
             ) OR NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='reservation' AND k='v:atomic-resume'
                   AND json_extract(data,'$.state')='Uncertain'
             ) THEN RAISE(ABORT,'atomic resume writes missing') END;
             SELECT RAISE(ABORT,'injected resume allocation failure');
         END;",
        )
        .unwrap();
    let before = closeout_snapshot(&root);
    let error = f.app.resume("operator", "a", "work").unwrap_err();
    assert!(error
        .to_string()
        .contains("injected resume allocation failure"));
    assert_resume_write_rollback(before, closeout_snapshot(&root));
    assert_eq!(
        f.app.invocation(&prepared.request.id).unwrap().state,
        s::InvocationState::Prepared
    );
    fault
        .execute_batch("DROP TRIGGER resume_slot_fault")
        .unwrap();
    drop(fault);
    drop(closeout);
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    let before = f.app.ledger().unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let after = f.app.ledger().unwrap();
    assert_eq!(after.permanent_records, before.permanent_records + 2);
    assert_eq!(
        after.allocated_history_events,
        before.allocated_history_events + 1
    );
    assert_eq!(
        after.ordinary.consumed.attempts,
        before.ordinary.consumed.attempts
    );
    assert_eq!(
        after
            .ordinary
            .reserved
            .checked_add(after.ordinary.uncertain)
            .unwrap(),
        before
            .ordinary
            .reserved
            .checked_add(before.ordinary.uncertain)
            .unwrap()
    );
    assert_eq!(
        f.app.invocation(&prepared.request.id).unwrap().state,
        s::InvocationState::Failed
    );
    assert!(closeout_head(&root).body["personas"]["a"]["active_invocation"].is_null());
    assert_eq!(
        closeout_head(&root).body["board"]["works"]["work"]["phase"],
        "Open"
    );
    assert_eq!(control_records(&root, "work")[0], original_record);
}

#[test]
fn exhausted_protected_retention_uses_prepaid_pause_and_ending_journals() {
    let mut f = Fixture::new(true);
    f.accept();
    let root = f.app.state_root().to_owned();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    exhaust_control_retention(&mut host, s::BudgetClass::Closeout);
    drop(host);
    drop(f.app);
    let mut app = Runtime::open_for_closeout(&root).unwrap();
    let before = app.ledger().unwrap();
    app.pause("operator", "work", "Bounded pause requested.")
        .unwrap();
    // Provisioning preserves the spent slot. Resume needs a distinct allocation,
    // which the actual aggregate retention exhaustion prevents here.
    let mut ordinary = Runtime::open(&root).unwrap();
    ordinary.provision_closeout("operator").unwrap();
    let before_resume = closeout_snapshot(&root);
    assert!(matches!(
        ordinary.resume("operator", "a", "work"),
        Err(RuntimeError::Store(s::StoreError::Quota {
            dimension: s::Resource::RetainedBytes,
            class: s::BudgetClass::Closeout,
        }))
    ));
    assert_resume_write_rollback(before_resume, closeout_snapshot(&root));
    drop(ordinary);
    let snapshot = closeout_snapshot(&root);
    assert!(matches!(
        app.pause("operator", "work", "A different pause."),
        Err(RuntimeError::Store(s::StoreError::Conflict(_)))
    ));
    assert_closeout_write_rollback(snapshot, closeout_snapshot(&root));
    app.closeout("operator", "work", true, "Bounded cancellation requested.")
        .unwrap();
    let after = app.ledger().unwrap();
    assert_eq!(
        after.closeout.consumed.retained_bytes,
        before.closeout.consumed.retained_bytes
    );
    assert_eq!(after.permanent_records, before.permanent_records);
    assert_eq!(
        after.allocated_history_events,
        before.allocated_history_events
    );
    let head = closeout_head(&root);
    assert_eq!(
        head.body["board"]["works"]["work"]["phase"]["Ended"]["kind"],
        "Canceled"
    );
    app.closeout("operator", "work", true, "Bounded cancellation requested.")
        .unwrap();
    assert_eq!(closeout_head(&root), head);
}

#[test]
fn exhausted_protected_navigation_refuses_open_without_partial_charge() {
    let mut f = Fixture::new(true);
    f.accept();
    let root = f.app.state_root().to_owned();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    exhaust_control_navigation(&mut host, s::BudgetClass::Closeout);
    drop(host);
    drop(f.app);
    let before = closeout_snapshot(&root);
    assert!(matches!(
        Runtime::open_for_closeout(&root),
        Err(RuntimeError::Store(s::StoreError::Quota {
            dimension: s::Resource::NavigationBytes,
            class: s::BudgetClass::Closeout,
        }))
    ));
    assert_eq!(closeout_snapshot(&root), before);
}

#[test]
fn frozen_protected_open_pauses_reopens_and_records_a_prepaid_unfinished_ending() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("frozen-terminal");
    let root = f.app.state_root().to_owned();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let trip = host
        .reserve(
            "control-freeze-trip",
            s::Resources {
                output_bytes: 1,
                ..s::Resources::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    host.mark_invocation_uncertain(&prepared.request.id, "Terminal observation pending")
        .unwrap();
    exhaust_control_navigation(&mut host, s::BudgetClass::Ordinary);
    exhaust_control_retention(&mut host, s::BudgetClass::Ordinary);
    assert!(matches!(
        host.settle(
            &trip.id,
            s::Usage::Measured(s::Resources {
                output_bytes: 2,
                ..s::Resources::default()
            }),
        ),
        Err(s::StoreError::Overrun(_))
    ));
    let terminal = closeout_terminal(&prepared);
    let frozen = host.ledger().unwrap();
    let reference = host
        .trusted_host()
        .record_terminal_evidence(terminal.clone(), |subject, object| {
            assert_eq!(
                subject,
                &s::EntityId::Invocation(prepared.request.id.clone())
            );
            assert_eq!(object, &terminal);
            Ok(())
        })
        .unwrap();
    assert_eq!(host.ledger().unwrap(), frozen);
    drop(host);
    drop(f.app);
    let mut app = Runtime::open_for_closeout(&root).unwrap();
    assert_eq!(
        app.terminal_evidence(&prepared.request.id).unwrap(),
        terminal
    );
    app.acknowledge_complete_rejection("operator", &prepared.request.id, reference)
        .unwrap();
    let original = closeout_head(&root);
    app.pause("operator", "work", "Frozen pause requested.")
        .unwrap();
    let paused = closeout_head(&root);
    assert!(paused.body["board"]["works"]["work"]["phase"]
        .get("Paused")
        .is_some());
    assert_eq!(paused.body["work_meta"], original.body["work_meta"]);
    assert_eq!(paused.body["sources"], original.body["sources"]);
    assert_eq!(paused.body["observations"], original.body["observations"]);
    drop(app);
    let mut app = Runtime::open_for_closeout(&root).unwrap();
    app.pause("operator", "work", "Frozen pause requested.")
        .unwrap();
    assert_eq!(closeout_head(&root), paused);
    app.closeout("operator", "work", false, "Frozen ending requested.")
        .unwrap();
    let ended = closeout_head(&root);
    assert_eq!(
        ended.body["board"]["works"]["work"]["phase"]["Ended"]["kind"],
        "Unfinished"
    );
    assert_eq!(ended.body["work_meta"], original.body["work_meta"]);
    assert_eq!(ended.body["sources"], original.body["sources"]);
    assert_eq!(ended.body["observations"], original.body["observations"]);
    assert_eq!(
        ended.body["personas"]["a"]["active_invocation"],
        original.body["personas"]["a"]["active_invocation"]
    );
    assert_eq!(
        ended.body["board"]["works"]["work"]["acceptances"],
        original.body["board"]["works"]["work"]["acceptances"]
    );
    assert_eq!(
        app.ledger().unwrap().permanent_records,
        frozen.permanent_records
    );
    assert_eq!(
        app.ledger().unwrap().allocated_history_events,
        frozen.allocated_history_events
    );
    assert_eq!(
        app.ledger().unwrap().closeout.consumed.retained_bytes,
        frozen.closeout.consumed.retained_bytes
    );
    assert!(app.ledger().unwrap().frozen);
    assert!(!app.ledger().unwrap().conservation_established);
    assert_eq!(app.ledger().unwrap().ordinary, frozen.ordinary);
    drop(app);
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let before = closeout_snapshot(&root);
    assert!(matches!(
        host.compare_and_swap_batch_in_class(
            vec![s::Change {
                expected_revision: None,
                object: s::StoredObject {
                    id: "arbitrary-frozen-write".into(),
                    revision: 1,
                    kind: "runtime-state".into(),
                    body: json!({"protected":true}),
                },
                depends_on: Vec::new(),
            }],
            s::BudgetClass::Closeout,
        ),
        Err(s::StoreError::Frozen)
    ));
    assert_eq!(closeout_snapshot(&root), before);
}

fn control_request(
    host: &s::Store,
    root: &std::path::Path,
    id: &str,
    work: &str,
    action: s::RuntimeControlAction,
) -> s::RuntimeControlRequest {
    s::RuntimeControlRequest {
        id: id.into(),
        expected_epoch: host.epoch().unwrap(),
        expected: closeout_head(root).exact_ref().unwrap(),
        control: s::RuntimeControl {
            operator: "operator".into(),
            work: work.into(),
            reason: "Explicit bounded control.".into(),
            action,
        },
    }
}

#[test]
fn prepaid_controls_are_atomic_exact_fenced_and_independent_across_works() {
    let mut f = Fixture::new(true);
    f.accept();
    let mut second = need();
    second.id = "second".into();
    f.app.add_need("operator", second).unwrap();
    let root = f.app.state_root().to_owned();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    // Extension fields participate in capacity and survive the narrow transition.
    let mut original = closeout_head(&root);
    original.revision += 1;
    original.body["future_scheduler_state"] = json!({"events":["retained event"],"remaining":7});
    original.body["board"]["works"]["work"]["future_obligation"] = json!({"unresolved":true});
    host.compare_and_swap(Some(original.revision - 1), original.clone())
        .unwrap();
    let trip = host
        .reserve(
            "control-fault-trip",
            s::Resources {
                output_bytes: 1,
                ..s::Resources::default()
            },
            s::BudgetClass::Ordinary,
        )
        .unwrap();
    assert!(matches!(
        host.settle(
            &trip.id,
            s::Usage::Measured(s::Resources {
                output_bytes: 2,
                ..s::Resources::default()
            })
        ),
        Err(s::StoreError::Overrun(_))
    ));
    // Freeze must refuse ordinary resume before it changes lifecycle receipts.
    let before = closeout_snapshot(&root);
    assert!(matches!(
        f.app.resume("operator", "a", "work"),
        Err(RuntimeError::Store(s::StoreError::Frozen))
    ));
    assert!(matches!(
        f.app.prepare(
            PrepareRequest {
                id: "frozen-new".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW
        ),
        Err(RuntimeError::Store(s::StoreError::Frozen))
    ));
    assert_eq!(closeout_snapshot(&root), before);
    let pause = control_request(
        &host,
        &root,
        "pause-first",
        "work",
        s::RuntimeControlAction::Pause,
    );
    let fault = Connection::open(root.join("state.sqlite3")).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER control_fill_fault BEFORE UPDATE ON store_cells
        WHEN NEW.ns='runtime_control' AND json_type(NEW.data,'$.commit')='object'
        BEGIN
            SELECT CASE WHEN NOT EXISTS (
                SELECT 1 FROM store_cells WHERE ns='object' AND k='runtime-state-v1'
                AND json_type(data,'$.body.board.works.work.phase.Paused')='object'
            ) THEN RAISE(ABORT,'head transition missing') END;
            SELECT RAISE(ABORT,'injected control fill failure');
        END;",
        )
        .unwrap();
    let error = host
        .trusted_host()
        .commit_runtime_control(pause.clone())
        .unwrap_err();
    assert!(error.to_string().contains("injected control fill failure"));
    assert_eq!(closeout_snapshot(&root), before);
    fault
        .execute_batch("DROP TRIGGER control_fill_fault")
        .unwrap();
    drop(fault);
    drop(host);
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    assert_eq!(closeout_snapshot(&root), before);
    let receipt = host
        .trusted_host()
        .commit_runtime_control(pause.clone())
        .unwrap();
    let paused = closeout_head(&root);
    assert_eq!(
        paused.body["future_scheduler_state"],
        original.body["future_scheduler_state"]
    );
    assert_eq!(
        paused.body["board"]["works"]["work"]["future_obligation"],
        original.body["board"]["works"]["work"]["future_obligation"]
    );
    assert_eq!(
        paused.body["board"]["works"]["second"],
        original.body["board"]["works"]["second"]
    );
    assert_eq!(paused.body["work_meta"], original.body["work_meta"]);
    let committed = closeout_snapshot(&root);
    assert_eq!(
        host.trusted_host()
            .commit_runtime_control(pause.clone())
            .unwrap(),
        receipt
    );
    assert_eq!(closeout_snapshot(&root), committed);
    for variant in 0..5 {
        let mut request = control_request(
            &host,
            &root,
            "next",
            "second",
            s::RuntimeControlAction::Pause,
        );
        match variant {
            0 => {
                request.id = pause.id.clone();
                request.control.reason.push('!');
            }
            1 => {
                request.id = pause.id.clone();
            }
            2 => {
                request.control.work = "work".into();
            }
            3 => {
                request.expected = pause.expected.clone();
            }
            4 => {
                request.expected_epoch = pause.expected_epoch;
            }
            _ => unreachable!(),
        }
        let error = host
            .trusted_host()
            .commit_runtime_control(request)
            .unwrap_err();
        match variant {
            0..=2 => assert!(matches!(error, s::StoreError::Conflict(_))),
            3 => assert!(matches!(error, s::StoreError::RevisionConflict(_))),
            4 => assert!(matches!(error, s::StoreError::StaleEpoch { .. })),
            _ => unreachable!(),
        }
        assert_eq!(closeout_snapshot(&root), committed);
    }
    for (id, work, action) in [
        ("pause-second", "second", s::RuntimeControlAction::Pause),
        ("end-first", "work", s::RuntimeControlAction::Canceled),
        ("end-second", "second", s::RuntimeControlAction::Unfinished),
    ] {
        let request = control_request(&host, &root, id, work, action);
        host.trusted_host().commit_runtime_control(request).unwrap();
    }
    let ended = closeout_head(&root);
    assert_eq!(
        ended.body["board"]["works"]["work"]["phase"]["Ended"]["kind"],
        "Canceled"
    );
    assert_eq!(
        ended.body["board"]["works"]["second"]["phase"]["Ended"]["kind"],
        "Unfinished"
    );
    assert_eq!(ended.body["work_meta"], original.body["work_meta"]);
    let after = closeout_snapshot(&root);
    assert_eq!(after.0["ordinary"], before.0["ordinary"]);
    assert_eq!(after.0["closeout"], before.0["closeout"]);
    assert_eq!(after.0["identities"], before.0["identities"]);
    assert_eq!(after.0["history_slots"], before.0["history_slots"]);
    assert_eq!(
        after.0["sequence"].as_u64().unwrap(),
        before.0["sequence"].as_u64().unwrap() + 4
    );
    assert_eq!(
        host.trusted_host().commit_runtime_control(pause).unwrap(),
        receipt
    );
    assert_eq!(closeout_snapshot(&root), after);
    assert_eq!(
        host.trusted_host().read_exact(&receipt.objects[0]).unwrap(),
        paused
    );
    assert_eq!(
        host.trusted_host()
            .read_exact(&original.exact_ref().unwrap())
            .unwrap(),
        original
    );
    let connection = Connection::open(root.join("state.sqlite3")).unwrap();
    assert!(connection
        .execute(
            "UPDATE store_cells SET data=data WHERE ns='runtime_control'",
            []
        )
        .is_err());
}

#[test]
fn control_snapshot_growth_is_rejected_before_optional_state_or_capacity_commits() {
    let f = Fixture::new(true);
    let root = f.app.state_root().to_owned();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let original = closeout_head(&root);
    let mut enlarged = original.clone();
    enlarged.revision += 1;
    enlarged.body["future_event_payload"] = json!("x".repeat(4 * 1024 * 1024 - 100_000));
    assert!(serde_json::to_vec(&enlarged.body).unwrap().len() < 4 * 1024 * 1024);
    let before = closeout_snapshot(&root);
    assert!(matches!(
        host.compare_and_swap(Some(original.revision), enlarged),
        Err(s::StoreError::TooLarge)
    ));
    assert_eq!(closeout_snapshot(&root), before);
    // Logical accounting includes the complete reserved slot and both peak copies.
    let allocation: u64 = 2 * 16 * 1024
        + before
            .1
            .iter()
            .map(|cell| 2 * (cell.3 as u64 + cell.0.len() as u64 + cell.1.len() as u64 + 128))
            .sum::<u64>();
    let ledger = host.ledger().unwrap();
    assert_eq!(
        allocation,
        ledger.ordinary.consumed.retained_bytes + ledger.closeout.consumed.retained_bytes
    );
    assert!(allocation <= ledger.limits.total.retained_bytes);
    let slots: Vec<_> = before
        .1
        .iter()
        .filter(|cell| cell.0 == "runtime_control")
        .collect();
    assert_eq!(slots.len(), 2);
    for slot in slots {
        assert_eq!(slot.3 as u64, storage_limits().max_record_bytes);
        assert_eq!(slot.4, 1);
        assert_eq!(slot.5, 1);
        let value: Value = serde_json::from_slice(&slot.2).unwrap();
        assert!(value["commit"].is_null());
    }
    let pause = control_request(
        &host,
        &root,
        "bounded-pause",
        "work",
        s::RuntimeControlAction::Pause,
    );
    host.trusted_host().commit_runtime_control(pause).unwrap();
    assert_eq!(host.ledger().unwrap().ordinary, ledger.ordinary);
    assert_eq!(host.ledger().unwrap().closeout, ledger.closeout);
}

#[test]
fn legacy_closeout_requires_explicit_unfrozen_prepaid_capacity_without_funding_reset() {
    for provision in ["absent", "available", "exhausted"] {
        let f = Fixture::new(true);
        let root = f._temp.path().join("legacy");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        // A legacy object has real retained state but no prepaid control records.
        let original = s::StoredObject {
            revision: 1,
            ..closeout_head(f.app.state_root())
        };
        host.compare_and_swap(None, original.clone()).unwrap();
        let trip = host
            .reserve(
                "legacy-trip",
                s::Resources {
                    output_bytes: 1,
                    ..s::Resources::default()
                },
                s::BudgetClass::Ordinary,
            )
            .unwrap();
        if provision == "exhausted" {
            exhaust_control_retention(&mut host, s::BudgetClass::Closeout);
        }
        let mut app = Runtime::open(&root).unwrap();
        assert!(!closeout_snapshot(&root)
            .1
            .iter()
            .any(|cell| cell.0 == "runtime_control_plan"));
        let before = app.ledger().unwrap();
        if provision == "available" {
            app.provision_closeout("operator").unwrap();
            assert!(
                app.ledger().unwrap().closeout.consumed.retained_bytes
                    > before.closeout.consumed.retained_bytes
            );
        } else if provision == "exhausted" {
            assert!(matches!(
                app.provision_closeout("operator"),
                Err(RuntimeError::Store(s::StoreError::Quota { .. }))
            ));
            assert_only_navigation_charged(before, app.ledger().unwrap());
            assert!(!closeout_snapshot(&root)
                .1
                .iter()
                .any(|cell| cell.0 == "runtime_control_plan"));
        }
        assert_eq!(closeout_head(&root), original);
        assert!(matches!(
            host.settle(
                &trip.id,
                s::Usage::Measured(s::Resources {
                    output_bytes: 2,
                    ..s::Resources::default()
                })
            ),
            Err(s::StoreError::Overrun(_))
        ));
        let before = closeout_snapshot(&root);
        assert!(matches!(
            app.provision_closeout("operator"),
            Err(RuntimeError::Store(s::StoreError::Frozen))
        ));
        assert!(matches!(
            app.resume("operator", "a", "work"),
            Err(RuntimeError::Store(s::StoreError::Frozen))
        ));
        assert_eq!(closeout_snapshot(&root), before);
        drop(app);
        let mut closeout = Runtime::open_for_closeout(&root).unwrap();
        let before = closeout_snapshot(&root);
        let result = closeout.pause("operator", "work", "Legacy pause.");
        if provision == "available" {
            result.unwrap();
            closeout
                .closeout("operator", "work", false, "Legacy unfinished ending.")
                .unwrap();
            assert_eq!(
                closeout_head(&root).body["work_meta"],
                original.body["work_meta"]
            );
        } else {
            assert!(matches!(
                result,
                Err(RuntimeError::Store(s::StoreError::Unsupported(_)))
            ));
            assert_closeout_write_rollback(before, closeout_snapshot(&root));
            assert_eq!(closeout_head(&root), original);
        }
    }
}

// Inspect accounting without adding navigation charges to the assertions.
fn adoption_accounting_record<T: serde::de::DeserializeOwned>(
    root: &std::path::Path,
    namespace: &str,
    key: &str,
) -> T {
    let (_, cells) = closeout_snapshot(root);
    let cell = cells
        .into_iter()
        .find(|cell| cell.0 == namespace && cell.1 == key)
        .unwrap();
    serde_json::from_slice(&cell.2).unwrap()
}

#[test]
fn runtime_adoption_usage_write_failure_rolls_back_the_complete_adoption() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("atomic-adoption-usage");
    let id = prepared.request.id.as_str();
    let root = f.app.state_root().to_owned();
    let mut d = decision(&prepared, true);
    d.operations = vec![
        write(
            "save-accounting-asset",
            p::Scope::Assets {},
            "accounting_asset",
            "Exact synthetic accounting regression bytes.",
            None,
        ),
        p::Operation::Communicate {
            id: "accounting-message".into(),
            recipient: "b".into(),
            subject: "Synthetic local fixture".into(),
            text: "Deposit only with the complete adoption.".into(),
        },
    ];
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Retain the exact synthetic fixture asset.".into(),
        operation_ids: vec!["save-accounting-asset".into()],
    };
    let bytes = serde_json::to_vec(&d).unwrap();
    f.app.dispatch_synthetic(id, NOW).unwrap();
    let head = closeout_head(&root);
    let dispatched: s::InvocationRecord = adoption_accounting_record(&root, "invocation", id);
    let key = format!("v:{id}");
    let reserved: s::ReservationRecord = adoption_accounting_record(&root, "reservation", &key);
    let ledger = f.app.ledger().unwrap();
    let fault = Connection::open(root.join("state.sqlite3")).unwrap();
    // Reservation flush follows decision, invocation and object writes. Prove
    // those writes happened in the same transaction before aborting settlement.
    fault
        .execute_batch(
            "CREATE TRIGGER runtime_adoption_usage_fault BEFORE UPDATE ON store_cells
         WHEN NEW.ns='reservation' AND NEW.k='v:atomic-adoption-usage'
              AND json_extract(NEW.data,'$.state')='Uncertain'
         BEGIN
             SELECT CASE WHEN NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='decision' AND k='atomic-adoption-usage'
             ) OR NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='invocation' AND k='atomic-adoption-usage'
                   AND json_extract(data,'$.state')='Adopted'
             ) OR NOT EXISTS (
                 SELECT 1 FROM store_cells WHERE ns='object' AND k='runtime-state-v1'
                   AND json_type(data,'$.body.sources.accounting_asset')='object'
                   AND json_type(data,'$.body.personas.a.active_invocation')='null'
             ) THEN RAISE(ABORT,'adoption writes missing before usage update') END;
             SELECT RAISE(ABORT,'injected runtime adoption usage failure');
         END;",
        )
        .unwrap();
    let error = f.app.adopt_response(id, &bytes, NOW).unwrap_err();
    assert!(matches!(
        &error,
        RuntimeError::Store(s::StoreError::Sqlite(_))
    ));
    assert!(error
        .to_string()
        .contains("injected runtime adoption usage failure"));
    assert_eq!(closeout_head(&root), head);
    assert_eq!(f.app.epoch().unwrap(), prepared.epoch);
    assert_only_navigation_charged(ledger.clone(), f.app.ledger().unwrap());
    assert_eq!(
        adoption_accounting_record::<s::ReservationRecord>(&root, "reservation", &key),
        reserved
    );
    let responded: s::InvocationRecord = adoption_accounting_record(&root, "invocation", id);
    assert_eq!(responded.state, s::InvocationState::Responded);
    assert_eq!(responded.response_hash, Some(s::sha256_bytes(&bytes)));
    assert_eq!(responded.history.len(), dispatched.history.len() + 1);
    assert_eq!(
        &responded.history[..dispatched.history.len()],
        &dispatched.history
    );
    let terminal = f.app.terminal_evidence(id).unwrap();
    assert_eq!(
        responded.history.last().unwrap().status,
        s::ReceiptStatus::Responded
    );
    assert_eq!(
        responded.history.last().unwrap().evidence,
        Some(terminal.exact_ref().unwrap())
    );
    let archive = format!("adoption:{}", s::sha256_bytes(id.as_bytes()));
    assert!(!closeout_snapshot(&root).1.iter().any(|cell| {
        (cell.0 == "decision" && cell.1 == id) || (cell.0 == "object" && cell.1 == archive)
    }));
    // Completion is already durable. Another failed adoption must preserve every
    // cell and counter, including receipt sequence and immutable revision entries.
    let before = closeout_snapshot(&root);
    let error = f.app.adopt_response(id, &bytes, NOW).unwrap_err();
    assert!(error
        .to_string()
        .contains("injected runtime adoption usage failure"));
    assert_resume_write_rollback(before, closeout_snapshot(&root));
    fault
        .execute_batch("DROP TRIGGER runtime_adoption_usage_fault")
        .unwrap();
    drop(fault);
    let before = closeout_snapshot(&root);
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    assert_resume_write_rollback(before, closeout_snapshot(&root));
    let report = f.app.adopt_response(id, &bytes, NOW).unwrap();
    assert_eq!(report.status, AdoptionStatus::Adopted);
    assert_eq!(report.accepted_epoch, Some(prepared.epoch + 1));
    assert!(report.synthetic && report.usage_is_unknown);
    let usage: s::ReservationRecord = adoption_accounting_record(&root, "reservation", &key);
    assert_eq!(usage.state, s::ReservationState::Uncertain);
    assert_eq!(usage.measured, None);
    assert_eq!(usage.history.len(), reserved.history.len() + 1);
    assert_eq!(&usage.history[..reserved.history.len()], &reserved.history);
    assert_eq!(
        usage.history.last().unwrap().status,
        s::ReceiptStatus::Uncertain
    );
    assert_eq!(usage.history.last().unwrap().epoch, prepared.epoch + 1);
    let after = f.app.ledger().unwrap();
    assert_eq!(
        after.ordinary.consumed.attempts,
        ledger.ordinary.consumed.attempts
    );
    assert_eq!(after.ordinary.reserved, s::Resources::default());
    assert_eq!(
        after.ordinary.uncertain,
        ledger
            .ordinary
            .uncertain
            .checked_add(s::Resources {
                attempts: 0,
                ..reserved.amount
            })
            .unwrap()
    );
    assert_eq!(
        after
            .ordinary
            .reserved
            .checked_add(after.ordinary.uncertain)
            .unwrap(),
        ledger
            .ordinary
            .reserved
            .checked_add(ledger.ordinary.uncertain)
            .unwrap()
    );
    let state = persisted_state(&f.app);
    assert_eq!(state["messages"].as_object().unwrap().len(), 1);
    assert_eq!(
        state["sources"]["accounting_asset"]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.app.terminal_evidence(id).unwrap(), terminal);
    let before = closeout_snapshot(&root);
    assert_eq!(
        serde_json::to_value(f.app.adopt_response(id, &bytes, NOW).unwrap()).unwrap(),
        serde_json::to_value(&report).unwrap()
    );
    assert_resume_write_rollback(before, closeout_snapshot(&root));
}

// Reproduce the historical split through the unchanged generic Store API.
// Copy the adoption unit from an identical, fully validated synthetic fixture;
// never rewind a reservation, erase a receipt, or alter controlling counters.
fn legacy_adoption_accounting_fixture() -> (Fixture, Prepared, Vec<u8>, AdoptionReport) {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("legacy-adoption-usage");
    let id = prepared.request.id.as_str();
    let root = f.app.state_root().to_owned();
    let mut donor = Fixture::new(true);
    donor.accept();
    let same = donor.prepare(id);
    assert_eq!(
        serde_json::to_value(&same).unwrap(),
        serde_json::to_value(&prepared).unwrap()
    );
    assert_eq!(closeout_head(&root), closeout_head(donor.app.state_root()));
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Communicate {
        id: "legacy-accounting-message".into(),
        recipient: "b".into(),
        subject: "Synthetic historical fixture".into(),
        text: "Exact replay must not deposit this again.".into(),
    });
    let bytes = format!(" \n{}\n ", serde_json::to_string(&d).unwrap()).into_bytes();
    let report = donor.app.submit_synthetic(id, &bytes, NOW).unwrap();
    assert_eq!(report.status, AdoptionStatus::Adopted);
    let terminal = donor.app.terminal_evidence(id).unwrap();
    let archive_id = format!("adoption:{}", s::sha256_bytes(id.as_bytes()));
    let archive: s::StoredObject =
        adoption_accounting_record(donor.app.state_root(), "object", &archive_id);
    let old = closeout_head(&root);
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    f.app.dispatch_synthetic(id, NOW).unwrap();
    let invocation = f.app.invocation(id).unwrap();
    let hash = s::sha256_bytes(&bytes);
    host.trusted_host()
        .complete_invocation_transport(
            id,
            &invocation.prepare_hash,
            &hash,
            terminal.clone(),
            |bound, object| {
                assert_eq!(bound.request, invocation.request);
                assert_eq!(object, &terminal);
                Ok(())
            },
        )
        .unwrap();
    let decision = s::Decision {
        expected_epoch: prepared.epoch,
        invocation_id: id.into(),
        response_hash: hash,
        changes: vec![
            s::Change {
                expected_revision: Some(old.revision),
                object: closeout_head(donor.app.state_root()),
                depends_on: Vec::new(),
            },
            s::Change {
                expected_revision: None,
                object: archive,
                depends_on: Vec::new(),
            },
        ],
        intents: Vec::new(),
    };
    let accepted = host.commit_decision(decision.clone()).unwrap();
    assert_eq!(Some(accepted.accepted_epoch), report.accepted_epoch);
    let before = closeout_snapshot(&root);
    assert_eq!(host.commit_decision(decision).unwrap(), accepted);
    assert_eq!(closeout_snapshot(&root), before);
    let usage: s::ReservationRecord =
        adoption_accounting_record(&root, "reservation", &format!("v:{id}"));
    assert_eq!(usage.state, s::ReservationState::Reserved);
    assert_eq!(usage.measured, None);
    assert_eq!(usage.history.len(), 1);
    drop(host);
    (f, prepared, bytes, report)
}

#[test]
fn legacy_adopted_replay_finishes_unknown_usage_and_preserves_later_measurements() {
    for overrun in [false, true] {
        let (mut f, prepared, bytes, report) = legacy_adoption_accounting_fixture();
        let id = prepared.request.id.as_str();
        let root = f.app.state_root().to_owned();
        let key = format!("v:{id}");
        let reserved: s::ReservationRecord = adoption_accounting_record(&root, "reservation", &key);
        let before = closeout_snapshot(&root);
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        assert_eq!(
            adoption_accounting_record::<s::InvocationRecord>(&root, "invocation", id).state,
            s::InvocationState::Adopted
        );
        assert_eq!(
            adoption_accounting_record::<s::ReservationRecord>(&root, "reservation", &key),
            reserved
        );
        // Equivalent parsed JSON with different exact bytes cannot repair usage.
        let mut different = bytes.clone();
        different.push(b' ');
        let before = closeout_snapshot(&root);
        assert!(matches!(
            f.app.adopt_response(id, &different, NOW),
            Err(RuntimeError::Store(s::StoreError::Conflict(_)))
        ));
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        let fault = Connection::open(root.join("state.sqlite3")).unwrap();
        fault
            .execute_batch(
                "CREATE TRIGGER legacy_adoption_usage_fault BEFORE UPDATE ON store_cells
         WHEN NEW.ns='reservation' AND NEW.k='v:legacy-adoption-usage'
              AND json_extract(NEW.data,'$.state')='Uncertain'
         BEGIN SELECT RAISE(ABORT,'injected legacy usage failure'); END;",
            )
            .unwrap();
        let before = closeout_snapshot(&root);
        let error = f.app.adopt_response(id, &bytes, NOW).unwrap_err();
        assert!(error.to_string().contains("injected legacy usage failure"));
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        fault
            .execute_batch("DROP TRIGGER legacy_adoption_usage_fault")
            .unwrap();
        drop(fault);
        let before = closeout_snapshot(&root);
        let ledger = f.app.ledger().unwrap();
        assert_eq!(
            serde_json::to_value(f.app.adopt_response(id, &bytes, NOW).unwrap()).unwrap(),
            serde_json::to_value(&report).unwrap()
        );
        let usage: s::ReservationRecord = adoption_accounting_record(&root, "reservation", &key);
        let mut expected_usage = reserved.clone();
        expected_usage.state = s::ReservationState::Uncertain;
        expected_usage.history.push(s::Receipt {
            sequence: before.0["sequence"].as_u64().unwrap() + 1,
            epoch: prepared.epoch + 1,
            subject: s::EntityId::Reservation(reserved.id.clone()),
            status: s::ReceiptStatus::Uncertain,
            detail: None,
            evidence: None,
        });
        assert_eq!(usage, expected_usage);
        let after_ledger = f.app.ledger().unwrap();
        assert_eq!(
            after_ledger
                .ordinary
                .reserved
                .checked_add(after_ledger.ordinary.uncertain)
                .unwrap(),
            ledger
                .ordinary
                .reserved
                .checked_add(ledger.ordinary.uncertain)
                .unwrap()
        );
        let mut expected_ledger = ledger;
        expected_ledger.ordinary.reserved = s::Resources::default();
        expected_ledger.ordinary.uncertain = expected_ledger
            .ordinary
            .uncertain
            .checked_add(s::Resources {
                attempts: 0,
                ..reserved.amount
            })
            .unwrap();
        assert_only_navigation_charged(expected_ledger.clone(), after_ledger);
        // Every historical cell is byte-identical except the reservation's one
        // appended uncertainty receipt. Its prepaid allocation remains unchanged.
        let after = closeout_snapshot(&root);
        let mut expected = before;
        expected.0["sequence"] = json!(usage.history.last().unwrap().sequence);
        expected.0["ordinary"]["reserved"] =
            serde_json::to_value(expected_ledger.ordinary.reserved).unwrap();
        expected.0["ordinary"]["uncertain"] =
            serde_json::to_value(expected_ledger.ordinary.uncertain).unwrap();
        expected
            .1
            .iter_mut()
            .find(|cell| cell.0 == "reservation" && cell.1 == key)
            .unwrap()
            .2 = after
            .1
            .iter()
            .find(|cell| cell.0 == "reservation" && cell.1 == key)
            .unwrap()
            .2
            .clone();
        assert_resume_write_rollback(expected, after);
        let before = closeout_snapshot(&root);
        assert_eq!(
            serde_json::to_value(f.app.adopt_response(id, &bytes, NOW).unwrap()).unwrap(),
            serde_json::to_value(&report).unwrap()
        );
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        // Later state changes cannot rebase the historical report. Measurements
        // below are explicit synthetic host fixtures, not actual provider usage.
        f.app
            .pause("operator", "work", "Later local accounting fixture pause.")
            .unwrap();
        let measured = s::Resources {
            attempts: reserved.amount.attempts,
            input_bytes: reserved.amount.input_bytes / 2,
            output_bytes: if overrun {
                reserved.amount.output_bytes + 1
            } else {
                reserved.amount.output_bytes / 2
            },
            navigation_bytes: reserved.amount.navigation_bytes / 2,
            retained_bytes: 0,
        };
        let mut host =
            s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
        let result = host.settle(&reserved.id, s::Usage::Measured(measured));
        if overrun {
            assert!(matches!(result, Err(s::StoreError::Overrun(_))));
        } else {
            result.unwrap();
        }
        drop(host);
        let settled: s::ReservationRecord = adoption_accounting_record(&root, "reservation", &key);
        assert_eq!(settled.measured, Some(measured));
        assert_eq!(
            settled.state,
            if overrun {
                s::ReservationState::Overrun
            } else {
                s::ReservationState::Settled
            }
        );
        assert_eq!(settled.history.len(), usage.history.len() + 1);
        assert_eq!(&settled.history[..usage.history.len()], &usage.history);
        for reopen in [false, true] {
            let before = closeout_snapshot(&root);
            if reopen {
                drop(f.app);
                f.app = Runtime::open(&root).unwrap();
            }
            assert_eq!(
                serde_json::to_value(f.app.adopt_response(id, &bytes, NOW).unwrap()).unwrap(),
                serde_json::to_value(&report).unwrap()
            );
            assert_resume_write_rollback(before, closeout_snapshot(&root));
            assert_eq!(
                adoption_accounting_record::<s::ReservationRecord>(&root, "reservation", &key),
                settled
            );
            assert_eq!(f.app.ledger().unwrap().frozen, overrun);
        }
    }
}

#[test]
fn no_change_and_closed_are_explicit_judgments_not_completion() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("closed-judgment");
    let mut d = decision(&prepared, true);
    d.work = p::WorkDisposition::Closed {
        reason: "My judgment is that the response is enough.".into(),
        evidence: vec![],
    };
    d.continuation = p::Continuation::Pause {
        reason: "Await explicit next authorization.".into(),
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let state = persisted_state(&f.app);
    assert_eq!(
        state["personas"]["a"]["focus"]["work"]["last_judgment"]["kind"],
        "closed"
    );
    let snapshot = f.app.snapshot().unwrap();
    assert!(matches!(
        snapshot
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .phase,
        w::Phase::Open
    ));
    assert!(!snapshot
        .board
        .completion_supported(&w::WorkId("work".into())));
}

#[test]
fn raw_fixture_ingress_cannot_fill_prepared_or_live_transport_slots() {
    for profile in [Profile::Synthetic, Profile::Direct] {
        let mut configuration = config();
        configuration.allow_direct_provider = true;
        let mut primary = seed("a");
        primary.model_confirmed = true;
        let mut f = Fixture::configured(true, configuration, primary, need());
        f.accept();
        let prepared = f
            .app
            .prepare(
                PrepareRequest {
                    id: "ingress".into(),
                    persona: "a".into(),
                    work: "work".into(),
                    profile,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap();
        let mut host =
            s::Store::open(&f.app.state_root().join("state.sqlite3"), storage_limits()).unwrap();
        let bytes = serde_json::to_vec(&decision(&prepared, true)).unwrap();
        let state = persisted_state(&f.app);
        let before = f.app.ledger().unwrap();
        assert!(f.app.adopt_response("ingress", &bytes, NOW).is_err());
        assert_eq!(persisted_state(&f.app), state);
        assert_eq!(f.app.epoch().unwrap(), prepared.epoch);
        assert_only_navigation_charged(before, f.app.ledger().unwrap());
        let inv = f.app.invocation("ingress").unwrap();
        assert_eq!(inv.state, s::InvocationState::Prepared);
        assert_eq!(inv.response_hash, None);
        let slot = format!("terminal:{}", s::sha256_bytes(b"ingress"));
        assert_eq!(
            host.trusted_host()
                .read_terminal_evidence(&s::EntityId::Invocation("ingress".into()), &slot,)
                .unwrap(),
            None
        );
        if profile == Profile::Synthetic {
            f.app.dispatch_synthetic("ingress", NOW).unwrap();
            assert_eq!(
                f.app.adopt_response("ingress", &bytes, NOW).unwrap().status,
                AdoptionStatus::Adopted
            );
            let terminal = f.app.terminal_evidence("ingress").unwrap();
            let inv = f.app.invocation("ingress").unwrap();
            assert_eq!(
                inv.history
                    .iter()
                    .find(|r| r.status == s::ReceiptStatus::Responded)
                    .unwrap()
                    .evidence,
                Some(terminal.exact_ref().unwrap())
            );
        } else {
            // Store dispatch is a fixture of the lifecycle boundary, not a provider call.
            host.dispatch_invocation("ingress").unwrap();
            let before = f.app.ledger().unwrap();
            let error = f.app.adopt_response("ingress", &bytes, NOW).unwrap_err();
            assert!(error
                .to_string()
                .contains("cannot supply provider evidence"));
            assert_only_navigation_charged(before, f.app.ledger().unwrap());
            let inv = f.app.invocation("ingress").unwrap();
            assert_eq!(inv.state, s::InvocationState::Dispatched);
            assert_eq!(inv.response_hash, None);
            assert_eq!(
                host.trusted_host()
                    .read_terminal_evidence(&s::EntityId::Invocation("ingress".into()), &slot,)
                    .unwrap(),
                None
            );
            assert_eq!(persisted_state(&f.app), state);
        }
    }
}

#[test]
fn malformed_late_fields_have_zero_store_reads_mutations_or_messages() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("invalid");
    f.app.dispatch_synthetic("invalid", NOW).unwrap();
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Communicate {
        id: "message".into(),
        recipient: "b".into(),
        subject: "Unvalidated".into(),
        text: "Must not be deposited.".into(),
    });
    let before = persisted_state(&f.app);
    let ledger = f.app.ledger().unwrap();
    let mut value = serde_json::to_value(&d).unwrap();
    value.as_object_mut().unwrap().remove("next_assets");
    assert!(matches!(
        f.app
            .adopt_response("invalid", &serde_json::to_vec(&value).unwrap(), NOW),
        Err(RuntimeError::Protocol(_))
    ));
    let mut value = serde_json::to_value(&d).unwrap();
    value["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"unknown_late_operation","id":"later"}));
    assert!(matches!(
        f.app
            .adopt_response("invalid", &serde_json::to_vec(&value).unwrap(), NOW),
        Err(RuntimeError::Protocol(_))
    ));
    assert_eq!(before, persisted_state(&f.app));
    assert_eq!(ledger, f.app.ledger().unwrap());
}

#[test]
fn seed_candidate_adoption_fences_earlier_read_and_requires_fresh_character() {
    let mut f = Fixture::new(false);
    let outside = record("outside", f::Scope::Assets, "This read must be fenced.");
    f.app
        .import_files("operator", "a", vec![outside.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("initialize");
    assert!(prepared.prompt.contains("operator-supplied candidate seed"));
    let mut d = decision(&prepared, false);
    let (ocean, vad) = profiles();
    d.operations = vec![
        p::Operation::Read {
            id: "early-read".into(),
            reference: reference(&outside),
        },
        write(
            "voice-write",
            p::Scope::Fragments {},
            "generated-voice",
            "I preserve the distinction between a proposal and a receipt.",
            None,
        ),
        p::Operation::CharacterAdopt {
            id: "character".into(),
            base_revision: None,
            ocean,
            vad,
            own_voice: vec![proposed("generated-voice", "voice-write")],
            reason: "Propose an initial coherent account.".into(),
            evidence: vec![],
        },
    ];
    d.fragment_maintenance = p::Maintenance::Change {
        reason: "Initialize my account.".into(),
        operation_ids: vec!["voice-write".into(), "character".into()],
    };
    let report = f.run(&prepared, &d);
    assert_eq!(report.status, AdoptionStatus::CharacterOnly);
    assert_eq!(report.fenced, vec!["early-read"]);
    let state = persisted_state(&f.app);
    assert_eq!(state["observations"].as_object().unwrap().len(), 0);
    assert_eq!(
        state["sources"]["generated-voice"]["revisions"][0]["record"]["author"],
        "a"
    );
    assert!(f
        .app
        .prepare(
            PrepareRequest {
                id: "premature".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary
            },
            NOW
        )
        .is_err());
    f.app.resume("operator", "a", "work").unwrap();
    let next = f.prepare("fresh-character");
    assert!(next
        .prompt
        .contains("I preserve the distinction between a proposal and a receipt."));
    assert!(next.character_revision.is_some());
}

#[test]
fn accepted_fragment_and_asset_supply_exact_bytes_without_distractor_inventory() {
    let mut f = Fixture::new(true);
    f.accept();
    let distractors = (0..300)
        .map(|n| {
            record(
                &format!("distractor-{n:03}"),
                f::Scope::Assets,
                "irrelevant retained fixture bytes",
            )
        })
        .collect();
    f.app
        .import_files("operator", "a", distractors, true, NOW)
        .unwrap();
    let prepared = f.prepare("save-and-select");
    assert!(!prepared.prompt.contains("distractor-"));
    let fragment = "I retain literal \\ paths and an exact newline.\nSecond line.";
    let asset = "Exact supporting bytes: λ, \\n, and punctuation.";
    let mut d = decision(&prepared, true);
    d.operations = vec![
        write(
            "f",
            p::Scope::Fragments {},
            "chosen-fragment",
            fragment,
            None,
        ),
        write("a", p::Scope::Assets {}, "chosen-asset", asset, None),
    ];
    d.fragment_maintenance = p::Maintenance::Change {
        reason: "Retain a characteristic prompt part.".into(),
        operation_ids: vec!["f".into()],
    };
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Retain separate supporting material.".into(),
        operation_ids: vec!["a".into()],
    };
    d.next_fragments = p::SelectionIntent::Replace {
        reason: "Use the exact saved fragment.".into(),
        references: vec![proposed("chosen-fragment", "f")],
    };
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Use the exact saved asset.".into(),
        references: vec![proposed("chosen-asset", "a")],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let persisted = persisted_state(&f.app);
    let bytes: Vec<u8> = serde_json::from_value(
        persisted["sources"]["chosen-asset"]["revisions"][0]["record"]["bytes"].clone(),
    )
    .unwrap();
    assert_eq!(bytes, asset.as_bytes());
    let next = f.prepare("exact-next");
    let context: Value =
        serde_json::from_str(next.prompt.split("QUALIFIED_CONTEXT\n").nth(1).unwrap()).unwrap();
    let texts: Vec<&str> = context["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["text"].as_str())
        .collect();
    assert!(texts.contains(&fragment));
    assert!(texts.contains(&asset));
    assert!(!next.prompt.contains("distractor-"));
}

#[test]
fn read_creates_observation_but_never_selects() {
    let mut f = Fixture::new(true);
    f.accept();
    let asset = record(
        "read-only",
        f::Scope::Assets,
        "Observed exact body without selection.",
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("read");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Read {
        id: "r".into(),
        reference: reference(&asset),
    });
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let persisted = persisted_state(&f.app);
    assert_eq!(
        persisted["personas"]["a"]["focus"]["work"]["assets"],
        json!([])
    );
    let next = f.prepare("read-observation");
    assert!(next
        .prompt
        .contains("Observed exact body without selection."));
    let context: Value =
        serde_json::from_str(next.prompt.split("QUALIFIED_CONTEXT\n").nth(1).unwrap()).unwrap();
    assert!(!context["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["reference"]["source"] == "read-only"));
}

#[test]
fn denied_capability_holds_entire_valid_response_without_effect_or_read() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("denied");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Search {
        id: "search-first".into(),
        scope: p::Scope::Assets {},
        query: "anything".into(),
        regex: false,
        limit: 1,
    });
    d.operations.push(p::Operation::Effect {
        id: "effect-later".into(),
        capability_id: "unimplemented.remote".into(),
        binding_revision: "binding".into(),
        inputs: json!({}),
        depends_on: vec![],
    });
    let before = persisted_state(&f.app);
    let report = f.run(&prepared, &d);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.unwrap().contains("capability"));
    assert_eq!(before, persisted_state(&f.app));
    assert_eq!(
        f.app.invocation("denied").unwrap().state,
        s::InvocationState::Failed
    );
}

#[test]
fn stale_save_holds_communication_and_selection_atomically() {
    let mut f = Fixture::new(true);
    f.accept();
    let original = record("editable", f::Scope::Assets, "original bytes");
    f.app
        .import_files("operator", "a", vec![original], false, NOW)
        .unwrap();
    let prepared = f.prepare("stale-save");
    let mut d = decision(&prepared, true);
    d.operations.push(write(
        "write",
        p::Scope::Assets {},
        "editable",
        "must not persist",
        Some("wrong-base".into()),
    ));
    d.operations.push(p::Operation::Communicate {
        id: "dependent-message".into(),
        recipient: "b".into(),
        subject: "Saved".into(),
        text: "Must not be sent.".into(),
    });
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Propose an edit.".into(),
        operation_ids: vec!["write".into()],
    };
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Select proposed bytes.".into(),
        references: vec![proposed("editable", "write")],
    };
    let before = persisted_state(&f.app);
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Held);
    assert_eq!(before, persisted_state(&f.app));
}

#[test]
fn global_epoch_and_invocation_identity_cannot_be_rebound() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("unique");
    let conflict = f.app.prepare(
        PrepareRequest {
            id: "unique".into(),
            persona: "b".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    assert!(conflict.is_err());
    f.app.dispatch_synthetic("unique", NOW).unwrap();
    f.app
        .pause("operator", "work", "An actual operator pause.")
        .unwrap();
    let report = f
        .app
        .adopt_response(
            "unique",
            &serde_json::to_vec(&decision(&prepared, true)).unwrap(),
            NOW,
        )
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.unwrap().contains("epoch"));
}

#[test]
fn withdrawal_withholds_entire_optional_bundle_and_blocks_exposed_old_response() {
    let mut f = Fixture::new(true);
    f.accept();
    let qualification = record(
        "qualification",
        f::Scope::Assets,
        "A mandatory exact qualification.",
    );
    let mut method = record(
        "method",
        f::Scope::Fragments,
        "Qualified optional guidance.",
    );
    method
        .qualifications
        .push(qualification.reference(f::Representation::Utf8));
    method = method.seal().unwrap();
    f.app
        .import_files(
            "operator",
            "a",
            vec![qualification.clone(), method.clone()],
            true,
            NOW,
        )
        .unwrap();
    let prepared = f.prepare("select-qualified");
    let mut d = decision(&prepared, true);
    d.next_fragments = p::SelectionIntent::Replace {
        reason: "Select a complete qualified bundle.".into(),
        references: vec![reference(&method)],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let exposed = f.prepare("exposed-before-withdrawal");
    f.app.dispatch_synthetic(&exposed.request.id, NOW).unwrap();
    f.app
        .withdraw_source("operator", "qualification", "Withdraw source access.")
        .unwrap();
    assert_eq!(
        f.app
            .adopt_response(
                &exposed.request.id,
                &serde_json::to_vec(&decision(&exposed, true)).unwrap(),
                NOW
            )
            .unwrap()
            .status,
        AdoptionStatus::Held
    );
    f.app.resume("operator", "a", "work").unwrap();
    let fresh = f.prepare("after-withdrawal");
    assert!(!fresh.prompt.contains("Qualified optional guidance."));
    assert!(!fresh.prompt.contains("A mandatory exact qualification."));
    let context: Value =
        serde_json::from_str(fresh.prompt.split("QUALIFIED_CONTEXT\n").nth(1).unwrap()).unwrap();
    assert!(!context["withheld"].as_array().unwrap().is_empty());
    assert_eq!(
        persisted_state(&f.app)["personas"]["a"]["focus"]["work"]["fragments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

// Admission inspection itself performs metered storage reads. A refusal must
// leave attempts, reservations, identities and retained state unchanged while
// honestly retaining that navigation charge.
fn assert_only_navigation_charged(mut before: s::LedgerReport, after: s::LedgerReport) {
    assert!(after.ordinary.consumed.navigation_bytes >= before.ordinary.consumed.navigation_bytes);
    before.ordinary.consumed.navigation_bytes = after.ordinary.consumed.navigation_bytes;
    // Immutable terminal lookup on exact recovery/replay uses the finite
    // protected navigation account. All other counters still must be equal.
    assert!(after.closeout.consumed.navigation_bytes >= before.closeout.consumed.navigation_bytes);
    before.closeout.consumed.navigation_bytes = after.closeout.consumed.navigation_bytes;
    assert_eq!(before, after);
}

#[test]
fn oversized_selection_is_held_and_repair_cannot_mint_a_reservation() {
    let mut f = Fixture::new(true);
    f.accept();
    let large = record("large", f::Scope::Assets, &"x".repeat(180_000));
    f.app
        .import_files("operator", "a", vec![large.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("overflow");
    let mut d = decision(&prepared, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Request the complete optional body.".into(),
        references: vec![reference(&large)],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Held);
    assert_eq!(
        persisted_state(&f.app)["personas"]["a"]["focus"]["work"]["assets"],
        json!([])
    );
    let before = f.app.ledger().unwrap();
    assert!(matches!(
        f.app.prepare(
            PrepareRequest {
                id: "repair".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::SelectionRepair
            },
            NOW
        ),
        Err(RuntimeError::Held(_))
    ));
    assert_only_navigation_charged(before, f.app.ledger().unwrap());
}

#[test]
fn next_call_lifetime_is_not_renewed_by_preserve_and_unknown_values_are_refused() {
    let mut f = Fixture::new(true);
    f.accept();
    let asset = record(
        "one-call",
        f::Scope::Assets,
        "Only the next admitted request.",
    );
    f.app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("select-once");
    let mut d = decision(&prepared, true);
    let mut r = reference(&asset);
    r.lifetime = Some("next_call".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "One admitted request only.".into(),
        references: vec![r],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let once = f.prepare("once");
    assert!(once.prompt.contains("Only the next admitted request."));
    assert_eq!(
        f.run(&once, &decision(&once, true)).status,
        AdoptionStatus::Adopted
    );
    let after = f.prepare("after-once");
    assert!(!after.prompt.contains("Only the next admitted request."));
    let mut d = decision(&after, true);
    let mut r = reference(&asset);
    r.lifetime = Some("forever-ish".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Unsupported request.".into(),
        references: vec![r],
    };
    assert_eq!(f.run(&after, &d).status, AdoptionStatus::Held);
}

#[test]
fn paused_restart_never_spends_or_replays_and_usage_remains_unknown() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("pause");
    let mut d = decision(&prepared, true);
    d.continuation = p::Continuation::Pause {
        reason: "Require an explicit resume.".into(),
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let before = f.app.ledger().unwrap();
    assert!(before.ordinary.uncertain.output_bytes > 0);
    let root = f.app.state_root().to_path_buf();
    drop(f.app);
    let mut app = Runtime::open(&root).unwrap();
    assert_eq!(
        before.ordinary.consumed.attempts,
        app.ledger().unwrap().ordinary.consumed.attempts
    );
    assert!(app
        .prepare(
            PrepareRequest {
                id: "no-autospend".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary
            },
            NOW
        )
        .is_err());
    assert_eq!(
        before.ordinary.consumed.attempts,
        app.ledger().unwrap().ordinary.consumed.attempts
    );
    app.resume("operator", "a", "work").unwrap();
    assert_eq!(
        before.ordinary.consumed.attempts,
        app.ledger().unwrap().ordinary.consumed.attempts
    );
}

#[test]
fn restart_of_dispatched_transport_remains_uncertain_and_cannot_resume() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("interrupted");
    let bytes = serde_json::to_vec(&decision(&prepared, true)).unwrap();
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    let root = f.app.state_root().to_path_buf();
    drop(f.app);
    let mut inspection =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    assert_eq!(
        inspection.invocation("interrupted").unwrap().state,
        s::InvocationState::Dispatched
    );
    drop(inspection);
    let mut app = Runtime::open(&root).unwrap();
    let recovered = app.invocation("interrupted").unwrap();
    assert_eq!(recovered.state, s::InvocationState::Uncertain);
    let epoch = app.epoch().unwrap();
    let attempts = app.ledger().unwrap().ordinary.consumed.attempts;
    assert!(matches!(
        app.adopt_response("interrupted", &bytes, NOW),
        Err(RuntimeError::Store(s::StoreError::ReconciliationRequired))
    ));
    assert!(app.submit_synthetic("interrupted", &bytes, NOW).is_err());
    assert!(app.terminal_evidence("interrupted").is_err());
    assert!(app.resume("operator", "a", "work").is_err());
    assert_eq!(app.invocation("interrupted").unwrap(), recovered);
    assert_eq!(app.epoch().unwrap(), epoch);
    assert_eq!(app.ledger().unwrap().ordinary.consumed.attempts, attempts);
}

#[test]
fn positive_work_artifact_judgment_release_local_delivery_and_closure_are_distinct() {
    let mut f = Fixture::new(true);
    f.accept();
    let caveat = record(
        "delivery-caveat",
        f::Scope::Assets,
        "EXACT MANDATORY DELIVERY CAVEAT",
    );
    let mut source = record(
        "result-file",
        f::Scope::Assets,
        "Exact accepted artifact bytes.",
    );
    source
        .qualifications
        .push(caveat.reference(f::Representation::Utf8));
    source = source.seal().unwrap();
    f.app
        .import_files(
            "operator",
            "a",
            vec![caveat.clone(), source.clone()],
            false,
            NOW,
        )
        .unwrap();
    let imported_revision = f.app.snapshot().unwrap().sources["result-file"].revisions[0].clone();
    let prepared = f.prepare("artifact");
    let mut d = decision(&prepared, true);
    let mut rewrite = write(
        "revise",
        p::Scope::Assets {},
        "result-file",
        "Exact revised artifact bytes.",
        Some(source.revision.clone()),
    );
    if let p::Operation::FileWrite { qualifications, .. } = &mut rewrite {
        qualifications.push(reference(&source));
    }
    d.operations = vec![
        rewrite,
        effect(
            "register",
            WorkAction::PutArtifact {
                artifact: "result-artifact".into(),
                base: None,
                subject: proposed("result-file", "revise"),
            },
            &["revise"],
        ),
    ];
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Retain a revision with its exact imported predecessor.".into(),
        operation_ids: vec!["revise".into()],
    };
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Retain both exact versions and their distinct origins.".into(),
        references: vec![reference(&source), proposed("result-file", "revise")],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let root = f.app.state_root().to_owned();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    let snapshot = f.app.snapshot().unwrap();
    let managed = &snapshot.sources["result-file"];
    assert_eq!(managed.imported_by.as_deref(), Some("operator"));
    assert_eq!(managed.revisions.len(), 2);
    assert_eq!(
        serde_json::to_value(&managed.revisions[0]).unwrap(),
        serde_json::to_value(&imported_revision).unwrap()
    );
    let Some(RevisionOrigin::OperatorImport {
        operator,
        event_index,
    }) = &managed.revisions[0].origin
    else {
        panic!("exact import origin absent")
    };
    assert_eq!(operator, "operator");
    assert_eq!(
        snapshot.operator_events[*event_index]["kind"],
        "file_import"
    );
    assert_eq!(
        snapshot.operator_events[*event_index]["operator"],
        *operator
    );
    assert_eq!(
        managed.revisions[1].origin,
        Some(RevisionOrigin::PersonaInvocation {
            persona: "a".into(),
            invocation_id: "artifact".into(),
            operation_id: "revise".into(),
        })
    );
    let authored_revision = managed.revisions[1].record.revision.clone();
    assert_eq!(
        f.app.adopted("artifact").unwrap().report.status,
        AdoptionStatus::Adopted
    );
    let mut legacy = serde_json::to_value(&imported_revision).unwrap();
    legacy.as_object_mut().unwrap().remove("origin");
    assert!(serde_json::from_value::<ManagedRevision>(legacy)
        .unwrap()
        .origin
        .is_none());
    let work = snapshot.board.work(&w::WorkId("work".into())).unwrap();
    let artifact =
        &work.artifacts[&w::ArtifactId("result-artifact".into())].revisions[&1].reference;
    let key = ArtifactKey {
        id: artifact.id.0.clone(),
        revision: artifact.revision,
        digest: hex::encode(artifact.digest),
    };
    f.app
        .record_human_evidence(
            "operator",
            "work",
            "human-check",
            "I inspected this exact fixture artifact; this is human testimony.",
            vec![key.clone()],
            set(&["result"]),
        )
        .unwrap();
    let prepared = f.prepare("judge-release");
    let context: Value =
        serde_json::from_str(prepared.prompt.split("QUALIFIED_CONTEXT\n").nth(1).unwrap()).unwrap();
    let attribution = context["source_attribution"].as_array().unwrap();
    let imported = attribution
        .iter()
        .find(|entry| {
            entry["reference"]["source"] == source.source
                && entry["reference"]["revision"] == source.revision
        })
        .unwrap();
    let authored = attribution
        .iter()
        .find(|entry| {
            entry["reference"]["source"] == source.source
                && entry["reference"]["revision"] == authored_revision
        })
        .unwrap();
    assert_eq!(imported["imported_by"], "operator");
    assert_eq!(
        imported["author_status"],
        "operator-imported author assertion"
    );
    assert!(authored["imported_by"].is_null());
    assert_eq!(authored["author_status"], "accepted persona authorship");
    assert_eq!(authored["origin"]["invocation_id"], "artifact");
    assert_eq!(authored["recorded_author"], "a");
    let mut d = decision(&prepared, true);
    let artifacts = BTreeMap::from([("result".into(), key)]);
    d.operations = vec![
        effect(
            "judge",
            WorkAction::Judge {
                id: "judgment".into(),
                artifacts: artifacts.clone(),
                sufficient: true,
                claims: BTreeMap::from([(
                    "result".into(),
                    ClaimInput {
                        met: true,
                        explanation: "The cited testimony concerns these exact bytes.".into(),
                        evidence: set(&["human-check"]),
                    },
                )]),
                explanation: "An attributed individual sufficiency judgment.".into(),
            },
            &[],
        ),
        effect(
            "shared",
            WorkAction::DecideShared {
                id: "shared".into(),
                artifacts,
            },
            &[],
        ),
        effect(
            "release",
            WorkAction::Release {
                id: "release".into(),
                shared: "shared".into(),
            },
            &[],
        ),
    ];
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    assert!(!f
        .app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&w::WorkId("work".into())));
    let prepared = f.prepare("local-delivery");
    let mut d = decision(&prepared, true);
    d.operations.push(effect(
        "deliver",
        WorkAction::DeliverRelease {
            release: "release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let persisted = persisted_state(&f.app);
    let delivery = persisted["local_deliveries"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let bytes: Vec<u8> = serde_json::from_value(delivery["artifacts"][0]["bytes"].clone()).unwrap();
    assert_eq!(bytes, b"Exact revised artifact bytes.");
    assert_eq!(delivery["external_export"], false);
    let deposited = &delivery["artifacts"][0];
    let bundle_json = deposited["qualified_bundle_json"].as_str().unwrap();
    assert_eq!(
        deposited["qualified_bundle_sha256"],
        s::sha256_bytes(bundle_json.as_bytes())
    );
    let bundle: Value = serde_json::from_str(bundle_json).unwrap();
    let delivered_attribution = bundle["source_attribution"].as_array().unwrap();
    assert_eq!(
        delivered_attribution
            .iter()
            .find(|entry| entry["reference"]["source"] == source.source
                && entry["reference"]["revision"] == source.revision)
            .unwrap(),
        imported
    );
    assert_eq!(
        delivered_attribution
            .iter()
            .find(|entry| entry["reference"]["source"] == source.source
                && entry["reference"]["revision"] == authored_revision)
            .unwrap(),
        authored
    );
    for (revision, text) in [
        (source.revision.as_str(), "Exact accepted artifact bytes."),
        (authored_revision.as_str(), "Exact revised artifact bytes."),
    ] {
        assert!(bundle["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["reference"]["source"] == source.source
                && entry["reference"]["revision"] == revision
                && entry["text"] == text));
    }
    assert!(bundle["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["reference"]["source"] == "delivery-caveat"
            && e["text"] == "EXACT MANDATORY DELIVERY CAVEAT"));
    assert!(deposited["qualification_bindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["reference"]["source"] == "result-file"
            && b["file_qualifications"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["source"] == "delivery-caveat")));
    let exact_artifact: ArtifactKey =
        serde_json::from_value(deposited["reference"].clone()).unwrap();
    let exact_source: p::ExactRef = serde_json::from_value(deposited["source"].clone()).unwrap();
    let binding_bytes = serde_json::to_vec(&(
        exact_artifact,
        exact_source,
        deposited["qualified_bundle_sha256"].clone(),
        deposited["qualification_bindings"].clone(),
    ))
    .unwrap();
    assert_eq!(
        deposited["bundle_binding_sha256"],
        s::sha256_bytes(&binding_bytes)
    );
    let prepared = f.prepare("close");
    let mut d = decision(&prepared, true);
    d.operations.push(effect(
        "close",
        WorkAction::CloseGoal {
            release: "release".into(),
        },
        &[],
    ));
    d.continuation = p::Continuation::Stop {
        reason: "The explicit work closure command has all required evidence.".into(),
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    assert!(f
        .app
        .snapshot()
        .unwrap()
        .board
        .completion_supported(&w::WorkId("work".into())));
}

#[test]
fn strict_and_hard_physical_requirements_fail_closed() {
    let mut f = Fixture::new(true);
    let before = f.app.ledger().unwrap();
    assert!(matches!(
        f.app.prepare(
            PrepareRequest {
                id: "strict".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Strict,
                mode: Mode::Ordinary
            },
            NOW
        ),
        Err(RuntimeError::NotReady(_))
    ));
    assert_eq!(before, f.app.ledger().unwrap());
    let mut config = config();
    config.require_physical_hard_bound = true;
    let root = f._temp.path().join("forbidden-state");
    assert!(matches!(
        Runtime::create(&root, config, "operator"),
        Err(RuntimeError::NotReady(_))
    ));
    assert!(!root.exists());
}

#[test]
fn mutually_qualified_proposals_are_adopted_atomically_and_survive_reopen() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("cycle");
    let mut d = decision(&prepared, true);
    let mut left = write(
        "left-write",
        p::Scope::Fragments {},
        "left",
        "LEFT EXACT GUIDANCE",
        None,
    );
    let mut right = write(
        "right-write",
        p::Scope::Assets {},
        "right",
        "RIGHT EXACT CONDITION",
        None,
    );
    if let p::Operation::FileWrite { qualifications, .. } = &mut left {
        qualifications.push(proposed("right", "right-write"));
    }
    if let p::Operation::FileWrite { qualifications, .. } = &mut right {
        qualifications.push(proposed("left", "left-write"));
    }
    d.operations = vec![left, right];
    d.fragment_maintenance = p::Maintenance::Change {
        reason: "Adopt exact qualified guidance.".into(),
        operation_ids: vec!["left-write".into()],
    };
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Retain its explicit condition.".into(),
        operation_ids: vec!["right-write".into()],
    };
    d.next_fragments = p::SelectionIntent::Replace {
        reason: "Use the coherent qualified record.".into(),
        references: vec![proposed("left", "left-write")],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    assert_eq!(state.accepted_bindings.len(), 2);
    assert!(state.sources["left"].revisions[0]
        .record
        .qualifications
        .is_empty());
    let root = f.app.state_root().to_path_buf();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    let next = f.prepare("after-cycle");
    assert!(next.prompt.contains("LEFT EXACT GUIDANCE"));
    assert!(next.prompt.contains("RIGHT EXACT CONDITION"));
    assert!(!next.prompt.contains("proposal-op:left-write"));
}

fn control(prepared: &Prepared) -> Value {
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
fn restricted_policy() -> f::Policy {
    let mut p = policy();
    p.id = "restricted".into();
    p.revision = "restricted-1".into();
    p
}
fn restricted_fixture() -> (Fixture, f::Policy, f::FileRevision) {
    let p = restricted_policy();
    let mut c = config();
    c.policies.active_policies.push(p.clone());
    let mut f = Fixture::configured(true, c, seed("a"), need());
    f.accept();
    let mut asset = record(
        "restricted-source",
        f::Scope::Assets,
        "DISTINCTIVE RESTRICTED SOURCE",
    );
    asset.policy = p.clone();
    asset = asset.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![asset.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("select-restricted");
    let mut d = decision(&prepared, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Use this exact permitted source.".into(),
        references: vec![reference(&asset)],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    (f, p, asset)
}

#[test]
fn private_work_and_human_evidence_cannot_be_written_under_a_broader_policy() {
    let mut private = restricted_policy();
    private.readers = set(&["a"]);
    let mut c = config();
    c.policies.active_policies.push(private.clone());
    let mut primary = seed("a");
    primary.grants.write_policies.insert(private.id.clone());
    let mut original = need();
    original.policy_id = private.id.clone();
    original.request = "PRIVATE ORIGINAL REQUEST".into();
    let mut f = Fixture::configured(true, c, primary, original);
    f.accept();
    f.app
        .record_human_evidence(
            "operator",
            "work",
            "testimony",
            "PRIVATE HUMAN TESTIMONY",
            vec![],
            set(&["result"]),
        )
        .unwrap();
    let prepared = f.prepare("copy-private");
    assert!(prepared.prompt.contains("PRIVATE ORIGINAL REQUEST"));
    assert!(prepared.prompt.contains("PRIVATE HUMAN TESTIMONY"));
    let mut d = decision(&prepared, true);
    d.operations.push(write(
        "copy",
        p::Scope::Assets {},
        "private-copy",
        "PRIVATE ORIGINAL REQUEST / PRIVATE HUMAN TESTIMONY",
        None,
    ));
    d.asset_maintenance = p::Maintenance::Change {
        reason: "Retain the supplied material.".into(),
        operation_ids: vec!["copy".into()],
    };
    let before = persisted_state(&f.app);
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
    f.app.resume("operator", "a", "work").unwrap();
    let prepared = f.prepare("copy-private-restricted");
    d.invocation_id = prepared.request.id.clone();
    d.epoch = prepared.epoch;
    if let p::Operation::FileWrite { policy_id, .. } = &mut d.operations[0] {
        *policy_id = private.id.clone();
    }
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    assert!(
        f.app.snapshot().unwrap().sources["private-copy"].revisions[0]
            .exposure
            .as_ref()
            .unwrap()
            .policies
            .iter()
            .any(|r| r.policy.id == private.id)
    );
}

#[test]
fn cleared_selection_does_not_remove_judgment_or_character_restrictions() {
    for character in [false, true] {
        let (mut f, policy, _) = restricted_fixture();
        let prepared = f.prepare("derive-and-clear");
        let mut d = decision(&prepared, true);
        d.next_assets = p::SelectionIntent::Clear {
            reason: "Clear optional selection, preserving source restrictions.".into(),
        };
        if character {
            let existing = f.app.snapshot().unwrap().personas["a"]
                .character
                .clone()
                .unwrap();
            let mut ocean = existing.ocean.clone();
            ocean.insert("openness".into(), "PROFILE RESTRICTED SENTINEL".into());
            d.operations.push(p::Operation::CharacterAdopt {
                id: "profile".into(),
                base_revision: Some(existing.revision),
                ocean,
                vad: existing.vad,
                own_voice: existing.own_voice,
                reason: "Revise the profile using supplied material.".into(),
                evidence: vec![],
            });
            d.fragment_maintenance = p::Maintenance::Change {
                reason: "Revise the characteristic account.".into(),
                operation_ids: vec!["profile".into()],
            };
        } else {
            d.work = p::WorkDisposition::Ongoing {
                reason: "JUDGMENT RESTRICTED SENTINEL".into(),
                evidence: vec![],
            };
        }
        assert_eq!(
            f.run(&prepared, &d).status,
            if character {
                AdoptionStatus::CharacterOnly
            } else {
                AdoptionStatus::Adopted
            }
        );
        assert_eq!(
            persisted_state(&f.app)["personas"]["a"]["focus"]["work"]["assets"],
            json!([])
        );
        f.app
            .revoke_policy(
                "operator",
                policy.identity(),
                "Withdraw derivative permission.",
            )
            .unwrap();
        if character {
            f.app.resume("operator", "a", "work").unwrap();
        }
        let result = f.app.prepare(
            PrepareRequest {
                id: "after-revocation".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        );
        if character {
            assert!(result.is_err());
        } else {
            let next = result.unwrap();
            assert!(!next.prompt.contains("JUDGMENT RESTRICTED SENTINEL"));
            assert_eq!(
                control(&next)["last_attributed_work_judgment"]["status"],
                "withheld_unavailable"
            );
        }
    }
}

#[test]
fn persona_origin_need_checks_current_lineage_at_intake_and_later_prepare() {
    for revoke_before in [true, false] {
        let (mut f, policy, _) = restricted_fixture();
        let prepared = f.prepare("origin-reply");
        let mut d = decision(&prepared, true);
        d.reply = p::ReplyDisposition::Authored {
            audience: vec!["human".into()].into_iter().collect(),
            text: "EXACT PERSONA ORIGINAL\\nλ".into(),
        };
        d.next_assets = p::SelectionIntent::Clear {
            reason: "Clear optional selection.".into(),
        };
        assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
        let mut input = need();
        input.id = "derived-work".into();
        input.request = "EXACT PERSONA ORIGINAL\\nλ".into();
        input.origin = NeedOrigin::PersonaResponse {
            invocation_id: "origin-reply".into(),
            operation_id: None,
        };
        if revoke_before {
            f.app
                .revoke_policy("operator", policy.identity(), "Withdraw before intake.")
                .unwrap();
        }
        let intake = f.app.add_need("operator", input);
        if revoke_before {
            assert!(intake.is_err());
        } else {
            intake.unwrap();
            let snapshot = f.app.snapshot().unwrap();
            assert_eq!(
                snapshot.work_meta["derived-work"].original.request,
                "EXACT PERSONA ORIGINAL\\nλ"
            );
            assert!(snapshot.work_meta["derived-work"].authored_origin.is_some());
            f.app
                .revoke_policy("operator", policy.identity(), "Withdraw after intake.")
                .unwrap();
            assert!(f
                .app
                .prepare(
                    PrepareRequest {
                        id: "derived-call".into(),
                        persona: "a".into(),
                        work: "derived-work".into(),
                        profile: Profile::Synthetic,
                        mode: Mode::Ordinary
                    },
                    NOW
                )
                .is_err());
        }
    }
}

#[test]
fn exact_only_search_predicates_are_excluded_and_negative_results_retain_lineage() {
    let mut observed_pages = Vec::new();
    for body in ["SECRET NEEDLE", "different protected bytes"] {
        let mut p = restricted_policy();
        p.derivative_use = f::DerivativeUse::ExactOnly;
        let mut c = config();
        c.policies.active_policies.push(p.clone());
        let mut f = Fixture::configured(true, c, seed("a"), need());
        f.accept();
        let mut asset = record("hidden", f::Scope::Assets, body);
        asset.policy = p;
        asset = asset.seal().unwrap();
        f.app
            .import_files("operator", "a", vec![asset], true, NOW)
            .unwrap();
        let prepared = f.prepare("search");
        let mut d = decision(&prepared, true);
        d.operations.push(p::Operation::Search {
            id: "query".into(),
            scope: p::Scope::Assets {},
            query: "SECRET NEEDLE".into(),
            regex: false,
            limit: 8,
        });
        assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
        let next = f.prepare("search-result");
        let core = control(&next);
        let observations = core["actual_observations"].as_array().unwrap();
        observed_pages.push(
            observations
                .iter()
                .find(|o| o["kind"] == "discovery")
                .unwrap()["payload"]["page"]
                .clone(),
        );
        assert!(!next.prompt.contains("hidden.txt"));
        let accounting = observations
            .iter()
            .find(|o| o["kind"] == "host_accounting")
            .unwrap();
        assert!(accounting["payload"]["actual_navigation"].is_string());
    }
    assert_eq!(observed_pages[0], observed_pages[1]);
    let (mut f, policy, _) = restricted_fixture();
    let prepared = f.prepare("negative-search");
    let mut d = decision(&prepared, true);
    d.next_assets = p::SelectionIntent::Clear {
        reason: "Keep the query observation only.".into(),
    };
    d.operations.push(p::Operation::Search {
        id: "negative".into(),
        scope: p::Scope::Assets {},
        query: "NO SUCH MATCH".into(),
        regex: false,
        limit: 8,
    });
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let observation = f
        .app
        .snapshot()
        .unwrap()
        .observations
        .into_values()
        .find(|o| o.kind == "discovery")
        .unwrap();
    assert!(observation
        .sources
        .iter()
        .any(|r| r.source == "restricted-source"));
    f.app
        .revoke_policy("operator", policy.identity(), "Withdraw searched scope.")
        .unwrap();
    let next = f.prepare("withheld-negative");
    assert!(!next.prompt.contains("NO SUCH MATCH"));
}

// Completion failure must roll back both the slot and digest. Later failures
// must leave the exact completed transport durable. SQLite faults exercise these
// boundaries without a production fault-injection interface.
fn terminal_boundary_fault(app: &Runtime, boundary: &str, hash: &str) -> Connection {
    let connection = Connection::open(app.state_root().join("state.sqlite3")).unwrap();
    let condition = match boundary {
        "completion" => "NEW.ns='invocation' AND NEW.k='terminal-boundary' AND json_extract(NEW.data,'$.state')='Responded'",
        "digest-only" => "NEW.ns='invocation' AND NEW.k='terminal-boundary' AND json_extract(NEW.data,'$.state')='Failed'",
        "adoption" => "NEW.ns='invocation' AND NEW.k='terminal-boundary' AND json_extract(NEW.data,'$.state') IN ('Adopted','Failed')",
        "rejected" => "NEW.ns='reservation' AND NEW.k='v:terminal-boundary' AND json_extract(NEW.data,'$.state')='Uncertain'",
        _ => panic!("unknown terminal boundary"),
    };
    let require_terminal = boundary != "completion";
    let slot = format!("terminal:{}", s::sha256_bytes(b"terminal-boundary"));
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER terminal_boundary_fault BEFORE UPDATE ON store_cells
         WHEN {condition}
         BEGIN
             SELECT CASE WHEN {require_terminal} AND NOT EXISTS (
                 SELECT 1 FROM store_cells
                 WHERE ns='terminal_evidence' AND k='{slot}'
                   AND json_type(data,'$.object')='object'
                   AND json_extract(data,'$.object.body.receipt.inner_sha256')='{hash}'
             ) THEN RAISE(ABORT,'terminal evidence was not stored first') END;
             SELECT RAISE(ABORT,'injected terminal boundary');
         END;"
        ))
        .unwrap();
    connection
}

#[test]
fn atomic_completion_and_later_disposition_boundaries_survive_restart_even_when_frozen() {
    for frozen in [false, true] {
        for (boundary, expected_state) in [
            ("completion", s::InvocationState::Dispatched),
            ("digest-only", s::InvocationState::Responded),
            ("rejected", s::InvocationState::Failed),
        ] {
            let mut f = Fixture::new(true);
            f.accept();
            let prepared = f.prepare("terminal-boundary");
            let root = f.app.state_root().to_owned();
            // Cooperating Store connections must be opened before dispatch.
            let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
            f.app.dispatch_synthetic("terminal-boundary", NOW).unwrap();
            if frozen {
                let trip = host
                    .reserve(
                        "terminal-freeze-trip",
                        s::Resources {
                            output_bytes: 1,
                            ..s::Resources::default()
                        },
                        s::BudgetClass::Ordinary,
                    )
                    .unwrap();
                assert!(matches!(
                    host.settle(
                        &trip.id,
                        s::Usage::Measured(s::Resources {
                            output_bytes: 2,
                            ..s::Resources::default()
                        })
                    ),
                    Err(s::StoreError::Overrun(_))
                ));
            }
            let mut d = decision(&prepared, true);
            d.epoch += 1;
            let bytes = serde_json::to_vec(&d).unwrap();
            let hash = s::sha256_bytes(&bytes);
            let fault = terminal_boundary_fault(&f.app, boundary, &hash);
            let state_before = persisted_state(&f.app);
            let ledger_before = f.app.ledger().unwrap();
            let attempts = ledger_before.ordinary.consumed.attempts;
            let identities = ledger_before.permanent_records;
            let epoch = f.app.epoch().unwrap();
            let error = f
                .app
                .adopt_response("terminal-boundary", &bytes, NOW)
                .unwrap_err();
            assert!(matches!(
                &error,
                RuntimeError::Store(s::StoreError::Sqlite(_))
            ));
            assert!(
                error.to_string().contains("injected terminal boundary"),
                "{error}"
            );
            assert_only_navigation_charged(ledger_before, f.app.ledger().unwrap());
            assert_eq!(f.app.epoch().unwrap(), epoch);
            assert_eq!(persisted_state(&f.app), state_before);
            let inv = f.app.invocation("terminal-boundary").unwrap();
            assert_eq!(inv.state, expected_state);
            assert_eq!(inv.prepared_epoch, prepared.epoch);
            assert_eq!(
                inv.response_hash.as_deref(),
                if boundary == "completion" {
                    None
                } else {
                    Some(hash.as_str())
                }
            );
            assert_eq!(
                host.trusted_host()
                    .reservation(&inv.reservation)
                    .unwrap()
                    .state,
                s::ReservationState::Reserved
            );
            if boundary == "completion" {
                assert!(inv.history.iter().all(|receipt| receipt.evidence.is_none()));
                assert!(f.app.terminal_evidence("terminal-boundary").is_err());
                fault
                    .execute_batch("DROP TRIGGER terminal_boundary_fault")
                    .unwrap();
                drop(fault);
                drop(host);
                drop(f.app);
                f.app = Runtime::open(&root).unwrap();
                let recovered = f.app.invocation("terminal-boundary").unwrap();
                assert_eq!(recovered.state, s::InvocationState::Uncertain);
                assert_eq!(recovered.response_hash, None);
                assert!(f.app.terminal_evidence("terminal-boundary").is_err());
                assert!(f.app.resume("operator", "a", "work").is_err());
                assert!(f.app.dispatch_synthetic("terminal-boundary", NOW).is_err());
                assert_eq!(f.app.epoch().unwrap(), epoch + 1);
                assert_eq!(f.app.ledger().unwrap().frozen, frozen);
                assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
                assert_eq!(f.app.ledger().unwrap().permanent_records, identities);
                assert_eq!(persisted_state(&f.app), state_before);
                continue;
            }
            let terminal = f.app.terminal_evidence("terminal-boundary").unwrap();
            assert_eq!(
                inv.history
                    .iter()
                    .find(|receipt| receipt.status == s::ReceiptStatus::Responded)
                    .unwrap()
                    .evidence,
                Some(terminal.exact_ref().unwrap())
            );
            assert_eq!(terminal.body["prepared_epoch"], prepared.epoch);
            assert_eq!(terminal.body["prompt_sha256"], prepared.prompt_sha256);
            assert_eq!(terminal.body["receipt"]["kind"], "synthetic_complete");
            assert_eq!(terminal.body["receipt"]["inner_sha256"], hash);
            let reference = terminal.exact_ref().unwrap();
            fault
                .execute_batch("DROP TRIGGER terminal_boundary_fault")
                .unwrap();
            drop(fault);
            drop(host);
            drop(f.app);
            f.app = Runtime::open(&root).unwrap();
            assert_eq!(f.app.ledger().unwrap().frozen, frozen);
            assert_eq!(f.app.epoch().unwrap(), epoch);
            let recovered = f.app.invocation("terminal-boundary").unwrap();
            assert_eq!(recovered.state, expected_state);
            assert_eq!(
                f.app.terminal_evidence("terminal-boundary").unwrap(),
                terminal
            );
            assert!(f.app.adopted("terminal-boundary").is_err());
            assert!(f.app.dispatch_synthetic("terminal-boundary", NOW).is_err());
            let mut forged = reference.clone();
            forged.sha256 = s::sha256_bytes(b"not the stored receipt");
            assert!(f
                .app
                .acknowledge_complete_rejection("operator", "terminal-boundary", forged)
                .is_err());
            assert_eq!(f.app.invocation("terminal-boundary").unwrap(), recovered);
            f.app
                .acknowledge_complete_rejection("operator", "terminal-boundary", reference.clone())
                .unwrap();
            let closed = f.app.invocation("terminal-boundary").unwrap();
            assert_eq!(closed.state, s::InvocationState::Failed);
            assert_eq!(closed.response_hash.as_deref(), Some(hash.as_str()));
            let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
            let usage = host
                .trusted_host()
                .reservation(&closed.reservation)
                .unwrap();
            assert_eq!(usage.state, s::ReservationState::Uncertain);
            assert_eq!(usage.measured, None);
            let acknowledged_epoch = f.app.epoch().unwrap();
            f.app
                .acknowledge_complete_rejection("operator", "terminal-boundary", reference.clone())
                .unwrap();
            assert_eq!(f.app.invocation("terminal-boundary").unwrap(), closed);
            assert_eq!(
                host.trusted_host()
                    .reservation(&closed.reservation)
                    .unwrap(),
                usage
            );
            assert_eq!(f.app.epoch().unwrap(), acknowledged_epoch);
            assert_eq!(persisted_state(&f.app), state_before);
            assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
            assert_eq!(f.app.ledger().unwrap().permanent_records, identities);
            drop(host);
            drop(f.app);
            f.app = Runtime::open(&root).unwrap();
            f.app
                .acknowledge_complete_rejection("operator", "terminal-boundary", reference)
                .unwrap();
            assert_eq!(f.app.invocation("terminal-boundary").unwrap(), closed);
            assert_eq!(
                f.app.terminal_evidence("terminal-boundary").unwrap(),
                terminal
            );
            assert_eq!(f.app.epoch().unwrap(), acknowledged_epoch);
            assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
            assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > 0);
        }
    }
}

#[test]
fn receipt_replay_keeps_exact_bytes_and_never_rebases_a_concurrent_mutation() {
    for mutate in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("terminal-boundary");
        let root = f.app.state_root().to_owned();
        f.app.dispatch_synthetic("terminal-boundary", NOW).unwrap();
        let d = decision(&prepared, true);
        let compact = serde_json::to_vec(&d).unwrap();
        let bytes = format!(" \n{}\n ", std::str::from_utf8(&compact).unwrap()).into_bytes();
        let hash = s::sha256_bytes(&bytes);
        let fault = terminal_boundary_fault(&f.app, "adoption", &hash);
        let attempts = f.app.ledger().unwrap().ordinary.consumed.attempts;
        let error = f
            .app
            .adopt_response("terminal-boundary", &bytes, NOW)
            .unwrap_err();
        assert!(
            error.to_string().contains("injected terminal boundary"),
            "{error}"
        );
        assert_eq!(f.app.epoch().unwrap(), prepared.epoch);
        let terminal = f.app.terminal_evidence("terminal-boundary").unwrap();
        assert_eq!(
            terminal.body["response_body"]["text"]
                .as_str()
                .unwrap()
                .as_bytes(),
            bytes.as_slice()
        );
        assert_eq!(terminal.body["response_body"]["sha256"], hash);
        let reference = terminal.exact_ref().unwrap();
        fault
            .execute_batch("DROP TRIGGER terminal_boundary_fault")
            .unwrap();
        drop(fault);
        // Ownership survives a response-persistence error until this Runtime
        // finishes handling the exact response or is dropped.
        let mut peer = Runtime::open(&root).unwrap();
        peer.status().unwrap();
        assert_eq!(f.app.epoch().unwrap(), prepared.epoch);
        // Even equivalent parsed Decisions with different exact bytes conflict
        // with the atomically retained receipt and digest.
        assert!(matches!(
            f.app.adopt_response("terminal-boundary", &compact, NOW),
            Err(RuntimeError::Store(s::StoreError::Conflict(_)))
        ));
        let inv = f.app.invocation("terminal-boundary").unwrap();
        assert_eq!(inv.state, s::InvocationState::Responded);
        assert_eq!(inv.response_hash.as_deref(), Some(hash.as_str()));
        assert_eq!(
            inv.history.last().unwrap().evidence.as_ref(),
            Some(&reference)
        );
        assert_eq!(
            f.app.terminal_evidence("terminal-boundary").unwrap(),
            terminal
        );
        if mutate {
            peer.pause(
                "operator",
                "work",
                "Concurrent change after receipt persistence.",
            )
            .unwrap();
        }
        let state_before = persisted_state(&f.app);
        // Re-submit the already received exact bytes locally; never redispatch.
        let report = f
            .app
            .adopt_response("terminal-boundary", &bytes, NOW)
            .unwrap();
        if mutate {
            assert_eq!(report.status, AdoptionStatus::Held);
            assert!(report.reason.as_ref().unwrap().contains("epoch"));
            assert_eq!(report.accepted_epoch, None);
            assert_eq!(persisted_state(&f.app), state_before);
            assert!(f.app.adopted("terminal-boundary").is_err());
        } else {
            assert_eq!(report.status, AdoptionStatus::Adopted);
            assert_eq!(report.accepted_epoch, Some(prepared.epoch + 1));
            let adopted = f.app.adopted("terminal-boundary").unwrap();
            assert_eq!(
                adopted.authored_json.as_deref(),
                Some(std::str::from_utf8(&bytes).unwrap())
            );
            assert_eq!(adopted.response_hash, hash);
            assert_eq!(
                adopted.provider_observation.unwrap()["terminal_evidence"],
                serde_json::to_value(&reference).unwrap()
            );
        }
        assert_eq!(f.app.epoch().unwrap(), prepared.epoch + 1);
        assert_eq!(
            f.app
                .invocation("terminal-boundary")
                .unwrap()
                .prepared_epoch,
            prepared.epoch
        );
        assert_eq!(
            f.app.terminal_evidence("terminal-boundary").unwrap(),
            terminal
        );
        assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
        drop(peer);
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        assert_eq!(
            f.app.terminal_evidence("terminal-boundary").unwrap(),
            terminal
        );
        if mutate {
            f.app
                .acknowledge_complete_rejection("operator", "terminal-boundary", reference)
                .unwrap();
        } else {
            let replay = f
                .app
                .adopt_response("terminal-boundary", &bytes, NOW)
                .unwrap();
            assert_eq!(
                serde_json::to_value(replay).unwrap(),
                serde_json::to_value(report).unwrap()
            );
        }
        assert!(f.app.dispatch_synthetic("terminal-boundary", NOW).is_err());
        assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
    }
}

#[test]
fn complete_wrong_headers_are_journaled_held_and_explicitly_recoverable() {
    for wrong_id in [true, false] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("wrong-header");
        let mut d = decision(&prepared, true);
        if wrong_id {
            d.invocation_id = "other-invocation".into();
        } else {
            d.epoch += 1;
        }
        let bytes = serde_json::to_vec(&d).unwrap();
        let before = persisted_state(&f.app);
        f.app.dispatch_synthetic("wrong-header", NOW).unwrap();
        let report = f.app.adopt_response("wrong-header", &bytes, NOW).unwrap();
        assert_eq!(report.status, AdoptionStatus::Held);
        assert_eq!(persisted_state(&f.app), before);
        let inv = f.app.invocation("wrong-header").unwrap();
        assert_eq!(inv.state, s::InvocationState::Failed);
        assert_eq!(inv.response_hash, Some(s::sha256_bytes(&bytes)));
        assert_eq!(
            f.app.terminal_evidence("wrong-header").unwrap().body["receipt"]["inner_sha256"],
            s::sha256_bytes(&bytes)
        );
        assert!(f.app.dispatch_synthetic("wrong-header", NOW).is_err());
        let root = f.app.state_root().to_owned();
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        f.app.resume("operator", "a", "work").unwrap();
        let next = f.prepare("recovered-header");
        let private_reason = if wrong_id {
            "response invocation identity mismatch"
        } else {
            "response epoch mismatch"
        };
        assert!(report.reason.as_deref().unwrap().contains(private_reason));
        assert!(inv
            .history
            .last()
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains(private_reason));
        assert!(next.prompt.contains(private_reason));
        assert!(!next.prompt.contains("private_invocation_history_only"));
        assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > 0);
    }
}

#[test]
fn two_reference_character_bundle_is_not_reexpanded_from_every_member() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut c = config();
    c.corpus.max_closure_references = 2;
    let mut app = Runtime::create(&temp.path().join("state"), c, "operator").unwrap();
    app.add_persona_seed("operator", seed("b")).unwrap();
    let caveat = record(
        "voice-caveat",
        f::Scope::Assets,
        "A required exact voice qualification.",
    );
    app.import_files("operator", "b", vec![caveat.clone()], true, NOW)
        .unwrap();
    let mut voice = record(
        "voice",
        f::Scope::Fragments,
        "I retain my complete qualification.",
    );
    voice
        .qualifications
        .push(caveat.reference(f::Representation::Utf8));
    voice = voice.seal().unwrap();
    let (ocean, vad) = profiles();
    app.import_persona(
        "operator",
        PersonaImport {
            seed: seed("a"),
            ocean,
            vad,
            voice,
        },
        NOW,
    )
    .unwrap();
    app.add_need("operator", need()).unwrap();
    let mut f = Fixture { _temp: temp, app };
    f.accept();
    let prepared = f.prepare("two-node");
    assert_eq!(prepared.bundle_roots.as_ref().unwrap().mandatory.len(), 1);
    assert_eq!(prepared.sources.len(), 2);
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn read_and_self_message_have_distinct_immutable_observation_ids() {
    let mut f = Fixture::new(true);
    f.accept();
    let source = record("read-source", f::Scope::Assets, "IMMUTABLE READ SENTINEL");
    f.app
        .import_files("operator", "a", vec![source.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("slash/invocation");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Read {
        id: "message/m".into(),
        reference: reference(&source),
    });
    d.operations.push(p::Operation::Communicate {
        id: "m".into(),
        recipient: "a".into(),
        subject: "Separate message".into(),
        text: "MESSAGE SENTINEL".into(),
    });
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let snapshot = f.app.snapshot().unwrap();
    let read = snapshot
        .observations
        .values()
        .find(|o| o.invocation.as_deref() == Some("slash/invocation") && o.kind == "qualified_read")
        .unwrap();
    let message = snapshot
        .observations
        .values()
        .find(|o| {
            o.invocation.as_deref() == Some("slash/invocation")
                && o.kind == "received_local_persona_message"
        })
        .unwrap();
    assert_ne!(read.id, message.id);
    assert!(serde_json::to_string(&read.payload)
        .unwrap()
        .contains("IMMUTABLE READ SENTINEL"));
    assert!(serde_json::to_string(&message.payload)
        .unwrap()
        .contains("MESSAGE SENTINEL"));
}

fn diagnostic_slot(app: &Runtime, id: &str) -> (Value, u64) {
    let key = format!("terminal:{}", s::sha256_bytes(id.as_bytes()));
    let (_, cells) = closeout_snapshot(app.state_root());
    let cell = cells
        .into_iter()
        .find(|cell| cell.0 == "terminal_evidence" && cell.1 == key)
        .unwrap();
    (serde_json::from_slice(&cell.2).unwrap(), cell.3 as u64)
}

#[test]
fn held_decision_retains_exact_private_body_without_partial_adoption_after_reopen() {
    for ownership_conflict in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let foreign = record(
            "diagnostic-foreign",
            f::Scope::Assets,
            "Existing foreign material.",
        );
        f.app
            .import_files("operator", "b", vec![foreign.clone()], false, NOW)
            .unwrap();
        let prepared = f.prepare("diagnostic-held");
        let mut d = decision(&prepared, true);
        let private = "PRIVATE_EXACT_OPERATION quote \" backslash \\ and λ\n";
        d.operations.push(write(
            "candidate-write",
            p::Scope::Assets {},
            "diagnostic-candidate",
            private,
            None,
        ));
        d.operations.push(p::Operation::Communicate {
            id: "candidate-message".into(),
            recipient: "human".into(),
            subject: "Unaccepted fixture proposal".into(),
            text: private.into(),
        });
        let mut mutations = vec!["candidate-write".into()];
        if ownership_conflict {
            d.operations.push(write(
                "foreign-write",
                p::Scope::Assets {},
                &foreign.source,
                private,
                Some(foreign.revision.clone()),
            ));
            mutations.push("foreign-write".into());
        } else {
            d.operations.push(p::Operation::Effect {
                id: "unavailable-effect".into(),
                capability_id: "unavailable.fixture".into(),
                binding_revision: "fixture-binding".into(),
                inputs: Value::Null,
                depends_on: vec![],
            });
        }
        d.asset_maintenance = p::Maintenance::Change {
            reason: "Account for every proposed asset mutation.".into(),
            operation_ids: mutations,
        };
        d.reply = p::ReplyDisposition::Authored {
            audience: vec!["human".into()],
            text: private.into(),
        };
        let encoded = serde_json::to_string(&d).unwrap().replace('λ', "\\u03bb");
        let raw = format!(" \r\n{encoded}\t ");
        assert_eq!(
            p::parse_decision(raw.as_bytes(), config().response_bytes).unwrap(),
            d
        );
        let hash = s::sha256_bytes(raw.as_bytes());
        let state_before = persisted_state(&f.app);
        let ledger_before = f.app.ledger().unwrap();
        let slot_before = diagnostic_slot(&f.app, &prepared.request.id);
        assert!(slot_before.0["object"].is_null());
        f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
        let report = f
            .app
            .adopt_response(&prepared.request.id, raw.as_bytes(), NOW)
            .unwrap();
        assert_eq!(report.status, AdoptionStatus::Held);
        assert!(report.applied.is_empty());
        assert!(report
            .reason
            .as_ref()
            .unwrap()
            .contains(if ownership_conflict {
                "ownership"
            } else {
                "capability"
            }));
        assert_eq!(persisted_state(&f.app), state_before);
        assert!(f.app.adopted(&prepared.request.id).is_err());
        let inv = f.app.invocation(&prepared.request.id).unwrap();
        assert_eq!(inv.state, s::InvocationState::Failed);
        assert_eq!(inv.response_hash.as_deref(), Some(hash.as_str()));
        let terminal = f.app.terminal_evidence(&prepared.request.id).unwrap();
        let body = &terminal.body["response_body"];
        assert_eq!(body["text"].as_str().unwrap().as_bytes(), raw.as_bytes());
        assert_eq!(body["bytes"], raw.len());
        assert_eq!(body["sha256"], hash);
        assert_eq!(body["kind"], "decision_json");
        assert_eq!(terminal.body["prepare_hash"], inv.prepare_hash);
        assert_eq!(terminal.body["invocation_id"], prepared.request.id);
        assert_eq!(terminal.body["prepared_epoch"], prepared.epoch);
        assert_eq!(terminal.body["prompt_sha256"], prepared.prompt_sha256);
        assert_eq!(
            inv.history
                .iter()
                .find(|r| r.status == s::ReceiptStatus::Responded)
                .unwrap()
                .evidence,
            Some(terminal.exact_ref().unwrap())
        );
        let slot_after = diagnostic_slot(&f.app, &prepared.request.id);
        assert_eq!(slot_after.1, slot_before.1);
        assert_eq!(
            slot_after.0["max_object_bytes"],
            slot_before.0["max_object_bytes"]
        );
        assert!(serde_json::to_vec(&slot_after.0).unwrap().len() as u64 <= slot_after.1);
        let ledger_after = f.app.ledger().unwrap();
        assert_eq!(ledger_after.limits, ledger_before.limits);
        assert_eq!(
            ledger_after.permanent_records,
            ledger_before.permanent_records
        );
        assert_eq!(
            ledger_after.ordinary.consumed.attempts,
            ledger_before.ordinary.consumed.attempts
        );
        assert_eq!(
            ledger_after.ordinary.consumed.retained_bytes,
            ledger_before.ordinary.consumed.retained_bytes
        );
        assert_eq!(
            ledger_after
                .ordinary
                .reserved
                .checked_add(ledger_after.ordinary.uncertain)
                .unwrap(),
            ledger_before
                .ordinary
                .reserved
                .checked_add(ledger_before.ordinary.uncertain)
                .unwrap()
        );
        let root = f.app.state_root().to_owned();
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        assert_eq!(persisted_state(&f.app), state_before);
        assert_eq!(f.app.invocation(&prepared.request.id).unwrap(), inv);
        assert_eq!(
            f.app.terminal_evidence(&prepared.request.id).unwrap(),
            terminal
        );
        f.app.resume("operator", "a", "work").unwrap();
        let resumed = persisted_state(&f.app);
        assert_eq!(resumed["work_meta"], state_before["work_meta"]);
        let recovery = resumed["observations"]
            .as_object()
            .unwrap()
            .values()
            .find(|o| o["kind"] == "terminal_recovery" && o["invocation"] == prepared.request.id)
            .unwrap();
        assert!(recovery["payload"]["terminal"]["response_body"]
            .get("text")
            .is_none());
        assert_eq!(
            recovery["payload"]["terminal"]["response_body"]["availability"],
            "private_terminal_only"
        );
        assert_eq!(
            recovery["payload"]["terminal_evidence"],
            json!(terminal.exact_ref().unwrap())
        );
        assert!(!serde_json::to_string(&resumed)
            .unwrap()
            .contains("PRIVATE_EXACT_OPERATION"));
        assert_eq!(
            f.app.terminal_evidence(&prepared.request.id).unwrap(),
            terminal
        );
    }
}

#[test]
fn terminal_slot_prepays_escaping_and_oversize_ingress_cannot_fill_or_expand_it() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("diagnostic-size");
    let maximum = config().response_bytes;
    assert_eq!(prepared.terminal_response_bytes, Some(maximum));
    let (slot, cell_capacity) = diagnostic_slot(&f.app, &prepared.request.id);
    let object_capacity = 65_536 + 6 * maximum as u64 + 1024;
    assert_eq!(slot["max_object_bytes"], object_capacity);
    assert_eq!(cell_capacity, object_capacity + 16 * 1024);
    assert_eq!(object_capacity, 263_168);
    assert_eq!(2 * (cell_capacity + 17 + 73 + 128), 559_540);
    let mut d = decision(&prepared, true);
    d.reply = p::ReplyDisposition::Authored {
        audience: vec!["human".into()],
        text: "quote \" slash \\ λ\n".repeat(512),
    };
    d.operations.push(p::Operation::Effect {
        id: "bounded-unavailable".into(),
        capability_id: "unavailable.fixture".into(),
        binding_revision: "fixture-binding".into(),
        inputs: Value::Null,
        depends_on: vec![],
    });
    let mut raw = serde_json::to_string(&d).unwrap();
    assert!(raw.len() < maximum);
    raw.push_str(&"\t".repeat(maximum - raw.len()));
    assert_eq!(raw.len(), maximum);
    assert!(p::parse_decision(raw.as_bytes(), maximum).is_ok());
    let state_before = persisted_state(&f.app);
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    for length in [maximum + 1, 262_145] {
        let mut oversized = raw.clone();
        oversized.push_str(&" ".repeat(length - oversized.len()));
        let mut before = closeout_snapshot(f.app.state_root());
        assert!(f
            .app
            .adopt_response(&prepared.request.id, oversized.as_bytes(), NOW)
            .is_err());
        let after = closeout_snapshot(f.app.state_root());
        for class in ["ordinary", "closeout"] {
            let previous = before.0[class]["consumed"]["navigation_bytes"]
                .as_u64()
                .unwrap();
            let current = after.0[class]["consumed"]["navigation_bytes"]
                .as_u64()
                .unwrap();
            assert!(current >= previous);
            before.0[class]["consumed"]["navigation_bytes"] = json!(current);
        }
        assert_eq!(before, after);
        assert_eq!(
            diagnostic_slot(&f.app, &prepared.request.id),
            (slot.clone(), cell_capacity)
        );
        let inv = f.app.invocation(&prepared.request.id).unwrap();
        assert_eq!(inv.state, s::InvocationState::Dispatched);
        assert!(inv.response_hash.is_none());
        assert_eq!(persisted_state(&f.app), state_before);
    }
    let retained_before = f.app.ledger().unwrap().ordinary.consumed.retained_bytes;
    assert_eq!(
        f.app
            .adopt_response(&prepared.request.id, raw.as_bytes(), NOW)
            .unwrap()
            .status,
        AdoptionStatus::Held
    );
    let terminal = f.app.terminal_evidence(&prepared.request.id).unwrap();
    assert_eq!(
        terminal.body["response_body"]["text"]
            .as_str()
            .unwrap()
            .as_bytes(),
        raw.as_bytes()
    );
    assert_eq!(
        terminal.body["response_body"]["sha256"],
        s::sha256_bytes(raw.as_bytes())
    );
    let serialized = serde_json::to_vec(&terminal).unwrap().len() as u64;
    assert!(serialized > maximum as u64);
    assert!(serialized <= object_capacity);
    let filled = diagnostic_slot(&f.app, &prepared.request.id);
    assert_eq!(filled.1, cell_capacity);
    assert!(serde_json::to_vec(&filled.0).unwrap().len() as u64 <= cell_capacity);
    assert_eq!(
        f.app.ledger().unwrap().ordinary.consumed.retained_bytes,
        retained_before
    );
    assert_eq!(persisted_state(&f.app), state_before);
}

#[test]
fn complete_response_is_durable_before_ordinary_state_read_exhaustion() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("completion-before-state-read");
    let root = f.app.state_root().to_owned();
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    let invocation = f.app.invocation(&prepared.request.id).unwrap();
    // Exhaust the complete ordinary navigation allowance through a real finite
    // reservation. Protected terminal reads and the prepaid slot remain available.
    exhaust_control_navigation(&mut host, s::BudgetClass::Ordinary);
    let before = persisted_state(&f.app);
    let raw = serde_json::to_vec(&decision(&prepared, true)).unwrap();
    assert!(f
        .app
        .adopt_response(&prepared.request.id, &raw, NOW)
        .is_err());
    let terminal = f.app.terminal_evidence(&prepared.request.id).unwrap();
    assert_eq!(
        terminal.body["response_body"]["text"]
            .as_str()
            .unwrap()
            .as_bytes(),
        raw
    );
    let completed = host
        .trusted_host()
        .invocation(&prepared.request.id)
        .unwrap();
    assert_eq!(completed.state, s::InvocationState::Responded);
    assert_eq!(completed.response_hash, Some(s::sha256_bytes(&raw)));
    assert_eq!(persisted_state(&f.app), before);
    assert_eq!(
        host.trusted_host()
            .reservation(&invocation.reservation)
            .unwrap()
            .state,
        s::ReservationState::Reserved
    );
    drop(host);
    drop(f.app);
    let mut reopened = Runtime::open_for_closeout(&root).unwrap();
    assert_eq!(
        reopened.terminal_evidence(&prepared.request.id).unwrap(),
        terminal
    );
}

#[test]
fn legacy_unfilled_metadata_slot_completes_large_response_without_expansion() {
    let mut f = Fixture::new(true);
    f.accept();
    let mut donor = Fixture::new(true);
    donor.accept();
    let mut prepared = donor.prepare("legacy-unfilled-slot");
    prepared.terminal_response_bytes = None;
    let root = f.app.state_root().to_owned();
    let old = closeout_head(&root);
    let new = closeout_head(donor.app.state_root());
    assert_eq!(new.revision, old.revision + 1);
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let donor_inv = donor.app.invocation(&prepared.request.id).unwrap();
    let reservation: s::ReservationRecord = adoption_accounting_record(
        donor.app.state_root(),
        "reservation",
        &format!("v:{}", donor_inv.id),
    );
    host.prepare_invocation_with_changes(
        f.app.epoch().unwrap(),
        vec![s::Change {
            expected_revision: Some(old.revision),
            object: new,
            depends_on: Vec::new(),
        }],
        s::InvocationSpec {
            id: prepared.request.id.clone(),
            expected_epoch: prepared.epoch,
            request: serde_json::to_value(&prepared).unwrap(),
            reservation: reservation.amount,
            class: s::BudgetClass::Ordinary,
        },
        vec![s::TerminalEvidenceReservation {
            id: format!(
                "terminal:{}",
                s::sha256_bytes(prepared.request.id.as_bytes())
            ),
            max_object_bytes: 65_536,
            class: s::BudgetClass::Ordinary,
        }],
    )
    .unwrap();
    let stored = f.app.invocation(&prepared.request.id).unwrap();
    assert!(stored
        .request
        .as_ref()
        .unwrap()
        .get("terminal_response_bytes")
        .is_none());
    let (_, capacity) = diagnostic_slot(&f.app, &prepared.request.id);
    assert_eq!(capacity, 65_536 + 16_384);
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    let mut response = decision(&prepared, true);
    response.epoch += 1;
    // Leading JSON whitespace is authored, bounded data; no large prose limit is bypassed.
    let raw = format!(
        "{}{}",
        " ".repeat(100_000),
        serde_json::to_string(&response).unwrap()
    );
    let retained = f.app.ledger().unwrap().ordinary.consumed.retained_bytes;
    assert_eq!(
        f.app
            .adopt_response(&prepared.request.id, raw.as_bytes(), NOW)
            .unwrap()
            .status,
        AdoptionStatus::Held
    );
    let terminal = f.app.terminal_evidence(&prepared.request.id).unwrap();
    assert!(terminal.body.get("response_body").is_none());
    assert_eq!(
        terminal.body["receipt"]["inner_sha256"],
        s::sha256_bytes(raw.as_bytes())
    );
    assert_eq!(diagnostic_slot(&f.app, &prepared.request.id).1, capacity);
    assert_eq!(
        f.app.ledger().unwrap().ordinary.consumed.retained_bytes,
        retained
    );
    f.app.resume("operator", "a", "work").unwrap();
    let next = f.prepare("after-legacy-unfilled-slot");
    assert!(interface_control(&next)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["payload"]["terminal"]["response_body"]["availability"] == "unavailable"));
}

#[test]
fn legacy_metadata_only_terminal_is_replayed_unchanged_and_reported_unavailable() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("legacy-diagnostic");
    let mut legacy_request = serde_json::to_value(&prepared).unwrap();
    legacy_request
        .as_object_mut()
        .unwrap()
        .remove("terminal_response_bytes");
    let legacy: Prepared = serde_json::from_value(legacy_request.clone()).unwrap();
    assert!(legacy.terminal_response_bytes.is_none());
    assert_eq!(serde_json::to_value(legacy).unwrap(), legacy_request);
    let mut d = decision(&prepared, true);
    d.epoch += 1;
    let raw = serde_json::to_vec(&d).unwrap();
    let hash = s::sha256_bytes(&raw);
    let root = f.app.state_root().to_owned();
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    let inv = f.app.invocation(&prepared.request.id).unwrap();
    let original = s::StoredObject {
        id: format!(
            "terminal:{}",
            s::sha256_bytes(prepared.request.id.as_bytes())
        ),
        revision: 1,
        kind: "runtime-terminal-transport".into(),
        body: json!({
            "invocation_id":prepared.request.id,"prepared_epoch":prepared.epoch,
            "prompt_sha256":prepared.prompt_sha256,
            "receipt":{"kind":"synthetic_complete","synthetic":true,
                "inner_sha256":hash,"transport_sha256":hash,
                "response_bytes":raw.len(),"provider_usage":"unknown"}
        }),
    };
    let completion = host
        .trusted_host()
        .complete_invocation_transport(
            &prepared.request.id,
            &inv.prepare_hash,
            &hash,
            original.clone(),
            |bound, observed| {
                assert_eq!(bound.request, inv.request);
                assert_eq!(observed, &original);
                Ok(())
            },
        )
        .unwrap();
    let state_before = persisted_state(&f.app);
    let retained_before = f.app.ledger().unwrap().ordinary.consumed.retained_bytes;
    assert_eq!(
        f.app
            .adopt_response(&prepared.request.id, &raw, NOW)
            .unwrap()
            .status,
        AdoptionStatus::Held
    );
    assert_eq!(persisted_state(&f.app), state_before);
    assert_eq!(
        f.app.terminal_evidence(&prepared.request.id).unwrap(),
        original
    );
    assert!(original.body.get("response_body").is_none());
    assert_eq!(
        f.app.ledger().unwrap().ordinary.consumed.retained_bytes,
        retained_before
    );
    let closed = f.app.invocation(&prepared.request.id).unwrap();
    assert_eq!(
        closed
            .history
            .iter()
            .find(|r| r.status == s::ReceiptStatus::Responded),
        Some(&completion)
    );
    drop(host);
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    assert_eq!(f.app.invocation(&prepared.request.id).unwrap(), closed);
    assert_eq!(
        f.app.terminal_evidence(&prepared.request.id).unwrap(),
        original
    );
    assert_eq!(persisted_state(&f.app), state_before);
    f.app
        .acknowledge_complete_rejection(
            "operator",
            &prepared.request.id,
            original.exact_ref().unwrap(),
        )
        .unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let resumed = persisted_state(&f.app);
    assert_eq!(resumed["work_meta"], state_before["work_meta"]);
    let recovery = resumed["observations"]
        .as_object()
        .unwrap()
        .values()
        .find(|o| o["kind"] == "terminal_recovery" && o["invocation"] == prepared.request.id)
        .unwrap();
    assert_eq!(
        recovery["payload"]["terminal"]["response_body"],
        json!({"availability":"unavailable"})
    );
    assert_eq!(
        f.app.terminal_evidence(&prepared.request.id).unwrap(),
        original
    );
    let next = f.prepare("fresh-diagnostic");
    assert_eq!(next.terminal_response_bytes, Some(config().response_bytes));
    let after = persisted_state(&f.app);
    assert_eq!(
        after["work_meta"]["work"]["attempts_left"]
            .as_u64()
            .unwrap(),
        resumed["work_meta"]["work"]["attempts_left"]
            .as_u64()
            .unwrap()
            - 1
    );
    assert_eq!(
        diagnostic_slot(&f.app, &next.request.id).0["max_object_bytes"],
        263_168
    );
    assert_eq!(
        f.app.terminal_evidence(&prepared.request.id).unwrap(),
        original
    );
}

#[test]
fn carrier_is_closed_unique_scalar_valid_and_preserves_inner_authored_bytes() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("carrier-parser");
    let mut d = decision(&prepared, true);
    d.work = p::WorkDisposition::Ongoing {
        reason: "Literal \\n, newline\n, quote \" and λ".into(),
        evidence: vec![],
    };
    let inner = format!(" \n{}\n ", serde_json::to_string(&d).unwrap());
    let outer = serde_json::to_vec(&json!({"decision_json":inner})).unwrap();
    let decoded = decode_provider_carrier(&outer, 32_768).unwrap();
    assert_eq!(decoded.as_bytes(), inner.as_bytes());
    assert_eq!(
        p::parse_decision(decoded.as_bytes(), 32_768)
            .unwrap()
            .invocation_id,
        "carrier-parser"
    );
    assert_eq!(
        provider_schema(),
        json!({"type":"object","properties":{"decision_json":{"type":"string"}},
        "required":["decision_json"],"additionalProperties":false})
    );
    assert_eq!(prepared.output_schema, provider_schema());
    assert!(prepared
        .prompt
        .contains(&serde_json::to_string(&p::decision_schema()).unwrap()));
    assert!(
        prepared.assembled_bytes_with_reserve
            >= prepared.prompt.len()
                + serde_json::to_vec(&provider_schema()).unwrap().len()
                + config().output_reserve_bytes
    );
    for bad in [
        r#"{"decision_json":"{}","decision_json":"{}"}"#,
        r#"{"decision_json":"{}","\u0064ecision_json":"{}"}"#,
        r#"{"decision_json":"\ud800"}"#,
        r#"{"decision_json":{}}"#,
        r#"{"decision_json":"{}","extra":true}"#,
        r#"{"response":"{}"}"#,
        r#"{"decision_json":"{}"} trailing"#,
    ] {
        assert!(
            decode_provider_carrier(bad.as_bytes(), 32_768).is_err(),
            "{bad}"
        );
    }
    let malformed =
        decode_provider_carrier(br#"{"decision_json":"{\"version\":1}"}"#, 32_768).unwrap();
    assert!(p::parse_decision(malformed.as_bytes(), 32_768).is_err());
}

#[cfg(target_os = "linux")]
use direct_provider_cases::{transport_fixture, Behavior as TransportBehavior};

#[cfg(target_os = "linux")]
fn live_fixture() -> Fixture {
    let mut c = config();
    c.allow_direct_provider = true;
    let mut primary = seed("a");
    primary.model_confirmed = true;
    let mut f = Fixture::configured(true, c, primary, need());
    f.accept();
    f
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn actual_application_transport_uses_carrier_and_retains_terminal_failures() {
    for mode in [
        "correct",
        "wrong-id",
        "wrong-epoch",
        "held-interface",
        "malformed-inner",
        "empty-inner",
        "escaped-malformed-inner",
        "wrong-carrier",
        "duplicate-outer",
        "malformed-outer",
        "complete-stream-failure",
        "bounded-stream-failure",
        "native",
        "native-result",
        "action",
        "action-result",
    ] {
        let mut f = live_fixture();
        let previous = f.app.invocation("accept").unwrap();
        let mut expected: Prepared = serde_json::from_value(previous.request.unwrap()).unwrap();
        expected.request.id = "transport-call".into();
        expected.epoch = f.app.epoch().unwrap() + 1;
        let mut d = decision(&expected, true);
        if mode == "wrong-id" {
            d.invocation_id = "wrong".into();
        }
        if mode == "wrong-epoch" {
            d.epoch += 1;
        }
        if matches!(
            mode,
            "held-interface" | "complete-stream-failure" | "bounded-stream-failure"
        ) {
            d.operations.push(write(
                "held-write",
                p::Scope::Assets {},
                "held-transport-source",
                "PRIVATE TRANSPORT OPERATION \"\\λ",
                None,
            ));
            d.asset_maintenance = p::Maintenance::Change {
                reason: "Account for the proposed save.".into(),
                operation_ids: vec!["held-write".into()],
            };
            d.operations.push(p::Operation::Communicate {
                id: "held-message".into(),
                recipient: "human".into(),
                subject: "Unaccepted transport proposal".into(),
                text: "PRIVATE TRANSPORT OPERATION \"\\λ".into(),
            });
            if mode == "held-interface" {
                d.operations.push(p::Operation::Effect {
                    id: "held-interface".into(),
                    capability_id: "unavailable.fixture".into(),
                    binding_revision: "fixture-binding".into(),
                    inputs: Value::Null,
                    depends_on: vec![],
                });
            }
        }
        let inner = format!(" \n{}\n", serde_json::to_string(&d).unwrap());
        let outer = match mode {
            "malformed-inner" => {
                serde_json::to_string(&json!({"decision_json":"{\"version\":1}"})).unwrap()
            }
            "empty-inner" => serde_json::to_string(&json!({"decision_json":""})).unwrap(),
            "escaped-malformed-inner" => serde_json::to_string(&json!({
                "decision_json":"\0\"\\λ".repeat(128)
            }))
            .unwrap(),
            "wrong-carrier" => "{\"response\":\"{}\"}".into(),
            "duplicate-outer" => "{\"decision_json\":\"{}\",\"decision_json\":\"{}\"}".into(),
            "malformed-outer" => "{invalid final carrier".into(),
            _ => serde_json::to_string(&json!({"decision_json":inner})).unwrap(),
        };
        let behavior = match mode {
            "native" => TransportBehavior::Forbidden("function_call"),
            "native-result" => TransportBehavior::Forbidden("function_call_output"),
            "action" => TransportBehavior::Forbidden("computer_call"),
            "action-result" => TransportBehavior::Forbidden("computer_call_output"),
            "complete-stream-failure" => TransportBehavior::FailAfterComplete,
            "bounded-stream-failure" => TransportBehavior::BoundedStream,
            _ => TransportBehavior::Valid,
        };
        let (provider, transport) = transport_fixture(&f, &outer, behavior);
        let before = persisted_state(&f.app);
        let attempts_before = f.app.ledger().unwrap().ordinary.consumed.attempts;
        let request = PrepareRequest {
            id: "transport-call".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Direct,
            mode: Mode::Ordinary,
        };
        let result = f
            .app
            .step_with_provider(
                request.clone(),
                &provider,
                ai_personas::provider::Cancellation::default(),
                NOW,
            )
            .await;
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            f._temp.path().join("fixture-entered").exists(),
            "fixture body must actually run"
        );
        let captured_schema: Value =
            serde_json::from_slice(&std::fs::read(f._temp.path().join("captured-schema")).unwrap())
                .unwrap();
        assert_eq!(captured_schema, provider_schema());
        let prompt = std::fs::read_to_string(f._temp.path().join("captured-prompt")).unwrap();
        assert!(prompt.contains(&serde_json::to_string(&p::decision_schema()).unwrap()));
        let transport_failed = matches!(
            mode,
            "native"
                | "native-result"
                | "action"
                | "action-result"
                | "complete-stream-failure"
                | "bounded-stream-failure"
        );
        if mode == "correct" {
            let report = result.unwrap();
            assert_eq!(report.status, AdoptionStatus::Adopted);
            assert!(!report.synthetic);
            let adopted = f.app.adopted("transport-call").unwrap();
            assert_eq!(adopted.authored_json.as_deref(), Some(inner.as_str()));
            assert_eq!(adopted.response_hash, s::sha256_bytes(inner.as_bytes()));
        } else if transport_failed {
            assert!(matches!(result, Err(RuntimeError::Provider(_))));
        } else {
            let report = result.unwrap();
            assert_eq!(report.status, AdoptionStatus::Held);
            assert!(!report.synthetic);
            assert!(report.applied.is_empty());
            if matches!(mode, "wrong-id" | "wrong-epoch" | "held-interface") {
                assert!(report.reason.as_deref().unwrap().contains(match mode {
                    "wrong-id" => "response invocation identity mismatch",
                    "wrong-epoch" => "response epoch mismatch",
                    _ => "capability",
                }));
            }
        }
        let root = f.app.state_root().to_owned();
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        let terminal = f.app.terminal_evidence("transport-call").unwrap();
        assert!(!serde_json::to_string(&terminal)
            .unwrap()
            .contains("PRIVATE DISCARDED FIXTURE PAYLOAD"));
        if transport_failed {
            assert!(terminal.body.get("response_body").is_none());
            assert!(!serde_json::to_string(&terminal)
                .unwrap()
                .contains("PRIVATE TRANSPORT OPERATION"));
            let receipt = &terminal.body["receipt"];
            assert_eq!(receipt["kind"], "provider_attempt_failed");
            assert_eq!(receipt["dispatch_may_have_begun"], true);
            assert_eq!(receipt["remote_outcome"], "unknown");
            assert_eq!(receipt["replay_authorized"], false);
            assert_eq!(
                receipt["failure"],
                match mode {
                    "complete-stream-failure" => "network",
                    "bounded-stream-failure" => "limit",
                    _ => "remote_failure",
                }
            );
            let attempt = &receipt["attempt_receipt"];
            assert_eq!(attempt["dispatch_may_have_begun"], true);
            assert_eq!(attempt["http_status"], 200);
            assert_eq!(attempt["stream_eof"], false);
            assert_eq!(
                attempt["response_completed"],
                mode == "complete-stream-failure"
            );
            assert_eq!(attempt["remote_stop_confirmed"], false);
            assert_eq!(attempt["remote_effects_and_charges_may_remain"], true);
            assert!(attempt["stream_bytes_observed"].as_u64().unwrap() > 0);
            if mode == "bounded-stream-failure" {
                assert_eq!(attempt["stream_bytes_observed"], 128 * 1024);
            }
            let invocation = f.app.invocation("transport-call").unwrap();
            assert_eq!(invocation.state, s::InvocationState::Failed);
            assert!(invocation.response_hash.is_none());
            let admitted: Prepared = serde_json::from_value(invocation.request.unwrap()).unwrap();
            assert_eq!(attempt["audit"], admitted.provider_preparation.unwrap());
            let after = persisted_state(&f.app);
            for field in [
                "messages",
                "sources",
                "board",
                "observations",
                "semantic_warnings",
            ] {
                assert_eq!(before[field], after[field]);
            }
            assert!(f.app.adopted("transport-call").is_err());
            assert!(f.app.resume("operator", "a", "work").is_err());
            assert!(f
                .app
                .acknowledge_complete_rejection(
                    "operator",
                    "transport-call",
                    terminal.exact_ref().unwrap()
                )
                .is_err());
        } else {
            assert_eq!(terminal.body["receipt"]["kind"], "provider_complete");
            assert_eq!(terminal.body["receipt"]["synthetic"], false);
            assert_eq!(
                terminal.body["receipt"]["attempt"]["response_completed"],
                true
            );
            assert_eq!(terminal.body["receipt"]["attempt"]["stream_eof"], true);
            assert_eq!(
                terminal.body["receipt"]["usage"]["actual_cost_microusd"],
                Value::Null
            );
            assert_eq!(
                terminal.body["receipt"]["transport_sha256"],
                s::sha256_bytes(outer.as_bytes())
            );
            let expected_body = match mode {
                "malformed-inner" => "{\"version\":1}".to_owned(),
                "empty-inner" => String::new(),
                "escaped-malformed-inner" => "\0\"\\λ".repeat(128),
                "wrong-carrier" | "duplicate-outer" | "malformed-outer" => outer.clone(),
                _ => inner.clone(),
            };
            let retained = &terminal.body["response_body"];
            assert_eq!(
                retained["text"].as_str().unwrap().as_bytes(),
                expected_body.as_bytes()
            );
            assert_eq!(retained["bytes"], expected_body.len());
            assert_eq!(
                retained["sha256"],
                s::sha256_bytes(expected_body.as_bytes())
            );
            assert_eq!(
                retained["kind"],
                if matches!(
                    mode,
                    "wrong-carrier" | "duplicate-outer" | "malformed-outer"
                ) {
                    "provider_body"
                } else {
                    "decision_json"
                }
            );
            let invocation = f.app.invocation("transport-call").unwrap();
            assert_eq!(terminal.body["prepare_hash"], invocation.prepare_hash);
            assert_eq!(
                invocation.response_hash.as_deref(),
                Some(s::sha256_bytes(expected_body.as_bytes()).as_str())
            );
            let admitted: Prepared = serde_json::from_value(invocation.request.unwrap()).unwrap();
            assert_eq!(terminal.body["invocation_id"], admitted.request.id);
            assert_eq!(terminal.body["prepared_epoch"], admitted.epoch);
            assert_eq!(terminal.body["prompt_sha256"], admitted.prompt_sha256);
            let slot = diagnostic_slot(&f.app, "transport-call");
            assert_eq!(slot.0["max_object_bytes"], 263_168);
            assert!(serde_json::to_vec(&slot.0).unwrap().len() as u64 <= slot.1);
            if mode != "correct" {
                let after = persisted_state(&f.app);
                assert_eq!(before["messages"], after["messages"]);
                assert_eq!(before["sources"], after["sources"]);
                assert_eq!(before["board"], after["board"]);
                assert_eq!(before["observations"], after["observations"]);
                assert_eq!(before["semantic_warnings"], after["semantic_warnings"]);
                f.app.resume("operator", "a", "work").unwrap();
                assert_eq!(f.app.terminal_evidence("transport-call").unwrap(), terminal);
                let resumed = persisted_state(&f.app);
                let recovery = resumed["observations"]
                    .as_object()
                    .unwrap()
                    .values()
                    .find(|o| {
                        o["kind"] == "terminal_recovery" && o["invocation"] == "transport-call"
                    })
                    .unwrap();
                assert!(recovery["payload"]["terminal"]["response_body"]
                    .get("text")
                    .is_none());
                assert_eq!(
                    recovery["payload"]["terminal"]["response_body"]["availability"],
                    "private_terminal_only"
                );
            }
        }
        let serialized = serde_json::to_string(&terminal).unwrap();
        assert!(!serialized.contains("synthetic-key"));
        assert!(!serialized.contains("synthetic-account"));
        assert!(f.app.dispatch_synthetic("transport-call", NOW).is_err());
        assert_eq!(
            f.app.ledger().unwrap().ordinary.consumed.attempts,
            attempts_before + 1
        );
        let usage: s::ReservationRecord =
            adoption_accounting_record(&root, "reservation", "v:transport-call");
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert!(usage.measured.is_none());
        let ledger = f.app.ledger().unwrap();
        assert!(f
            .app
            .step_with_provider(
                request,
                &provider,
                ai_personas::provider::Cancellation::default(),
                NOW,
            )
            .await
            .is_err());
        assert_only_navigation_charged(ledger, f.app.ledger().unwrap());
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(f.app.terminal_evidence("transport-call").unwrap(), terminal);
        assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > 0);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn stale_caller_time_cannot_authorize_a_live_dispatch() {
    let trusted = unix_now().unwrap();
    let mut timed = restricted_policy();
    timed.expires_at_unix = Some(trusted - 1);
    let mut c = config();
    c.allow_direct_provider = true;
    c.policies.active_policies.push(timed.clone());
    let mut primary = seed("a");
    primary.model_confirmed = true;
    let mut original = need();
    original.policy_id = timed.id.clone();
    let f = Fixture::configured(true, c, primary, original);
    let (provider, transport) =
        transport_fixture(&f, "{\"decision_json\":\"{}\"}", TransportBehavior::Valid);
    let mut f = f;
    // Runtime rechecks exposure against unix_now(), independently of both the
    // stale admission argument and the injected provider credential clock.
    let result = f
        .app
        .step_with_provider(
            PrepareRequest {
                id: "expired-at-dispatch".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Direct,
                mode: Mode::Ordinary,
            },
            &provider,
            ai_personas::provider::Cancellation::default(),
            trusted - 10,
        )
        .await;
    let error = result.unwrap_err();
    assert!(!f._temp.path().join("fixture-entered").exists());
    assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let inv = f.app.invocation("expired-at-dispatch").unwrap();
    assert_eq!(inv.state, s::InvocationState::Failed);
    assert_eq!(inv.response_hash, None);
    assert!(inv
        .history
        .iter()
        .all(|receipt| receipt.status != s::ReceiptStatus::Dispatched));
    let prepared: Prepared = serde_json::from_value(inv.request.clone().unwrap()).unwrap();
    let terminal = f.app.terminal_evidence("expired-at-dispatch").unwrap();
    assert_eq!(terminal.body["invocation_id"], prepared.request.id);
    assert_eq!(terminal.body["prepared_epoch"], prepared.epoch);
    assert_eq!(terminal.body["prompt_sha256"], prepared.prompt_sha256);
    assert_eq!(
        terminal.body["receipt"]["kind"],
        "provider_prelaunch_rejection"
    );
    assert_eq!(terminal.body["receipt"]["stage"], "runtime_authorization");
    assert_eq!(terminal.body["receipt"]["launched"], false);
    assert_eq!(terminal.body["receipt"]["failure"], error.to_string());
    assert_eq!(terminal.body["receipt"]["cancellation"]["succeeded"], true);
    assert!(terminal.body["receipt"]["cancellation"]["failure"].is_null());
    let before = f.app.snapshot().unwrap();
    let attempts_left = before.work_meta["work"].attempts_left;
    assert_eq!(attempts_left, before.config.max_work_attempts - 1);
    let attempts = f.app.ledger().unwrap().ordinary.consumed.attempts;
    assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > 0);
    let root = f.app.state_root().to_owned();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    assert_eq!(
        f.app.terminal_evidence("expired-at-dispatch").unwrap(),
        terminal
    );
    assert_eq!(f.app.invocation("expired-at-dispatch").unwrap(), inv);
    f.app.resume("operator", "a", "work").unwrap();
    let resumed = f.app.snapshot().unwrap();
    assert!(resumed.personas["a"].active_invocation.is_none());
    assert!(resumed.personas["a"].focus["work"].permit);
    assert_eq!(resumed.work_meta["work"].attempts_left, attempts_left);
    assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
    assert!(resumed
        .observations
        .values()
        .any(|observation| observation.kind == "terminal_recovery"
            && observation.invocation.as_deref() == Some("expired-at-dispatch")
            && observation.payload["terminal"] == terminal.body));
    // Resume permits fresh admission, but cannot renew the expired policy.
    assert!(f
        .app
        .prepare(
            PrepareRequest {
                id: "fresh-after-prelaunch-rejection".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Direct,
                mode: Mode::Ordinary,
            },
            trusted
        )
        .is_err());
    assert_eq!(
        f.app.snapshot().unwrap().work_meta["work"].attempts_left,
        attempts_left
    );
    assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
    assert!(!f._temp.path().join("fixture-entered").exists());
}

fn repair_request(id: &str) -> PrepareRequest {
    PrepareRequest {
        id: id.into(),
        persona: "a".into(),
        work: "work".into(),
        profile: Profile::Synthetic,
        mode: Mode::SelectionRepair,
    }
}
fn repair_decision(prepared: &Prepared, preserve: bool) -> p::Decision {
    let mut d = decision(prepared, preserve);
    d.continuation = p::Continuation::Stop {
        reason: "Stop this restricted decision; any next call needs explicit admission.".into(),
    };
    d
}
fn repair_config(attempts: u64) -> Config {
    let mut c = config();
    c.max_repair_attempts = attempts;
    // Explicitly fund the deliberately conservative host projection envelope
    // from the existing finite work/global navigation limits.
    c.navigation_per_call.scanned_bytes = 768 * 1024 * 1024;
    c.navigation_per_call.inspected_revisions = 262_144;
    c.navigation_per_work.scanned_bytes = 6 * 1024 * 1024 * 1024;
    c.navigation_per_work.inspected_revisions = 2_097_152;
    let mut private = policy();
    private.id = "repair-private-policy".into();
    private.revision = "repair-private-policy-1".into();
    c.policies.active_policies.push(private);
    c
}
fn overflowed_repair_fixture(attempts: u64) -> (Fixture, f::FileRevision, f::FileRevision) {
    let mut f = Fixture::with_config(true, repair_config(attempts));
    f.accept();
    let large = record("repair-large", f::Scope::Assets, &"L".repeat(90_000));
    let small = record("repair-small", f::Scope::Assets, "SMALL EXACT REPAIR READ");
    let mut private = record(
        "repair-denied-identity",
        f::Scope::Assets,
        "DENIED REPAIR INVENTORY BODY",
    );
    private.policy.id = "repair-private-policy".into();
    private.policy.revision = "repair-private-policy-1".into();
    private = private.seal().unwrap();
    f.app
        .import_files(
            "operator",
            "a",
            vec![large.clone(), small.clone(), private.clone()],
            true,
            NOW,
        )
        .unwrap();
    let prepared = f.prepare("repair-select-initially-fitting");
    let mut d = decision(&prepared, true);
    let voice = f.app.snapshot().unwrap().sources["voice"].revisions[0]
        .record
        .clone();
    d.next_fragments = p::SelectionIntent::Replace {
        reason: "Also retain the independently mandatory exact voice.".into(),
        references: vec![reference(&voice)],
    };
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Choose these exact fitting optional entries.".into(),
        references: vec![reference(&large), reference(&small), reference(&private)],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let snapshot = f.app.snapshot().unwrap();
    let fit = snapshot.personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap();
    assert!(fit.ordinary_bytes <= snapshot.config.context_bytes);
    let growth = snapshot.config.context_bytes - fit.ordinary_bytes + 8192;
    for index in 0..growth.div_ceil(8192) {
        f.app
            .record_human_evidence(
                "operator",
                "work",
                &format!("mandatory-growth-{index}"),
                &"Observed mandatory evidence. ".repeat(300),
                Vec::new(),
                set(&["result"]),
            )
            .unwrap();
    }
    let before = f.app.snapshot().unwrap();
    let result = f.app.prepare(
        PrepareRequest {
            id: "ordinary-growth-block".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    assert!(
        matches!(result, Err(RuntimeError::SelectionOverflow { .. })),
        "{result:?}"
    );
    let after = f.app.snapshot().unwrap();
    assert!(after.personas["a"].focus["work"].repair_trigger.is_some());
    assert!(after.personas["a"].active_invocation.is_none());
    assert_eq!(
        before.personas["a"].owner_epoch,
        after.personas["a"].owner_epoch
    );
    assert_eq!(
        before.work_meta["work"].attempts_left,
        after.work_meta["work"].attempts_left
    );
    assert_eq!(after.work_meta["work"].repair_attempts_left, attempts);
    (f, small, private)
}

#[test]
fn later_mandatory_growth_repair_clear_requires_fresh_ordinary_admission() {
    let (mut f, _, _) = overflowed_repair_fixture(1);
    let before = f.app.snapshot().unwrap();
    let pending = before.personas["a"].focus["work"]
        .pending_observations
        .clone();
    let prepared = f.app.prepare(repair_request("repair-clear"), NOW).unwrap();
    assert!(prepared.prompt.contains("RESTRICTED SELECTION REPAIR ONLY"));
    assert!(!prepared.prompt.contains(&"L".repeat(1000)));
    let report = f.run(&prepared, &repair_decision(&prepared, false));
    assert_eq!(report.status, AdoptionStatus::Adopted);
    assert!(report.applied.is_empty());
    assert!(report.reason.unwrap().contains("Fresh ordinary admission"));
    let after = f.app.snapshot().unwrap();
    assert_eq!(
        after.work_meta["work"].attempts_left,
        before.work_meta["work"].attempts_left - 1
    );
    assert_eq!(after.work_meta["work"].repair_attempts_left, 0);
    assert_eq!(
        after.board.work(&w::WorkId("work".into())).unwrap().version,
        before
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .version
    );
    let focus = &after.personas["a"].focus["work"];
    assert!(!focus.permit);
    assert!(focus.assets.as_ref().unwrap().is_empty());
    assert!(pending
        .iter()
        .all(|id| focus.pending_observations.contains(id)));
    assert!(after.messages.is_empty());
    assert!(after.local_deliveries.is_empty());
    let ledger = f.app.ledger().unwrap();
    assert!(f
        .app
        .prepare(
            PrepareRequest {
                id: "no-automatic-task".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary
            },
            NOW
        )
        .is_err());
    assert_only_navigation_charged(ledger, f.app.ledger().unwrap());
    f.app.resume("operator", "a", "work").unwrap();
    let ordinary = f.prepare("fresh-after-repair");
    assert!(ordinary.prompt.contains("Observed mandatory evidence."));
    assert!(!ordinary.prompt.contains(&"L".repeat(1000)));
    assert!(ordinary.assembled_bytes_with_reserve <= after.config.context_bytes);
}

#[test]
fn repair_rejects_task_and_character_operations_before_any_read_or_selection_change() {
    for character in [false, true] {
        let (mut f, small, _) = overflowed_repair_fixture(1);
        let prepared = f
            .app
            .prepare(repair_request("forbidden-repair"), NOW)
            .unwrap();
        let mut d = repair_decision(&prepared, true);
        d.operations.push(p::Operation::Read {
            id: "early-read".into(),
            reference: reference(&small),
        });
        if character {
            let (ocean, vad) = profiles();
            d.operations.push(write(
                "voice-write",
                p::Scope::Fragments {},
                "forbidden-repair-voice",
                "Must not become character.",
                None,
            ));
            d.operations.push(p::Operation::CharacterAdopt {
                id: "character".into(),
                base_revision: prepared.character_revision.clone(),
                ocean,
                vad,
                own_voice: vec![proposed("forbidden-repair-voice", "voice-write")],
                reason: "This operation is prohibited in repair.".into(),
                evidence: vec![],
            });
            d.fragment_maintenance = p::Maintenance::Change {
                reason: "Prohibited character change.".into(),
                operation_ids: vec!["voice-write".into(), "character".into()],
            };
        } else {
            d.operations.push(p::Operation::Communicate {
                id: "task-action".into(),
                recipient: "b".into(),
                subject: "Prohibited task output".into(),
                text: "This must never be deposited.".into(),
            });
        }
        let before = persisted_state(&f.app);
        let report = f.run(&prepared, &d);
        assert_eq!(report.status, AdoptionStatus::Held);
        assert_eq!(before, persisted_state(&f.app));
        assert_eq!(
            f.app.snapshot().unwrap().work_meta["work"].repair_attempts_left,
            0
        );
        assert_eq!(
            f.app.invocation("forbidden-repair").unwrap().state,
            s::InvocationState::Failed
        );
        assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > 0);
    }
}

#[test]
fn repair_preserve_read_and_noop_spend_finite_allowance_across_restart() {
    let (mut f, small, _) = overflowed_repair_fixture(3);
    let baseline = f.app.snapshot().unwrap().work_meta["work"].attempts_left;
    let initial_uncertain = f.app.ledger().unwrap().ordinary.uncertain.output_bytes;
    for index in 0..3 {
        if index > 0 {
            let root = f.app.state_root().to_path_buf();
            drop(f.app);
            f.app = Runtime::open(&root).unwrap();
            f.app.resume("operator", "a", "work").unwrap();
        }
        let id = format!("bounded-repair-{index}");
        let prepared = f.app.prepare(repair_request(&id), NOW).unwrap();
        if index < 2 {
            assert!(!prepared.prompt.contains("SMALL EXACT REPAIR READ"));
        } else {
            assert!(prepared.prompt.contains("SMALL EXACT REPAIR READ"));
        }
        let mut d = repair_decision(&prepared, true);
        if index == 1 {
            d.operations.push(p::Operation::Read {
                id: "needed-exact-read".into(),
                reference: reference(&small),
            });
        }
        let report = f.run(&prepared, &d);
        assert_eq!(report.status, AdoptionStatus::Adopted);
        assert!(report.reason.unwrap().contains("remains oversized"));
        let state = f.app.snapshot().unwrap();
        assert_eq!(state.work_meta["work"].repair_attempts_left, 2 - index);
        assert_eq!(state.work_meta["work"].attempts_left, baseline - index - 1);
        assert!(!state.personas["a"].focus["work"].permit);
        assert_eq!(
            state.personas["a"].focus["work"]
                .assets
                .as_ref()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            f.app.invocation(&id).unwrap().state,
            s::InvocationState::Adopted
        );
    }
    let state = f.app.snapshot().unwrap();
    let matching: Vec<_> = state
        .observations
        .values()
        .filter(|observation| {
            observation.invocation.as_deref() == Some("bounded-repair-1")
                && observation.kind == "selection_repair_read"
        })
        .collect();
    assert_eq!(matching.len(), 1);
    let read_id = matching[0].id.as_str();
    assert!(state.observations[read_id].payload.get("result").is_some());
    assert!(state.personas["a"].focus["work"]
        .pending_observations
        .iter()
        .any(|id| id == read_id));
    assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > initial_uncertain);
    let root = f.app.state_root().to_path_buf();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let ledger = f.app.ledger().unwrap();
    let before = persisted_state(&f.app);
    assert!(matches!(
        f.app.prepare(repair_request("exhausted-repair"), NOW),
        Err(RuntimeError::Held(_))
    ));
    assert_eq!(before, persisted_state(&f.app));
    assert_only_navigation_charged(ledger, f.app.ledger().unwrap());
}

#[test]
fn repair_inventory_hides_denied_identities_and_reports_mandatory_supply() {
    let (mut f, _, private) = overflowed_repair_fixture(1);
    f.app
        .revoke_policy(
            "operator",
            private.policy.identity(),
            "Withdraw only this optional source policy.",
        )
        .unwrap();
    let prepared = f
        .app
        .prepare(repair_request("filtered-repair"), NOW)
        .unwrap();
    assert!(!prepared.prompt.contains("repair-denied-identity"));
    assert!(!prepared.prompt.contains("DENIED REPAIR INVENTORY BODY"));
    assert!(!prepared
        .repair_read_references
        .iter()
        .any(|r| r.source == private.source));
    let control: Value = serde_json::from_str(
        prepared
            .prompt
            .split("CONTROL\n")
            .nth(1)
            .unwrap()
            .split("\nQUALIFIED_CONTEXT\n")
            .next()
            .unwrap(),
    )
    .unwrap();
    let receipt = &control["selection_repair"];
    assert_eq!(receipt["inventory_is_guidance"], false);
    let voice = receipt["body_receipt"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["reference"]["source"] == "voice")
        .unwrap();
    assert_eq!(voice["status"], "supplied_mandatory");
    assert!(prepared
        .prompt
        .contains("I state what I observed and what I only infer."));
    assert_eq!(
        f.run(&prepared, &repair_decision(&prepared, false)).status,
        AdoptionStatus::Adopted
    );
}

#[test]
fn selection_repair_accounts_for_event_bodies_ranges_and_withholding() {
    let authority = |app: &mut Runtime| {
        let state = app.snapshot().unwrap();
        let work = state.board.work(&w::WorkId("work".into())).unwrap();
        WorkAuthority {
            work: "work".into(),
            version: work.version,
            authority: work.mandate().authority.clone(),
        }
    };
    let event_text = "EVENT_REPAIR_FULL_BODY λ: literal \\n and newline.\nEND";
    let qualification_text = "PREFIX EXACT EVENT QUALIFICATION SUFFIX";
    let required_text = "EXACT EVENT QUALIFICATION";
    for case in ["full", "range", "registered", "denied", "withheld"] {
        let configuration = repair_config(1);
        let event_policy = configuration
            .policies
            .active_policies
            .iter()
            .find(|policy| policy.id == "repair-private-policy")
            .unwrap()
            .clone();
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        let large = record("event-repair-large", f::Scope::Assets, &"L".repeat(90_000));
        let qualification = record(
            "event-repair-qualification",
            f::Scope::Assets,
            qualification_text,
        );
        let qualification_reference = qualification.reference(f::Representation::Utf8);
        let mut required = qualification_reference.clone();
        let start = qualification_text.find(required_text).unwrap();
        required.start = Some(start);
        required.end = Some(start + required_text.len());
        let mut event_source = record("event-repair-source", f::Scope::Assets, event_text);
        event_source.policy = event_policy.clone();
        event_source.qualifications.push(required.clone());
        event_source = event_source.seal().unwrap();
        let event_reference = event_source.reference(f::Representation::Utf8);
        fixture
            .app
            .import_files(
                "operator",
                "a",
                vec![large.clone(), qualification.clone(), event_source.clone()],
                true,
                NOW,
            )
            .unwrap();

        // Accept the optional choice before registration or delivery supplies
        // anything independently. The full qualification is selected separately
        // so an event supplying only its required range must report partial supply.
        let mut selected = reference(&event_source);
        if case == "range" {
            selected.start = Some(6);
            selected.end = Some(12);
        }
        let prepared = fixture.prepare("event-repair-select");
        let mut response = decision(&prepared, true);
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Retain these exact optional entries while they fit.".into(),
            references: vec![
                reference(&large),
                selected.clone(),
                reference(&qualification),
            ],
        };
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let accepted = fixture.app.snapshot().unwrap();
        let fit = accepted.personas["a"].focus["work"]
            .selection_fit
            .as_ref()
            .unwrap();
        assert!(fit.ordinary_bytes <= accepted.config.context_bytes);

        let binding = authority(&mut fixture.app);
        let dependency = fixture
            .app
            .register_dependency(
                "operator",
                RegisterDependencyRequest {
                    binding,
                    id: "event-repair-dependency".into(),
                    source: reference(&event_source),
                    participants: set(&["a"]),
                    accountability: w::Accountability::Owner(w::PersonaId("a".into())),
                    origin: LocalEventOrigin::LocalMock,
                },
                NOW,
            )
            .unwrap();
        if case == "denied" {
            fixture
                .app
                .revoke_policy("operator", event_policy.identity(), "Deny event delivery.")
                .unwrap();
        }
        let event_id = if case == "registered" {
            None
        } else {
            let binding = authority(&mut fixture.app);
            let result = fixture.app.deliver_local_event(
                "operator",
                DeliverLocalEventRequest {
                    binding,
                    id: "event-repair-delivery".into(),
                    dependency_id: dependency.id.clone(),
                    binding_revision: dependency.revision.clone(),
                    observation: dependency.source.clone(),
                    max_delivery_bytes: 32 * 1024,
                },
                NOW,
            );
            if case == "denied" {
                assert!(matches!(result, Err(RuntimeError::Held(_))));
                None
            } else {
                let receipt = result.unwrap();
                assert_eq!(receipt.delivered_to, set(&["a"]));
                assert!(!receipt.duplicate);
                assert!(!receipt.independent_authentication && !receipt.external_subscription);
                let state = fixture.app.snapshot().unwrap();
                Some(
                    state.work_meta["work"].local_dependencies[&dependency.id]
                        .event
                        .as_ref()
                        .unwrap()
                        .observations["a"]
                        .clone(),
                )
            }
        };
        if case == "withheld" {
            fixture
                .app
                .revoke_policy(
                    "operator",
                    event_policy.identity(),
                    "Withhold the retained event before repair admission.",
                )
                .unwrap();
        }

        let growth = accepted.config.context_bytes - fit.ordinary_bytes + 8192;
        for index in 0..growth.div_ceil(8192) {
            fixture
                .app
                .record_human_evidence(
                    "operator",
                    "work",
                    &format!("event-repair-growth-{index}"),
                    &"Observed mandatory evidence. ".repeat(300),
                    Vec::new(),
                    set(&["result"]),
                )
                .unwrap();
        }
        let before = fixture.app.snapshot().unwrap();
        assert_eq!(
            before.work_meta["work"].local_dependencies[&dependency.id]
                .event
                .is_some(),
            event_id.is_some()
        );
        let result = fixture.app.prepare(
            PrepareRequest {
                mode: Mode::Ordinary,
                ..repair_request("event-repair-overflow")
            },
            NOW,
        );
        assert!(
            matches!(result, Err(RuntimeError::SelectionOverflow { .. })),
            "{case}: {result:?}"
        );
        let overflowed = fixture.app.snapshot().unwrap();
        assert!(overflowed.personas["a"].focus["work"]
            .repair_trigger
            .is_some());
        assert!(overflowed.personas["a"].active_invocation.is_none());
        assert_eq!(
            overflowed.personas["a"].admission_serial,
            before.personas["a"].admission_serial
        );
        assert_eq!(
            overflowed.work_meta["work"].attempts_left,
            before.work_meta["work"].attempts_left
        );
        let prepared = fixture
            .app
            .prepare(repair_request("event-repair-restricted"), NOW)
            .unwrap();
        let core = control(&prepared);
        let repair = &core["selection_repair"];
        assert_eq!(core["mode"], "selection_repair");
        assert_eq!(repair["ordinary_fits"], false);
        assert!(prepared.assembled_bytes_with_reserve <= accepted.config.context_bytes);
        assert!(prepared.bundle_roots.as_ref().unwrap().assets.is_empty());
        assert!(!prepared.prompt.contains(&"L".repeat(1000)));
        assert!(!prepared.prompt.contains(qualification_text));

        let supplied = matches!(case, "full" | "range");
        let mandatory: BTreeSet<f::Reference> =
            serde_json::from_value(repair["mandatory_body_references"].clone()).unwrap();
        let mut expected = BTreeSet::from([accepted.sources["voice"].revisions[0]
            .record
            .reference(f::Representation::Utf8)]);
        if supplied {
            expected.insert(event_reference.clone());
            expected.insert(required.clone());
        }
        assert_eq!(mandatory, expected, "{case}");
        let bodies = repair["body_receipt"].as_array().unwrap();
        let qualification_receipt = bodies
            .iter()
            .find(|entry| entry["reference"] == json!(qualification_reference))
            .unwrap();
        assert_eq!(
            qualification_receipt["status"],
            if supplied {
                "partly_supplied_mandatory"
            } else {
                "withheld_optional_body"
            },
            "{case}"
        );
        assert_eq!(
            qualification_receipt["independently_supplied_ranges"],
            if supplied {
                json!([required])
            } else {
                json!([])
            }
        );
        if matches!(case, "denied" | "withheld") {
            assert!(!prepared.prompt.contains(&event_source.source));
            assert!(core["dependencies"].as_array().unwrap().is_empty());
            assert!(!prepared
                .repair_read_references
                .iter()
                .any(|reference| reference.source == event_source.source));
        } else {
            assert_eq!(
                core["dependencies"][0]["payload_supplied_by_registration"],
                false
            );
            assert!(repair["selection_inventory"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["selection"]["reference"] == json!(selected)));
            let root_receipt = bodies
                .iter()
                .find(|entry| entry["reference"]["source"] == event_source.source)
                .unwrap();
            assert_eq!(root_receipt["reference"]["start"], json!(selected.start));
            assert_eq!(root_receipt["reference"]["end"], json!(selected.end));
            assert_eq!(
                root_receipt["status"],
                if supplied {
                    "supplied_mandatory"
                } else {
                    "withheld_optional_body"
                }
            );
            assert_eq!(
                root_receipt["independently_supplied_ranges"],
                if supplied {
                    json!([event_reference])
                } else {
                    json!([])
                }
            );
        }
        let observations = core["actual_observations"].as_array().unwrap();
        let rendered_event = observations
            .iter()
            .find(|observation| observation["kind"] == "local_dependency_event");
        if supplied {
            let event = rendered_event.unwrap();
            assert_eq!(event["id"].as_str(), event_id.as_deref());
            assert!(prepared
                .supplied_observations
                .contains(event_id.as_ref().unwrap()));
            for bundle in [
                &event["payload"]["result"],
                &event["current_required_qualifications"][0],
            ] {
                let entries = bundle["entries"].as_array().unwrap();
                for (exact, text) in [(&event_reference, event_text), (&required, required_text)] {
                    assert!(entries.iter().any(|entry| {
                        rendered_body_reference(bundle, &entry["reference"]) == *exact
                            && entry["text"] == text
                    }));
                }
            }
        } else {
            assert!(rendered_event.is_none());
            assert!(!prepared.prompt.contains("EVENT_REPAIR_FULL_BODY"));
            assert!(!prepared.prompt.contains(required_text));
            if let Some(id) = &event_id {
                assert!(!prepared.supplied_observations.contains(id));
                assert!(observations.iter().any(|observation| {
                    observation["status"] == "withheld_unavailable"
                        && observation["body_supplied"] == false
                }));
            }
        }

        let report = fixture.run(&prepared, &repair_decision(&prepared, false));
        assert_eq!(report.status, AdoptionStatus::Adopted, "{case}: {report:?}");
        assert!(report.applied.is_empty());
        let after = fixture.app.snapshot().unwrap();
        assert_eq!(after.work_meta["work"].repair_attempts_left, 0);
        assert_eq!(
            after.work_meta["work"].attempts_left,
            before.work_meta["work"].attempts_left - 1
        );
        assert_eq!(
            after.board.work(&w::WorkId("work".into())).unwrap().version,
            before
                .board
                .work(&w::WorkId("work".into()))
                .unwrap()
                .version
        );
        let focus = &after.personas["a"].focus["work"];
        assert!(!focus.permit && !focus.ordinary_continuation);
        assert!(focus.assets.as_ref().unwrap().is_empty());
        assert!(before.personas["a"].focus["work"]
            .pending_observations
            .iter()
            .all(|id| focus.pending_observations.contains(id)));
    }
}

#[test]
fn repair_requires_declared_allowance_and_a_later_growth_overflow_witness() {
    let (mut f, _, _) = overflowed_repair_fixture(0);
    let ledger = f.app.ledger().unwrap();
    let before = persisted_state(&f.app);
    assert!(matches!(
        f.app.prepare(repair_request("disabled-repair"), NOW),
        Err(RuntimeError::Held(_))
    ));
    assert_eq!(before, persisted_state(&f.app));
    assert_only_navigation_charged(ledger, f.app.ledger().unwrap());
    let mut f = Fixture::with_config(true, repair_config(1));
    f.accept();
    let large = record("initially-invalid", f::Scope::Assets, &"X".repeat(180_000));
    f.app
        .import_files("operator", "a", vec![large.clone()], true, NOW)
        .unwrap();
    let prepared = f.prepare("invalid-initial-selection");
    let mut d = decision(&prepared, true);
    d.next_assets = p::SelectionIntent::Replace {
        reason: "This proposed selection does not fit initially.".into(),
        references: vec![reference(&large)],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Held);
    f.app.resume("operator", "a", "work").unwrap();
    let before = persisted_state(&f.app);
    let ledger = f.app.ledger().unwrap();
    assert!(matches!(
        f.app.prepare(repair_request("no-growth-witness"), NOW),
        Err(RuntimeError::Held(_))
    ));
    assert_eq!(before, persisted_state(&f.app));
    assert_only_navigation_charged(ledger, f.app.ledger().unwrap());
    assert_eq!(
        f.app.snapshot().unwrap().work_meta["work"].repair_attempts_left,
        1
    );
}

#[test]
fn repair_mandatory_overflow_and_changed_policy_do_not_create_dispatch_authority() {
    let (mut f, _, _) = overflowed_repair_fixture(1);
    for index in 0..20 {
        f.app
            .record_human_evidence(
                "operator",
                "work",
                &format!("too-much-mandatory-{index}"),
                &"M".repeat(16_000),
                Vec::new(),
                set(&["result"]),
            )
            .unwrap();
    }
    let before = f.app.snapshot().unwrap();
    assert!(f
        .app
        .prepare(repair_request("mandatory-core-block"), NOW)
        .is_err());
    let after = f.app.snapshot().unwrap();
    assert!(after.personas["a"].active_invocation.is_none());
    assert_eq!(
        before.work_meta["work"].attempts_left,
        after.work_meta["work"].attempts_left
    );
    assert_eq!(
        before.work_meta["work"].repair_attempts_left,
        after.work_meta["work"].repair_attempts_left
    );
    assert!(f.app.invocation("mandatory-core-block").is_err());
    let (mut f, small, _) = overflowed_repair_fixture(1);
    let prepared = f
        .app
        .prepare(repair_request("repair-policy-change"), NOW)
        .unwrap();
    f.app
        .withdraw_source("operator", &small.source, "Actual later withdrawal.")
        .unwrap();
    assert!(f.app.dispatch_synthetic(&prepared.request.id, NOW).is_err());
    assert_eq!(
        f.app.snapshot().unwrap().work_meta["work"].repair_attempts_left,
        0
    );
    assert_eq!(
        f.app.invocation(&prepared.request.id).unwrap().state,
        s::InvocationState::Prepared
    );
}

fn cli_command(root: &std::path::Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ai-personas"));
    command
        .arg("--state-root")
        .arg(root)
        .args(["--operator", "operator"])
        .kill_on_drop(true);
    command
}

async fn cli_output(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut command = cli_command(root);
    command.args(args);
    tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await
        .expect("CLI must finish without waiting for a live execution owner")
        .unwrap()
}

fn cli_success(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture_json(
    directory: &std::path::Path,
    name: &str,
    value: &impl serde::Serialize,
) -> std::path::PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

#[tokio::test(flavor = "current_thread")]
async fn cli_synthetic_submission_validates_before_dispatch_and_preserves_exact_bytes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = directory.path().join("state");
    let configuration = fixture_json(directory.path(), "config.json", &config());
    cli_success(cli_output(&root, &["init-state", configuration.to_str().unwrap()]).await);
    let (ocean, vad) = profiles();
    let imported = fixture_json(
        directory.path(),
        "persona.json",
        &PersonaImport {
            seed: seed("a"),
            ocean,
            vad,
            voice: record(
                "voice",
                f::Scope::Fragments,
                "I distinguish observations from inferences.",
            ),
        },
    );
    cli_success(cli_output(&root, &["import-persona", imported.to_str().unwrap()]).await);
    let original = fixture_json(directory.path(), "need.json", &need());
    cli_success(cli_output(&root, &["add-need", original.to_str().unwrap()]).await);
    let prepared: Prepared = serde_json::from_value(cli_success(
        cli_output(
            &root,
            &[
                "prepare",
                "--persona",
                "a",
                "--work",
                "work",
                "--invocation",
                "submission",
                "--profile",
                "synthetic",
            ],
        )
        .await,
    ))
    .unwrap();
    let mut d = decision(&prepared, false);
    d.operations.push(p::Operation::Responsibility {
        id: "offer-a".into(),
        work_id: "work".into(),
        terms_revision: control(&prepared)["own_pending_offers"][0]["terms_revision"]
            .as_str()
            .unwrap()
            .into(),
        disposition: "accept".into(),
        reason: "I accept the supplied exact terms.".into(),
    });
    let mut malformed = serde_json::to_value(&d).unwrap();
    malformed.as_object_mut().unwrap().remove("next_assets");
    let bad = fixture_json(directory.path(), "malformed.json", &malformed);
    let mut app = Runtime::open(&root).unwrap();
    let before = app.ledger().unwrap();
    let state_before = persisted_state(&app);
    assert!(!cli_output(
        &root,
        &["synthetic-submit", "submission", bad.to_str().unwrap()]
    )
    .await
    .status
    .success());
    assert!(!cli_output(&root, &["dispatch-synthetic", "submission"])
        .await
        .status
        .success());
    assert_eq!(app.ledger().unwrap(), before);
    assert_eq!(persisted_state(&app), state_before);
    assert_eq!(
        app.invocation("submission").unwrap().state,
        s::InvocationState::Prepared
    );

    let bytes = format!(" \n{}\n ", serde_json::to_string(&d).unwrap());
    let response = directory.path().join("response.json");
    std::fs::write(&response, bytes.as_bytes()).unwrap();
    let report = cli_success(
        cli_output(
            &root,
            &["synthetic-submit", "submission", response.to_str().unwrap()],
        )
        .await,
    );
    assert_eq!(report["status"], "adopted");
    assert_eq!(report["synthetic"], true);
    cli_success(cli_output(&root, &["status"]).await);
    let adopted = app.adopted("submission").unwrap();
    assert_eq!(adopted.authored_json.as_deref(), Some(bytes.as_str()));
    assert_eq!(adopted.response_hash, s::sha256_bytes(bytes.as_bytes()));
    assert_eq!(
        app.terminal_evidence("submission").unwrap().body["receipt"]["kind"],
        "synthetic_complete"
    );
    assert_eq!(app.epoch().unwrap(), prepared.epoch + 1);
    assert!(!cli_output(
        &root,
        &["synthetic-submit", "submission", response.to_str().unwrap()]
    )
    .await
    .status
    .success());
    assert_eq!(
        cli_success(
            cli_output(
                &root,
                &["adopt-response", "submission", response.to_str().unwrap()]
            )
            .await
        ),
        report
    );
    assert_eq!(app.ledger().unwrap().ordinary.consumed.attempts, 1);
}

#[cfg(target_os = "linux")]
struct FixtureGate(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl FixtureGate {
    fn release(&self) {
        std::fs::write(&self.0, b"release").unwrap();
    }
}

#[cfg(target_os = "linux")]
impl Drop for FixtureGate {
    fn drop(&mut self) {
        // Also release the finite local fixture if an assertion unwinds.
        let _ = std::fs::write(&self.0, b"release");
    }
}

#[cfg(target_os = "linux")]
fn gated_transport_fixture(
    f: &Fixture,
    response: &str,
) -> (direct_provider_cases::OwnerCall, FixtureGate) {
    let release = f._temp.path().join("release-fixture");
    let call = direct_provider_cases::OwnerCall {
        root: f.app.state_root().to_owned(),
        id: "owned-call".into(),
        response: response.into(),
        result: f._temp.path().join("owner-result.json"),
        release: release.clone(),
    };
    (call, FixtureGate(release))
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn injected_owner_step_allows_cli_status_fences_mutation_and_recovers_owner_death() {
    for disposition in ["adopt", "mutate", "kill"] {
        let mut f = live_fixture();
        let root = f.app.state_root().to_owned();
        let previous = f.app.invocation("accept").unwrap();
        let mut expected: Prepared = serde_json::from_value(previous.request.unwrap()).unwrap();
        expected.request.id = "owned-call".into();
        expected.epoch = f.app.epoch().unwrap() + 1;
        let inner = serde_json::to_string(&decision(&expected, true)).unwrap();
        let carrier = serde_json::to_string(&json!({"decision_json":inner})).unwrap();
        let (owner, gate) = gated_transport_fixture(&f, &carrier);
        let response_path = f._temp.path().join("response.json");
        std::fs::write(&response_path, inner.as_bytes()).unwrap();
        let attempts = f.app.ledger().unwrap().ordinary.consumed.attempts;
        let work_attempts = f.app.snapshot().unwrap().work_meta["work"].attempts_left;
        let mut child = direct_provider_cases::spawn_owner(&owner).await;
        let launches = f._temp.path().join("fixture-launches");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if std::fs::read_to_string(&launches).is_ok_and(|text| text == "launch\n") {
                    break;
                }
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "step exited before fixture launch"
                );
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("local subprocess fixture must actually launch");
        assert!(f._temp.path().join("fixture-entered").exists());
        assert_eq!(
            f.app.invocation("owned-call").unwrap().state,
            s::InvocationState::Dispatched
        );
        let epoch = f.app.epoch().unwrap();
        assert_eq!(epoch, expected.epoch);
        cli_success(cli_output(&root, &["status"]).await);
        assert_eq!(f.app.epoch().unwrap(), epoch);
        assert_eq!(
            f.app.invocation("owned-call").unwrap().state,
            s::InvocationState::Dispatched
        );

        // A response file is not evidence of this live provider's completion.
        assert!(!cli_output(
            &root,
            &[
                "adopt-response",
                "owned-call",
                response_path.to_str().unwrap()
            ]
        )
        .await
        .status
        .success());
        assert!(f.app.terminal_evidence("owned-call").is_err());
        assert_eq!(f.app.invocation("owned-call").unwrap().response_hash, None);
        let mut contender = owner.clone();
        contender.id = "contending-call".into();
        contender.result = f._temp.path().join("contender-result.json");
        let busy = direct_provider_cases::owner_output(&contender).await;
        assert!(!busy.status.success());
        assert!(String::from_utf8_lossy(&busy.stderr).contains("execution owner"));
        assert_eq!(f.app.epoch().unwrap(), epoch);
        assert_eq!(
            f.app.ledger().unwrap().ordinary.consumed.attempts,
            attempts + 1
        );

        if disposition == "kill" {
            child.kill().await.unwrap();
            child.wait().await.unwrap();
            // The release gate is still closed. Killing the actual process
            // holding Runtime's owner must release the OS lock and leave the
            // dispatched attempt uncertain, without inventing a completion.
            assert!(!owner.release.exists());
            assert!(!owner.result.exists());
            cli_success(cli_output(&root, &["status"]).await);
            let recovered = f.app.invocation("owned-call").unwrap();
            assert_eq!(recovered.state, s::InvocationState::Uncertain);
            assert_eq!(recovered.response_hash, None);
            assert!(f.app.terminal_evidence("owned-call").is_err());
            cli_success(cli_output(&root, &["status"]).await);
            assert_eq!(f.app.invocation("owned-call").unwrap(), recovered);
            assert!(!direct_provider_cases::owner_output(&owner)
                .await
                .status
                .success());
            assert!(!direct_provider_cases::owner_output(&contender)
                .await
                .status
                .success());
            assert!(!cli_output(
                &root,
                &[
                    "synthetic-submit",
                    "owned-call",
                    response_path.to_str().unwrap()
                ]
            )
            .await
            .status
            .success());
            gate.release();
        } else {
            if disposition == "mutate" {
                cli_success(
                    cli_output(&root, &["pause", "work", "Concurrent authority change."]).await,
                );
                assert_eq!(
                    f.app.invocation("owned-call").unwrap().state,
                    s::InvocationState::Dispatched
                );
            }
            gate.release();
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
                    .await
                    .expect("released fixture must reach terminal disposition")
                    .unwrap();
            let report = direct_provider_cases::owner_result(&owner, output);
            assert_eq!(report["synthetic"], false);
            if disposition == "mutate" {
                assert_eq!(report["status"], "held");
                assert!(report["reason"].as_str().unwrap().contains("epoch"));
                assert!(f.app.adopted("owned-call").is_err());
                assert_eq!(
                    f.app.invocation("owned-call").unwrap().state,
                    s::InvocationState::Failed
                );
            } else {
                assert_eq!(report["status"], "adopted");
                let accepted = f.app.adopted("owned-call").unwrap();
                assert_eq!(accepted.authored_json.as_deref(), Some(inner.as_str()));
                assert_eq!(accepted.response_hash, s::sha256_bytes(inner.as_bytes()));
            }
            assert_eq!(
                f.app.terminal_evidence("owned-call").unwrap().body["receipt"]["kind"],
                "provider_complete"
            );
        }
        assert_eq!(f.app.epoch().unwrap(), epoch + 1);
        assert_eq!(
            f.app.ledger().unwrap().ordinary.consumed.attempts,
            attempts + 1
        );
        assert!(f.app.ledger().unwrap().ordinary.uncertain.output_bytes > 0);
        assert_eq!(std::fs::read_to_string(&launches).unwrap(), "launch\n");
        assert_eq!(
            f.app.snapshot().unwrap().work_meta["work"].attempts_left,
            work_attempts - 1
        );
        let usage: s::ReservationRecord =
            adoption_accounting_record(&root, "reservation", "v:owned-call");
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert!(usage.measured.is_none());
        assert_eq!(
            f.app.ledger().unwrap().ordinary.reserved,
            s::Resources::default()
        );
        let metadata = std::fs::metadata(root.join("execution.lock")).unwrap();
        assert_eq!(metadata.len(), 0);
        assert_eq!(metadata.permissions().mode() & 0o077, 0);
        let mut store = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        drop(store.acquire_execution_owner().unwrap());
    }
}

fn interface_control(prepared: &Prepared) -> Value {
    let tail = prepared.prompt.split_once("\nCONTROL\n").unwrap().1;
    serde_json::from_str(tail.split_once("\nQUALIFIED_CONTEXT\n").unwrap().0).unwrap()
}

fn interface_peer_need(overlapping: bool) -> NeedInput {
    let mut original = need();
    let mut terms = original.invitations[0].terms.clone();
    if overlapping {
        terms.exact_terms = "Accept the explicitly offered exclusive outcome.".into();
    } else {
        terms.outcomes.clear();
        terms.exact_terms = "Contribute without exclusive outcome or review-role ownership.".into();
    }
    original.invitations.push(Invitation {
        id: "offer-b".into(),
        to: "b".into(),
        terms,
    });
    original
}

fn initialize_interface_peer(fixture: &mut Fixture) {
    let prepared = fixture
        .app
        .prepare(
            PrepareRequest {
                id: "interface-initialize-b".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let (ocean, vad) = profiles();
    let mut response = decision(&prepared, false);
    response.operations = vec![
        write(
            "interface-save-voice",
            p::Scope::Fragments {},
            "interface-voice-b",
            "I distinguish supplied bodies from metadata and attributed judgments.",
            None,
        ),
        p::Operation::CharacterAdopt {
            id: "interface-adopt-voice".into(),
            base_revision: None,
            ocean,
            vad,
            own_voice: vec![proposed("interface-voice-b", "interface-save-voice")],
            reason: "Initialize this synthetic participant's exact character.".into(),
            evidence: Vec::new(),
        },
    ];
    response.fragment_maintenance = p::Maintenance::Change {
        reason: "Account for both initialization operations.".into(),
        operation_ids: vec![
            "interface-save-voice".into(),
            "interface-adopt-voice".into(),
        ],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::CharacterOnly
    );
    fixture.app.resume("operator", "b", "work").unwrap();
}

#[test]
fn pending_offer_context_exposes_exclusive_conflicts_without_changing_acceptance() {
    for overlapping in [true, false] {
        let mut fixture =
            Fixture::configured(true, config(), seed("a"), interface_peer_need(overlapping));
        initialize_interface_peer(&mut fixture);
        fixture.accept();
        let prepared = fixture
            .app
            .prepare(
                PrepareRequest {
                    id: "interface-accept-b".into(),
                    persona: "b".into(),
                    work: "work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap();
        let core = interface_control(&prepared);
        assert_eq!(core["work_id"], "work");
        assert!(core["own_acceptance"].is_null());
        let offer = &core["own_pending_offers"][0];
        assert_eq!(offer["id"], "offer-b");
        assert_eq!(offer["mandate_current"], true);
        assert!(offer["handoff_from"].is_null());
        assert_eq!(
            offer["ownership_conflicts"],
            if overlapping {
                json!([{"persona":"a","acceptance_id":"offer-a",
                "outcomes":["result"],"review_roles":[]}])
            } else {
                json!([])
            }
        );
        let mut response = decision(&prepared, true);
        response.continuation = p::Continuation::Stop {
            reason: "Stop this bounded contribution while retaining any accepted responsibility."
                .into(),
        };
        response.operations.push(p::Operation::Responsibility {
            id: offer["id"].as_str().unwrap().into(),
            work_id: core["work_id"].as_str().unwrap().into(),
            terms_revision: offer["terms_revision"].as_str().unwrap().into(),
            disposition: "accept".into(),
            reason: "Explicitly accept exactly these supplied terms.".into(),
        });
        let before = persisted_state(&fixture.app);
        let report = fixture.run(&prepared, &response);
        let after = persisted_state(&fixture.app);
        if overlapping {
            assert_eq!(report.status, AdoptionStatus::Held);
            assert_eq!(
                report.reason.as_deref(),
                Some("conflicting immutable identity or ownership")
            );
            assert_eq!(after, before);
            assert_eq!(
                after["board"]["works"]["work"]["offers"]["offer-b"]["status"],
                "Pending"
            );
        } else {
            assert_eq!(report.status, AdoptionStatus::Adopted);
            assert_eq!(
                after["board"]["works"]["work"]["participants"]["b"]["acceptance"],
                "offer-b"
            );
            assert_eq!(
                after["board"]["works"]["work"]["acceptances"]["offer-a"],
                before["board"]["works"]["work"]["acceptances"]["offer-a"]
            );
        }
        assert_eq!(
            after["work_meta"]["work"]["attempts_left"],
            before["work_meta"]["work"]["attempts_left"]
        );
        assert_eq!(
            after["work_meta"]["work"]["attempts_left"],
            config().max_work_attempts - 3
        );
        assert!(after["board"]["works"]["work"]["artifacts"]
            .as_object()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn work_capability_schema_accepts_optional_fields_and_preserves_binding_rejection() {
    for fault in [
        "none",
        "base",
        "start",
        "end",
        "lifetime",
        "capability",
        "binding",
        "unknown_subject",
        "subject_array",
        "withdraw",
    ] {
        let mut fixture = Fixture::new(true);
        fixture.accept();
        let prepared = fixture.prepare("interface-register");
        let core = interface_control(&prepared);
        let schema = &core["work_capability"]["inputs_schema"];
        let put = schema["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variant| variant["properties"]["kind"]["const"] == "put_artifact")
            .unwrap();
        assert!(!put["required"].as_array().unwrap().contains(&json!("base")));
        for field in ["start", "end", "lifetime"] {
            assert!(!schema["$defs"]["ExactRef"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field)));
        }
        let body = "Exact abstract note.\nLiteral \\n and λ.";
        let mut response = decision(&prepared, true);
        response.asset_maintenance = p::Maintenance::Change {
            reason: "Retain the exact proposed supporting asset.".into(),
            operation_ids: vec!["interface-save".into()],
        };
        response.operations = vec![
            write(
                "interface-save",
                p::Scope::Assets {},
                "interface-source",
                body,
                None,
            ),
            effect(
                "interface-register",
                WorkAction::PutArtifact {
                    artifact: "interface-artifact".into(),
                    base: None,
                    subject: proposed("interface-source", "interface-save"),
                },
                &["interface-save"],
            ),
        ];
        if let p::Operation::Effect {
            capability_id,
            binding_revision,
            inputs,
            ..
        } = response.operations.last_mut().unwrap()
        {
            *capability_id = core["work_capability"]["id"].as_str().unwrap().into();
            *binding_revision = core["work_capability"]["binding_revision"]
                .as_str()
                .unwrap()
                .into();
            match fault {
                "none" | "withdraw" => (),
                "base" => {
                    inputs.as_object_mut().unwrap().remove("base");
                }
                field @ ("start" | "end" | "lifetime") => {
                    inputs["subject"].as_object_mut().unwrap().remove(field);
                }
                "capability" => *capability_id = "unexposed.command.v1".into(),
                "binding" => *binding_revision = "unexposed-binding".into(),
                "unknown_subject" => inputs["subject"]["PRIVATE_REJECTED_FIELD"] = json!(true),
                "subject_array" => {
                    inputs["subject"] = json!([
                        "interface-source",
                        "proposal-op:interface-save",
                        "utf8",
                        null,
                        null,
                        null
                    ])
                }
                _ => unreachable!(),
            }
        } else {
            unreachable!();
        }
        // The outer protocol deliberately leaves capability inputs opaque.
        p::parse_decision(
            &serde_json::to_vec(&response).unwrap(),
            config().response_bytes,
        )
        .unwrap();
        let before = persisted_state(&fixture.app);
        let report = fixture.run(&prepared, &response);
        if matches!(
            fault,
            "capability" | "binding" | "unknown_subject" | "subject_array"
        ) {
            assert_eq!(report.status, AdoptionStatus::Held, "{fault}: {report:?}");
            assert!(report.applied.is_empty());
            assert_eq!(persisted_state(&fixture.app), before);
            let reason = report.reason.unwrap();
            assert!(
                reason.contains(if matches!(fault, "capability" | "binding") {
                    "unknown, unimplemented, or stale capability"
                } else if fault == "subject_array" {
                    "requires an object"
                } else {
                    "unknown field"
                })
            );
            if fault == "unknown_subject" {
                assert!(reason.contains("PRIVATE_REJECTED_FIELD"));
                let terminal = fixture.app.terminal_evidence(&prepared.request.id).unwrap();
                assert!(terminal.body["response_body"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("PRIVATE_REJECTED_FIELD"));
                fixture.app.resume("operator", "a", "work").unwrap();
                let next = fixture.prepare("private-diagnostic-recovery");
                assert!(!next.prompt.contains("PRIVATE_REJECTED_FIELD"));
                assert!(!serde_json::to_string(&persisted_state(&fixture.app))
                    .unwrap()
                    .contains("PRIVATE_REJECTED_FIELD"));
            }
            continue;
        }
        assert_eq!(report.status, AdoptionStatus::Adopted);
        let state = fixture.app.snapshot().unwrap();
        let work = state.board.work(&w::WorkId("work".into())).unwrap();
        let artifact = &work.artifacts[&w::ArtifactId("interface-artifact".into())];
        let revision = &artifact.revisions[&artifact.current];
        assert_eq!(revision.bytes, body.as_bytes());
        let key = ArtifactKey {
            id: revision.reference.id.0.clone(),
            revision: revision.reference.revision,
            digest: hex::encode(revision.reference.digest),
        };
        let mapping_id = s::sha256_bytes(&serde_json::to_vec(&key).unwrap());
        let subject = &state.work_meta["work"].artifact_sources[&mapping_id];
        assert_eq!(subject.source, "interface-source");
        assert_eq!(subject.revision, state.sources["interface-source"].head);
        assert_eq!(
            state.sources["interface-source"].revisions[0].record.bytes,
            body.as_bytes()
        );
        if fault == "withdraw" {
            fixture
                .app
                .withdraw_source(
                    "operator",
                    "interface-source",
                    "Withdraw exact source access.",
                )
                .unwrap();
        }
        let next = fixture.prepare("interface-observe-binding");
        let next_core = interface_control(&next);
        if fault == "withdraw" {
            assert!(next_core["work_artifacts"].as_array().unwrap().is_empty());
            assert!(!next.prompt.contains("interface-source"));
            assert!(!next.prompt.contains("interface-artifact"));
            assert!(!next_core["actual_observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|observation| observation["kind"] == "accepted_artifact_binding"));
            continue;
        }
        let catalog = &next_core["work_artifacts"][0];
        assert_eq!(catalog["artifact"], json!(key));
        assert_eq!(catalog["source"], json!(subject));
        assert_eq!(catalog["owner"], "a");
        assert_eq!(catalog["source_owner"], "a");
        assert_eq!(catalog["metadata_only"], true);
        let receipt = next_core["actual_observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|observation| observation["kind"] == "accepted_artifact_binding")
            .unwrap();
        assert_eq!(receipt["payload"], json!({"artifact":key,"source":subject}));
        assert!(!next
            .sources
            .iter()
            .any(|reference| reference.source == "interface-source"));
        let context: Value =
            serde_json::from_str(next.prompt.split_once("\nQUALIFIED_CONTEXT\n").unwrap().1)
                .unwrap();
        assert!(context["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["reference"]["source"] != "interface-source"));
    }
}

#[test]
fn revoked_asset_policy_withholds_mapping_without_claiming_body_supply() {
    let mut configuration = config();
    let mut asset_policy = policy();
    asset_policy.id = "revocable-asset-policy".into();
    asset_policy.revision = "revocable-asset-policy-1".into();
    configuration
        .policies
        .active_policies
        .push(asset_policy.clone());
    let mut primary = seed("a");
    primary
        .grants
        .write_policies
        .insert(asset_policy.id.clone());
    let mut fixture = Fixture::configured(true, configuration, primary, need());
    fixture.accept();
    let prepared = fixture.prepare("register-revocable-asset");
    let mut save = write(
        "save-revocable",
        p::Scope::Assets {},
        "revocable-asset",
        "REVOKED_BODY_SENTINEL",
        None,
    );
    if let p::Operation::FileWrite { policy_id, .. } = &mut save {
        *policy_id = asset_policy.id.clone();
    }
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain an exact supporting asset under its own policy.".into(),
        operation_ids: vec!["save-revocable".into()],
    };
    response.operations = vec![
        save,
        effect(
            "register-revocable",
            WorkAction::PutArtifact {
                artifact: "revocable-artifact".into(),
                base: None,
                subject: proposed("revocable-asset", "save-revocable"),
            },
            &["save-revocable"],
        ),
    ];
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let before = fixture.app.snapshot().unwrap();
    let binding = before
        .observations
        .values()
        .find(|o| o.kind == "accepted_artifact_binding")
        .unwrap();
    assert!(binding.sources.is_empty());
    assert!(binding.qualified_roots.is_empty());
    assert!(binding
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .iter()
        .any(|r| r.source == "revocable-asset"));
    fixture
        .app
        .revoke_policy(
            "operator",
            asset_policy.identity(),
            "Withdraw asset processing permission.",
        )
        .unwrap();
    let next = fixture.prepare("observe-after-asset-revocation");
    let core = interface_control(&next);
    assert!(core["work_artifacts"].as_array().unwrap().is_empty());
    assert!(!core["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["kind"] == "accepted_artifact_binding"));
    assert!(!next.prompt.contains("\"revocable-asset\""));
    assert!(!next.prompt.contains("revocable-artifact"));
    assert!(!next.prompt.contains("REVOKED_BODY_SENTINEL"));
    assert!(!next.sources.iter().any(|r| r.source == "revocable-asset"));
    let after = fixture.app.snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(after.observations.get(&binding.id)).unwrap(),
        serde_json::to_value(Some(binding)).unwrap()
    );
}

#[test]
fn artifact_catalog_filters_private_sources_and_separates_file_and_artifact_owners() {
    let mut configuration = config();
    let mut private = policy();
    private.id = "interface-private-policy".into();
    private.revision = "interface-private-policy-1".into();
    private.readers = set(&["a"]);
    private.audience = set(&["a", "human"]);
    configuration.policies.active_policies.push(private.clone());
    let mut primary = seed("a");
    primary.audience = private.audience.clone();
    primary.grants.write_policies.insert(private.id.clone());
    let mut fixture = Fixture::configured(true, configuration, primary, interface_peer_need(false));
    initialize_interface_peer(&mut fixture);
    fixture.accept();
    let shared = record(
        "interface-shared-source",
        f::Scope::Assets,
        "INTERFACE_SHARED_BODY",
    );
    fixture
        .app
        .import_files("operator", "b", vec![shared.clone()], false, NOW)
        .unwrap();
    let prepared = fixture.prepare("interface-read-shared");
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Read {
        id: "interface-read".into(),
        reference: reference(&shared),
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("interface-publish");
    assert!(interface_control(&prepared)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|observation| observation["payload"]["result"]["entries"]
            .as_array()
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry["text"] == "INTERFACE_SHARED_BODY"))));
    let mut private_write = write(
        "interface-save-private",
        p::Scope::Assets {},
        "interface-private-source",
        "INTERFACE_PRIVATE_BODY",
        None,
    );
    if let p::Operation::FileWrite { policy_id, .. } = &mut private_write {
        *policy_id = private.id;
    }
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain one explicitly restricted supporting asset.".into(),
        operation_ids: vec!["interface-save-private".into()],
    };
    response.operations = vec![
        effect(
            "interface-register-shared",
            WorkAction::PutArtifact {
                artifact: "interface-shared-artifact".into(),
                base: None,
                subject: reference(&shared),
            },
            &[],
        ),
        private_write,
        effect(
            "interface-register-private",
            WorkAction::PutArtifact {
                artifact: "interface-private-artifact".into(),
                base: None,
                subject: proposed("interface-private-source", "interface-save-private"),
            },
            &["interface-save-private"],
        ),
    ];
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    assert_eq!(
        fixture
            .app
            .snapshot()
            .unwrap()
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .artifacts
            .len(),
        2
    );
    let peer = fixture
        .app
        .prepare(
            PrepareRequest {
                id: "interface-peer-catalog".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    for hidden in [
        "interface-private-source",
        "interface-private-artifact",
        "INTERFACE_PRIVATE_BODY",
    ] {
        assert!(!peer.prompt.contains(hidden));
    }
    assert!(!peer
        .sources
        .iter()
        .any(|reference| reference.source == shared.source));
    let core = interface_control(&peer);
    let catalog = core["work_artifacts"].as_array().unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0]["owner"], "a");
    assert_eq!(catalog[0]["source_owner"], "b");
    assert_eq!(catalog[0]["source_protected"], false);
    assert_eq!(catalog[0]["metadata_only"], true);
    let key: ArtifactKey = serde_json::from_value(catalog[0]["artifact"].clone()).unwrap();
    let subject: p::ExactRef = serde_json::from_value(catalog[0]["source"].clone()).unwrap();
    assert_eq!(subject, reference(&shared));
    let offer = &core["own_pending_offers"][0];
    let mut response = decision(&peer, true);
    response.operations = vec![
        p::Operation::Responsibility {
            id: offer["id"].as_str().unwrap().into(),
            work_id: core["work_id"].as_str().unwrap().into(),
            terms_revision: offer["terms_revision"].as_str().unwrap().into(),
            disposition: "accept".into(),
            reason: "Explicitly accept this distinct contribution scope.".into(),
        },
        p::Operation::Read {
            id: "interface-read".into(),
            reference: subject.clone(),
        },
    ];
    assert_eq!(
        fixture.run(&peer, &response).status,
        AdoptionStatus::Adopted
    );
    assert!(fixture.app.snapshot().unwrap().personas["b"].focus["work"]
        .assets
        .as_ref()
        .unwrap()
        .is_empty());
    let received = fixture
        .app
        .prepare(
            PrepareRequest {
                id: "interface-peer-received".into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    assert!(interface_control(&received)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|observation| observation["payload"]["result"]["entries"]
            .as_array()
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry["text"] == "INTERFACE_SHARED_BODY"))));
    let mut response = decision(&received, true);
    // Reusing an outer operation id is legal across invocations. Owning the
    // supporting file still does not authorize updating another actor's artifact.
    response.operations.push(effect(
        "interface-register-shared",
        WorkAction::PutArtifact {
            artifact: key.id.clone(),
            base: Some(key),
            subject,
        },
        &[],
    ));
    let before = persisted_state(&fixture.app);
    let report = fixture.run(&received, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert_eq!(
        report.reason.as_deref(),
        Some("context or decision lacks authority")
    );
    assert_eq!(persisted_state(&fixture.app), before);
}

#[test]
fn initialization_seed_is_not_resupplied_after_accepted_character_or_reopen() {
    let marker = "BOOTSTRAP_ONLY_SENTINEL_START_FROM_THIS_SEED";
    let mut primary = seed("a");
    primary.text = marker.into();
    let mut initial = Fixture::configured(false, config(), primary.clone(), need());
    let prepared = initial.prepare("bootstrap");
    assert!(prepared.prompt.contains(marker));
    assert!(prepared
        .prompt
        .contains("\"lifecycle_role\":\"initialization\""));
    let mut continuing = Fixture::configured(true, config(), primary, need());
    let original = continuing.app.snapshot().unwrap().personas["a"]
        .character
        .clone()
        .unwrap();
    let root = continuing._temp.path().join("state");
    drop(continuing.app);
    continuing.app = Runtime::open(&root).unwrap();
    let prepared = continuing.prepare("continue-accepted");
    assert!(!prepared.prompt.contains(marker));
    assert!(prepared
        .prompt
        .contains("\"lifecycle_role\":\"continuing_persona\""));
    assert!(prepared
        .prompt
        .contains("I state what I observed and what I only infer."));
    assert_eq!(
        prepared.character_revision.as_ref(),
        Some(&original.revision)
    );
    let retained = continuing.app.snapshot().unwrap();
    assert_eq!(retained.personas["a"].seed.text, marker);
    assert_eq!(
        retained.personas["a"].character.as_ref().unwrap().revision,
        original.revision
    );
}

#[test]
fn unaccepted_persona_can_stop_and_request_clarification_without_taking_responsibility() {
    let mut f = Fixture::new(true);
    let prepared = f.prepare("clarify-before-acceptance");
    let mut d = decision(&prepared, false);
    d.continuation = p::Continuation::Stop {
        reason: "Clarification is needed before accepting.".into(),
    };
    d.reply = p::ReplyDisposition::Authored {
        audience: vec!["human".into()],
        text: "Please clarify the requested outcome before I accept responsibility.".into(),
    };
    let report = f.run(&prepared, &d);
    assert_eq!(report.status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    let work = state.board.work(&w::WorkId("work".into())).unwrap();
    assert!(work.acceptance(&w::PersonaId("a".into())).is_none());
    assert!(matches!(
        work.offers[&w::RecordId("offer-a".into())].status,
        w::OfferStatus::Pending
    ));
    assert!(matches!(
        work.participants[&w::PersonaId("a".into())].continuation,
        Some(w::Continuation::Ended { .. })
    ));
    assert!(!state.personas["a"].focus["work"].permit);
    assert_eq!(state.messages.len(), 1);
    assert!(matches!(work.phase, w::Phase::Open));
}

// These faults target rejection of a complete response with a wrong epoch,
// independently of the successful decision-adoption transaction.
fn synthetic_owner_disposition_fault(app: &Runtime, settlement: bool) -> Connection {
    let condition = if settlement {
        "NEW.ns='reservation' AND NEW.k='v:synthetic-owner-recovery' AND json_extract(NEW.data,'$.state')='Uncertain'"
    } else {
        "NEW.ns='invocation' AND NEW.k='synthetic-owner-recovery' AND json_extract(NEW.data,'$.state')='Failed'"
    };
    let connection = Connection::open(app.state_root().join("state.sqlite3")).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER synthetic_owner_disposition_fault BEFORE UPDATE ON store_cells
         WHEN {condition}
         BEGIN SELECT RAISE(ABORT,'injected synthetic owner disposition failure'); END;"
        ))
        .unwrap();
    connection
}

fn assert_retained_synthetic_owner_after_recovery(
    app: &mut Runtime,
    host: &mut s::Store,
    id: &str,
) {
    assert!(matches!(
        app.dispatch_synthetic(id, NOW),
        Err(RuntimeError::Held(reason))
            if reason == "runtime already owns a synthetic dispatch"
    ));
    assert!(matches!(
        host.acquire_execution_owner(),
        Err(s::StoreError::InvalidState(reason))
            if reason == "an execution owner is already active"
    ));
}

#[test]
fn synthetic_owner_acknowledgement_allows_fresh_dispatch_and_submit_in_same_runtime() {
    for settlement in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("synthetic-owner-recovery");
        let id = prepared.request.id.as_str();
        let root = f.app.state_root().to_owned();
        let mut host =
            s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
        f.app.dispatch_synthetic(id, NOW).unwrap();
        let mut response = decision(&prepared, true);
        response.epoch += 1;
        let bytes = serde_json::to_vec(&response).unwrap();
        let hash = s::sha256_bytes(&bytes);
        let fault = synthetic_owner_disposition_fault(&f.app, settlement);
        let state_before = persisted_state(&f.app);
        let ledger_before = f.app.ledger().unwrap();
        let error = f.app.adopt_response(id, &bytes, NOW).unwrap_err();
        assert!(error
            .to_string()
            .contains("injected synthetic owner disposition failure"));
        assert_only_navigation_charged(ledger_before.clone(), f.app.ledger().unwrap());
        let completed = f.app.invocation(id).unwrap();
        assert_eq!(
            completed.state,
            if settlement {
                s::InvocationState::Failed
            } else {
                s::InvocationState::Responded
            }
        );
        assert_eq!(completed.response_hash.as_deref(), Some(hash.as_str()));
        let terminal = f.app.terminal_evidence(id).unwrap();
        let reference = terminal.exact_ref().unwrap();
        assert_eq!(terminal.body["receipt"]["inner_sha256"], hash);
        assert_eq!(
            completed
                .history
                .iter()
                .find(|receipt| receipt.status == s::ReceiptStatus::Responded)
                .unwrap()
                .evidence,
            Some(reference.clone())
        );
        assert_eq!(persisted_state(&f.app), state_before);
        assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);

        // Failed rejection and failed settlement must both retain ownership.
        let before = closeout_snapshot(&root);
        let error = f
            .app
            .acknowledge_complete_rejection("operator", id, reference.clone())
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("injected synthetic owner disposition failure"));
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);
        assert_eq!(f.app.invocation(id).unwrap(), completed);
        fault
            .execute_batch("DROP TRIGGER synthetic_owner_disposition_fault")
            .unwrap();

        // The settlement case enters acknowledgement's already-Failed branch
        // with a retained owner. Neither case drops or reopens this Runtime.
        f.app
            .acknowledge_complete_rejection("operator", id, reference.clone())
            .unwrap();
        drop(host.acquire_execution_owner().unwrap());
        let closed = f.app.invocation(id).unwrap();
        assert_eq!(closed.state, s::InvocationState::Failed);
        assert!(closed.history.starts_with(&completed.history));
        assert_eq!(
            closed.history.len(),
            completed.history.len() + usize::from(!settlement)
        );
        assert_eq!(closed.response_hash, completed.response_hash);
        let usage = host.reservation(&closed.reservation).unwrap();
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert_eq!(usage.measured, None);
        assert_eq!(persisted_state(&f.app), state_before);
        let before = closeout_snapshot(&root);
        f.app
            .acknowledge_complete_rejection("operator", id, reference)
            .unwrap();
        assert_resume_write_rollback(before, closeout_snapshot(&root));

        f.app.resume("operator", "a", "work").unwrap();
        let resumed = f.app.snapshot().unwrap();
        assert!(resumed.personas["a"].active_invocation.is_none());
        assert!(resumed.personas["a"].focus["work"].permit);
        assert_eq!(
            persisted_state(&f.app)["work_meta"],
            state_before["work_meta"]
        );
        let recoveries = resumed
            .observations
            .values()
            .filter(|observation| {
                observation.kind == "terminal_recovery"
                    && observation.invocation.as_deref() == Some(id)
            })
            .collect::<Vec<_>>();
        assert_eq!(recoveries.len(), 1);
        let mut diagnostic = terminal.body.clone();
        diagnostic["response_body"]
            .as_object_mut()
            .unwrap()
            .remove("text");
        diagnostic["response_body"]["availability"] = json!("private_terminal_only");
        assert_eq!(recoveries[0].payload["terminal"], diagnostic);
        assert_eq!(
            recoveries[0].payload["terminal_evidence"],
            json!(terminal.exact_ref().unwrap())
        );
        assert_eq!(f.app.terminal_evidence(id).unwrap(), terminal);
        assert_eq!(recoveries[0].payload["provider_usage"], "unknown");
        assert_eq!(recoveries[0].payload["replay_authorized"], false);
        let recovered_ledger = f.app.ledger().unwrap();
        assert_eq!(
            recovered_ledger.ordinary.consumed.attempts,
            ledger_before.ordinary.consumed.attempts
        );
        assert_eq!(
            recovered_ledger
                .ordinary
                .reserved
                .checked_add(recovered_ledger.ordinary.uncertain)
                .unwrap(),
            ledger_before
                .ordinary
                .reserved
                .checked_add(ledger_before.ordinary.uncertain)
                .unwrap()
        );
        assert!(f.app.adopted(id).is_err());

        let next = f.prepare("synthetic-owner-fresh-dispatch");
        assert_eq!(
            f.run(&next, &decision(&next, true)).status,
            AdoptionStatus::Adopted
        );
        let next = f.prepare("synthetic-owner-fresh-submit");
        let bytes = serde_json::to_vec(&decision(&next, true)).unwrap();
        assert_eq!(
            f.app
                .submit_synthetic(&next.request.id, &bytes, NOW)
                .unwrap()
                .status,
            AdoptionStatus::Adopted
        );
        assert_eq!(f.app.invocation(id).unwrap(), closed);
        assert_eq!(f.app.terminal_evidence(id).unwrap(), terminal);
        assert_eq!(host.reservation(&closed.reservation).unwrap(), usage);
        assert_eq!(
            f.app.ledger().unwrap().ordinary.consumed.attempts,
            ledger_before.ordinary.consumed.attempts + 2
        );
        assert_eq!(
            f.app.snapshot().unwrap().work_meta["work"].attempts_left,
            resumed.work_meta["work"].attempts_left - 2
        );
    }
}

#[test]
fn synthetic_owner_resume_cleanup_waits_for_durable_commit_in_same_runtime() {
    for already_failed in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("synthetic-owner-recovery");
        let id = prepared.request.id.as_str();
        let root = f.app.state_root().to_owned();
        let mut host =
            s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
        f.app.dispatch_synthetic(id, NOW).unwrap();
        let mut response = decision(&prepared, true);
        response.epoch += 1;
        let bytes = serde_json::to_vec(&response).unwrap();
        let fault = synthetic_owner_disposition_fault(&f.app, already_failed);
        let error = f.app.adopt_response(id, &bytes, NOW).unwrap_err();
        assert!(error
            .to_string()
            .contains("injected synthetic owner disposition failure"));
        let terminal = f.app.terminal_evidence(id).unwrap();
        let pending = f.app.invocation(id).unwrap();
        assert_eq!(
            pending.state,
            if already_failed {
                s::InvocationState::Failed
            } else {
                s::InvocationState::Responded
            }
        );
        fault
            .execute_batch("DROP TRIGGER synthetic_owner_disposition_fault")
            .unwrap();
        // No separate host settlement: explicit recovery must finish any
        // still-Reserved failed-attempt accounting in its own transaction.
        let usage_before = host.reservation(&pending.reservation).unwrap();
        let state_before = persisted_state(&f.app);
        let ledger_before = f.app.ledger().unwrap();
        fault
            .execute_batch(
                "CREATE TRIGGER synthetic_owner_resume_write_fault BEFORE UPDATE ON store_cells
             WHEN NEW.ns='object' AND NEW.k='runtime-state-v1'
             BEGIN SELECT RAISE(ABORT,'injected synthetic owner resume failure'); END;",
            )
            .unwrap();
        let before = closeout_snapshot(&root);
        let error = f.app.resume("operator", "a", "work").unwrap_err();
        assert!(error
            .to_string()
            .contains("injected synthetic owner resume failure"));
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);
        assert_eq!(f.app.invocation(id).unwrap(), pending);
        assert_eq!(
            host.reservation(&pending.reservation).unwrap(),
            usage_before
        );
        assert_eq!(persisted_state(&f.app), state_before);
        fault
            .execute_batch("DROP TRIGGER synthetic_owner_resume_write_fault")
            .unwrap();

        f.app.resume("operator", "a", "work").unwrap();
        drop(host.acquire_execution_owner().unwrap());
        let recovered = f.app.invocation(id).unwrap();
        assert_eq!(recovered.state, s::InvocationState::Failed);
        assert!(recovered.history.starts_with(&pending.history));
        assert_eq!(
            recovered.history.len(),
            pending.history.len() + usize::from(!already_failed)
        );
        assert_eq!(recovered.response_hash, pending.response_hash);
        assert_eq!(f.app.terminal_evidence(id).unwrap(), terminal);
        let usage = host.reservation(&recovered.reservation).unwrap();
        assert_eq!(usage.state, s::ReservationState::Uncertain);
        assert_eq!(usage.measured, None);
        let after = f.app.ledger().unwrap();
        assert_eq!(
            after.ordinary.consumed.attempts,
            ledger_before.ordinary.consumed.attempts
        );
        assert_eq!(
            after
                .ordinary
                .reserved
                .checked_add(after.ordinary.uncertain)
                .unwrap(),
            ledger_before
                .ordinary
                .reserved
                .checked_add(ledger_before.ordinary.uncertain)
                .unwrap()
        );
        let resumed = persisted_state(&f.app);
        assert!(resumed["personas"]["a"]["active_invocation"].is_null());
        assert_eq!(resumed["work_meta"], state_before["work_meta"]);
        assert_eq!(resumed["sources"], state_before["sources"]);
        assert_eq!(resumed["messages"], state_before["messages"]);
        assert_eq!(
            resumed["observations"]
                .as_object()
                .unwrap()
                .values()
                .filter(|observation| {
                    observation["kind"] == "terminal_recovery" && observation["invocation"] == id
                })
                .count(),
            usize::from(already_failed)
        );
        let next = f.prepare("synthetic-owner-after-resume");
        assert_eq!(
            f.run(&next, &decision(&next, true)).status,
            AdoptionStatus::Adopted
        );
        assert_eq!(f.app.invocation(id).unwrap(), recovered);
        assert_eq!(host.reservation(&recovered.reservation).unwrap(), usage);
    }
}

#[test]
fn synthetic_owner_recovery_preserves_unrelated_and_unresolved_ownership() {
    let mut f = Fixture::new(true);
    f.accept();
    let mut second = need();
    second.id = "owner-second".into();
    let terms = second.invitations[0].terms.clone();
    second.invitations.push(Invitation {
        id: "owner-offer-b".into(),
        to: "b".into(),
        terms,
    });
    f.app.add_need("operator", second).unwrap();
    let historical = f.prepare("synthetic-owner-history");
    let mut response = decision(&historical, true);
    response.epoch += 1;
    assert_eq!(f.run(&historical, &response).status, AdoptionStatus::Held);
    let historical_record = f.app.invocation(&historical.request.id).unwrap();
    let historical_terminal = f.app.terminal_evidence(&historical.request.id).unwrap();
    f.app.resume("operator", "a", "work").unwrap();

    let active = f.prepare("synthetic-owner-unresolved");
    let id = active.request.id.as_str();
    let root = f.app.state_root().to_owned();
    let mut host =
        s::Store::open_for_inspection(&root.join("state.sqlite3"), storage_limits()).unwrap();
    f.app.dispatch_synthetic(id, NOW).unwrap();
    let dispatched = f.app.invocation(id).unwrap();
    let before = closeout_snapshot(&root);
    f.app
        .acknowledge_complete_rejection(
            "operator",
            &historical.request.id,
            historical_terminal.exact_ref().unwrap(),
        )
        .unwrap();
    assert_resume_write_rollback(before, closeout_snapshot(&root));
    assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);
    assert_eq!(
        f.app.invocation(&historical.request.id).unwrap(),
        historical_record
    );

    // Keep the universal work/persona fence, including for an owned dispatch.
    let before = closeout_snapshot(&root);
    assert!(matches!(
        f.app.resume("operator", "a", "owner-second"),
        Err(RuntimeError::Held(reason))
            if reason == "recovery targets a different admitted work"
    ));
    assert_resume_write_rollback(before, closeout_snapshot(&root));
    assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);

    // A successful resume with no active invocation must not release a's owner.
    f.app.resume("operator", "b", "owner-second").unwrap();
    assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);
    let other = f
        .app
        .prepare(
            PrepareRequest {
                id: "synthetic-owner-other-persona".into(),
                persona: "b".into(),
                work: "owner-second".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    // Nor may successful terminal cleanup of a different invocation release it.
    f.app.resume("operator", "b", "owner-second").unwrap();
    assert_eq!(
        f.app.invocation(&other.request.id).unwrap().state,
        s::InvocationState::Failed
    );
    assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);
    assert_eq!(f.app.invocation(id).unwrap(), dispatched);

    for uncertain in [false, true] {
        if uncertain {
            host.mark_invocation_uncertain(id, "Fixture transport outcome remains unresolved")
                .unwrap();
        }
        let unresolved = f.app.invocation(id).unwrap();
        assert_eq!(
            unresolved.state,
            if uncertain {
                s::InvocationState::Uncertain
            } else {
                s::InvocationState::Dispatched
            }
        );
        assert_eq!(unresolved.response_hash, None);
        let before = closeout_snapshot(&root);
        assert!(matches!(
            f.app.resume("operator", "a", "work"),
            Err(RuntimeError::Held(reason))
                if reason.contains("unresolved transport")
        ));
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        assert_retained_synthetic_owner_after_recovery(&mut f.app, &mut host, id);
        assert_eq!(f.app.invocation(id).unwrap(), unresolved);
        assert!(f.app.terminal_evidence(id).is_err());
    }
    let usage = host.reservation(&dispatched.reservation).unwrap();
    assert_eq!(usage.state, s::ReservationState::Uncertain);
    assert_eq!(usage.measured, None);
    assert_eq!(
        f.app.terminal_evidence(&historical.request.id).unwrap(),
        historical_terminal
    );
}

#[test]
fn resume_cannot_abandon_another_work_prepared_or_responded_invocation() {
    for responded in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let mut second = need();
        second.id = "second".into();
        f.app.add_need("operator", second).unwrap();
        let prepared = f.prepare("work-specific-recovery");
        let root = f.app.state_root().to_owned();
        let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
        if responded {
            f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
            host.record_response(
                &prepared.request.id,
                &s::sha256_bytes(b"retained fixture response"),
            )
            .unwrap();
        }
        let before = closeout_snapshot(&root);
        let invocation = f.app.invocation(&prepared.request.id).unwrap();
        let result = f.app.resume("operator", "a", "second");
        assert!(
            result.is_err(),
            "unrelated work resume must not abandon an active decision"
        );
        assert_resume_write_rollback(before, closeout_snapshot(&root));
        assert_eq!(f.app.invocation(&prepared.request.id).unwrap(), invocation);
        assert_eq!(
            closeout_head(&root).body["personas"]["a"]["active_invocation"],
            prepared.request.id
        );
        // The same retained decision is still recoverable through its actual work.
        f.app.resume("operator", "a", "work").unwrap();
        assert_eq!(
            f.app.invocation(&prepared.request.id).unwrap().state,
            s::InvocationState::Failed
        );
        assert!(closeout_head(&root).body["personas"]["a"]["active_invocation"].is_null());
    }
}

#[test]
fn formal_assessment_requires_a_real_accepted_review_role() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("role-artifact");
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain an exact supporting asset.".into(),
        operation_ids: vec!["role-save".into()],
    };
    response.operations = vec![
        write(
            "role-save",
            p::Scope::Assets {},
            "role-source",
            "Bounded authored contribution.",
            None,
        ),
        effect(
            "role-register",
            WorkAction::PutArtifact {
                artifact: "role-artifact".into(),
                base: None,
                subject: proposed("role-source", "role-save"),
            },
            &["role-save"],
        ),
    ];
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("ungranted-review");
    let core = interface_control(&prepared);
    assert_eq!(core["mandate"]["review_roles"], json!({}));
    assert_eq!(
        core["own_acceptance"][1]["terms"]["review_roles"],
        json!([])
    );
    assert!(prepared
        .prompt
        .contains("Empty accepted review_roles means no formal assessment role"));
    assert!(prepared
        .prompt
        .contains("empty evidence cannot establish a passing assessment"));
    let artifact: ArtifactKey =
        serde_json::from_value(core["work_artifacts"][0]["artifact"].clone()).unwrap();
    let mut response = decision(&prepared, true);
    response.operations = vec![
        effect(
            "ungranted-assess",
            WorkAction::Assess {
                id: "ungranted-assessment".into(),
                artifact,
                outcome: "result".into(),
                role: "contributor".into(),
                evidence: BTreeSet::new(),
                passes: true,
                explanation: "An authored opinion is not an accepted reviewer role.".into(),
                limitations: vec!["No formal reviewer authority or bound evidence.".into()],
            },
            &[],
        ),
        p::Operation::Communicate {
            id: "ungranted-message".into(),
            recipient: "b".into(),
            subject: "Unaccepted assessment".into(),
            text: "This must not be deposited.".into(),
        },
    ];
    let before = persisted_state(&f.app);
    let report = f.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert_eq!(
        report.reason.as_deref(),
        Some("invalid accepted reviewer role")
    );
    assert!(report.applied.is_empty());
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn preserved_own_asset_does_not_supply_peer_body_until_explicit_read() {
    let mut f = Fixture::new(true);
    f.accept();
    let own = record("selection-own", f::Scope::Assets, "OWN_SELECTION_BODY");
    let peer = record("selection-peer", f::Scope::Assets, "PEER_SELECTION_BODY");
    f.app
        .import_files("operator", "a", vec![own.clone()], false, NOW)
        .unwrap();
    f.app
        .import_files("operator", "b", vec![peer.clone()], false, NOW)
        .unwrap();
    let prepared = f.prepare("select-own");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select this exact own asset.".into(),
        references: vec![reference(&own)],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("preserve-own");
    assert!(prepared.prompt.contains("OWN_SELECTION_BODY"));
    assert!(!prepared.prompt.contains("PEER_SELECTION_BODY"));
    assert!(prepared
        .prompt
        .contains("next_assets preserve keeps the previously selected exact asset references"));
    assert_eq!(
        prepared.bundle_roots.as_ref().unwrap().assets[0].source,
        "selection-own"
    );
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    let prepared = f.prepare("read-peer");
    assert!(!prepared.prompt.contains("PEER_SELECTION_BODY"));
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Read {
        id: "explicit-peer-read".into(),
        reference: reference(&peer),
    });
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("peer-result-supplied");
    assert!(prepared.prompt.contains("OWN_SELECTION_BODY"));
    assert!(prepared.prompt.contains("PEER_SELECTION_BODY"));
    assert_eq!(prepared.bundle_roots.as_ref().unwrap().assets.len(), 1);
    assert_eq!(
        prepared.bundle_roots.as_ref().unwrap().assets[0].source,
        "selection-own"
    );
    assert!(interface_control(&prepared)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["payload"]["result"]["entries"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|e| e["text"] == "PEER_SELECTION_BODY"))));
}

// These hand-authored synthetic choices exercise optional collaboration affordances.
// a and b make temporary task contributions, not permanent author/reviewer roles.
// No model runs here: exact prompt supply proves neither attention nor review quality,
// and this possible sequence is not a requirement that every task include peer review.
const PEER_ARTIFACT_FULL_BODY: &str = "# Observatory display label\nThe bronze model is illustrative.\nFULL_BODY_ONLY_SENTINEL: measurements remain provisional.\n";
const PEER_ARTIFACT_SUMMARY: &str = "Draft ready: observatory-display-label at proposal-op:save-display-label. Summary: the bronze model is illustrative.";

fn peer_artifact_prepare(fixture: &mut Fixture, id: &str) -> Prepared {
    fixture
        .app
        .prepare(
            PrepareRequest {
                id: id.into(),
                persona: "b".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap()
}

fn peer_artifact_catalog_fixture() -> (Fixture, Prepared, p::ExactRef) {
    let mut original = interface_peer_need(false);
    original.request = "Prepare an invented observatory display label; participants may choose how to contribute to this task.".into();
    original.outcomes.get_mut("result").unwrap().criterion =
        "Retain an exact display label and state the limits of any attributed judgment.".into();
    let mut fixture = Fixture::configured(true, config(), seed("a"), original);
    initialize_interface_peer(&mut fixture);
    fixture.accept();
    let authored = fixture.prepare("write-temporary-contribution");
    let mut response = decision(&authored, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain this temporary task contribution as an asset.".into(),
        operation_ids: vec!["save-display-label".into()],
    };
    response.operations = vec![
        write(
            "save-display-label",
            p::Scope::Assets {},
            "observatory-display-label",
            PEER_ARTIFACT_FULL_BODY,
            None,
        ),
        p::Operation::WorkCommand {
            id: "register-display-label".into(),
            inputs: serde_json::to_value(WorkAction::PutArtifact {
                artifact: "display-label-draft".into(),
                base: None,
                subject: proposed("observatory-display-label", "save-display-label"),
            })
            .unwrap(),
            depends_on: vec!["save-display-label".into()],
        },
        p::Operation::Communicate {
            id: "share-condensed-label".into(),
            recipient: "b".into(),
            subject: "Optional contribution to the display label".into(),
            text: PEER_ARTIFACT_SUMMARY.into(),
        },
    ];
    let report = fixture.run(&authored, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");

    let prepared = peer_artifact_prepare(&mut fixture, "peer-observe-catalog");
    let control = interface_control(&prepared);
    let catalog = control["work_artifacts"].as_array().unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0]["artifact"]["id"], "display-label-draft");
    assert_eq!(catalog[0]["artifact"]["revision"], 1);
    assert_eq!(
        catalog[0]["artifact"]["digest"],
        s::sha256_bytes(PEER_ARTIFACT_FULL_BODY.as_bytes())
    );
    assert_eq!(catalog[0]["metadata_only"], true);
    let exact: p::ExactRef = serde_json::from_value(catalog[0]["source"].clone()).unwrap();
    assert_eq!(exact.source, "observatory-display-label");
    assert!(!exact.revision.starts_with("proposal-"));
    assert_eq!(exact.representation, "utf8");
    assert_eq!((exact.start, exact.end), (None, None));
    let message = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "received_local_persona_message")
        .unwrap();
    assert_eq!(message["payload"]["sender"], "a");
    assert_eq!(message["payload"]["text"], PEER_ARTIFACT_SUMMARY);
    assert_eq!(message["payload"]["local_deposit_only"], true);
    assert!(!prepared.prompt.contains("FULL_BODY_ONLY_SENTINEL"));
    assert!(!prepared.sources.iter().any(|r| r.source == exact.source));
    assert!(prepared.bundle_roots.as_ref().unwrap().assets.is_empty());
    (fixture, prepared, exact)
}

fn peer_artifact_optional_response(prepared: &Prepared) -> p::Decision {
    let control = interface_control(prepared);
    let offer = &control["own_pending_offers"][0];
    let mut response = decision(prepared, true);
    response.operations.push(p::Operation::Responsibility {
        id: offer["id"].as_str().unwrap().into(),
        work_id: control["work_id"].as_str().unwrap().into(),
        terms_revision: offer["terms_revision"].as_str().unwrap().into(),
        disposition: "accept".into(),
        reason: "For this synthetic branch I choose the offered nonexclusive contribution.".into(),
    });
    response
}

fn assert_peer_artifact_exact_body(entry: &Value, exact: &p::ExactRef) {
    let supplied: p::ExactRef = serde_json::from_value(entry["reference"].clone()).unwrap();
    assert_eq!(supplied.source, exact.source);
    assert_eq!(supplied.revision, exact.revision);
    assert_eq!(supplied.representation, exact.representation);
    assert_eq!((supplied.start, supplied.end), (None, None));
    assert_eq!(
        entry["text"].as_str().unwrap().as_bytes(),
        PEER_ARTIFACT_FULL_BODY.as_bytes()
    );
}

#[test]
fn peer_artifact_affordance_explicit_read_supplies_full_body_on_fresh_request() {
    let (mut fixture, prepared, exact) = peer_artifact_catalog_fixture();
    let mut response = peer_artifact_optional_response(&prepared);
    response.operations.push(p::Operation::Read {
        id: "choose-exact-label-read".into(),
        reference: exact.clone(),
    });
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let fresh = peer_artifact_prepare(&mut fixture, "peer-observe-exact-read");
    let control = interface_control(&fresh);
    let observation = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "qualified_read")
        .unwrap();
    let entry = observation["payload"]["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["reference"]["source"] == exact.source)
        .unwrap();
    assert_peer_artifact_exact_body(entry, &exact);
    assert!(fresh.prompt.contains("FULL_BODY_ONLY_SENTINEL"));
    assert!(fresh
        .sources
        .iter()
        .any(|r| r.source == exact.source && r.revision == exact.revision));
    assert!(fresh.bundle_roots.as_ref().unwrap().assets.is_empty());
    assert!(fixture.app.snapshot().unwrap().work_meta["work"]
        .model_evidence
        .is_empty());
}

#[test]
fn peer_artifact_affordance_explicit_selection_supplies_full_body_on_fresh_request() {
    let (mut fixture, prepared, exact) = peer_artifact_catalog_fixture();
    let mut response = peer_artifact_optional_response(&prepared);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "For this synthetic branch I choose the exact catalog source for my next request."
            .into(),
        references: vec![exact.clone()],
    };
    let report = fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let fresh = peer_artifact_prepare(&mut fixture, "peer-observe-exact-selection");
    let bundle: Value =
        serde_json::from_str(fresh.prompt.split_once("\nQUALIFIED_CONTEXT\n").unwrap().1).unwrap();
    let entry = bundle["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["reference"]["source"] == exact.source)
        .unwrap();
    assert_peer_artifact_exact_body(entry, &exact);
    let selected = &fresh.bundle_roots.as_ref().unwrap().assets;
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].source, exact.source);
    assert_eq!(selected[0].revision, exact.revision);
    assert!(fixture.app.snapshot().unwrap().work_meta["work"]
        .model_evidence
        .is_empty());
}

#[test]
fn peer_artifact_affordance_prior_message_alias_is_not_a_cross_request_binding() {
    let (mut fixture, prepared, mut exact) = peer_artifact_catalog_fixture();
    exact.revision = "proposal-op:save-display-label".into();
    let mut response = peer_artifact_optional_response(&prepared);
    response.operations.extend([
        p::Operation::Read {
            id: "try-prior-message-alias".into(),
            reference: exact,
        },
        p::Operation::Communicate {
            id: "must-not-deposit-after-invalid-alias".into(),
            recipient: "a".into(),
            subject: "This invalid branch must adopt nothing".into(),
            text: "No body-supply or review claim may be deposited from this invalid response."
                .into(),
        },
    ]);
    fixture
        .app
        .dispatch_synthetic(&prepared.request.id, NOW)
        .unwrap();
    let before = persisted_state(&fixture.app);
    // This malformed alias is rejected before adoption, not returned as Held.
    let error = fixture
        .app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&response).unwrap(),
            NOW,
        )
        .unwrap_err();
    assert!(matches!(&error, RuntimeError::Protocol(_)), "{error:?}");
    assert!(error
        .to_string()
        .contains("derived reference has no exact file proposal in this response"));
    assert_eq!(persisted_state(&fixture.app), before);
}

#[test]
fn imported_source_identifiers_follow_decision_reference_rules() {
    let mut f = Fixture::new(true);
    for invalid_source in ["peer draft".to_owned(), "x".repeat(257)] {
        let before = persisted_state(&f.app);
        let imported = record(
            &invalid_source,
            f::Scope::Assets,
            "Unreachable source fixture.",
        );
        assert!(f
            .app
            .import_files("operator", "b", vec![imported], false, NOW)
            .is_err());
        assert_eq!(persisted_state(&f.app), before);
    }
    let source = "s".repeat(256);
    let imported = record(&source, f::Scope::Assets, "REACHABLE_BOUNDARY_ASSET");
    f.app
        .import_files("operator", "b", vec![imported.clone()], false, NOW)
        .unwrap();
    f.accept();
    let prepared = f.prepare("select-boundary-source");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Inspect the exact permitted imported asset.".into(),
        references: vec![reference(&imported)],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("boundary-body-supplied");
    assert!(prepared.prompt.contains("REACHABLE_BOUNDARY_ASSET"));
}

#[test]
fn artifact_identifiers_round_trip_through_registration_and_update() {
    for length in [129, 256] {
        let mut f = Fixture::new(true);
        f.accept();
        let asset = record("identifier-source", f::Scope::Assets, "Exact asset body.");
        f.app
            .import_files("operator", "a", vec![asset.clone()], false, NOW)
            .unwrap();
        let artifact = "a".repeat(length);
        let prepared = f.prepare("register-long-artifact");
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "register",
            WorkAction::PutArtifact {
                artifact: artifact.clone(),
                base: None,
                subject: reference(&asset),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let prepared = f.prepare("update-long-artifact");
        let key: ArtifactKey = serde_json::from_value(
            interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
        )
        .unwrap();
        assert_eq!(key.id, artifact);
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "update",
            WorkAction::PutArtifact {
                artifact: artifact.clone(),
                base: Some(key.clone()),
                subject: reference(&asset),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let prepared = f.prepare("updated-long-artifact");
        assert_eq!(
            interface_control(&prepared)["work_artifacts"][0]["artifact"]["revision"],
            key.revision + 1
        );
    }
}

#[test]
fn accepted_selection_fit_records_instruction_and_schema_measurement() {
    let mut f = Fixture::new(true);
    f.accept();
    let before = f.app.snapshot().unwrap();
    let fit = before.personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap();
    let measured = fit.fixed_contract_bytes.unwrap();
    assert!(measured > serde_json::to_vec(&p::decision_schema()).unwrap().len());
    let prepared = f.prepare("unchanged-fixed-contract");
    assert_eq!(
        f.run(&prepared, &decision(&prepared, true)).status,
        AdoptionStatus::Adopted
    );
    let after = f.app.snapshot().unwrap();
    let fit = after.personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap();
    assert_eq!(fit.fixed_contract_bytes, Some(measured));
    assert!(fit.ordinary_bytes <= after.config.context_bytes);
}

const MODEL_NOTE: &str = "Option A favors a small first draft. Option B favors a fuller outline. Cost is unknown. No external verification is claimed.";
fn model_evidence_fixture(
    selection: &str,
    outside_review: bool,
) -> (Fixture, Prepared, ArtifactKey) {
    model_evidence_fixture_with_peer(selection, outside_review, false)
}
fn model_evidence_fixture_with_peer(
    selection: &str,
    outside_review: bool,
    peer: bool,
) -> (Fixture, Prepared, ArtifactKey) {
    model_evidence_fixture_with_stakeholder(selection, outside_review, peer, false)
}
fn model_evidence_fixture_with_stakeholder(
    selection: &str,
    outside_review: bool,
    peer: bool,
    stakeholder: bool,
) -> (Fixture, Prepared, ArtifactKey) {
    let mut original = need();
    if stakeholder {
        original.stakeholders.insert("human".into());
    }
    original.request =
        "Write a brief comparison of two ordinary planning options, stating unknown costs.".into();
    original.outcomes.get_mut("result").unwrap().criterion =
        "The exact note compares both options, states cost is unknown, and makes no outside verification claim. An explained judgment of the actual note suffices.".into();
    if outside_review {
        original.review_roles.insert(
            "outside".into(),
            RoleSpec {
                terms: "Qualified outside assessment is explicitly required for this fixture."
                    .into(),
                funding: "Existing finite fixture allowance.".into(),
                policy: "No self-assessment substitute.".into(),
                independence: w::Independence::QualifiedOutside,
            },
        );
        original
            .outcomes
            .get_mut("result")
            .unwrap()
            .required_reviews
            .insert("outside".into());
        original.invitations[0]
            .terms
            .review_roles
            .insert(w::RoleId("outside".into()));
    }
    if peer {
        let mut terms = original.invitations[0].terms.clone();
        terms.outcomes.clear();
        terms.review_roles.clear();
        terms.exact_terms = "Optional contribution without exclusive outcome ownership.".into();
        original.invitations.push(Invitation {
            id: "offer-b".into(),
            to: "b".into(),
            terms,
        });
    }
    let mut f = Fixture::configured(true, config(), seed("a"), original);
    f.accept();
    let prepared = f.prepare("author-model-note");
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain the exact authored comparison as a supporting asset.".into(),
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
    if selection != "none" {
        let mut selected = proposed("model-note", "save-note");
        if selection == "partial" {
            selected.start = Some(0);
            selected.end = Some(6);
        }
        response.next_assets = p::SelectionIntent::Replace {
            reason: "Explicitly choose the exact authored note for the next call.".into(),
            references: vec![selected],
        };
    }
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("inspect-model-note");
    let key: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    (f, prepared, key)
}
fn model_evidence_operation(key: &ArtifactKey) -> p::Operation {
    effect("record-note-judgment", WorkAction::RecordModelEvidence {
        id: "model-note-evidence".into(), artifact: key.clone(), outcome: "result".into(),
        explanation: "I read both named options and the explicit unknown-cost and no-verification statements in the supplied note.".into(),
        limitations: vec!["This is a model judgment of a text, not execution, measurement, human acceptance or independent assurance.".into()],
    }, &[])
}
fn model_judgment_and_shared(key: &ArtifactKey) -> Vec<p::Operation> {
    let artifacts = BTreeMap::from([("result".into(), key.clone())]);
    vec![
        effect(
            "judge-note",
            WorkAction::Judge {
                id: "model-note-judgment".into(),
                artifacts: artifacts.clone(),
                sufficient: true,
                claims: BTreeMap::from([(
                    "result".into(),
                    ClaimInput {
                        met: true,
                        explanation: "The actual note satisfies this bounded writing criterion."
                            .into(),
                        evidence: set(&["model-note-evidence"]),
                    },
                )]),
                explanation: "Attributed sufficiency judgment under a writing-only criterion."
                    .into(),
            },
            &[],
        ),
        effect(
            "agree-note",
            WorkAction::DecideShared {
                id: "model-note-shared".into(),
                artifacts,
            },
            &[],
        ),
    ]
}

#[test]
fn ordinary_model_evidence_can_complete_an_exact_synthetic_writing_task_without_human_testimony() {
    let (mut f, prepared, key) = model_evidence_fixture("none", false);
    assert!(!prepared.prompt.contains(MODEL_NOTE));
    let source: p::ExactRef =
        serde_json::from_value(interface_control(&prepared)["work_artifacts"][0]["source"].clone())
            .unwrap();
    let mut read = decision(&prepared, true);
    read.operations.push(p::Operation::Read {
        id: "read-note".into(),
        reference: source,
    });
    assert_eq!(f.run(&prepared, &read).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("record-and-judge-note");
    assert!(prepared.prompt.contains(MODEL_NOTE));
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
    let result = f.run(&prepared, &response);
    assert_eq!(
        result.status,
        AdoptionStatus::Adopted,
        "{:?}",
        result.reason
    );
    let state = f.app.snapshot().unwrap();
    assert!(state.work_meta["work"].human_evidence.is_empty());
    let work = state.board.work(&w::WorkId("work".into())).unwrap();
    let evidence = &work.evidence[&w::RecordId("model-note-evidence".into())];
    assert_eq!(evidence.source, w::SourceKind::ModelJudgment);
    assert_eq!(evidence.artifacts.len(), 1);
    assert_eq!(evidence.criteria.len(), 1);
    let record = &state.work_meta["work"].model_evidence["model-note-evidence"];
    assert_eq!(record["judgment"]["persona"], "a");
    assert_eq!(record["judgment"]["synthetic"], true);
    assert_eq!(record["judgment"]["operation"], "record-note-judgment");
    assert!(
        record["judgment"]["qualified_subject"]["qualified_bundle_sha256"]
            .as_str()
            .is_some()
    );
    assert!(record["judgment"]["qualified_subject"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["reference"]["source"] == "model-note"
            && entry["body_sha256"] == s::sha256_bytes(MODEL_NOTE.as_bytes())));
    assert_eq!(record["judgment"]["invocation"], prepared.request.id);
    assert_eq!(record["judgment"]["prompt_sha256"], prepared.prompt_sha256);
    assert_eq!(
        record["judgment"]["semantic_truth_verified_by_runtime"],
        false
    );
    assert_eq!(record["judgment"]["independent_assurance"], false);
    assert_eq!(record["judgment"]["execution_or_measurement"], false);
    assert_eq!(
        hex::encode(evidence.exact.digest),
        s::sha256_bytes(&serde_json::to_vec(&record["judgment"]).unwrap())
    );
    assert!(!state.board.completion_supported(&w::WorkId("work".into())));

    let prepared = f.prepare("deliver-model-note");
    let supplied = interface_control(&prepared);
    let release_receipt = supplied["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["kind"] == "accepted_work_release")
        .expect("a fresh decision must receive the exact persisted release identity");
    let exposed_release = release_receipt["payload"]["release"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(release_receipt["payload"]["metadata_only"], true);
    assert_eq!(
        supplied["closure_readiness"]["records"][0]["release"],
        exposed_release
    );
    assert_eq!(
        supplied["closure_readiness"]["records"][0]["unestablished_deliveries"],
        json!(["human"])
    );
    assert_eq!(
        supplied["closure_readiness"]["records"][0]["permitted_prerequisites_observed"],
        false
    );
    assert_eq!(
        release_receipt["payload"]["record"],
        serde_json::to_value(&work.releases[&w::RecordId(exposed_release.clone())]).unwrap()
    );
    assert_eq!(
        interface_control(&prepared)["model_evidence"]["model-note-evidence"]["record"]["source"],
        "ModelJudgment"
    );
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "deliver-note",
        WorkAction::DeliverRelease {
            release: exposed_release.clone(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let state = persisted_state(&f.app);
    let delivery = state["local_deliveries"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(delivery["external_export"], false);
    let bytes: Vec<u8> = serde_json::from_value(delivery["artifacts"][0]["bytes"].clone()).unwrap();
    assert_eq!(bytes, MODEL_NOTE.as_bytes());
    let prepared = f.prepare("close-model-note");
    let supplied = interface_control(&prepared);
    assert_eq!(supplied["mandate"]["stakeholders"], json!([]));
    assert_eq!(
        supplied["closure_readiness"]["records"][0]["release_current"], "unestablished",
        "partial freshness is not a prohibition on a checked closure proposal"
    );
    let receipt = &supplied["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["kind"] == "accepted_local_delivery")
        .expect("a fresh close decision must receive the exact local delivery receipt")["payload"]
        ["record"];
    assert_eq!(receipt["release"], exposed_release);
    assert_eq!(receipt["recipient"], "human");
    assert!(receipt["receipt"]["id"].as_str().is_some());
    assert_eq!(
        supplied["closure_readiness"]["records"][0]["release"],
        exposed_release
    );
    assert_eq!(
        supplied["closure_readiness"]["records"][0]["permitted_prerequisites_observed"],
        true
    );
    assert_eq!(
        supplied["closure_readiness"]["completion_established"],
        false
    );
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "close-note",
        WorkAction::CloseGoal {
            release: exposed_release.clone(),
        },
        &[],
    ));
    response.continuation = p::Continuation::Stop {
        reason: "The exact local writing artifact was delivered under the accepted criterion."
            .into(),
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let state = f.app.snapshot().unwrap();
    assert!(state.board.completion_supported(&w::WorkId("work".into())));
    assert!(state.work_meta["work"].human_evidence.is_empty());
    assert_eq!(
        state
            .board
            .work(&w::WorkId("work".into()))
            .unwrap()
            .evidence
            .len(),
        1
    );
}

#[test]
fn closure_readiness_exposes_missing_stakeholder_without_substituting_model_judgment() {
    let (mut f, prepared, key) =
        model_evidence_fixture_with_stakeholder("full", false, false, true);
    assert!(interface_control(&prepared)
        .get("closure_readiness")
        .is_none());
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
    response.operations.push(effect(
        "deliver-note",
        WorkAction::DeliverRelease {
            release: "model-note-release".into(),
            recipients: set(&["human"]),
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("inspect-closure-blocker");
    let supplied = interface_control(&prepared);
    let records = supplied["closure_readiness"]["records"].as_array().unwrap();
    assert_eq!(
        records.len(),
        1,
        "duplicate release/delivery receipts coalesce"
    );
    assert_eq!(records[0]["release"], "model-note-release");
    assert_eq!(records[0]["release_current"], "unestablished");
    assert_eq!(records[0]["unestablished_deliveries"], json!([]));
    assert_eq!(
        records[0]["unestablished_stakeholder_acceptance"],
        json!(["human"])
    );
    assert_eq!(records[0]["permitted_prerequisites_observed"], false);
    assert!(prepared
        .prompt
        .contains("latest positive exact-release consent"));
    let before = f.app.snapshot().unwrap();
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "close-note",
        WorkAction::CloseGoal {
            release: "model-note-release".into(),
        },
        &[],
    ));
    response.continuation = p::Continuation::Stop {
        reason: "Synthetic attempted closure without required human acceptance.".into(),
    };
    let result = f.run(&prepared, &response);
    assert_eq!(result.status, AdoptionStatus::Held);
    assert_eq!(f.app.snapshot().unwrap().board, before.board);
}

#[test]
fn model_evidence_rejects_metadata_partial_body_and_same_response_read() {
    for mode in ["none", "partial", "same-response-read"] {
        let (mut f, prepared, key) =
            model_evidence_fixture(if mode == "partial" { "partial" } else { "none" }, false);
        assert!(!prepared.prompt.contains(MODEL_NOTE));
        let mut response = decision(&prepared, true);
        if mode == "same-response-read" {
            let reference = serde_json::from_value(
                interface_control(&prepared)["work_artifacts"][0]["source"].clone(),
            )
            .unwrap();
            response.operations.push(p::Operation::Read {
                id: "not-yet-observed".into(),
                reference,
            });
        }
        response.operations.push(model_evidence_operation(&key));
        let before = persisted_state(&f.app);
        let result = f.run(&prepared, &response);
        assert_eq!(result.status, AdoptionStatus::Held);
        assert!(result.reason.unwrap().contains("actually supplied"));
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn late_local_delivery_failure_rolls_back_earlier_deposit_and_receipt() {
    for fail_second_delivery in [false, true] {
        let (mut f, prepared, key) = model_evidence_fixture("full", false);
        assert!(prepared.prompt.contains(MODEL_NOTE));
        let mut response = decision(&prepared, true);
        response.operations.push(model_evidence_operation(&key));
        response.operations.extend(model_judgment_and_shared(&key));
        response.operations.push(effect(
            "release-note",
            WorkAction::Release {
                id: "rollback-release".into(),
                shared: "model-note-shared".into(),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let prepared = f.prepare("deliver-then-reject");
        let control = interface_control(&prepared);
        let release = control["actual_observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|observation| observation["kind"] == "accepted_work_release")
            .unwrap()["payload"]["release"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut response = decision(&prepared, true);
        response.operations.push(effect(
            "first-valid-deposit",
            WorkAction::DeliverRelease {
                release: release.clone(),
                recipients: set(&["human"]),
            },
            &[],
        ));
        if fail_second_delivery {
            // b is in the admitted source audience, so package assembly is
            // permitted. Its absent configured delivery inbox is checked in
            // the second delivery loop, after the first deposit, board receipt
            // and accepted_local_delivery observation have been staged.
            assert!(prepared.audience.contains("b"));
            response.operations.push(effect(
                "second-invalid-inbox",
                WorkAction::DeliverRelease {
                    release,
                    recipients: set(&["b"]),
                },
                &[],
            ));
        }
        let before = persisted_state(&f.app);
        let result = f.run(&prepared, &response);
        let after = persisted_state(&f.app);
        if fail_second_delivery {
            assert_eq!(result.status, AdoptionStatus::Held);
            assert!(result
                .reason
                .unwrap()
                .contains("only explicitly configured local recipient inboxes are supported"));
            assert_eq!(after["board"], before["board"]);
            assert_eq!(after["local_deliveries"], before["local_deliveries"]);
            assert_eq!(after["observations"], before["observations"]);
            assert_eq!(
                after["personas"]["a"]["focus"]["work"]["pending_observations"],
                before["personas"]["a"]["focus"]["work"]["pending_observations"]
            );
            assert_eq!(after, before);
        } else {
            // Same first operation in isolation is a positive control: it
            // actually deposits bytes and queues the new metadata receipt.
            assert_eq!(result.status, AdoptionStatus::Adopted);
            assert_eq!(after["local_deliveries"].as_object().unwrap().len(), 1);
            assert_eq!(
                after["observations"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter(|o| o["kind"] == "accepted_local_delivery")
                    .count(),
                1
            );
            let state = f.app.snapshot().unwrap();
            assert_eq!(
                state
                    .board
                    .work(&w::WorkId("work".into()))
                    .unwrap()
                    .deliveries
                    .len(),
                1
            );
        }
    }
}

#[test]
fn model_evidence_invalid_tail_or_outcome_cannot_partially_commit() {
    for wrong_outcome in [false, true] {
        let (mut f, prepared, key) = model_evidence_fixture("full", false);
        let mut response = decision(&prepared, true);
        let mut evidence = model_evidence_operation(&key);
        if wrong_outcome {
            if let p::Operation::Effect { inputs, .. } = &mut evidence {
                inputs["outcome"] = json!("unadopted-outcome");
            }
        }
        response.operations.push(evidence);
        // Isolate the invalid outcome from the independently invalid tail.
        if !wrong_outcome {
            response.operations.push(effect(
                "invalid-late-release",
                WorkAction::Release {
                    id: "invalid-release".into(),
                    shared: "missing-shared-decision".into(),
                },
                &[],
            ));
        }
        let before = persisted_state(&f.app);
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn model_evidence_does_not_complete_automatically_or_allow_duplicate_or_forged_provenance() {
    for forge in [false, true] {
        let (mut f, prepared, key) = model_evidence_fixture("full", false);
        let mut response = decision(&prepared, true);
        response.operations.push(model_evidence_operation(&key));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        let state = f.app.snapshot().unwrap();
        let work = state.board.work(&w::WorkId("work".into())).unwrap();
        assert_eq!(work.evidence.len(), 1);
        assert!(work.judgments.is_empty() && work.shared.is_empty() && work.releases.is_empty());
        assert!(!state.board.completion_supported(&w::WorkId("work".into())));
        let prepared = f.prepare("repeat-or-forge-model-evidence");
        let mut response = decision(&prepared, true);
        let mut operation = model_evidence_operation(&key);
        if forge {
            if let p::Operation::Effect { inputs, .. } = &mut operation {
                inputs["id"] = json!("forged-evidence");
                inputs["source_kind"] = json!("HumanJudgment");
            }
        }
        response.operations.push(operation);
        let before = persisted_state(&f.app);
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn model_evidence_stale_artifact_cannot_use_an_old_supplied_body() {
    let (mut f, prepared, old) = model_evidence_fixture("full", false);
    let subject =
        serde_json::from_value(interface_control(&prepared)["work_artifacts"][0]["source"].clone())
            .unwrap();
    let mut response = decision(&prepared, true);
    response.operations.push(effect(
        "new-artifact-revision",
        WorkAction::PutArtifact {
            artifact: old.id.clone(),
            base: Some(old.clone()),
            subject,
        },
        &[],
    ));
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("reject-stale-model-evidence");
    assert!(prepared.prompt.contains(MODEL_NOTE));
    let mut response = decision(&prepared, true);
    response.operations.push(model_evidence_operation(&old));
    let before = persisted_state(&f.app);
    let result = f.run(&prepared, &response);
    assert_eq!(result.status, AdoptionStatus::Held);
    assert!(result.reason.unwrap().contains("exact current artifact"));
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn model_evidence_cannot_waive_required_outside_review() {
    let (mut f, prepared, key) = model_evidence_fixture("full", true);
    let mut response = decision(&prepared, true);
    response.operations.push(model_evidence_operation(&key));
    response.operations.push(effect(
        "self-assessment",
        WorkAction::Assess {
            id: "self-assessment".into(),
            artifact: key.clone(),
            outcome: "result".into(),
            role: "outside".into(),
            evidence: set(&["model-note-evidence"]),
            passes: true,
            explanation: "An attributed self-assessment of the actual note.".into(),
            limitations: vec!["No qualified outside assessor participated.".into()],
        },
        &[],
    ));
    response.operations.extend(model_judgment_and_shared(&key));
    let before = persisted_state(&f.app);
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn local_delivery_requires_responsibility_accepted_before_its_operation() {
    for accept_first in [false, true] {
        let (mut f, prepared, key) = model_evidence_fixture_with_peer("full", false, true);
        let mut response = decision(&prepared, true);
        response.operations.push(model_evidence_operation(&key));
        response.operations.extend(model_judgment_and_shared(&key));
        response.operations.push(effect(
            "release-for-peer-delivery",
            WorkAction::Release {
                id: "model-note-release".into(),
                shared: "model-note-shared".into(),
            },
            &[],
        ));
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        initialize_interface_peer(&mut f);
        let prepared = f
            .app
            .prepare(
                PrepareRequest {
                    id: "peer-delivery".into(),
                    persona: "b".into(),
                    work: "work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap();
        let mut response = decision(&prepared, true);
        response.continuation = p::Continuation::Stop {
            reason: "Stop after the explicit delivery disposition.".into(),
        };
        if accept_first {
            let control = interface_control(&prepared);
            let offer = &control["own_pending_offers"][0];
            response.operations.push(p::Operation::Responsibility {
                id: offer["id"].as_str().unwrap().into(),
                work_id: "work".into(),
                terms_revision: offer["terms_revision"].as_str().unwrap().into(),
                disposition: "accept".into(),
                reason: "Voluntarily accept the current contribution terms before delivery.".into(),
            });
        }
        response.operations.push(effect(
            "peer-deliver-note",
            WorkAction::DeliverRelease {
                release: "model-note-release".into(),
                recipients: set(&["human"]),
            },
            &[],
        ));
        let before = persisted_state(&f.app);
        let result = f.run(&prepared, &response);
        if accept_first {
            assert_eq!(
                result.status,
                AdoptionStatus::Adopted,
                "{:?}",
                result.reason
            );
            assert_eq!(
                persisted_state(&f.app)["local_deliveries"]
                    .as_object()
                    .unwrap()
                    .len(),
                1
            );
        } else {
            assert_eq!(result.status, AdoptionStatus::Held);
            assert!(result.reason.unwrap().contains("accepted responsibility"));
            assert_eq!(persisted_state(&f.app), before);
        }
    }
}

#[test]
fn model_evidence_forward_reference_and_malformed_action_are_rejected_atomically() {
    for mode in [
        "forward",
        "array",
        "oversized",
        "blank",
        "too-many-limitations",
    ] {
        let (mut f, prepared, key) = model_evidence_fixture("full", false);
        let mut response = decision(&prepared, true);
        if mode == "forward" {
            response.operations.extend(model_judgment_and_shared(&key));
        }
        let mut operation = model_evidence_operation(&key);
        if let p::Operation::Effect { inputs, .. } = &mut operation {
            match mode {
                "array" => inputs["artifact"] = json!([key.id, key.revision, key.digest]),
                "oversized" => inputs["explanation"] = json!("x".repeat(16_385)),
                "blank" => inputs["explanation"] = json!(" "),
                "too-many-limitations" => {
                    inputs["limitations"] = json!(vec!["Bounded limitation"; 257])
                }
                _ => (),
            }
        }
        response.operations.push(operation);
        let before = persisted_state(&f.app);
        assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Held);
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn local_delivery_cannot_forward_reference_a_later_release() {
    for release_first in [false, true] {
        let (mut f, prepared, key) = model_evidence_fixture("full", false);
        let mut response = decision(&prepared, true);
        response.operations.push(model_evidence_operation(&key));
        response.operations.extend(model_judgment_and_shared(&key));
        let release = effect(
            "release-in-order",
            WorkAction::Release {
                id: "ordered-release".into(),
                shared: "model-note-shared".into(),
            },
            &[],
        );
        let delivery = effect(
            "deliver-in-order",
            WorkAction::DeliverRelease {
                release: "ordered-release".into(),
                recipients: set(&["human"]),
            },
            &[],
        );
        response.operations.extend(if release_first {
            vec![release, delivery]
        } else {
            vec![delivery, release]
        });
        let before = persisted_state(&f.app);
        let result = f.run(&prepared, &response);
        if release_first {
            assert_eq!(
                result.status,
                AdoptionStatus::Adopted,
                "{:?}",
                result.reason
            );
            let state = persisted_state(&f.app);
            assert_eq!(state["local_deliveries"].as_object().unwrap().len(), 1);
            let prepared = f.prepare("close-after-ordered-delivery");
            let mut response = decision(&prepared, false);
            response.operations.push(effect(
                "close-ordered-goal",
                WorkAction::CloseGoal {
                    release: "ordered-release".into(),
                },
                &[],
            ));
            response.continuation = p::Continuation::Stop {
                reason: "Close after the retained exact local delivery.".into(),
            };
            assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        } else {
            assert_eq!(result.status, AdoptionStatus::Held);
            assert!(result.reason.unwrap().contains("release must exist before"));
            assert_eq!(persisted_state(&f.app), before);
        }
    }
}

#[test]
fn qualified_read_supplies_one_complete_current_body_within_context_bound() {
    // Keep the original complete body and 128 KiB cap. The helper also proves
    // that even one duplicate body would exceed this cap.
    qualified_read_context_bound_case(1500, true);
}

#[test]
fn qualified_read_complete_body_exceeding_context_bound_is_held_without_trimming() {
    qualified_read_context_bound_case(1600, false);
}

fn qualified_read_context_bound_case(root_repetitions: usize, expect_fit: bool) {
    let mut configuration = config();
    configuration.context_bytes = 128 * 1024;
    configuration.navigation_per_call.result_bytes = 64 * 1024;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    let root_text = "READ_ROOT_COMPLETE_".repeat(root_repetitions);
    let qualification_text = "READ_QUALIFICATION_COMPLETE_".repeat(300);
    let qualification = record(
        "read-cost-qualification",
        f::Scope::Assets,
        &qualification_text,
    );
    let mut root = record("read-cost-root", f::Scope::Assets, &root_text);
    root.qualifications
        .push(qualification.reference(f::Representation::Utf8));
    root = root.seal().unwrap();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![root.clone(), qualification.clone()],
            true,
            NOW,
        )
        .unwrap();
    let prepared = fixture.prepare("request-complete-read");
    eprintln!(
        "read-cost baseline bytes={} bound={}",
        prepared.assembled_bytes_with_reserve,
        128 * 1024
    );
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Read {
        id: "read-complete".into(),
        reference: reference(&root),
    });
    let state_before = persisted_state(&fixture.app);
    let report = fixture.run(&prepared, &response);
    eprintln!("read-cost root_repetitions={root_repetitions}: {report:?}");
    if !expect_fit {
        assert_eq!(report.status, AdoptionStatus::Held, "{report:?}");
        let reason = report.reason.unwrap();
        assert!(
            reason.contains("mandatory attributed context exceeds admitted bound")
                || (reason.contains("mandatory context requires")
                    && reason.contains("limit 131072")),
            "{reason}"
        );
        assert_eq!(persisted_state(&fixture.app), state_before);
        assert!(!fixture
            .app
            .snapshot()
            .unwrap()
            .observations
            .values()
            .any(|observation| observation.kind == "qualified_read"));
        return;
    }
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let before = fixture.app.snapshot().unwrap();
    let retained = before
        .observations
        .values()
        .find(|o| o.kind == "qualified_read")
        .unwrap();
    assert!(retained.payload["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["text"] == root_text));
    let receipt_manifest = retained.payload["result"]["manifest_sha256"].clone();
    let receipt_bytes = retained.payload["result"]["context_bytes"].clone();
    let next = fixture
        .app
        .prepare(
            PrepareRequest {
                id: "observe-complete-read".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .expect("one complete qualified observation must fit without trimming");
    eprintln!(
        "read-cost observed bytes={} bound={}",
        next.assembled_bytes_with_reserve,
        128 * 1024
    );
    assert!(next.assembled_bytes_with_reserve <= 128 * 1024);
    assert!(
        next.assembled_bytes_with_reserve + root_text.len() + qualification_text.len() > 128 * 1024,
        "the fixture must reject even one additional raw copy of the qualified body"
    );
    assert_eq!(next.prompt.matches(&root_text).count(), 1);
    assert_eq!(next.prompt.matches(&qualification_text).count(), 1);
    let control = interface_control(&next);
    let observation = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "qualified_read")
        .unwrap();
    let result = &observation["payload"]["result"];
    for source in [&root, &qualification] {
        let entry = result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["reference"]["source"] == source.source)
            .unwrap();
        let copied: p::ExactRef = serde_json::from_value(entry["reference"].clone()).unwrap();
        assert_eq!(copied.source, source.source);
        assert_eq!(copied.revision, source.revision);
        assert_eq!(copied.representation, "utf8");
        assert_eq!(
            (copied.start, copied.end, copied.lifetime),
            (None, None, None)
        );
        assert_eq!(entry["text"], std::str::from_utf8(&source.bytes).unwrap());
        assert_eq!(entry["policy"], json!(source.policy));
        assert!(result["source_attribution"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["reference"]["source"] == source.source && a["imported_by"] == "operator"));
    }
    assert_eq!(
        observation["payload"]["read_receipt"]["manifest_sha256"],
        receipt_manifest
    );
    assert_eq!(
        observation["payload"]["read_receipt"]["context_bytes"],
        receipt_bytes
    );
    assert!(observation["current_required_qualifications"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result.get("rendered_context").is_none());
    assert_eq!(
        serde_json::to_value(&fixture.app.snapshot().unwrap().observations[&retained.id]).unwrap(),
        serde_json::to_value(retained).unwrap()
    );
}

#[test]
fn qualified_read_rechecks_required_policy_before_supplying_retained_body() {
    for restriction in ["revoked", "expired", "withdrawn"] {
        let mut configuration = config();
        let mut required_policy = policy();
        required_policy.id = "read-recheck-policy".into();
        if restriction == "expired" {
            required_policy.expires_at_unix = Some(NOW + 1);
        }
        configuration
            .policies
            .active_policies
            .push(required_policy.clone());
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        let mut required = record(
            "read-recheck-required",
            f::Scope::Assets,
            "RECHECK_REQUIRED_COMPLETE_BODY",
        );
        required.policy = required_policy.clone();
        required = required.seal().unwrap();
        let mut root = record(
            "read-recheck-root",
            f::Scope::Assets,
            "RECHECK_ROOT_COMPLETE_BODY",
        );
        root.qualifications
            .push(required.reference(f::Representation::Utf8));
        root = root.seal().unwrap();
        fixture
            .app
            .import_files(
                "operator",
                "a",
                vec![root.clone(), required.clone()],
                true,
                NOW,
            )
            .unwrap();
        let prepared = fixture.prepare("read-before-restriction");
        let mut response = decision(&prepared, true);
        response.operations.push(p::Operation::Read {
            id: "read-before-restriction".into(),
            reference: reference(&root),
        });
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let before = fixture.app.snapshot().unwrap();
        let retained = before
            .observations
            .values()
            .find(|o| o.kind == "qualified_read")
            .unwrap();
        assert_eq!(
            retained.qualified_roots,
            vec![root.reference(f::Representation::Utf8)]
        );
        match restriction {
            "revoked" => fixture
                .app
                .revoke_policy(
                    "operator",
                    required_policy.identity(),
                    "Synthetic revocation.",
                )
                .unwrap(),
            "withdrawn" => fixture
                .app
                .withdraw_source("operator", &required.source, "Synthetic withdrawal.")
                .unwrap(),
            _ => (),
        }
        let next = fixture
            .app
            .prepare(
                PrepareRequest {
                    id: "observe-after-restriction".into(),
                    persona: "a".into(),
                    work: "work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                if restriction == "expired" {
                    NOW + 1
                } else {
                    NOW
                },
            )
            .unwrap();
        assert!(
            !next.prompt.contains("RECHECK_ROOT_COMPLETE_BODY"),
            "{restriction}"
        );
        assert!(
            !next.prompt.contains("RECHECK_REQUIRED_COMPLETE_BODY"),
            "{restriction}"
        );
        assert!(
            !next.supplied_observations.contains(&retained.id),
            "{restriction}"
        );
        assert!(
            interface_control(&next)["actual_observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["status"] == "withheld_unavailable" && o["body_supplied"] == false),
            "{restriction}"
        );
        assert_eq!(
            serde_json::to_value(&fixture.app.snapshot().unwrap().observations[&retained.id])
                .unwrap(),
            serde_json::to_value(retained).unwrap()
        );
    }
}

const INITIAL_CHARACTER_VOICE: &str =
    "SYNTHETIC OWN VOICE: I distinguish an intended choice from an accepted observation.";

fn character_selection_response(prepared: &Prepared) -> p::Decision {
    let mut response = decision(prepared, false);
    let (ocean, vad) = profiles();
    response.operations = vec![
        write(
            "new-voice",
            p::Scope::Fragments {},
            "new-character-voice",
            INITIAL_CHARACTER_VOICE,
            None,
        ),
        p::Operation::CharacterAdopt {
            id: "new-character".into(),
            base_revision: prepared.character_revision.clone(),
            ocean,
            vad,
            own_voice: vec![proposed("new-character-voice", "new-voice")],
            reason: "Adopt this synthetic coherent character account.".into(),
            evidence: vec![],
        },
    ];
    response.fragment_maintenance = p::Maintenance::Change {
        reason: "The exact voice write and character binding form one unit.".into(),
        operation_ids: vec!["new-voice".into(), "new-character".into()],
    };
    response.continuation = p::Continuation::Pause {
        reason: "Wait for an explicitly authorized fresh request.".into(),
    };
    response
}

fn assert_character_voice_is_mandatory_on_fresh_request(f: &mut Fixture, optional_count: usize) {
    let state = f.app.snapshot().unwrap();
    let focus = &state.personas["a"].focus["work"];
    assert!(!focus.permit);
    assert!(state.personas["a"].active_invocation.is_none());
    assert_eq!(focus.fragments.as_ref().unwrap().len(), optional_count);
    assert!(focus.assets.as_ref().unwrap().is_empty());
    let character = state.personas["a"].character.as_ref().unwrap();
    assert_eq!(character.own_voice.len(), 1);
    assert_eq!(character.own_voice[0].source, "new-character-voice");
    let voice_revision = character.own_voice[0].revision.clone();
    assert!(f
        .app
        .prepare(
            PrepareRequest {
                id: "not-automatically-resumed".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary
            },
            NOW
        )
        .is_err());
    f.app.resume("operator", "a", "work").unwrap();
    let fresh = f.prepare("fresh-character-contract");
    assert!(fresh.prompt.contains(INITIAL_CHARACTER_VOICE));
    let roots = fresh.bundle_roots.as_ref().unwrap();
    assert!(roots
        .mandatory
        .iter()
        .any(|r| r.source == "new-character-voice" && r.revision == voice_revision));
    assert!(!roots
        .fragments
        .iter()
        .any(|r| r.source == "new-character-voice"));
    assert!(roots.assets.is_empty());
    assert_eq!(roots.fragments.len(), optional_count);
}

#[test]
fn character_initialization_selection_contract_clear_preserve_and_proposal_are_distinct() {
    for variant in [
        "clear",
        "preserve-uninitialized",
        "replace-own-voice-proposal",
    ] {
        let mut f = Fixture::new(false);
        let prepared = f.prepare("initial-character-contract");
        // Both the supplied duties and emitted schema explain the distinction;
        // README-only guidance would not help the primary form this response.
        assert!(prepared
            .prompt
            .contains("Clearing optional selections does not remove"));
        assert!(prepared
            .prompt
            .contains("Retain an already initialized optional choice"));
        assert!(prepared.prompt.contains("Clear optional context only"));
        let state = f.app.snapshot().unwrap();
        assert!(state.personas["a"].character.is_none());
        assert!(state.personas["a"].focus["work"].fragments.is_none());
        assert!(state.personas["a"].focus["work"].assets.is_none());
        let mut response = character_selection_response(&prepared);
        match variant {
            "preserve-uninitialized" => {
                response.next_fragments = p::SelectionIntent::Preserve {
                    reason: "Try preserving a choice before one exists.".into(),
                }
            }
            "replace-own-voice-proposal" => {
                response.next_fragments = p::SelectionIntent::Replace {
                    reason: "Try additionally selecting the same new mandatory voice.".into(),
                    references: vec![proposed("new-character-voice", "new-voice")],
                }
            }
            _ => (),
        }
        let before = persisted_state(&f.app);
        let report = f.run(&prepared, &response);
        println!(
            "initial {variant}: {:?}, {:?}",
            report.status, report.reason
        );
        if variant == "clear" {
            assert_eq!(report.status, AdoptionStatus::CharacterOnly);
            assert_eq!(report.applied, vec!["new-voice", "new-character"]);
            assert_character_voice_is_mandatory_on_fresh_request(&mut f, 0);
        } else {
            assert_eq!(report.status, AdoptionStatus::Held);
            let reason = report.reason.unwrap();
            assert!(reason.contains(if variant == "preserve-uninitialized" {
                "cannot preserve a nonexistent choice"
            } else {
                "restricted unit permits only subsets of whole exact existing optional entries"
            }));
            assert_eq!(persisted_state(&f.app), before);
            let state = f.app.snapshot().unwrap();
            assert!(state.personas["a"].character.is_none());
            assert!(!state.sources.contains_key("new-character-voice"));
        }
    }
}

#[test]
fn character_change_preserves_initialized_optional_choices_or_explicitly_clears_them() {
    for variant in ["preserve-empty", "preserve-existing", "clear-existing"] {
        let mut f = Fixture::new(true);
        f.accept();
        if variant != "preserve-empty" {
            let optional = record(
                "existing-optional-character-context",
                f::Scope::Fragments,
                "SYNTHETIC OPTIONAL CONTEXT: I ask for an exact observation.",
            );
            f.app
                .import_files("operator", "a", vec![optional.clone()], false, NOW)
                .unwrap();
            let prepared = f.prepare("choose-existing-optional-context");
            let mut response = decision(&prepared, true);
            response.next_fragments = p::SelectionIntent::Replace {
                reason: "Select an exact existing optional fragment before changing character."
                    .into(),
                references: vec![reference(&optional)],
            };
            assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
        }
        let prepared = f.prepare("changed-character-contract");
        let mut response = character_selection_response(&prepared);
        if variant != "clear-existing" {
            response.next_fragments = p::SelectionIntent::Preserve {
                reason: "Retain the initialized optional choice without renewal.".into(),
            };
        }
        response.next_assets = p::SelectionIntent::Preserve {
            reason: "Retain the already initialized empty asset choice.".into(),
        };
        let report = f.run(&prepared, &response);
        println!(
            "changed {variant}: {:?}, {:?}",
            report.status, report.reason
        );
        assert_eq!(report.status, AdoptionStatus::CharacterOnly);
        assert_character_voice_is_mandatory_on_fresh_request(
            &mut f,
            usize::from(variant == "preserve-existing"),
        );
    }
}

fn aggregate_selection_response(prepared: &Prepared, caveat: &f::FileRevision) -> p::Decision {
    let mut response = decision(prepared, true);
    let mut a = write(
        "save-a",
        p::Scope::Assets {},
        "aggregate-a",
        "EXACT_AGGREGATE_ASSET_A",
        None,
    );
    if let p::Operation::FileWrite { qualifications, .. } = &mut a {
        qualifications.push(reference(caveat));
    }
    response.operations = vec![
        a,
        write(
            "save-b",
            p::Scope::Assets {},
            "aggregate-b",
            "EXACT_AGGREGATE_ASSET_B",
            None,
        ),
        effect(
            "register-a",
            WorkAction::PutArtifact {
                artifact: "aggregate-artifact".into(),
                base: None,
                subject: proposed("aggregate-a", "save-a"),
            },
            &["save-a"],
        ),
    ];
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain both exact synthetic assets.".into(),
        operation_ids: vec!["save-a".into(), "save-b".into()],
    };
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Explicitly select the qualified asset and independent asset.".into(),
        references: vec![
            proposed("aggregate-a", "save-a"),
            proposed("aggregate-b", "save-b"),
        ],
    };
    response
}

#[test]
fn aggregate_selection_admission_rejects_unusable_replacement_and_supplies_complete_fitting_pair() {
    for bound in [4, 6] {
        let mut configuration = config();
        configuration.corpus.max_closure_references = bound;
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        let caveat = record(
            "aggregate-caveat",
            f::Scope::Assets,
            "EXACT_AGGREGATE_CAVEAT",
        );
        fixture
            .app
            .import_files("operator", "a", vec![caveat.clone()], false, NOW)
            .unwrap();
        let prepared = fixture.prepare("aggregate-selection");
        let response = aggregate_selection_response(&prepared, &caveat);
        let prior_assets = fixture.app.snapshot().unwrap().personas["a"].focus["work"]
            .assets
            .clone();
        let before = json!(fixture.app.snapshot().unwrap());
        let report = fixture.run(&prepared, &response);
        let snapshot = fixture.app.snapshot().unwrap();
        if bound == 4 {
            // Store terminal/accounting changes are separate from Runtime state.
            // No staged file, work effect, observation or fit checkpoint commits.
            assert_eq!(json!(snapshot), before);
            assert_eq!(
                report.status,
                AdoptionStatus::Held,
                "already inadmissible replacement"
            );
            assert!(report
                .reason
                .unwrap()
                .contains("combined context allowance"));
            assert_eq!(
                fixture
                    .app
                    .invocation("aggregate-selection")
                    .unwrap()
                    .runtime_failure_context,
                Some(json!({"kind":"unknown"}))
            );
            assert_eq!(
                json!(snapshot.personas["a"].focus["work"].assets),
                json!(prior_assets)
            );
            assert!(!snapshot.sources.contains_key("aggregate-a"));
            assert!(!snapshot.sources.contains_key("aggregate-b"));
            assert!(snapshot
                .board
                .work(&w::WorkId("work".into()))
                .unwrap()
                .artifacts
                .is_empty());
        } else {
            assert_eq!(report.status, AdoptionStatus::Adopted);
            assert_eq!(
                snapshot
                    .board
                    .work(&w::WorkId("work".into()))
                    .unwrap()
                    .artifacts
                    .len(),
                1
            );
            let root = fixture.app.state_root().to_owned();
            fixture.app = Runtime::open(&root).unwrap();
            let fresh = fixture.prepare("aggregate-fresh");
            let context: Value =
                serde_json::from_str(fresh.prompt.split("QUALIFIED_CONTEXT\n").nth(1).unwrap())
                    .unwrap();
            assert_eq!(context["withheld"], json!([]));
            for source in ["voice", "aggregate-a", "aggregate-b", "aggregate-caveat"] {
                assert!(
                    context["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|e| e["reference"]["source"] == source),
                    "missing {source}"
                );
            }
            assert!(fresh.prompt.contains("EXACT_AGGREGATE_ASSET_A"));
            assert!(fresh.prompt.contains("EXACT_AGGREGATE_CAVEAT"));
            assert!(fresh.prompt.contains("EXACT_AGGREGATE_ASSET_B"));
            let entries = context["entries"].as_array().unwrap();
            let qualified = entries
                .iter()
                .find(|e| e["reference"]["source"] == "aggregate-a")
                .unwrap();
            assert_eq!(qualified["text"], "EXACT_AGGREGATE_ASSET_A");
            assert!(qualified["roles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|role| role["role"] == "root"
                    && role["bundle"] == json!({"kind":"selected_asset","index":0})));
            let caveat_entry = entries
                .iter()
                .find(|e| e["reference"]["source"] == "aggregate-caveat")
                .unwrap();
            assert_eq!(caveat_entry["text"], "EXACT_AGGREGATE_CAVEAT");
            assert!(caveat_entry["roles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|role| role["role"] == "qualification"
                    && role["bundle"] == json!({"kind":"selected_asset","index":0})
                    && role["required_by"] == qualified["reference"]));
        }
    }
}

fn aggregate_preserved_fixture() -> (Fixture, f::FileRevision) {
    let mut configuration = config();
    configuration.corpus.max_closure_references = 4;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    let caveat = record(
        "aggregate-caveat",
        f::Scope::Assets,
        "EXACT_AGGREGATE_CAVEAT",
    );
    let mut asset = record("aggregate-a", f::Scope::Assets, "EXACT_AGGREGATE_ASSET_A");
    asset
        .qualifications
        .push(caveat.reference(f::Representation::Utf8));
    asset = asset.seal().unwrap();
    fixture
        .app
        .import_files("operator", "a", vec![caveat, asset.clone()], false, NOW)
        .unwrap();
    let prepared = fixture.prepare("select-qualified-a");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select the complete qualified asset.".into(),
        references: vec![reference(&asset)],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    (fixture, asset)
}

fn aggregate_cross_scope_response(prepared: &Prepared) -> p::Decision {
    let mut response = decision(prepared, true);
    response.operations.push(write(
        "save-fragment",
        p::Scope::Fragments {},
        "aggregate-fragment",
        "EXACT_NEW_FRAGMENT",
        None,
    ));
    response.fragment_maintenance = p::Maintenance::Change {
        reason: "Retain an independent synthetic fragment.".into(),
        operation_ids: vec!["save-fragment".into()],
    };
    response.next_fragments = p::SelectionIntent::Replace {
        reason: "Select this exact fragment.".into(),
        references: vec![proposed("aggregate-fragment", "save-fragment")],
    };
    response
}

#[test]
fn aggregate_selection_admission_cannot_displace_a_preserved_other_scope() {
    let (mut fixture, _) = aggregate_preserved_fixture();
    let prepared = fixture.prepare("cross-scope-addition");
    assert!(prepared.prompt.contains("EXACT_AGGREGATE_ASSET_A"));
    assert!(prepared.prompt.contains("EXACT_AGGREGATE_CAVEAT"));
    let prior = fixture.app.snapshot().unwrap().personas["a"].focus["work"].clone();
    let report = fixture.run(&prepared, &aggregate_cross_scope_response(&prepared));
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report
        .reason
        .unwrap()
        .contains("makes preserved context unavailable"));
    let snapshot = fixture.app.snapshot().unwrap();
    assert_eq!(
        json!(snapshot.personas["a"].focus["work"].assets),
        json!(prior.assets)
    );
    assert_eq!(
        json!(snapshot.personas["a"].focus["work"].fragments),
        json!(prior.fragments)
    );
    assert!(!snapshot.sources.contains_key("aggregate-fragment"));
}

#[test]
fn aggregate_selection_admission_allows_preserving_already_unavailable_other_scope() {
    let (mut fixture, _) = aggregate_preserved_fixture();
    fixture
        .app
        .withdraw_source(
            "operator",
            "aggregate-caveat",
            "Synthetic authority withdrawal.",
        )
        .unwrap();
    let prepared = fixture.prepare("already-unavailable-addition");
    assert!(!prepared.prompt.contains("EXACT_AGGREGATE_ASSET_A"));
    assert!(!prepared.prompt.contains("EXACT_AGGREGATE_CAVEAT"));
    let report = fixture.run(&prepared, &aggregate_cross_scope_response(&prepared));
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let fresh = fixture.prepare("after-unavailable-preserved");
    assert!(fresh.prompt.contains("EXACT_NEW_FRAGMENT"));
    assert!(!fresh.prompt.contains("EXACT_AGGREGATE_ASSET_A"));
    assert_eq!(
        fixture.app.snapshot().unwrap().personas["a"].focus["work"]
            .assets
            .as_ref()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn aggregate_selection_admission_preserves_clear_and_fitting_reduction() {
    let mut configuration = config();
    configuration.corpus.max_closure_references = 6;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    let caveat = record(
        "aggregate-caveat",
        f::Scope::Assets,
        "EXACT_AGGREGATE_CAVEAT",
    );
    fixture
        .app
        .import_files("operator", "a", vec![caveat.clone()], false, NOW)
        .unwrap();
    let prepared = fixture.prepare("fit-pair");
    assert_eq!(
        fixture
            .run(&prepared, &aggregate_selection_response(&prepared, &caveat))
            .status,
        AdoptionStatus::Adopted
    );
    let selected = fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .assets
        .as_ref()
        .unwrap()[0]
        .reference
        .clone();
    let prepared = fixture.prepare("reduce-pair");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Retain only the qualified asset.".into(),
        references: vec![selected],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("clear-pair");
    assert!(prepared.prompt.contains("EXACT_AGGREGATE_ASSET_A"));
    assert!(!prepared.prompt.contains("EXACT_AGGREGATE_ASSET_B"));
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Clear {
        reason: "Explicitly remove optional assets.".into(),
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let fresh = fixture.prepare("cleared-pair");
    assert!(!fresh.prompt.contains("EXACT_AGGREGATE_ASSET_A"));
    assert!(fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .assets
        .as_ref()
        .unwrap()
        .is_empty());
}

#[test]
fn aggregate_selection_admission_uses_edge_and_byte_quotas_too() {
    for dimension in ["edges", "bytes"] {
        let caveat = record("aggregate-caveat", f::Scope::Assets, &"CAVEAT_".repeat(200));
        let mut asset = record("aggregate-a", f::Scope::Assets, &"QUALIFIED_".repeat(200));
        asset
            .qualifications
            .push(caveat.reference(f::Representation::Utf8));
        asset = asset.seal().unwrap();
        let mut configuration = config();
        if dimension == "edges" {
            configuration.corpus.max_closure_edges = 2;
        } else {
            let closure_bytes = serde_json::to_vec(&asset).unwrap().len()
                + serde_json::to_vec(&caveat).unwrap().len();
            configuration.corpus.max_closure_bytes = closure_bytes * 2;
        }
        let mut fixture = Fixture::with_config(true, configuration);
        fixture.accept();
        fixture
            .app
            .import_files("operator", "a", vec![caveat, asset.clone()], false, NOW)
            .unwrap();
        let prepared = fixture.prepare("select-single");
        let mut response = decision(&prepared, true);
        response.next_assets = p::SelectionIntent::Replace {
            reason: "The qualified asset fits its two-root share.".into(),
            references: vec![reference(&asset)],
        };
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted,
            "{dimension}"
        );
        let prepared = fixture.prepare("add-third-root");
        assert!(prepared.prompt.contains(&"QUALIFIED_".repeat(200)));
        let report = fixture.run(&prepared, &aggregate_cross_scope_response(&prepared));
        assert_eq!(report.status, AdoptionStatus::Held, "{dimension}");
        assert!(report
            .reason
            .unwrap()
            .contains("makes preserved context unavailable"));
        assert!(!fixture
            .app
            .snapshot()
            .unwrap()
            .sources
            .contains_key("aggregate-fragment"));
    }
}

#[test]
fn aggregate_selection_admission_does_not_count_expired_next_call_roots() {
    let mut configuration = config();
    configuration.corpus.max_closure_references = 4;
    let mut fixture = Fixture::with_config(true, configuration);
    fixture.accept();
    let caveat = record(
        "aggregate-caveat",
        f::Scope::Assets,
        "EXACT_AGGREGATE_CAVEAT",
    );
    let mut asset = record("aggregate-a", f::Scope::Assets, "EXACT_EXPIRING_ASSET");
    asset
        .qualifications
        .push(caveat.reference(f::Representation::Utf8));
    asset = asset.seal().unwrap();
    let mut fragment = record(
        "aggregate-fragment",
        f::Scope::Fragments,
        "EXACT_QUALIFIED_FRAGMENT",
    );
    fragment
        .qualifications
        .push(caveat.reference(f::Representation::Utf8));
    fragment = fragment.seal().unwrap();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![caveat, asset.clone(), fragment.clone()],
            false,
            NOW,
        )
        .unwrap();
    let prepared = fixture.prepare("select-expiring");
    let mut response = decision(&prepared, true);
    let mut once = reference(&asset);
    once.lifetime = Some("next_call".into());
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Retain the asset for one admission only.".into(),
        references: vec![once],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = fixture.prepare("consume-expiring");
    assert!(prepared.prompt.contains("EXACT_EXPIRING_ASSET"));
    let mut response = decision(&prepared, true);
    response.next_fragments = p::SelectionIntent::Replace {
        reason: "Choose qualified context after the asset expires.".into(),
        references: vec![reference(&fragment)],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let fresh = fixture.prepare("after-expiring");
    assert!(fresh.prompt.contains("EXACT_QUALIFIED_FRAGMENT"));
    assert!(fresh.prompt.contains("EXACT_AGGREGATE_CAVEAT"));
    assert!(!fresh.prompt.contains("EXACT_EXPIRING_ASSET"));
}

#[path = "selection_expiry/cases.rs"]
mod selection_expiry_cases;

#[test]
fn formal_evidence_contract_is_supplied_in_prompt_and_action_schema() {
    let (_f, prepared, _key) = model_evidence_fixture("full", false);
    assert!(prepared
        .prompt
        .contains("judge.claims.<outcome>.evidence and assess.evidence"));
    assert!(prepared.prompt.contains("response.work.evidence"));
    let schema = &interface_control(&prepared)["work_capability"]["inputs_schema"];
    let claim = schema["$defs"]["ClaimInput"]["properties"]["evidence"]["description"]
        .as_str()
        .unwrap();
    assert!(claim.contains("formal work EvidenceRecord.id"));
    assert!(
        claim.contains("observation") && claim.contains("met=false") && claim.contains("met=true")
    );
    let assess = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["properties"]["kind"]["const"] == "assess")
        .unwrap();
    let description = assess["properties"]["evidence"]["description"]
        .as_str()
        .unwrap();
    assert!(description.contains("formal work EvidenceRecord.id"));
    assert!(description.contains("passes=false") && description.contains("observation"));
}

#[test]
fn formal_evidence_namespace_rejects_supplied_message_ids_without_promoting_or_filtering() {
    for mixed_namespace in [false, true] {
        let (mut f, prepared, key) = model_evidence_fixture("full", false);
        let mut messages = decision(&prepared, true);
        // Real local deposits to the same persona isolate the identifier namespace
        // without requiring another persona's initialization or fabricated state.
        for id in ["context-one", "context-two"] {
            messages.operations.push(p::Operation::Communicate {
                id: id.into(),
                recipient: "a".into(),
                subject: "Attributed critique".into(),
                text: "Consider the tradeoff and preserve the stated unknowns.".into(),
            });
        }
        assert_eq!(f.run(&prepared, &messages).status, AdoptionStatus::Adopted);
        let prepared = f.prepare("judge-with-supplied-message-context");
        let observations = interface_control(&prepared)["actual_observations"]
            .as_array()
            .unwrap()
            .clone();
        let ids = observations
            .iter()
            .filter(|o| o["kind"] == "received_local_persona_message")
            .map(|o| o["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        let mut response = decision(&prepared, true);
        response.work = p::WorkDisposition::Ongoing {
            reason: "The supplied messages inform this attributed account; formal judgment examines the actual note.".into(),
            evidence: ids.clone(),
        };
        response.operations.push(model_evidence_operation(&key));
        let mut judgment = model_judgment_and_shared(&key);
        if mixed_namespace {
            let p::Operation::Effect { inputs, .. } = &mut judgment[0] else {
                panic!("expected judgment effect")
            };
            inputs["claims"]["result"]["evidence"]
                .as_array_mut()
                .unwrap()
                .extend(ids.iter().map(|id| json!(id)));
        }
        response.operations.extend(judgment);
        let before = f.app.snapshot().unwrap().board;
        f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
        let result = f.app.adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&response).unwrap(),
            NOW,
        );
        if mixed_namespace {
            let report = result.unwrap();
            assert_eq!(report.status, AdoptionStatus::Held);
            let error = report.reason.unwrap();
            eprintln!("mixed supplied messages/formal evidence rejected: {error}");
            assert!(error.contains("claim evidence identities"));
            assert_eq!(
                f.app.snapshot().unwrap().board,
                before,
                "no partial formal evidence or judgment adoption"
            );
        } else {
            assert_eq!(result.unwrap().status, AdoptionStatus::Adopted);
            let state = f.app.snapshot().unwrap();
            let work = state.board.work(&"work".into()).unwrap();
            assert_eq!(work.evidence.len(), 1);
            assert_eq!(
                work.judgments[0].input.claims[&"result".into()].evidence,
                BTreeSet::from([w::RecordId("model-note-evidence".into())])
            );
            assert!(ids
                .iter()
                .all(|id| !work.evidence.contains_key(&w::RecordId(id.clone()))));
        }
    }
}

const CITATION_DIAGNOSTIC: &str =
    "supplied observation reference not supplied as formal work evidence";

fn citation_diagnostic_fixture(
    secret: &str,
) -> (Fixture, Prepared, ArtifactKey, String, f::Policy) {
    let mut configuration = config();
    let mut optional_policy = policy();
    optional_policy.id = "citation-context-policy".into();
    let mut hidden_policy = policy();
    hidden_policy.id = "hidden-citation-policy".into();
    hidden_policy.readers = set(&["b"]);
    configuration
        .policies
        .active_policies
        .extend([optional_policy.clone(), hidden_policy.clone()]);
    let mut f = Fixture::with_config(true, configuration);
    f.accept();
    let mut hidden_work = need();
    hidden_work.id = "hidden-citation-work".into();
    hidden_work.request = secret.into();
    hidden_work.decision_rule = w::DecisionRule::Designated(w::PersonaId("b".into()));
    hidden_work.invitations[0].to = "b".into();
    f.app.add_need("operator", hidden_work).unwrap();
    let mut context = record(
        "citation-context",
        f::Scope::Assets,
        "AUTHORIZED_CITATION_CONTEXT",
    );
    context.policy = optional_policy.clone();
    context = context.seal().unwrap();
    let mut hidden = record("hidden-citation-source", f::Scope::Assets, secret);
    hidden.policy = hidden_policy;
    hidden = hidden.seal().unwrap();
    f.app
        .import_files("operator", "a", vec![context.clone()], false, NOW)
        .unwrap();
    f.app
        .import_files("operator", "b", vec![hidden], false, NOW)
        .unwrap();
    let prepared = f.prepare("citation-author");
    let mut response = decision(&prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain the exact synthetic comparison.".into(),
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
        reason: "Supply the exact artifact and optional context.".into(),
        references: vec![proposed("model-note", "save-note"), reference(&context)],
    };
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("citation-message");
    let key = serde_json::from_value(control(&prepared)["work_artifacts"][0]["artifact"].clone())
        .unwrap();
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Communicate {
        id: "ordinary-citation-context".into(),
        recipient: "a".into(),
        subject: "Ordinary context".into(),
        text: "This message is an observation, not formal proof.".into(),
    });
    assert_eq!(f.run(&prepared, &response).status, AdoptionStatus::Adopted);
    let prepared = f.prepare("citation-input");
    assert!(prepared.prompt.contains("AUTHORIZED_CITATION_CONTEXT"));
    assert!(!prepared.prompt.contains(secret));
    let message = control(&prepared)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == "received_local_persona_message")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (f, prepared, key, message, optional_policy)
}

fn citation_diagnostic_response(
    prepared: &Prepared,
    key: &ArtifactKey,
    evidence: &str,
    assess: bool,
) -> p::Decision {
    let mut response = decision(prepared, true);
    response.operations.push(if assess {
        effect(
            "citation-assess",
            WorkAction::Assess {
                id: "citation-assessment".into(),
                artifact: key.clone(),
                outcome: "result".into(),
                role: "outside".into(),
                evidence: set(&[evidence]),
                passes: false,
                explanation: "This deliberately exercises a supplied citation mismatch.".into(),
                limitations: vec!["Synthetic fixture only.".into()],
            },
            &[],
        )
    } else {
        effect(
            "citation-judge",
            WorkAction::Judge {
                id: "citation-judgment".into(),
                artifacts: BTreeMap::from([("result".into(), key.clone())]),
                sufficient: false,
                claims: BTreeMap::from([(
                    "result".into(),
                    ClaimInput {
                        met: false,
                        explanation: "This deliberately exercises a supplied citation mismatch."
                            .into(),
                        evidence: set(&[evidence]),
                    },
                )]),
                explanation: "Synthetic namespace control.".into(),
            },
            &[],
        )
    });
    response
}

#[test]
fn citation_diagnostic_known_scope_survives_restart_and_is_actually_supplied() {
    let mut reasons = BTreeSet::new();
    for (secret, assess) in [
        ("HIDDEN_BOARD_VARIANT_ALPHA", false),
        ("HIDDEN_BOARD_VARIANT_BETA", false),
        ("HIDDEN_BOARD_VARIANT_ALPHA", true),
        ("HIDDEN_BOARD_VARIANT_BETA", true),
    ] {
        let (mut f, prepared, key, message, _) = citation_diagnostic_fixture(secret);
        let before = persisted_state(&f.app);
        let report = f.run(
            &prepared,
            &citation_diagnostic_response(&prepared, &key, &message, assess),
        );
        assert_eq!(report.status, AdoptionStatus::Held);
        let reason = report.reason.unwrap();
        assert!(reason.contains(CITATION_DIAGNOSTIC), "{reason}");
        reasons.insert(reason);
        assert_eq!(
            persisted_state(&f.app),
            before,
            "no staged response effect commits"
        );
        let failed = f.app.invocation(&prepared.request.id).unwrap();
        let terminal = f.app.terminal_evidence(&prepared.request.id).unwrap();
        let context = failed.runtime_failure_context.as_ref().unwrap();
        assert_eq!(context["kind"], "known");
        assert_eq!(
            context["terminal_sha256"],
            terminal.exact_ref().unwrap().sha256
        );
        assert_eq!(context["exposure"], json!(prepared.exposure));
        let root = f.app.state_root().to_owned();
        drop(f.app);
        f.app = Runtime::open(&root).unwrap();
        let attempts = f.app.ledger().unwrap().ordinary.consumed.attempts;
        f.app.resume("operator", "a", "work").unwrap();
        assert_eq!(f.app.ledger().unwrap().ordinary.consumed.attempts, attempts);
        let resumed = f.app.snapshot().unwrap();
        let diagnostic = resumed
            .observations
            .values()
            .find(|o| o.kind == "terminal_recovery")
            .unwrap();
        let next = f.prepare("citation-diagnostic-supplied");
        assert!(next.supplied_observations.contains(&diagnostic.id));
        assert!(next.prompt.contains(CITATION_DIAGNOSTIC));
        assert!(!next.prompt.contains(secret));
        assert_eq!(diagnostic.payload["replay_authorized"], false);
        assert!(diagnostic.payload["terminal"]["response_body"]
            .get("text")
            .is_none());
        assert_eq!(
            f.run(&next, &decision(&next, true)).status,
            AdoptionStatus::Adopted
        );
        assert!(!f.app.snapshot().unwrap().personas["a"].focus["work"]
            .pending_observations
            .contains(&diagnostic.id));
        assert_eq!(f.app.invocation(&prepared.request.id).unwrap(), failed);
        assert_eq!(
            f.app.terminal_evidence(&prepared.request.id).unwrap(),
            terminal
        );
    }
    assert_eq!(
        reasons.len(),
        1,
        "fixed reason does not encode hidden values or action details"
    );
}

#[test]
fn citation_diagnostic_rechecks_retired_and_revoked_exposure() {
    for restriction in ["retired", "revoked"] {
        let (mut f, prepared, key, message, optional_policy) =
            citation_diagnostic_fixture("HIDDEN_RECHECK_VALUE");
        assert_eq!(
            f.run(
                &prepared,
                &citation_diagnostic_response(&prepared, &key, &message, false)
            )
            .status,
            AdoptionStatus::Held
        );
        assert_eq!(
            f.app
                .invocation(&prepared.request.id)
                .unwrap()
                .runtime_failure_context
                .unwrap()["kind"],
            "known"
        );
        f.app.resume("operator", "a", "work").unwrap();
        let diagnostic = f
            .app
            .snapshot()
            .unwrap()
            .observations
            .values()
            .find(|o| o.kind == "terminal_recovery")
            .unwrap()
            .clone();
        if restriction == "retired" {
            f.app
                .withdraw_source("operator", "citation-context", "Synthetic retirement.")
                .unwrap();
        } else {
            f.app
                .revoke_policy(
                    "operator",
                    optional_policy.identity(),
                    "Synthetic revocation.",
                )
                .unwrap();
        }
        let next = f.prepare("citation-diagnostic-restricted");
        assert!(
            !next.supplied_observations.contains(&diagnostic.id),
            "{restriction}"
        );
        assert!(!next.prompt.contains(CITATION_DIAGNOSTIC), "{restriction}");
        assert!(
            !next.prompt.contains("AUTHORIZED_CITATION_CONTEXT"),
            "{restriction}"
        );
        assert!(control(&next)["actual_observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["status"] == "withheld_unavailable"));
        let state = f.app.snapshot().unwrap();
        assert_eq!(json!(state.observations[&diagnostic.id]), json!(diagnostic));
        assert!(state.personas["a"].focus["work"]
            .pending_observations
            .contains(&diagnostic.id));
    }
}

#[test]
fn citation_diagnostic_does_not_reclassify_unsupplied_or_arbitrary_board_failures() {
    let (mut f, prepared, key, _, _) = citation_diagnostic_fixture("HIDDEN_UNRELATED_VALUE");
    let report = f.run(
        &prepared,
        &citation_diagnostic_response(&prepared, &key, "authored-but-never-supplied-record", false),
    );
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(!report.reason.unwrap().contains(CITATION_DIAGNOSTIC));
    assert_eq!(
        f.app
            .invocation(&prepared.request.id)
            .unwrap()
            .runtime_failure_context,
        Some(json!({"kind":"unknown"}))
    );
    f.app.resume("operator", "a", "work").unwrap();
    let diagnostic = f
        .app
        .snapshot()
        .unwrap()
        .observations
        .values()
        .find(|o| o.kind == "terminal_recovery")
        .unwrap()
        .clone();
    assert!(diagnostic.exposure.is_none());
    let next = f.prepare("citation-unknown-diagnostic");
    assert!(!next.supplied_observations.contains(&diagnostic.id));
    assert!(!next.prompt.contains("claim evidence identities"));
}

#[test]
fn citation_diagnostic_allows_earlier_same_response_formal_id_without_promoting_observations() {
    for earlier in [true, false] {
        let (mut f, prepared, key, message, _) =
            citation_diagnostic_fixture("HIDDEN_FORMAL_CONTROL");
        let mut response = citation_diagnostic_response(&prepared, &key, &message, false);
        let mut formal = model_evidence_operation(&key);
        if let p::Operation::Effect { inputs, .. } = &mut formal {
            inputs["id"] = json!(message);
        }
        if earlier {
            response.operations.insert(0, formal);
        } else {
            response.operations.push(formal);
        }
        let report = f.run(&prepared, &response);
        assert_eq!(
            report.status,
            if earlier {
                AdoptionStatus::Adopted
            } else {
                AdoptionStatus::Held
            },
            "{:?}",
            report.reason
        );
        let state = f.app.snapshot().unwrap();
        let work = state.board.work(&"work".into()).unwrap();
        assert_eq!(work.evidence.contains_key(&w::RecordId(message)), earlier);
        if !earlier {
            assert!(report.reason.unwrap().contains(CITATION_DIAGNOSTIC));
        }
    }
}

#[path = "work_interface/cases.rs"]
mod work_interface_cases;
