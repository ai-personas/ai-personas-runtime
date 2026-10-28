//! Hints from known schema members after a typed decoder has rejected an input.
//! This is not a validator or a repair pass. Opaque values stay opaque and
//! dynamic map keys and rejected tags never become diagnostic text.
use serde_json::Value;
use std::collections::BTreeSet;

fn resolved<'a>(schema: &'a Value, root: &'a Value, received: &Value) -> Option<&'a Value> {
    let mut current = schema;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current as *const Value) {
            return None;
        }
        if let Some(reference) = current["$ref"].as_str() {
            current = root.pointer(reference.strip_prefix('#')?)?;
        } else if let Some(options) = current["oneOf"]
            .as_array()
            .or_else(|| current["anyOf"].as_array())
        {
            if let Some(kind) = received.get("kind").and_then(Value::as_str) {
                if let Some(variant) = options
                    .iter()
                    .find(|variant| variant["properties"]["kind"]["const"].as_str() == Some(kind))
                {
                    current = variant;
                    continue;
                }
            }
            // Generated Option<T> schemas have one nullable and one value
            // branch. Do not guess among other unrecognized alternatives.
            if options.len() == 2 && options.iter().any(|option| option["type"] == "null") {
                current = options
                    .iter()
                    .find(|option| (option["type"] == "null") == received.is_null())?;
            } else {
                return None;
            }
        } else {
            return Some(current);
        }
    }
}

pub(crate) fn object_variant<'a>(
    schema: &'a Value,
    root: &'a Value,
    received: &Value,
) -> Option<&'a Value> {
    let current = resolved(schema, root, received)?;
    current["properties"].as_object().map(|_| current)
}

/// Describe unknown or missing members and definite type/enum mismatches within
/// the known section. Internally tagged Serde errors may omit the nested path,
/// so inspect its known descendants too, without copying arbitrary input text.
pub(crate) fn issues(schema: &Value, root: &Value, received: &Value, section: &str) -> Vec<String> {
    let mut pending = vec![(schema, received, section.to_owned())];
    let mut issues = BTreeSet::new();
    while let Some((schema, received, path)) = pending.pop() {
        if let Some(expected) = mismatch(schema, root, received, &mut BTreeSet::new()) {
            issues.insert(format!("{path}: expected {expected}"));
            continue;
        }
        let alternatives = schema;
        let Some(schema) = resolved(schema, root, received) else {
            // An absent or unknown tag cannot select a variant. Describe the
            // declared alternatives without guessing which one was intended.
            if let Some(values) = received.as_object() {
                if let Some(kinds) = tagged_kinds(schema, root, &mut BTreeSet::new()) {
                    if values.get("kind").is_some_and(|value|
                        value.as_str().is_none_or(|kind| !kinds.contains(kind)))
                    {
                        issues.insert(format!("{path}.kind: expected one of {}",
                            serde_json::to_string(&kinds).expect("schema tag strings serialize")));
                    }
                }
                for name in required_members(schema, root, &mut BTreeSet::new()) {
                    if !values.contains_key(name) {
                        issues.insert(format!("{path}.{name} is required"));
                    }
                }
            }
            continue;
        };
        if let Some(fields) = schema["properties"].as_object() {
            let Some(values) = received.as_object() else {
                continue;
            };
            if schema["additionalProperties"] == false
                && values.keys().any(|name| !fields.contains_key(name))
            {
                // Name a misplaced member only when the schema declares that
                // exact name at this position in another alternative. This
                // locates common variant mix-ups without echoing authored keys
                // or guessing which operation the persona intended.
                for name in declared_members(alternatives, root) {
                    if values.contains_key(name) && !fields.contains_key(name) {
                        issues.insert(format!("{path}.{name} is not allowed for this variant"));
                    }
                }
                issues.insert(format!(
                    "{path} contains unknown members; allowed fields: {}",
                    fields
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(required) = schema["required"].as_array() {
                for name in required.iter().filter_map(Value::as_str) {
                    if fields.contains_key(name) && !values.contains_key(name) {
                        issues.insert(format!("{path}.{name} is required"));
                    }
                }
            }
            for (name, field) in fields {
                if let Some(value) = values.get(name) {
                    pending.push((field, value, format!("{path}.{name}")));
                }
            }
        } else if let (Some(item), Some(values)) = (schema.get("items"), received.as_array()) {
            for (index, value) in values.iter().enumerate() {
                pending.push((item, value, format!("{path}[{index}]")));
            }
        } else if let (Some(item), Some(values)) = (
            schema
                .get("additionalProperties")
                .filter(|value| value.is_object()),
            received.as_object(),
        ) {
            // A map's declared value shape is known; its authored keys are not.
            // Identical expectations collapse, without printing those keys.
            for value in values.values() {
                pending.push((item, value, format!("{path} values")));
            }
        }
    }
    issues.into_iter().collect()
}

fn declared_members<'a>(schema: &'a Value, root: &'a Value) -> BTreeSet<&'a str> {
    let mut pending = vec![schema];
    let mut visited = BTreeSet::new();
    let mut names = BTreeSet::new();
    while let Some(schema) = pending.pop() {
        if !visited.insert(schema as *const Value) { continue; }
        if let Some(reference) = schema["$ref"].as_str() {
            if let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p)) {
                pending.push(target);
            }
        }
        for union in ["oneOf", "anyOf", "allOf"] {
            if let Some(branches) = schema[union].as_array() { pending.extend(branches); }
        }
        if let Some(fields) = schema["properties"].as_object() {
            names.extend(fields.keys().map(String::as_str));
        }
    }
    names
}

/// Return the complete declared tag set only when every object alternative is
/// tagged. A broad or opaque branch prevents a claim about allowed values.
/// The caller has an actual object, so nullable non-object branches cannot apply.
fn tagged_kinds<'a>(
    schema: &'a Value,
    root: &'a Value,
    visiting: &mut BTreeSet<*const Value>,
) -> Option<BTreeSet<&'a str>> {
    let address = schema as *const Value;
    if !visiting.insert(address) {
        return None;
    }
    let result = (|| {
        if let Some(reference) = schema["$ref"].as_str() {
            return tagged_kinds(root.pointer(reference.strip_prefix('#')?)?, root, visiting);
        }
        if let Some(options) = schema["oneOf"].as_array()
            .or_else(|| schema["anyOf"].as_array())
        {
            let mut kinds = BTreeSet::new();
            for option in options.iter().filter(|option| option["type"] != "null") {
                kinds.extend(tagged_kinds(option, root, visiting)?);
            }
            return (!kinds.is_empty()).then_some(kinds);
        }
        if !schema["required"].as_array()?.iter().any(|name| name == "kind") {
            return None;
        }
        let tag = &schema["properties"]["kind"];
        if let Some(kind) = tag["const"].as_str() {
            return Some(BTreeSet::from([kind]));
        }
        let kinds: BTreeSet<_> = tag["enum"].as_array()?.iter()
            .map(Value::as_str).collect::<Option<_>>()?;
        (!kinds.is_empty()).then_some(kinds)
    })();
    visiting.remove(&address);
    result
}

