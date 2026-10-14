//! Test-first contract for native work commands. All stores are newly created
//! synthetic fixtures. Raw JSON keeps these tests compilable before the new
//! Operation variant exists; positive parse assertions deliberately fail first.
use super::*;

const BODY: &str = "Exact synthetic artifact. Literal \\n and λ.";

fn registration(prepared: &Prepared) -> Value {
    let mut response = decision(prepared, true);
    response.asset_maintenance = p::Maintenance::Change {
        reason: "Retain the exact authored asset.".into(),
        operation_ids: vec!["save".into()],
    };
    response.operations = vec![
        write("save", p::Scope::Assets {}, "native-source", BODY, None),
        effect(
            "register",
            WorkAction::PutArtifact {
                artifact: "native-artifact".into(),
                base: None,
                subject: proposed("native-source", "save"),
            },
            &["save"],
        ),
    ];
    serde_json::to_value(response).unwrap()
}

fn native(mut response: Value) -> Value {
    for operation in response["operations"].as_array_mut().unwrap() {
        if operation["kind"] == "effect" {
            operation["kind"] = json!("work_command");
            operation.as_object_mut().unwrap().remove("capability_id");
            operation
                .as_object_mut()
                .unwrap()
                .remove("binding_revision");
        }
    }
    response
}

fn require_native_parser(response: &Value) {
    let parsed = p::parse_decision(
        &serde_json::to_vec(response).unwrap(),
        config().response_bytes,
    );
    assert!(
        parsed.is_ok(),
        "native work_command must be supported before testing its guards: {parsed:?}"
    );
}

fn run_raw(f: &mut Fixture, prepared: &Prepared, response: &Value) -> AdoptionReport {
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    f.app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(response).unwrap(),
            NOW,
        )
        .unwrap()
}

fn assert_artifact(f: &mut Fixture, prepared: &Prepared, response: &Value) {
    let raw = serde_json::to_vec(response).unwrap();
    let hash = s::sha256_bytes(&raw);
    assert_eq!(
        f.app
            .invocation(&prepared.request.id)
            .unwrap()
            .response_hash,
        Some(hash)
    );
    let terminal = f.app.terminal_evidence(&prepared.request.id).unwrap();
    assert_eq!(
        terminal.body["response_body"]["text"],
        String::from_utf8(raw.clone()).unwrap()
    );
    let state = f.app.snapshot().unwrap();
    let work = state.board.work(&w::WorkId("work".into())).unwrap();
    let artifact = &work.artifacts[&w::ArtifactId("native-artifact".into())];
    assert_eq!(artifact.revisions[&artifact.current].bytes, BODY.as_bytes());
    assert_eq!(
        state.sources["native-source"]
            .revisions
            .last()
            .unwrap()
            .record
            .bytes,
        BODY.as_bytes()
    );
    let before = persisted_state(&f.app);
    let replay = f
        .app
        .adopt_response(&prepared.request.id, &raw, NOW)
        .unwrap();
    assert_eq!(replay.status, AdoptionStatus::Adopted);
    assert_eq!(persisted_state(&f.app), before);
    let mut changed = response.clone();
    changed["work"]["reason"] = json!("Different bytes cannot rewrite adopted history.");
    assert!(f
        .app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&changed).unwrap(),
            NOW
        )
        .is_err());
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn legacy_exact_effect_retains_artifact_raw_provenance_and_replay() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("legacy-exact");
    let response = registration(&prepared);
    p::parse_decision(
        &serde_json::to_vec(&response).unwrap(),
        config().response_bytes,
    )
    .unwrap();
    assert_eq!(
        run_raw(&mut f, &prepared, &response).status,
        AdoptionStatus::Adopted
    );
    assert_artifact(&mut f, &prepared, &response);
}

