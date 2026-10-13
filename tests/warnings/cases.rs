//! Domain-neutral mechanism fixtures. No semantic usefulness is asserted.
use super::*;

struct WarningFixture {
    fixture: Fixture,
    target: f::FileRevision,
    basis: f::FileRevision,
    qualification: f::FileRevision,
}

fn warning_config() -> Config {
    let mut configuration = config();
    let mut qualification = policy();
    qualification.id = "qualification-policy".into();
    configuration.policies.active_policies.push(qualification);
    configuration
}

fn warning_fixture_with(configuration: Config, primary: Seed, owner: &str) -> WarningFixture {
    warning_fixture_with_target(
        configuration,
        primary,
        owner,
        "Exact historical subject bytes.",
    )
}

fn warning_fixture_with_target(
    configuration: Config,
    primary: Seed,
    owner: &str,
    target_text: &str,
) -> WarningFixture {
    let mut fixture = Fixture::configured(true, configuration, primary, need());
    let target = record("warning-target", f::Scope::Assets, target_text);
    let basis = record(
        "warning-basis",
        f::Scope::Assets,
        "Exact supplied fixture observation.",
    );
    let mut qualification = record(
        "warning-qualification",
        f::Scope::Assets,
        "Explicit qualification supplied in full.",
    );
    qualification.policy.id = "qualification-policy".into();
    qualification = qualification.seal().unwrap();
    fixture
        .app
        .import_files("operator", owner, vec![target.clone()], false, NOW)
        .unwrap();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![basis.clone(), qualification.clone()],
            false,
            NOW,
        )
        .unwrap();
    fixture.accept();
    let prepared = fixture.prepare("warning-prime");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Supply the exact subjects for the next fixture decision.".into(),
        references: vec![
            reference(&target),
            reference(&basis),
            reference(&qualification),
        ],
    };
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    WarningFixture {
        fixture,
        target,
        basis,
        qualification,
    }
}

fn warning_fixture() -> WarningFixture {
    warning_fixture_with(warning_config(), seed("a"), "a")
}

fn adopt_operation(x: &WarningFixture, ineligible: bool) -> p::Operation {
    p::Operation::WarningAdopt {
        id: "adopt-warning".into(),
        warning_id: "warning1".into(),
        scope: p::Scope::Assets {},
        targets: vec![reference(&x.target)],
        uses: p::WarningUses::AllRepresentations {},
        meaning: "I explicitly qualify this exact revision; this fixture establishes no semantic verdict.".into(),
        basis: vec![reference(&x.basis)],
        disposition: if ineligible {
            p::WarningDisposition::Ineligible { reason: "I explicitly withhold all uses of this exact revision.".into() }
        } else {
            p::WarningDisposition::Qualify { required: vec![reference(&x.qualification)] }
        },
        policy_id: "local-policy".into(),
        depends_on: Vec::new(),
    }
}

fn warning_response(prepared: &Prepared, operation: p::Operation) -> p::Decision {
    let mut response = decision(prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Explicit warning lifecycle decision, independently attributed.".into(),
        operation_ids: vec![operation.id().into()],
    };
    response.operations.push(operation);
    response
}

fn accept_warning(x: &mut WarningFixture, ineligible: bool) -> (Prepared, p::Decision) {
    let prepared = x.fixture.prepare("warning-adoption");
    let response = warning_response(&prepared, adopt_operation(x, ineligible));
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(
        report.status,
        AdoptionStatus::Adopted,
        "{:?}",
        report.reason
    );
    assert!(report.reason.unwrap().contains("not content correctness"));
    (prepared, response)
}

fn resolve_operation(x: &WarningFixture, revision: &str, release: bool) -> p::Operation {
    p::Operation::WarningResolve {
        id: "resolve-warning".into(),
        warning_id: "warning1".into(),
        warning_revision: revision.into(),
        scope: p::Scope::Assets {},
        basis: vec![reference(&x.basis)],
        disposition: if release {
            p::WarningResolutionDisposition::Release {}
        } else {
            p::WarningResolutionDisposition::Retain {}
        },
        reason: "I explicitly reconsider this exact warning using the supplied evidence; no content verdict is inferred.".into(),
        depends_on: Vec::new(),
    }
}

fn warning_control(prepared: &Prepared) -> Value {
    let tail = prepared.prompt.split_once("\nCONTROL\n").unwrap().1;
    serde_json::from_str(tail.split_once("\nQUALIFIED_CONTEXT\n").unwrap().0).unwrap()
}

fn warning_context(prepared: &Prepared) -> Value {
    serde_json::from_str(
        prepared
            .prompt
            .split_once("\nQUALIFIED_CONTEXT\n")
            .unwrap()
            .1,
    )
    .unwrap()
}

fn context_has_source(prepared: &Prepared, source: &str) -> bool {
    warning_context(prepared)["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["reference"]["source"] == source)
}

fn warning_read_fixture(configuration: Config, ineligible: bool) -> WarningFixture {
    let mut x = warning_fixture_with_target(
        configuration,
        seed("a"),
        "a",
        &"Withheld diagnostic fixture body.".repeat(256),
    );
    accept_warning(&mut x, ineligible);
    let prepared = x.fixture.prepare("warning-read-history");
    let revision = warning_control(&prepared)["semantic_warnings"][0]["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut operation = resolve_operation(&x, &revision, false);
    if let p::Operation::WarningResolve { reason, .. } = &mut operation {
        *reason = r#"Retained fixture history: "quoted", \ and é."#.repeat(24);
    }
    let mut response = warning_response(&prepared, operation);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Keep supplied evidence without selecting the diagnostic target.".into(),
        references: vec![reference(&x.basis), reference(&x.qualification)],
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    // Exactly four admissions: acceptance, selection, adoption, and retention.
    x
}

fn warning_read_payload(prepared: &Prepared) -> Value {
    let control = warning_control(prepared);
    let warnings = control["semantic_warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["resolutions"].as_array().unwrap().len(), 1);
    // This existing context summary is not evidence of a read observation.
    assert!(!control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|observation| observation["kind"] == "qualified_read"));
    json!({"status":"unavailable","text_supplied":false,"semantic_warnings":warnings})
}

fn warning_read_payload_bytes(ineligible: bool) -> usize {
    let mut x = warning_read_fixture(warning_config(), ineligible);
    let prepared = x.fixture.prepare("warning-read-measure");
    serde_json::to_vec(&warning_read_payload(&prepared))
        .unwrap()
        .len()
}

fn empty_warning_read() -> Value {
    json!({"status":"unavailable","text_supplied":false,"semantic_warnings":[]})
}

fn prepare_warning_read(x: &mut WarningFixture, id: &str) -> (Prepared, State) {
    let initial = x.fixture.app.snapshot().unwrap();
    let prepared = x.fixture.prepare(id);
    let state = x.fixture.app.snapshot().unwrap();
    let left = initial.work_meta["work"].navigation_left;
    let reserved = prepared.navigation;
    assert_eq!(
        state.work_meta["work"].navigation_left,
        f::NavigationLimits {
            calls: left.calls - reserved.calls,
            scanned_bytes: left.scanned_bytes - reserved.scanned_bytes,
            inspected_revisions: left.inspected_revisions - reserved.inspected_revisions,
            matches: left.matches - reserved.matches,
            result_bytes: left.result_bytes - reserved.result_bytes,
        }
    );
    assert_eq!(
        state.work_meta["work"].attempts_left,
        initial.work_meta["work"].attempts_left - 1
    );
    assert!(!context_has_source(&prepared, &x.target.source));
    assert!(!prepared
        .prompt
        .contains("Withheld diagnostic fixture body."));
    (prepared, state)
}

fn warning_read_response(x: &WarningFixture, prepared: &Prepared, count: usize) -> p::Decision {
    let mut response = decision(prepared, true);
    response.operations = (0..count)
        .map(|index| p::Operation::Read {
            id: format!("read-{index}"),
            reference: reference(&x.target),
        })
        .collect();
    response
}

fn retained_warning_read<'a>(
    state: &'a State,
    prepared: &Prepared,
    index: usize,
) -> &'a Observation {
    let suffix = format!(":read-{index}");
    let mut reads = state.observations.values().filter(|observation| {
        observation.invocation.as_deref() == Some(prepared.request.id.as_str())
            && observation.kind == "qualified_read"
            && observation.id.ends_with(&suffix)
    });
    let read = reads.next().unwrap();
    assert!(reads.next().is_none());
    read
}

