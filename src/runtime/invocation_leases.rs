//! A locally terminal failure holds its actor, not every peer in the runtime.
use super::*;

impl Runtime {
    /// Inspect only current persona leases. Failed records cannot adopt another
    /// response; their actor stays unavailable until its own explicit recovery
    /// or accepted-continuation retirement. No lease, receipt or usage is changed.
    /// Other states, including adoption awaiting local completion, still block
    /// foreground mutations of the shared semantic epoch.
    pub(super) fn invocation_leases_terminal(&mut self, state: &State) -> Result<bool> {
        self.invocation_leases_owned(state, &BTreeSet::new())
    }

    /// Only the foreground coordinator may supply the exact calls whose futures
    /// it still owns. An orphan or a call owned elsewhere retains the old fence.
    pub(super) fn invocation_leases_owned(
        &mut self,
        state: &State,
        owned: &BTreeSet<String>,
    ) -> Result<bool> {
        check_invocation_leases(state, owned, |id| {
            Ok(self.store.trusted_host().invocation(id)?)
        })
    }
}

/// Share the same lease checks with a transaction-bound operator projection.
pub(crate) fn check_invocation_leases(
    state: &State,
    owned: &BTreeSet<String>,
    mut lookup: impl FnMut(&str) -> Result<s::InvocationRecord>,
) -> Result<bool> {
    for (actor, persona) in &state.personas {
        let Some(id) = &persona.active_invocation else {
            continue;
        };
        let invocation = lookup(id)?;
        let pending = owned.contains(id)
            && matches!(
                invocation.state,
                s::InvocationState::Dispatched | s::InvocationState::Responded
            );
        if !pending && invocation.state != s::InvocationState::Failed {
            return Ok(false);
        }
        let prepared = decode_prepared(&invocation)?;
        require(
            invocation.id == *id
                && prepared.request.id == *id
                && prepared.request.persona == *actor
                && persona.owner_epoch == prepared.owner_epoch
                && persona.admission_serial == prepared.admission_serial
                && (!pending
                    || (prepared.request.profile == Profile::Direct
                        && prepared.intake_episode.is_none()
                        && prepared.decision_scope.is_some())),
            "invocation lease binding changed",
        )?;
    }
    Ok(true)
}
