//! Distinguish the action we can trace now from the action card, if any,
//! supplied alongside a human report in the first verified receiving decision.
use super::{context::Snapshot, failure, Connection, Event, Result};
use crate::neural::{character_paths, experience};
use serde_json::{json, Value};

pub(super) struct Asking {
    episode: Value,
    in_received: Option<&'static str>,
}

impl Asking {
    pub(super) fn trace(db: &Connection, report: &Event) -> Result<Self> {
        let episode = experience::action::origin(db, &report.target, &report.work, &report.reference())?
            .filter(|e| e["relation"] == "outside_report")
            .ok_or_else(|| failure("Cannot verify the decision that asked for this report"))?;
        Ok(Self {
            episode,
            in_received: None,
        })
    }

    pub(super) fn received(&mut self, input: &Snapshot) {
        let mut matches = input.data["development"]["action_results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["outcome"] == self.episode["outcome"]);
        self.in_received = Some(match matches.next() {
            None => "not_supplied",
            Some(v) if v == &self.episode && matches.next().is_none() => "supplied",
            Some(_) => "differs",
        });
    }

    fn numbers(&self, db: &Connection) -> Result<Value> {
        let e = &self.episode;
        let owner = e["owner"].as_str().ok_or_else(|| failure("Missing requester"))?;
        let decision = e["decision"]
            .as_str()
            .and_then(|r| r.strip_prefix("experience:"))
            .ok_or_else(|| failure("Missing requesting decision"))?;
        let query = character_paths::Query {
            decision: format!("character_use:{decision}"),
            axis: None,
            hidden_unit: None,
            compare: None,
        };
        let paths = character_paths::inspect(db, Some(owner), &query)?;
        let model = paths["supplied"]["package"]
            .as_str()
            .and_then(|v| v.strip_prefix("object:"));
        let then = &e["character_then"];
        if paths["decision"] != e["decision"]
            || paths["choice"]["response"] != e["source_response"]
            || paths["basis"] != then["basis"]
            || paths["epoch"] != then["epoch"]
            || model != then["model"].as_str()
        {
            return Err(failure("The numerical input differs from the requesting choice"));
        }
        Ok(
            json!({"character_use":paths["character_use"],"snapshot":paths["snapshot"],"package":paths["supplied"]["package"],
            "epoch":paths["epoch"],"basis":paths["basis"],"outputs":paths["supplied"]["paths"]["outputs"]}),
        )
    }

    pub(super) fn view(self, db: &Connection) -> Value {
        let numbers = self.numbers(db);
        let mut v = json!({"episode":self.episode,"card_in_received_input":self.in_received});
        match numbers {
            Ok(n) => v["character_at_request"] = n,
            Err(e) => v["character_error"] = json!(e.to_string()),
        }
        v
    }
}
