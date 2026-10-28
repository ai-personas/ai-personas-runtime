//! Closed proposal data. Parsing establishes shape, never creation authority.
use super::*;

pub const INTERFACE: &str = "need.propose.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantReference {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(equal = 64), regex(pattern = "^[0-9a-f]{64}$"))]
    pub sha256: String,
}
impl GrantReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check_id(&self.id, "intake_delegation.id")?;
        check_hash(&self.sha256, "intake_delegation.sha256")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub description: String,
    #[schemars(length(min = 1))]
    pub criterion: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Invitee {
    #[schemars(length(min = 1))]
    pub persona: String,
    #[schemars(length(min = 1))]
    pub terms: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub grant: GrantReference,
    #[schemars(length(min = 1))]
    pub purpose: String,
    #[schemars(length(min = 1))]
    pub request: String,
    #[schemars(length(min = 1))]
    pub outcomes: Vec<Outcome>,
    #[schemars(length(min = 1))]
    pub invitees: Vec<Invitee>,
}

pub(super) fn check_hash(value: &str, path: &str) -> Result<(), ProtocolError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid(
            path,
            "expected 64 lowercase hexadecimal SHA-256 digits",
        ));
    }
    Ok(())
}

impl Proposal {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.grant.validate()?;
        check_text(&self.purpose, 4096, "need.purpose")?;
        check_text(&self.request, 16_384, "need.request")?;
        check_count(self.outcomes.len(), 1, 8, "need.outcomes")?;
        check_count(self.invitees.len(), 1, 8, "need.invitees")?;
        let mut outcomes = BTreeSet::new();
        for outcome in &self.outcomes {
            check_id(&outcome.id, "need.outcomes.id")?;
            if !outcomes.insert(&outcome.id) {
                return Err(invalid(
                    "need.outcomes",
                    "unique outcome identities required",
                ));
            }
            check_text(&outcome.description, 4096, "need.outcomes.description")?;
            check_text(&outcome.criterion, 4096, "need.outcomes.criterion")?;
        }
        let mut people = BTreeSet::new();
        for invitee in &self.invitees {
            check_id(&invitee.persona, "need.invitees.persona")?;
            if !people.insert(&invitee.persona) {
                return Err(invalid(
                    "need.invitees",
                    "unique persona identities required",
                ));
            }
            check_text(&invitee.terms, 4096, "need.invitees.terms")?;
        }
        Ok(())
    }
}

pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Proposal))
        .expect("serializable need proposal schema")
}

pub fn binding_revision() -> String {
    let bytes =
        serde_json::to_vec(&(INTERFACE, schema())).expect("serializable need proposal interface");
    hex::encode(Sha256::digest(bytes))
}
