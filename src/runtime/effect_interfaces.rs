//! Resolve authored capability IDs against this invocation's exact admission.
use super::*;

/// Callers filter by the authored capability ID. Never choose an arbitrary
/// version if an admission supplies more than one interface for that ID.
pub(super) fn supplied<'a, T: 'a>(
    mut matching: impl Iterator<Item = &'a T>,
    capability_id: &str,
) -> Result<&'a T> {
    let interface = matching.next().ok_or_else(|| {
        held(format!("capability {capability_id} was not supplied by this admission"))
    })?;
    require(
        matching.next().is_none(),
        &format!("capability {capability_id} has ambiguous admitted interfaces"),
    )?;
    Ok(interface)
}

/// Decode against the exact input definition admitted to this invocation.
/// No current work, asset, host directory or implementation binding is read.
/// Borrow the JSON value so scripts and data do not need an intermediate clone.
pub(super) fn inputs<T: serde::de::DeserializeOwned + schemars::JsonSchema>(
    admitted_schema: &Value,
    received: &Value,
) -> Result<T> {
    require(
        *admitted_schema == serde_json::to_value(schemars::schema_for!(T))?,
        "admitted capability input definition differs from the current decoder",
    )?;
    Ok(T::deserialize(received)?)
}

/// The runtime retains the complete binding in Prepared and in each accepted
/// job. The authored operation selects it by ID, without repeating its digest.
pub(super) fn definition(interface: &impl Serialize) -> Result<Value> {
    let mut definition = serde_json::to_value(interface)?;
    let fields = definition.as_object_mut()
        .ok_or_else(|| invalid("effect interface definition is not an object"))?;
    require(
        fields.remove("binding_revision").is_some(),
        "effect interface binding absent",
    )?;
    Ok(definition)
}
