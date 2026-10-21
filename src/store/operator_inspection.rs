//! Project one bounded operator view from a coherent, read-only set of records.
use super::*;

pub(crate) struct RuntimeInspection<'a, 'db> {
    work: &'a Work<'db>,
}

impl RuntimeInspection<'_, '_> {
    pub(crate) fn epoch(&self) -> u64 {
        self.work.meta.epoch
    }
    pub(crate) fn frozen(&self) -> bool {
        self.work.meta.frozen
    }

    pub(crate) fn invocation(&self, id: &str) -> Result<Option<InvocationRecord>> {
        self.work.id(id)?;
        let record: Option<InvocationRecord> = self.work.get("invocation", id)?;
        if let Some(record) = &record {
            live_invocation(record)?;
        }
        Ok(record)
    }

    pub(crate) fn reservation(&self, id: &ReservationId) -> Result<ReservationRecord> {
        self.work.id(&id.id)?;
        self.work.required("reservation", &id.key())
    }
}

impl TrustedHost<'_> {
    /// The trusted projection validates operator authority and disclosure before
    /// returning its bounded output. Internal heads and supporting records never
    /// cross this view boundary. Only the returned projection spends Ordinary
    /// navigation. The callback has no Store mutation or execution capability.
    pub(crate) fn runtime_projection<T: Serialize, E, F>(
        &mut self,
        maximum: u64,
        project: F,
    ) -> Result<std::result::Result<T, E>>
    where
        F: FnOnce(
            StoredObject,
            StoredObject,
            &RuntimeInspection<'_, '_>,
        ) -> std::result::Result<T, E>,
    {
        self.store.transaction(|work| {
            let runtime = work.required("object", RUNTIME_CONTROL_STATE_ID)?;
            let launch = work.required("object", "runtime-launch-v1")?;
            let view = project(runtime, launch, &RuntimeInspection { work });
            if let Ok(view) = &view {
                work.returned(
                    view,
                    maximum.min(work.limits.max_read_bytes),
                    BudgetClass::Ordinary,
                )?;
            }
            Ok(view)
        })
    }
}
