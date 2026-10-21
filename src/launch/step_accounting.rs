//! Actual saved-launch funding supplied with the admission that spent a step.
use super::*;

pub(crate) const RULE: &str = "This is the saved launch allowance after reserving this input, shared across all its personas and works. It is separate from attempts_left for this work and does not promise a personal next turn. Other admissions can spend the remaining steps; a foreground session can also end sooner. Zero permits this already admitted response and its available operations, but funds no later model input. Consider the remaining useful work, observed results and any unresolved limitations when choosing what to deliver and whether to continue. An Immediate choice creates no funding, and a command proposed now is not yet an observed result.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchStepAllowance {
    pub launch: s::ObjectRef,
    pub steps_spent: u64,
    pub lifetime_step_limit: u64,
}

impl LaunchStepAllowance {
    pub(crate) fn control(&self) -> Result<Value> {
        require(
            self.launch.id == ID
                && self.launch.revision > 0
                && self.steps_spent > 0
                && self.steps_spent <= self.lifetime_step_limit,
            "invalid admitted launch step accounting",
        )?;
        Ok(json!({
            "launch":self.launch,
            "steps_spent_including_this_admission":self.steps_spent,
            "lifetime_step_limit":self.lifetime_step_limit,
            "remaining_after_this_admission":self.lifetime_step_limit-self.steps_spent,
            "scope":"shared_saved_launch",
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
    };
    allowance.control()?;
    Ok(Some(allowance))
}

pub(super) fn require_capacity(saved: &Saved) -> Result<()> {
    if saved.steps >= saved.setup.max_total_steps {
        Err(RuntimeError::LifetimeStepLimit)
    } else {
        Ok(())
    }
}

/// Distinguish a spent lifetime allocation from a foreground session boundary.
/// This reads the actual saved counter, never a guessed number of model calls.
pub(super) fn bound_stop(app: &mut Runtime) -> Result<&'static str> {
    let (_, saved) = load(app)?;
    Ok(if saved.steps >= saved.setup.max_total_steps {
        "lifetime_step_limit"
    } else {
        "step_limit"
    })
}
