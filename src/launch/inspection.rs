//! One fresh metered runtime/launch snapshot for one operator projection.
//! This is not a cross-request cache or an alternative mutation admission path.
use super::*;

pub(super) struct Snapshot {
    pub runtime: s::StoredObject,
    pub state: State,
    pub launch: s::StoredObject,
    pub saved: Saved,
    pub epoch: u64,
}

impl Snapshot {
    pub fn open(root: &Path) -> Result<(Runtime, Self)> {
        let (mut app, runtime, state, epoch) = Runtime::open_for_launch_snapshot(root)?;
        let (launch, saved) = load(&mut app)?;
        let snapshot = Self { runtime, state, launch, saved, epoch };
        snapshot.unchanged(&app)?;
        Ok((app, snapshot))
    }

    /// Metering does not advance the semantic epoch. A concurrent state change
    /// invalidates the projection; do not return content checked against an
    /// earlier permission or ownership state as a fresh successful view.
    pub fn unchanged(&self, app: &Runtime) -> Result<()> {
        let actual = app.store.epoch()?;
        if actual != self.epoch {
            return Err(s::StoreError::StaleEpoch { expected: self.epoch, actual }.into());
        }
        Ok(())
    }
}