fn assert_warning_read_unavailable(read: &Observation, expected: &Value) {
    assert_eq!(&read.payload, expected);
    assert!(read.payload.get("result").is_none());
    assert_eq!(read.payload["text_supplied"], false);
    assert!(!read
        .payload
        .to_string()
        .contains("Withheld diagnostic fixture body."));
    assert!(read
        .sources
        .iter()
        .all(|source| { source.source.starts_with("semantic-warning:") }));
    assert_eq!(read.qualified_roots, read.sources);
    if expected["semantic_warnings"].as_array().unwrap().is_empty() {
        assert!(read.sources.is_empty());
    }
}

fn warning_read_admission(state: &State) -> f::Admission {
    let primary = &state.personas["a"].seed;
    f::Admission {
        principal: primary.id.clone(),
        destination: primary.destination.clone(),
        audience: primary.audience.clone(),
        now_unix: NOW,
    }
}

fn warning_read_usage(
    state: &State,
    records: &[&f::FileRevision],
    result_bytes: usize,
) -> f::NavigationUsage {
    let admission_bytes = serde_json::to_vec(&warning_read_admission(state))
        .unwrap()
        .len();
    f::NavigationUsage {
        calls: records.len(),
        scanned_bytes: records
            .iter()
            .map(|record| {
                state.config.corpus.max_closure_bytes
                    + admission_bytes
                    + serde_json::to_vec(&record.reference(f::Representation::Utf8))
                        .unwrap()
                        .len()
            })
            .sum(),
        inspected_revisions: records.len() * state.config.corpus.max_closure_references,
        result_bytes,
        ..f::NavigationUsage::default()
    }
}

fn assert_warning_read_accounting(
    before: &State,
    after: &State,
    prepared: &Prepared,
    used: f::NavigationUsage,
) {
    let mut accounts = after.observations.values().filter(|observation| {
        observation.invocation.as_deref() == Some(prepared.request.id.as_str())
            && observation.kind == "host_accounting"
    });
    let account = accounts.next().unwrap();
    assert!(accounts.next().is_none());
    assert_eq!(
        account.payload["actual_navigation"],
        serde_json::to_value(used).unwrap()
    );
    assert_eq!(
        account.payload["reserved_allowance"],
        serde_json::to_value(prepared.navigation).unwrap()
    );
    assert_eq!(account.payload["allowance_refunded"], false);
    assert!(used.calls <= prepared.navigation.calls);
    assert!(used.scanned_bytes <= prepared.navigation.scanned_bytes);
    assert!(used.inspected_revisions <= prepared.navigation.inspected_revisions);
    assert!(used.matches <= prepared.navigation.matches);
    assert!(used.result_bytes <= prepared.navigation.result_bytes);
    assert_eq!(
        after.work_meta["work"].navigation_left,
        before.work_meta["work"].navigation_left
    );
    assert_eq!(
        after.work_meta["work"].attempts_left,
        before.work_meta["work"].attempts_left
    );
    assert_eq!(after.accepted_bindings, before.accepted_bindings);
    assert_eq!(
        serde_json::to_value(&after.semantic_warnings).unwrap(),
        serde_json::to_value(&before.semantic_warnings).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&after.sources).unwrap(),
        serde_json::to_value(&before.sources).unwrap()
    );
    for observation in after.observations.values().filter(|observation| {
        observation.invocation.as_deref() == Some(prepared.request.id.as_str())
    }) {
        assert!(after.personas["a"].focus["work"]
            .pending_observations
            .contains(&observation.id));
    }
}

