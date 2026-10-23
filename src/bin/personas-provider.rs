//! Machine boundary for a later dialogue. No interactive shell or credential input.
#[cfg(unix)]
mod cli {
    use ai_personas::{auth::onboarding as backend, provider::Cancellation};
    use clap::{error::ErrorKind, Parser, Subcommand};
    use serde::Serialize;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    #[derive(Parser)]
    #[command(name = "personas-provider", about = "Explicit SIWC settings and optional capability checks", after_help = backend::COST_NOTICE)]
    struct Cli {
        /// Selected vault; relative paths and existing aliases are accepted.
        #[arg(long, global = true)]
        vault: Option<PathBuf>,
        #[command(subcommand)]
        command: Command,
    }
    #[derive(Subcommand)]
    enum Command {
        /// Generate settings for explicit model/effort choices. No catalogue or inference.
        Configure {
            #[arg(long)]
            model: String,
            #[arg(long)]
            effort: String,
        },
        /// Read the selected account's LIVE catalogue. No inference.
        Catalogue,
        /// Retain a plan and return its exact request identity digest; no inference.
        Plan {
            #[arg(long)]
            id: String,
            #[arg(long)]
            catalogue_sha256: Option<String>,
            #[arg(long)]
            model: String,
            /// Unverified candidate, not an assertion of model support. No default.
            #[arg(long)]
            effort: String,
        },
        /// Consume one exact plan once; MAY consume plan usage. No automatic retries.
        Probe {
            #[arg(long)]
            id: String,
            #[arg(long)]
            plan_sha256: String,
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
        // Explicit commands retain their actual evidence without another approval flag.
        let mut host = backend::Backend::open(vault)?;
        match cli.command {
            Command::Configure { model, effort } => value(host.configure(&model, &effort)?),
            Command::Catalogue => value(host.catalogue().await?),
            Command::Plan {
                id,
                catalogue_sha256,
                model,
                effort,
            } => value(host.plan(backend::PlanRequest {
                id,
                catalogue_sha256,
                model,
                effort,
            })?),
            Command::Probe {
                id,
                plan_sha256,
            } => value(
                host.execute(&id, &plan_sha256, cancellation)
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
