//! Table heading identity is independent of renderer font measurement.
//! Renderers supply measured widths; the kernel chooses stable ordered aliases.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeadingMode {
    #[default]
    Literal,
    AutoIndex,
    Indexed,
}
impl HeadingMode {
    pub fn is_literal(&self) -> bool {
        *self == Self::Literal
    }
}
#[derive(Debug, PartialEq)]
pub struct HeadingPlan {
    pub headings: Vec<String>,
    pub key: Vec<(String, String)>,
}
/// All columns receive stable A..L aliases if indexing is needed. Mixing literal
/// headings named "A" with indexed "A" would make the explanatory key ambiguous.
pub fn plan(
    mode: HeadingMode,
    headings: &[String],
    heading_widths: &[f64],
    data_widths: &[f64],
) -> Result<HeadingPlan, String> {
    let count = headings.len();
    if count == 0
        || count > 12
        || heading_widths.len() != count
        || data_widths.len() != count
        || heading_widths
            .iter()
            .chain(data_widths)
            .any(|w| !w.is_finite() || *w < 0.)
    {
        return Err("Invalid table heading measurements.".into());
    }
    let indexed = match mode {
        HeadingMode::Literal => false,
        HeadingMode::Indexed => true,
        HeadingMode::AutoIndex => heading_widths
            .iter()
            .zip(data_widths)
            .any(|(h, d)| *h > *d + 1e-7),
    };
    if !indexed {
        return Ok(HeadingPlan {
            headings: headings.to_vec(),
            key: vec![],
        });
    }
    if headings.iter().any(|h| h.trim().is_empty()) {
        return Err("Indexed table headings require a full label for every column.".into());
    }
    let letters: Vec<_> = (0..count)
        .map(|i| ((b'A' + i as u8) as char).to_string())
        .collect();
    Ok(HeadingPlan {
        key: letters
            .iter()
            .cloned()
            .zip(headings.iter().cloned())
            .collect(),
        headings: letters,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_preserve_order_full_meanings_and_avoid_literal_letter_collisions() {
        let labels = vec![
            "House Number".into(),
            "A".into(),
            "Number of Houses".into(),
            "Earthing".into(),
        ];
        let indexed = plan(
            HeadingMode::AutoIndex,
            &labels,
            &[20., 1., 30., 15.],
            &[5., 1., 4., 3.],
        )
        .unwrap();
        assert_eq!(indexed.headings, ["A", "B", "C", "D"]);
        assert_eq!(indexed.key[1], ("B".into(), "A".into()));
        assert_eq!(indexed.key[3], ("D".into(), "Earthing".into()));
        assert_eq!(
            plan(HeadingMode::Literal, &labels, &[20.; 4], &[2.; 4])
                .unwrap()
                .headings,
            labels
        );
        assert!(plan(HeadingMode::AutoIndex, &labels, &[2.; 4], &[20.; 4])
            .unwrap()
            .key
            .is_empty());
        assert!(!plan(HeadingMode::Indexed, &labels, &[2.; 4], &[20.; 4])
            .unwrap()
            .key
            .is_empty());
    }
    #[test]
    fn invalid_geometry_is_refused() {
        for widths in [vec![], vec![f64::NAN], vec![-1.]] {
            assert!(plan(HeadingMode::Indexed, &["x".into()], &widths, &[1.]).is_err());
        }
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use crate::printing::{overrides::TableOverride, TableBinding};
    use serde_json::json;
    #[test]
    fn old_documents_keep_their_serialization_and_new_modes_are_closed() {
        let old =
            json!({"layer":"poles","columns":["number"],"headings":["Pole Number"],"widths":[1]});
        let mut table: TableBinding = serde_json::from_value(old.clone()).unwrap();
        assert!(serde_json::to_value(&table)
            .unwrap()
            .get("heading_mode")
            .is_none());
        table.heading_mode = HeadingMode::AutoIndex;
        let saved = serde_json::to_value(&table).unwrap();
        assert_eq!(saved["heading_mode"], "auto_index");
        assert_eq!(
            serde_json::from_value::<TableBinding>(saved).unwrap(),
            table
        );
        let mut bad = old;
        bad["heading_mode"] = json!("truncate");
        assert!(serde_json::from_value::<TableBinding>(bad).is_err());
        let patch: TableOverride =
            serde_json::from_value(json!({"heading_mode":"indexed"})).unwrap();
        assert_eq!(patch.heading_mode, Some(HeadingMode::Indexed));
        assert!(serde_json::to_value(TableOverride::default())
            .unwrap()
            .get("heading_mode")
            .is_none());
    }
}