#[test]
fn warning_read_diagnostics_require_admitted_calls_and_scan_capacity() {
    for bound in ["calls", "scan", "inspections"] {
        let mut configuration = warning_config();
        match bound {
            "calls" => {
                configuration.navigation_per_call.calls = 1;
                configuration.navigation_per_work.calls = 4;
            }
            "scan" => {
                // Even the public admission/reference overhead cannot fit.
                configuration.navigation_per_call.scanned_bytes =
                    configuration.corpus.max_closure_bytes;
            }
            "inspections" => configuration.navigation_per_call.inspected_revisions = 0,
            _ => unreachable!(),
        }
        let mut x = warning_read_fixture(configuration, true);
        let (prepared, before) = prepare_warning_read(&mut x, "warning-read-admission");
        let permitted = warning_read_payload(&prepared);
        assert!(serde_json::to_vec(&permitted).unwrap().len() <= prepared.navigation.result_bytes);
        if bound == "calls" {
            assert_eq!(prepared.navigation.calls, 0);
        }
        let response = warning_read_response(&x, &prepared, 2);
        assert_eq!(
            x.fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let after = x.fixture.app.snapshot().unwrap();
        for index in 0..2 {
            assert_warning_read_unavailable(
                retained_warning_read(&after, &prepared, index),
                &empty_warning_read(),
            );
        }
        assert_warning_read_accounting(
            &before,
            &after,
            &prepared,
            f::NavigationUsage {
                calls: if bound == "calls" { 0 } else { 2 },
                ..f::NavigationUsage::default()
            },
        );
    }
}

#[test]
fn warning_read_complete_history_must_fit_both_result_limits() {
    let bytes = warning_read_payload_bytes(true);
    for bound in ["per_read", "cumulative", "empty"] {
        let mut configuration = warning_config();
        let per_call = if bound == "per_read" {
            bytes - 1
        } else {
            bytes + 1
        };
        let remaining = match bound {
            "per_read" => bytes * 2,
            "cumulative" => bytes - 1,
            "empty" => 0,
            _ => unreachable!(),
        };
        configuration.navigation_per_call.result_bytes = per_call;
        configuration.navigation_per_work.result_bytes = 4 * per_call + remaining;
        let mut x = warning_read_fixture(configuration, true);
        let (prepared, before) = prepare_warning_read(&mut x, "warning-read-result-bound");
        let permitted = warning_read_payload(&prepared);
        assert_eq!(serde_json::to_vec(&permitted).unwrap().len(), bytes);
        let mut without_history = permitted.clone();
        without_history["semantic_warnings"][0]["resolutions"] = json!([]);
        assert!(serde_json::to_vec(&without_history).unwrap().len() < bytes - 1);
        assert_eq!(prepared.navigation.result_bytes, per_call.min(remaining));
        if bound != "per_read" {
            assert!(bytes <= before.config.navigation_per_call.result_bytes);
        }
        let response = warning_read_response(&x, &prepared, 2);
        assert_eq!(
            x.fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let after = x.fixture.app.snapshot().unwrap();
        for index in 0..2 {
            assert_warning_read_unavailable(
                retained_warning_read(&after, &prepared, index),
                &empty_warning_read(),
            );
        }
        assert_warning_read_accounting(
            &before,
            &after,
            &prepared,
            warning_read_usage(&before, &[&x.target, &x.target], 0),
        );
    }
}

#[test]
fn warning_read_fitting_history_is_charged_exactly_and_cannot_repeat_for_free() {
    let bytes = warning_read_payload_bytes(true);
    let mut configuration = warning_config();
    configuration.navigation_per_call.result_bytes = bytes;
    configuration.navigation_per_work.result_bytes = 5 * bytes;
    let mut x = warning_read_fixture(configuration, true);
    let (prepared, before) = prepare_warning_read(&mut x, "warning-read-exact-fit");
    let expected = warning_read_payload(&prepared);
    assert_eq!(serde_json::to_vec(&expected).unwrap().len(), bytes);
    assert_eq!(prepared.navigation.result_bytes, bytes);
    assert!(before.semantic_warnings["warning1"].active());
    assert!(matches!(
        before.accepted_bindings[0].disposition,
        f::BindingDisposition::Ineligible { .. }
    ));
    let response = warning_read_response(&x, &prepared, 2);
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let after = x.fixture.app.snapshot().unwrap();
    let first = retained_warning_read(&after, &prepared, 0);
    assert_warning_read_unavailable(first, &expected);
    let warning = &before.semantic_warnings["warning1"];
    let mut markers = vec![warning.marker.clone()];
    markers.extend(warning.history.iter().map(|event| event.marker.clone()));
    markers.sort();
    markers.dedup();
    assert_eq!(first.sources, markers);
    assert_eq!(serde_json::to_vec(&first.payload).unwrap().len(), bytes);
    assert_warning_read_unavailable(
        retained_warning_read(&after, &prepared, 1),
        &empty_warning_read(),
    );
    assert_warning_read_accounting(
        &before,
        &after,
        &prepared,
        warning_read_usage(&before, &[&x.target, &x.target], bytes),
    );
    let first_id = first.id.clone();
    let next = x.fixture.prepare("warning-read-observed");
    assert!(!context_has_source(&next, &x.target.source));
    assert!(!next.prompt.contains("Withheld diagnostic fixture body."));
    let control = warning_control(&next);
    // The retained read keeps its complete inherited applicability lineage.
    // The ineligible target therefore withholds this observation on later
    // supply; the separate permitted warning-control view remains available.
    // Budgeting the read must not weaken that existing applicability gate.
    let slot = after.personas["a"].focus["work"]
        .pending_observations
        .iter()
        .position(|id| id == &first_id)
        .unwrap();
    assert_eq!(
        control["actual_observations"][slot],
        json!({"slot":slot,"status":"withheld_unavailable","body_supplied":false})
    );
    assert!(!control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|observation| observation["id"] == first_id));
    assert_eq!(control["semantic_warnings"], expected["semantic_warnings"]);
}

#[test]
fn warning_read_prior_success_consumes_the_same_result_allowance() {
    let bytes = warning_read_payload_bytes(true);
    let mut configuration = warning_config();
    configuration.navigation_per_call.result_bytes = bytes;
    configuration.navigation_per_work.result_bytes = 5 * bytes;
    let mut x = warning_read_fixture(configuration, true);
    let (prepared, before) = prepare_warning_read(&mut x, "warning-read-after-success");
    assert_eq!(
        serde_json::to_vec(&warning_read_payload(&prepared))
            .unwrap()
            .len(),
        bytes
    );
    let mut response = warning_read_response(&x, &prepared, 3);
    response.operations[0] = p::Operation::Read {
        id: "read-0".into(),
        reference: reference(&x.basis),
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let after = x.fixture.app.snapshot().unwrap();
    let success = retained_warning_read(&after, &prepared, 0);
    assert_eq!(
        success.payload["result"]["entries"][0]["reference"]["source"],
        x.basis.source
    );
    assert_eq!(
        success.payload["result"]["entries"][0]["text"],
        std::str::from_utf8(&x.basis.bytes).unwrap()
    );
    let charged = success.payload["result"]["context_bytes"].as_u64().unwrap() as usize;
    assert_eq!(
        charged,
        success.payload["result"]["rendered_context"]
            .as_str()
            .unwrap()
            .len()
    );
    assert!(charged > 0 && charged < bytes);
    for index in 1..3 {
        assert_warning_read_unavailable(
            retained_warning_read(&after, &prepared, index),
            &empty_warning_read(),
        );
    }
    assert_warning_read_accounting(
        &before,
        &after,
        &prepared,
        warning_read_usage(&before, &[&x.basis, &x.target, &x.target], charged),
    );
}

#[test]
fn warning_read_overflow_or_result_exhaustion_cannot_become_warning_discovery() {
    let bytes = warning_read_payload_bytes(false);
    for cumulative in [false, true] {
        let mut configuration = warning_config();
        let per_call = if cumulative { 32 * 1024 } else { bytes };
        configuration.navigation_per_call.result_bytes = per_call;
        configuration.navigation_per_work.result_bytes = 4 * per_call + bytes;
        let mut x = warning_read_fixture(configuration, false);
        let (prepared, before) = prepare_warning_read(&mut x, "warning-read-overflow");
        let permitted = warning_read_payload(&prepared);
        assert_eq!(serde_json::to_vec(&permitted).unwrap().len(), bytes);
        assert!(bytes <= per_call);
        assert_eq!(prepared.navigation.result_bytes, bytes);
        // Establish the precise Files failure while the smaller warning view
        // would fit. All fixture records and qualifications are permitted.
        let corpus = f::Corpus::from_revisions(
            before.sources.values().flat_map(|source| {
                source
                    .revisions
                    .iter()
                    .map(|revision| revision.record.clone())
            }),
            before.config.policies.clone(),
            before.config.corpus.clone(),
        )
        .unwrap()
        .with_bindings(before.accepted_bindings.clone())
        .unwrap();
        let mut navigation = f::NavigationBudget::new(prepared.navigation);
        let error = corpus
            .read(
                &x.target.reference(f::Representation::Utf8),
                &warning_read_admission(&before),
                &mut navigation,
                per_call,
            )
            .unwrap_err();
        if cumulative {
            assert_eq!(error, f::FilesError::NavigationExhausted);
        } else {
            assert!(matches!(error, f::FilesError::ReadOverflow { .. }));
        }
        assert_eq!(
            navigation.used(),
            warning_read_usage(&before, &[&x.target], 0)
        );
        let response = warning_read_response(&x, &prepared, 1);
        assert_eq!(
            x.fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let after = x.fixture.app.snapshot().unwrap();
        assert_warning_read_unavailable(
            retained_warning_read(&after, &prepared, 0),
            &empty_warning_read(),
        );
        assert_warning_read_accounting(
            &before,
            &after,
            &prepared,
            warning_read_usage(&before, &[&x.target], 0),
        );
    }
}

#[test]
fn warning_read_staging_is_discarded_when_a_later_operation_rejects_the_decision() {
    let bytes = warning_read_payload_bytes(true);
    let mut configuration = warning_config();
    configuration.navigation_per_call.result_bytes = bytes;
    configuration.navigation_per_work.result_bytes = 5 * bytes;
    let mut x = warning_read_fixture(configuration, true);
    let (prepared, before) = prepare_warning_read(&mut x, "warning-read-rollback");
    assert_eq!(
        serde_json::to_vec(&warning_read_payload(&prepared))
            .unwrap()
            .len(),
        bytes
    );
    let mut response = warning_read_response(&x, &prepared, 2);
    let mut absent = reference(&x.basis);
    absent.source = "absent-assessment-source".into();
    response.operations.push(p::Operation::Assess {
        id: "reject-after-reads".into(),
        subject: absent,
        criteria: "Requires the exact available subject.".into(),
        evidence: Vec::new(),
        judgment: "Fixture proposal with an unavailable subject.".into(),
    });
    assert!(p::validate_decision(&response).is_ok());
    let retained = persisted_state(&x.fixture.app);
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Held
    );
    assert_eq!(persisted_state(&x.fixture.app), retained);
    let after = x.fixture.app.snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(!after.observations.values().any(|observation| {
        observation.invocation.as_deref() == Some(prepared.request.id.as_str())
    }));
    assert_eq!(
        x.fixture
            .app
            .invocation(&prepared.request.id)
            .unwrap()
            .state,
        s::InvocationState::Failed
    );
    // Preparation's attempt/navigation debit remains; neither staged reads nor
    // a fabricated actual-navigation accounting observation were adopted.
    assert_eq!(
        after.work_meta["work"].navigation_left,
        before.work_meta["work"].navigation_left
    );
    assert_eq!(
        after.work_meta["work"].attempts_left,
        before.work_meta["work"].attempts_left
    );
}

#[test]
fn failed_patch_requires_a_separate_funded_warning_decision_and_creates_no_warning() {
    let mut x = warning_fixture();
    let before = x.fixture.app.snapshot().unwrap().work_meta["work"].attempts_left;
    let prepared = x.fixture.prepare("warning-failed-patch");
    let mut response = warning_response(&prepared, adopt_operation(&x, false));
    response.operations.insert(
        0,
        write(
            "bad-save",
            p::Scope::Assets {},
            &x.target.source,
            "Unaccepted replacement.",
            Some("0".repeat(64)),
        ),
    );
    if let p::Operation::WarningAdopt { depends_on, .. } = &mut response.operations[1] {
        depends_on.push("bad-save".into());
    }
    if let p::Maintenance::Change { operation_ids, .. } = &mut response.asset_maintenance {
        operation_ids.insert(0, "bad-save".into());
    }
    assert!(p::validate_decision(&response).is_ok());
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Held
    );
    let state = x.fixture.app.snapshot().unwrap();
    assert!(state.semantic_warnings.is_empty());
    assert!(state.accepted_bindings.is_empty());
    assert_eq!(
        state.sources[&x.target.source].revisions[0].record,
        x.target
    );
    assert_eq!(state.work_meta["work"].attempts_left, before - 1);
    x.fixture.app.resume("operator", "a", "work").unwrap();
    accept_warning(&mut x, false);
    let state = x.fixture.app.snapshot().unwrap();
    assert_eq!(state.semantic_warnings.len(), 1);
    assert_eq!(
        state.sources[&x.target.source].revisions[0].record,
        x.target
    );
    assert_eq!(state.work_meta["work"].attempts_left, before - 2);
}

#[test]
fn a_failed_patch_without_warning_authorship_never_creates_a_warning() {
    let mut x = warning_fixture();
    let prepared = x.fixture.prepare("warning-no-automatic-adoption");
    let mut response = decision(&prepared, true);
    response.operations.push(write(
        "bad-save",
        p::Scope::Assets {},
        &x.target.source,
        "Unaccepted bytes.",
        Some("0".repeat(64)),
    ));
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Propose only a content change.".into(),
        operation_ids: vec!["bad-save".into()],
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Held
    );
    assert!(x
        .fixture
        .app
        .snapshot()
        .unwrap()
        .semantic_warnings
        .is_empty());
}

