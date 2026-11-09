//! Directly authored character parameters. Forward arithmetic only: no fitting,
//! gradients, optimizer, automatic parameter search, or implicit weight update.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    pub id: String,
    /// Shared interface meaning, not a persona-specific instruction.
    pub meaning: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureContract {
    pub version: u32,
    pub inputs: Vec<Axis>,
    pub outputs: Vec<Axis>,
}

impl FeatureContract {
    pub fn standard() -> Self {
        let inputs = [
            (
                "uncertainty",
                "Uncertainty about facts relevant to the current choice",
            ),
            (
                "ambiguity",
                "Unspecified intent or requirements relevant to this choice",
            ),
            ("novelty", "Unfamiliarity of this situation"),
            (
                "evidence_conflict",
                "Conflict among relevant observations or claims",
            ),
            (
                "consequence",
                "Magnitude of consequences of an incorrect choice",
            ),
            ("reversibility", "Ease of reversing the contemplated action"),
            ("urgency", "Evidence of actual time pressure"),
            (
                "peer_relevance",
                "Evidence that an available peer could help",
            ),
            (
                "obligation_gap",
                "Extent of outstanding accepted responsibility",
            ),
            (
                "feasible_work",
                "Useful remaining work possible with current resources",
            ),
            ("progress", "Observed advance toward the intended result"),
            (
                "repetition",
                "Repeated activity without new relevant evidence or improvement",
            ),
            (
                "integration_gap",
                "Extent to which useful parts have yet to form the intended result",
            ),
            (
                "social_opportunity",
                "Opportunity for a meaningful social exchange",
            ),
            (
                "criticism",
                "Relevant criticism or contrary perspective received",
            ),
            (
                "valence",
                "Current modeled affect: unpleasant (-1) to pleasant (+1)",
            ),
            (
                "arousal",
                "Current modeled affect: low (-1) to high (+1) activation",
            ),
            (
                "dominance",
                "Current modeled affect: low (-1) to high (+1) sense of control",
            ),
        ];
        let outputs = [
            (
                "exploration",
                "Inclination to explore alternatives and unfamiliar approaches",
            ),
            (
                "verification",
                "Inclination to seek discriminating checks and evidence",
            ),
            (
                "clarification",
                "Inclination to clarify relevant uncertainty with others",
            ),
            (
                "collaboration",
                "Inclination to seek or offer a useful contribution",
            ),
            ("challenge", "Inclination to question a claim or agreement"),
            (
                "revision",
                "Willingness to revise an interpretation or approach",
            ),
            (
                "integration",
                "Attention to how contributions form a useful whole",
            ),
            (
                "persistence",
                "Inclination to pursue further purposeful effort",
            ),
            (
                "initiative",
                "Inclination to originate useful work or interaction",
            ),
            (
                "warmth",
                "Warmth and interpersonal consideration in communication",
            ),
            (
                "assertiveness",
                "Directness and willingness to state a position",
            ),
            ("detail", "Preference for elaboration rather than brevity"),
        ];
        let axes = |items: &[(&str, &str)]| {
            items
                .iter()
                .map(|(id, meaning)| Axis {
                    id: (*id).into(),
                    meaning: (*meaning).into(),
                })
                .collect()
        };
        Self {
            version: 1,
            inputs: axes(&inputs),
            outputs: axes(&outputs),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.inputs.is_empty() || self.outputs.is_empty() {
            return Err("feature contract needs version 1 and nonempty inputs and outputs".into());
        }
        for axes in [&self.inputs, &self.outputs] {
            let mut ids = BTreeSet::new();
            for axis in axes {
                if axis.id.is_empty()
                    || axis.id.chars().any(char::is_whitespace)
                    || axis.meaning.trim().is_empty()
                    || !ids.insert(&axis.id)
                {
                    return Err(format!("invalid or duplicate feature axis {}", axis.id));
                }
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        digest(self)
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Tensor {
    Bias,
    Direct,
    Hidden,
    HiddenBias,
    Output,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub tensor: Tensor,
    pub row: usize,
    /// Bias tensors use column zero. Input columns are [value, known] per input axis.
    pub column: usize,
    pub value: f64,
}

/// Sparse representation: every unspecified coordinate is exactly zero.
/// No random values or learned defaults are supplied by the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelPackage {
    pub version: u32,
    pub persona: String,
    pub contract: String,
    pub predecessor: Option<String>,
    pub hidden_units: usize,
    pub parameters: Vec<Parameter>,
    pub intent: String,
    pub evidence: Vec<String>,
}

impl ModelPackage {
    pub fn validate(&self, contract: &FeatureContract) -> Result<()> {
        contract.validate()?;
        if self.version != 1
            || self.persona.trim().is_empty()
            || self.intent.trim().is_empty()
            || self.contract != contract.digest()?
        {
            return Err("invalid model version, owner, intent or feature-contract binding".into());
        }
        let mut seen = BTreeSet::new();
        for p in &self.parameters {
            self.check_coordinate(contract, p.tensor, p.row, p.column)?;
            if !p.value.is_finite() || !seen.insert((p.tensor, p.row, p.column)) {
                return Err("nonfinite or duplicate model parameter".into());
            }
        }
        Ok(())
    }

    fn check_coordinate(
        &self,
        contract: &FeatureContract,
        tensor: Tensor,
        row: usize,
        column: usize,
    ) -> Result<()> {
        let inputs = contract
            .inputs
            .len()
            .checked_mul(2)
            .ok_or("input dimension overflow")?;
        let shape = match tensor {
            Tensor::Bias => (contract.outputs.len(), 1),
            Tensor::Direct => (contract.outputs.len(), inputs),
            Tensor::Hidden => (self.hidden_units, inputs),
            Tensor::HiddenBias => (self.hidden_units, 1),
            Tensor::Output => (contract.outputs.len(), self.hidden_units),
        };
        if row >= shape.0 || column >= shape.1 {
            return Err(format!(
                "parameter {tensor:?}[{row},{column}] outside {shape:?}"
            ));
        }
        Ok(())
    }

    pub fn canonicalize(&mut self) {
        for p in &mut self.parameters {
            if p.value == 0.0 {
                p.value = 0.0;
            }
        }
        self.parameters.retain(|p| p.value != 0.0);
        self.parameters.sort_by_key(|p| (p.tensor, p.row, p.column));
    }

    pub fn digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.canonicalize();
        digest(&canonical)
    }

    pub fn evaluate(
        &self,
        contract: &FeatureContract,
        appraisal: &Appraisal,
    ) -> Result<Evaluation> {
        self.validate(contract)?;
        appraisal.validate(contract)?;
        let mut input = Vec::with_capacity(contract.inputs.len() * 2);
        for axis in &contract.inputs {
            match appraisal.features.get(&axis.id).and_then(|f| f.value) {
                Some(value) => input.extend([value, 1.0]),
                None => input.extend([0.0, 0.0]),
            }
        }
        let mut hidden = Vec::new();
        hidden
            .try_reserve_exact(self.hidden_units)
            .map_err(|e| format!("host allocation failure: {e}"))?;
        hidden.resize(self.hidden_units, 0.0);
        let mut direct = vec![0.0; contract.outputs.len()];
        let mut bias = vec![0.0; contract.outputs.len()];
        for p in &self.parameters {
            match p.tensor {
                Tensor::Bias => bias[p.row] = p.value,
                Tensor::Direct => direct[p.row] += p.value * input[p.column],
                Tensor::Hidden => hidden[p.row] += p.value * input[p.column],
                Tensor::HiddenBias => hidden[p.row] += p.value,
                Tensor::Output => {}
            }
        }
        if hidden.iter().chain(&direct).any(|v| !v.is_finite()) {
            return Err(
                "forward arithmetic overflow; no output or parameter update adopted".into(),
            );
        }
        for v in &mut hidden {
            *v = v.max(0.0);
        }
        let mut interaction = vec![0.0; contract.outputs.len()];
        for p in &self.parameters {
            if p.tensor == Tensor::Output {
                interaction[p.row] += p.value * hidden[p.column];
            }
        }
        let mut outputs = BTreeMap::new();
        for (i, axis) in contract.outputs.iter().enumerate() {
            let preactivation = bias[i] + direct[i] + interaction[i];
            if !preactivation.is_finite() {
                return Err("forward output overflow".into());
            }
            outputs.insert(
                axis.id.clone(),
                Output {
                    value: preactivation.tanh(),
                    bias: bias[i],
                    direct: direct[i],
                    interaction: interaction[i],
                },
            );
        }
        Ok(Evaluation {
            model: self.digest()?,
            contract: self.contract.clone(),
            appraisal: appraisal.clone(),
            hidden,
            outputs,
        })
    }

    pub fn patch(&self, contract: &FeatureContract, patch: &ParameterPatch) -> Result<Self> {
        self.validate(contract)?;
        if patch.expected_model != self.digest()?
            || patch.reason.trim().is_empty()
            || patch.edits.is_empty()
        {
            return Err("patch requires the exact predecessor, reason and literal edits".into());
        }
        let mut candidate = self.clone();
        let mut parameters: BTreeMap<_, _> = self
            .parameters
            .iter()
            .map(|p| ((p.tensor, p.row, p.column), p.value))
            .collect();
        let mut seen = BTreeSet::new();
        for edit in &patch.edits {
            self.check_coordinate(contract, edit.tensor, edit.row, edit.column)?;
            let key = (edit.tensor, edit.row, edit.column);
            if !seen.insert(key)
                || !edit.value.is_finite()
                || !edit.expected.is_finite()
                || parameters.get(&key).copied().unwrap_or(0.0) != edit.expected
            {
                return Err(format!("conflicting or invalid literal edit at {key:?}"));
            }
            parameters.insert(key, edit.value);
        }
        candidate.parameters = parameters
            .into_iter()
            .map(|((tensor, row, column), value)| Parameter {
                tensor,
                row,
                column,
                value,
            })
            .collect();
        candidate.predecessor = Some(self.digest()?);
        candidate.intent = patch.reason.clone();
        candidate.evidence = patch.evidence.clone();
        candidate.canonicalize();
        candidate.validate(contract)?;
        Ok(candidate)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureValue {
    /// Signed normalized value in [-1,1]; null means explicitly unknown.
    pub value: Option<f64>,
    pub evidence: Vec<String>,
    pub interpretation: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Appraisal {
    /// Omitted axes remain explicitly unknown in the evaluation input mask.
    pub features: BTreeMap<String, FeatureValue>,
}
impl Appraisal {
    pub fn validate(&self, contract: &FeatureContract) -> Result<()> {
        for (id, feature) in &self.features {
            if !contract.inputs.iter().any(|axis| axis.id == *id)
                || feature
                    .value
                    .is_some_and(|v| !v.is_finite() || !(-1.0..=1.0).contains(&v))
                || feature.interpretation.trim().is_empty()
            {
                return Err(format!("invalid appraisal axis/value/interpretation: {id}"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub tensor: Tensor,
    pub row: usize,
    pub column: usize,
    pub expected: f64,
    pub value: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParameterPatch {
    pub expected_model: String,
    pub edits: Vec<Edit>,
    pub reason: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub value: f64,
    pub bias: f64,
    pub direct: f64,
    pub interaction: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub model: String,
    pub contract: String,
    pub appraisal: Appraisal,
    pub hidden: Vec<f64>,
    pub outputs: BTreeMap<String, Output>,
}

pub fn digest<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_vec(value)
        .map(|bytes| crate::store::sha256_bytes(&bytes))
        .map_err(|e| e.to_string())
}