#[test]
fn legacy_short_or_wrong_binding_never_rescues_or_partially_applies() {
    for binding in [work_binding()[..63].to_owned(), "f".repeat(64)] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("legacy-stale");
        let mut response = registration(&prepared);
        response["operations"][1]["binding_revision"] = json!(binding);
        response["operations"].as_array_mut().unwrap().push(json!({
            "kind":"communicate", "id":"message", "recipient":"b",
            "subject":"Atomic tail", "text":"Must not be delivered."
        }));
        p::parse_decision(
            &serde_json::to_vec(&response).unwrap(),
            config().response_bytes,
        )
        .unwrap();
        let before = persisted_state(&f.app);
        let report = run_raw(&mut f, &prepared, &response);
        assert_eq!(report.status, AdoptionStatus::Held);
        assert!(report
            .reason
            .unwrap()
            .contains("unknown, unimplemented, or stale capability"));
        assert!(report.applied.is_empty());
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn native_exact_command_retains_artifact_raw_provenance_and_replay() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("native-exact");
    let response = native(registration(&prepared));
    require_native_parser(&response);
    assert_eq!(
        run_raw(&mut f, &prepared, &response).status,
        AdoptionStatus::Adopted
    );
    assert_artifact(&mut f, &prepared, &response);
}

#[test]
fn mixed_native_and_malformed_legacy_is_atomic_and_never_repaired() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("mixed-stale");
    let mut response = native(registration(&prepared));
    require_native_parser(&response);
    let mut legacy = registration(&prepared)["operations"][1].clone();
    legacy["id"] = json!("stale-legacy");
    legacy["binding_revision"] = json!(&work_binding()[..63]);
    response["operations"].as_array_mut().unwrap().push(legacy);
    p::parse_decision(
        &serde_json::to_vec(&response).unwrap(),
        config().response_bytes,
    )
    .unwrap();
    let before = persisted_state(&f.app);
    let report = run_raw(&mut f, &prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(report
        .reason
        .unwrap()
        .contains("unknown, unimplemented, or stale capability"));
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn native_wire_is_strict_not_an_alias_for_malformed_effects() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("native-wire");
    let response = native(registration(&prepared));
    require_native_parser(&response);
    for field in ["capability_id", "binding_revision", "unexpected"] {
        let mut invalid = response.clone();
        invalid["operations"][1][field] = json!("not allowed");
        assert!(
            p::parse_decision(
                &serde_json::to_vec(&invalid).unwrap(),
                config().response_bytes
            )
            .is_err(),
            "{field}"
        );
    }
    for missing in ["id", "inputs", "depends_on"] {
        let mut invalid = response.clone();
        invalid["operations"][1]
            .as_object_mut()
            .unwrap()
            .remove(missing);
        assert!(
            p::parse_decision(
                &serde_json::to_vec(&invalid).unwrap(),
                config().response_bytes
            )
            .is_err(),
            "{missing}"
        );
    }
    for invalid_op in [
        json!(["register", {"kind":"put_artifact"}, ["save"]]),
        json!({"kind":"work_command", "id":"register", "inputs":[], "depends_on":["save"]}),
        json!({"kind":"work_command", "id":"register", "inputs":{}, "depends_on":"save"}),
    ] {
        let mut invalid = response.clone();
        invalid["operations"][1] = invalid_op;
        assert!(p::parse_decision(
            &serde_json::to_vec(&invalid).unwrap(),
            config().response_bytes
        )
        .is_err());
    }
    let mut cycle = response.clone();
    cycle["operations"][1]["depends_on"] = json!(["other-command"]);
    let mut other = cycle["operations"][1].clone();
    other["id"] = json!("other-command");
    other["depends_on"] = json!(["register"]);
    cycle["operations"].as_array_mut().unwrap().push(other);
    assert!(p::parse_decision(
        &serde_json::to_vec(&cycle).unwrap(),
        config().response_bytes
    )
    .is_err());
    let mut deep = response.clone();
    let mut nested = json!(null);
    for _ in 0..128 {
        nested = json!({"nested":nested});
    }
    deep["operations"][1]["inputs"]["nested"] = nested;
    assert!(
        p::parse_decision(&serde_json::to_vec(&deep).unwrap(), config().response_bytes).is_err()
    );
    let raw = serde_json::to_string(&response).unwrap();
    let duplicate = raw.replacen(
        "\"kind\":\"work_command\"",
        "\"kind\":\"work_command\",\"kind\":\"work_command\"",
        1,
    );
    assert_ne!(duplicate, raw);
    assert!(p::parse_decision(duplicate.as_bytes(), config().response_bytes).is_err());
    assert!(p::parse_decision(raw.as_bytes(), raw.len() - 1).is_err());
    let mut invalid = response.clone();
    invalid["operations"][1]["id"] = json!("save");
    assert!(p::parse_decision(
        &serde_json::to_vec(&invalid).unwrap(),
        config().response_bytes
    )
    .is_err());
    let mut invalid = response;
    invalid["asset_maintenance"]["operation_ids"] = json!(["save", "register"]);
    assert!(
        p::parse_decision(
            &serde_json::to_vec(&invalid).unwrap(),
            config().response_bytes
        )
        .is_err(),
        "work commands are not asset-maintenance writes"
    );
}

