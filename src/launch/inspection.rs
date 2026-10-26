//! One fresh metered runtime/launch snapshot for one operator projection.
//! This is not a cross-request cache or an alternative mutation admission path.
use super::*;

/// A complete view, including lifecycle and exact record lookups, is
/// built inside one Store transaction. Only its final public output is charged.
pub(super) fn project<T: Serialize, E: From<RuntimeError> + From<s::StoreError>>(
    root: &Path,
    projection: impl FnOnce(&Snapshot, &s::RuntimeInspection<'_, '_>) -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let mut store = Runtime::inspection_store(root)?;
    store.trusted_host().runtime_projection(
        session::PROJECTION_BYTE_ESTIMATE as u64,
        |runtime, launch, records| {
            let state = Runtime::decode_inspection_state(&runtime)?;
            let saved = decode_saved(&launch)?;
            let snapshot = Snapshot {
                runtime: std::sync::Arc::new(runtime),
                state,
                launch,
                saved,
                epoch: records.epoch(),
            };
            projection(&snapshot, records)
        },
    )?
}

pub(super) struct Snapshot {
    pub runtime: std::sync::Arc<s::StoredObject>,
    pub state: State,
    pub launch: s::StoredObject,
    pub saved: Saved,
    pub epoch: u64,
}

impl Snapshot {
    pub fn open(root: &Path) -> Result<(Runtime, Self)> {
        let (mut app, runtime, state, epoch) = Runtime::open_for_launch_snapshot(root)?;
        let (launch, saved) = load(&mut app)?;
        let snapshot = Self {
            runtime,
            state,
            launch,
            saved,
            epoch,
        };
        snapshot.unchanged(&app)?;
        Ok((app, snapshot))
    }

    /// Metering does not advance the semantic epoch. A concurrent state change
    /// invalidates the projection; do not return content checked against an
    /// earlier permission or ownership state as a fresh successful view.
    pub fn unchanged(&self, app: &Runtime) -> Result<()> {
        let actual = app.store.epoch()?;
        if actual != self.epoch {
            return Err(s::StoreError::StaleEpoch {
                expected: self.epoch,
                actual,
            }
            .into());
        }
        Ok(())
    }
}
