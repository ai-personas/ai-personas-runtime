//! Actor-facing reference copies use the strict Decision wire representation.
use super::*;

fn context(prepared: &Prepared) -> Value {
    serde_json::from_str(
        prepared
            .prompt
            .split_once("\nQUALIFIED_CONTEXT\n")
            .unwrap()
            .1,
    )
    .unwrap()
}
fn observed(prepared: &Prepared, kind: &str) -> Value {
    interface_control(prepared)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["kind"] == kind)
        .unwrap()["payload"]
        .clone()
}
fn exact(value: &Value) -> p::ExactRef {
    serde_json::from_value(value.clone()).expect("actor reference must copy directly into ExactRef")
}

#[test]
fn actionable_discovery_references_copy_to_read_without_changing_retained_pages() {
    for legacy in [false, true] {
        let mut fixture = Fixture::new(true);
        fixture.accept();
        let source = record("copyable-asset", f::Scope::Assets, "unselected needle body");
        fixture
            .app
            .import_files("operator", "a", vec![source.clone()], false, NOW)
            .unwrap();
        let prepared = fixture.prepare("find");
        let mut response = decision(&prepared, true);
        response.operations.push(if legacy {
            p::Operation::Search {
                id: "find".into(),
                scope: p::Scope::Assets {},
                query: "needle".into(),
                regex: false,
                limit: 1,
            }
        } else {
            p::Operation::Discover {
                id: "find".into(),
                request: f::DiscoveryRequest {
                    scope: f::Scope::Assets,
                    surface: f::SearchSurface::Path,
                    mode: f::SearchMode::Listing,
                    hidden_paths: f::HiddenPaths::Include,
                    retirement: f::RetirementFilter::AllRetained,
                },
                limit: 1,
                cursor: None,
            }
        });
        assert_eq!(
            fixture.run(&prepared, &response).status,
            AdoptionStatus::Adopted
        );
        let retained = serde_json::to_value(fixture.app.snapshot().unwrap().observations).unwrap();
        let next = fixture.prepare("read-hit");
        let payload = observed(&next, "discovery");
        let page: Value = if legacy {
            serde_json::from_str(payload["page"].as_str().unwrap()).unwrap()
        } else {
            payload["page"].clone()
        };
        let copied = exact(&page["hits"][0]["reference"]);
        assert_eq!(copied.source, source.source);
        assert_eq!(copied.revision, source.revision);
        assert_eq!(copied.representation, "utf8");
        assert_eq!(page["reference_format"], "decision_exact_ref");
        assert!(!next.prompt.contains("unselected needle body"));
        assert_eq!(
            serde_json::to_value(fixture.app.snapshot().unwrap().observations).unwrap(),
            retained
        );
        let mut response = decision(&next, true);
        response.operations.push(p::Operation::Read {
            id: "read".into(),
            reference: copied,
        });
        let bytes = serde_json::to_vec(&response).unwrap();
        p::parse_decision(&bytes, config().response_bytes).unwrap();
        assert_eq!(
            fixture.run(&next, &response).status,
            AdoptionStatus::Adopted
        );
        let after = fixture.prepare("see-read");
        let result = observed(&after, "qualified_read")["result"].clone();
        let entry = &result["entries"][0];
        assert_eq!(exact(&entry["reference"]).source, source.source);
        assert_eq!(entry["text"], "unselected needle body");
        let copied = exact(&entry["reference"]);
        let mut response = decision(&after, true);
        response.operations.push(effect(
            "register",
            WorkAction::PutArtifact {
                artifact: "copied-artifact".into(),
                base: None,
                subject: copied.clone(),
            },
            &[],
        ));
        let report = fixture.run(&after, &response);
        assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
        let catalogued = fixture.prepare("artifact-metadata");
        let core = interface_control(&catalogued);
        let artifact = core["work_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["source"]["source"] == source.source)
            .unwrap();
        assert_eq!(exact(&artifact["source"]), copied);
    }
}

#[test]
fn actionable_context_preserves_projected_ranges_and_qualifications() {
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let qualification = record("copyable-caveat", f::Scope::Assets, "literal qualification");
    let mut source = record("copyable-json", f::Scope::Assets, r#"{"body":"xλyz"}"#);
    let mut required = qualification.reference(f::Representation::Utf8);
    required.start = Some(0);
    required.end = Some(7);
    source.qualifications.push(required);
    source = source.seal().unwrap();
    fixture
        .app
        .import_files(
            "operator",
            "a",
            vec![source.clone(), qualification],
            false,
            NOW,
        )
        .unwrap();
    let prepared = fixture.prepare("read-json");
    let mut root = reference(&source);
    root.representation = "json_string:/body".into();
    root.start = Some(1);
    root.end = Some(3);
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Read {
        id: "read".into(),
        reference: root.clone(),
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let next = fixture.prepare("copy-read-into-artifact");
    for entry in context(&next)["entries"].as_array().unwrap() {
        assert_eq!(exact(&entry["reference"]).representation, "utf8"); // mandatory own voice
    }
    let result = observed(&next, "qualified_read")["result"].clone();
    assert_eq!(result["reference_format"], "decision_exact_ref");
    let entry = result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["reference"]["source"] == source.source)
        .unwrap();
    let copied = exact(&entry["reference"]);
    assert_eq!(
        (copied.representation.as_str(), copied.start, copied.end),
        ("json_string:/body", Some(1), Some(3))
    );
    assert_eq!(entry["text"], "λ");
    let caveat = result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["reference"]["source"] == "copyable-caveat")
        .unwrap();
    assert_eq!(caveat["text"], "literal");
    let role = &caveat["roles"][0];
    assert_eq!(exact(&role["required_by"]), copied);
    for attribution in result["source_attribution"].as_array().unwrap() {
        exact(&attribution["reference"]);
    }
    let mut response = decision(&next, true);
    response.operations.push(p::Operation::Read {
        id: "reread".into(),
        reference: copied,
    });
    let report = fixture.run(&next, &response);
    assert_eq!(report.status, AdoptionStatus::Adopted, "{report:?}");
}

#[test]
fn actionable_references_do_not_coerce_file_layer_objects_in_responses() {
    for nested in [false, true] {
        let mut fixture = Fixture::new(true);
        fixture.accept();
        let prepared = fixture.prepare("reject-object-reference");
        let mut response = decision(&prepared, true);
        response.operations.push(write(
            "save",
            p::Scope::Assets {},
            "untouched",
            "no partial save",
            None,
        ));
        response.asset_maintenance = p::Maintenance::Change {
            reason: "Synthetic atomicity test.".into(),
            operation_ids: vec!["save".into()],
        };
        if nested {
            let mut operation = effect(
                "register",
                WorkAction::PutArtifact {
                    artifact: "bad-copy".into(),
                    base: None,
                    subject: proposed("untouched", "save"),
                },
                &["save"],
            );
            if let p::Operation::Effect { inputs, .. } = &mut operation {
                inputs["subject"]["representation"] = json!({"kind":"utf8"});
            }
            response.operations.push(operation);
        } else {
            response.operations.push(p::Operation::Read {
                id: "read".into(),
                reference: proposed("untouched", "save"),
            });
        }
        let mut value = serde_json::to_value(&response).unwrap();
        if !nested {
            value["operations"][1]["reference"]["representation"] = json!({"kind":"utf8"});
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        if nested {
            p::parse_decision(&bytes, config().response_bytes).unwrap();
        } else {
            assert!(p::parse_decision(&bytes, config().response_bytes).is_err());
        }
        let before = persisted_state(&fixture.app);
        fixture
            .app
            .dispatch_synthetic(&prepared.request.id, NOW)
            .unwrap();
        let result = fixture
            .app
            .adopt_response(&prepared.request.id, &bytes, NOW);
        if nested {
            let report = result.unwrap();
            assert_eq!(report.status, AdoptionStatus::Held);
            assert!(report.applied.is_empty());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(persisted_state(&fixture.app), before);
    }
}

#[test]
fn discovery_rejects_unrepresentable_json_pointers_before_emitting_actionable_hits() {
    for pointer in ["/a b".to_owned(), format!("/{}", "a".repeat(244))] {
        let mut fixture = Fixture::new(true);
        fixture.accept();
        let source = record(
            "pointer-asset",
            f::Scope::Assets,
            &serde_json::to_string(&json!({&pointer[1..]: "needle"})).unwrap(),
        );
        fixture
            .app
            .import_files("operator", "a", vec![source], false, NOW)
            .unwrap();
        let prepared = fixture.prepare("unsupported-pointer");
        let mut response = decision(&prepared, true);
        response.operations.push(p::Operation::Discover {
            id: "find".into(),
            request: f::DiscoveryRequest {
                scope: f::Scope::Assets,
                surface: f::SearchSurface::Text {
                    representation: f::Representation::JsonString {
                        pointer: pointer.clone(),
                    },
                },
                mode: f::SearchMode::Literal {
                    needle: "needle".into(),
                },
                hidden_paths: f::HiddenPaths::Include,
                retirement: f::RetirementFilter::AllRetained,
            },
            limit: 1,
            cursor: None,
        });
        let bytes = serde_json::to_vec(&response).unwrap();
        let error = p::parse_decision(&bytes, config().response_bytes)
            .expect_err("discovery must reject a pointer that cannot be copied into Read");
        assert!(error.to_string().contains("request.surface"));
        let before = persisted_state(&fixture.app);
        fixture
            .app
            .dispatch_synthetic(&prepared.request.id, NOW)
            .unwrap();
        assert!(matches!(
            fixture
                .app
                .adopt_response(&prepared.request.id, &bytes, NOW),
            Err(RuntimeError::Protocol(_))
        ));
        assert_eq!(persisted_state(&fixture.app), before);
    }
}

#[test]
fn discovery_boundary_json_pointer_copies_unchanged_to_read() {
    // Prefix is 12 bytes: exactly 244 pointer bytes fit the 256-byte identifier.
    let pointer = format!("/{}", "a".repeat(243));
    let mut fixture = Fixture::new(true);
    fixture.accept();
    let source = record(
        "boundary-pointer",
        f::Scope::Assets,
        &serde_json::to_string(&json!({&pointer[1..]: "needle"})).unwrap(),
    );
    fixture
        .app
        .import_files("operator", "a", vec![source], false, NOW)
        .unwrap();
    let prepared = fixture.prepare("boundary-find");
    let mut response = decision(&prepared, true);
    response.operations.push(p::Operation::Discover {
        id: "find".into(),
        request: f::DiscoveryRequest {
            scope: f::Scope::Assets,
            surface: f::SearchSurface::Text {
                representation: f::Representation::JsonString {
                    pointer: pointer.clone(),
                },
            },
            mode: f::SearchMode::Literal {
                needle: "needle".into(),
            },
            hidden_paths: f::HiddenPaths::Include,
            retirement: f::RetirementFilter::AllRetained,
        },
        limit: 1,
        cursor: None,
    });
    assert_eq!(
        fixture.run(&prepared, &response).status,
        AdoptionStatus::Adopted
    );
    let next = fixture.prepare("boundary-read");
    let hit = observed(&next, "discovery")["page"]["hits"][0]["reference"].clone();
    let copied = exact(&hit);
    assert_eq!(copied.representation, format!("json_string:{pointer}"));
    assert_eq!(copied.representation.len(), p::MAX_ID_BYTES);
    let mut response = decision(&next, true);
    response.operations.push(p::Operation::Read {
        id: "read".into(),
        reference: copied,
    });
    p::parse_decision(
        &serde_json::to_vec(&response).unwrap(),
        config().response_bytes,
    )
    .unwrap();
    assert_eq!(
        fixture.run(&next, &response).status,
        AdoptionStatus::Adopted
    );
    let after = fixture.prepare("boundary-read-result");
    assert_eq!(
        observed(&after, "qualified_read")["result"]["entries"][0]["text"],
        "needle"
    );
}

#[test]
fn imported_qualification_references_are_actionable_or_explicitly_rejected() {
    for (pointer, key, supported) in [
        ("/a b".to_owned(), "a b".to_owned(), false),
        (format!("/{}", "a".repeat(244)), "a".repeat(244), false),
        ("/a~1b~0c".to_owned(), "a/b~c".to_owned(), true),
    ] {
        let mut f = Fixture::new(true);
        f.accept();
        let qualification = record(
            "imported-qualification",
            f::Scope::Assets,
            &serde_json::to_string(&json!({key: "Exact qualification text."})).unwrap(),
        );
        let original_bytes = qualification.bytes.clone();
        let mut root = record(
            "imported-root",
            f::Scope::Assets,
            "Root with a required qualification.",
        );
        root.qualifications
            .push(qualification.reference(f::Representation::JsonString { pointer }));
        root = root.seal().unwrap();
        let before = persisted_state(&f.app);
        let imported = f.app.import_files(
            "operator",
            "a",
            vec![qualification.clone(), root.clone()],
            false,
            NOW,
        );
        if let Err(error) = imported {
            assert!(
                !supported,
                "representable escaped pointer must remain usable: {error}"
            );
            assert!(
                error.to_string().contains("import.qualifications"),
                "{error}"
            );
            assert_eq!(persisted_state(&f.app), before);
            continue;
        }
        let prepared = f.prepare("read-imported-root");
        let mut d = decision(&prepared, true);
        d.operations.push(p::Operation::Read {
            id: "root-read".into(),
            reference: reference(&root),
        });
        p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
        assert_eq!(f.run(&prepared, &d).status, AdoptionStatus::Adopted);
        let next = f.prepare("copy-imported-qualification");
        let result = observed(&next, "qualified_read")["result"].clone();
        assert_eq!(result["reference_format"], "decision_exact_ref");
        let entry = result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["reference"]["source"] == qualification.source)
            .unwrap();
        assert_eq!(entry["text"], "Exact qualification text.");
        let mut d = decision(&next, true);
        d.operations.push(p::Operation::Read {
            id: "copy-read".into(),
            reference: exact(&entry["reference"]),
        });
        p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).expect(
            "an advertised actionable qualification must copy into a complete strict Read Decision",
        );
        assert_eq!(f.run(&next, &d).status, AdoptionStatus::Adopted);
        assert_eq!(
            f.app.snapshot().unwrap().sources[&qualification.source].revisions[0]
                .record
                .bytes,
            original_bytes
        );
        assert!(
            supported,
            "unsupported imported qualification must be rejected explicitly"
        );
    }
}

#[test]
fn retained_unsupported_qualification_is_withheld_before_actionable_supply() {
    let mut f = Fixture::new(true);
    f.accept();
    let qualification = record(
        "legacy-qualification",
        f::Scope::Assets,
        r#"{"a b":"Retained required text."}"#,
    );
    let root = record("legacy-qualified-root", f::Scope::Assets, "Root body.");
    f.app
        .import_files(
            "operator",
            "a",
            vec![qualification.clone(), root.clone()],
            false,
            NOW,
        )
        .unwrap();
    let mut retained = root.clone();
    retained
        .qualifications
        .push(qualification.reference(f::Representation::JsonString {
            pointer: "/a b".into(),
        }));
    retained = retained.seal().unwrap();
    let state_root = f.app.state_root().to_owned();
    drop(f.app);
    let mut host = s::Store::open(&state_root.join("state.sqlite3"), storage_limits()).unwrap();
    let old = host.read("runtime-state-v1").unwrap().unwrap();
    let mut object = old.clone();
    object.revision += 1;
    object.body["sources"][&root.source]["head"] = json!(retained.revision);
    object.body["sources"][&root.source]["revisions"][0]["record"] =
        serde_json::to_value(&retained).unwrap();
    host.compare_and_swap_batch(vec![s::Change {
        expected_revision: Some(old.revision),
        object,
        depends_on: vec![],
    }])
    .unwrap();
    drop(host);
    f.app = Runtime::open(&state_root).unwrap();
    let prepared = f.prepare("read-legacy-qualification");
    let mut d = decision(&prepared, true);
    d.operations.push(p::Operation::Read {
        id: "read-root".into(),
        reference: reference(&retained),
    });
    p::parse_decision(&serde_json::to_vec(&d).unwrap(), config().response_bytes).unwrap();
    let report = f.run(&prepared, &d);
    assert_eq!(report.status, AdoptionStatus::Adopted);
    // The exact native receipt is retained, but the existing recheck boundary
    // withholds unsupported context rather than advertising an invalid wire ref.
    let after = f.prepare("see-legacy-qualification-disposition");
    assert!(interface_control(&after)["actual_observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["status"] == "withheld_unavailable" && entry["body_supplied"] == false));
    assert!(!after.prompt.contains("json_string:/a b"));
    assert!(!after.prompt.contains("Retained required text."));
    assert_eq!(
        f.app.snapshot().unwrap().sources[&qualification.source].revisions[0]
            .record
            .bytes,
        qualification.bytes
    );
}
