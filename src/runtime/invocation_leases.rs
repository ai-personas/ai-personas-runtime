//! A locally terminal failure holds its actor, not every peer in the runtime.
use super::*;

impl Runtime {
    /// Inspect only current persona leases. Failed records cannot adopt another
    /// response; their actor stays unavailable until its own explicit recovery
    /// or accepted-continuation retirement. No lease, receipt or usage is changed.
    /// Other states, including adoption awaiting local completion, still block
    /// foreground mutations of the shared semantic epoch.
    pub(super) fn invocation_leases_terminal(&mut self, state: &State) -> Result<bool> {
        for (actor, persona) in &state.personas {
            let Some(id) = &persona.active_invocation else {
                continue;
            };
            let invocation = self.store.trusted_host().invocation(id)?;
            if invocation.state != s::InvocationState::Failed {
                return Ok(false);
            }
            let prepared = decode_prepared(&invocation)?;
            require(
                invocation.id == *id
                    && prepared.request.id == *id
                    && prepared.request.persona == *actor
                    && persona.owner_epoch == prepared.owner_epoch
                    && persona.admission_serial == prepared.admission_serial,
                "failed invocation lease binding changed",
            )?;
        }
        Ok(true)
    }
}
