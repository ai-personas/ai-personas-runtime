//! Actual saved-launch usage supplied with the admission that recorded a step.
use super::*;

pub(crate) const RULE: &str = "Steps count admitted model calls across this saved launch, not remaining work or a quota. Choose continuation according to the actual need and observed results.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchStepAllowance {
    pub launch: s::ObjectRef,
    pub steps_spent: u64,
    pub lifetime_step_limit: u64,
    /// The saved task directory at this exact launch admission. Absence records
    /// an admission whose host interface used the foreground process directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<PathBuf>,
}

impl LaunchStepAllowance {
    pub(crate) fn control(&self) -> Result<Value> {
        require(
            self.launch.id == ID
                && self.launch.revision > 0
                && self.steps_spent > 0,
            "invalid admitted launch step accounting",
        )?;
        Ok(json!({
            "launch":self.launch,
            "steps_spent_including_this_admission":self.steps_spent,
            "limits_enforced":false,
            "scope":"shared_saved_launch",
            "working_directory":self.working_directory,
            "rule":RULE
        }))
    }
}

pub(crate) fn capture(
    app: &mut Runtime,
    request: &PrepareRequest,
) -> Result<Option<LaunchStepAllowance>> {
    let Some(object) = app.store.read(ID)? else {
        return Ok(None);
    };
    let saved = decode_saved(&object)?;
    // A direct Runtime admission without a matching saved-launch reservation
    // does not acquire an invented launch allowance. Episode preparation has
    // already committed its own launch debit before this capture.
    if saved.works.get(&request.work).and_then(Option::as_deref) != Some(request.id.as_str()) {
        return Ok(None);
    }
    require(
        saved.format == 2 && request.id == format!("call-{:08}", saved.steps),
        "saved launch reservation does not match this admission",
    )?;
    let allowance = LaunchStepAllowance {
        launch: object.exact_ref()?,
        steps_spent: saved.steps,
        lifetime_step_limit: saved.setup.max_total_steps,
        working_directory: Some(saved.working_directory),
    };
    allowance.control()?;
    Ok(Some(allowance))
}
