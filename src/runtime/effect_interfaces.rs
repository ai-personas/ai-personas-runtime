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
/// Borrow the JSON value for decoding, then check the typed wire containers.
pub(super) fn inputs<T: serde::de::DeserializeOwned + Serialize + schemars::JsonSchema>(
    admitted_schema: &Value,
    received: &Value,
) -> Result<T> {
    require(
        *admitted_schema == serde_json::to_value(schemars::schema_for!(T))?,
        "admitted capability input definition differs from the current decoder",
    )?;
    let input = T::deserialize(received).map_err(|error| {
        // A malformed string can become an unknown object key. Serde's raw
        // error would then copy that authored body into failure metadata and
        // later recovery prompts. Derive hints only from this admitted schema;
        // the complete response remains retained separately for exact reads.
        let section = "capability inputs";
        let hint = p::ShapeHint {
            section: section.into(),
            allowed_fields: crate::schema_diagnostics::object_variant(
                admitted_schema, admitted_schema, received,
            )
            .and_then(|object| object["properties"].as_object())
            .map(|fields| fields.keys().cloned().collect())
            .unwrap_or_default(),
            issues: crate::schema_diagnostics::issues(
                admitted_schema, admitted_schema, received, section,
            ),
        };
        p::ProtocolError::Invalid {
            path: hint.to_string(),
            rule: p::shape_rule(&error),
        }
    })?;
    // Effect.inputs is opaque to the outer Decision decoder. Serde accepts
    // positional arrays for named structs, including nested exact references;
    // compare against this capability's typed shape before staging any job.
    // Actual array fields and opaque JSON values keep their declared shapes.
    if !p::same_container_shapes(received, &serde_json::to_value(&input)?) {
        return Err(p::ProtocolError::Invalid {
            path: "capability inputs".into(),
            rule: "typed records require JSON objects, not positional arrays",
        }
        .into());
    }
    Ok(input)
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
