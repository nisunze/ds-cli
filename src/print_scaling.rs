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

/// Every ISO paper keeps symbology at its authored physical size: one CSS
/// pixel is 1/96 inch on A5 and on A0 alike. Line widths, dashes, symbols and
/// text therefore measure the same millimetres on every sheet, and a larger
/// sheet shows the same extent at a larger map scale with *finer* relative
/// detail, which is what a large-format map is for.
///
/// The earlier table scaled by the ISO linear ratio (A0 = √8 ≈ 2.83): an A0
/// sheet then read as a photocopy-enlarged A3. Owner ruling 2026-09-08: A3 is
/// near right, A0 far too big; the morning A0 prints with unscaled symbology
/// were right. Authors may still record deliberate factors in metadata.
pub const PAPERS: [&str; 7] = ["A0", "A1", "A2", "A3", "A4", "A5", "Custom"];

impl Default for PrintScaling {
    fn default() -> Self {
        Self {
            reference_paper: "A3".into(),
            units: PrintUnits::CssPx,
            factors: PAPERS.iter().map(|paper| ((*paper).into(), 1.)).collect(),
        }
    }
}

/// The superseded ISO-area table that every `_print` style created before
/// 2026-09-08 carries. Reading it yields the identity policy above so those
/// styles print at the ruled size without a hand-edited document.
fn legacy_iso_area_table() -> PrintScaling {
    PrintScaling {
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

impl PrintScaling {
    fn is_legacy_iso_area_table(&self) -> bool {
        let legacy = legacy_iso_area_table();
        self.reference_paper == legacy.reference_paper
            && self.units == legacy.units
            && self.factors.len() == legacy.factors.len()
            && legacy
                .factors
                .iter()
                .all(|(paper, factor)| self.factors.get(paper).is_some_and(|f| (f - factor).abs() < 1e-9))
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
    if scaling.is_legacy_iso_area_table() {
        return Ok(Some(PrintScaling::default()));
    }
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
        assert_eq!(scaling.factor("print_a0").unwrap(), 1.);
        assert_eq!(scaling.factor("print_a5").unwrap(), 1.);
        assert!(scaling_from_style(&original).unwrap().is_none());
    }

    #[test]
    fn the_legacy_iso_area_table_reads_as_the_identity_policy() {
        let legacy = json!({"type":"line","metadata":{"print_scaling":{"reference_paper":"A3","units":"css_px",
            "factors":{"A0":2.8284271247461903,"A1":2,"A2":1.4142135623730951,"A3":1,"A4":0.7071067811865476,"A5":0.5,"Custom":1}}}});
        let scaling = scaling_from_style(&legacy).unwrap().unwrap();
        assert_eq!(scaling, PrintScaling::default());
        assert_eq!(scaling.factor("A0").unwrap(), 1.);
        // A deliberately authored table is honoured as written.
        let authored = json!({"type":"line","metadata":{"print_scaling":{"reference_paper":"A3","units":"css_px",
            "factors":{"A0":1.5,"A1":1.25,"A2":1.1,"A3":1,"A4":1,"A5":1,"Custom":1}}}});
        assert_eq!(scaling_from_style(&authored).unwrap().unwrap().factor("A0").unwrap(), 1.5);
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
