//! Machine boundary for a later dialogue. No interactive shell or credential input.
#[cfg(unix)]
mod cli {
    use ai_personas::{auth::onboarding as backend, provider::Cancellation};
    use clap::{error::ErrorKind, Parser, Subcommand};
    use serde::Serialize;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    #[derive(Parser)]
    #[command(name = "personas-provider", about = "Provider setup and optional capability checks", after_help = backend::COST_NOTICE)]
    struct Cli {
        /// Selected vault; relative paths and existing aliases are accepted.
        #[arg(long, global = true)]
        vault: Option<PathBuf>,
        #[command(subcommand)]
        command: Command,
    }
    #[derive(Subcommand)]
    enum Command {
        /// Read selected Codex account metadata, or sign in without exporting tokens.
        AccountCodex {
            #[arg(long)]
            codex_executable: PathBuf,
            #[arg(long)]
            host_home: PathBuf,
            #[arg(long)]
            codex_home: PathBuf,
            /// Start browser sign-in. Authorization events are JSON lines on stderr.
            #[arg(long)]
            sign_in: bool,
            /// Use device-code sign-in instead of a local browser callback.
            #[arg(long, requires = "sign_in")]
            device_code: bool,
        },
        /// Discover Codex model names, reasoning options and defaults. No model call.
        CatalogueCodex {
            #[arg(long)]
            codex_executable: PathBuf,
            #[arg(long)]
            host_home: PathBuf,
            #[arg(long)]
            codex_home: PathBuf,
        },
        /// Use an existing Codex ChatGPT login for direct inference. No model call.
        ConfigureCodex {
            #[arg(long)]
            codex_executable: PathBuf,
            #[arg(long)]
            host_home: PathBuf,
            #[arg(long)]
            codex_home: PathBuf,
            #[arg(long)]
            model: String,
            #[arg(long)]
            effort: String,
        },
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
        if let Command::AccountCodex { codex_executable, host_home, codex_home, sign_in, device_code } = &cli.command {
            if cli.vault.is_some() { return Err(backend::Code::InvalidRequest); }
            let (executable, home, state, login) = (codex_executable.clone(), host_home.clone(), codex_home.clone(), sign_in.then_some(*device_code));
            return tokio::task::spawn_blocking(move || ai_personas::auth::codex_account(
                &executable, &home, &state, login, cancellation, |event| eprintln!("{event}"),
            )).await.map_err(|_| backend::Code::CredentialUnavailable)?;
        }
        if let Command::CatalogueCodex { codex_executable, host_home, codex_home } = &cli.command {
            if cli.vault.is_some() {
                return Err(backend::Code::InvalidRequest);
            }
            let (executable, home, state) = (codex_executable.clone(), host_home.clone(), codex_home.clone());
            return value(tokio::task::spawn_blocking(move || ai_personas::auth::catalogue_codex_cancellable(
                &executable, &home, &state, cancellation,
            )).await.map_err(|_| backend::Code::CatalogueUnavailable)??);
        }
        if let Command::ConfigureCodex { codex_executable, host_home, codex_home, model, effort } = &cli.command {
            if cli.vault.is_some() {
                return Err(backend::Code::InvalidRequest);
            }
            return value(ai_personas::auth::configure_codex(
                codex_executable, host_home, codex_home, model, effort,
            ).map_err(|error| match error {
                ai_personas::provider::ProviderError::Rejected(ai_personas::provider::Failure::Configuration) => backend::Code::InvalidRequest,
                ai_personas::provider::ProviderError::Rejected(ai_personas::provider::Failure::CredentialExpired) => backend::Code::CredentialExpired,
                _ => backend::Code::CredentialUnavailable,
            })?);
        }
        // Global Clap options cannot be required. Enforce explicit selection here,
        // before any backend or path access; help exits before calling run.
        let vault = cli.vault.as_deref().ok_or(backend::Code::InvalidRequest)?;
        // Explicit commands retain their actual evidence without another approval flag.
        let mut host = backend::Backend::open(vault)?;
        match cli.command {
            Command::ConfigureCodex { .. } | Command::CatalogueCodex { .. } | Command::AccountCodex { .. } => unreachable!("handled before vault access"),
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
