//! Machine boundary for a later dialogue. No interactive shell or credential input.
#[cfg(unix)]
mod cli {
    use ai_personas::{auth::onboarding as backend, provider::Cancellation};
    use clap::{error::ErrorKind, Parser, Subcommand};
    use serde::Serialize;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    #[derive(Parser)]
    #[command(name = "personas-provider", about = "Explicit SIWC catalogue and one-use capability evidence", after_help = backend::COST_NOTICE)]
    struct Cli {
        /// Required for operations: selected vault, absolute, private and outside repositories.
        #[arg(long, global = true)]
        vault: Option<PathBuf>,
        #[command(subcommand)]
        command: Command,
    }
    #[derive(Subcommand)]
    enum Command {
        /// Read the selected account's LIVE catalogue. No inference.
        Catalogue {
            #[arg(long)]
            authorize_read: bool,
            #[arg(long)]
            retain_private_evidence: bool,
        },
        /// Retain a plan and return its exact consent digest; no inference.
        Plan {
            #[arg(long)]
            id: String,
            #[arg(long)]
            catalogue_sha256: String,
            #[arg(long)]
            model: String,
            /// Unverified candidate, not an assertion of model support. No default.
            #[arg(long)]
            effort: String,
            #[arg(long)]
            timeout_ms: u64,
            /// Observed-usage threshold, NOT a remote token cap.
            #[arg(long)]
            max_observed_total_tokens: u64,
            #[arg(long)]
            require_remote_token_hard_bound: bool,
            #[arg(long)]
            retain_private_evidence: bool,
        },
        /// Consume one exact plan once; MAY consume plan usage. No automatic retries.
        Probe {
            #[arg(long)]
            id: String,
            #[arg(long)]
            plan_sha256: String,
            #[arg(long)]
            authorize_one_request: bool,
            #[arg(long)]
            retain_private_evidence: bool,
            #[arg(long)]
            acknowledge_uncapped_remote_usage: bool,
        },
        /// Retained historical metadata only. Never replays or resolves credentials.
        ProbeStatus {
            #[arg(long)]
            id: String,
            #[arg(long)]
            plan_sha256: String,
        },
        /// Generate native DirectSettings from exact completed proof; no secrets.
        /// Does not save a runtime configuration or grant runtime sharing/funding.
        Configuration {
            #[arg(long)]
            id: String,
            #[arg(long)]
            plan_sha256: String,
        },
        /// Diagnose same-account expiry rebinding; never refreshes or migrates.
        RenewalStatus {
            #[arg(long)]
            saved_config: PathBuf,
        },
    }
    fn value(v: impl Serialize) -> backend::Result<Value> {
        serde_json::to_value(v).map_err(|_| backend::Code::InvalidState)
    }
    async fn run(cli: Cli, cancellation: Cancellation) -> backend::Result<Value> {
        // Global Clap options cannot be required. Enforce explicit selection here,
        // before any backend or path access; help exits before calling run.
        let vault = cli.vault.as_deref().ok_or(backend::Code::InvalidRequest)?;
        // Refuse absent affirmative grants before opening even the selected vault.
        let denied = match &cli.command {
            Command::Catalogue {
                authorize_read,
                retain_private_evidence,
            } => !authorize_read || !retain_private_evidence,
            Command::Plan {
                retain_private_evidence,
                ..
            } => !retain_private_evidence,
            // Retained replay inspection is separately available through status.
            Command::Probe {
                authorize_one_request,
                retain_private_evidence,
                acknowledge_uncapped_remote_usage,
                ..
            } => {
                !authorize_one_request
                    || !retain_private_evidence
                    || !acknowledge_uncapped_remote_usage
            }
            _ => false,
        };
        if denied {
            return Err(backend::Code::Denied);
        }
        let mut host = backend::Backend::open(vault)?;
        match cli.command {
            Command::Catalogue {
                authorize_read,
                retain_private_evidence,
            } => value(
                host.catalogue(authorize_read, retain_private_evidence)
                    .await?,
            ),
            Command::Plan {
                id,
                catalogue_sha256,
                model,
                effort,
                timeout_ms,
                max_observed_total_tokens,
                require_remote_token_hard_bound,
                retain_private_evidence,
            } => value(host.plan(
                backend::PlanRequest {
                    id,
                    catalogue_sha256,
                    model,
                    effort,
                    timeout_ms,
                    max_observed_total_tokens,
                    require_remote_token_hard_bound,
                },
                retain_private_evidence,
            )?),
            Command::Probe {
                id,
                plan_sha256,
                authorize_one_request,
                retain_private_evidence,
                acknowledge_uncapped_remote_usage,
            } => value(
                host.execute(
                    &id,
                    &plan_sha256,
                    backend::ProbeConsent {
                        authorize_one_request,
                        retain_private_evidence,
                        acknowledge_uncapped_remote_usage,
                    },
                    cancellation,
                )
                .await?,
            ),
            Command::ProbeStatus { id, plan_sha256 } => {
                value(host.probe_status(&id, &plan_sha256)?)
            }
            Command::Configuration { id, plan_sha256 } => {
                value(host.configuration(&id, &plan_sha256)?)
            }
            Command::RenewalStatus { saved_config } => {
                let saved = ai_personas::runtime::read_json_file(&saved_config, 32_768)
                    .map_err(|_| backend::Code::InvalidRequest)?;
                value(host.renewal_status(&saved)?)
            }
        }
    }
    pub async fn main() -> i32 {
        let cli = match Cli::try_parse() {
            Ok(v) => v,
            Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
                let _ = e.print();
                return 0;
            }
            Err(_) => {
                eprintln!(
                    "{}",
                    json!({"status":"error","code":"invalid_arguments","automatic_retry":false})
                );
                return 2;
            }
        };
        let cancellation = Cancellation::default();
        let result = {
            let running = run(cli, cancellation.clone());
            tokio::pin!(running);
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => { cancellation.cancel(); running.await },
                result = &mut running => result,
            }
        };
        match result {
            Ok(data) => {
                println!(
                    "{}",
                    json!({"status":"ok","data":data,"automatic_retry":false})
                );
                0
            }
            Err(code) => {
                eprintln!(
                    "{}",
                    json!({"status":"error","code":code,"automatic_retry":false})
                );
                1
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn help_without_vault_needs_no_operational_arguments() {
            for subcommand in [
                None,
                Some("catalogue"),
                Some("plan"),
                Some("probe"),
                Some("probe-status"),
                Some("configuration"),
                Some("renewal-status"),
            ] {
                let mut args = vec!["personas-provider"];
                args.extend(subcommand);
                args.push("--help");
                match Cli::try_parse_from(args) {
                    Err(error) => assert_eq!(error.kind(), ErrorKind::DisplayHelp),
                    Ok(_) => panic!("help must exit before operational validation"),
                }
            }
        }

        #[test]
        fn explicit_vault_is_global_before_or_after_subcommand() {
            let selected = "/unopened/private path/personas-account.json";
            for args in [
                vec!["personas-provider", "--vault", selected, "catalogue"],
                vec!["personas-provider", "catalogue", "--vault", selected],
            ] {
                let cli = Cli::try_parse_from(args).unwrap();
                assert_eq!(cli.vault, Some(PathBuf::from(selected)));
                assert!(matches!(
                    cli.command,
                    Command::Catalogue {
                        authorize_read: false,
                        retain_private_evidence: false,
                    }
                ));
            }
        }

        #[tokio::test]
        async fn missing_vault_is_rejected_before_backend_or_saved_config_access() {
            for args in [
                vec![
                    "personas-provider",
                    "catalogue",
                    "--authorize-read",
                    "--retain-private-evidence",
                ],
                vec![
                    "personas-provider",
                    "renewal-status",
                    "--saved-config",
                    "/unopened/private/saved-config.json",
                ],
            ] {
                let cli = Cli::try_parse_from(args).unwrap();
                assert!(cli.vault.is_none());
                assert!(matches!(
                    run(cli, Cancellation::default()).await,
                    Err(backend::Code::InvalidRequest)
                ));
            }
        }

        #[test]
        fn model_effort_and_finite_limits_have_no_defaults() {
            assert!(Cli::try_parse_from([
                "personas-provider",
                "--vault",
                "/private/vault",
                "plan",
                "--id",
                "one"
            ])
            .is_err());
        }
        #[tokio::test]
        async fn denied_probe_never_opens_a_vault() {
            let cli = Cli::try_parse_from([
                "personas-provider",
                "--vault",
                "/missing/private/vault",
                "probe",
                "--id",
                "one",
                "--plan-sha256",
                &"a".repeat(64),
            ])
            .unwrap();
            assert!(matches!(
                run(cli, Cancellation::default()).await,
                Err(backend::Code::Denied)
            ));
        }
        #[test]
        fn no_bearer_or_network_target_arguments() {
            assert!(Cli::try_parse_from([
                "personas-provider",
                "--vault",
                "/private/vault",
                "catalogue",
                "--access-token",
                "sentinel"
            ])
            .is_err());
            assert!(Cli::try_parse_from([
                "personas-provider",
                "--vault",
                "/private/vault",
                "catalogue",
                "--url",
                "https://example.invalid"
            ])
            .is_err());
        }
    }
}
#[cfg(unix)]
#[tokio::main]
async fn main() {
    std::process::exit(cli::main().await);
}
#[cfg(not(unix))]
fn main() {
    eprintln!(
        "{{\"status\":\"error\",\"code\":\"platform_unsupported\",\"automatic_retry\":false}}"
    );
    std::process::exit(1);
}
