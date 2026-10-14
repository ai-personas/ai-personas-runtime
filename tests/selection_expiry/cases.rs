use super::*;

#[test]
fn rejected_next_call_expiry_retains_growth_repair_witness_across_restart() {
    expiry_growth_repair_fixture(false);
}

#[test]
fn retained_expiry_checkpoint_retains_growth_repair_witness_across_restart() {
    expiry_growth_repair_fixture(true);
}

// Only the synthetic fixture uses Store authority to recreate an older checkpoint.
// Production migration changes witness hashes, never selection or admission state.
fn advance_retained_expiry_checkpoint(f: &Fixture) {
    let root = f.app.state_root();
    let mut host = s::Store::open(&root.join("state.sqlite3"), storage_limits()).unwrap();
    let mut old = closeout_head(root);
    let serial = old.body["personas"]["a"]["admission_serial"]
        .as_u64()
        .unwrap();
    let deadline = old.body["personas"]["a"]["focus"]["work"]["assets"][1]["last_admission"]
        .as_u64()
        .unwrap();
    assert_eq!(serial, deadline);
    old.body["personas"]["a"]["admission_serial"] = json!(serial + 1);
    old.revision += 1;
    host.compare_and_swap(Some(old.revision - 1), old).unwrap();
}

fn expiry_growth_repair_fixture(retained_checkpoint: bool) {
    let mut f = Fixture::with_config(true, repair_config(1));
    f.accept();
    let persistent = record("expiry-persistent", f::Scope::Assets, &"P".repeat(90_000));
    let once = record(
        "expiry-once",
        f::Scope::Assets,
        "EXACT_EXPIRED_NEXT_CALL_BODY",
    );
    f.app
        .import_files(
            "operator",
            "a",
            vec![persistent.clone(), once.clone()],
            true,
            NOW,
        )
        .unwrap();
    let prepared = f.prepare("expiry-select");
    let mut d = decision(&prepared, true);
    let mut one_call = reference(&once);
    one_call.lifetime = Some("next_call".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Retain one persistent entry and one entry for the next admission only.".into(),
        references: vec![reference(&persistent), one_call],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let initial = f.app.snapshot().unwrap();
    let accepted_fit = initial.personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap()
        .clone();
    assert!(accepted_fit.ordinary_bytes <= initial.config.context_bytes);
    let consumed = f.prepare("expiry-rejected");
    assert!(consumed.prompt.contains("EXACT_EXPIRED_NEXT_CALL_BODY"));
    let after_admission = f.app.snapshot().unwrap();
    // Rejection leaves the old exact fit witness in place. The next preparation
    // must verify the mechanically expired subset, not assume successful adoption.
    assert_eq!(
        after_admission.personas["a"].focus["work"]
            .selection_fit
            .as_ref()
            .unwrap()
            .selection_sha256,
        accepted_fit.selection_sha256
    );
    let stored_selection =
        serde_json::to_value(&after_admission.personas["a"].focus["work"].assets).unwrap();
    let mut rejected = decision(&consumed, true);
    rejected.epoch += 1;
    assert_eq!(f.run(&consumed, &rejected).status, AdoptionStatus::Held);
    assert_eq!(
        f.app.invocation("expiry-rejected").unwrap().state,
        s::InvocationState::Failed
    );
    if retained_checkpoint {
        advance_retained_expiry_checkpoint(&f);
    }
    let root = f.app.state_root().to_path_buf();
    drop(f.app);
    f.app = Runtime::open(&root).unwrap();
    f.app.resume("operator", "a", "work").unwrap();
    let growth = initial.config.context_bytes - accepted_fit.ordinary_bytes + 16_384;
    for index in 0..growth.div_ceil(8192) {
        f.app
            .record_human_evidence(
                "operator",
                "work",
                &format!("expiry-growth-{index}"),
                &"Actual synthetic mandatory observation. ".repeat(220),
                Vec::new(),
                set(&["result"]),
            )
            .unwrap();
    }
    let before = f.app.snapshot().unwrap();
    let failure = f.app.prepare(
        PrepareRequest {
            id: "expiry-growth-overflow".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    assert!(
        matches!(failure, Err(RuntimeError::SelectionOverflow { .. })),
        "{failure:?}"
    );
    let after = f.app.snapshot().unwrap();
    assert!(
        after.personas["a"].focus["work"].repair_trigger.is_some(),
        "Expired next_call entry must not destroy the surviving selection's original fit witness"
    );
    assert_eq!(
        serde_json::to_value(&after.personas["a"].focus["work"].assets).unwrap(),
        stored_selection
    );
    assert_eq!(
        before.personas["a"].admission_serial,
        after.personas["a"].admission_serial
    );
    assert_eq!(
        serde_json::to_value(&before.personas["a"].focus["work"].selection_fit).unwrap(),
        serde_json::to_value(&after.personas["a"].focus["work"].selection_fit).unwrap()
    );
    assert_eq!(
        before.personas["a"].owner_epoch,
        after.personas["a"].owner_epoch
    );
    assert_eq!(
        before.work_meta["work"].attempts_left,
        after.work_meta["work"].attempts_left
    );
    assert_eq!(after.work_meta["work"].repair_attempts_left, 1);
    let repair = f
        .app
        .prepare(repair_request("expiry-clear-repair"), NOW)
        .unwrap();
    assert!(!repair.prompt.contains("EXACT_EXPIRED_NEXT_CALL_BODY"));
    assert!(!repair
        .repair_read_references
        .iter()
        .any(|r| r.source == once.source));
    assert_eq!(
        f.run(&repair, &repair_decision(&repair, false)).status,
        AdoptionStatus::Adopted
    );
    let repaired = f.app.snapshot().unwrap();
    assert_eq!(repaired.work_meta["work"].repair_attempts_left, 0);
    assert!(!repaired.personas["a"].focus["work"].permit);
    assert!(repaired.personas["a"].focus["work"]
        .assets
        .as_ref()
        .unwrap()
        .is_empty());
    f.app.resume("operator", "a", "work").unwrap();
    let fresh = f.prepare("expiry-after-repair");
    assert!(!fresh.prompt.contains("EXACT_EXPIRED_NEXT_CALL_BODY"));
    assert!(!fresh.prompt.contains(&"P".repeat(1000)));
    assert!(fresh
        .prompt
        .contains("Actual synthetic mandatory observation."));
}

fn selected_expiry_fixture() -> (Fixture, SelectionFit) {
    let mut f = Fixture::with_config(true, repair_config(1));
    f.accept();
    let once = record("independent-once", f::Scope::Assets, "ONE CALL ONLY");
    let persistent = record("independent-persistent", f::Scope::Assets, "PERSISTENT");
    f.app
        .import_files(
            "operator",
            "a",
            vec![once.clone(), persistent.clone()],
            true,
            NOW,
        )
        .unwrap();
    let prepared = f.prepare("independent-select");
    let mut d = decision(&prepared, true);
    let mut exact = reference(&once);
    exact.lifetime = Some("next_call".into());
    d.next_assets = p::SelectionIntent::Replace {
        reason: "Explicit independent synthetic selection.".into(),
        references: vec![reference(&persistent), exact],
    };
    assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
    let fit = f.app.snapshot().unwrap().personas["a"].focus["work"]
        .selection_fit
        .clone()
        .unwrap();
    (f, fit)
}

#[test]
fn retained_expiry_atomic_invocation_failure_preserves_entire_runtime_state() {
    let (mut f, fit) = selected_expiry_fixture();
    let used = f.prepare("independent-consumed");
    assert!(used.prompt.contains("ONE CALL ONLY"));
    let mut rejected = decision(&used, true);
    rejected.epoch += 1;
    assert_eq!(f.run(&used, &rejected).status, AdoptionStatus::Held);
    advance_retained_expiry_checkpoint(&f);
    f.app.resume("operator", "a", "work").unwrap();
    let before = persisted_state(&f.app);
    assert_eq!(
        before["personas"]["a"]["focus"]["work"]["selection_fit"]["selection_sha256"],
        fit.selection_sha256
    );
    let fault = Connection::open(f.app.state_root().join("state.sqlite3")).unwrap();
    fault.execute_batch("CREATE TRIGGER independent_expiry_fault BEFORE INSERT ON store_cells WHEN NEW.ns='invocation' AND NEW.k='independent-expiry-atomic-fail' BEGIN SELECT RAISE(ABORT,'independent-expiry-reservation-failure'); END;").unwrap();
    let outcome = f.app.prepare(
        PrepareRequest {
            id: "independent-expiry-atomic-fail".into(),
            persona: "a".into(),
            work: "work".into(),
            profile: Profile::Synthetic,
            mode: Mode::Ordinary,
        },
        NOW,
    );
    assert!(outcome
        .unwrap_err()
        .to_string()
        .contains("independent-expiry-reservation-failure"));
    assert_eq!(before, persisted_state(&f.app), "Failed atomic admission must not persist the normalized witness, serial, ownership, attempt or navigation state.");
    assert!(f.app.invocation("independent-expiry-atomic-fail").is_err());
    fault
        .execute_batch("DROP TRIGGER independent_expiry_fault")
        .unwrap();
    let fresh = f.prepare("independent-expiry-after-fault");
    assert!(!fresh.prompt.contains("ONE CALL ONLY"));
    let after = f.app.snapshot().unwrap();
    let normalized = after.personas["a"].focus["work"]
        .selection_fit
        .as_ref()
        .unwrap();
    assert_ne!(normalized.selection_sha256, fit.selection_sha256);
    assert_eq!(
        (
            normalized.growth_bytes,
            normalized.fixed_contract_bytes,
            normalized.ordinary_bytes
        ),
        (
            fit.growth_bytes,
            fit.fixed_contract_bytes,
            fit.ordinary_bytes
        )
    );
}
