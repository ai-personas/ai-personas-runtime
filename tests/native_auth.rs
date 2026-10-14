//! Offline parsing and fail-closed CLI paths; no subprocess or live transport.
#![cfg(unix)]
#[path = "../src/bin/personas-auth.rs"]
#[allow(dead_code)]
mod helper;
use clap::Parser;

const CANARY: &str = "NATIVE_ARGUMENT_MUST_NOT_APPEAR";
fn arguments() -> Vec<&'static str> {
    vec![
        "personas-auth",
        "--vault",
        "/unused-native-fixture/vault",
        "native-run",
        "--authorize",
        "--state-root",
        "/unused-native-fixture/state",
        "--operator",
        "requester",
    ]
}

#[test]
fn native_arguments_require_authorization_and_exactly_one_input() {
    for input in [
        vec!["--need", "Synthetic need."],
        vec!["--work", "work-00000001"],
    ] {
        let mut args = arguments();
        args.extend(input);
        assert!(helper::Cli::try_parse_from(args.clone()).is_ok());
        args.retain(|arg| *arg != "--authorize");
        assert!(helper::Cli::try_parse_from(args).is_err());
    }
    for extra in [
        vec![],
        vec!["--need", "Synthetic need.", "--work", "work-00000001"],
        vec!["--need", "Synthetic need.", "--runtime-bin", CANARY],
        vec!["--need", "Synthetic need.", "--provider-config", CANARY],
        vec!["--need", "Synthetic need.", "--", CANARY],
    ] {
        let mut args = arguments();
        args.extend(extra);
        assert!(helper::Cli::try_parse_from(args).is_err());
    }
    for flag in ["--state-root", "--operator"] {
        let mut args = arguments();
        let index = args.iter().position(|arg| *arg == flag).unwrap();
        args.drain(index..index + 2);
        args.extend(["--need", "Synthetic need."]);
        assert!(helper::Cli::try_parse_from(args).is_err());
    }
}

#[tokio::test]
async fn relative_state_rejects_before_selected_vault_access() {
    let mut args = arguments();
    let index = args.iter().position(|arg| *arg == "--state-root").unwrap();
    args[index + 1] = "relative";
    args.extend(["--need", CANARY]);
    let error = helper::run(helper::Cli::try_parse_from(args).unwrap())
        .await
        .unwrap_err();
    assert!(!format!("{error:?}: {error}").contains(CANARY));
}

#[tokio::test]
async fn generic_exec_remains_unavailable_without_accessing_compatibility_inputs() {
    for allow in [false, true] {
        let mut args = vec!["personas-auth", "--vault", CANARY, "exec"];
        if allow {
            args.push("--allow-handoff");
        }
        args.extend([
            "--provider-config",
            CANARY,
            "--runtime-bin",
            CANARY,
            "--",
            CANARY,
        ]);
        assert_eq!(
            helper::run(helper::Cli::try_parse_from(args).unwrap())
                .await
                .unwrap(),
            1
        );
    }
}
