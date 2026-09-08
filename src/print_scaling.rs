//! Physical scaling of one governed `_print` document across paper sizes.
//! No map, transport, or paper-specific style copies.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintScaling {
    pub reference_paper: String,
    /// CSS screen dimensions are converted at 96 dpi before paper scaling.
    pub units: PrintUnits,
    pub factors: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PrintUnits {
    CssPx,
    Physical,
}

impl Default for PrintScaling {
    fn default() -> Self {
        Self {
            reference_paper: "A3".into(),
            units: PrintUnits::CssPx,
            factors: [
                ("A0", 8_f64.sqrt()),
                ("A1", 2.),
                ("A2", 2_f64.sqrt()),
                ("A3", 1.),
                ("A4", 0.5_f64.sqrt()),
                ("A5", 0.5),
                ("Custom", 1.),
            ]
            .into_iter()
            .map(|(paper, scale)| (paper.into(), scale))
            .collect(),
        }
    }
}

impl PrintScaling {
    pub fn validate(&self) -> Result<(), String> {
        let papers = ["A0", "A1", "A2", "A3", "A4", "A5", "Custom"];
        if !papers.contains(&self.reference_paper.as_str())
            || self.factors.len() != papers.len()
            || papers.iter().any(|paper| {
                !self
                    .factors
                    .get(*paper)
                    .is_some_and(|n| n.is_finite() && (0.05..=20.).contains(n))
            })
            || self.factors.get(&self.reference_paper) != Some(&1.)
        {
            return Err("Print scaling requires A0–A5 and Custom factors in 0.05–20, with reference paper factor 1.".into());
        }
        Ok(())
    }

    pub fn factor(&self, paper: &str) -> Result<f64, String> {
        self.validate()?;
        let key = match paper {
            "print_a0" => "A0",
            "print_a1" => "A1",
            "print_a2" => "A2",
            "print_a3" => "A3",
            "print_a4" => "A4",
            "print_a5" => "A5",
            "print_custom" => "Custom",
            other => other,
        };
        self.factors
            .get(key)
            .copied()
            .ok_or_else(|| "Unknown print paper.".into())
    }
}

pub fn scaling_from_style(document: &Value) -> Result<Option<PrintScaling>, String> {
    let Some(value) = document
        .get("metadata")
        .and_then(|m| m.get("print_scaling"))
    else {
        return Ok(None);
    };
    let scaling: PrintScaling =
        serde_json::from_value(value.clone()).map_err(|e| format!("Invalid print scaling: {e}"))?;
    scaling.validate()?;
    Ok(Some(scaling))
}

pub fn apply_print_scaling(mut document: Value, scaling: PrintScaling) -> Result<Value, String> {
    scaling.validate()?;
    let document_object = document.as_object_mut().ok_or("Style must be an object.")?;
    let metadata = document_object
        .entry("metadata")
        .or_insert_with(|| serde_json::json!({}));
    metadata
        .as_object_mut()
        .ok_or("Style metadata must be an object.")?
        .insert(
            "print_scaling".into(),
            serde_json::to_value(scaling).map_err(|e| e.to_string())?,
        );
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn one_document_scales_by_iso_linear_ratio_without_changing_symbols() {
        let original = json!({"type":"symbol","layout":{"icon-image":"school","icon-size":1.2},"metadata":{"source":"catalog"}});
        let style = apply_print_scaling(original.clone(), PrintScaling::default()).unwrap();
        let scaling = scaling_from_style(&style).unwrap().unwrap();
        assert_eq!(style["layout"], original["layout"]);
        assert_eq!(style["metadata"]["source"], "catalog");
        assert_eq!(scaling.factor("print_a3").unwrap(), 1.);
        assert_eq!(scaling.factor("print_a0").unwrap(), 8_f64.sqrt());
        assert_eq!(scaling.factor("print_a5").unwrap(), 0.5);
        assert!(scaling_from_style(&original).unwrap().is_none());
    }

    #[test]
    fn invalid_or_incomplete_factors_never_silently_become_one() {
        let mut scaling = PrintScaling::default();
        scaling.factors.insert("A3".into(), 2.);
        assert!(scaling.validate().is_err());
        scaling = PrintScaling::default();
        scaling.factors.remove("A0");
        assert!(scaling.validate().is_err());
        scaling = PrintScaling::default();
        scaling.factors.insert("A0".into(), f64::NAN);
        assert!(scaling.validate().is_err());
        assert!(PrintScaling::default().factor("unknown").is_err());
    }
}
