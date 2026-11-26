//! What an operation's local ID can acknowledge in a decision. This classifies
//! a proposal, not the outcome of a dispatched tool or the quality of an asset.
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::neural) enum Channel {
    PersonaMaintenance,
    AssetMaintenance,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum OperationId {
    Required,
    Optional,
}

fn rule(tool: &str) -> Option<(Channel, OperationId)> {
    use Channel::{AssetMaintenance, PersonaMaintenance};
    use OperationId::{Optional, Required};
    Some(match tool {
        "persona.initialize" | "persona.patch" | "development.update" | "development.close" => {
            (PersonaMaintenance, Required)
        }
        "persona.evaluate" | "persona.assess" => (PersonaMaintenance, Optional),
        "relationship.record"
        | "relationship.retire"
        | "asset.save"
        | "asset.import"
        | "asset.organize"
        | "asset.register"
        | "geometry.install"
        | "command.install" => (AssetMaintenance, Required),
        "result.review"
        | "asset.feedback"
        | "image.read"
        | "image.render_svg"
        | "document.print_html"
        | "document.render_pdf"
        | "command.run"
        | "command.local"
        | "geometry.render"
        | "geometry.inspect_stl"
        | "geometry.profile_stl"
        | "work.outcome"
        | "result.deliver"
        | "human.issue_packet"
        | "workspace.bundle" => (AssetMaintenance, Optional),
        _ => return None,
    })
}

impl Channel {
    pub(in crate::neural) fn required(self, tool: &str) -> bool {
        rule(tool) == Some((self, OperationId::Required))
    }

    pub(in crate::neural) fn applicable(self, tool: &str) -> bool {
        rule(tool).is_some_and(|(channel, _)| channel == self)
    }
}

pub(super) fn descriptor(tool: &str) -> Option<Value> {
    rule(tool).map(|(field, operation_id)| json!({"field":field,"operation_id":operation_id}))
}
