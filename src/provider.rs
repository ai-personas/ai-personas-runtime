//! Bounded experimental transport targeting Codex CLI 0.160.0, not a complete persona runtime.
//!
//! Dependencies: serde (derive), serde_json, sha2, hex, thiserror, tempfile,
//! tokio (full), and libc on Unix. No shell, custom backend, auth-file reader,
//! retry loop, output-file response channel, or automatic inference probe.
//!
//! The operator supplies an exact executable pin and a verified model catalog.
//! Those inputs are trusted configuration, not facts discovered from a model.
//! Executable, schema and configuration storage require external protection
//! against changes between inspection and consumption. Path digest checks do not
//! bind consumed bytes, interpreters, dependencies, helpers or referenced files.
//!
//! Production dispatch remains unsupported: this module supplies neither verified
//! CLI capability controls nor a trusted artifact-protection attestation scheme.
//! `invoke` requires attestation; `dispatch` also rejects a successful verifier
//! callback until those production prerequisites are implemented and established.
//! Explicit `invoke_research`/`dispatch_research` access requires a configuration
//! permitting research. Observation rejects reported native actions but cannot
//! undo them. Research is never an automatic production fallback.
//! Hard model-context bounds are unsupported.
//!
//! A successful result establishes local transport completion and JSON syntax,
//! not schema conformance, persona-response validity, permission, persistence,
//! delivery, model identity when unreported, or a known bill. The caller must
//! validate the complete response and journal admission before dispatch. This
//! module neither deduplicates invocation IDs nor retries uncertain attempts.
//!
//! Temporary schema files belong in a private operator directory outside all
//! repositories. Explicit completion checks removal; dropping a preparation or
//! cancelling its future provides only best-effort cleanup. Unlinking is not
//! secure erasure. CLI/provider retention and hidden inputs remain unobserved.

