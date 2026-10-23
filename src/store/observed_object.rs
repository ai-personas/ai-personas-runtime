//! Recheck complete current object bytes before reusing a prior decoded read.
//! This process-local identity never replaces a transaction or a current read.
use super::*;

/// Constructed only from a decoded Store read. Fields stay private so a caller
/// cannot supply an unverified digest or a smaller navigation charge.
pub(crate) struct ObjectReadIdentity {
    key: String,
    raw_sha256: String,
    returned_bytes: u64,
}

pub(crate) enum ObjectRead {
    Absent,
    Unchanged,
    Present { object: StoredObject, identity: ObjectReadIdentity },
}

impl Store {
    /// Read current bytes and validate cell metadata in the normal transaction.
    /// Only an exact match with a prior successful read omits JSON decoding.
    /// Charge the same complete canonical result size as an ordinary read.
    pub(crate) fn read_observed_object(
        &mut self,
        id: &str,
        previous: Option<&ObjectReadIdentity>,
    ) -> Result<ObjectRead> {
        self.transaction(|work| {
            work.id(id)?;
            let cell = phase_timing::measure("store.read_object_bytes", || work.cell("object", id))?;
            let Some(cell) = cell else {
                work.returned(&None::<StoredObject>, work.limits.max_read_bytes, BudgetClass::Ordinary)?;
                return Ok(ObjectRead::Absent);
            };
            let raw_sha256 = phase_timing::measure("store.fingerprint_object_bytes", || {
                Ok::<_, StoreError>(sha256_bytes(&cell.data))
            })?;
            if let Some(previous) = previous.filter(|known| known.key == id && known.raw_sha256 == raw_sha256) {
                return phase_timing::measure("store.reuse_observed_object", || {
                    work.charge_returned_bytes(previous.returned_bytes, BudgetClass::Ordinary)?;
                    Ok(ObjectRead::Unchanged)
                });
            }
            let object: StoredObject = phase_timing::measure("store.decode_observed_object", || {
                decode_json(&cell.data)
            })?;
            let returned_bytes = phase_timing::measure("store.measure_observed_object", || {
                serialized_size(&Some(&object))
            })?;
            work.charge_returned_bytes(returned_bytes, BudgetClass::Ordinary)?;
            Ok(ObjectRead::Present {
                object,
                identity: ObjectReadIdentity { key: id.into(), raw_sha256, returned_bytes },
            })
        })
    }
}