#[test]
fn native_dependencies_keep_protocol_and_runtime_boundaries() {
    for fault in [
        "unknown",
        "self",
        "missing-save",
        "not-save",
        "wrong-source",
        "range",
        "representation",
        "unknown-input",
        "subject-array",
    ] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("native-dependency");
        let mut response = native(registration(&prepared));
        require_native_parser(&response);
        match fault {
            "unknown" => response["operations"][1]["depends_on"] = json!(["absent"]),
            "self" => response["operations"][1]["depends_on"] = json!(["register"]),
            "missing-save" => response["operations"][1]["depends_on"] = json!([]),
            "not-save" => {
                response["operations"].as_array_mut().unwrap().insert(1, json!({"kind":"communicate", "id":"message", "recipient":"b", "subject":"Not a save", "text":"No dependency authority."}));
                response["operations"][2]["depends_on"] = json!(["save", "message"]);
            }
            "wrong-source" => {
                response["operations"][1]["inputs"]["subject"]["source"] = json!("other-source")
            }
            "range" => response["operations"][1]["inputs"]["subject"]["start"] = json!(1),
            "representation" => {
                response["operations"][1]["inputs"]["subject"]["representation"] =
                    json!("json_string:/body")
            }
            "unknown-input" => response["operations"][1]["inputs"]["unexpected"] = json!(true),
            "subject-array" => {
                response["operations"][1]["inputs"]["subject"] =
                    json!(["native-source", "proposal-op:save", "utf8"])
            }
            _ => unreachable!(),
        }
        let parsed = p::parse_decision(
            &serde_json::to_vec(&response).unwrap(),
            config().response_bytes,
        );
        if matches!(fault, "unknown" | "self") {
            assert!(parsed.is_err(), "{fault}");
        } else {
            parsed.unwrap();
            let before = persisted_state(&f.app);
            let report = run_raw(&mut f, &prepared, &response);
            assert_eq!(report.status, AdoptionStatus::Held, "{fault}: {report:?}");
            assert_eq!(persisted_state(&f.app), before, "{fault}");
        }
    }
}

fn descriptor(prepared: &Prepared) -> Value {
    let value = serde_json::to_value(prepared).unwrap();
    assert!(value["work_interface"].is_object(), "Prepared must retain the exact rendered work interface; no descriptor exists on the baseline");
    value["work_interface"].clone()
}

#[test]
fn prepared_retains_the_rendered_interface_and_accounts_for_its_bytes() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("native-snapshot");
    let retained = descriptor(&prepared);
    let core = interface_control(&prepared);
    assert_eq!(retained["capability_id"], "work.command.v1");
    assert_eq!(retained["capability_id"], core["work_capability"]["id"]);
    assert_eq!(
        retained["binding_revision"],
        core["work_capability"]["binding_revision"]
    );
    assert_eq!(
        retained["inputs_schema"],
        core["work_capability"]["inputs_schema"]
    );
    assert_eq!(retained["inputs_schema"], work_schema());
    assert_eq!(
        retained["binding_revision"],
        s::sha256_bytes(&serde_json::to_vec(&retained["inputs_schema"]).unwrap())
    );
    let encoded = serde_json::to_value(&prepared).unwrap();
    let invocation = f.app.invocation(&prepared.request.id).unwrap();
    assert_eq!(invocation.request.as_ref().unwrap(), &encoded);
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<Prepared>(encoded.clone()).unwrap()).unwrap(),
        encoded
    );
    assert_eq!(
        prepared.prompt_sha256,
        s::sha256_bytes(prepared.prompt.as_bytes())
    );
    // The existing admission charges the complete serialized request twice.
    let (_, cells) = closeout_snapshot(f.app.state_root());
    let key = format!("v:{}", invocation.reservation.id);
    let cell = cells
        .iter()
        .find(|cell| cell.0 == "reservation" && cell.1 == key)
        .unwrap();
    let reservation: s::ReservationRecord = serde_json::from_slice(&cell.2).unwrap();
    assert!(
        reservation.amount.input_bytes >= (serde_json::to_vec(&encoded).unwrap().len() * 2) as u64
    );
}