use serde::{de, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const SUPPORTED_CLI_VERSION: &str = "0.160.0";
const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROCESS_MS: u64 = 86_400_000;
const MAX_CLEANUP_MS: u64 = 60_000;
/// Fixed discard allowance per pipe after rejection, including the entire read
/// that first discovers a failure. Exhaustion closes the pipe without claiming EOF.
/// This is separate from the finite cleanup deadline and preserves limit APIs.
pub const MAX_CLEANUP_DISCARD_BYTES_PER_PIPE: usize = 64 * 1024;

#[derive(Clone, Serialize)]
pub struct ProviderConfig {
    pub executable: PathBuf,
    pub model: String,
    pub effort: String,
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_response_bytes: usize,
    pub require_hard_context_bound: bool,
    pub require_actionless: bool,
}

pub struct ProviderRequest {
    pub prompt: String,
    pub output_schema: Value,
    pub working_directory: PathBuf,
    pub invocation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ModelAvailability {
    /// The operator has verified availability; actual invocation identity is separate.
    VerifiedAvailable,
    AdvertisedOnly,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelSupport {
    pub availability: ModelAvailability,
    pub efforts: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelCatalog {
    pub entries: BTreeMap<String, ModelSupport>,
    /// Immutable reference to the operator's catalog evidence, not its contents.
    pub evidence_sha256: String,
}

#[derive(Clone, Serialize)]
pub struct ExecutablePin {
    /// Absolute, canonical, operator-approved executable path.
    pub operator_path: PathBuf,
    pub sha256: String,
    /// Version verified by the operator, or by an explicit offline probe.
    pub cli_version: String,
}

#[derive(Clone, Serialize)]
pub struct OperatorPolicy {
    pub executable: ExecutablePin,
    pub temporary_directory: PathBuf,
    /// Exact feature identifiers the operator verified this CLI supports disabling.
    pub supported_disable_features: BTreeSet<String>,
    /// Emitted as global `--disable FEATURE` arguments. No feature is inferred.
    pub disabled_features: BTreeSet<String>,
}

/// All bounds are explicit; no absent setting means unlimited capacity.
#[derive(Clone, Debug, Serialize)]
pub struct AdmissionLimits {
    pub max_prompt_bytes: usize,
    pub max_schema_bytes: usize,
    pub max_schema_nodes: usize,
    pub max_schema_depth: usize,
    pub max_control_bytes: usize,
    pub max_executable_bytes: u64,
    pub max_timeout_ms: u64,
    pub max_stream_bytes: usize,
    pub cleanup_timeout_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Always unknown for this adapter, including subscription-backed use.
    pub actual_cost_microusd: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ActionlessAssurance {
    UnverifiedResearch,
    ExternalAttestation { evidence_sha256: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ObservableLimitation {
    FullPersonaContractNotEstablished,
    ActualContextAndHiddenInputsUnobserved,
    HardTokenAndCostBoundsUnsupported,
    CliAndRemoteRetentionUncontrolled,
    RemoteStopNotConfirmed,
    EscapedProcessGroupsNotContained,
    ExecutableAndEnvironmentIntegrityRequireExternalProtection,
    JsonSchemaComplianceNotLocallyValidated,
    FilesystemMetadataFootprintUnknown,
    BillingUnknownIncludingSubscriptions,
    ReportedModelUnavailable,
    UsagePartlyOrEntirelyUnavailable,
    NativeActionsNotPreventedByResearchProfile,
    FinalMessageInferredFromCompletedTurnWhenPhaseAbsent,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeProfile {
    pub cli_version: String,
    pub actionless: ActionlessAssurance,
    pub hard_model_context_bound: bool,
    pub full_design_conformance: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TransportBytes {
    /// Bytes actually written to the subprocess's stdin, not remote wire bytes.
    pub stdin_written: u64,
    /// Exact schema file payload made available to the CLI.
    pub schema_file: u64,
    /// Local command, environment, executable-path and working-directory bytes.
    pub control_bytes: u64,
    pub stdout_observed: u64,
    pub stderr_observed: u64,
    /// The CLI's network transport is not observable through this interface.
    pub remote_wire_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestAudit {
    pub invocation_id: String,
    pub prompt_sha256: String,
    pub prompt_bytes: u64,
    /// Hash of the exact serialized Value supplied in the schema file, not of
    /// any unavailable original JSON source representation.
    pub schema_sha256: String,
    pub schema_bytes: u64,
    pub executable_sha256: String,
    pub catalog_evidence_sha256: String,
    pub environment_sha256: String,
    pub argv_sha256: String,
    /// Binds actual arguments (including the actual temporary schema path),
    /// stdin/schema digests, cwd, environment digest, policy, and all limits.
    pub configuration_sha256: String,
    pub control_bytes: u64,
    pub schema_allocated_bytes: Option<u64>,
    pub filesystem_metadata_bytes: Option<u64>,
    pub schema_cleanup_confirmed: bool,
}

pub struct ProviderOutput {
    pub response: String,
    pub usage: Usage,
    pub requested_model: String,
    pub reported_model: Option<String>,
    pub requested_effort: String,
    pub transport_bytes: TransportBytes,
    pub exit_status: ProcessExit,
    pub stderr: StderrMetadata,
    pub event_type_counts: BTreeMap<EventType, u64>,
    pub item_type_counts: BTreeMap<ItemType, u64>,
    pub audit: RequestAudit,
    pub process: ProcessReport,
    pub profile: RuntimeProfile,
    pub observable_limitations: Vec<ObservableLimitation>,
}

impl fmt::Debug for ProviderOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderOutput")
            .field("response_bytes", &self.response.len())
            .field("usage", &self.usage)
            .field("exit_status", &self.exit_status)
            .field("profile", &self.profile)
            .finish_non_exhaustive()
    }
}

/// A claim whose authenticity and substantive evidence MUST be checked by an
/// external trusted verifier. Merely constructing this value grants nothing.
#[derive(Clone, Debug, Serialize)]
pub struct ActionlessAttestation {
    pub executable_sha256: String,
    pub configuration_sha256: String,
    pub evidence_sha256: String,
    pub issuer: String,
    pub authenticator: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttestationRejection;

/// Implementations are part of the operator's trusted computing base. They must
/// authenticate the issuer/evidence and establish the exact CLI controls and
/// actionlessness for the bound configuration. Evidence must also bind protection
/// of consumed executable/schema/configuration bytes, interpreters, dependencies
/// and PATH-resolved helpers throughout inspection and execution. Environment
/// value hashes do not authenticate files referenced by those values.
/// A prompt promise or an unchecked caller boolean is not verification. This
/// callback interface alone does not enable the unsupported production profile.
pub trait AttestationVerifier: Send + Sync {
    fn verify(&self, attestation: &ActionlessAttestation) -> Result<(), AttestationRejection>;
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("hard model-context bounds are unsupported by this CLI adapter")]
    UnsupportedHardContextBound,
    #[error("strict actionless execution requires an external exact-configuration attestation")]
    ActionlessAttestationRequired,
    #[error("the actionless capability attestation was rejected")]
    AttestationRejected,
    #[error(
        "production CLI capability controls and protected artifact consumption are not established"
    )]
    UnsupportedProductionProfile,
    #[error("the exact requested model is not verified available")]
    ModelUnavailable,
    #[error("the requested effort is not supported for the exact requested model")]
    EffortUnsupported,
    #[error("invalid provider configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("invalid request: {0}")]
    InvalidRequest(&'static str),
    #[error("admission limit exceeded: {0}")]
    AdmissionLimit(&'static str),
    #[error("the executable does not match its operator pin")]
    ExecutableMismatch,
    #[error("unsupported CLI version")]
    UnsupportedCliVersion,
    #[error("temporary storage is not an eligible private directory outside the repository")]
    InvalidTemporaryDirectory,
    #[error("this platform cannot provide the supported process-group runner")]
    UnsupportedPlatform,
    #[error("preparation belongs to a different provider configuration")]
    ForeignPreparation,
    #[error("local I/O failed during {operation}: {kind:?}")]
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
    #[error("prelaunch rejection also failed to confirm temporary-file cleanup")]
    PrelaunchCleanupFailed { original: Box<ProviderError> },
    #[error("provider attempt failed: {failure:?}")]
    Attempt {
        failure: AttemptFailure,
        receipt: Box<AttemptReceipt>,
    },
    #[error("offline probe failed: {reason:?}")]
    Probe {
        reason: ProbeFailure,
        process: Box<ProcessReport>,
    },
}

#[derive(Clone, Debug)]
pub enum AttemptFailure {
    Process(ProcessFailureKind),
    NonZeroExit,
    ExecutableOrSchemaChanged,
    TemporaryCleanup,
    Protocol(StreamFailure),
}

#[derive(Clone, Debug, Serialize)]
pub struct AttemptReceipt {
    pub audit: RequestAudit,
    pub process: ProcessReport,
    pub stream: StreamSummary,
    pub remote_effects_and_charges_may_remain: bool,
    /// Private diagnostic only: a bounded final candidate from a completed turn
    /// whose structured JSON validation failed (including parser limits). Never serialized as receipt
    /// metadata or treated as successful output or permission to replay.
    #[serde(skip_serializing)]
    pub rejected_final_response: Option<RejectedFinalResponse>,
}

#[derive(Clone)]
pub struct RejectedFinalResponse(String);
impl RejectedFinalResponse {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for RejectedFinalResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RejectedFinalResponse")
            .field("bytes", &self.0.len())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub enum ProbeFailure {
    Process(ProcessFailureKind),
    UnsupportedVersion,
    IndeterminateLoginStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct OfflineProbe {
    pub version: String,
    pub executable_sha256: String,
    pub logged_in: bool,
    pub version_process: ProcessReport,
    pub login_process: ProcessReport,
}

pub struct CodexProvider {
    config: ProviderConfig,
    catalog: ModelCatalog,
    policy: OperatorPolicy,
    limits: AdmissionLimits,
    environment: BTreeMap<String, String>,
    environment_sha256: String,
    binding_sha256: String,
}

impl fmt::Debug for CodexProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexProvider")
            .field("configuration_digest", &self.binding_sha256)
            .finish_non_exhaustive()
    }
}

pub struct PreparedInvocation {
    spec: ProcessSpec,
    schema: NamedTempFile,
    audit: RequestAudit,
    provider_binding: String,
}

impl PreparedInvocation {
    pub fn audit(&self) -> &RequestAudit {
        &self.audit
    }

    pub fn cancel(self) -> Result<(), ProviderError> {
        self.schema
            .close()
            .map_err(|e| local_io("schema cleanup", e))
    }
}

impl fmt::Debug for PreparedInvocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedInvocation")
            .field("configuration_digest", &self.audit.configuration_sha256)
            .finish_non_exhaustive()
    }
}

impl CodexProvider {
    pub fn new(
        config: ProviderConfig,
        catalog: ModelCatalog,
        policy: OperatorPolicy,
        limits: AdmissionLimits,
    ) -> Result<Self, ProviderError> {
        if config.require_hard_context_bound {
            return Err(ProviderError::UnsupportedHardContextBound);
        }
        let support = catalog
            .entries
            .get(&config.model)
            .filter(|s| s.availability == ModelAvailability::VerifiedAvailable)
            .ok_or(ProviderError::ModelUnavailable)?;
        if !support.efforts.contains(&config.effort) {
            return Err(ProviderError::EffortUnsupported);
        }
        if !identifier(&config.model) || !identifier(&config.effort) {
            return Err(ProviderError::InvalidConfiguration(
                "model or effort syntax",
            ));
        }
        if !digest_string(&catalog.evidence_sha256) || !digest_string(&policy.executable.sha256) {
            return Err(ProviderError::InvalidConfiguration("evidence digest"));
        }
        if policy.executable.cli_version != SUPPORTED_CLI_VERSION {
            return Err(ProviderError::UnsupportedCliVersion);
        }
        if !config.executable.is_absolute()
            || config.executable != policy.executable.operator_path
            || !policy.temporary_directory.is_absolute()
        {
            return Err(ProviderError::InvalidConfiguration("operator paths"));
        }
        for feature in &policy.disabled_features {
            if !identifier(feature) || !policy.supported_disable_features.contains(feature) {
                return Err(ProviderError::InvalidConfiguration(
                    "unsupported disable feature",
                ));
            }
        }
        validate_limits(&config, &limits)?;
        let environment = cli_environment()?;
        let environment_sha256 = serialized_digest(&environment)?;
        let binding_sha256 = serialized_digest(&(
            "codex-provider-v1",
            &config,
            &catalog.evidence_sha256,
            support,
            &policy,
            &limits,
            &environment_sha256,
        ))?;
        Ok(Self {
            config,
            catalog,
            policy,
            limits,
            environment,
            environment_sha256,
            binding_sha256,
        })
    }

    /// Creates only a bounded private schema file; it does not launch a process.
    /// Strict capability evidence can be issued against the returned exact audit.
    pub fn prepare(&self, request: ProviderRequest) -> Result<PreparedInvocation, ProviderError> {
        if self.config.require_hard_context_bound {
            return Err(ProviderError::UnsupportedHardContextBound);
        }
        if !cfg!(unix) {
            return Err(ProviderError::UnsupportedPlatform);
        }
        if request.prompt.trim().is_empty() {
            return Err(ProviderError::InvalidRequest("empty prompt"));
        }
        if request.prompt.len() > self.limits.max_prompt_bytes {
            return Err(ProviderError::AdmissionLimit("prompt bytes"));
        }
        if !identifier(&request.invocation_id) {
            return Err(ProviderError::InvalidRequest("invocation identifier"));
        }
        if !request.working_directory.is_absolute() {
            return Err(ProviderError::InvalidRequest(
                "absolute working directory required",
            ));
        }
        let cwd = fs::canonicalize(&request.working_directory)
            .map_err(|e| local_io("working directory", e))?;
        if !cwd.is_dir() {
            return Err(ProviderError::InvalidRequest(
                "working directory is not a directory",
            ));
        }
        let temporary_root = private_temporary_root(&self.policy.temporary_directory, &cwd)?;
        verify_executable(&self.policy.executable, self.limits.max_executable_bytes)?;
        let schema_bytes = serialize_schema(&request.output_schema, &self.limits)?;
        let prompt_sha256 = digest(request.prompt.as_bytes());
        let schema_sha256 = digest(&schema_bytes);
        let mut schema = tempfile::Builder::new()
            .prefix("persona-schema-")
            .suffix(".json")
            .tempfile_in(&temporary_root)
            .map_err(|e| local_io("schema creation", e))?;
        // Every fallible step after creation routes through explicit removal.
        let preparation = (|| -> Result<(ProcessSpec, RequestAudit), ProviderError> {
            schema
                .write_all(&schema_bytes)
                .map_err(|e| local_io("schema write", e))?;
            schema.flush().map_err(|e| local_io("schema flush", e))?;
            let metadata = schema
                .as_file()
                .metadata()
                .map_err(|e| local_io("schema metadata", e))?;
            #[cfg(unix)]
            let allocated = {
                use std::os::unix::fs::MetadataExt;
                metadata.blocks().checked_mul(512)
            };
            #[cfg(not(unix))]
            let allocated = None;
            let _ = metadata;
            let args = self.arguments(schema.path())?;
            let control_bytes =
                control_size(&self.config.executable, &cwd, &args, &self.environment)?;
            if control_bytes > self.limits.max_control_bytes as u64 {
                return Err(ProviderError::AdmissionLimit(
                    "command and environment bytes",
                ));
            }
            let argv_sha256 = serialized_digest(&args)?;
            let configuration_sha256 = serialized_digest(&(
                &self.binding_sha256,
                &cwd,
                &args,
                &prompt_sha256,
                &schema_sha256,
                &request.invocation_id,
            ))?;
            let audit = RequestAudit {
                invocation_id: request.invocation_id,
                prompt_sha256,
                prompt_bytes: request.prompt.len() as u64,
                schema_sha256,
                schema_bytes: schema_bytes.len() as u64,
                executable_sha256: self.policy.executable.sha256.clone(),
                catalog_evidence_sha256: self.catalog.evidence_sha256.clone(),
                environment_sha256: self.environment_sha256.clone(),
                argv_sha256,
                configuration_sha256,
                control_bytes,
                schema_allocated_bytes: allocated,
                filesystem_metadata_bytes: None,
                schema_cleanup_confirmed: false,
            };
            let spec = ProcessSpec {
                executable: self.config.executable.clone(),
                arguments: args.into_iter().map(OsString::from).collect(),
                input: request.prompt.into_bytes(),
                working_directory: cwd,
                environment: self
                    .environment
                    .iter()
                    .map(|(k, v)| (OsString::from(k), OsString::from(v)))
                    .collect(),
                limits: self.process_limits(),
            };
            Ok((spec, audit))
        })();
        match preparation {
            Ok((spec, audit)) => Ok(PreparedInvocation {
                spec,
                schema,
                audit,
                provider_binding: self.binding_sha256.clone(),
            }),
            Err(original) => Err(prelaunch_cleanup(schema, original)),
        }
    }

    /// Production convenience entry point. Without exact-preparation evidence it
    /// refuses before preparation, regardless of whether research is permitted.
    pub async fn invoke(&self, request: ProviderRequest) -> Result<ProviderOutput, ProviderError> {
        let _ = request;
        if self.config.require_hard_context_bound {
            return Err(ProviderError::UnsupportedHardContextBound);
        }
        Err(ProviderError::ActionlessAttestationRequired)
    }

    /// Explicit research execution; reported actions can only be rejected after
    /// observation. This method cannot relax a strict provider configuration.
    pub async fn invoke_research(
        &self,
        request: ProviderRequest,
    ) -> Result<ProviderOutput, ProviderError> {
        if self.config.require_hard_context_bound {
            return Err(ProviderError::UnsupportedHardContextBound);
        }
        if self.config.require_actionless {
            return Err(ProviderError::ActionlessAttestationRequired);
        }
        let prepared = self.prepare(request)?;
        self.dispatch_research(prepared).await
    }

    /// Reserved production boundary. Callback authentication is necessary but
    /// cannot establish the missing supported CLI/protection implementation.
    /// No process is launched, including when a synthetic verifier accepts.
    pub async fn dispatch(
        &self,
        prepared: PreparedInvocation,
        attestation: Option<&ActionlessAttestation>,
        verifier: Option<&dyn AttestationVerifier>,
    ) -> Result<ProviderOutput, ProviderError> {
        let original = if self.config.require_hard_context_bound {
            ProviderError::UnsupportedHardContextBound
        } else if prepared.provider_binding != self.binding_sha256 {
            ProviderError::ForeignPreparation
        } else {
            match self.check_attestation(&prepared.audit, attestation, verifier) {
                Err(error) => error,
                Ok(_) => ProviderError::UnsupportedProductionProfile,
            }
        };
        Err(prelaunch_cleanup(prepared.schema, original))
    }

    /// Explicit research dispatch. No attestation or production assurance is
    /// inferred from successful transport or from the selected arguments.
    pub async fn dispatch_research(
        &self,
        prepared: PreparedInvocation,
    ) -> Result<ProviderOutput, ProviderError> {
        let admission = (|| {
            if self.config.require_hard_context_bound {
                return Err(ProviderError::UnsupportedHardContextBound);
            }
            if prepared.provider_binding != self.binding_sha256 {
                return Err(ProviderError::ForeignPreparation);
            }
            if self.config.require_actionless {
                return Err(ProviderError::ActionlessAttestationRequired);
            }
            let assurance = ActionlessAssurance::UnverifiedResearch;
            verify_executable(&self.policy.executable, self.limits.max_executable_bytes)?;
            if hash_file(prepared.schema.path(), self.limits.max_schema_bytes as u64)?
                != prepared.audit.schema_sha256
            {
                return Err(ProviderError::InvalidRequest("prepared schema changed"));
            }
            Ok(assurance)
        })();
        let assurance = match admission {
            Ok(value) => value,
            Err(original) => return Err(prelaunch_cleanup(prepared.schema, original)),
        };
        let PreparedInvocation {
            spec,
            schema,
            mut audit,
            ..
        } = prepared;
        let mut collector = JsonlCollector::new(
            &self.config.model,
            self.config.max_stdout_bytes,
            self.config.max_response_bytes,
        );
        let run = bounded_process(spec, &mut collector).await;
        let (process, mut failure) = match run {
            Ok(report) => (report, None),
            Err(error) => (error.report, Some(AttemptFailure::Process(error.kind))),
        };
        if failure.is_none()
            && !process
                .exit_status
                .as_ref()
                .is_some_and(ProcessExit::success)
        {
            failure = Some(AttemptFailure::NonZeroExit);
        }
        // A postcheck detects some changes; it is not a substitute for externally
        // protected executable/configuration storage during the launch interval.
        let unchanged =
            verify_executable(&self.policy.executable, self.limits.max_executable_bytes).is_ok()
                && hash_file(schema.path(), self.limits.max_schema_bytes as u64)
                    .is_ok_and(|hash| hash == audit.schema_sha256);
        if !unchanged && failure.is_none() {
            failure = Some(AttemptFailure::ExecutableOrSchemaChanged);
        }
        audit.schema_cleanup_confirmed = schema.close().is_ok();
        if !audit.schema_cleanup_confirmed && failure.is_none() {
            failure = Some(AttemptFailure::TemporaryCleanup);
        }
        let summary = collector.summary.clone();
        if let Some(failure) = failure {
            let rejected_final_response = if unchanged
                && audit.schema_cleanup_confirmed
                && matches!(
                    failure,
                    AttemptFailure::Process(ProcessFailureKind::Sink(
                        StreamFailure::InvalidStructuredResponse
                    ))
                ) {
                collector.take_rejected_final_response()
            } else {
                None
            };
            return Err(ProviderError::Attempt {
                failure,
                receipt: Box::new(AttemptReceipt {
                    audit,
                    process,
                    stream: summary,
                    remote_effects_and_charges_may_remain: true,
                    rejected_final_response,
                }),
            });
        }
        let response = match collector.take_response() {
            Ok(response) => response,
            Err(error) => {
                return Err(ProviderError::Attempt {
                    failure: AttemptFailure::Protocol(error),
                    receipt: Box::new(AttemptReceipt {
                        audit,
                        process,
                        stream: summary,
                        remote_effects_and_charges_may_remain: true,
                        rejected_final_response: None,
                    }),
                })
            }
        };
        let mut limitations = vec![
            ObservableLimitation::FullPersonaContractNotEstablished,
            ObservableLimitation::ActualContextAndHiddenInputsUnobserved,
            ObservableLimitation::HardTokenAndCostBoundsUnsupported,
            ObservableLimitation::CliAndRemoteRetentionUncontrolled,
            ObservableLimitation::RemoteStopNotConfirmed,
            ObservableLimitation::EscapedProcessGroupsNotContained,
            ObservableLimitation::ExecutableAndEnvironmentIntegrityRequireExternalProtection,
            ObservableLimitation::JsonSchemaComplianceNotLocallyValidated,
            ObservableLimitation::FilesystemMetadataFootprintUnknown,
            ObservableLimitation::BillingUnknownIncludingSubscriptions,
        ];
        if assurance == ActionlessAssurance::UnverifiedResearch {
            limitations.push(ObservableLimitation::NativeActionsNotPreventedByResearchProfile);
        }
        if summary.reported_model.is_none() {
            limitations.push(ObservableLimitation::ReportedModelUnavailable);
        }
        if summary.usage.input_tokens.is_none()
            || summary.usage.output_tokens.is_none()
            || summary.usage.cached_input_tokens.is_none()
        {
            limitations.push(ObservableLimitation::UsagePartlyOrEntirelyUnavailable);
        }
        if summary.final_phase_inferred {
            limitations
                .push(ObservableLimitation::FinalMessageInferredFromCompletedTurnWhenPhaseAbsent);
        }
        let exit_status = process
            .exit_status
            .clone()
            .ok_or_else(|| ProviderError::Attempt {
                failure: AttemptFailure::NonZeroExit,
                receipt: Box::new(AttemptReceipt {
                    audit: audit.clone(),
                    process: process.clone(),
                    stream: summary.clone(),
                    remote_effects_and_charges_may_remain: true,
                    rejected_final_response: None,
                }),
            })?;
        Ok(ProviderOutput {
            response,
            usage: summary.usage,
            requested_model: self.config.model.clone(),
            reported_model: summary.reported_model,
            requested_effort: self.config.effort.clone(),
            transport_bytes: TransportBytes {
                stdin_written: process.stdin_bytes_written,
                schema_file: audit.schema_bytes,
                control_bytes: audit.control_bytes,
                stdout_observed: process.stdout_bytes_observed,
                stderr_observed: process.stderr.observed_bytes,
                remote_wire_bytes: None,
            },
            exit_status,
            stderr: process.stderr.clone(),
            event_type_counts: summary.event_type_counts,
            item_type_counts: summary.item_type_counts,
            audit,
            process,
            profile: RuntimeProfile {
                cli_version: SUPPORTED_CLI_VERSION.into(),
                actionless: assurance,
                hard_model_context_bound: false,
                full_design_conformance: false,
            },
            observable_limitations: limitations,
        })
    }

    fn check_attestation(
        &self,
        audit: &RequestAudit,
        attestation: Option<&ActionlessAttestation>,
        verifier: Option<&dyn AttestationVerifier>,
    ) -> Result<ActionlessAssurance, ProviderError> {
        let Some(attestation) = attestation else {
            return Err(ProviderError::ActionlessAttestationRequired);
        };
        let verifier = verifier.ok_or(ProviderError::AttestationRejected)?;
        if attestation.executable_sha256 != audit.executable_sha256
            || attestation.configuration_sha256 != audit.configuration_sha256
            || !digest_string(&attestation.evidence_sha256)
            || !identifier(&attestation.issuer)
            || attestation.authenticator.is_empty()
            || attestation.authenticator.len() > 16 * 1024
        {
            return Err(ProviderError::AttestationRejected);
        }
        verifier
            .verify(attestation)
            .map_err(|_| ProviderError::AttestationRejected)?;
        Ok(ActionlessAssurance::ExternalAttestation {
            evidence_sha256: attestation.evidence_sha256.clone(),
        })
    }

    fn arguments(&self, schema: &Path) -> Result<Vec<String>, ProviderError> {
        let mut args = vec!["-a".into(), "never".into()];
        for feature in &self.policy.disabled_features {
            args.push("--disable".into());
            args.push(feature.clone());
        }
        args.extend([
            "exec".into(),
            "--ignore-user-config".into(),
            "--ignore-rules".into(),
            "--ephemeral".into(),
            "-s".into(),
            "read-only".into(),
            "-m".into(),
            self.config.model.clone(),
            "-c".into(),
            format!(
                "model_reasoning_effort={}",
                serde_json::to_string(&self.config.effort)
                    .map_err(|_| ProviderError::InvalidConfiguration("effort encoding"))?
            ),
            "--json".into(),
            "--color".into(),
            "never".into(),
            "--output-schema".into(),
            path_text(schema)?.into(),
            "-".into(),
        ]);
        Ok(args)
    }

    fn process_limits(&self) -> ProcessLimits {
        ProcessLimits {
            timeout_ms: self.config.timeout_ms,
            cleanup_timeout_ms: self.limits.cleanup_timeout_ms,
            max_input_bytes: self.limits.max_prompt_bytes,
            max_stdout_bytes: self.config.max_stdout_bytes,
            max_stderr_bytes: self.config.max_stderr_bytes,
        }
    }

    /// Explicit, bounded CLI version and login-status probes. Neither uses an
    /// inference command. Login contents are discarded; only documented exit
    /// codes 0/1 are interpreted. The CLI, not this adapter, reads its own auth.
    pub async fn probe_offline(
        &self,
        working_directory: PathBuf,
    ) -> Result<OfflineProbe, ProviderError> {
        verify_executable(&self.policy.executable, self.limits.max_executable_bytes)?;
        let make_spec = |arguments: Vec<OsString>| ProcessSpec {
            executable: self.config.executable.clone(),
            arguments,
            input: Vec::new(),
            working_directory: working_directory.clone(),
            environment: self
                .environment
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v)))
                .collect(),
            limits: self.process_limits(),
        };
        let mut version_sink = VersionSink::default();
        let version_process =
            bounded_process(make_spec(vec!["--version".into()]), &mut version_sink)
                .await
                .map_err(|e| ProviderError::Probe {
                    reason: ProbeFailure::Process(e.kind),
                    process: Box::new(e.report),
                })?;
        if !version_process
            .exit_status
            .as_ref()
            .is_some_and(ProcessExit::success)
            || !version_sink.matches()
        {
            return Err(ProviderError::Probe {
                reason: ProbeFailure::UnsupportedVersion,
                process: Box::new(version_process),
            });
        }
        verify_executable(&self.policy.executable, self.limits.max_executable_bytes)?;
        let login_process = bounded_process(
            make_spec(vec!["login".into(), "status".into()]),
            &mut DiscardStdout,
        )
        .await
        .map_err(|e| ProviderError::Probe {
            reason: ProbeFailure::Process(e.kind),
            process: Box::new(e.report),
        })?;
        let logged_in = match login_process.exit_status.as_ref() {
            Some(ProcessExit {
                code: Some(0),
                signal: None,
            }) => true,
            Some(ProcessExit {
                code: Some(1),
                signal: None,
            }) => false,
            _ => {
                return Err(ProviderError::Probe {
                    reason: ProbeFailure::IndeterminateLoginStatus,
                    process: Box::new(login_process),
                })
            }
        };
        verify_executable(&self.policy.executable, self.limits.max_executable_bytes)?;
        Ok(OfflineProbe {
            version: SUPPORTED_CLI_VERSION.into(),
            executable_sha256: self.policy.executable.sha256.clone(),
            logged_in,
            version_process,
            login_process,
        })
    }
}

fn validate_limits(config: &ProviderConfig, limits: &AdmissionLimits) -> Result<(), ProviderError> {
    if limits.max_prompt_bytes == 0
        || limits.max_prompt_bytes > MAX_BUFFER_BYTES
        || limits.max_schema_bytes == 0
        || limits.max_schema_bytes > MAX_BUFFER_BYTES
        || limits.max_schema_nodes == 0
        || limits.max_schema_nodes > 1_000_000
        || limits.max_schema_depth == 0
        || limits.max_schema_depth > 64
        || limits.max_control_bytes == 0
        || limits.max_control_bytes > MAX_BUFFER_BYTES
        || limits.max_executable_bytes == 0
        || limits.max_executable_bytes == u64::MAX
        || limits.max_stream_bytes == 0
        || limits.max_stream_bytes > MAX_BUFFER_BYTES
        || limits.max_timeout_ms == 0
        || limits.max_timeout_ms > MAX_PROCESS_MS
        || limits.cleanup_timeout_ms == 0
        || limits.cleanup_timeout_ms > MAX_CLEANUP_MS
        || config.timeout_ms == 0
        || config.timeout_ms > limits.max_timeout_ms
        || config.max_stdout_bytes == 0
        || config.max_stdout_bytes > limits.max_stream_bytes
        || config.max_stderr_bytes > limits.max_stream_bytes
        || config.max_response_bytes == 0
        || config.max_response_bytes > config.max_stdout_bytes
    {
        return Err(ProviderError::InvalidConfiguration("resource limits"));
    }
    Ok(())
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

fn digest_string(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn local_io(operation: &'static str, error: io::Error) -> ProviderError {
    ProviderError::Io {
        operation,
        kind: error.kind(),
    }
}

fn prelaunch_cleanup(schema: NamedTempFile, original: ProviderError) -> ProviderError {
    match schema.close() {
        Ok(()) => original,
        Err(_) => ProviderError::PrelaunchCleanupFailed {
            original: Box::new(original),
        },
    }
}

fn path_text(path: &Path) -> Result<&str, ProviderError> {
    path.to_str()
        .ok_or(ProviderError::InvalidConfiguration("non-UTF-8 path"))
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_digest(value: &impl Serialize) -> Result<String, ProviderError> {
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| ProviderError::InvalidConfiguration("audit serialization"))?;
    Ok(hex::encode(writer.0.finalize()))
}

fn hash_file(path: &Path, maximum: u64) -> Result<String, ProviderError> {
    let mut file = File::open(path).map_err(|e| local_io("pinned file open", e))?;
    let metadata = file
        .metadata()
        .map_err(|e| local_io("pinned file metadata", e))?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(ProviderError::AdmissionLimit("pinned file bytes"));
    }
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| local_io("pinned file read", e))?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(ProviderError::AdmissionLimit("pinned file bytes"))?;
        if total > maximum {
            return Err(ProviderError::AdmissionLimit("pinned file bytes"));
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn verify_executable(pin: &ExecutablePin, maximum: u64) -> Result<(), ProviderError> {
    let canonical =
        fs::canonicalize(&pin.operator_path).map_err(|e| local_io("executable resolution", e))?;
    if canonical != pin.operator_path || hash_file(&canonical, maximum)? != pin.sha256 {
        return Err(ProviderError::ExecutableMismatch);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&canonical)
            .map_err(|e| local_io("executable permissions", e))?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(ProviderError::ExecutableMismatch);
        }
    }
    Ok(())
}

fn cli_environment() -> Result<BTreeMap<String, String>, ProviderError> {
    // Intentionally do not inherit API keys, backend overrides, arbitrary CODEX
    // settings, tracing variables, or loader injection variables. Existing CLI
    // authentication remains in CLI-owned storage; this code never opens it.
    let mut environment = BTreeMap::new();
    for name in [
        "HOME",
        "USERPROFILE",
        "CODEX_HOME",
        "XDG_CONFIG_HOME",
        "PATH",
        "SystemRoot",
        "WINDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            let value = value
                .into_string()
                .map_err(|_| ProviderError::InvalidConfiguration("non-UTF-8 environment"))?;
            environment.insert(name.to_owned(), value);
        }
    }
    Ok(environment)
}

fn control_size(
    executable: &Path,
    cwd: &Path,
    args: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<u64, ProviderError> {
    let mut total = 0_u64;
    let mut add = |n: usize| -> Result<(), ProviderError> {
        total = total
            .checked_add(n as u64)
            .and_then(|v| v.checked_add(1))
            .ok_or(ProviderError::AdmissionLimit("control bytes"))?;
        Ok(())
    };
    add(path_text(executable)?.len())?;
    add(path_text(cwd)?.len())?;
    for arg in args {
        add(arg.len())?;
    }
    for (key, value) in environment {
        add(key.len())?;
        add(value.len())?;
    }
    Ok(total)
}

fn private_temporary_root(path: &Path, cwd: &Path) -> Result<PathBuf, ProviderError> {
    let root = fs::canonicalize(path).map_err(|e| local_io("temporary directory", e))?;
    if !root.is_dir() || root.starts_with(cwd) {
        return Err(ProviderError::InvalidTemporaryDirectory);
    }
    // These checks supplement, not replace, the operator's requirement to select
    // storage outside every repository (including repositories without markers).
    for ancestor in root.ancestors() {
        for marker in [".git", ".hg", ".svn"] {
            match fs::symlink_metadata(ancestor.join(marker)) {
                Ok(_) => return Err(ProviderError::InvalidTemporaryDirectory),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(local_io("repository boundary", e)),
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&root)
            .map_err(|e| local_io("temporary permissions", e))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(ProviderError::InvalidTemporaryDirectory);
        }
    }
    Ok(root)
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    maximum: usize,
    overflow: bool,
}
impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            self.overflow = true;
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialize_schema(value: &Value, limits: &AdmissionLimits) -> Result<Vec<u8>, ProviderError> {
    if !value.is_object() && !value.is_boolean() {
        return Err(ProviderError::InvalidRequest(
            "schema root must be an object or boolean",
        ));
    }
    // Bound depth and the pending traversal before serialization can recurse.
    let mut pending = vec![(value, 0_usize)];
    let mut nodes = 0_usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > limits.max_schema_nodes || depth > limits.max_schema_depth {
            return Err(ProviderError::AdmissionLimit("schema structure"));
        }
        let children: Box<dyn Iterator<Item = &Value> + '_> = match value {
            Value::Array(values) => Box::new(values.iter()),
            Value::Object(values) => Box::new(values.values()),
            _ => Box::new(std::iter::empty()),
        };
        for child in children {
            if pending.len() >= limits.max_schema_nodes.saturating_sub(nodes) {
                return Err(ProviderError::AdmissionLimit("schema structure"));
            }
            pending.push((child, depth + 1));
        }
    }
    let mut output = LimitedBuffer {
        bytes: Vec::new(),
        maximum: limits.max_schema_bytes,
        overflow: false,
    };
    if serde_json::to_writer(&mut output, value).is_err() {
        return Err(if output.overflow {
            ProviderError::AdmissionLimit("schema bytes")
        } else {
            ProviderError::InvalidRequest("schema serialization")
        });
    }
    Ok(output.bytes)
}

// ---- Generic bounded local process runner ---------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct ProcessLimits {
    pub timeout_ms: u64,
    pub cleanup_timeout_ms: u64,
    pub max_input_bytes: usize,
    /// Rejection threshold. Total reads are bounded by this plus one detection
    /// byte and MAX_CLEANUP_DISCARD_BYTES_PER_PIPE, not by this value alone.
    pub max_stdout_bytes: usize,
    /// Same accounting as stdout; stderr contents are never retained.
    pub max_stderr_bytes: usize,
}

