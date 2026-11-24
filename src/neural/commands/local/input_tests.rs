use super::*;

#[test]
fn receiving_reports_conflicting_inputs_and_ordinary_scratch_alternative() {
    let db = Connection::open_in_memory().unwrap();
    let run: Run = serde_json::from_value(json!({"program":"python3", "directory":"workspace",
        "receiving":{"delivery":"delivery:edition@1","purpose":"Check issued bytes"},
        "files":{"dependency.zip":"object:retained"},"stdin":"object:input"})).unwrap();
    // This fails before inspecting a delivery or launching any program.
    let error = run.project_inputs(&db).unwrap_err().to_string();
    for detail in ["directory:workspace", "files", "stdin", "omit receiving", "ordinary scratch run",
        "Dependencies are not loaded implicitly"] {
        assert!(error.contains(detail), "Missing actionable diagnostic: {detail}: {error}");
    }
}

#[test]
fn ordinary_scratch_inputs_are_not_subject_to_receiving_isolation() {
    let db = Connection::open_in_memory().unwrap();
    let run: Run = serde_json::from_value(json!({"program":"python3",
        "files":{"dependency.zip":"object:retained"},"stdin":"object:input"})).unwrap();
    assert_eq!(run.project_inputs(&db).unwrap(),run.files);
}

#[test]
fn receiving_schema_explains_that_dependencies_are_not_implicitly_supplied() {
    let schema = serde_json::to_value(schemars::schema_for!(Run)).unwrap();
    let description = schema["properties"]["receiving"]["description"].as_str().unwrap();
    assert!(description.contains("Do not combine with files"));
    assert!(description.contains("ordinary scratch run"));
    assert!(description.contains("does not implicitly load dependencies"));
}
