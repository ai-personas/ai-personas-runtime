use ai_personas::{
    launch,
    runtime::{self, ResearchSettings},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    process::{Command, Output},
};

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ai-personas"))
        .arg("--state-root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn provider(root: &Path) -> ResearchSettings {
    ResearchSettings {
        executable: root.join("no-provider"),
        executable_sha256: "0".repeat(64),
        cli_version: "0.160.0".into(),
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
    }
}
#[test]
fn first_use_setup_and_stored_result_are_json_and_inference_free() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let working = root.path().join("working");
    fs::create_dir(&working).unwrap();
    let settings = root.path().join("provider.json");
    fs::write(
        &settings,
        serde_json::to_vec(&provider(root.path())).unwrap(),
    )
    .unwrap();
    let output = cli(
        &state,
        &[
            "--operator",
            "requester",
            "setup",
            "--provider-config",
            settings.to_str().unwrap(),
            "--working-directory",
            working.to_str().unwrap(),
            "--destination",
            "fixture",
            "--share-cohort",
            "--allow-nonconformant-research",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["inference_performed"],
        false
    );
    let mut app = launch::open(&state, None).unwrap();
    assert_eq!(app.snapshot().unwrap().personas.len(), 3);
    let work = launch::start(&mut app, "A synthetic local request.").unwrap();
    let serials: Vec<_> = app
        .snapshot()
        .unwrap()
        .personas
        .values()
        .map(|p| p.admission_serial)
        .collect();
    drop(app);
    let output = cli(&state, &["result", &work, "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "unfinished");
    assert_eq!(result["messages"], json!([]));
    let mut app = launch::open(&state, None).unwrap();
    assert_eq!(
        app.snapshot()
            .unwrap()
            .personas
            .values()
            .map(|p| p.admission_serial)
            .collect::<Vec<_>>(),
        serials
    );
    assert!(!root.path().join("no-provider").exists());
}
#[test]
fn cli_requires_explicit_human_attribution_and_rejects_conflicting_run_arguments() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("absent");
    let request = root.path().join("human.json");
    fs::write(&request, b"{}").unwrap();
    let out = cli(&state, &["stakeholder-decision", request.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--operator"));
    assert!(!state.exists());
    let out = cli(&state, &["run", "need", "--resume", "work", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!state.exists());
}
#[test]
fn default_state_path_resolves_only_from_explicit_user_environment() {
    let root = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ai-personas"))
        .env("XDG_STATE_HOME", root.path())
        .args(["result", "missing", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!root.path().join("ai-personas").exists());
    assert!(runtime::read_json_file::<Value>(&root.path().join("absent"), 16_384).is_err());
}
