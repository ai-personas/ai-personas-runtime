//! Requester input can arrive while an execution owner retains its transports.
//! Inbox writes do not change semantic state or invalidate an admitted call.
use super::*;

impl Store {
    /// Inspect and change the input inbox in one transaction. Only the execution
    /// owner may also install observations into the runtime at a settled boundary.
    /// Input identities, immutable revisions and actual storage accounting remain
    /// ordinary Store records; neither route dispatches or recovers an invocation.
    pub(crate) fn feedback_transaction<T: Serialize, E, F>(
        &mut self,
        owner: Option<&ExecutionOwner>,
        build: F,
    ) -> Result<std::result::Result<T, E>>
    where
        F: FnOnce(StoredObject, StoredObject, &RuntimeInspection<'_, '_>)
            -> std::result::Result<(T, Vec<Change>), E>,
    {
        self.transaction(|work| {
            let runtime = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let launch = work.required("object", "runtime-launch-v1")?;
            let (result, changes) = match build(runtime, launch, &RuntimeInspection { work }) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let mut semantic = false;
            for change in &changes {
                let object = &change.object;
                let inbox = object.id == "requester-feedback-inbox-v1"
                    && object.kind == "requester-feedback-inbox-v1";
                let input = object.id.starts_with("requester-feedback-input:")
                    && object.kind == "requester-feedback-input-v1";
                if object.id == RUNTIME_CONTROL_STATE_ID && owner.is_some() {
                    semantic = true;
                } else if !inbox && !input {
                    return Err(StoreError::Invalid("requester feedback transaction object".into()));
                }
            }
            let candidates = work.validate_changes(&changes)?;
            for change in &changes {
                work.dependencies(&change.depends_on, &candidates)?;
            }
            if semantic {
                work.bump_epoch()?;
            }
            work.apply_changes(&changes, BudgetClass::Ordinary)?;
            work.returned(&result, work.limits.max_read_bytes, BudgetClass::Ordinary)?;
            Ok(Ok(result))
        })
    }
}