// This seam alters ONLY a newly created synthetic invocation's Prepared value.
// It models a trusted old preparation for resolver tests; it is not a runtime
// API or a claim that a model may rewrite a signed/historical request. All
// unrelated Prepared fields, invocation metadata, state and Store meta remain
// byte-identical. It is never used on any existing research or operator Store.
fn replace_synthetic_prepared(
    f: &Fixture,
    original: &Prepared,
    snapshot: Option<Value>,
) -> Prepared {
    let mut updated = serde_json::to_value(original).unwrap();
    let original_descriptor = descriptor(original);
    match &snapshot {
        Some(snapshot) => updated["work_interface"] = snapshot.clone(),
        None => {
            updated.as_object_mut().unwrap().remove("work_interface");
        }
    }
    if let Some(snapshot) = &snapshot {
        let mut core = interface_control(original);
        core["work_capability"]["id"] = snapshot["capability_id"].clone();
        core["work_capability"]["binding_revision"] = snapshot["binding_revision"].clone();
        core["work_capability"]["inputs_schema"] = snapshot["inputs_schema"].clone();
        let (head, tail) = original.prompt.split_once("\nCONTROL\n").unwrap();
        let (_, rest) = tail.split_once("\nQUALIFIED_CONTEXT\n").unwrap();
        let prompt = format!(
            "{head}\nCONTROL\n{}\nQUALIFIED_CONTEXT\n{rest}",
            serde_json::to_string(&core).unwrap()
        );
        updated["prompt_sha256"] = json!(s::sha256_bytes(prompt.as_bytes()));
        updated["prompt"] = json!(prompt);
    }
    let prepared: Prepared = serde_json::from_value(updated.clone()).unwrap();
    let mut unchanged = updated.clone();
    unchanged["work_interface"] = original_descriptor;
    unchanged["prompt"] = json!(original.prompt);
    unchanged["prompt_sha256"] = json!(original.prompt_sha256);
    assert_eq!(unchanged, serde_json::to_value(original).unwrap());
    let before = closeout_snapshot(f.app.state_root());
    let cell = before
        .1
        .iter()
        .find(|cell| cell.0 == "invocation" && cell.1 == original.request.id)
        .unwrap();
    let mut invocation: Value = serde_json::from_slice(&cell.2).unwrap();
    assert_eq!(
        invocation["request"],
        serde_json::to_value(original).unwrap()
    );
    invocation["request"] = updated;
    let bytes = serde_json::to_vec(&invocation).unwrap();
    assert!(bytes.len() <= usize::try_from(cell.3).unwrap());
    let connection = Connection::open(f.app.state_root().join("state.sqlite3")).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE store_cells SET data=?1 WHERE ns='invocation' AND k=?2",
                rusqlite::params![bytes, original.request.id]
            )
            .unwrap(),
        1
    );
    let after = closeout_snapshot(f.app.state_root());
    assert_eq!(
        after.0, before.0,
        "authority epoch and counters must not change in this seam"
    );
    for (before_cell, after_cell) in before.1.iter().zip(&after.1) {
        if before_cell.0 == "invocation" && before_cell.1 == original.request.id {
            let mut restored: Value = serde_json::from_slice(&after_cell.2).unwrap();
            restored["request"] = serde_json::to_value(original).unwrap();
            assert_eq!(
                restored,
                serde_json::from_slice::<Value>(&before_cell.2).unwrap()
            );
        } else {
            assert_eq!(before_cell, after_cell);
        }
    }
    prepared
}

