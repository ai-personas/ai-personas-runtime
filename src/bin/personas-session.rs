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
        /// Exact launch reference previously inspected before authorizing this session.
        #[arg(long)]
        expected_launch: Option<PathBuf>,
        /// Explicit existing SIWC vault. No sign-in, refresh, export or fallback.
        #[arg(long, requires = "authorize_native")]
        vault: Option<PathBuf>,
        #[arg(long, requires = "vault", conflicts_with_all = ["codex_executable", "authorize_codex_auth_only"])]
        authorize_native: bool,
        /// Explicit trusted Codex executable; used only for auth-only app-server JSON-RPC.
        #[arg(long, requires_all = ["authorize_codex_auth_only", "host_home", "codex_home"], conflicts_with_all = ["vault", "authorize_native"])]
        codex_executable: Option<PathBuf>,
        /// Explicit non-secret HOME for the authorized auth-only process.
        #[arg(long, requires = "codex_executable", conflicts_with_all = ["vault", "authorize_native"])]
        host_home: Option<PathBuf>,
        /// Explicit existing Codex state directory; selecting it does not export its contents.
        #[arg(long, requires = "codex_executable", conflicts_with_all = ["vault", "authorize_native"])]
        codex_home: Option<PathBuf>,
        /// Authorizes one auth-only status request. Codex may proactively refresh stored credentials
        /// even though the request sets refreshToken=false.
        #[arg(long, requires_all = ["codex_executable", "host_home", "codex_home"], conflicts_with_all = ["vault", "authorize_native"])]
        authorize_codex_auth_only: bool,
    },
    /// Retained session metadata only. Unrecorded outcome never authorizes replay.
    SessionStatus {
        #[arg(long)]
        id: String,
    },
    /// Current permission-filtered retained result; no cached private output replay.
    Result { work: String },
    /// Current permitted shared questions, replies and conclusions; no inference.
    Questions { work: String },
    /// Retained characteristic guidance and optional exact accepted-call evidence.
    Development {
        persona: String,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        invocation: Option<String>,
    },
    /// Deposit exact attributed requester feedback; replay creates no duplicate.
    Feedback { file: PathBuf },
    /// Reply to an exact shared question without inference or an explicit resume.
    AnswerQuestion { file: PathBuf },
    /// One exact explicit participant resume; no inference, pause lift or recovery.
    ResumeParticipant { file: PathBuf },
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
            expected_launch,
            vault,
            authorize_native,
            codex_executable,
            host_home,
            codex_home,
            authorize_codex_auth_only,
        } => {
            if !authorize_run
                || vault.is_some() != authorize_native
                || codex_executable.is_some() != authorize_codex_auth_only
                || codex_executable.is_some() != host_home.is_some()
                || codex_executable.is_some() != codex_home.is_some()
                || (vault.is_some() && codex_executable.is_some())
            {
                emit::<()>(Err(session::BoundaryError::InvalidRequest))
            } else {
                let route = match (vault, codex_executable, host_home, codex_home) {
                    (Some(vault), None, None, None) => SessionRoute::NativeSiwc { vault },
                    (None, Some(executable), Some(host_home), Some(codex_home)) => SessionRoute::CodexAuthOnly {
                        executable,
                        host_home,
                        codex_home,
                        authorize_codex_auth_only,
                    },
                    (None, None, None, None) => SessionRoute::SavedEnvironment,
                    _ => unreachable!("mutually exclusive CLI routes validated"),
                };
                if matches!(&route, SessionRoute::CodexAuthOnly { .. }) {
                    eprintln!(
                        "Notice: Codex may proactively refresh stored credentials even though this auth-only request sets refreshToken=false. The selected CODEX_HOME is a store location; token-bearing status can return an ephemeral bearer to the trusted adapter."
                    );
                }
                let request = SessionRequest {
                    id,
                    operator,
                    work,
                    route,
                };
                if let Some(file) = expected_launch {
                    let reference = (|| -> session::BoundaryResult<ai_personas::store::ObjectRef> {
                        let value: serde_json::Value =
                            ai_personas::runtime::read_json_file(&file, 4096)?;
                        if !value.is_object() {
                            return Err(session::BoundaryError::InvalidRequest);
                        }
                        Ok(serde_json::from_value(value)?)
                    })();
                    match reference {
                        Ok(reference) => {
                            emit(session::run_session_with_launch(&root, &request, &reference).await)
                        }
                        Err(error) => emit::<()>(Err(error)),
                    }
                } else {
                    emit(session::run_session(&root, &request).await)
                }
            }
        }
        Command::SessionStatus { id } => emit(session::session_status(&root, &operator, &id)),
        Command::Result { work } => emit(session::result_view(&root, &operator, &work)),
        Command::Questions { work } => emit(session::questions_view(&root, &operator, &work)),
        Command::Development {
            persona,
            source,
            invocation,
        } => emit(session::development_view(
            &root,
            &operator,
            &persona,
            source.as_deref(),
            invocation.as_deref(),
        )),
        Command::Feedback { file } => emit((|| -> session::BoundaryResult<serde_json::Value> {
            let input = ai_personas::runtime::read_json_file(&file, 65_536)?;
            session::submit_feedback(&root, &operator, &input)
        })()),
        Command::AnswerQuestion { file } => {
            emit((|| -> session::BoundaryResult<serde_json::Value> {
                let input = ai_personas::runtime::read_json_file(&file, 65_536)?;
                session::submit_question_answer(&root, &operator, &input)
            })())
        }
        Command::ResumeParticipant { file } => {
            emit((|| -> session::BoundaryResult<serde_json::Value> {
                let value: serde_json::Value = ai_personas::runtime::read_json_file(&file, 16_384)?;
                for path in ["", "/expected_runtime", "/expected_launch"] {
                    if !value.pointer(path).is_some_and(serde_json::Value::is_object) {
                        return Err(session::BoundaryError::InvalidRequest);
                    }
                }
                let input = serde_json::from_value(value)?;
                session::resume_participant(&root, &operator, &input)
            })())
        }
    };
    if !ok {
        std::process::exit(1);
    }
}
