use super::{Appraisal, FeatureContract, ModelPackage, Output, Parameter, Result, Tensor};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct InputTerm<'a> {
    column: usize,
    axis: &'a str,
    channel: &'static str,
    known: bool,
    input: f64,
    weight: f64,
    product: f64,
}

#[derive(Serialize)]
pub struct HiddenPath<'a> {
    unit: usize,
    bias: f64,
    inputs: Vec<InputTerm<'a>>,
    preactivation: f64,
    activation: f64,
    output_weight: f64,
    contribution: f64,
}

#[derive(Serialize)]
pub struct OutputPaths<'a> {
    pub outputs: BTreeMap<String, f64>,
    pub hidden_units: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<OutputPath<'a>>,
}

#[derive(Serialize)]
pub struct OutputPath<'a> {
    row: usize,
    total: Output,
    preactivation: f64,
    tanh_at_limit: bool,
    direct_terms: Vec<InputTerm<'a>>,
    hidden_paths: Vec<HiddenPath<'a>>,
}

impl ModelPackage {
    /// Expand one output of the same forward pass used to supply decisions.
    /// Sparse coordinates omitted from the result have weight zero. An explicit
    /// hidden unit also shows an existing unit whose outgoing weight is zero.
    pub fn output_paths<'a>(
        &self,
        contract: &'a FeatureContract,
        appraisal: &Appraisal,
        axis: Option<&str>,
        hidden_unit: Option<usize>,
    ) -> Result<OutputPaths<'a>> {
        if hidden_unit.is_some() && axis.is_none() {
            return Err("Choose an output axis to inspect a hidden unit".into());
        }
        let pass = self.forward(contract, appraisal)?;
        let outputs = pass
            .outputs
            .iter()
            .map(|(id, o)| (id.clone(), o.value))
            .collect();
        let mut result = OutputPaths {
            outputs,
            hidden_units: self.hidden_units,
            selected: None,
        };
        let Some(axis) = axis else { return Ok(result) };
        let row = contract
            .outputs
            .iter()
            .position(|a| a.id == axis)
            .ok_or_else(|| format!("Unknown output axis {axis}"))?;
        let total = pass.outputs[axis].clone();
        let term = |p: &Parameter| {
            let index = p.column / 2;
            let input = pass.input[p.column];
            InputTerm {
                column: p.column,
                axis: &contract.inputs[index].id,
                channel: if p.column % 2 == 0 { "value" } else { "known" },
                known: pass.input[index * 2 + 1] == 1.0,
                input,
                weight: p.value,
                product: p.value * input,
            }
        };
        let mut direct_terms = Vec::new();
        let mut hidden_paths = BTreeMap::new();
        let mut add_unit = |unit: usize, weight: f64| {
            let preactivation = pass.hidden_sums[unit];
            let activation = preactivation.max(0.0);
            hidden_paths.insert(
                unit,
                HiddenPath {
                    unit,
                    bias: 0.0,
                    inputs: Vec::new(),
                    preactivation,
                    activation,
                    output_weight: weight,
                    contribution: weight * activation,
                },
            );
        };
        if let Some(unit) = hidden_unit.filter(|&i| i < self.hidden_units) {
            add_unit(unit, 0.0);
        }
        for p in self
            .parameters
            .iter()
            .filter(|p| p.value != 0.0 && p.row == row)
        {
            match p.tensor {
                Tensor::Direct => direct_terms.push(term(p)),
                Tensor::Output if hidden_unit.is_none_or(|u| u == p.column) => {
                    add_unit(p.column, p.value)
                }
                _ => {}
            }
        }
        for p in self.parameters.iter().filter(|p| p.value != 0.0) {
            if let Some(path) = hidden_paths.get_mut(&p.row) {
                match p.tensor {
                    Tensor::Hidden => path.inputs.push(term(p)),
                    Tensor::HiddenBias => path.bias = p.value,
                    _ => {}
                }
            }
        }
        direct_terms.sort_by_key(|t| t.column);
        for path in hidden_paths.values_mut() {
            path.inputs.sort_by_key(|t| t.column);
        }
        result.selected = Some(OutputPath {
            row,
            preactivation: total.bias + total.direct + total.interaction,
            tanh_at_limit: total.value.abs() == 1.0,
            total,
            direct_terms,
            hidden_paths: hidden_paths.into_values().collect(),
        });
        Ok(result)
    }
}
