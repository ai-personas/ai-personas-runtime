//! Closed proposal data. Parsing establishes shape, never creation authority.
use super::*;

pub const INTERFACE: &str = "need.propose.v1";
pub const MAX_PROPOSAL_BYTES: usize = 32_768;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantReference {
    #[schemars(length(min = 1, max = 96))]
    pub id: String,
    #[schemars(length(equal = 64), regex(pattern = "^[0-9a-f]{64}$"))]
    pub sha256: String,
}
impl GrantReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check_id(&self.id, "intake_delegation.id")?;
        if self.id.len() > 96 {
            return Err(invalid(
                "intake_delegation.id",
                "grant identity exceeds 96 bytes",
            ));
        }
        check_hash(&self.sha256, "intake_delegation.sha256")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    #[schemars(length(min = 1, max = 128))]
    pub id: String,
    #[schemars(length(min = 1, max = 4096))]
    pub description: String,
    #[schemars(length(min = 1, max = 4096))]
    pub criterion: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Invitee {
    #[schemars(length(min = 1, max = 128))]
    pub persona: String,
    #[schemars(length(min = 1, max = 4096))]
    pub terms: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub grant: GrantReference,
    #[schemars(length(min = 1, max = 4096))]
    pub purpose: String,
    #[schemars(length(min = 1, max = 16384))]
    pub request: String,
    #[schemars(length(min = 1, max = 8))]
    pub outcomes: Vec<Outcome>,
    #[schemars(length(min = 1, max = 8))]
    pub invitees: Vec<Invitee>,
    /// A requested child ceiling, bounded by the episode remainder after the
    /// authoring admission. This neither reserves nor allocates child resources.
    pub allocation: crate::runtime::WorkAllocation,
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
            if outcome.id.len() > 128 || !outcomes.insert(&outcome.id) {
                return Err(invalid(
                    "need.outcomes",
                    "bounded unique outcome identities required",
                ));
            }
            check_text(&outcome.description, 4096, "need.outcomes.description")?;
            check_text(&outcome.criterion, 4096, "need.outcomes.criterion")?;
        }
        let mut people = BTreeSet::new();
        for invitee in &self.invitees {
            check_id(&invitee.persona, "need.invitees.persona")?;
            if invitee.persona.len() > 128 || !people.insert(&invitee.persona) {
                return Err(invalid(
                    "need.invitees",
                    "bounded unique persona identities required",
                ));
            }
            check_text(&invitee.terms, 4096, "need.invitees.terms")?;
        }
        let allocation = self.allocation;
        let navigation = allocation.navigation;
        if !(1..=128).contains(&allocation.attempts)
            || allocation.repair_attempts > allocation.attempts
            || navigation.calls > 128
            || navigation.scanned_bytes > 1024 * 1024 * 1024
            || navigation.inspected_revisions > 262_144
            || navigation.matches > 4096
            || navigation.result_bytes > 512 * 1024
        {
            return Err(invalid(
                "need.allocation",
                "unsupported bounded child allocation",
            ));
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|_| invalid("need", "proposal serialization failed"))?;
        if bytes.len() > MAX_PROPOSAL_BYTES {
            return Err(invalid(
                "need",
                "complete proposal exceeds 32768 UTF-8 JSON bytes",
            ));
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