fn required_members<'a>(
    schema: &'a Value,
    root: &'a Value,
    visiting: &mut BTreeSet<*const Value>,
) -> BTreeSet<&'a str> {
    let address = schema as *const Value;
    if !visiting.insert(address) {
        return BTreeSet::new();
    }
    let result = if let Some(reference) = schema["$ref"].as_str() {
        reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
            .map(|target| required_members(target, root, visiting))
            .unwrap_or_default()
    } else if let Some(options) = schema["oneOf"]
        .as_array()
        .or_else(|| schema["anyOf"].as_array())
    {
        let mut members = options
            .iter()
            .map(|option| required_members(option, root, visiting));
        let mut common = members.next().unwrap_or_default();
        for branch in members {
            common.retain(|name| branch.contains(name));
        }
        common
    } else {
        schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| schema["properties"].get(*name).is_some())
            .collect()
    };
    visiting.remove(&address);
    result
}

fn mismatch(
    schema: &Value,
    root: &Value,
    received: &Value,
    visiting: &mut BTreeSet<*const Value>,
) -> Option<String> {
    let address = schema as *const Value;
    if !visiting.insert(address) {
        return None;
    }
    let result = constraint(schema, root, received, visiting);
    visiting.remove(&address);
    result
}

fn constraint(
    schema: &Value,
    root: &Value,
    received: &Value,
    visiting: &mut BTreeSet<*const Value>,
) -> Option<String> {
    if let Some(reference) = schema["$ref"].as_str() {
        return mismatch(
            root.pointer(reference.strip_prefix('#')?)?,
            root,
            received,
            visiting,
        );
    }
    if let Some(options) = schema["oneOf"]
        .as_array()
        .or_else(|| schema["anyOf"].as_array())
    {
        let mut expected = BTreeSet::new();
        for option in options {
            // A compatible or unrecognized branch supplies no mismatch hint.
            expected.insert(mismatch(option, root, received, visiting)?);
        }
        return (!expected.is_empty())
            .then(|| expected.into_iter().collect::<Vec<_>>().join(" or "));
    }
    if let Some(values) = schema["enum"].as_array() {
        if !values.contains(received) {
            return Some(format!("one of {}", schema["enum"]));
        }
    }
    if let Some(value) = schema.get("const") {
        if value != received {
            return Some(format!("{value}"));
        }
    }
    let matches_type = |name: &str| match name {
        "object" => received.is_object(),
        "array" => received.is_array(),
        "string" => received.is_string(),
        "boolean" => received.is_boolean(),
        "null" => received.is_null(),
        "integer" => received.is_i64() || received.is_u64(),
        "number" => received.is_number(),
        _ => true,
    };
    if let Some(name) = schema["type"].as_str() {
        return (!matches_type(name)).then(|| format!("JSON {name}"));
    }
    if let Some(types) = schema["type"].as_array() {
        let names = types.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        if !names.is_empty() && !names.iter().any(|name| matches_type(name)) {
            return Some(format!("JSON {}", names.join(" or ")));
        }
    }
    None
}
