//! The network-fit and scale-limit policy documents of a print layout and
//! their admission. Fitting a network onto the sheet is printing computation
//! and lives in the Reporter.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type Rect = [f64; 4];
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NetworkFit {
    pub source_layers: Vec<String>,
    pub clearance_mm: f64,
}
impl NetworkFit {
    pub fn validate(&self) -> Result<(), String> {
        if self.source_layers.is_empty()
            || self.source_layers.len() > 16
            || self.source_layers.iter().any(|id| !super::identifier(id))
            || self.source_layers.iter().collect::<BTreeSet<_>>().len() != self.source_layers.len()
            || !super::bounded(self.clearance_mm, 1., 30.)
        {
            return Err(
                "network fit needs 1..16 unique engineering source layers and 1..30 mm clearance"
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScaleLimits {
    pub min_denominator: f64,
    pub max_denominator: f64,
}
impl ScaleLimits {
    pub fn validate(&self) -> Result<(), String> {
        if !super::bounded(self.min_denominator, 10., 100_000_000.)
            || !super::bounded(self.max_denominator, self.min_denominator, 100_000_000.)
        {
            return Err("print scale limits need ordered denominators within 10..100000000".into());
        }
        Ok(())
    }
}
