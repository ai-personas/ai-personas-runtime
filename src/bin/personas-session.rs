//! Machine-readable Stage 1 boundary. Not an interactive onboarding wizard.
//! New commands have static errors; existing ai-personas/personas-auth stay intact.
use ai_personas::launch::session::{self, NeedRequest, SessionRequest, SessionRoute};
use clap::{error::ErrorKind, Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "personas-session",
    about = "Operator-only saved-launch inspection, keyed intake and single-use sessions"
)]
struct Cli {
    #[arg(long, global = true)]
    state_root: Option<PathBuf>,
    #[arg(long, global = true)]
    operator: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Preview or atomically initialize an exact fresh launch; never repair/reset a Store.
    Bootstrap {
        file: PathBuf,
        #[arg(
            long,
            required_unless_present = "authorize_setup",
            conflicts_with = "authorize_setup"
        )]
        preview: bool,
        #[arg(long, required_unless_present = "preview", conflicts_with = "preview")]
        authorize_setup: bool,
    },
    /// No owner recovery, credential resolution, grants or inference. Metered reads.
    Inspect {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Record a need once, without inference, under existing configured authority.
    Intake {
        #[arg(long)]
        id: String,
        #[arg(long)]
        require_acceptance: bool,
        need: String,
    },
    /// Consume an ID before one bounded foreground run. Repetition never dispatches.
    Run {
        #[arg(long)]
        id: String,
        #[arg(long)]
        work: String,
        #[arg(long, required = true)]
        authorize_run: bool,
        /// Explicit existing SIWC vault. No sign-in, refresh, export or fallback.
        #[arg(long, requires = "authorize_native")]
        vault: Option<PathBuf>,
        #[arg(long, requires = "vault")]
        authorize_native: bool,
    },
    /// Retained session metadata only. Unrecorded outcome never authorizes replay.
    SessionStatus {
        #[arg(long)]
        id: String,
    },
    /// Current permission-filtered retained result; no cached private output replay.
    Result { work: String },
}
fn emit<T: Serialize>(result: session::BoundaryResult<T>) -> bool {
    match result {
        Ok(value) => {
            // A failed stdout write must not restart an operation. The caller
            // retains its input/session key and queries that key after restart.
            use std::io::Write;
            let output =
                serde_json::json!({"version":1,"ok":true,"result":value,"automatic_retry":false});
            let mut out = std::io::stdout().lock();
            serde_json::to_writer(&mut out, &output).is_ok() && writeln!(&mut out).is_ok()
        }
        Err(code) => {
            eprintln!(
                "{}",
                serde_json::json!({"version":1,"ok":false,"code":code,"automatic_retry":false})
            );
            false
        }
    }
}
#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return;
        }
        Err(_) => {
            // Clap may echo arbitrary supplied values; never print them on error.
            emit::<()>(Err(session::BoundaryError::InvalidRequest));
            std::process::exit(2);
        }
    };
    let (Some(root), Some(operator)) = (cli.state_root, cli.operator) else {
        emit::<()>(Err(session::BoundaryError::InvalidRequest));
        std::process::exit(2);
    };
    let ok = match cli.command {
        Command::Bootstrap {
            file,
            preview,
            authorize_setup,
        } => {
            let result = (|| -> session::BoundaryResult<serde_json::Value> {
                if preview == authorize_setup {
                    return Err(session::BoundaryError::InvalidRequest);
                }
                let request: ai_personas::launch::bootstrap::Request =
                    ai_personas::runtime::read_json_file(&file, 65_536)?;
                if request.operator != operator {
                    return Err(session::BoundaryError::Unauthorized);
                }
                if preview {
                    Ok(ai_personas::launch::bootstrap::preview(&request)?)
                } else {
                    Ok(serde_json::to_value(
                        ai_personas::launch::bootstrap::apply(&root, &request)?,
                    )?)
                }
            })();
            emit(result)
        }
        Command::Inspect { after, limit } => {
            emit(session::inspect(&root, &operator, after.as_deref(), limit))
        }
        Command::Intake {
            id,
            need,
            require_acceptance,
        } => emit(session::intake(
            &root,
            &NeedRequest {
                id,
                operator,
                need,
                require_acceptance,
            },
        )),
        Command::Run {
            id,
            work,
            authorize_run,
            vault,
            authorize_native,
        } => {
            if !authorize_run || vault.is_some() != authorize_native {
                emit::<()>(Err(session::BoundaryError::InvalidRequest))
            } else {
                let route = match vault {
                    Some(vault) => SessionRoute::NativeSiwc { vault },
                    None => SessionRoute::SavedEnvironment,
                };
                emit(
                    session::run_session(
                        &root,
                        &SessionRequest {
                            id,
                            operator,
                            work,
                            route,
                        },
                    )
                    .await,
                )
            }
        }
        Command::SessionStatus { id } => emit(session::session_status(&root, &operator, &id)),
        Command::Result { work } => emit(session::result_view(&root, &operator, &work)),
    };
    if !ok {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_requires_preview_or_explicit_setup_authorization() {
        assert!(
            Cli::try_parse_from(["personas-session", "bootstrap", "/fixture/request.json"])
                .is_err()
        );
        for mode in ["--preview", "--authorize-setup"] {
            assert!(Cli::try_parse_from([
                "personas-session",
                "bootstrap",
                "/fixture/request.json",
                mode
            ])
            .is_ok());
        }
        assert!(Cli::try_parse_from([
            "personas-session",
            "bootstrap",
            "/fixture/request.json",
            "--preview",
            "--authorize-setup"
        ])
        .is_err());
    }
    #[test]
    fn run_requires_explicit_consent() {
        assert!(
            Cli::try_parse_from(["personas-session", "run", "--id", "s", "--work", "w"]).is_err()
        );
    }
    #[test]
    fn native_route_requires_its_separate_consent() {
        assert!(Cli::try_parse_from([
            "personas-session",
            "run",
            "--id",
            "s",
            "--work",
            "w",
            "--authorize-run",
            "--vault",
            "/fixture/vault"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "personas-session",
            "run",
            "--id",
            "s",
            "--work",
            "w",
            "--authorize-run",
            "--vault",
            "/fixture/vault",
            "--authorize-native"
        ])
        .is_ok());
    }
    #[test]
    fn exact_need_is_one_literal_argument() {
        let cli = Cli::try_parse_from([
            "personas-session",
            "--state-root",
            "/fixture/state",
            "--operator",
            "fixture",
            "intake",
            "--id",
            "key",
            "--",
            "--model; $(not-a-command)",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Command::Intake { need, .. } if need == "--model; $(not-a-command)")
        );
    }
}