/// A generic runner specification, intentionally without Debug. Its caller owns
/// executable authorization. The Codex adapter independently restricts that path.
/// Environment inheritance is disabled; only these exact entries are supplied.
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub input: Vec<u8>,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub limits: ProcessLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}
impl ProcessExit {
    pub fn success(&self) -> bool {
        self.code == Some(0) && self.signal.is_none()
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StderrMetadata {
    /// Bytes actually drained. If eof is false, this is only a lower bound.
    pub observed_bytes: u64,
    pub eof: bool,
    pub exceeded_limit: bool,
    /// Always zero: stderr contents are never returned or logged.
    pub retained_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum GroupSignal {
    Sent,
    AlreadyGone,
    Failed,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StopMetadata {
    pub process_group_sigkill: Option<GroupSignal>,
    pub direct_kill_requested: bool,
    pub cleanup_deadline_reached: bool,
    pub stdout_cleanup_budget_exhausted: bool,
    pub stderr_cleanup_budget_exhausted: bool,
    pub remote_stop_confirmed: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ProcessReport {
    pub launched: bool,
    pub stdin_bytes_written: u64,
    pub stdout_bytes_observed: u64,
    pub stdout_eof: bool,
    pub stderr: StderrMetadata,
    pub exit_status: Option<ProcessExit>,
    pub stop: StopMetadata,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessFailureKind {
    InvalidLimits,
    InvalidPaths,
    InputTooLarge,
    UnsupportedPlatform,
    Spawn(io::ErrorKind),
    StdoutRead(io::ErrorKind),
    StderrRead(io::ErrorKind),
    StdinWrite(io::ErrorKind),
    Wait(io::ErrorKind),
    StdoutOverflow,
    StderrOverflow,
    Timeout,
    DrainIncomplete,
    GroupTerminationFailed,
    Sink(StreamFailure),
}

#[derive(Debug, Error)]
#[error("bounded process failed: {kind:?}")]
pub struct ProcessFailure {
    pub kind: ProcessFailureKind,
    pub report: ProcessReport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFailure {
    MalformedJsonl,
    TruncatedJsonl,
    LineTooLarge,
    UnexpectedEvent,
    InvalidEventOrder,
    NativeActionObserved,
    UnsupportedItem,
    ProviderReportedFailure,
    ReportedModelMismatch,
    ResponseTooLarge,
    MissingCompletion,
    MissingFinalResponse,
    InvalidStructuredResponse,
}

/// The runner never accumulates stdout. A sink must keep its own retained data
/// bounded and must not perform blocking work. Failures contain no payload text.
pub trait StdoutSink {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), StreamFailure>;
    fn finish(&mut self) -> Result<(), StreamFailure> {
        Ok(())
    }
}

pub struct DiscardStdout;
impl StdoutSink for DiscardStdout {
    fn bytes(&mut self, _: &[u8]) -> Result<(), StreamFailure> {
        Ok(())
    }
}

/// Drains both output pipes concurrently with stdin writes and process waiting.
/// Overflow, parser rejection, or timeout kills the owned Linux process group.
/// Linux waitid(WNOWAIT) observes leader exit without releasing its identity;
/// group signaling is disarmed before any reaping operation is polled.
/// The caller must preserve exclusive reaping ownership: no competing waiters,
/// SIGCHLD auto-reaping, or concurrent changes to that process-wide disposition.
/// Other platforms remain unsupported until an equivalent runner is supplied.
/// Cleanup has a finite deadline and a fixed discard allowance per pipe. The
/// entire read discovering rejection is charged to that allowance. Exhaustion
/// closes the pipe and retains eof=false; observed bytes remain a lower bound.
/// Descendants that escape the group and remote inference are not contained.
/// Dropping this future signals only an armed, unreaped group and leaves Tokio
/// best-effort child reaping. It yields no receipt and must be journaled as unknown.
pub async fn bounded_process(
    spec: ProcessSpec,
    sink: &mut dyn StdoutSink,
) -> Result<ProcessReport, ProcessFailure> {
    #[cfg(target_os = "linux")]
    {
        run_unix(spec, sink).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (spec, sink);
        Err(ProcessFailure {
            kind: ProcessFailureKind::UnsupportedPlatform,
            report: ProcessReport::default(),
        })
    }
}

#[cfg(target_os = "linux")]
struct OwnedProcessGroup {
    pid: libc::pid_t,
    armed: bool,
}

#[cfg(target_os = "linux")]
impl OwnedProcessGroup {
    fn signal(&mut self) -> GroupSignal {
        // Disarming precedes both the syscall and every poll of Child::wait.
        // A repeated call, including after reaping, never uses the numeric ID.
        if !self.armed {
            return GroupSignal::Failed;
        }
        self.armed = false;
        // SAFETY: process_group(0) created this group, pid > 1, and exclusive
        // reaping ownership keeps its leader's PID reserved until after this
        // call. WNOWAIT observes exit without relinquishing that reservation.
        let result = unsafe { libc::kill(-self.pid, libc::SIGKILL) };
        if result == 0 {
            GroupSignal::Sent
        } else if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            GroupSignal::AlreadyGone
        } else {
            GroupSignal::Failed
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for OwnedProcessGroup {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.signal();
        }
    }
}

#[cfg(target_os = "linux")]
fn leader_exited_without_reaping(pid: libc::pid_t) -> io::Result<bool> {
    loop {
        // SAFETY: zero initialization is valid for this output structure;
        // waitid writes into it synchronously. P_PID selects our direct child.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            // SAFETY: successful waitid initializes the child-status fields;
            // si_pid remains zero when WNOHANG observes no exit.
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(target_os = "linux")]
async fn run_unix(
    spec: ProcessSpec,
    sink: &mut dyn StdoutSink,
) -> Result<ProcessReport, ProcessFailure> {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let mut report = ProcessReport::default();
    let reject = |kind| ProcessFailure {
        kind,
        report: ProcessReport::default(),
    };
    if spec.limits.timeout_ms == 0
        || spec.limits.timeout_ms > MAX_PROCESS_MS
        || spec.limits.cleanup_timeout_ms == 0
        || spec.limits.cleanup_timeout_ms > MAX_CLEANUP_MS
        || spec.limits.max_input_bytes > MAX_BUFFER_BYTES
        || spec.limits.max_stdout_bytes > MAX_BUFFER_BYTES
        || spec.limits.max_stderr_bytes > MAX_BUFFER_BYTES
    {
        return Err(reject(ProcessFailureKind::InvalidLimits));
    }
    if spec.input.len() > spec.limits.max_input_bytes {
        return Err(reject(ProcessFailureKind::InputTooLarge));
    }
    if !spec.executable.is_absolute() || !spec.working_directory.is_absolute() {
        return Err(reject(ProcessFailureKind::InvalidPaths));
    }
    let started = Instant::now();
    let deadline = started
        .checked_add(Duration::from_millis(spec.limits.timeout_ms))
        .ok_or_else(|| reject(ProcessFailureKind::InvalidLimits))?;
    let mut command = tokio::process::Command::new(&spec.executable);
    command
        .args(&spec.arguments)
        .current_dir(&spec.working_directory)
        .env_clear()
        .envs(&spec.environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // The identity-aware group guard owns cancellation signaling. Tokio
        // must not independently signal a cached PID after identity loss.
        .kill_on_drop(false);
    command.as_std_mut().process_group(0);
    let mut child = command
        .spawn()
        .map_err(|e| reject(ProcessFailureKind::Spawn(e.kind())))?;
    report.launched = true;
    let pid = match child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .filter(|id| *id > 1)
    {
        Some(pid) => pid,
        None => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(
                Duration::from_millis(spec.limits.cleanup_timeout_ms),
                child.wait(),
            )
            .await;
            return Err(ProcessFailure {
                kind: ProcessFailureKind::GroupTerminationFailed,
                report,
            });
        }
    };
    // Declaration order also makes cancellation drop the guard before Child.
    let mut group = OwnedProcessGroup { pid, armed: true };
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut stdout_buffer = [0_u8; 8192];
    let mut stderr_buffer = [0_u8; 8192];
    let mut stdout_cleanup_remaining = MAX_CLEANUP_DISCARD_BYTES_PER_PIPE;
    let mut stderr_cleanup_remaining = MAX_CLEANUP_DISCARD_BYTES_PER_PIPE;
    let mut input_position = 0_usize;
    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut leader_exited = false;
    let mut wait_done = false;
    let mut failure: Option<ProcessFailureKind> = None;
    let mut cleanup_deadline: Option<Instant> = None;
    loop {
        if input_position == spec.input.len() {
            stdin.take();
        }
        if group.armed {
            match leader_exited_without_reaping(pid) {
                Ok(exited) => leader_exited = exited,
                Err(error) => {
                    // In particular, ECHILD means identity ownership is lost.
                    // Do not signal a numeric ID on this error or during Drop.
                    group.armed = false;
                    failure.get_or_insert(ProcessFailureKind::Wait(error.kind()));
                }
            }
        }
        if (failure.is_some() || leader_exited) && cleanup_deadline.is_none() {
            stdin.take();
            if group.armed {
                let signal = group.signal();
                if signal == GroupSignal::Failed {
                    failure.get_or_insert(ProcessFailureKind::GroupTerminationFailed);
                    report.stop.direct_kill_requested = true;
                    // No reaping operation has yet been polled.
                    let _ = child.start_kill();
                }
                report.stop.process_group_sigkill = Some(signal);
            }
            cleanup_deadline =
                Some(Instant::now() + Duration::from_millis(spec.limits.cleanup_timeout_ms));
        }
        if failure.is_some() {
            if !stdout_done && stdout_cleanup_remaining == 0 {
                stdout.take();
                stdout_done = true;
                report.stop.stdout_cleanup_budget_exhausted = true;
            }
            if !stderr_done && stderr_cleanup_remaining == 0 {
                stderr.take();
                stderr_done = true;
                report.stop.stderr_cleanup_budget_exhausted = true;
            }
        }
        if wait_done && stdout_done && stderr_done {
            break;
        }
        let stdout_read_size = stdout_buffer.len().min(if failure.is_some() {
            stdout_cleanup_remaining
        } else {
            (spec.limits.max_stdout_bytes + 1).saturating_sub(report.stdout_bytes_observed as usize)
        });
        let stderr_read_size = stderr_buffer.len().min(if failure.is_some() {
            stderr_cleanup_remaining
        } else {
            (spec.limits.max_stderr_bytes + 1).saturating_sub(report.stderr.observed_bytes as usize)
        });
        let current_deadline = cleanup_deadline.unwrap_or(deadline);
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(current_deadline)) => {
                if cleanup_deadline.is_some() {
                    report.stop.cleanup_deadline_reached = true;
                    failure.get_or_insert(ProcessFailureKind::DrainIncomplete);
                    break;
                }
                failure.get_or_insert(ProcessFailureKind::Timeout);
            }
            // Idle children need an observation wakeup even when pipes stay open.
            _ = tokio::time::sleep(Duration::from_millis(10)), if group.armed => {}
            result = async {
                stderr.as_mut().expect("piped stderr").read(&mut stderr_buffer[..stderr_read_size]).await
            }, if !stderr_done => {
                match result {
                    Ok(0) => {
                        stderr.take();
                        stderr_done = true;
                        report.stderr.eof = true;
                    }
                    Ok(n) => {
                        report.stderr.observed_bytes += n as u64;
                        if report.stderr.observed_bytes > spec.limits.max_stderr_bytes as u64 {
                            report.stderr.exceeded_limit = true;
                            failure.get_or_insert(ProcessFailureKind::StderrOverflow);
                        }
                        if failure.is_some() {
                            stderr_cleanup_remaining = stderr_cleanup_remaining.saturating_sub(n);
                        }
                    }
                    Err(e) => {
                        stderr.take();
                        stderr_done = true;
                        failure.get_or_insert(ProcessFailureKind::StderrRead(e.kind()));
                    }
                }
                stderr_buffer.fill(0);
            }
            result = async {
                stdout.as_mut().expect("piped stdout").read(&mut stdout_buffer[..stdout_read_size]).await
            }, if !stdout_done => {
                match result {
                    Ok(0) => {
                        stdout.take();
                        stdout_done = true;
                        report.stdout_eof = true;
                        if failure.is_none() {
                            if let Err(error) = sink.finish() {
                                failure = Some(ProcessFailureKind::Sink(error));
                            }
                        }
                    }
                    Ok(n) => {
                        report.stdout_bytes_observed += n as u64;
                        if report.stdout_bytes_observed > spec.limits.max_stdout_bytes as u64 {
                            failure.get_or_insert(ProcessFailureKind::StdoutOverflow);
                        } else if failure.is_none() {
                            if let Err(error) = sink.bytes(&stdout_buffer[..n]) {
                                failure = Some(ProcessFailureKind::Sink(error));
                            }
                        }
                        if failure.is_some() {
                            // Charge the whole failing read, including bytes that
                            // preceded rejection within this same buffer.
                            stdout_cleanup_remaining = stdout_cleanup_remaining.saturating_sub(n);
                        }
                    }
                    Err(e) => {
                        stdout.take();
                        stdout_done = true;
                        failure.get_or_insert(ProcessFailureKind::StdoutRead(e.kind()));
                    }
                }
                stdout_buffer.fill(0);
            }
            result = async {
                match stdin.as_mut() {
                    Some(pipe) => pipe.write(&spec.input[input_position..]).await,
                    None => Ok(0),
                }
            }, if stdin.is_some() && failure.is_none() => {
                match result {
                    Ok(0) => { failure = Some(ProcessFailureKind::StdinWrite(io::ErrorKind::WriteZero)); }
                    Ok(n) => {
                        input_position += n;
                        report.stdin_bytes_written += n as u64;
                    }
                    Err(e) => { failure = Some(ProcessFailureKind::StdinWrite(e.kind())); }
                }
            }
            // The guard is already disarmed whenever this future can be polled.
            result = child.wait(), if cleanup_deadline.is_some() && !wait_done => {
                wait_done = true;
                match result {
                    Ok(status) => {
                        report.exit_status = Some(ProcessExit { code: status.code(), signal: status.signal() });
                        if input_position != spec.input.len() {
                            failure.get_or_insert(ProcessFailureKind::StdinWrite(io::ErrorKind::BrokenPipe));
                        }
                    }
                    Err(e) => { failure.get_or_insert(ProcessFailureKind::Wait(e.kind())); }
                }
            }
        }
    }
    report.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    if let Some(kind) = failure {
        Err(ProcessFailure { kind, report })
    } else {
        Ok(report)
    }
}

// ---- Payload-minimizing JSONL interpretation -------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum EventType {
    #[serde(rename = "thread.started")]
    ThreadStarted,
    #[serde(rename = "turn.started")]
    TurnStarted,
    #[serde(rename = "item.started")]
    ItemStarted,
    #[serde(rename = "item.updated")]
    ItemUpdated,
    #[serde(rename = "item.completed")]
    ItemCompleted,
    #[serde(rename = "turn.completed")]
    TurnCompleted,
    #[serde(rename = "turn.failed")]
    TurnFailed,
    #[serde(rename = "error")]
    Error,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum ItemType {
    #[serde(rename = "agent_message")]
    AssistantMessage,
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(rename = "error")]
    Error,
    // Explicit action-labelled kinds. Recognition is not CLI compatibility
    // evidence, nor proof that the reported operation actually executed.
    #[serde(rename = "command_execution")]
    CommandExecution,
    #[serde(rename = "file_change")]
    FileChange,
    #[serde(rename = "mcp_tool_call")]
    McpToolCall,
    #[serde(rename = "web_search")]
    WebSearch,
    /// Legacy public variant retained for source compatibility; never decoded.
    #[serde(skip_deserializing)]
    NativeOrUnknown,
    /// Unrecognized kinds fail closed without asserting a native action.
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Deserialize)]
enum MessagePhase {
    #[serde(rename = "commentary")]
    Commentary,
    #[serde(rename = "final_answer")]
    Final,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct ItemHeader {
    #[serde(rename = "type")]
    kind: ItemType,
    phase: Option<MessagePhase>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct EventHeader {
    #[serde(rename = "type")]
    kind: EventType,
    item: Option<ItemHeader>,
    model: Option<String>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct TextEvent {
    item: TextItem,
}
#[derive(Deserialize)]
struct TextItem {
    text: String,
}

/// A coarse hint from provider-reported error text, not proof of root cause.
/// Original messages and arbitrary codes are never retained in receipts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailureHint {
    Schema,
    RateLimit,
    Authentication,
    ModelUnavailable,
    Network,
    Other,
}

fn provider_failure_hint(bytes: &[u8]) -> ProviderFailureHint {
    #[derive(Deserialize)]
    struct Detail {
        message: Option<String>,
        text: Option<String>,
        code: Option<String>,
    }
    #[derive(Deserialize)]
    struct Projection {
        message: Option<String>,
        error: Option<Detail>,
        item: Option<Detail>,
    }
    let Ok(projected) = serde_json::from_slice::<Projection>(bytes) else {
        return ProviderFailureHint::Other;
    };
    let mut fields = Vec::new();
    if let Some(message) = projected.message {
        fields.push(message);
    }
    for detail in [projected.error, projected.item].into_iter().flatten() {
        fields.extend(
            [detail.message, detail.text, detail.code]
                .into_iter()
                .flatten(),
        );
    }
    for field in fields {
        let text = field.to_ascii_lowercase();
        if text.contains("invalid_json_schema")
            || text.contains("invalid schema")
            || text.contains("schema for response_format")
            || text.contains("schema for text.format")
        {
            return ProviderFailureHint::Schema;
        }
        if text.contains("rate_limit")
            || text.contains("rate limit")
            || text.contains("insufficient_quota")
        {
            return ProviderFailureHint::RateLimit;
        }
        if text.contains("authentication_error")
            || text.contains("unauthorized")
            || text.contains("not logged in")
        {
            return ProviderFailureHint::Authentication;
        }
        if text.contains("model_not_found") || text.contains("model is not supported") {
            return ProviderFailureHint::ModelUnavailable;
        }
        if text.contains("connection") || text.contains("network") || text.contains("timed out") {
            return ProviderFailureHint::Network;
        }
    }
    ProviderFailureHint::Other
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StreamSummary {
    pub usage: Usage,
    pub reported_model: Option<String>,
    pub event_type_counts: BTreeMap<EventType, u64>,
    pub item_type_counts: BTreeMap<ItemType, u64>,
    pub native_action_observed: bool,
    pub unsupported_item_observed: bool,
    pub final_phase_inferred: bool,
    pub completed_turn: bool,
    pub provider_failure_hint: Option<ProviderFailureHint>,
    pub nonfatal_error_items: u64,
    pub last_nonfatal_error_hint: Option<ProviderFailureHint>,
}

struct JsonlCollector {
    requested_model: String,
    maximum_line: usize,
    maximum_response: usize,
    line: Vec<u8>,
    thread_started: bool,
    turn_started: bool,
    explicit_final: bool,
    validated: bool,
    rejection: Option<StreamFailure>,
    candidate: Option<String>,
    summary: StreamSummary,
}

impl JsonlCollector {
    fn new(model: &str, maximum_line: usize, maximum_response: usize) -> Self {
        Self {
            requested_model: model.into(),
            maximum_line,
            maximum_response,
            line: Vec::new(),
            thread_started: false,
            turn_started: false,
            explicit_final: false,
            validated: false,
            rejection: None,
            candidate: None,
            summary: StreamSummary::default(),
        }
    }

    fn observe_model(&mut self, model: Option<&str>) -> Result<(), StreamFailure> {
        if let Some(model) = model {
            if model != self.requested_model {
                return Err(StreamFailure::ReportedModelMismatch);
            }
            self.summary.reported_model = Some(self.requested_model.clone());
        }
        Ok(())
    }

    fn event(&mut self, bytes: &[u8]) -> Result<(), StreamFailure> {
        // This first projection uses serde's IgnoredAny for unselected fields.
        // It does not build a Value tree or copy reasoning/tool payload strings.
        let header: EventHeader =
            serde_json::from_slice(bytes).map_err(|_| StreamFailure::MalformedJsonl)?;
        *self
            .summary
            .event_type_counts
            .entry(header.kind)
            .or_default() += 1;
        self.observe_model(header.model.as_deref())?;
        if let Some(item) = &header.item {
            *self.summary.item_type_counts.entry(item.kind).or_default() += 1;
            self.observe_model(item.model.as_deref())?;
            match item.kind {
                ItemType::CommandExecution
                | ItemType::FileChange
                | ItemType::McpToolCall
                | ItemType::WebSearch => {
                    self.summary.native_action_observed = true;
                    return Err(StreamFailure::NativeActionObserved);
                }
                ItemType::Unknown | ItemType::NativeOrUnknown => {
                    self.summary.unsupported_item_observed = true;
                    return Err(StreamFailure::UnsupportedItem);
                }
                _ => {}
            }
            if item.kind == ItemType::Error {
                // codex-rs/exec/src/exec_events.rs (rust-v0.160.0): ErrorItem
                // is non-fatal, unlike ThreadEvent::Error or TurnFailed. The
                // installed CLI also emits capability notices before turn.started.
                if !self.thread_started
                    || self.summary.completed_turn
                    || !matches!(
                        header.kind,
                        EventType::ItemStarted | EventType::ItemUpdated | EventType::ItemCompleted
                    )
                {
                    return Err(StreamFailure::InvalidEventOrder);
                }
                self.summary.nonfatal_error_items += 1;
                self.summary.last_nonfatal_error_hint = Some(provider_failure_hint(bytes));
                return Ok(());
            }
        }
        if self.summary.completed_turn {
            return Err(StreamFailure::InvalidEventOrder);
        }
        match header.kind {
            EventType::ThreadStarted => {
                if self.thread_started || self.turn_started {
                    return Err(StreamFailure::InvalidEventOrder);
                }
                self.thread_started = true;
            }
            EventType::TurnStarted => {
                if !self.thread_started || self.turn_started {
                    return Err(StreamFailure::InvalidEventOrder);
                }
                self.turn_started = true;
            }
            EventType::ItemStarted | EventType::ItemUpdated | EventType::ItemCompleted => {
                if !self.turn_started {
                    return Err(StreamFailure::InvalidEventOrder);
                }
                let item = header.item.ok_or(StreamFailure::MalformedJsonl)?;
                if item.kind == ItemType::AssistantMessage
                    && header.kind == EventType::ItemCompleted
                {
                    match item.phase {
                        Some(MessagePhase::Commentary) => return Ok(()),
                        Some(MessagePhase::Unknown) => return Err(StreamFailure::UnexpectedEvent),
                        _ => {}
                    }
                    if self.explicit_final {
                        return Err(StreamFailure::InvalidEventOrder);
                    }
                    let text: TextEvent =
                        serde_json::from_slice(bytes).map_err(|_| StreamFailure::MalformedJsonl)?;
                    if text.item.text.len() > self.maximum_response {
                        return Err(StreamFailure::ResponseTooLarge);
                    }
                    self.explicit_final = matches!(item.phase, Some(MessagePhase::Final));
                    self.summary.final_phase_inferred = item.phase.is_none();
                    // For the CLI's phase-less event shape, only the last completed
                    // assistant message preceding turn.completed is a candidate.
                    self.candidate = Some(text.item.text);
                }
            }
            EventType::TurnCompleted => {
                if !self.turn_started || header.item.is_some() {
                    return Err(StreamFailure::InvalidEventOrder);
                }
                if let Some(usage) = header.usage {
                    if let (Some(cached), Some(input)) =
                        (usage.cached_input_tokens, usage.input_tokens)
                    {
                        if cached > input {
                            return Err(StreamFailure::MalformedJsonl);
                        }
                    }
                    self.summary.usage = Usage {
                        input_tokens: usage.input_tokens,
                        cached_input_tokens: usage.cached_input_tokens,
                        output_tokens: usage.output_tokens,
                        actual_cost_microusd: None,
                    };
                }
                self.summary.completed_turn = true;
            }
            EventType::TurnFailed | EventType::Error => {
                self.summary.provider_failure_hint = Some(provider_failure_hint(bytes));
                return Err(StreamFailure::ProviderReportedFailure);
            }
            EventType::Unknown => return Err(StreamFailure::UnexpectedEvent),
        }
        Ok(())
    }

    fn take_rejected_final_response(&mut self) -> Option<RejectedFinalResponse> {
        if self.rejection == Some(StreamFailure::InvalidStructuredResponse)
            && self.summary.completed_turn
            && self.line.is_empty()
            && !self.summary.native_action_observed
            && !self.summary.unsupported_item_observed
        {
            self.candidate
                .take()
                .filter(|text| text.len() <= self.maximum_response)
                .map(RejectedFinalResponse)
        } else {
            None
        }
    }

    fn take_response(mut self) -> Result<String, StreamFailure> {
        if let Some(error) = self.rejection {
            return Err(error);
        }
        if !self.validated {
            return Err(StreamFailure::MissingCompletion);
        }
        self.candidate
            .take()
            .ok_or(StreamFailure::MissingFinalResponse)
    }
}

impl StdoutSink for JsonlCollector {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), StreamFailure> {
        if let Some(error) = self.rejection {
            return Err(error);
        }
        if !bytes.is_empty() {
            self.validated = false;
        }
        let result = (|| {
            for &byte in bytes {
                if byte == b'\n' {
                    let mut line = std::mem::take(&mut self.line);
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    let result = self.event(&line);
                    line.fill(0);
                    line.clear();
                    self.line = line;
                    result?;
                } else {
                    if self.line.len() == self.maximum_line {
                        return Err(StreamFailure::LineTooLarge);
                    }
                    self.line.push(byte);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.rejection = Some(error);
            self.candidate = None;
        }
        result
    }

    fn finish(&mut self) -> Result<(), StreamFailure> {
        if let Some(error) = self.rejection {
            return Err(error);
        }
        let result = (|| {
            if !self.line.is_empty() {
                return Err(StreamFailure::TruncatedJsonl);
            }
            if !self.summary.completed_turn {
                return Err(StreamFailure::MissingCompletion);
            }
            let candidate = self
                .candidate
                .as_ref()
                .ok_or(StreamFailure::MissingFinalResponse)?;
            let mut deserializer = serde_json::Deserializer::from_str(candidate);
            UniqueJson::deserialize(&mut deserializer)
                .map_err(|_| StreamFailure::InvalidStructuredResponse)?;
            deserializer
                .end()
                .map_err(|_| StreamFailure::InvalidStructuredResponse)?;
            Ok(())
        })();
        self.validated = result.is_ok();
        if let Err(error) = result {
            self.rejection = Some(error);
            // Only an observed completed turn with a bounded final message can
            // preserve a private rejected carrier. All stream/action failures
            // still discard the candidate and remain non-replayable failures.
            if error != StreamFailure::InvalidStructuredResponse {
                self.candidate = None;
            }
        }
        result
    }
}

impl Drop for JsonlCollector {
    fn drop(&mut self) {
        self.line.fill(0);
    }
}

/// Validates JSON syntax and decoded object-key uniqueness without retaining a
/// response Value tree. serde_json's recursion limit remains enabled.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unique-key JSON")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<UniqueJson, A::Error> {
                while sequence.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<UniqueJson, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = map.next_key::<String>()? {
                    if !names.insert(name) {
                        return Err(de::Error::custom("duplicate object member"));
                    }
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// A finite recognizer: even an unexpected version response is never retained.
#[derive(Default)]
struct VersionSink {
    position: usize,
    mismatch: bool,
}
impl VersionSink {
    fn matches(&self) -> bool {
        !self.mismatch
            && (self.position == b"codex-cli 0.160.0".len()
                || self.position == b"codex-cli 0.160.0\n".len())
    }
}
impl StdoutSink for VersionSink {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), StreamFailure> {
        let expected = b"codex-cli 0.160.0\n";
        for &byte in bytes {
            if expected.get(self.position).copied() != Some(byte) {
                self.mismatch = true;
            }
            self.position = self.position.saturating_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn events(response: &str, usage: Option<Value>, model: Option<&str>) -> Vec<u8> {
        let mut thread = json!({"type":"thread.started","thread_id":"t"});
        if let Some(model) = model {
            thread["model"] = model.into();
        }
        let mut completed = json!({"type":"turn.completed"});
        if let Some(usage) = usage {
            completed["usage"] = usage;
        }
        let records = [
            thread,
            json!({"type":"turn.started"}),
            json!({"type":"item.completed","item":{"type":"agent_message","text":response}}),
            completed,
        ];
        let mut bytes = Vec::new();
        for record in records {
            serde_json::to_writer(&mut bytes, &record).unwrap();
            bytes.push(b'\n');
        }
        bytes
    }

    fn collect(bytes: &[u8]) -> Result<JsonlCollector, StreamFailure> {
        let mut collector = JsonlCollector::new("model-a", 16_384, 4096);
        for chunk in bytes.chunks(3) {
            collector.bytes(chunk)?;
        }
        collector.finish()?;
        Ok(collector)
    }

    #[test]
    fn bounded_rejected_final_diagnostics_include_parser_limits_without_becoming_output() {
        for response in [
            "{invalid".to_owned(),
            "{\"key\":1,\"key\":2}".to_owned(),
            "1e400".to_owned(),
            format!("{}0{}", "[".repeat(129), "]".repeat(129)),
        ] {
            let mut collector = JsonlCollector::new("model-a", 16_384, 4096);
            collector.bytes(&events(&response, None, None)).unwrap();
            assert_eq!(
                collector.finish(),
                Err(StreamFailure::InvalidStructuredResponse)
            );
            assert!(!collector.validated);
            let diagnostic = collector.take_rejected_final_response().unwrap();
            assert_eq!(diagnostic.as_str(), response);
            assert!(!format!("{diagnostic:?}").contains(&response));
            assert_eq!(
                collector.take_response().unwrap_err(),
                StreamFailure::InvalidStructuredResponse
            );
        }
    }

    #[test]
    fn completed_response_preserves_exact_decoded_text_and_usage() {
        let response = "{ \"value\": \"α\\nβ\", \"count\": 7 }";
        let collector = collect(&events(
            response,
            Some(json!({
                "input_tokens":11,"cached_input_tokens":3,"output_tokens":5
            })),
            Some("model-a"),
        ))
        .unwrap();
        assert_eq!(collector.summary.usage.input_tokens, Some(11));
        assert_eq!(collector.summary.usage.cached_input_tokens, Some(3));
        assert_eq!(collector.summary.usage.output_tokens, Some(5));
        assert_eq!(collector.summary.usage.actual_cost_microusd, None);
        assert_eq!(collector.summary.reported_model.as_deref(), Some("model-a"));
        assert_eq!(collector.take_response().unwrap(), response);
    }

    #[test]
    fn nonfatal_error_item_before_turn_does_not_abort_a_completed_response() {
        let mut records = events("{}", None, None);
        let first_end = records.iter().position(|b| *b == b'\n').unwrap() + 1;
        let notice = b"{\"type\":\"item.completed\",\"item\":{\"id\":\"notice\",\"type\":\"error\",\"message\":\"Optional Code Mode unavailable; PRIVATE-DIAGNOSTIC\"}}\n";
        records.splice(first_end..first_end, notice.iter().copied());
        let collector = collect(&records).unwrap();
        assert_eq!(collector.summary.nonfatal_error_items, 1);
        assert_eq!(collector.summary.provider_failure_hint, None);
        assert!(collector.summary.completed_turn);
        assert!(!serde_json::to_string(&collector.summary)
            .unwrap()
            .contains("PRIVATE-DIAGNOSTIC"));
        assert_eq!(collector.take_response().unwrap(), "{}");
        let mut parser = JsonlCollector::new("model-a", 16384, 4096);
        assert_eq!(parser.bytes(notice), Err(StreamFailure::InvalidEventOrder));
    }

    #[test]
    fn provider_failure_hint_is_retained_without_original_error_text() {
        let mut parser = JsonlCollector::new("model-a", 16384, 4096);
        let event = serde_json::json!({"type":"error","message":"Invalid schema for response_format; private-field-that-must-not-be-retained"});
        let mut bytes = serde_json::to_vec(&event).unwrap();
        bytes.push(b'\n');
        assert_eq!(
            parser.bytes(&bytes),
            Err(StreamFailure::ProviderReportedFailure)
        );
        assert_eq!(
            parser.summary.provider_failure_hint,
            Some(ProviderFailureHint::Schema)
        );
        assert!(!serde_json::to_string(&parser.summary)
            .unwrap()
            .contains("private-field"));
    }

    #[test]
    fn absent_usage_and_actual_model_remain_unknown() {
        let collector = collect(&events("{}", None, None)).unwrap();
        assert_eq!(collector.summary.usage, Usage::default());
        assert_eq!(collector.summary.reported_model, None);
        assert!(collector.summary.final_phase_inferred);
        assert_eq!(collector.take_response().unwrap(), "{}");
    }

    #[test]
    fn malformed_truncated_missing_completion_and_duplicate_members_fail() {
        let mut parser = JsonlCollector::new("model-a", 1024, 1024);
        assert_eq!(
            parser.bytes(b"not-json\n"),
            Err(StreamFailure::MalformedJsonl)
        );
        let mut truncated = events("{}", None, None);
        truncated.pop();
        assert!(matches!(
            collect(&truncated),
            Err(StreamFailure::TruncatedJsonl)
        ));
        assert!(matches!(
            collect(b"{\"type\":\"thread.started\"}\n"),
            Err(StreamFailure::MissingCompletion)
        ));
        assert!(matches!(
            collect(&events("{\"x\":1,\"\\u0078\":2}", None, None)),
            Err(StreamFailure::InvalidStructuredResponse)
        ));
        assert!(matches!(
            collect(&events("{} trailing", None, None)),
            Err(StreamFailure::InvalidStructuredResponse)
        ));
        // Exercise string decoding, not just object-key decoding. Valid scalar
        // JSON remains a caller schema concern; these strings are invalid JSON.
        for response in [r#""\ud800""#, r#""\udc00""#, r#""\ud800x""#, r#""\q""#] {
            assert!(matches!(
                collect(&events(response, None, None)),
                Err(StreamFailure::InvalidStructuredResponse)
            ));
        }
        assert!(matches!(
            collect(&events("{}", Some(json!({"input_tokens":-1})), None)),
            Err(StreamFailure::MalformedJsonl)
        ));
    }

    #[test]
    fn wrong_model_and_native_action_cannot_be_salvaged() {
        assert!(matches!(
            collect(&events("{}", None, Some("model-b"))),
            Err(StreamFailure::ReportedModelMismatch)
        ));
        let mut parser = JsonlCollector::new("model-a", 4096, 1024);
        parser.bytes(&events("{}", None, None)).unwrap();
        parser.finish().unwrap();
        assert!(parser.validated);
        let result = parser.bytes(b"{\"type\":\"item.started\",\"item\":{\"type\":\"command_execution\",\"command\":\"private-action-payload\"}}\n");
        assert_eq!(result, Err(StreamFailure::NativeActionObserved));
        assert!(parser.summary.native_action_observed);
        assert!(!parser.summary.unsupported_item_observed);
        assert!(!format!("{:?}", parser.summary).contains("private-action-payload"));
        assert_eq!(parser.finish(), Err(StreamFailure::NativeActionObserved));
        assert_eq!(
            parser.take_response(),
            Err(StreamFailure::NativeActionObserved)
        );
    }

    #[test]
    fn reasoning_and_commentary_are_not_retained_as_response_or_metadata() {
        let mut parser = JsonlCollector::new("model-a", 4096, 1024);
        parser
            .bytes(b"{\"type\":\"thread.started\"}\n{\"type\":\"turn.started\"}\n")
            .unwrap();
        parser.bytes(b"{\"type\":\"item.completed\",\"item\":{\"text\":\"private-reasoning-sentinel\",\"type\":\"reasoning\"}}\n").unwrap();
        assert!(parser.candidate.is_none());
        assert!(parser.line.is_empty());
        parser.bytes(b"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"phase\":\"commentary\",\"text\":\"private-commentary-sentinel\"}}\n").unwrap();
        assert!(parser.candidate.is_none());
        parser.bytes(b"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"phase\":\"final_answer\",\"text\":\"{}\"}}\n{\"type\":\"turn.completed\"}\n").unwrap();
        parser.finish().unwrap();
        let metadata = format!("{:?}", parser.summary);
        assert!(!metadata.contains("private-reasoning-sentinel"));
        assert!(!metadata.contains("private-commentary-sentinel"));
        assert!(!parser.summary.final_phase_inferred);
        assert_eq!(parser.take_response().unwrap(), "{}");
    }

    #[test]
    fn response_and_line_limits_are_independent() {
        let mut parser = JsonlCollector::new("model-a", 4096, 2);
        assert_eq!(
            parser.bytes(&events("{\"x\":1}", None, None)),
            Err(StreamFailure::ResponseTooLarge)
        );
        let mut parser = JsonlCollector::new("model-a", 8, 8);
        assert_eq!(parser.bytes(b"123456789"), Err(StreamFailure::LineTooLarge));
    }

    #[test]
    fn version_recognizer_discards_unexpected_text() {
        let mut accepted = VersionSink::default();
        accepted.bytes(b"codex-cli 0.160.0\n").unwrap();
        assert!(accepted.matches());
        let mut rejected = VersionSink::default();
        rejected.bytes(b"unexpected sensitive diagnostic").unwrap();
        assert!(!rejected.matches());
    }

    #[cfg(unix)]
    fn setup() -> (
        tempfile::TempDir,
        ProviderConfig,
        ModelCatalog,
        OperatorPolicy,
        AdmissionLimits,
        ProviderRequest,
    ) {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("pinned-cli");
        fs::write(&executable, b"synthetic executable identity").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let working = root.path().join("work");
        let state = root.path().join("state");
        fs::create_dir(&working).unwrap();
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        let config = ProviderConfig {
            executable: executable.clone(),
            model: "model-a".into(),
            effort: "high".into(),
            timeout_ms: 1000,
            max_stdout_bytes: 16_384,
            max_stderr_bytes: 4096,
            max_response_bytes: 4096,
            require_hard_context_bound: false,
            require_actionless: false,
        };
        let catalog = ModelCatalog {
            entries: BTreeMap::from([(
                "model-a".into(),
                ModelSupport {
                    availability: ModelAvailability::VerifiedAvailable,
                    efforts: BTreeSet::from(["high".into()]),
                },
            )]),
            evidence_sha256: digest(b"catalog evidence"),
        };
        let policy = OperatorPolicy {
            executable: ExecutablePin {
                operator_path: executable,
                sha256: digest(b"synthetic executable identity"),
                cli_version: SUPPORTED_CLI_VERSION.into(),
            },
            temporary_directory: state,
            supported_disable_features: BTreeSet::new(),
            disabled_features: BTreeSet::new(),
        };
        let limits = AdmissionLimits {
            max_prompt_bytes: 4096,
            max_schema_bytes: 4096,
            max_schema_nodes: 1000,
            max_schema_depth: 32,
            max_control_bytes: 65_536,
            max_executable_bytes: 1024,
            max_timeout_ms: 10_000,
            max_stream_bytes: 65_536,
            cleanup_timeout_ms: 2000,
        };
        let request = ProviderRequest {
            prompt: "α\n\\literal\n".into(),
            output_schema: json!({"type":"object","properties":{"value":{"type":"string"}},"additionalProperties":false}),
            working_directory: working,
            invocation_id: "invocation-1".into(),
        };
        (root, config, catalog, policy, limits, request)
    }

    #[cfg(unix)]
    #[test]
    fn exact_prompt_schema_and_arguments_are_bound_without_claiming_cli_support() {
        let (_root, config, catalog, policy, limits, request) = setup();
        let prompt = request.prompt.as_bytes().to_vec();
        let schema = serde_json::to_vec(&request.output_schema).unwrap();
        let provider = CodexProvider::new(config, catalog, policy, limits).unwrap();
        let prepared = provider.prepare(request).unwrap();
        assert_eq!(prepared.spec.input, prompt);
        assert_eq!(prepared.audit.prompt_sha256, digest(&prompt));
        assert_eq!(prepared.audit.schema_sha256, digest(&schema));
        assert_eq!(fs::read(prepared.schema.path()).unwrap(), schema);
        let arguments: Vec<_> = prepared
            .spec
            .arguments
            .iter()
            .map(|s| s.to_str().unwrap())
            .collect();
        assert_eq!(
            arguments,
            vec![
                "-a",
                "never",
                "exec",
                "--ignore-user-config",
                "--ignore-rules",
                "--ephemeral",
                "-s",
                "read-only",
                "-m",
                "model-a",
                "-c",
                "model_reasoning_effort=\"high\"",
                "--json",
                "--color",
                "never",
                "--output-schema",
                prepared.schema.path().to_str().unwrap(),
                "-",
            ]
        );
        assert_eq!(
            prepared.audit.argv_sha256,
            serialized_digest(&arguments).unwrap()
        );
        assert_eq!(
            prepared.audit.configuration_sha256,
            serialized_digest(&(
                &provider.binding_sha256,
                &prepared.spec.working_directory,
                &arguments,
                &prepared.audit.prompt_sha256,
                &prepared.audit.schema_sha256,
                &prepared.audit.invocation_id,
            ))
            .unwrap()
        );
        assert_eq!(
            prepared.audit.environment_sha256,
            serialized_digest(&provider.environment).unwrap()
        );
        let path = prepared.schema.path().to_owned();
        prepared.cancel().unwrap();
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn catalog_never_falls_back_or_treats_advertising_as_availability() {
        let (_root, mut config, catalog, policy, limits, _request) = setup();
        config.model = "model-b".into();
        assert!(matches!(
            CodexProvider::new(
                config.clone(),
                catalog.clone(),
                policy.clone(),
                limits.clone()
            ),
            Err(ProviderError::ModelUnavailable)
        ));
        config.model = "model-a".into();
        config.effort = "unsupported".into();
        assert!(matches!(
            CodexProvider::new(
                config.clone(),
                catalog.clone(),
                policy.clone(),
                limits.clone()
            ),
            Err(ProviderError::EffortUnsupported)
        ));
        config.effort = "high".into();
        let mut advertised = catalog;
        advertised.entries.get_mut("model-a").unwrap().availability =
            ModelAvailability::AdvertisedOnly;
        assert!(matches!(
            CodexProvider::new(config, advertised, policy, limits),
            Err(ProviderError::ModelUnavailable)
        ));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unsupported_guarantees_refuse_before_any_executable_access_or_launch() {
        let (root, mut config, catalog, mut policy, limits, request) = setup();
        let absent = root.path().join("does-not-exist");
        config.executable = absent.clone();
        policy.executable.operator_path = absent;
        config.require_hard_context_bound = true;
        assert!(matches!(
            CodexProvider::new(
                config.clone(),
                catalog.clone(),
                policy.clone(),
                limits.clone()
            ),
            Err(ProviderError::UnsupportedHardContextBound)
        ));
        config.require_hard_context_bound = false;
        config.require_actionless = true;
        let provider = CodexProvider::new(config, catalog, policy, limits).unwrap();
        assert!(matches!(
            provider.invoke(request).await,
            Err(ProviderError::ActionlessAttestationRequired)
        ));
        assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn synthetic_verifier_tests_plumbing_but_never_enables_production() {
        struct SyntheticVerifier;
        impl AttestationVerifier for SyntheticVerifier {
            fn verify(&self, value: &ActionlessAttestation) -> Result<(), AttestationRejection> {
                if value.authenticator == b"synthetic-proof" {
                    Ok(())
                } else {
                    Err(AttestationRejection)
                }
            }
        }
        let (_root, mut config, catalog, policy, limits, request) = setup();
        config.require_actionless = true;
        let provider = CodexProvider::new(config, catalog, policy, limits).unwrap();
        let prepared = provider.prepare(request).unwrap();
        let mut attestation = ActionlessAttestation {
            executable_sha256: prepared.audit.executable_sha256.clone(),
            configuration_sha256: prepared.audit.configuration_sha256.clone(),
            evidence_sha256: digest(b"synthetic external evidence"),
            issuer: "test-verifier".into(),
            authenticator: b"synthetic-proof".to_vec(),
        };
        assert!(matches!(
            provider.check_attestation(&prepared.audit, Some(&attestation), None),
            Err(ProviderError::AttestationRejected)
        ));
        assert!(matches!(
            provider.check_attestation(
                &prepared.audit,
                Some(&attestation),
                Some(&SyntheticVerifier)
            ),
            Ok(ActionlessAssurance::ExternalAttestation { .. })
        ));
        attestation.configuration_sha256 = digest(b"different configuration");
        assert!(matches!(
            provider.check_attestation(
                &prepared.audit,
                Some(&attestation),
                Some(&SyntheticVerifier)
            ),
            Err(ProviderError::AttestationRejected)
        ));
        attestation.configuration_sha256 = prepared.audit.configuration_sha256.clone();
        let path = prepared.schema.path().to_owned();
        assert!(matches!(
            provider
                .dispatch(prepared, Some(&attestation), Some(&SyntheticVerifier))
                .await,
            Err(ProviderError::UnsupportedProductionProfile)
        ));
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn oversized_input_schema_and_repository_temp_storage_are_rejected() {
        let (_root, config, catalog, policy, mut limits, mut request) = setup();
        limits.max_prompt_bytes = 2;
        let provider = CodexProvider::new(
            config.clone(),
            catalog.clone(),
            policy.clone(),
            limits.clone(),
        )
        .unwrap();
        assert!(matches!(
            provider.prepare(request),
            Err(ProviderError::AdmissionLimit("prompt bytes"))
        ));
        let (_, _, _, _, _, fresh) = setup();
        request = ProviderRequest {
            working_directory: policy.temporary_directory.parent().unwrap().join("work"),
            ..fresh
        };
        limits.max_prompt_bytes = 4096;
        limits.max_schema_bytes = 2;
        let provider = CodexProvider::new(
            config.clone(),
            catalog.clone(),
            policy.clone(),
            limits.clone(),
        )
        .unwrap();
        assert!(matches!(
            provider.prepare(request),
            Err(ProviderError::AdmissionLimit("schema bytes"))
        ));
        fs::create_dir(policy.temporary_directory.parent().unwrap().join(".git")).unwrap();
        assert!(matches!(
            private_temporary_root(
                &policy.temporary_directory,
                &policy.temporary_directory.parent().unwrap().join("work")
            ),
            Err(ProviderError::InvalidTemporaryDirectory)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn post_creation_admission_error_explicitly_removes_schema() {
        let (_root, config, catalog, policy, mut limits, request) = setup();
        let state = policy.temporary_directory.clone();
        limits.max_control_bytes = 1;
        let provider = CodexProvider::new(config, catalog, policy, limits).unwrap();
        assert!(matches!(
            provider.prepare(request),
            Err(ProviderError::AdmissionLimit(
                "command and environment bytes"
            ))
        ));
        assert_eq!(fs::read_dir(state).unwrap().count(), 0);
    }

    #[test]
    fn explicit_prelaunch_cleanup_preserves_original_error_on_removal_failure() {
        let root = tempfile::tempdir().unwrap();
        let schema = NamedTempFile::new_in(root.path()).unwrap();
        let path = schema.path().to_owned();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let error = prelaunch_cleanup(schema, ProviderError::AdmissionLimit("test rejection"));
        match error {
            ProviderError::PrelaunchCleanupFailed { original } => {
                assert!(matches!(
                    *original,
                    ProviderError::AdmissionLimit("test rejection")
                ));
            }
            _ => panic!("removal uncertainty was hidden"),
        }
        fs::remove_dir(path).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn research_permission_does_not_enable_unattested_production() {
        let (_root, config, catalog, policy, limits, request) = setup();
        assert!(!config.require_actionless);
        let provider = CodexProvider::new(config, catalog, policy, limits).unwrap();
        let prepared = provider.prepare(request).unwrap();
        let path = prepared.schema.path().to_owned();
        assert!(matches!(
            provider.dispatch(prepared, None, None).await,
            Err(ProviderError::ActionlessAttestationRequired)
        ));
        assert!(!path.exists());
        let invalid = ProviderRequest {
            prompt: String::new(),
            output_schema: Value::Null,
            working_directory: PathBuf::new(),
            invocation_id: String::new(),
        };
        assert!(matches!(
            provider.invoke(invalid).await,
            Err(ProviderError::ActionlessAttestationRequired)
        ));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn explicit_research_cannot_relax_a_strict_configuration() {
        let (_root, mut config, catalog, policy, limits, request) = setup();
        config.require_actionless = true;
        let provider = CodexProvider::new(config, catalog, policy, limits).unwrap();
        let prepared = provider.prepare(request).unwrap();
        let path = prepared.schema.path().to_owned();
        assert!(matches!(
            provider.dispatch_research(prepared).await,
            Err(ProviderError::ActionlessAttestationRequired)
        ));
        assert!(!path.exists());
        let invalid = ProviderRequest {
            prompt: String::new(),
            output_schema: Value::Null,
            working_directory: PathBuf::new(),
            invocation_id: String::new(),
        };
        assert!(matches!(
            provider.invoke_research(invalid).await,
            Err(ProviderError::ActionlessAttestationRequired)
        ));
    }

    #[cfg(target_os = "linux")]
    const FIXTURE_MARKER: &[u8] = b"\nPERSONA_PROVIDER_FIXTURE_ENTERED_v1\n";
    #[cfg(target_os = "linux")]
    const UNREAD_INPUT_BYTES: usize = 4 * 1024 * 1024;

    // Only fixture tests strip the libtest preamble. Production parsing does not.
    #[cfg(target_os = "linux")]
    struct FixtureSink<'a> {
        inner: &'a mut dyn StdoutSink,
        matched: usize,
        started: bool,
    }

    #[cfg(target_os = "linux")]
    impl<'a> FixtureSink<'a> {
        fn new(inner: &'a mut dyn StdoutSink) -> Self {
            Self {
                inner,
                matched: 0,
                started: false,
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl StdoutSink for FixtureSink<'_> {
        fn bytes(&mut self, bytes: &[u8]) -> Result<(), StreamFailure> {
            if self.started {
                return self.inner.bytes(bytes);
            }
            for (index, &byte) in bytes.iter().enumerate() {
                if byte == FIXTURE_MARKER[self.matched] {
                    self.matched += 1;
                } else {
                    self.matched = usize::from(byte == FIXTURE_MARKER[0]);
                }
                if self.matched == FIXTURE_MARKER.len() {
                    self.started = true;
                    return self.inner.bytes(&bytes[index + 1..]);
                }
            }
            Ok(())
        }
        fn finish(&mut self) -> Result<(), StreamFailure> {
            if !self.started {
                return Err(StreamFailure::MissingCompletion);
            }
            self.inner.finish()
        }
    }

    #[cfg(target_os = "linux")]
    fn fixture_spec(mode: &str) -> ProcessSpec {
        ProcessSpec {
            executable: std::env::current_exe().unwrap(),
            arguments: vec![
                "--ignored".into(),
                "--exact".into(),
                format!(
                    "{}::process_fixture",
                    module_path!()
                        .split_once("::")
                        .map(|(_, rest)| rest)
                        .unwrap_or(module_path!())
                )
                .into(),
                "--nocapture".into(),
            ],
            input: Vec::new(),
            working_directory: std::env::temp_dir(),
            environment: BTreeMap::from([("PERSONA_PROVIDER_FIXTURE".into(), mode.into())]),
            limits: ProcessLimits {
                timeout_ms: 3000,
                cleanup_timeout_ms: 2000,
                max_input_bytes: 1024,
                max_stdout_bytes: 65_536,
                max_stderr_bytes: 65_536,
            },
        }
    }

    #[cfg(target_os = "linux")]
    fn fixture_command(mode: &str) -> std::process::Command {
        let spec = fixture_spec(mode);
        let mut command = std::process::Command::new(spec.executable);
        command
            .args(spec.arguments)
            .current_dir(spec.working_directory)
            .env_clear()
            .envs(spec.environment)
            .stdin(Stdio::null());
        command
    }

    #[cfg(target_os = "linux")]
    async fn acknowledged_pid(path: &Path) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match fs::read_to_string(path) {
                Ok(text) => {
                    if let Ok(pid) = text.parse() {
                        return pid;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => panic!("fixture acknowledgment failed: {:?}", error.kind()),
            }
            assert!(
                Instant::now() < deadline,
                "fixture body did not acknowledge startup"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[cfg(target_os = "linux")]
    fn process_observation_dead(pid: u32, observation: io::Result<String>) -> bool {
        match observation {
            // Linux may open procfs while the task exists, then return ESRCH
            // from the read after it exits. This is disappearance, like ENOENT;
            // other observation failures must still fail the cleanup test.
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.raw_os_error() == Some(libc::ESRCH) =>
            {
                true
            }
            Ok(stat) => stat
                .rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')),
            Err(error) => panic!(
                "process-state observation failed: pid={pid} kind={:?} raw_os_error={:?} display={error}",
                error.kind(),
                error.raw_os_error()
            ),
        }
    }

    #[cfg(target_os = "linux")]
    async fn assert_process_dead(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // Observation only: this helper never signals a numeric PID. An
            // orphan zombie cannot execute or keep inherited pipes open.
            if process_observation_dead(pid, fs::read_to_string(format!("/proc/{pid}/stat"))) {
                return;
            }
            assert!(Instant::now() < deadline, "fixture process remained live");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // Hold a procfs descriptor across exit of our own child to exercise ESRCH.
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn process_stat_read_after_owned_child_exit_reports_disappearance() {
        use std::io::Read;
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("owned.pid");
        let mut command = fixture_command("wait");
        command
            .env("PERSONA_PROVIDER_PID_FILE", &pid_file)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        let pid = acknowledged_pid(&pid_file).await;
        assert_eq!(pid, child.id());
        let mut stat = fs::File::open(format!("/proc/{pid}/stat")).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let error = stat.read_to_string(&mut String::new()).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
        assert!(process_observation_dead(pid, Err(error)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_observation_rejects_other_errors_and_non_zombie_states() {
        for errno in [libc::ENOENT, libc::ESRCH] {
            assert!(process_observation_dead(
                0,
                Err(io::Error::from_raw_os_error(errno))
            ));
        }
        for errno in [
            libc::EACCES,
            libc::EPERM,
            libc::EIO,
            libc::EBADF,
            libc::EINTR,
            libc::EAGAIN,
        ] {
            assert!(
                std::panic::catch_unwind(|| {
                    process_observation_dead(0, Err(io::Error::from_raw_os_error(errno)))
                })
                .is_err(),
                "unexpected observation error {errno} was ignored"
            );
        }
        assert!(std::panic::catch_unwind(|| {
            process_observation_dead(0, Err(io::Error::other("unclassified synthetic failure")))
        })
        .is_err());
        for state in ["R", "S", "D", "T", "t", "I"] {
            assert!(!process_observation_dead(
                0,
                Ok(format!("0 (fixture) {state} 0"))
            ));
        }
        assert!(!process_observation_dead(0, Ok("malformed".into())));
        assert!(process_observation_dead(0, Ok("0 (fixture) Z 0".into())));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn process_liveness_assertion_still_rejects_a_live_process() {
        let failure = tokio::spawn(async { assert_process_dead(std::process::id()).await })
            .await
            .expect_err("a live process must not satisfy the death assertion");
        assert!(failure.is_panic());
        let payload = failure.into_panic();
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("fixture process remained live"));
    }

    /// The test executable itself is the only fixture program. Running this
    /// ignored test manually without the dedicated environment variable is inert.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn process_fixture() {
        use std::os::unix::process::CommandExt;
        let Ok(mode) = std::env::var("PERSONA_PROVIDER_FIXTURE") else {
            return;
        };
        io::stdout().write_all(FIXTURE_MARKER).unwrap();
        io::stdout().flush().unwrap();
        if let Some(path) = std::env::var_os("PERSONA_PROVIDER_LEADER_PID_FILE") {
            fs::write(path, std::process::id().to_string()).unwrap();
        }
        if matches!(
            mode.as_str(),
            "success" | "wait" | "descendant-child" | "escaped-writer"
        ) {
            if let Some(path) = std::env::var_os("PERSONA_PROVIDER_PID_FILE") {
                fs::write(path, std::process::id().to_string()).unwrap();
            }
        }
        match mode.as_str() {
            "stdout" => loop {
                if io::stdout().write_all(&[b'x'; 8192]).is_err() {
                    std::process::exit(0);
                }
            },
            "stderr" => loop {
                if io::stderr().write_all(&[b'e'; 8192]).is_err() {
                    std::process::exit(0);
                }
            },
            "wait" | "descendant-child" => std::thread::sleep(Duration::from_secs(30)),
            "blocked-stdin" => {
                // Establish that the unread payload really exceeds this pipe's
                // capacity; no read or pipe-size mutation is performed.
                let capacity = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETPIPE_SZ) };
                assert!(capacity > 0 && (capacity as usize) < UNREAD_INPUT_BYTES);
                std::thread::sleep(Duration::from_secs(30));
            }
            "pressure" => {
                let stdout = std::thread::spawn(|| {
                    io::stdout().write_all(&vec![b'x'; 256 * 1024]).unwrap();
                });
                let stderr = std::thread::spawn(|| {
                    io::stderr().write_all(&vec![b'e'; 256 * 1024]).unwrap();
                });
                stdout.join().unwrap();
                stderr.join().unwrap();
            }
            "descendant-parent" | "leader-exit" | "escaped-parent" => {
                let path = std::env::var_os("PERSONA_PROVIDER_PID_FILE").unwrap();
                let escaped = mode == "escaped-parent";
                let mut command = fixture_command(if escaped {
                    "escaped-writer"
                } else {
                    "descendant-child"
                });
                command.env("PERSONA_PROVIDER_PID_FILE", &path);
                if escaped {
                    command.process_group(0);
                }
                let mut child = command.spawn().unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !Path::new(&path).exists() {
                    assert!(Instant::now() < deadline, "descendant did not start");
                    std::thread::sleep(Duration::from_millis(5));
                }
                if mode == "leader-exit" {
                    std::process::exit(0);
                }
                let _ = child.wait();
            }
            "escaped-writer" => {
                // A fixture-only watchdog prevents a defective runner from
                // leaving an unbounded escaped test process behind.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_secs(10));
                    std::process::exit(3);
                });
                let stdout =
                    std::thread::spawn(|| while io::stdout().write_all(&[b'x'; 8192]).is_ok() {});
                let stderr =
                    std::thread::spawn(|| while io::stderr().write_all(&[b'e'; 8192]).is_ok() {});
                stdout.join().unwrap();
                stderr.join().unwrap();
            }
            "protocol-success"
            | "native-after-complete"
            | "model-after-complete"
            | "unknown-after-complete"
            | "invalid-scalar-string" => {
                let response = if mode == "invalid-scalar-string" {
                    r#""\ud800""#
                } else {
                    "{}"
                };
                io::stdout()
                    .write_all(&events(response, None, None))
                    .unwrap();
                let trailing = match mode.as_str() {
                    "native-after-complete" => Some(json!({"type":"item.started","item":{
                        "type":"command_execution","command":"private-action-payload"}})),
                    "model-after-complete" => Some(json!({"type":"error","model":"model-b"})),
                    "unknown-after-complete" => Some(json!({"type":"item.completed","item":{
                        "type":"future_information","text":"private-unknown-payload"}})),
                    _ => None,
                };
                if let Some(record) = trailing {
                    serde_json::to_writer(io::stdout(), &record).unwrap();
                    io::stdout().write_all(b"\n").unwrap();
                }
                io::stdout().flush().unwrap();
            }
            "success" => {}
            _ => std::process::exit(2),
        }
        std::process::exit(0);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn runner_accepts_a_real_successful_rust_process() {
        let mut discard = DiscardStdout;
        let mut sink = FixtureSink::new(&mut discard);
        let report = bounded_process(fixture_spec("success"), &mut sink)
            .await
            .unwrap();
        assert!(sink.started, "zero selected tests is not fixture success");
        assert!(report.launched);
        assert!(report.exit_status.as_ref().unwrap().success());
        assert!(report.stdout_eof && report.stderr.eof);
        assert_eq!(report.stderr.retained_bytes, 0);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn stdout_overflow_kills_and_reaps_without_returning_payload() {
        let mut spec = fixture_spec("stdout");
        spec.limits.max_stdout_bytes = 1024;
        let error = bounded_process(spec, &mut DiscardStdout).await.unwrap_err();
        assert_eq!(error.kind, ProcessFailureKind::StdoutOverflow);
        assert!(error.report.stdout_bytes_observed > 1024);
        assert!(
            error.report.stdout_bytes_observed
                <= (1025 + MAX_CLEANUP_DISCARD_BYTES_PER_PIPE) as u64
        );
        assert!(error.report.exit_status.is_some());
        assert!(error.report.stdout_eof || error.report.stop.stdout_cleanup_budget_exhausted);
        assert!(error.report.stderr.eof);
        assert_eq!(
            error.report.stop.process_group_sigkill,
            Some(GroupSignal::Sent)
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn stderr_flood_is_drained_counted_and_killed() {
        let mut spec = fixture_spec("stderr");
        spec.limits.max_stderr_bytes = 1024;
        let error = bounded_process(spec, &mut DiscardStdout).await.unwrap_err();
        assert_eq!(error.kind, ProcessFailureKind::StderrOverflow);
        assert!(error.report.stderr.exceeded_limit);
        assert!(error.report.stderr.observed_bytes > 1024);
        assert!(
            error.report.stderr.observed_bytes
                <= (1025 + MAX_CLEANUP_DISCARD_BYTES_PER_PIPE) as u64
        );
        assert_eq!(error.report.stderr.retained_bytes, 0);
        assert!(error.report.exit_status.is_some());
        assert!(error.report.stderr.eof || error.report.stop.stderr_cleanup_budget_exhausted);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn timeout_has_local_stop_receipt_without_remote_stop_claim() {
        let mut discard = DiscardStdout;
        let mut sink = FixtureSink::new(&mut discard);
        let error = bounded_process(fixture_spec("wait"), &mut sink)
            .await
            .unwrap_err();
        assert!(
            sink.started,
            "timeout before fixture entry is not this oracle"
        );
        assert_eq!(error.kind, ProcessFailureKind::Timeout);
        assert!(error.report.exit_status.is_some());
        assert_eq!(
            error.report.stop.process_group_sigkill,
            Some(GroupSignal::Sent)
        );
        assert!(!error.report.stop.remote_stop_confirmed);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn blocked_stdin_does_not_block_timeout_or_reaping() {
        let mut spec = fixture_spec("blocked-stdin");
        spec.input = vec![b'p'; UNREAD_INPUT_BYTES];
        spec.limits.max_input_bytes = spec.input.len();
        let mut discard = DiscardStdout;
        let mut sink = FixtureSink::new(&mut discard);
        let error = bounded_process(spec, &mut sink).await.unwrap_err();
        assert!(sink.started);
        assert_eq!(error.kind, ProcessFailureKind::Timeout);
        assert!(error.report.stdin_bytes_written > 0);
        assert!(error.report.stdin_bytes_written < UNREAD_INPUT_BYTES as u64);
        assert!(error.report.exit_status.is_some());
        assert!(error.report.stdout_eof && error.report.stderr.eof);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn simultaneous_pipe_pressure_completes_with_both_streams_counted() {
        let mut spec = fixture_spec("pressure");
        spec.limits.max_stdout_bytes = 512 * 1024;
        spec.limits.max_stderr_bytes = 512 * 1024;
        let mut discard = DiscardStdout;
        let mut sink = FixtureSink::new(&mut discard);
        let report = bounded_process(spec, &mut sink).await.unwrap();
        assert!(sink.started);
        assert!(report.exit_status.as_ref().unwrap().success());
        assert!(report.stdout_bytes_observed >= 256 * 1024);
        assert_eq!(report.stderr.observed_bytes, 256 * 1024);
        assert!(report.stdout_eof && report.stderr.eof);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn complete_response_then_rejection_fails_at_runner_boundary() {
        let mut parser = JsonlCollector::new("model-a", 16_384, 4096);
        let mut sink = FixtureSink::new(&mut parser);
        let report = bounded_process(fixture_spec("protocol-success"), &mut sink)
            .await
            .unwrap();
        assert!(sink.started);
        assert!(report.exit_status.as_ref().unwrap().success());
        assert_eq!(parser.take_response().unwrap(), "{}");
        for (mode, expected) in [
            ("native-after-complete", StreamFailure::NativeActionObserved),
            ("model-after-complete", StreamFailure::ReportedModelMismatch),
            ("unknown-after-complete", StreamFailure::UnsupportedItem),
            (
                "invalid-scalar-string",
                StreamFailure::InvalidStructuredResponse,
            ),
        ] {
            let mut parser = JsonlCollector::new("model-a", 16_384, 4096);
            let mut sink = FixtureSink::new(&mut parser);
            let error = bounded_process(fixture_spec(mode), &mut sink)
                .await
                .unwrap_err();
            assert!(sink.started);
            assert_eq!(error.kind, ProcessFailureKind::Sink(expected));
            assert!(error.report.exit_status.is_some());
            assert!(parser.summary.completed_turn);
            assert_eq!(
                parser.summary.native_action_observed,
                mode == "native-after-complete"
            );
            assert_eq!(
                parser.summary.unsupported_item_observed,
                mode == "unknown-after-complete"
            );
            let metadata = format!("{:?}", parser.summary);
            assert!(!metadata.contains("private-action-payload"));
            assert!(!metadata.contains("private-unknown-payload"));
            assert_eq!(parser.take_response(), Err(expected));
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn timeout_also_terminates_a_rust_descendant_in_the_group() {
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("descendant.pid");
        let mut spec = fixture_spec("descendant-parent");
        spec.environment.insert(
            "PERSONA_PROVIDER_PID_FILE".into(),
            pid_file.clone().into_os_string(),
        );
        let error = bounded_process(spec, &mut DiscardStdout).await.unwrap_err();
        assert_eq!(error.kind, ProcessFailureKind::Timeout);
        let pid: u32 = fs::read_to_string(&pid_file)
            .expect("descendant must have started")
            .parse()
            .unwrap();
        assert_process_dead(pid).await;
        assert!(error.report.stdout_eof && error.report.stderr.eof);
        assert!(error.report.exit_status.is_some());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn leader_exit_before_drain_terminates_inherited_pipe_holders() {
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("descendant.pid");
        let mut spec = fixture_spec("leader-exit");
        spec.environment.insert(
            "PERSONA_PROVIDER_PID_FILE".into(),
            pid_file.clone().into_os_string(),
        );
        let mut discard = DiscardStdout;
        let mut sink = FixtureSink::new(&mut discard);
        let report = bounded_process(spec, &mut sink).await.unwrap();
        assert!(sink.started);
        assert!(report.exit_status.as_ref().unwrap().success());
        assert_eq!(report.stop.process_group_sigkill, Some(GroupSignal::Sent));
        let pid: u32 = fs::read_to_string(pid_file)
            .expect("descendant startup acknowledgment")
            .parse()
            .unwrap();
        assert_process_dead(pid).await;
        assert!(report.stdout_eof && report.stderr.eof);
        assert!(!report.stop.cleanup_deadline_reached);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn waitid_preserves_identity_and_disarmed_guard_never_signals_after_reap() {
        use std::os::unix::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("leader.pid");
        let mut command = fixture_command("success");
        command
            .env("PERSONA_PROVIDER_PID_FILE", &pid_file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = i32::try_from(child.id()).unwrap();
        let mut group = OwnedProcessGroup { pid, armed: true };
        let deadline = Instant::now() + Duration::from_secs(3);
        while !leader_exited_without_reaping(pid).unwrap() {
            assert!(Instant::now() < deadline, "leader did not exit");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(acknowledged_pid(&pid_file).await, pid as u32);
        // Repeated successful observations establish that the first did not reap.
        assert!(leader_exited_without_reaping(pid).unwrap());
        assert_eq!(group.signal(), GroupSignal::Sent);
        assert!(!group.armed);
        assert!(child.wait().unwrap().success());
        assert_eq!(group.signal(), GroupSignal::Failed);
        drop(group);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn escaped_writers_exhaust_cleanup_budgets_without_false_eof() {
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("escaped.pid");
        let mut spec = fixture_spec("escaped-parent");
        spec.environment.insert(
            "PERSONA_PROVIDER_PID_FILE".into(),
            pid_file.clone().into_os_string(),
        );
        spec.limits.max_stdout_bytes = 1024;
        spec.limits.max_stderr_bytes = 1024;
        let error = bounded_process(spec, &mut DiscardStdout).await.unwrap_err();
        assert!(matches!(
            error.kind,
            ProcessFailureKind::StdoutOverflow | ProcessFailureKind::StderrOverflow
        ));
        let report = error.report;
        assert!(report.stop.stdout_cleanup_budget_exhausted);
        assert!(report.stop.stderr_cleanup_budget_exhausted);
        assert!(!report.stdout_eof && !report.stderr.eof);
        assert!(!report.stop.cleanup_deadline_reached);
        assert!(report.exit_status.is_some());
        for observed in [report.stdout_bytes_observed, report.stderr.observed_bytes] {
            assert!(observed >= MAX_CLEANUP_DISCARD_BYTES_PER_PIPE as u64);
            assert!(observed <= (1025 + MAX_CLEANUP_DISCARD_BYTES_PER_PIPE) as u64);
        }
        // Exhaustion must close both pipes, allowing the escaped Rust fixture
        // to finish its writers. Its watchdog is only a failure fallback.
        let pid: u32 = fs::read_to_string(pid_file)
            .expect("escaped writer startup acknowledgment")
            .parse()
            .unwrap();
        assert_process_dead(pid).await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_after_startup_kills_the_unreaped_owned_group() {
        let root = tempfile::tempdir().unwrap();
        let descendant_file = root.path().join("descendant.pid");
        let leader_file = root.path().join("leader.pid");
        let mut spec = fixture_spec("descendant-parent");
        spec.limits.timeout_ms = 10_000;
        spec.environment.insert(
            "PERSONA_PROVIDER_PID_FILE".into(),
            descendant_file.clone().into_os_string(),
        );
        spec.environment.insert(
            "PERSONA_PROVIDER_LEADER_PID_FILE".into(),
            leader_file.clone().into_os_string(),
        );
        let mut discard = DiscardStdout;
        let mut running = Box::pin(bounded_process(spec, &mut discard));
        let descendant = tokio::select! {
            result = &mut running => panic!("fixture ended before cancellation: {result:?}"),
            pid = acknowledged_pid(&descendant_file) => pid,
        };
        let leader: u32 = fs::read_to_string(leader_file).unwrap().parse().unwrap();
        drop(running);
        assert_process_dead(leader).await;
        assert_process_dead(descendant).await;
        // Cancellation intentionally produces no terminal receipt or charge claim.
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn oversized_runner_input_is_rejected_without_launch() {
        let mut spec = fixture_spec("success");
        spec.executable = PathBuf::from("/nonexistent-provider-fixture");
        spec.limits.max_input_bytes = 1;
        spec.input = vec![1, 2];
        let error = bounded_process(spec, &mut DiscardStdout).await.unwrap_err();
        assert_eq!(error.kind, ProcessFailureKind::InputTooLarge);
        assert!(!error.report.launched);
    }
}