#[test]
fn absent_legacy_snapshot_roundtrips_without_backfill_and_rejects_native_only() {
    for use_native in [false, true] {
        let mut f = Fixture::new(true);
        f.accept();
        let original = f.prepare("legacy-snapshot");
        descriptor(&original);
        let prepared = replace_synthetic_prepared(&f, &original, None);
        assert!(
            serde_json::to_value(&prepared)
                .unwrap()
                .get("work_interface")
                .is_none(),
            "legacy serialization must not add a field and change reserialized request hashes"
        );
        assert!(
            interface_control(&prepared)["work_capability"].is_object(),
            "missing retained snapshot must not be filled from prompt"
        );
        let response = if use_native {
            native(registration(&prepared))
        } else {
            registration(&prepared)
        };
        if use_native {
            require_native_parser(&response);
        }
        let before = persisted_state(&f.app);
        let report = run_raw(&mut f, &prepared, &response);
        assert_eq!(
            report.status,
            if use_native {
                AdoptionStatus::Held
            } else {
                AdoptionStatus::Adopted
            }
        );
        if use_native {
            assert_eq!(persisted_state(&f.app), before);
        } else {
            assert_artifact(&mut f, &prepared, &response);
        }
    }
}

#[test]
fn same_epoch_schema_drift_and_inconsistent_snapshots_fail_closed() {
    for fault in ["old-schema", "hash-mismatch", "wrong-capability"] {
        let mut f = Fixture::new(true);
        f.accept();
        let original = f.prepare("schema-drift");
        let mut snapshot = descriptor(&original);
        match fault {
            "old-schema" => {
                let schema = &mut snapshot["inputs_schema"];
                let put = schema["oneOf"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|v| v["properties"]["kind"]["const"] == "put_artifact")
                    .unwrap();
                assert!(put["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("base")
                    .is_some());
                snapshot["binding_revision"] =
                    json!(s::sha256_bytes(&serde_json::to_vec(schema).unwrap()));
                assert_ne!(snapshot["binding_revision"], work_binding());
            }
            "hash-mismatch" => snapshot["binding_revision"] = json!("0".repeat(64)),
            "wrong-capability" => snapshot["capability_id"] = json!("fake.command.v1"),
            _ => unreachable!(),
        }
        let prepared = replace_synthetic_prepared(&f, &original, Some(snapshot));
        assert_eq!(prepared.epoch, original.epoch);
        assert_eq!(prepared.owner_epoch, original.owner_epoch);
        assert_eq!(prepared.work_version, original.work_version);
        let mut response = native(registration(&prepared));
        response["operations"][1]["inputs"]
            .as_object_mut()
            .unwrap()
            .remove("base");
        // This payload is accepted by the current action parser: rejecting it
        // must depend on the trusted interface, not a fortuitous grammar error.
        serde_json::from_value::<WorkAction>(response["operations"][1]["inputs"].clone()).unwrap();
        require_native_parser(&response);
        let before = persisted_state(&f.app);
        let report = run_raw(&mut f, &prepared, &response);
        assert_eq!(report.status, AdoptionStatus::Held, "{fault}: {report:?}");
        assert_eq!(persisted_state(&f.app), before, "{fault}");
    }
}

#[test]
fn native_command_does_not_grant_work_authority() {
    let mut primary = seed("a");
    primary.grants.work_commands = false;
    let mut f = Fixture::configured(true, config(), primary, need());
    f.accept();
    let prepared = f.prepare("no-work-grant");
    let response = native(registration(&prepared));
    require_native_parser(&response);
    let before = persisted_state(&f.app);
    let report = run_raw(&mut f, &prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn native_invocation_and_epoch_fences_remain_exact() {
    for fault in ["invocation", "epoch"] {
        let mut f = Fixture::new(true);
        f.accept();
        let prepared = f.prepare("current-fence");
        let mut response = native(registration(&prepared));
        require_native_parser(&response);
        match fault {
            "invocation" => response["invocation_id"] = json!("other-call"),
            "epoch" => response["epoch"] = json!(prepared.epoch + 1),
            _ => unreachable!(),
        }
        let before = persisted_state(&f.app);
        let report = run_raw(&mut f, &prepared, &response);
        assert_eq!(report.status, AdoptionStatus::Held);
        assert_eq!(persisted_state(&f.app), before);
    }
}

#[test]
fn native_character_change_fences_ordinary_actions() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("character-fence");
    let mut response = serde_json::to_value(character_selection_response(&prepared)).unwrap();
    let ordinary = native(registration(&prepared));
    response["operations"]
        .as_array_mut()
        .unwrap()
        .extend(ordinary["operations"].as_array().unwrap().clone());
    response["asset_maintenance"] = ordinary["asset_maintenance"].clone();
    require_native_parser(&response);
    let report = run_raw(&mut f, &prepared, &response);
    assert_eq!(report.status, AdoptionStatus::CharacterOnly);
    let state = f.app.snapshot().unwrap();
    assert!(!state.sources.contains_key("native-source"));
    assert!(state
        .board
        .work(&w::WorkId("work".into()))
        .unwrap()
        .artifacts
        .is_empty());
    assert_eq!(
        state.personas["a"].character.as_ref().unwrap().own_voice[0].source,
        "new-character-voice"
    );
}

#[test]
fn native_command_is_not_available_in_selection_repair() {
    // Parse a fresh ordinary control first. Otherwise the baseline's generic
    // unknown-kind error would falsely appear to prove the repair fence.
    let mut ordinary = Fixture::new(true);
    ordinary.accept();
    let p = ordinary.prepare("repair-wire-control");
    require_native_parser(&native(registration(&p)));
    let (mut f, small, _) = overflowed_repair_fixture(1);
    let prepared = f.app.prepare(repair_request("native-repair"), NOW).unwrap();
    let mut response = serde_json::to_value(repair_decision(&prepared, true)).unwrap();
    response["operations"] = json!([
        {"kind":"read", "id":"read", "reference":reference(&small)},
        {"kind":"work_command", "id":"close", "inputs":{"kind":"close_goal","release":"missing-release"}, "depends_on":[]}
    ]);
    require_native_parser(&response);
    let before = persisted_state(&f.app);
    let report = run_raw(&mut f, &prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn native_late_board_failure_rolls_back_prior_writes_and_communication() {
    let mut f = Fixture::new(true);
    f.accept();
    let prepared = f.prepare("late-board-failure");
    let mut response = native(registration(&prepared));
    require_native_parser(&response);
    response["operations"].as_array_mut().unwrap().extend([
        json!({"kind":"communicate", "id":"message", "recipient":"b", "subject":"Atomic tail", "text":"Never delivered on failure."}),
        json!({"kind":"work_command", "id":"invalid-release", "inputs":{"kind":"release","id":"invalid-release","shared":"missing-shared"}, "depends_on":[]}),
    ]);
    let last = response["operations"].as_array().unwrap().last().unwrap();
    assert!(matches!(
        serde_json::from_value::<WorkAction>(last["inputs"].clone()).unwrap(),
        WorkAction::Release { .. }
    ));
    let before = persisted_state(&f.app);
    let report = run_raw(&mut f, &prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(
        report
            .reason
            .as_deref()
            .unwrap()
            .contains("missing shared decision"),
        "{report:?}"
    );
    assert_eq!(persisted_state(&f.app), before);
    assert_eq!(
        f.app
            .invocation(&prepared.request.id)
            .unwrap()
            .response_hash,
        Some(s::sha256_bytes(&serde_json::to_vec(&response).unwrap()))
    );
}

#[test]
fn native_diagnostics_preserve_supplied_observation_vs_formal_evidence_order() {
    for earlier in [true, false] {
        let (mut f, prepared, key, message, _) =
            citation_diagnostic_fixture("HIDDEN_NATIVE_DIAGNOSTIC");
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
        let response = native(serde_json::to_value(response).unwrap());
        require_native_parser(&response);
        let report = run_raw(&mut f, &prepared, &response);
        assert_eq!(
            report.status,
            if earlier {
                AdoptionStatus::Adopted
            } else {
                AdoptionStatus::Held
            }
        );
        if !earlier {
            assert!(report.reason.unwrap().contains(CITATION_DIAGNOSTIC));
        }
        let state = f.app.snapshot().unwrap();
        assert_eq!(
            state
                .board
                .work(&"work".into())
                .unwrap()
                .evidence
                .contains_key(&w::RecordId(message)),
            earlier
        );
    }
}

#[test]
fn missing_snapshot_never_enables_request_only_native_diagnostic() {
    let (mut f, original, key, message, _) =
        citation_diagnostic_fixture("HIDDEN_MISSING_INTERFACE");
    let prepared = replace_synthetic_prepared(&f, &original, None);
    let response = native(
        serde_json::to_value(citation_diagnostic_response(
            &prepared, &key, &message, false,
        ))
        .unwrap(),
    );
    require_native_parser(&response);
    let before = persisted_state(&f.app);
    let report = run_raw(&mut f, &prepared, &response);
    assert_eq!(report.status, AdoptionStatus::Held);
    assert!(!report.reason.unwrap().contains(CITATION_DIAGNOSTIC));
    assert_eq!(persisted_state(&f.app), before);
}

#[test]
fn native_responsibility_acceptance_is_required_and_ordered() {
    for acceptance in ["before", "after", "absent"] {
        let mut f = Fixture::new(true);
        let prepared = f.prepare("native-responsibility");
        let mut response = native(registration(&prepared));
        let clear = serde_json::to_value(decision(&prepared, false)).unwrap();
        response["next_fragments"] = clear["next_fragments"].clone();
        response["next_assets"] = clear["next_assets"].clone();
        if acceptance != "absent" {
            let state = f.app.snapshot().unwrap();
            let work = state.board.work(&w::WorkId("work".into())).unwrap();
            let accept = serde_json::to_value(p::Operation::Responsibility {
                id: "offer-a".into(),
                work_id: "work".into(),
                terms_revision: offer_revision(work, &w::RecordId("offer-a".into())).unwrap(),
                disposition: "accept".into(),
                reason: "Accept the supplied terms voluntarily.".into(),
            })
            .unwrap();
            if acceptance == "before" {
                response["operations"]
                    .as_array_mut()
                    .unwrap()
                    .insert(1, accept);
            } else {
                response["operations"].as_array_mut().unwrap().push(accept);
            }
        }
        require_native_parser(&response);
        let before = persisted_state(&f.app);
        let report = run_raw(&mut f, &prepared, &response);
        assert_eq!(
            report.status,
            if acceptance == "before" {
                AdoptionStatus::Adopted
            } else {
                AdoptionStatus::Held
            },
            "{acceptance}: {report:?}"
        );
        if acceptance != "before" {
            assert_eq!(persisted_state(&f.app), before);
        }
    }
}

#[test]
fn native_current_policy_expiry_still_rejects_the_whole_response() {
    let mut configuration = config();
    configuration.policies.active_policies[0].expires_at_unix = Some(NOW + 1);
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut voice = record(
        "voice",
        f::Scope::Fragments,
        "I distinguish current permissions from retained history.",
    );
    voice.policy = configuration.policies.active_policies[0].clone();
    voice = voice.seal().unwrap();
    let mut app = Runtime::create(&temp.path().join("state"), configuration, "operator").unwrap();
    app.add_persona_seed("operator", seed("b")).unwrap();
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
    let prepared = f.prepare("native-expired-policy");
    let response = native(registration(&prepared));
    require_native_parser(&response);
    let before = persisted_state(&f.app);
    f.app.dispatch_synthetic(&prepared.request.id, NOW).unwrap();
    let report = f
        .app
        .adopt_response(
            &prepared.request.id,
            &serde_json::to_vec(&response).unwrap(),
            NOW + 2,
        )
        .unwrap();
    assert_eq!(report.status, AdoptionStatus::Held);
    assert_eq!(persisted_state(&f.app), before);
}
