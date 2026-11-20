//! Operator-only comparison. Replay one hypothetical input on each owner's
//! *adopted* packages. Nothing here supplies context or changes a persona.
use super::{character_paths, failure, store, subject, Result};
use crate::{
    neural::protocol::Persona,
    persona_model::{Appraisal, FeatureContract, FeatureValue},
};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub type Values = BTreeMap<String, f64>;

#[derive(Serialize)]
pub struct Atlas {
    /// Only known axes appear here. A known zero is not an omitted input.
    pub inputs: Values,
    pub contract: String,
    pub people: Vec<Entry>,
    pub scope: &'static str,
}

#[derive(Serialize)]
pub struct Entry {
    pub owner: String,
    #[serde(flatten)]
    pub reading: Reading,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Reading {
    Pending {
        profile: Profile,
    },
    Available {
        profile: Profile,
        epochs: Vec<Epoch>,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Serialize)]
pub struct Profile {
    pub label: String,
    pub identity: String,
    pub origin: String,
    pub seed: String,
    pub seed_author: String,
}

#[derive(Serialize)]
pub struct Epoch {
    pub epoch: u64,
    pub kind: String,
    pub change: String,
    pub decision: String,
    pub reason: String,
    pub package: String,
    pub numerical: String,
    pub inclinations: Values,
}

pub fn parse_inputs(pairs: &[String]) -> Result<Values> {
    let mut values = Values::new();
    for pair in pairs {
        let (axis, value) = pair
            .split_once('=')
            .ok_or_else(|| failure("Use --input AXIS=VALUE; omitted axes stay unknown"))?;
        if values.insert(axis.into(), value.parse()?).is_some() {
            return Err(failure(format!("Duplicate atlas input: {axis}")));
        }
    }
    Ok(values)
}

/// Use one SQLite read snapshot for the whole ensemble. An unavailable
/// participant never turns into a zero vector or borrows a neighbour's model.
pub fn inspect(db: &Connection, owners: &[String], inputs: &Values) -> Result<Atlas> {
    let contract = FeatureContract::standard();
    let appraisal = Appraisal {
        features: inputs
            .iter()
            .map(|(axis, value)| {
                (
                    axis.clone(),
                    FeatureValue {
                        value: Some(*value),
                        interpretation: "Operator comparison input".into(),
                        evidence: vec![],
                    },
                )
            })
            .collect(),
        reconsideration: None,
    };
    appraisal.validate(&contract).map_err(failure)?;
    let all = {
        let mut q = db.prepare("SELECT id FROM heads WHERE kind='persona' ORDER BY id")?;
        let ids = q
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };
    let selected = if owners.is_empty() { &all } else { owners };
    let mut seen = BTreeSet::new();
    let mut people = Vec::new();
    for owner in selected {
        if all.binary_search(owner).is_err() || !seen.insert(owner) {
            return Err(failure(format!(
                "Unknown or repeated atlas persona: {owner}"
            )));
        }
        let reading =
            history(db, owner, &contract, &appraisal).unwrap_or_else(|e| Reading::Unavailable {
                reason: e.to_string(),
            });
        people.push(Entry {
            owner: owner.clone(),
            reading,
        });
    }
    Ok(Atlas { inputs: inputs.clone(), contract: contract.digest().map_err(failure)?, people,
        scope: "Local forward arithmetic on one shared input, not an observed decision, quality score or evidence of beneficial maturation." })
}

fn history(
    db: &Connection,
    owner: &str,
    contract: &FeatureContract,
    appraisal: &Appraisal,
) -> Result<Reading> {
    let head = store::required(db, "persona", owner)?;
    let identity = head.reference.handle();
    let origin = store::exact(db, &format!("persona:{owner}@1"))?;
    let now: Persona = serde_json::from_value(head.body)?;
    let seed: Persona = serde_json::from_value(origin.body)?;
    if store::exact(db, &identity)?.reference != head.reference
        || now.id != owner
        || seed.id != owner
        || seed.epoch != 0
        || seed.model.is_some()
        || seed.seed != now.seed
        || seed.seed_author != now.seed_author
    {
        return Err(failure(
            "Identity or starting profile differs from its retained record",
        ));
    }
    let profile = Profile {
        label: now.label,
        identity,
        origin: origin.reference.handle(),
        seed: seed.seed,
        seed_author: seed.seed_author,
    };
    let mut statement =
        db.prepare("SELECT seq FROM events WHERE actor=?1 AND kind='model_changed' ORDER BY seq")?;
    let changes = statement.query_map([owner], |row| row.get::<_, i64>(0))?;
    let mut epochs = Vec::new();
    let mut previous = None;
    for change in changes {
        let reference = format!("experience:event-{}@1", change?);
        // The same verifier as the single-generation inspector: accepted
        // decision, authoring candidate, exact persona versions and switch.
        let switch = subject::resolve(db, Some(owner), &reference)?;
        let epoch = epochs.len() as u64 + 1;
        if switch.epoch != Some(epoch) || switch.alternate != previous {
            return Err(failure(
                "Model switches do not form one continuous adoption history",
            ));
        }
        let package = character_paths::package(db, &switch.supplied, owner)?;
        let field = |key: &str| {
            switch.detail[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| failure(format!("Model switch lacks {key}")))
        };
        epochs.push(Epoch {
            epoch,
            kind: field("kind")?,
            change: reference,
            decision: field("decision")?,
            reason: field("reason")?,
            package: format!("object:{}", switch.supplied),
            numerical: package.numerical_digest(contract).map_err(failure)?,
            inclinations: package
                .evaluate(contract, appraisal)
                .map_err(failure)?
                .inclinations(),
        });
        previous = Some(switch.supplied);
    }
    if now.model != previous || now.epoch != epochs.len() as u64 {
        return Err(failure(
            "Active identity differs from its recorded model switches",
        ));
    }
    Ok(if epochs.is_empty() {
        Reading::Pending { profile }
    } else {
        Reading::Available { profile, epochs }
    })
}
