//! Resolve native asset input once, while staging the accepted host command.
use super::*;
use schemars::JsonSchema;

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Commands use the shared, mutable host filesystem, including the configured
/// task directory. There is no private per-persona copy or filesystem transaction;
/// another participant can change a file while your model call runs. Inspect
/// current files before revising them or judging the final package, and choose
/// useful coordination with peers for shared edits. A prior command result or
/// captured preview describes the observed bytes, not later contents at that path.
pub(super) struct Inputs {
    /// Bash command or script; tools and packages may be installed directly.
    pub command: String,
    /// Choose the next decision's result context: complete supplies all retained
    /// command and output text; reference supplies execution metadata and exact
    /// readable text references. Both retain the complete observed output.
    pub result_context: context::ResultContext,
    /// Host working directory; omission uses the offered default.
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Literal UTF-8 stdin. Mutually exclusive with stdin_asset.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Supply an exact full native supporting asset as stdin without copying
    /// its body into this response. Use cat > a host path to export it, or an
    /// interpreter such as python3 - to execute it. This does not select model
    /// context. A same-response proposal requires its save ID in depends_on.
    #[serde(default)]
    pub stdin_asset: Option<Asset>,
    /// Optional timeout chosen by the persona; omission means no timeout.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Asset {
    /// Exact full UTF-8 asset, including a proposal-op:<save-id> revision.
    Reference { reference: p::ExactRef },
    /// Handle from this admission's input_file_choices table; metadata is enough
    /// to name an input, and does not claim that its body was supplied to the LLM.
    InputFile { handle: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub reference: p::ExactRef,
    pub bytes: usize,
    pub sha256: String,
    binding_sha256: String,
}

impl Inputs {
    fn reference(&self, prepared: &Prepared) -> Result<Option<p::ExactRef>> {
        self.stdin_asset
            .as_ref()
            .map(|asset| match asset {
                Asset::Reference { reference } => Ok(reference.clone()),
                Asset::InputFile { handle } => {
                    input_files::resolve_output(prepared, std::slice::from_ref(handle))?
                        .pop()
                        .ok_or_else(|| held("host stdin asset handle unavailable"))
                }
            })
            .transpose()
    }

    pub(super) fn validate(
        &self,
        prepared: &Prepared,
        decision: &p::Decision,
        dependencies: &[String],
    ) -> Result<()> {
        require(
            self.stdin.is_none() || self.stdin_asset.is_none(),
            "choose literal stdin or stdin_asset, not both",
        )?;
        if let Some(reference) = self.reference(prepared)? {
            if reference.revision.starts_with(p::PROPOSED_OPERATION_PREFIX)
                || reference.revision.starts_with(p::PROPOSED_REVISION_PREFIX)
            {
                let index = proposal_index(decision);
                let operation = index
                    .get(&reference.revision)
                    .and_then(|index| decision.operations.get(*index))
                    .ok_or_else(|| held("host stdin asset proposal absent"))?;
                let save = operation.file_save().unwrap();
                require(
                    *save.source == reference.source
                        && matches!(save.scope, p::Scope::Assets {})
                        && dependencies.iter().any(|id| id == save.id),
                    "host stdin asset proposal needs its exact supporting save dependency",
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn materialize(
        self,
        state: &State,
        prepared: &Prepared,
        proposals: &BTreeMap<String, String>,
        now: i64,
    ) -> Result<(h::Inputs, Option<Receipt>)> {
        let reference = self
            .reference(prepared)?
            .map(|r| resolve(&r, proposals))
            .transpose()?;
        let mut stdin = self.stdin;
        let receipt = if let Some(reference) = reference {
            let reference = wire_reference(&file_reference(&reference)?);
            let a = admission(&state.personas[&prepared.request.persona], now);
            let (file, binding_sha256) = current_asset(state, &reference, &a)?;
            let text = std::str::from_utf8(&file.bytes)
                .map_err(|_| held("host stdin asset is not UTF-8"))?
                .to_owned();
            let receipt = Receipt {
                reference,
                bytes: file.bytes.len(),
                sha256: s::sha256_bytes(&file.bytes),
                binding_sha256,
            };
            stdin = Some(text);
            Some(receipt)
        } else {
            None
        };
        Ok((
            h::Inputs {
                command: self.command,
                working_directory: self.working_directory,
                stdin,
                timeout_seconds: self.timeout_seconds,
            },
            receipt,
        ))
    }
}

fn current_asset<'a>(
    state: &'a State,
    reference: &p::ExactRef,
    a: &f::Admission,
) -> Result<(&'a f::FileRevision, String)> {
    let reference = file_reference(reference)?;
    require(
        reference.representation == f::Representation::Utf8
            && reference.start.is_none()
            && reference.end.is_none(),
        "host stdin requires an exact full UTF-8 supporting asset",
    )?;
    let stored = record(state, &reference)?;
    require(
        stored.record.scope == f::Scope::Assets
            && !stored.record.retired
            && lineage_permitted(state, std::slice::from_ref(&reference), a),
        "host stdin asset unavailable",
    )?;
    // Resolve the complete qualification closure for the exact asset. The
    // runner consumes these bytes directly, without a corpus or prompt quota.
    f::assemble(
        &corpus(state, a)?,
        std::slice::from_ref(&reference),
        &[],
        &[],
        a,
        usize::MAX,
        0,
        0,
    )?;
    let bindings: Vec<_> = state
        .accepted_bindings
        .iter()
        .filter(|binding| {
            binding.source == reference.source && binding.file_revision == reference.revision
        })
        .collect();
    Ok((&stored.record, digest(&(stored, bindings))?))
}

pub(super) fn require_current(state: &State, job: &Job, now: i64) -> Result<()> {
    if let Some(receipt) = &job.stdin_asset {
        let a = admission(&state.personas[&job.actor], now);
        let (file, binding) = current_asset(state, &receipt.reference, &a)?;
        require(
            binding == receipt.binding_sha256
                && file.bytes.len() == receipt.bytes
                && s::sha256_bytes(&file.bytes) == receipt.sha256
                && job.input.stdin.as_deref().map(str::as_bytes) == Some(file.bytes.as_slice())
                && exposure_permitted(state, &job.exposure, &a),
            "host stdin asset or its current bindings changed before dispatch",
        )?;
    }
    Ok(())
}

/// Results retain the input identity without duplicating large script/data
/// bodies. The accepted intent still binds the complete actual stdin bytes.
pub(super) fn observed(job: &Job) -> Result<Value> {
    let mut value = serde_json::to_value(&job.input)?;
    if let Some(choice) = job.result_context {
        value["result_context"] = serde_json::to_value(choice)?;
    }
    if let Some(receipt) = &job.stdin_asset {
        value.as_object_mut().unwrap().remove("stdin");
        value["stdin_asset"] = json!({"reference":receipt.reference,
            "bytes":receipt.bytes,"sha256":receipt.sha256,"body_supplied":false,
            "meaning":"Exact native asset bytes materialized as command stdin; body retained in the accepted intent, not repeated in this result. This is not evidence that the process consumed every byte."});
    }
    Ok(value)
}