#[test]
fn whole_response_warning_protocol_is_closed_bounded_and_dependency_checked() {
    let mut x = warning_fixture();
    let prepared = x.fixture.prepare("warning-protocol");
    let response = warning_response(&prepared, adopt_operation(&x, false));
    let valid = serde_json::to_value(&response).unwrap();
    assert!(p::parse_decision(&serde_json::to_vec(&valid).unwrap(), 262_144).is_ok());
    assert!(p::decision_schema().to_string().contains("warning_adopt"));
    let mut bad = Vec::new();
    let mut value = valid.clone();
    value["operations"][0]
        .as_object_mut()
        .unwrap()
        .remove("basis");
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["ungranted_authority"] = json!(true);
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["targets"][0]["start"] = json!(1);
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["basis"][0]["end"] = json!(0);
    value["operations"][0]["basis"][0]["start"] = json!(0);
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["targets"][0]["revision"] = json!("proposal-op:missing");
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["basis"] = json!([reference(&x.basis), reference(&x.basis)]);
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["meaning"] = json!("x".repeat(4097));
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["depends_on"] = json!(["adopt-warning"]);
    bad.push(value);
    let mut value = valid.clone();
    value["operations"][0]["depends_on"] = json!(["missing"]);
    bad.push(value);
    let mut value = valid.clone();
    value["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"read","id":"new-read","reference":reference(&x.basis)}));
    value["operations"][0]["depends_on"] = json!(["new-read"]);
    bad.push(value);
    let mut value = valid.clone();
    let mut second = value["operations"][0].clone();
    second["id"] = json!("other-operation");
    value["operations"].as_array_mut().unwrap().push(second);
    value["asset_maintenance"]["operation_ids"] = json!(["adopt-warning", "other-operation"]);
    bad.push(value);
    let before = persisted_state(&x.fixture.app);
    for value in bad {
        assert!(x
            .fixture
            .app
            .submit_synthetic(
                &prepared.request.id,
                &serde_json::to_vec(&value).unwrap(),
                NOW
            )
            .is_err());
        assert_eq!(persisted_state(&x.fixture.app), before);
    }
    let mut trailing = serde_json::to_vec(&valid).unwrap();
    trailing.extend_from_slice(b" trailing");
    assert!(x
        .fixture
        .app
        .submit_synthetic(&prepared.request.id, &trailing, NOW)
        .is_err());
    assert_eq!(
        x.fixture
            .app
            .invocation(&prepared.request.id)
            .unwrap()
            .state,
        s::InvocationState::Prepared
    );
    assert!(x
        .fixture
        .app
        .snapshot()
        .unwrap()
        .semantic_warnings
        .is_empty());
}

#[test]
fn old_index_revision_read_and_current_context_keep_warning_after_content_change() {
    let mut x = warning_fixture();
    let index = record(
        "unchanged-index",
        f::Scope::Assets,
        &serde_json::to_string(&reference(&x.target)).unwrap(),
    );
    x.fixture
        .app
        .import_files("operator", "a", vec![index.clone()], false, NOW)
        .unwrap();
    accept_warning(&mut x, false);
    let prepared = x.fixture.prepare("warning-content-fix");
    assert!(context_has_source(&prepared, &x.target.source));
    assert!(context_has_source(&prepared, &x.qualification.source));
    assert_eq!(
        warning_control(&prepared)["semantic_warnings"][0]["active"],
        true
    );
    let mut response = decision(&prepared, true);
    response.operations.push(write(
        "new-content",
        p::Scope::Assets {},
        &x.target.source,
        "Different accepted subject bytes.",
        Some(x.target.revision.clone()),
    ));
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Change bytes without resolving the warning.".into(),
        operation_ids: vec!["new-content".into()],
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = x.fixture.prepare("warning-old-discovery");
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Search {
        id: "old-search".into(),
        scope: p::Scope::Assets {},
        query: "Exact historical subject bytes.".into(),
        regex: false,
        limit: 8,
    });
    response.operations.push(p::Operation::Read {
        id: "old-read".into(),
        reference: reference(&x.target),
    });
    response.operations.push(p::Operation::Read {
        id: "index-read".into(),
        reference: reference(&index),
    });
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = x.fixture.app.snapshot().unwrap();
    assert!(state.semantic_warnings["warning1"].active());
    assert!(state.semantic_warnings["warning1"].history.is_empty());
    assert_eq!(state.sources[&index.source].revisions[0].record, index);
    assert_eq!(
        state.sources[&x.target.source].revisions[0].record,
        x.target
    );
    assert_ne!(state.sources[&x.target.source].head, x.target.revision);
    let read = state
        .observations
        .values()
        .find(|observation| {
            observation.invocation.as_deref() == Some("warning-old-discovery")
                && observation.kind == "qualified_read"
                && observation.payload["result"]["entries"][0]["reference"]["source"]
                    == x.target.source
        })
        .unwrap();
    let entries = read.payload["result"]["entries"].as_array().unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry["reference"]["source"] == x.qualification.source));
    assert!(entries.iter().any(|entry| entry["reference"]["source"]
        .as_str()
        .unwrap()
        .starts_with("semantic-warning:")));
    let search = state
        .observations
        .values()
        .find(|observation| {
            observation.invocation.as_deref() == Some("warning-old-discovery")
                && observation.kind == "discovery"
        })
        .unwrap();
    let page: Value = serde_json::from_str(search.payload["page"].as_str().unwrap()).unwrap();
    assert!(page["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|hit| hit["reference"]["revision"] == x.target.revision));
    let prepared = x.fixture.prepare("warning-old-context");
    assert!(prepared.prompt.contains("Exact historical subject bytes."));
    assert!(prepared
        .prompt
        .contains("Explicit qualification supplied in full."));
}

#[test]
fn missing_or_denied_accompaniment_withholds_entire_bundle_without_removing_history() {
    for deny in [false, true] {
        let mut x = warning_fixture();
        accept_warning(&mut x, false);
        if deny {
            x.fixture
                .app
                .revoke_policy(
                    "operator",
                    x.qualification.policy.identity(),
                    "Fixture policy withdrawal.",
                )
                .unwrap();
        } else {
            x.fixture
                .app
                .withdraw_source(
                    "operator",
                    &x.qualification.source,
                    "Fixture source withdrawal.",
                )
                .unwrap();
        }
        let prepared = x.fixture.prepare("warning-unavailable");
        assert!(!context_has_source(&prepared, &x.target.source));
        assert!(!prepared.prompt.contains("Exact historical subject bytes."));
        assert!(!prepared
            .prompt
            .contains("Explicit qualification supplied in full."));
        let mut response = decision(&prepared, true);
        response.operations.push(p::Operation::Read {
            id: "denied-read".into(),
            reference: reference(&x.target),
        });
        assert_eq!(
            x.fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let state = x.fixture.app.snapshot().unwrap();
        assert!(state.semantic_warnings["warning1"].active());
        assert_eq!(state.accepted_bindings.len(), 1);
        let observation = state
            .observations
            .values()
            .find(|observation| {
                observation.invocation.as_deref() == Some("warning-unavailable")
                    && observation.kind == "qualified_read"
            })
            .unwrap();
        assert_eq!(observation.payload["status"], "unavailable");
        assert_eq!(observation.payload["text_supplied"], false);
        assert!(observation.payload.get("result").is_none());
        assert_eq!(observation.payload["semantic_warnings"], json!([]));
        assert!(observation.sources.is_empty());
        assert!(observation.qualified_roots.is_empty());
    }
}

#[test]
fn ineligible_target_does_not_hide_its_permitted_marker_or_supply_target_bytes() {
    let mut x = warning_fixture();
    accept_warning(&mut x, true);
    let prepared = x.fixture.prepare("warning-ineligible-context");
    assert!(!context_has_source(&prepared, &x.target.source));
    assert!(!prepared.prompt.contains("Exact historical subject bytes."));
    let control = warning_control(&prepared);
    assert_eq!(control["semantic_warnings"][0]["active"], true);
    assert_eq!(
        control["semantic_warnings"][0]["operation"]["targets"][0]["revision"],
        x.target.revision
    );
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Read {
        id: "ineligible-read".into(),
        reference: reference(&x.target),
    });
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = x.fixture.app.snapshot().unwrap();
    let observation = state
        .observations
        .values()
        .find(|observation| {
            observation.invocation.as_deref() == Some("warning-ineligible-context")
                && observation.kind == "qualified_read"
        })
        .unwrap();
    assert_eq!(observation.payload["text_supplied"], false);
    assert_eq!(
        observation.payload["semantic_warnings"][0]["id"],
        "warning1"
    );
    assert!(observation
        .sources
        .iter()
        .all(|source| source.source.starts_with("semantic-warning:")));
}

#[test]
fn authorized_resolution_is_explicit_chained_and_replay_cannot_restore_old_binding() {
    let mut x = warning_fixture();
    let (adoption, adoption_response) = accept_warning(&mut x, true);
    let genesis = x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"].clone();
    let prepared = x.fixture.prepare("warning-retain");
    let response = warning_response(
        &prepared,
        resolve_operation(&x, genesis.current_revision(), false),
    );
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let retained = x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"].clone();
    assert_eq!(retained.revision, genesis.revision);
    assert_eq!(retained.history.len(), 1);
    assert_ne!(retained.current_revision(), genesis.current_revision());
    let prepared = x.fixture.prepare("warning-stale-resolution");
    let response = warning_response(
        &prepared,
        resolve_operation(&x, genesis.current_revision(), true),
    );
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Held
    );
    assert_eq!(
        x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"]
            .history
            .len(),
        1
    );
    x.fixture.app.resume("operator", "a", "work").unwrap();
    let prepared = x.fixture.prepare("warning-release");
    let response = warning_response(
        &prepared,
        resolve_operation(&x, retained.current_revision(), true),
    );
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let released = x.fixture.app.snapshot().unwrap();
    assert!(!released.semantic_warnings["warning1"].active());
    assert_eq!(released.semantic_warnings["warning1"].history.len(), 2);
    let active_bindings = serde_json::to_value(&released.accepted_bindings).unwrap();
    assert!(
        matches!(&released.accepted_bindings[0].disposition, f::BindingDisposition::Qualify { required } if required.len() == 1)
    );
    assert_ne!(
        released.accepted_bindings[0].revision,
        genesis.bindings[0].revision
    );
    for (prepared, response) in [(&adoption, &adoption_response), (&prepared, &response)] {
        assert_eq!(
            x.fixture
                .app
                .adopt_response(
                    &prepared.request.id,
                    &serde_json::to_vec(response).unwrap(),
                    NOW
                )
                .unwrap()
                .status,
            AdoptionStatus::Adopted
        );
        assert_eq!(
            serde_json::to_value(x.fixture.app.snapshot().unwrap().accepted_bindings).unwrap(),
            active_bindings
        );
    }
    let mut conflicting = response.clone();
    if let p::Operation::WarningResolve { reason, .. } = &mut conflicting.operations[0] {
        *reason = "Conflicting replay content.".into();
    }
    assert!(x
        .fixture
        .app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&conflicting).unwrap(),
            NOW
        )
        .is_err());
    let next = x.fixture.prepare("warning-released-context");
    assert!(context_has_source(&next, &x.target.source));
    assert_eq!(
        warning_control(&next)["semantic_warnings"][0]["active"],
        false
    );
    let response = warning_response(
        &next,
        resolve_operation(
            &x,
            released.semantic_warnings["warning1"].current_revision(),
            false,
        ),
    );
    assert_eq!(x.fixture.run(&next, &response).status, AdoptionStatus::Held);
    assert_eq!(
        x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"]
            .history
            .len(),
        2
    );
}

