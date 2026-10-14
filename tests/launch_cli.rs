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
fn provider(_root: &Path) -> ResearchSettings {
    use ai_personas::provider::{
        AccessMode, CapabilityEvidence, CredentialReference, Limits, SecretReference,
    };
    let reference = |name: &str| SecretReference { env: name.into() };
    ResearchSettings {
        mode: AccessMode::PlatformApiKey,
        model: "fixture-model".into(),
        effort: "medium".into(),
        credentials: CredentialReference::ApiKey {
            key_env: reference("UNREAD_LAUNCH_FIXTURE_KEY"),
            account_env: reference("UNREAD_LAUNCH_FIXTURE_ACCOUNT"),
        },
        evidence: CapabilityEvidence {
            mode: AccessMode::PlatformApiKey,
            model: "fixture-model".into(),
            efforts: BTreeSet::from(["medium".into()]),
            account_env: reference("UNREAD_LAUNCH_FIXTURE_EVIDENCE_ACCOUNT"),
            account_sha256: "3".repeat(64),
            transport_profile: "platform_responses_v2".into(),
            catalog_evidence_sha256: "1".repeat(64),
            structured_output_evidence_sha256: Some("2".repeat(64)),
        },
        limits: Limits {
            timeout_ms: 1000,
            max_prompt_bytes: 192 * 1024,
            max_schema_bytes: 262_144,
            max_schema_nodes: 65_536,
            max_schema_depth: 64,
            max_request_bytes: 2 * 1024 * 1024,
            max_response_bytes: 32_768,
            max_stream_bytes: 128 * 1024,
            max_event_bytes: 96 * 1024,
            max_events: 1024,
        },
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
            "--allow-direct-provider",
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