#[test]
fn warning_survives_pause_resume_closeout_restart_and_fresh_work_authority() {
    let mut x = warning_fixture();
    accept_warning(&mut x, false);
    let original =
        serde_json::to_value(&x.fixture.app.snapshot().unwrap().semantic_warnings).unwrap();
    x.fixture
        .app
        .pause("operator", "work", "Pause correction work.")
        .unwrap();
    x.fixture.app.resume("operator", "a", "work").unwrap();
    assert_eq!(
        serde_json::to_value(&x.fixture.app.snapshot().unwrap().semantic_warnings).unwrap(),
        original
    );
    x.fixture
        .app
        .closeout("operator", "work", false, "End correction work unfinished.")
        .unwrap();
    let root = x.fixture.app.state_root().to_path_buf();
    x.fixture.app = Runtime::open(&root).unwrap();
    assert_eq!(
        serde_json::to_value(&x.fixture.app.snapshot().unwrap().semantic_warnings).unwrap(),
        original
    );
    assert!(x.fixture.app.resume("operator", "a", "work").is_err());
    let mut later = need();
    later.id = "later-work".into();
    later.authorization = "Fresh explicit local authority for reconsideration.".into();
    later.invitations[0].id = "later-offer".into();
    x.fixture.app.add_need("operator", later).unwrap();
    let prepared = x
        .fixture
        .app
        .prepare(
            PrepareRequest {
                id: "later-selection".into(),
                persona: "a".into(),
                work: "later-work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let mut response = decision(&prepared, false);
    response.continuation = p::Continuation::Stop {
        reason: "Select exact material without claiming accepted responsibility.".into(),
    };
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Explicitly reconsider the old exact revision.".into(),
        references: vec![reference(&x.target), reference(&x.basis)],
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    x.fixture.app.resume("operator", "a", "later-work").unwrap();
    let prepared = x
        .fixture
        .app
        .prepare(
            PrepareRequest {
                id: "later-resolution".into(),
                persona: "a".into(),
                work: "later-work".into(),
                profile: Profile::Synthetic,
                mode: Mode::Ordinary,
            },
            NOW,
        )
        .unwrap();
    let revision = warning_control(&prepared)["semantic_warnings"][0]["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut response = warning_response(&prepared, resolve_operation(&x, &revision, true));
    response.continuation = p::Continuation::Stop {
        reason: "No further continuation requested.".into(),
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let state = x.fixture.app.snapshot().unwrap();
    let warning = &state.semantic_warnings["warning1"];
    assert_eq!(warning.authored.work, "work");
    assert_eq!(warning.history[0].authored.work, "later-work");
    assert!(!warning.active());
}

#[test]
fn warning_adoption_and_resolution_persistence_failures_are_atomic() {
    let mut x = warning_fixture();
    let prepared = x.fixture.prepare("warning-persistence");
    let response = warning_response(&prepared, adopt_operation(&x, false));
    let before = persisted_state(&x.fixture.app);
    let connection = Connection::open(x.fixture.app.state_root().join("state.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_warning_fixture BEFORE UPDATE ON store_cells WHEN NEW.ns='object' AND NEW.k='runtime-state-v1' AND json_type(NEW.data,'$.body.semantic_warnings.warning1')='object' BEGIN SELECT RAISE(ABORT,'warning fixture rejection'); END;").unwrap();
    x.fixture
        .app
        .dispatch_synthetic(&prepared.request.id, NOW)
        .unwrap();
    let bytes = serde_json::to_vec(&response).unwrap();
    assert!(matches!(
        x.fixture
            .app
            .adopt_response(&prepared.request.id, &bytes, NOW),
        Err(RuntimeError::Store(s::StoreError::Sqlite(_)))
    ));
    assert_eq!(persisted_state(&x.fixture.app), before);
    assert_eq!(
        x.fixture
            .app
            .invocation(&prepared.request.id)
            .unwrap()
            .state,
        s::InvocationState::Responded
    );
    connection
        .execute_batch("DROP TRIGGER reject_warning_fixture;")
        .unwrap();
    assert_eq!(
        x.fixture
            .app
            .adopt_response(&prepared.request.id, &bytes, NOW)
            .unwrap()
            .status,
        AdoptionStatus::Adopted
    );
    let warning = x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"].clone();
    let prepared = x.fixture.prepare("warning-resolution-persistence");
    let response = warning_response(
        &prepared,
        resolve_operation(&x, warning.current_revision(), true),
    );
    let before = persisted_state(&x.fixture.app);
    connection.execute_batch("CREATE TRIGGER reject_warning_fixture BEFORE UPDATE ON store_cells WHEN NEW.ns='object' AND NEW.k='runtime-state-v1' AND json_array_length(NEW.data,'$.body.semantic_warnings.warning1.history')>0 BEGIN SELECT RAISE(ABORT,'resolution fixture rejection'); END;").unwrap();
    x.fixture
        .app
        .dispatch_synthetic(&prepared.request.id, NOW)
        .unwrap();
    let bytes = serde_json::to_vec(&response).unwrap();
    assert!(matches!(
        x.fixture
            .app
            .adopt_response(&prepared.request.id, &bytes, NOW),
        Err(RuntimeError::Store(s::StoreError::Sqlite(_)))
    ));
    assert_eq!(persisted_state(&x.fixture.app), before);
    connection
        .execute_batch("DROP TRIGGER reject_warning_fixture;")
        .unwrap();
    assert_eq!(
        x.fixture
            .app
            .adopt_response(&prepared.request.id, &bytes, NOW)
            .unwrap()
            .status,
        AdoptionStatus::Adopted
    );
    assert_eq!(
        x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"]
            .history
            .len(),
        1
    );
}

#[test]
fn warning_scope_ownership_character_and_stale_authority_fences_hold() {
    let mut ungranted = seed("a");
    ungranted.grants.assets = false;
    for (primary, owner) in [(ungranted, "a"), (seed("a"), "b")] {
        let mut x = warning_fixture_with(warning_config(), primary, owner);
        let prepared = x.fixture.prepare("warning-permission-fence");
        let response = warning_response(&prepared, adopt_operation(&x, false));
        assert_eq!(
            x.fixture.run(&prepared, &response).status,
            AdoptionStatus::Held
        );
        assert!(x
            .fixture
            .app
            .snapshot()
            .unwrap()
            .semantic_warnings
            .is_empty());
    }
    let mut x = warning_fixture();
    let voice = x.fixture.app.snapshot().unwrap().personas["a"]
        .character
        .as_ref()
        .unwrap()
        .own_voice[0]
        .clone();
    let prepared = x.fixture.prepare("warning-character-fence");
    let mut operation = adopt_operation(&x, false);
    if let p::Operation::WarningAdopt { scope, targets, .. } = &mut operation {
        *scope = p::Scope::Fragments {};
        *targets = vec![voice];
    }
    let mut response = warning_response(&prepared, operation);
    std::mem::swap(
        &mut response.fragment_maintenance,
        &mut response.asset_maintenance,
    );
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.unwrap().contains("protected sources"));
    assert!(x
        .fixture
        .app
        .snapshot()
        .unwrap()
        .semantic_warnings
        .is_empty());
    x.fixture.app.resume("operator", "a", "work").unwrap();
    let prepared = x.fixture.prepare("warning-stale-authority");
    let response = warning_response(&prepared, adopt_operation(&x, false));
    x.fixture
        .app
        .dispatch_synthetic(&prepared.request.id, NOW)
        .unwrap();
    x.fixture
        .app
        .pause("operator", "work", "Authority changed after dispatch.")
        .unwrap();
    assert_eq!(
        x.fixture
            .app
            .adopt_response(
                &prepared.request.id,
                &serde_json::to_vec(&response).unwrap(),
                NOW
            )
            .unwrap()
            .status,
        AdoptionStatus::Held
    );
    assert!(x
        .fixture
        .app
        .snapshot()
        .unwrap()
        .semantic_warnings
        .is_empty());
}

#[test]
fn another_primary_cannot_resolve_the_owners_warning() {
    let mut x = warning_fixture();
    accept_warning(&mut x, false);
    let revision = x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"]
        .current_revision()
        .to_owned();
    let mut other = need();
    other.id = "other-work".into();
    other.invitations[0].id = "other-offer".into();
    other.invitations[0].to = "b".into();
    x.fixture.app.add_need("operator", other).unwrap();
    let request = |id: &str| PrepareRequest {
        id: id.into(),
        persona: "b".into(),
        work: "other-work".into(),
        profile: Profile::Synthetic,
        mode: Mode::Ordinary,
    };
    let prepared = x
        .fixture
        .app
        .prepare(request("other-initialization"), NOW)
        .unwrap();
    let mut response = decision(&prepared, false);
    let (ocean, vad) = profiles();
    response.operations = vec![
        write(
            "other-voice-save",
            p::Scope::Fragments {},
            "other-voice",
            "I distinguish fixture evidence from judgments.",
            None,
        ),
        p::Operation::CharacterAdopt {
            id: "other-character".into(),
            base_revision: None,
            ocean,
            vad,
            own_voice: vec![proposed("other-voice", "other-voice-save")],
            reason: "Explicit bounded initialization.".into(),
            evidence: Vec::new(),
        },
    ];
    response.fragment_maintenance = p::Maintenance::Change {
        reason: "Initialize the exact account.".into(),
        operation_ids: vec!["other-voice-save".into(), "other-character".into()],
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::CharacterOnly
    );
    x.fixture.app.resume("operator", "b", "other-work").unwrap();
    let prepared = x
        .fixture
        .app
        .prepare(request("other-resolution"), NOW)
        .unwrap();
    let mut response = warning_response(&prepared, resolve_operation(&x, &revision, true));
    response.continuation = p::Continuation::Stop {
        reason: "Stop without claiming responsibility.".into(),
    };
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report.reason.unwrap().contains("original primary"));
    assert!(
        x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"]
            .history
            .is_empty()
    );
}

#[test]
fn warning_basis_must_be_actually_supplied_and_missing_accompaniment_cannot_be_adopted() {
    for missing_basis in [false, true] {
        let mut x = warning_fixture();
        let unobserved = record(
            "not-supplied",
            f::Scope::Assets,
            "Retained but never supplied as a body.",
        );
        x.fixture
            .app
            .import_files("operator", "a", vec![unobserved.clone()], false, NOW)
            .unwrap();
        let prepared = x.fixture.prepare("warning-basis-fence");
        let mut operation = adopt_operation(&x, false);
        if let p::Operation::WarningAdopt {
            basis, disposition, ..
        } = &mut operation
        {
            if missing_basis {
                *basis = vec![reference(&unobserved)];
            } else {
                let mut absent = reference(&unobserved);
                absent.source = "absent-source".into();
                *disposition = p::WarningDisposition::Qualify {
                    required: vec![absent],
                };
            }
        }
        let response = warning_response(&prepared, operation);
        assert_eq!(
            x.fixture.run(&prepared, &response).status,
            AdoptionStatus::Held
        );
        assert!(x
            .fixture
            .app
            .snapshot()
            .unwrap()
            .semantic_warnings
            .is_empty());
    }
}

#[test]
fn restricted_repair_rejects_both_warning_operations_before_dispatch() {
    let mut configuration = warning_config();
    configuration.max_repair_attempts = 2;
    configuration.navigation_per_call.scanned_bytes = 1024 * 1024 * 1024;
    configuration.navigation_per_call.inspected_revisions = 1_000_000;
    configuration.navigation_per_work.scanned_bytes = 7 * 1024 * 1024 * 1024;
    configuration.navigation_per_work.inspected_revisions = 8_000_000;
    // Optional bodies must exceed the repair-control overhead. The limit still
    // derives from an actual accepted selection; only the fixture content grows.
    let optional = "Optional historical fixture subject. ".repeat(256);
    let mut probe = warning_fixture_with_target(configuration.clone(), seed("a"), "a", &optional);
    let fitted = probe.fixture.app.snapshot().unwrap().personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap()
        .ordinary_bytes;
    drop(probe);
    configuration.context_bytes = fitted + 256;
    let mut x = warning_fixture_with_target(configuration, seed("a"), "a", &optional);
    x.fixture
        .app
        .record_human_evidence(
            "operator",
            "work",
            "repair-growth",
            &"x".repeat(2048),
            Vec::new(),
            BTreeSet::new(),
        )
        .unwrap();
    let ordinary = x.fixture.app.prepare(
        PrepareRequest {
            id: "warning-overflow".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    assert!(matches!(
        ordinary,
        Err(RuntimeError::SelectionOverflow { .. })
    ));
    let prepared = x
        .fixture
        .app
        .prepare(
            PrepareRequest {
                id: "warning-repair".into(),
                persona: "a".into(),
                work: "work".into(),
                profile: Profile::Synthetic,
                mode: Mode::SelectionRepair,
            },
            NOW,
        )
        .unwrap();
    let before = persisted_state(&x.fixture.app);
    for operation in [
        adopt_operation(&x, false),
        resolve_operation(&x, &"0".repeat(64), true),
    ] {
        let mut response = warning_response(&prepared, operation);
        response.continuation = p::Continuation::Stop {
            reason: "Restricted stop.".into(),
        };
        assert!(p::validate_decision(&response).is_ok());
        assert!(x
            .fixture
            .app
            .submit_synthetic(
                &prepared.request.id,
                &serde_json::to_vec(&response).unwrap(),
                NOW
            )
            .is_err());
        assert_eq!(persisted_state(&x.fixture.app), before);
        assert_eq!(
            x.fixture
                .app
                .invocation(&prepared.request.id)
                .unwrap()
                .state,
            s::InvocationState::Prepared
        );
    }
}

#[test]
fn model_evidence_rejects_same_response_qualification_change() {
    let mut x = warning_fixture();
    let prepared = x.fixture.prepare("register-before-model-warning");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Select the target and basis; the unused qualification is not selected.".into(),
        references: vec![reference(&x.target), reference(&x.basis)],
    };
    response.operations.push(effect(
        "register-model-warning-target",
        WorkAction::PutArtifact {
            artifact: "model-warning-artifact".into(),
            base: None,
            subject: reference(&x.target),
        },
        &[],
    ));
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = x.fixture.prepare("warn-and-record-model-evidence");
    assert!(!prepared
        .prompt
        .contains("Explicit qualification supplied in full."));
    let artifact: ArtifactKey = serde_json::from_value(
        interface_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
    )
    .unwrap();
    let mut response = warning_response(&prepared, adopt_operation(&x, false));
    response
        .operations
        .push(model_evidence_operation(&artifact));
    let before = persisted_state(&x.fixture.app);
    let result = x.fixture.run(&prepared, &response);
    assert_eq!(result.status, AdoptionStatus::Held);
    assert!(result
        .reason
        .unwrap()
        .contains("qualification bindings changed"));
    assert_eq!(persisted_state(&x.fixture.app), before);
}

#[test]
fn pending_read_supplies_new_warning_qualification_once_with_original_receipt() {
    let mut x = warning_fixture();
    let prepared = x.fixture.prepare("read-before-other-work-warning");
    let mut response = decision(&prepared, true);
    response.next_assets = p::SelectionIntent::Clear {
        reason: "Observe the exact read separately from optional selections.".into(),
    };
    response.operations.push(p::Operation::Read {
        id: "read-before-warning".into(),
        reference: reference(&x.target),
    });
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let before = x.fixture.app.snapshot().unwrap();
    let retained = before
        .observations
        .values()
        .find(|o| {
            o.invocation.as_deref() == Some("read-before-other-work-warning")
                && o.kind == "qualified_read"
        })
        .unwrap();
    assert_eq!(
        retained.payload["result"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mut other_need = need();
    other_need.id = "warning-work".into();
    x.fixture.app.add_need("operator", other_need).unwrap();
    let prepare_other = |fixture: &mut Fixture, id: &str| {
        fixture
            .app
            .prepare(
                PrepareRequest {
                    id: id.into(),
                    persona: "a".into(),
                    work: "warning-work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap()
    };
    let prepared = prepare_other(&mut x.fixture, "prime-other-work-warning");
    let mut response = decision(&prepared, false);
    let control = interface_control(&prepared);
    let offer = &control["own_pending_offers"][0];
    response.operations.push(p::Operation::Responsibility {
        id: offer["id"].as_str().unwrap().into(),
        work_id: "warning-work".into(),
        terms_revision: offer["terms_revision"].as_str().unwrap().into(),
        disposition: "accept".into(),
        reason: "Accept this independent synthetic maintenance responsibility.".into(),
    });
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Supply the target and warning basis in this independent work.".into(),
        references: vec![
            reference(&x.target),
            reference(&x.basis),
            reference(&x.qualification),
        ],
    };
    assert_eq!(
        x.fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let prepared = prepare_other(&mut x.fixture, "adopt-other-work-warning");
    let response = warning_response(&prepared, adopt_operation(&x, false));
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");

    let prepared = x.fixture.prepare("recheck-pending-read-warning");
    let control = warning_control(&prepared);
    let observation = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == retained.id)
        .unwrap();
    let entries = observation["payload"]["result"]["entries"]
        .as_array()
        .unwrap();
    for source in [&x.target, &x.qualification] {
        assert_eq!(
            entries
                .iter()
                .filter(
                    |e| e["reference"] == json!(source.reference(f::Representation::Utf8))
                        && e["text"] == std::str::from_utf8(&source.bytes).unwrap()
                )
                .count(),
            1
        );
    }
    let target = entries
        .iter()
        .find(|e| e["reference"]["source"] == x.target.source)
        .unwrap();
    assert!(!target["accepted_bindings"].as_array().unwrap().is_empty());
    assert!(!target["binding_restrictions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(entries.iter().any(|e| e["reference"]["source"]
        .as_str()
        .unwrap()
        .starts_with("semantic-warning:")));
    assert!(observation["current_required_qualifications"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        observation["payload"]["read_receipt"]["manifest_sha256"],
        retained.payload["result"]["manifest_sha256"]
    );
    assert_eq!(
        serde_json::to_value(&x.fixture.app.snapshot().unwrap().observations[&retained.id])
            .unwrap(),
        serde_json::to_value(retained).unwrap()
    );
}

// Leave a successful Read pending while an independent responsibility changes
// its warning accompaniment through the public adoption/resolution path.
fn pending_read_after_warning_release(
    repair: bool,
    replacement: bool,
) -> (WarningFixture, Observation) {
    let mut configuration = repair_config(1);
    // Fund the host's conservative traversal envelope for the retained warning
    // history and the independent work; these are explicit fixture allowances.
    configuration.navigation_per_call.scanned_bytes = 900 * 1024 * 1024;
    configuration.navigation_per_call.inspected_revisions = 8_000_000;
    configuration.navigation_per_work.scanned_bytes = 7 * 1024 * 1024 * 1024;
    configuration.navigation_per_work.inspected_revisions = 64_000_000;
    configuration.response_bytes = 8 * 1024;
    configuration.navigation_per_call.result_bytes = 8 * 1024;
    configuration.policies.active_policies.extend(
        warning_config()
            .policies
            .active_policies
            .into_iter()
            .filter(|p| p.id == "qualification-policy"),
    );
    let mut x = warning_fixture_with(configuration, seed("a"), "a");
    accept_warning(&mut x, false);
    let large = record(
        "read-accounting-large",
        f::Scope::Assets,
        &"L".repeat(60_000),
    );
    x.fixture
        .app
        .import_files("operator", "a", vec![large.clone()], false, NOW)
        .unwrap();
    let prepared = x.fixture.prepare("read-before-warning-release");
    let mut response = decision(&prepared, true);
    response.next_assets = if repair {
        p::SelectionIntent::Replace {
            reason: "Keep optional bodies whose supply receipt can be checked after repair.".into(),
            references: vec![reference(&large), reference(&x.qualification)],
        }
    } else {
        p::SelectionIntent::Clear {
            reason: "Inspect only the pending Read and artifact metadata next time.".into(),
        }
    };
    response.operations.push(effect(
        "register-old-qualification",
        WorkAction::PutArtifact {
            artifact: "old-qualification-artifact".into(),
            base: None,
            subject: reference(&x.qualification),
        },
        &[],
    ));
    response.operations.push(p::Operation::Read {
        id: "pending-read-before-release".into(),
        reference: reference(&x.target),
    });
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let retained = x
        .fixture
        .app
        .snapshot()
        .unwrap()
        .observations
        .values()
        .find(|o| {
            o.invocation.as_deref() == Some("read-before-warning-release")
                && o.kind == "qualified_read"
        })
        .unwrap()
        .clone();
    assert!(retained
        .sources
        .contains(&x.qualification.reference(f::Representation::Utf8)));

    let mut other_need = need();
    other_need.id = "warning-release-work".into();
    x.fixture.app.add_need("operator", other_need).unwrap();
    let prepare_other = |fixture: &mut Fixture, id: &str| {
        fixture
            .app
            .prepare(
                PrepareRequest {
                    id: id.into(),
                    persona: "a".into(),
                    work: "warning-release-work".into(),
                    profile: Profile::Synthetic,
                    mode: Mode::Ordinary,
                },
                NOW,
            )
            .unwrap()
    };
    let prepared = prepare_other(&mut x.fixture, "accept-warning-release-work");
    let mut response = decision(&prepared, false);
    let control = warning_control(&prepared);
    let offer = &control["own_pending_offers"][0];
    response.operations.push(p::Operation::Responsibility {
        id: offer["id"].as_str().unwrap().into(),
        work_id: "warning-release-work".into(),
        terms_revision: offer["terms_revision"].as_str().unwrap().into(),
        disposition: "accept".into(),
        reason: "Accept the independent synthetic warning-maintenance responsibility.".into(),
    });
    response.next_assets = p::SelectionIntent::Replace {
        reason: "Supply the exact subject and resolution basis.".into(),
        references: vec![reference(&x.target), reference(&x.basis)],
    };
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    let prepared = prepare_other(&mut x.fixture, "release-other-work-warning");
    let revision = x.fixture.app.snapshot().unwrap().semantic_warnings["warning1"]
        .current_revision()
        .to_owned();
    let response = warning_response(&prepared, resolve_operation(&x, &revision, true));
    let report = x.fixture.run(&prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");

    if replacement {
        let new_qualification = record(
            "replacement-qualification",
            f::Scope::Assets,
            "Fresh replacement qualification supplied in full.",
        );
        x.fixture
            .app
            .import_files("operator", "a", vec![new_qualification.clone()], false, NOW)
            .unwrap();
        let prepared = prepare_other(&mut x.fixture, "replace-other-work-warning");
        let mut operation = adopt_operation(&x, false);
        if let p::Operation::WarningAdopt {
            warning_id,
            disposition,
            ..
        } = &mut operation
        {
            *warning_id = "replacement-warning".into();
            *disposition = p::WarningDisposition::Qualify {
                required: vec![reference(&new_qualification)],
            };
        }
        let response = warning_response(&prepared, operation);
        let report = x.fixture.run(&prepared, &response);
        assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
    }
    (x, retained)
}

fn assert_released_read_body(
    x: &mut WarningFixture,
    retained: &Observation,
    prepared: &Prepared,
    replacement: bool,
) -> Vec<f::Reference> {
    let control = warning_control(prepared);
    let observation = control["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == retained.id)
        .unwrap();
    let entries = observation["payload"]["result"]["entries"]
        .as_array()
        .unwrap();
    assert!(entries
        .iter()
        .any(|e| e["reference"]["source"] == x.target.source));
    assert!(!entries
        .iter()
        .any(|e| e["reference"]["source"] == x.qualification.source));
    assert!(!prepared
        .prompt
        .contains(std::str::from_utf8(&x.qualification.bytes).unwrap()));
    assert_eq!(
        entries
            .iter()
            .any(|e| e["reference"]["source"] == "replacement-qualification"),
        replacement,
    );
    assert_eq!(
        observation["payload"]["read_receipt"]["manifest_sha256"],
        retained.payload["result"]["manifest_sha256"],
    );
    assert!(observation["current_required_qualifications"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        serde_json::to_value(&x.fixture.app.snapshot().unwrap().observations[&retained.id])
            .unwrap(),
        serde_json::to_value(retained).unwrap(),
    );
    // Removing an old body from projection must not erase confidentiality lineage.
    assert!(prepared
        .exposure
        .as_ref()
        .unwrap()
        .sources
        .contains(&x.qualification.reference(f::Representation::Utf8)));
    entries
        .iter()
        .map(|e| serde_json::from_value(e["reference"].clone()).unwrap())
        .collect()
}

#[test]
fn released_pending_read_repair_receipts_only_claim_current_bodies() {
    for replacement in [false, true] {
        let (mut x, retained) = pending_read_after_warning_release(true, replacement);
        let accepted = x.fixture.app.snapshot().unwrap();
        let fit = accepted.personas["a"].focus["work"]
            .selection_fit
            .as_ref()
            .unwrap();
        let growth = accepted.config.context_bytes - fit.ordinary_bytes + 8192;
        for index in 0..growth.div_ceil(8192) {
            x.fixture
                .app
                .record_human_evidence(
                    "operator",
                    "work",
                    &format!("read-accounting-growth-{index}"),
                    &"Observed mandatory evidence. ".repeat(300),
                    Vec::new(),
                    set(&["result"]),
                )
                .unwrap();
        }
        let result = x.fixture.app.prepare(
            PrepareRequest {
                mode: Mode::Ordinary,
                ..repair_request("released-read-overflow")
            },
            NOW,
        );
        assert!(
            matches!(result, Err(RuntimeError::SelectionOverflow { .. })),
            "{result:?}"
        );
        let prepared = x
            .fixture
            .app
            .prepare(repair_request("released-read-repair"), NOW)
            .unwrap();
        let fresh = assert_released_read_body(&mut x, &retained, &prepared, replacement);
        let control = warning_control(&prepared);
        let repair = &control["selection_repair"];
        let mandatory: BTreeSet<f::Reference> =
            serde_json::from_value(repair["mandatory_body_references"].clone()).unwrap();
        let mut expected: BTreeSet<_> = fresh.into_iter().collect();
        expected.extend(
            warning_context(&prepared)["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| serde_json::from_value::<f::Reference>(e["reference"].clone()).unwrap()),
        );
        assert_eq!(
            mandatory, expected,
            "replacement={replacement}: receipts must describe rendered bodies only"
        );
        let old = repair["body_receipt"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["reference"] == json!(x.qualification.reference(f::Representation::Utf8)))
            .unwrap();
        assert_eq!(old["status"], "withheld_optional_body");
        assert_eq!(old["independently_supplied_ranges"], json!([]));

        x.fixture
            .app
            .revoke_policy(
                "operator",
                x.qualification.policy.identity(),
                "The omitted old body still carries current confidentiality restrictions.",
            )
            .unwrap();
        assert!(x
            .fixture
            .app
            .dispatch_synthetic(&prepared.request.id, NOW)
            .is_err());
    }
}

#[test]
fn released_pending_read_does_not_authorize_omitted_body_as_evidence() {
    for warning_basis in [false, true] {
        let (mut x, retained) = pending_read_after_warning_release(false, false);
        let prepared = x.fixture.prepare("released-read-evidence-attempt");
        assert_released_read_body(&mut x, &retained, &prepared, false);
        let response = if warning_basis {
            let mut operation = adopt_operation(&x, false);
            if let p::Operation::WarningAdopt {
                warning_id, basis, ..
            } = &mut operation
            {
                *warning_id = "omitted-body-basis-warning".into();
                *basis = vec![reference(&x.qualification)];
            }
            warning_response(&prepared, operation)
        } else {
            let key: ArtifactKey = serde_json::from_value(
                warning_control(&prepared)["work_artifacts"][0]["artifact"].clone(),
            )
            .unwrap();
            let mut response = decision(&prepared, true);
            response.operations.push(model_evidence_operation(&key));
            response
        };
        let before = persisted_state(&x.fixture.app);
        let result = x.fixture.run(&prepared, &response);
        assert_eq!(result.status, AdoptionStatus::Held, "{result:?}");
        assert!(result.reason.unwrap().contains(if warning_basis {
            "warning basis was not an exact body supplied"
        } else {
            "actually supplied"
        }));
        assert_eq!(persisted_state(&x.fixture.app), before);
        assert!(x.fixture.app.snapshot().unwrap().work_meta["work"]
            .model_evidence
            .is_empty());
    }
}
